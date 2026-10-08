use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use system_core::{CommandMetadata, ModuleSchema, SystemModule};

use super::descriptors::{ModuleActivation, ModuleClass, ModuleDescriptor, ModuleRequirement};
use super::factory::ConstructionReceipt;
use super::provider_config::ProviderDeploymentConfig;

pub fn validate_command_metadata(
    module_name: &str,
    schema: &ModuleSchema,
    commands: &[CommandMetadata],
) -> Result<(), String> {
    let schema_names: HashSet<&str> = schema
        .commands
        .iter()
        .map(|command| command.name.as_str())
        .collect();
    let mut metadata_names = HashSet::new();

    for metadata in commands {
        if !metadata_names.insert(metadata.name) {
            return Err(format!(
                "SYS_COMMAND_METADATA_DUPLICATE:{module_name}:{}",
                metadata.name
            ));
        }
        if !schema_names.contains(metadata.name) {
            return Err(format!(
                "SYS_COMMAND_METADATA_UNKNOWN:{module_name}:{}",
                metadata.name
            ));
        }
    }

    for schema_name in schema_names {
        if !metadata_names.contains(schema_name) {
            return Err(format!(
                "SYS_COMMAND_METADATA_MISSING:{module_name}:{schema_name}"
            ));
        }
    }

    Ok(())
}

pub fn validate_descriptor_catalog(descriptors: &[ModuleDescriptor]) -> Result<(), String> {
    let mut registry_keys = HashSet::new();
    let mut factories = HashSet::new();

    for descriptor in descriptors {
        if !registry_keys.insert(descriptor.registry_key) {
            return Err(format!(
                "SYS_MODULE_DESCRIPTOR_DUPLICATE_REGISTRY_KEY:{}",
                descriptor.registry_key
            ));
        }
        if !factories.insert(descriptor.factory) {
            return Err(format!(
                "SYS_MODULE_DESCRIPTOR_DUPLICATE_FACTORY:{:?}",
                descriptor.factory
            ));
        }

        let provider_requirements: Vec<_> = descriptor
            .requirements
            .iter()
            .filter_map(|requirement| match requirement {
                ModuleRequirement::ProviderConfig(provider) => Some(*provider),
                _ => None,
            })
            .collect();
        match (descriptor.class, descriptor.activation) {
            (ModuleClass::Provider, ModuleActivation::ProviderConfigured(provider))
                if provider_requirements.as_slice() == [provider] => {}
            (ModuleClass::Provider, _) => {
                return Err(format!(
                    "SYS_MODULE_DESCRIPTOR_INVALID_PROVIDER:{}",
                    descriptor.registry_key
                ));
            }
            (_, ModuleActivation::Always) if provider_requirements.is_empty() => {}
            _ => {
                return Err(format!(
                    "SYS_MODULE_DESCRIPTOR_INVALID_PROVIDER_PAIRING:{}",
                    descriptor.registry_key
                ));
            }
        }
    }

    for descriptor in descriptors {
        for requirement in descriptor.requirements {
            if let ModuleRequirement::Module(dependency) = requirement {
                if *dependency == descriptor.registry_key {
                    return Err(format!(
                        "SYS_MODULE_DESCRIPTOR_SELF_DEPENDENCY:{}",
                        descriptor.registry_key
                    ));
                }
                if !registry_keys.contains(dependency) {
                    return Err(format!(
                        "SYS_MODULE_DESCRIPTOR_UNKNOWN_DEPENDENCY:{}:{}",
                        descriptor.registry_key, dependency
                    ));
                }
            }
        }
    }

    Ok(())
}

pub fn validate_build_order(
    descriptors: &[ModuleDescriptor],
    build_order: &[&str],
) -> Result<(), String> {
    let positions: HashMap<_, _> = build_order
        .iter()
        .enumerate()
        .map(|(index, name)| (*name, index))
        .collect();

    for descriptor in descriptors {
        let Some(position) = positions.get(descriptor.registry_key) else {
            continue;
        };
        for requirement in descriptor.requirements {
            let ModuleRequirement::Module(dependency) = requirement else {
                continue;
            };
            let dependency_position = positions.get(dependency).ok_or_else(|| {
                format!(
                    "SYS_MODULE_DESCRIPTOR_DEPENDENCY_NOT_BUILT:{}:{}",
                    descriptor.registry_key, dependency
                )
            })?;
            if dependency_position >= position {
                return Err(format!(
                    "SYS_MODULE_DESCRIPTOR_BUILD_ORDER:{}:{}",
                    descriptor.registry_key, dependency
                ));
            }
        }
    }

    Ok(())
}

pub fn validate_descriptor_projection(
    descriptors: &[ModuleDescriptor],
    modules: &HashMap<String, Arc<dyn SystemModule>>,
    build_order: &[&str],
    construction_receipts: &[ConstructionReceipt],
    provider_config: &ProviderDeploymentConfig,
) -> Result<(), String> {
    validate_descriptor_catalog(descriptors)?;

    let descriptor_by_key: HashMap<_, _> = descriptors
        .iter()
        .map(|descriptor| (descriptor.registry_key, descriptor))
        .collect();
    let active: HashSet<_> = descriptors
        .iter()
        .filter(|descriptor| match descriptor.activation {
            ModuleActivation::Always => true,
            ModuleActivation::ProviderConfigured(provider) => {
                provider_config.is_configured(provider)
            }
        })
        .map(|descriptor| descriptor.registry_key)
        .collect();

    for key in modules.keys() {
        let descriptor = descriptor_by_key
            .get(key.as_str())
            .ok_or_else(|| format!("SYS_MODULE_WITHOUT_DESCRIPTOR:{key}"))?;
        if !active.contains(descriptor.registry_key) {
            return Err(format!("SYS_DISABLED_PROVIDER_BUILT:{key}"));
        }
    }

    for name in &active {
        if !modules.contains_key(*name) {
            return Err(format!("SYS_ACTIVE_DESCRIPTOR_NOT_BUILT:{name}"));
        }
    }

    let module_keys: HashSet<_> = modules.keys().map(String::as_str).collect();
    let build_order_keys: HashSet<_> = build_order.iter().copied().collect();
    if build_order_keys.len() != build_order.len() {
        return Err("SYS_MODULE_BUILD_ORDER_DUPLICATE_KEY".into());
    }
    for key in &build_order_keys {
        if !active.contains(key) {
            return Err(format!("SYS_MODULE_BUILD_ORDER_UNKNOWN_KEY:{key}"));
        }
        if !module_keys.contains(key) {
            return Err(format!("SYS_MODULE_BUILD_ORDER_WITHOUT_MODULE:{key}"));
        }
    }

    let mut receipt_keys = HashSet::new();
    let mut receipt_factories = HashSet::new();
    for receipt in construction_receipts {
        if !receipt_keys.insert(receipt.registry_key) {
            return Err(format!(
                "SYS_MODULE_CONSTRUCTION_DUPLICATE_KEY:{}",
                receipt.registry_key
            ));
        }
        if !receipt_factories.insert(receipt.factory) {
            return Err(format!(
                "SYS_MODULE_CONSTRUCTION_DUPLICATE_FACTORY:{:?}",
                receipt.factory
            ));
        }
        let descriptor = descriptor_by_key.get(receipt.registry_key).ok_or_else(|| {
            format!(
                "SYS_MODULE_CONSTRUCTION_WITHOUT_DESCRIPTOR:{}",
                receipt.registry_key
            )
        })?;
        if descriptor.factory != receipt.factory {
            return Err(format!(
                "SYS_MODULE_FACTORY_SELECTOR_MISMATCH:{}:{:?}:{:?}",
                receipt.registry_key, descriptor.factory, receipt.factory
            ));
        }
    }

    if module_keys != active {
        return Err("SYS_MODULE_SET_CLOSURE_MODULES".into());
    }
    if build_order_keys != active {
        return Err("SYS_MODULE_SET_CLOSURE_BUILD_ORDER".into());
    }
    if receipt_keys != active {
        return Err("SYS_MODULE_SET_CLOSURE_CONSTRUCTION".into());
    }

    validate_build_order(descriptors, build_order)?;

    for (key, module) in modules {
        let descriptor = descriptor_by_key
            .get(key.as_str())
            .expect("module descriptor was checked above");
        let metadata_name = module.metadata().name;
        if metadata_name != descriptor.metadata_name {
            return Err(format!(
                "SYS_MODULE_METADATA_NAME_MISMATCH:{}:{}:{}",
                descriptor.registry_key, descriptor.metadata_name, metadata_name
            ));
        }
        let schema_name = module.schema().name;
        if schema_name != descriptor.schema_name {
            return Err(format!(
                "SYS_MODULE_SCHEMA_NAME_MISMATCH:{}:{}:{}",
                descriptor.registry_key, descriptor.schema_name, schema_name
            ));
        }

        for requirement in descriptor.requirements {
            if let ModuleRequirement::Module(dependency) = requirement
                && !modules.contains_key(*dependency)
            {
                return Err(format!(
                    "SYS_MODULE_DESCRIPTOR_DEPENDENCY_NOT_BUILT:{}:{}",
                    descriptor.registry_key, dependency
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use serde_json::Value;
    use system_core::{
        AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ExecutionContext,
        ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
    };

    use crate::registry::descriptors::{
        ModuleActivation, ModuleClass, ModuleDescriptor, ModuleFactoryId, ModuleRequirement,
        ProviderId,
    };
    use crate::registry::factory::ConstructionReceipt;
    use crate::registry::provider_config::ProviderDeploymentConfig;

    use super::{
        validate_build_order, validate_command_metadata, validate_descriptor_catalog,
        validate_descriptor_projection,
    };

    const READ: CommandMetadata = CommandMetadata::new(
        "read",
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseRead],
        SimulationSupport::Supported,
    );
    const OTHER: CommandMetadata = CommandMetadata::new(
        "other",
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseRead],
        SimulationSupport::Supported,
    );

    fn schema(commands: &[&str]) -> ModuleSchema {
        ModuleSchema {
            name: "probe".into(),
            description: "probe".into(),
            commands: commands
                .iter()
                .map(|name| CommandSchema {
                    name: (*name).into(),
                    description: "probe".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                })
                .collect(),
        }
    }

    #[test]
    fn rejects_missing_metadata() {
        let error =
            validate_command_metadata("probe", &schema(&["read", "write"]), &[READ]).unwrap_err();
        assert_eq!(error, "SYS_COMMAND_METADATA_MISSING:probe:write");
    }

    #[test]
    fn rejects_duplicate_and_unknown_metadata() {
        let duplicate =
            validate_command_metadata("probe", &schema(&["read"]), &[READ, READ]).unwrap_err();
        assert_eq!(duplicate, "SYS_COMMAND_METADATA_DUPLICATE:probe:read");

        let unknown =
            validate_command_metadata("probe", &schema(&["read"]), &[READ, OTHER]).unwrap_err();
        assert_eq!(unknown, "SYS_COMMAND_METADATA_UNKNOWN:probe:other");
    }

    #[derive(Clone)]
    struct NamedModule {
        metadata_name: &'static str,
        schema_name: &'static str,
    }

    impl SystemModule for NamedModule {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                name: self.metadata_name.into(),
                version: "1.0.0".into(),
                description: "descriptor validation probe".into(),
                author: "test".into(),
                wasm_compatible: false,
                storage: None,
            }
        }

        fn init(&mut self, _config: Value) -> Result<(), String> {
            Ok(())
        }

        fn commands(&self) -> Vec<CommandMetadata> {
            vec![]
        }

        fn execute(
            &self,
            _command: &str,
            _payload: Value,
            _ctx: &ExecutionContext,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }

        fn schema(&self) -> ModuleSchema {
            ModuleSchema {
                name: self.schema_name.into(),
                description: "descriptor validation probe".into(),
                commands: vec![],
            }
        }
    }

    const NONE: &[ModuleRequirement] = &[];
    const DEPENDS_ON_LEAF: &[ModuleRequirement] = &[ModuleRequirement::Module("leaf")];
    const UNKNOWN_DEPENDENCY: &[ModuleRequirement] = &[ModuleRequirement::Module("unknown")];
    const SELF_DEPENDENCY: &[ModuleRequirement] = &[ModuleRequirement::Module("dependent")];
    const PROVIDER_CONFIG: &[ModuleRequirement] =
        &[ModuleRequirement::ProviderConfig(ProviderId::SfExpress)];
    const OTHER_PROVIDER_CONFIG: &[ModuleRequirement] =
        &[ModuleRequirement::ProviderConfig(ProviderId::WechatPay)];

    fn descriptor(
        registry_key: &'static str,
        factory: ModuleFactoryId,
        requirements: &'static [ModuleRequirement],
    ) -> ModuleDescriptor {
        ModuleDescriptor {
            registry_key,
            metadata_name: registry_key,
            schema_name: registry_key,
            class: ModuleClass::Business,
            activation: ModuleActivation::Always,
            requirements,
            factory,
        }
    }

    fn receipt(registry_key: &'static str, factory: ModuleFactoryId) -> ConstructionReceipt {
        ConstructionReceipt {
            registry_key,
            factory,
        }
    }

    fn named_module(name: &'static str) -> Arc<dyn SystemModule> {
        Arc::new(NamedModule {
            metadata_name: name,
            schema_name: name,
        })
    }

    #[test]
    fn descriptor_catalog_rejects_duplicate_keys_and_factory_selectors() {
        let duplicate_name = [
            descriptor("leaf", ModuleFactoryId::Device, NONE),
            descriptor("leaf", ModuleFactoryId::Order, NONE),
        ];
        assert!(
            validate_descriptor_catalog(&duplicate_name)
                .unwrap_err()
                .contains("DUPLICATE_REGISTRY_KEY")
        );

        let duplicate_factory = [
            descriptor("leaf", ModuleFactoryId::Device, NONE),
            descriptor("other", ModuleFactoryId::Device, NONE),
        ];
        assert!(
            validate_descriptor_catalog(&duplicate_factory)
                .unwrap_err()
                .contains("DUPLICATE_FACTORY")
        );
    }

    #[test]
    fn descriptor_catalog_rejects_unknown_and_self_dependencies() {
        let unknown = [descriptor(
            "dependent",
            ModuleFactoryId::Order,
            UNKNOWN_DEPENDENCY,
        )];
        assert!(
            validate_descriptor_catalog(&unknown)
                .unwrap_err()
                .contains("UNKNOWN_DEPENDENCY")
        );

        let self_dependency = [descriptor(
            "dependent",
            ModuleFactoryId::Order,
            SELF_DEPENDENCY,
        )];
        assert!(
            validate_descriptor_catalog(&self_dependency)
                .unwrap_err()
                .contains("SELF_DEPENDENCY")
        );
    }

    #[test]
    fn build_order_rejects_dependency_after_dependent() {
        let descriptors = [
            descriptor("leaf", ModuleFactoryId::Device, NONE),
            descriptor("dependent", ModuleFactoryId::Order, DEPENDS_ON_LEAF),
        ];
        assert!(
            validate_build_order(&descriptors, &["dependent", "leaf"])
                .unwrap_err()
                .contains("BUILD_ORDER")
        );
    }

    #[test]
    fn projection_rejects_missing_extra_and_mismatched_modules() {
        let descriptors = [descriptor("leaf", ModuleFactoryId::Device, NONE)];
        let config = ProviderDeploymentConfig::default();

        let missing = HashMap::new();
        assert!(
            validate_descriptor_projection(&descriptors, &missing, &[], &[], &config)
                .unwrap_err()
                .contains("ACTIVE_DESCRIPTOR_NOT_BUILT")
        );

        let mut extra = HashMap::new();
        extra.insert("extra".into(), named_module("extra"));
        assert!(
            validate_descriptor_projection(
                &descriptors,
                &extra,
                &["extra"],
                &[receipt("extra", ModuleFactoryId::Device)],
                &config,
            )
            .unwrap_err()
            .contains("WITHOUT_DESCRIPTOR")
        );

        let mut metadata_mismatch = HashMap::new();
        metadata_mismatch.insert(
            "leaf".into(),
            Arc::new(NamedModule {
                metadata_name: "wrong",
                schema_name: "leaf",
            }) as Arc<dyn SystemModule>,
        );
        assert!(
            validate_descriptor_projection(
                &descriptors,
                &metadata_mismatch,
                &["leaf"],
                &[receipt("leaf", ModuleFactoryId::Device)],
                &config,
            )
            .unwrap_err()
            .contains("METADATA_NAME_MISMATCH")
        );

        let mut schema_mismatch = HashMap::new();
        schema_mismatch.insert(
            "leaf".into(),
            Arc::new(NamedModule {
                metadata_name: "leaf",
                schema_name: "wrong",
            }) as Arc<dyn SystemModule>,
        );
        assert!(
            validate_descriptor_projection(
                &descriptors,
                &schema_mismatch,
                &["leaf"],
                &[receipt("leaf", ModuleFactoryId::Device)],
                &config,
            )
            .unwrap_err()
            .contains("SCHEMA_NAME_MISMATCH")
        );
    }

    #[test]
    fn projection_rejects_provider_activation_mismatches() {
        let provider = ModuleDescriptor {
            registry_key: "sf_express",
            metadata_name: "sf_express",
            schema_name: "sf_express",
            class: ModuleClass::Provider,
            activation: ModuleActivation::ProviderConfigured(ProviderId::SfExpress),
            requirements: PROVIDER_CONFIG,
            factory: ModuleFactoryId::SfExpress,
        };

        let mut disabled_built = HashMap::new();
        disabled_built.insert("sf_express".into(), named_module("sf_express"));
        assert!(
            validate_descriptor_projection(
                &[provider],
                &disabled_built,
                &["sf_express"],
                &[receipt("sf_express", ModuleFactoryId::SfExpress)],
                &ProviderDeploymentConfig::default(),
            )
            .unwrap_err()
            .contains("DISABLED_PROVIDER_BUILT")
        );

        validate_descriptor_projection(
            &[provider],
            &HashMap::new(),
            &[],
            &[],
            &ProviderDeploymentConfig,
        )
        .unwrap();
    }

    #[test]
    fn projection_rejects_missing_declared_module_dependency() {
        let descriptors = [
            descriptor("leaf", ModuleFactoryId::Device, NONE),
            descriptor("dependent", ModuleFactoryId::Order, DEPENDS_ON_LEAF),
        ];
        let mut modules = HashMap::new();
        modules.insert("dependent".into(), named_module("dependent"));
        assert!(
            validate_descriptor_projection(
                &descriptors,
                &modules,
                &["dependent"],
                &[receipt("dependent", ModuleFactoryId::Order)],
                &ProviderDeploymentConfig::default(),
            )
            .unwrap_err()
            .contains("ACTIVE_DESCRIPTOR_NOT_BUILT")
        );
    }

    #[test]
    fn valid_unequal_identity_mapping_passes_and_wrong_registry_mapping_fails() {
        let descriptor = ModuleDescriptor {
            registry_key: "device",
            metadata_name: "feature-device",
            schema_name: "feature-device",
            class: ModuleClass::Business,
            activation: ModuleActivation::Always,
            requirements: NONE,
            factory: ModuleFactoryId::Device,
        };
        let mut modules = HashMap::new();
        modules.insert(
            "device".into(),
            Arc::new(NamedModule {
                metadata_name: "feature-device",
                schema_name: "feature-device",
            }) as Arc<dyn SystemModule>,
        );
        validate_descriptor_projection(
            &[descriptor],
            &modules,
            &["device"],
            &[receipt("device", ModuleFactoryId::Device)],
            &ProviderDeploymentConfig::default(),
        )
        .unwrap();

        let wrong_receipt = [receipt("other", ModuleFactoryId::Device)];
        assert!(
            validate_descriptor_projection(
                &[descriptor],
                &modules,
                &["device"],
                &wrong_receipt,
                &ProviderDeploymentConfig::default(),
            )
            .unwrap_err()
            .contains("CONSTRUCTION_WITHOUT_DESCRIPTOR")
        );
    }

    #[test]
    fn projection_rejects_decorative_selector_mutation_and_unknown_build_key() {
        let descriptor = descriptor("device", ModuleFactoryId::Order, NONE);
        let mut modules = HashMap::new();
        modules.insert("device".into(), named_module("device"));
        assert!(
            validate_descriptor_projection(
                &[descriptor],
                &modules,
                &["device"],
                &[receipt("device", ModuleFactoryId::Device)],
                &ProviderDeploymentConfig::default(),
            )
            .unwrap_err()
            .contains("FACTORY_SELECTOR_MISMATCH")
        );

        assert!(
            validate_descriptor_projection(
                &[descriptor],
                &modules,
                &["unknown"],
                &[receipt("device", ModuleFactoryId::Order)],
                &ProviderDeploymentConfig::default(),
            )
            .unwrap_err()
            .contains("BUILD_ORDER_UNKNOWN_KEY")
        );
    }

    #[test]
    fn descriptor_catalog_rejects_invalid_provider_pairings() {
        let provider_always = ModuleDescriptor {
            registry_key: "provider",
            metadata_name: "provider",
            schema_name: "provider",
            class: ModuleClass::Provider,
            activation: ModuleActivation::Always,
            requirements: PROVIDER_CONFIG,
            factory: ModuleFactoryId::SfExpress,
        };
        assert!(
            validate_descriptor_catalog(&[provider_always])
                .unwrap_err()
                .contains("INVALID_PROVIDER")
        );

        let non_provider_configured = ModuleDescriptor {
            registry_key: "business",
            metadata_name: "business",
            schema_name: "business",
            class: ModuleClass::Business,
            activation: ModuleActivation::ProviderConfigured(ProviderId::SfExpress),
            requirements: PROVIDER_CONFIG,
            factory: ModuleFactoryId::Device,
        };
        assert!(
            validate_descriptor_catalog(&[non_provider_configured])
                .unwrap_err()
                .contains("INVALID_PROVIDER_PAIRING")
        );

        let provider_without_config = ModuleDescriptor {
            registry_key: "provider",
            metadata_name: "provider",
            schema_name: "provider",
            class: ModuleClass::Provider,
            activation: ModuleActivation::ProviderConfigured(ProviderId::SfExpress),
            requirements: NONE,
            factory: ModuleFactoryId::SfExpress,
        };
        assert!(
            validate_descriptor_catalog(&[provider_without_config])
                .unwrap_err()
                .contains("INVALID_PROVIDER")
        );

        let mismatched_config = ModuleDescriptor {
            registry_key: "provider",
            metadata_name: "provider",
            schema_name: "provider",
            class: ModuleClass::Provider,
            activation: ModuleActivation::ProviderConfigured(ProviderId::SfExpress),
            requirements: OTHER_PROVIDER_CONFIG,
            factory: ModuleFactoryId::SfExpress,
        };
        assert!(
            validate_descriptor_catalog(&[mismatched_config])
                .unwrap_err()
                .contains("INVALID_PROVIDER")
        );

        let non_provider_with_config = ModuleDescriptor {
            registry_key: "business",
            metadata_name: "business",
            schema_name: "business",
            class: ModuleClass::Business,
            activation: ModuleActivation::Always,
            requirements: PROVIDER_CONFIG,
            factory: ModuleFactoryId::Device,
        };
        assert!(
            validate_descriptor_catalog(&[non_provider_with_config])
                .unwrap_err()
                .contains("INVALID_PROVIDER_PAIRING")
        );
    }
}

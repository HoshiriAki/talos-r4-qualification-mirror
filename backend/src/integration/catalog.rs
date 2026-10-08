use std::collections::BTreeMap;

use system_core::{ExecutionContext, ExecutionMode};

use super::types::{
    CapabilityId, IntegrationError, ProviderBinding, ProviderBindingId, ProviderHealth,
    ProviderInstance, ProviderInstanceId, ProviderLifecycle, ProviderManifest, ProviderReadiness,
    ResolvedProviderBinding,
};

#[derive(Default)]
pub struct ProviderCatalog {
    manifests: BTreeMap<(String, String), ProviderManifest>,
    // Provider instance and binding IDs are tenant-local database identities.
    // Loading the complete catalog must therefore retain the tenant in the
    // in-memory key rather than allowing the last loaded tenant to win.
    instances: BTreeMap<(String, ProviderInstanceId), ProviderInstance>,
    bindings: BTreeMap<(String, ProviderBindingId), ProviderBinding>,
}

impl ProviderCatalog {
    pub fn register_manifest(
        &mut self,
        manifest: ProviderManifest,
    ) -> Result<(), IntegrationError> {
        manifest.validate()?;
        self.manifests.insert(
            (
                manifest.provider_id.as_str().to_string(),
                manifest.version.clone(),
            ),
            manifest,
        );
        Ok(())
    }

    pub fn register_instance(
        &mut self,
        instance: ProviderInstance,
    ) -> Result<(), IntegrationError> {
        instance.validate()?;
        self.instances
            .insert((instance.tenant_id.clone(), instance.id.clone()), instance);
        Ok(())
    }

    pub fn register_binding(&mut self, binding: ProviderBinding) -> Result<(), IntegrationError> {
        binding.validate()?;
        self.bindings
            .insert((binding.tenant_id.clone(), binding.id.clone()), binding);
        Ok(())
    }

    pub fn resolve(
        &self,
        ctx: &ExecutionContext,
        capability: &CapabilityId,
    ) -> Result<ResolvedProviderBinding, IntegrationError> {
        if !matches!(ctx.execution_mode(), ExecutionMode::Normal) {
            // Preview must not enter provider execution. Simulation creates
            // effect intent through an explicit simulation adapter instead.
            return Err(IntegrationError::ModeBlocked);
        }
        let tenant_id = ctx
            .data_scope()
            .tenant_id_opt()
            .ok_or(IntegrationError::BindingUnavailable)?
            .as_str();
        let binding = self
            .bindings
            .values()
            .find(|binding| {
                binding.tenant_id == tenant_id
                    && binding.enabled
                    && binding.capability == *capability
            })
            .cloned()
            .ok_or(IntegrationError::BindingUnavailable)?;
        let instance = self
            .instances
            .get(&(tenant_id.to_owned(), binding.provider_instance_id.clone()))
            .cloned()
            .ok_or(IntegrationError::BindingUnavailable)?;
        if instance.tenant_id != tenant_id
            || instance.lifecycle != ProviderLifecycle::Active
            || instance.health != ProviderHealth::Ready
            || !instance.readiness.permits_dispatch()
            || instance.config_revision != binding.config_revision
        {
            return Err(IntegrationError::InstanceNotReady);
        }
        let manifest = self
            .manifests
            .get(&(
                instance.provider_id.as_str().to_string(),
                instance.manifest_version.clone(),
            ))
            .cloned()
            .ok_or(IntegrationError::BindingUnavailable)?;
        if !manifest.readiness.permits_dispatch() {
            return Err(IntegrationError::InstanceNotReady);
        }
        if !manifest.declares(capability) {
            return Err(IntegrationError::CapabilityNotDeclared);
        }
        if manifest.secret_schema.iter().any(|requirement| {
            requirement.required && !instance.secret_refs.contains_key(&requirement.name)
        }) {
            return Err(IntegrationError::RequiredSecretMissing);
        }
        Ok(ResolvedProviderBinding {
            manifest,
            instance,
            binding,
        })
    }

    /// Returns only Manifest declarations. Manifests describe required secret
    /// references, never secret values.
    pub fn manifests(&self) -> Vec<ProviderManifest> {
        self.manifests.values().cloned().collect()
    }

    pub fn manifest(
        &self,
        provider_id: &super::types::ProviderId,
        version: &str,
    ) -> Option<ProviderManifest> {
        self.manifests
            .get(&(provider_id.as_str().to_owned(), version.to_owned()))
            .cloned()
    }

    pub fn instance(&self, tenant_id: &str, id: &ProviderInstanceId) -> Option<ProviderInstance> {
        self.instances
            .get(&(tenant_id.to_owned(), id.clone()))
            .cloned()
    }

    /// Tenant administration may inspect its own configured instances. Callers
    /// must project this value before serialization so `secret_refs` cannot
    /// become an API response by accident.
    pub fn instances_for_tenant(&self, tenant_id: &str) -> Vec<ProviderInstance> {
        self.instances
            .values()
            .filter(|instance| instance.tenant_id == tenant_id)
            .cloned()
            .collect()
    }

    pub fn bindings_for_tenant(&self, tenant_id: &str) -> Vec<ProviderBinding> {
        self.bindings
            .values()
            .filter(|binding| binding.tenant_id == tenant_id)
            .cloned()
            .collect()
    }

    pub fn fixture_binding(
        manifest: ProviderManifest,
        mut instance: ProviderInstance,
        binding: ProviderBinding,
    ) -> Self {
        instance.readiness = ProviderReadiness::Fixture;
        instance.lifecycle = ProviderLifecycle::Active;
        instance.health = ProviderHealth::Ready;
        let mut catalog = Self::default();
        catalog
            .register_manifest(manifest)
            .expect("fixture manifest must validate");
        catalog
            .register_instance(instance)
            .expect("fixture instance must validate");
        catalog
            .register_binding(binding)
            .expect("fixture binding must validate");
        catalog
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
        NoopHttpClient, PlatformMembershipId, PlatformRole, RequestId, Revision, SimulationId,
        TenantId, TenantScope,
    };

    use super::ProviderCatalog;
    use crate::integration::types::{
        CapabilityId, IntegrationError, ProviderBinding, ProviderBindingId, ProviderHealth,
        ProviderId, ProviderInstance, ProviderInstanceId, ProviderLifecycle, ProviderManifest,
        ProviderReadiness,
    };

    fn fixture() -> (
        ProviderManifest,
        ProviderInstance,
        ProviderBinding,
        CapabilityId,
    ) {
        let capability = CapabilityId::new("fixture.payment.charge").unwrap();
        let provider_id = ProviderId::new("fixture-payments").unwrap();
        let manifest = ProviderManifest {
            provider_id: provider_id.clone(),
            version: "1.0.0".into(),
            capabilities: BTreeSet::from([capability.clone()]),
            config_schema: vec![],
            secret_schema: vec![],
            api_versions: BTreeMap::new(),
            webhook_types: BTreeSet::new(),
            simulation_capabilities: BTreeSet::from([capability.clone()]),
            readiness: ProviderReadiness::Fixture,
            compatibility: BTreeMap::new(),
        };
        let instance = ProviderInstance {
            id: ProviderInstanceId::new("instance-a").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_id,
            manifest_version: "1.0.0".into(),
            config_revision: "rev-1".into(),
            config: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            lifecycle: ProviderLifecycle::Draft,
            health: ProviderHealth::Unknown,
            readiness: ProviderReadiness::Stub,
        };
        let binding = ProviderBinding {
            id: ProviderBindingId::new("binding-a").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_instance_id: instance.id.clone(),
            capability: capability.clone(),
            config_revision: "rev-1".into(),
            enabled: true,
        };
        (manifest, instance, binding, capability)
    }

    fn context(mode: ExecutionMode, tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        let authority = AuthorityContext::Platform {
            membership_id: PlatformMembershipId::new("platform-member").unwrap(),
            roles: vec![PlatformRole::Owner],
        };
        ExecutionContext::new(
            ActorIdentity::with_authority("actor", authority).unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::new(
                tenant_id,
                match &mode {
                    ExecutionMode::Simulation(id) => Namespace::Simulation(id.clone()),
                    _ => Namespace::Production,
                },
                Revision::new("revision-1").unwrap(),
            )
            .unwrap(),
            mode,
            RequestId::new("request-1").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn tenant_binding_isolation_and_readiness_are_enforced() {
        let (manifest, instance, binding, capability) = fixture();
        let catalog = ProviderCatalog::fixture_binding(manifest, instance, binding);
        assert!(
            catalog
                .resolve(&context(ExecutionMode::Normal, "tenant-a"), &capability)
                .is_ok()
        );
        assert_eq!(
            catalog
                .resolve(&context(ExecutionMode::Normal, "tenant-b"), &capability)
                .unwrap_err(),
            IntegrationError::BindingUnavailable
        );
    }

    #[test]
    fn preview_and_simulation_cannot_dispatch_a_provider() {
        let (manifest, instance, binding, capability) = fixture();
        let catalog = ProviderCatalog::fixture_binding(manifest, instance, binding);
        let preview = ExecutionMode::ReadOnlyPreview(
            system_core::PreviewSessionId::new("preview-a").unwrap(),
        );
        let simulation = ExecutionMode::Simulation(SimulationId::new("simulation-a").unwrap());
        assert_eq!(
            catalog
                .resolve(&context(preview, "tenant-a"), &capability)
                .unwrap_err(),
            IntegrationError::ModeBlocked
        );
        assert_eq!(
            catalog
                .resolve(&context(simulation, "tenant-a"), &capability)
                .unwrap_err(),
            IntegrationError::ModeBlocked
        );
    }
}

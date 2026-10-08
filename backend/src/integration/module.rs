use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use super::catalog_authority::IntegrationCatalogAuthority;
use super::deposit::DepositReconciliationOutcome;
use super::deposit_refund_persistence_contract::DepositRefundPersistence;
#[cfg(feature = "postgres")]
use super::deposit_refund_persistence_postgres::PostgresDepositRefundPersistence;
#[cfg(feature = "postgres")]
use super::keystore::KeyStore;
use super::operation::ExternalOperation;
use super::operation_runtime_contract::OperationRuntimePersistence;
#[cfg(feature = "postgres")]
use super::operation_runtime_postgres::PostgresOperationRuntimePersistence;
use super::store::IntegrationStore;
use super::types::{
    ApiVersion, CapabilityId, CompatibilityRule, ConfigField, ConfigValueType, DepositId,
    ExternalOperationId, IntegrationError, Money, NonSecretConfigValue, ProviderBinding,
    ProviderBindingId, ProviderHealth, ProviderId, ProviderInstance, ProviderInstanceId,
    ProviderLifecycle, ProviderManifest, ProviderReadiness, SecretPurpose, SecretRef,
    SecretRequirement, SecretValueType, WebhookEndpointId, WebhookInboxId,
};
use super::webhook_persistence_contract::WebhookAdminPersistence;
#[cfg(feature = "postgres")]
use super::webhook_persistence_postgres::PostgresWebhookPersistence;
#[cfg(feature = "postgres")]
use crate::observability::MetricsSink;

const MODULE_NAME: &str = "integration";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManifestInput {
    provider_id: String,
    version: String,
    capabilities: Vec<String>,
    #[serde(default)]
    config_schema: Vec<ConfigField>,
    #[serde(default)]
    secret_schema: Vec<SecretRequirement>,
    #[serde(default)]
    api_versions: BTreeMap<CapabilityId, ApiVersion>,
    #[serde(default)]
    webhook_types: Vec<String>,
    #[serde(default)]
    simulation_capabilities: Vec<String>,
    readiness: ProviderReadiness,
    #[serde(default)]
    compatibility: BTreeMap<CapabilityId, CompatibilityRule>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstanceInput {
    id: String,
    provider_id: String,
    manifest_version: String,
    config_revision: String,
    #[serde(default)]
    config: BTreeMap<String, NonSecretConfigValue>,
    #[serde(default)]
    secret_refs: BTreeMap<String, String>,
    lifecycle: ProviderLifecycle,
    health: ProviderHealth,
    readiness: ProviderReadiness,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BindingInput {
    id: String,
    provider_instance_id: String,
    capability: String,
    config_revision: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CapabilityInput {
    capability: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateWebhookEndpointInput {
    binding_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SetWebhookEndpointEnabledInput {
    endpoint_id: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplayWebhookInput {
    inbox_id: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanOperationInput {
    capability: String,
    operation_type: String,
    idempotency_key: String,
    request_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateDepositInput {
    authority_kind: String,
    authority_id: String,
    amount_minor: i64,
    currency: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestRefundInput {
    deposit_id: String,
    amount_minor: i64,
    currency: String,
    idempotency_key: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordDepositReceiptInput {
    deposit_id: String,
    amount_minor: i64,
    currency: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PlanRefundOperationInput {
    refund_id: String,
    capability: String,
    request_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReconcileRefundInput {
    refund_id: String,
    outcome: DepositReconciliationOutcome,
    evidence_ref: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BindingInspection {
    provider_id: String,
    manifest_version: String,
    provider_instance_id: String,
    provider_binding_id: String,
    capability: String,
    binding_revision: String,
    readiness: ProviderReadiness,
    health: ProviderHealth,
    lifecycle: ProviderLifecycle,
}

/// Deliberately omits `secret_refs` and config values. The tenant control
/// surface needs readiness and revision facts, not material that could be used
/// to resolve a Provider credential outside KeyStore.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstanceInspection {
    provider_instance_id: String,
    provider_id: String,
    manifest_version: String,
    config_revision: String,
    config_keys: Vec<String>,
    configured_secret_names: Vec<String>,
    lifecycle: ProviderLifecycle,
    readiness: ProviderReadiness,
    health: ProviderHealth,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BindingSummary {
    provider_binding_id: String,
    provider_instance_id: String,
    capability: String,
    binding_revision: String,
    enabled: bool,
}

/// Stable public projection of a manifest. It uses the HTTP camelCase profile
/// without changing the stored schema representation used by older rows.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestInspection {
    provider_id: String,
    version: String,
    capabilities: Vec<String>,
    config_schema: Vec<ManifestConfigFieldInspection>,
    secret_schema: Vec<ManifestSecretRequirementInspection>,
    api_versions: BTreeMap<CapabilityId, ApiVersion>,
    webhook_types: Vec<String>,
    simulation_capabilities: Vec<String>,
    readiness: ProviderReadiness,
    compatibility: BTreeMap<CapabilityId, CompatibilityRule>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestConfigFieldInspection {
    name: String,
    required: bool,
    value_type: ConfigValueType,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestSecretRequirementInspection {
    name: String,
    required: bool,
    value_type: SecretValueType,
    purpose: SecretPurpose,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookEndpointInspection {
    webhook_endpoint_id: String,
    provider_binding_id: String,
    provider_id: String,
    enabled: bool,
    created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookDeadLetterInspection {
    inbox_id: String,
    webhook_endpoint_id: String,
    provider_event_id: String,
    reason: String,
    replay_count: u64,
    created_at: String,
    replayed_at: Option<String>,
}

#[derive(Debug)]
enum IntegrationModuleError {
    InvalidInput(String),
    Integration(IntegrationError),
    Serialize(String),
    UnknownCommand(String),
}

impl From<IntegrationError> for IntegrationModuleError {
    fn from(value: IntegrationError) -> Self {
        Self::Integration(value)
    }
}

impl IntegrationModuleError {
    fn into_json(self) -> String {
        let (category, code, message) = match self {
            Self::InvalidInput(message) => ("val", "VAL_INTEGRATION_INPUT", message),
            Self::Integration(error) => integration_error_payload(error),
            Self::Serialize(message) => ("sys", "SYS_INTEGRATION_SERIALIZE", message),
            Self::UnknownCommand(command) => (
                "sys",
                "SYS_UNKNOWN_COMMAND",
                format!("unknown integration command: {command}"),
            ),
        };
        serde_json::to_string(&ErrorPayload {
            category: category.into(),
            code: code.into(),
            message,
            field: None,
            context: None,
        })
        .unwrap_or_else(|_| {
            r#"{"category":"sys","code":"SYS_INTEGRATION_SERIALIZE","message":"failed to serialize integration error","field":null,"context":null}"#.into()
        })
    }
}

fn integration_error_payload(error: IntegrationError) -> (&'static str, &'static str, String) {
    let message = error.to_string();
    let (category, code) = match error {
        IntegrationError::BlankValue { .. }
        | IntegrationError::InvalidManifest(_)
        | IntegrationError::InvalidSecretReference
        | IntegrationError::RequiredSecretMissing
        | IntegrationError::InvalidTransportRequest => ("val", "VAL_INTEGRATION_INPUT"),
        IntegrationError::BindingUnavailable | IntegrationError::SecretUnavailable => {
            ("biz", "BIZ_INTEGRATION_NOT_FOUND")
        }
        IntegrationError::BindingRevisionStale
        | IntegrationError::InstanceNotReady
        | IntegrationError::InvalidOperationTransition
        | IntegrationError::RefundAmountExceedsAvailable
        | IntegrationError::RefundOperationUnavailable
        | IntegrationError::CurrencyMismatch => ("biz", "BIZ_INTEGRATION_CONFLICT"),
        IntegrationError::CircuitOpen
        | IntegrationError::ConcurrencyLimited
        | IntegrationError::RateLimited => ("biz", "BIZ_INTEGRATION_RATE_LIMITED"),
        IntegrationError::ModeBlocked
        | IntegrationError::SecretAccessDenied
        | IntegrationError::EgressDenied => ("auth", "AUTH_INTEGRATION_FORBIDDEN"),
        IntegrationError::CapabilityNotDeclared
        | IntegrationError::WebhookUnverifiable
        | IntegrationError::OperationNotDue
        | IntegrationError::ResponseTooLarge
        | IntegrationError::Timeout
        | IntegrationError::TransportFailure => ("biz", "BIZ_INTEGRATION_REJECTED"),
        IntegrationError::Persistence => ("sys", "SYS_INTEGRATION_PERSISTENCE"),
    };
    (category, code, message)
}

#[derive(Clone)]
pub struct IntegrationModule {
    catalog: IntegrationCatalogAuthority,
    operations: Arc<dyn OperationRuntimePersistence>,
    webhook_admin: Arc<dyn WebhookAdminPersistence>,
    finance: Arc<dyn DepositRefundPersistence>,
}

impl IntegrationModule {
    pub fn new(store: IntegrationStore) -> Self {
        let catalog = store.catalog_authority();
        Self::new_with_persistence(
            catalog,
            Arc::new(store.clone()),
            Arc::new(store.clone()),
            Arc::new(store),
        )
    }

    pub(crate) fn new_with_webhook_admin(
        store: IntegrationStore,
        webhook_admin: Arc<dyn WebhookAdminPersistence>,
    ) -> Self {
        let catalog = store.catalog_authority();
        Self::new_with_persistence(
            catalog,
            Arc::new(store.clone()),
            webhook_admin,
            Arc::new(store),
        )
    }

    pub(crate) fn new_with_persistence(
        catalog: IntegrationCatalogAuthority,
        operations: Arc<dyn OperationRuntimePersistence>,
        webhook_admin: Arc<dyn WebhookAdminPersistence>,
        finance: Arc<dyn DepositRefundPersistence>,
    ) -> Self {
        Self {
            catalog,
            operations,
            webhook_admin,
            finance,
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn new_with_postgres_webhook(store: IntegrationStore, pool: sqlx::PgPool) -> Self {
        let catalog = store.catalog_authority();
        Self::new_with_persistence(
            catalog,
            Arc::new(store.clone()),
            Arc::new(PostgresWebhookPersistence::new(pool)),
            Arc::new(store),
        )
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn new_with_postgres_persistence(
        pool: sqlx::PgPool,
        key_store: Arc<dyn KeyStore>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self::new_with_persistence(
            IntegrationCatalogAuthority::postgres(pool.clone(), key_store),
            Arc::new(PostgresOperationRuntimePersistence::new_with_metrics(
                pool.clone(),
                metrics.clone(),
            )),
            Arc::new(PostgresWebhookPersistence::new(pool.clone())),
            Arc::new(PostgresDepositRefundPersistence::new_with_metrics(
                pool, metrics,
            )),
        )
    }

    fn parse<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload)
            .map_err(|error| IntegrationModuleError::InvalidInput(error.to_string()).into_json())
    }

    fn tenant_id<'a>(ctx: &'a ExecutionContext) -> Result<&'a str, IntegrationModuleError> {
        ctx.data_scope()
            .tenant_id_opt()
            .map(|tenant| tenant.as_str())
            .ok_or_else(|| {
                IntegrationModuleError::InvalidInput("tenant business scope required".into())
            })
    }

    fn actor_ref(ctx: &ExecutionContext) -> &str {
        ctx.actor().id().unwrap_or("system")
    }

    fn register_manifest(&self, input: ManifestInput) -> Result<Value, IntegrationModuleError> {
        let capabilities = unique_ids(input.capabilities, "capability")?;
        let simulation_capabilities =
            unique_ids(input.simulation_capabilities, "simulation capability")?;
        let manifest = ProviderManifest {
            provider_id: ProviderId::new(input.provider_id)?,
            version: input.version,
            capabilities,
            config_schema: input.config_schema,
            secret_schema: input.secret_schema,
            api_versions: input.api_versions,
            webhook_types: input.webhook_types.into_iter().collect(),
            simulation_capabilities,
            readiness: input.readiness,
            compatibility: input.compatibility,
        };
        self.catalog.save_manifest(&manifest)?;
        json(&serde_json::json!({
            "providerId": manifest.provider_id.as_str(),
            "version": manifest.version,
            "registered": true,
        }))
    }

    fn upsert_instance(
        &self,
        ctx: &ExecutionContext,
        input: InstanceInput,
    ) -> Result<Value, IntegrationModuleError> {
        let tenant_id = Self::tenant_id(ctx)?.to_owned();
        let secret_refs = input
            .secret_refs
            .into_iter()
            .map(|(name, reference)| Ok((name, SecretRef::new(reference)?)))
            .collect::<Result<BTreeMap<_, _>, IntegrationError>>()?;
        let instance = ProviderInstance {
            id: ProviderInstanceId::new(input.id)?,
            tenant_id,
            provider_id: ProviderId::new(input.provider_id)?,
            manifest_version: input.manifest_version,
            config_revision: input.config_revision,
            config: input.config,
            secret_refs,
            lifecycle: input.lifecycle,
            health: input.health,
            readiness: input.readiness,
        };
        self.catalog.save_instance(&instance)?;
        json(&serde_json::json!({
            "providerInstanceId": instance.id.as_str(),
            "configRevision": instance.config_revision,
            "registered": true,
        }))
    }

    fn upsert_binding(
        &self,
        ctx: &ExecutionContext,
        input: BindingInput,
    ) -> Result<Value, IntegrationModuleError> {
        let binding = ProviderBinding {
            id: ProviderBindingId::new(input.id)?,
            tenant_id: Self::tenant_id(ctx)?.to_owned(),
            provider_instance_id: ProviderInstanceId::new(input.provider_instance_id)?,
            capability: CapabilityId::new(input.capability)?,
            config_revision: input.config_revision,
            enabled: input.enabled,
        };
        self.catalog.save_binding(&binding, Self::actor_ref(ctx))?;
        json(&serde_json::json!({
            "providerBindingId": binding.id.as_str(),
            "capability": binding.capability.as_str(),
            "bindingRevision": binding.config_revision,
            "enabled": binding.enabled,
        }))
    }

    fn inspect_binding(
        &self,
        ctx: &ExecutionContext,
        input: CapabilityInput,
    ) -> Result<Value, IntegrationModuleError> {
        let capability = CapabilityId::new(input.capability)?;
        let resolved = self.catalog.catalog()?.resolve(ctx, &capability)?;
        json(&BindingInspection {
            provider_id: resolved.instance.provider_id.as_str().to_owned(),
            manifest_version: resolved.instance.manifest_version,
            provider_instance_id: resolved.instance.id.as_str().to_owned(),
            provider_binding_id: resolved.binding.id.as_str().to_owned(),
            capability: resolved.binding.capability.as_str().to_owned(),
            binding_revision: resolved.binding.config_revision,
            readiness: resolved.instance.readiness,
            health: resolved.instance.health,
            lifecycle: resolved.instance.lifecycle,
        })
    }

    fn list_manifests(&self) -> Result<Value, IntegrationModuleError> {
        let manifests = self
            .catalog
            .list_manifests()?
            .into_iter()
            .map(|manifest| ManifestInspection {
                provider_id: manifest.provider_id.as_str().to_owned(),
                version: manifest.version,
                capabilities: manifest
                    .capabilities
                    .into_iter()
                    .map(|capability| capability.as_str().to_owned())
                    .collect(),
                config_schema: manifest
                    .config_schema
                    .into_iter()
                    .map(|field| ManifestConfigFieldInspection {
                        name: field.name,
                        required: field.required,
                        value_type: field.value_type,
                    })
                    .collect(),
                secret_schema: manifest
                    .secret_schema
                    .into_iter()
                    .map(|requirement| ManifestSecretRequirementInspection {
                        name: requirement.name,
                        required: requirement.required,
                        value_type: requirement.value_type,
                        purpose: requirement.purpose,
                    })
                    .collect(),
                api_versions: manifest.api_versions,
                webhook_types: manifest.webhook_types.into_iter().collect(),
                simulation_capabilities: manifest
                    .simulation_capabilities
                    .into_iter()
                    .map(|capability| capability.as_str().to_owned())
                    .collect(),
                readiness: manifest.readiness,
                compatibility: manifest.compatibility,
            })
            .collect::<Vec<_>>();
        json(&serde_json::json!({
            "manifests": manifests,
        }))
    }

    fn list_instances(&self, ctx: &ExecutionContext) -> Result<Value, IntegrationModuleError> {
        let instances = self
            .catalog
            .list_instances(Self::tenant_id(ctx)?)?
            .into_iter()
            .map(|instance| InstanceInspection {
                provider_instance_id: instance.id.as_str().to_owned(),
                provider_id: instance.provider_id.as_str().to_owned(),
                manifest_version: instance.manifest_version,
                config_revision: instance.config_revision,
                config_keys: instance.config.into_keys().collect(),
                configured_secret_names: instance.secret_refs.into_keys().collect(),
                lifecycle: instance.lifecycle,
                readiness: instance.readiness,
                health: instance.health,
            })
            .collect::<Vec<_>>();
        json(&serde_json::json!({ "instances": instances }))
    }

    fn list_bindings(&self, ctx: &ExecutionContext) -> Result<Value, IntegrationModuleError> {
        let bindings = self
            .catalog
            .list_bindings(Self::tenant_id(ctx)?)?
            .into_iter()
            .map(|binding| BindingSummary {
                provider_binding_id: binding.id.as_str().to_owned(),
                provider_instance_id: binding.provider_instance_id.as_str().to_owned(),
                capability: binding.capability.as_str().to_owned(),
                binding_revision: binding.config_revision,
                enabled: binding.enabled,
            })
            .collect::<Vec<_>>();
        json(&serde_json::json!({ "bindings": bindings }))
    }

    fn create_webhook_endpoint(
        &self,
        ctx: &ExecutionContext,
        input: CreateWebhookEndpointInput,
    ) -> Result<Value, IntegrationModuleError> {
        let endpoint_token = uuid::Uuid::new_v4().simple().to_string();
        let endpoint_id = self.webhook_admin.register_webhook_endpoint(
            Self::tenant_id(ctx)?,
            &ProviderBindingId::new(input.binding_id)?,
            endpoint_token.as_bytes(),
        )?;
        // The token is returned exactly once. Persistence retains only its
        // hash, and all subsequent management APIs expose the endpoint ID.
        json(&serde_json::json!({
            "webhookEndpointId": endpoint_id.as_str(),
            "endpointToken": endpoint_token,
        }))
    }

    fn list_webhook_endpoints(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<Value, IntegrationModuleError> {
        let endpoints = self
            .webhook_admin
            .list_webhook_endpoints(Self::tenant_id(ctx)?)?
            .into_iter()
            .map(|endpoint| WebhookEndpointInspection {
                webhook_endpoint_id: endpoint.endpoint_id.as_str().to_owned(),
                provider_binding_id: endpoint.binding_id.as_str().to_owned(),
                provider_id: endpoint.provider_id.as_str().to_owned(),
                enabled: endpoint.enabled,
                created_at: endpoint.created_at,
            })
            .collect::<Vec<_>>();
        json(&serde_json::json!({ "webhookEndpoints": endpoints }))
    }

    fn set_webhook_endpoint_enabled(
        &self,
        ctx: &ExecutionContext,
        input: SetWebhookEndpointEnabledInput,
    ) -> Result<Value, IntegrationModuleError> {
        let endpoint_id = WebhookEndpointId::new(input.endpoint_id)?;
        self.webhook_admin.set_webhook_endpoint_enabled(
            Self::tenant_id(ctx)?,
            &endpoint_id,
            input.enabled,
        )?;
        json(&serde_json::json!({
            "webhookEndpointId": endpoint_id.as_str(),
            "enabled": input.enabled,
        }))
    }

    fn list_webhook_dead_letters(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<Value, IntegrationModuleError> {
        let dead_letters = self
            .webhook_admin
            .list_webhook_dead_letters(Self::tenant_id(ctx)?)?
            .into_iter()
            .map(|letter| WebhookDeadLetterInspection {
                inbox_id: letter.inbox_id.as_str().to_owned(),
                webhook_endpoint_id: letter.endpoint_id.as_str().to_owned(),
                provider_event_id: letter.provider_event_id,
                reason: letter.reason,
                replay_count: letter.replay_count,
                created_at: letter.created_at,
                replayed_at: letter.replayed_at,
            })
            .collect::<Vec<_>>();
        json(&serde_json::json!({ "webhookDeadLetters": dead_letters }))
    }

    fn replay_webhook(
        &self,
        ctx: &ExecutionContext,
        input: ReplayWebhookInput,
    ) -> Result<Value, IntegrationModuleError> {
        let inbox_id = WebhookInboxId::new(input.inbox_id)?;
        self.webhook_admin.replay_webhook(
            Self::tenant_id(ctx)?,
            &inbox_id,
            Self::actor_ref(ctx),
            &input.reason,
        )?;
        json(&serde_json::json!({
            "inboxId": inbox_id.as_str(),
            "replayed": true,
        }))
    }

    fn plan_operation(
        &self,
        ctx: &ExecutionContext,
        input: PlanOperationInput,
    ) -> Result<Value, IntegrationModuleError> {
        let capability = CapabilityId::new(input.capability)?;
        let resolved = self.catalog.catalog()?.resolve(ctx, &capability)?;
        let mut operation = ExternalOperation::planned(
            ExternalOperationId::new(uuid::Uuid::new_v4().to_string())?,
            &resolved,
            input.operation_type,
            input.idempotency_key,
            input.request_hash,
        )?;
        operation.ready()?;
        let operation_id = self.operations.persist_operation(&operation)?;
        json(&serde_json::json!({
            "externalOperationId": operation_id.as_str(),
            "state": "ready",
            "providerReadiness": resolved.instance.readiness,
        }))
    }

    fn create_deposit(
        &self,
        ctx: &ExecutionContext,
        input: CreateDepositInput,
    ) -> Result<Value, IntegrationModuleError> {
        let deposit_id = self.finance.create_deposit(
            Self::tenant_id(ctx)?,
            &input.authority_kind,
            &input.authority_id,
            Money::new(input.amount_minor, input.currency)?,
        )?;
        json(&serde_json::json!({ "depositId": deposit_id.as_str(), "state": "expected" }))
    }

    fn request_refund(
        &self,
        ctx: &ExecutionContext,
        input: RequestRefundInput,
    ) -> Result<Value, IntegrationModuleError> {
        let refund_id = self.finance.request_refund(
            Self::tenant_id(ctx)?,
            &DepositId::new(input.deposit_id)?,
            Money::new(input.amount_minor, input.currency)?,
            &input.idempotency_key,
            &input.reason,
        )?;
        json(&serde_json::json!({ "refundId": refund_id.as_str(), "state": "requested" }))
    }

    fn record_deposit_received(
        &self,
        ctx: &ExecutionContext,
        input: RecordDepositReceiptInput,
    ) -> Result<Value, IntegrationModuleError> {
        let tenant_id = Self::tenant_id(ctx)?;
        let deposit_id = DepositId::new(input.deposit_id)?;
        self.finance.record_deposit_received(
            tenant_id,
            &deposit_id,
            Money::new(input.amount_minor, input.currency)?,
            Self::actor_ref(ctx),
        )?;
        json(&serde_json::json!({ "depositId": deposit_id.as_str(), "state": "recorded" }))
    }

    fn plan_refund_operation(
        &self,
        ctx: &ExecutionContext,
        input: PlanRefundOperationInput,
    ) -> Result<Value, IntegrationModuleError> {
        let tenant_id = Self::tenant_id(ctx)?;
        let refund_id = super::types::RefundId::new(input.refund_id)?;
        let capability = CapabilityId::new(input.capability)?;
        let resolved = self.catalog.catalog()?.resolve(ctx, &capability)?;
        let mut operation = ExternalOperation::planned(
            ExternalOperationId::new(uuid::Uuid::new_v4().to_string())?,
            &resolved,
            "refund",
            format!("refund:{}", refund_id.as_str()),
            input.request_hash,
        )?;
        operation.ready()?;
        let operation_id = self.operations.persist_operation(&operation)?;
        self.finance
            .link_refund_operation(tenant_id, &refund_id, &operation_id)?;
        json(&serde_json::json!({
            "refundId": refund_id.as_str(),
            "externalOperationId": operation_id.as_str(),
            "state": "approved",
        }))
    }

    fn reconcile_refund(
        &self,
        ctx: &ExecutionContext,
        input: ReconcileRefundInput,
    ) -> Result<Value, IntegrationModuleError> {
        let refund_id = super::types::RefundId::new(input.refund_id)?;
        self.finance.reconcile_refund(
            Self::tenant_id(ctx)?,
            &refund_id,
            input.outcome,
            &input.evidence_ref,
            Some(Self::actor_ref(ctx)),
        )?;
        json(&serde_json::json!({
            "refundId": refund_id.as_str(),
            "reconciled": true,
            "outcome": input.outcome,
        }))
    }
}

impl SystemModule for IntegrationModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "1.0.0".into(),
            description: "Tenant-scoped Provider Integration Fabric control plane".into(),
            author: "TALOS".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "register_manifest",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "list_manifests",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "inspect_binding",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "list_instances",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "list_bindings",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "upsert_instance",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "upsert_binding",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "create_webhook_endpoint",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "list_webhook_endpoints",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "set_webhook_endpoint_enabled",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "list_webhook_dead_letters",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "replay_webhook",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "plan_operation",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "create_deposit",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "request_refund",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "record_deposit_received",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "plan_refund_operation",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "reconcile_refund",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
        ]
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let result = match command {
            "register_manifest" => self.register_manifest(Self::parse(payload)?),
            "list_manifests" => self.list_manifests(),
            "inspect_binding" => self.inspect_binding(ctx, Self::parse(payload)?),
            "list_instances" => self.list_instances(ctx),
            "list_bindings" => self.list_bindings(ctx),
            "upsert_instance" => self.upsert_instance(ctx, Self::parse(payload)?),
            "upsert_binding" => self.upsert_binding(ctx, Self::parse(payload)?),
            "create_webhook_endpoint" => self.create_webhook_endpoint(ctx, Self::parse(payload)?),
            "list_webhook_endpoints" => self.list_webhook_endpoints(ctx),
            "set_webhook_endpoint_enabled" => {
                self.set_webhook_endpoint_enabled(ctx, Self::parse(payload)?)
            }
            "list_webhook_dead_letters" => self.list_webhook_dead_letters(ctx),
            "replay_webhook" => self.replay_webhook(ctx, Self::parse(payload)?),
            "plan_operation" => self.plan_operation(ctx, Self::parse(payload)?),
            "create_deposit" => self.create_deposit(ctx, Self::parse(payload)?),
            "request_refund" => self.request_refund(ctx, Self::parse(payload)?),
            "record_deposit_received" => self.record_deposit_received(ctx, Self::parse(payload)?),
            "plan_refund_operation" => self.plan_refund_operation(ctx, Self::parse(payload)?),
            "reconcile_refund" => self.reconcile_refund(ctx, Self::parse(payload)?),
            other => Err(IntegrationModuleError::UnknownCommand(other.into())),
        };
        result.map_err(IntegrationModuleError::into_json)
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: MODULE_NAME.into(),
            description: "Provider-neutral Integration Fabric commands".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|command| CommandSchema {
                    name: command.name.to_owned(),
                    description: "Integration Fabric command".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                })
                .collect(),
        }
    }
}

fn unique_ids(
    values: Vec<String>,
    label: &str,
) -> Result<BTreeSet<CapabilityId>, IntegrationModuleError> {
    let mut result = BTreeSet::new();
    for value in values {
        let id = CapabilityId::new(value)?;
        if !result.insert(id) {
            return Err(IntegrationModuleError::InvalidInput(format!(
                "duplicate {label}"
            )));
        }
    }
    Ok(result)
}

fn json<T: Serialize>(value: &T) -> Result<Value, IntegrationModuleError> {
    serde_json::to_value(value)
        .map_err(|error| IntegrationModuleError::Serialize(error.to_string()))
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use secrecy::Secret;
    use serde_json::Value;
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode,
        NoopHttpClient, PlatformMembershipId, PlatformRole, RequestId, Revision, TenantId,
        TenantMembershipId, TenantRole, TenantScope,
    };

    use crate::db::migrations::run_migrations;
    use crate::integration::keystore::{
        InMemoryKeyStore, SecretEnvironment, SecretRegistrationScope,
    };
    use crate::integration::store::IntegrationStore;
    use crate::integration::types::{ProviderInstanceId, SecretRef};
    use crate::registry::ModuleRegistry;

    fn registry() -> (ModuleRegistry, Pool<SqliteConnectionManager>) {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO tenants (id, name, slug, status, plan, created_at, updated_at)
             VALUES ('tenant-a', 'Tenant A', 'tenant-a', 'active', 'free', 'now', 'now');
             INSERT INTO identities (id, username, password_hash, display_name, status, created_at, updated_at)
             VALUES
               ('platform-owner', 'platform-owner', 'hash', 'Platform Owner', 'active', 'now', 'now'),
               ('tenant-admin', 'tenant-admin', 'hash', 'Tenant Admin', 'active', 'now', 'now');
             INSERT INTO platform_memberships (id, identity_id, status, created_at, updated_at)
             VALUES ('platform-membership', 'platform-owner', 'active', 'now', 'now');
             INSERT INTO platform_role_grants (id, platform_membership_id, role, granted_by_identity_id, granted_at)
             VALUES ('platform-owner-grant', 'platform-membership', 'platform_owner', 'platform-owner', 'now');
             INSERT INTO tenant_memberships (id, identity_id, tenant_id, role, status, created_at, updated_at)
             VALUES ('tenant-membership', 'tenant-admin', 'tenant-a', 'admin', 'active', 'now', 'now');",
        )
        .unwrap();
        drop(conn);
        let key_store = Arc::new(InMemoryKeyStore::default());
        key_store.insert(
            SecretRef::new("keystore://fixture/payment-a").unwrap(),
            SecretEnvironment::Fixture,
            BTreeSet::from([SecretRegistrationScope::new(
                "tenant-a",
                ProviderInstanceId::new("instance-a").unwrap(),
            )
            .unwrap()]),
            Secret::new(b"fixture-only-secret".to_vec()),
        );
        let store = IntegrationStore::with_key_store(pool.clone(), key_store);
        (
            ModuleRegistry::assemble(pool.clone(), Arc::new(NoopHttpClient), store).unwrap(),
            pool,
        )
    }

    fn platform_context() -> ExecutionContext {
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "platform-owner",
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::platform(),
            DataScope::platform(Revision::new("revision-a").unwrap()),
            ExecutionMode::Normal,
            RequestId::new("request-platform").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn tenant_context(mode: ExecutionMode) -> ExecutionContext {
        let tenant = TenantId::new("tenant-a").unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "tenant-admin",
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new("tenant-membership").unwrap(),
                    tenant_id: tenant.clone(),
                    role: TenantRole::Admin,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("revision-a").unwrap()).unwrap(),
            mode,
            RequestId::new("request-tenant").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn preview_context() -> ExecutionContext {
        let tenant = TenantId::new("tenant-a").unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "platform-owner",
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("revision-a").unwrap()).unwrap(),
            ExecutionMode::ReadOnlyPreview(
                system_core::PreviewSessionId::new("preview-a").unwrap(),
            ),
            RequestId::new("request-preview").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn manifest() -> Value {
        serde_json::json!({
            "providerId": "fixture-payment",
            "version": "1.0.0",
            "capabilities": ["payment.refund"],
            "configSchema": [{"name":"endpoint","required":true,"valueType":"https_origin"}],
            "secretSchema": [{"name":"api_key","required":true,"valueType":"opaque_credential","purpose":"provider_authentication"}],
            "apiVersions": {"payment.refund": "v1"},
            "webhookTypes": ["refund.updated"],
            "simulationCapabilities": ["payment.refund"],
            "readiness": "fixture",
            "compatibility": {"payment.refund":"backward_compatible"}
        })
    }

    #[test]
    fn registry_governs_tenant_binding_and_never_returns_secret_refs() {
        let (registry, _) = registry();
        registry
            .execute(
                "integration",
                "register_manifest",
                manifest(),
                &platform_context(),
            )
            .unwrap();
        let tenant = tenant_context(ExecutionMode::Normal);
        registry
            .execute(
                "integration",
                "upsert_instance",
                serde_json::json!({
                    "id": "instance-a", "providerId": "fixture-payment", "manifestVersion": "1.0.0",
                    "configRevision": "revision-1", "config": {"endpoint":"https://fixture.invalid"},
                    "secretRefs": {"api_key":"keystore://fixture/payment-a"}, "lifecycle":"active", "health":"ready", "readiness":"fixture"
                }),
                &tenant,
            )
            .unwrap();
        registry
            .execute(
                "integration",
                "upsert_binding",
                serde_json::json!({"id":"binding-a","providerInstanceId":"instance-a","capability":"payment.refund","configRevision":"revision-1","enabled":true}),
                &tenant,
            )
            .unwrap();
        let created_endpoint = registry
            .execute(
                "integration",
                "create_webhook_endpoint",
                serde_json::json!({"bindingId":"binding-a"}),
                &tenant,
            )
            .unwrap();
        let endpoint_id = created_endpoint["webhookEndpointId"].as_str().unwrap();
        let endpoint_token = created_endpoint["endpointToken"].as_str().unwrap();
        assert!(!endpoint_token.is_empty());
        let endpoints = registry
            .execute(
                "integration",
                "list_webhook_endpoints",
                serde_json::json!({}),
                &tenant,
            )
            .unwrap();
        assert_eq!(endpoints["webhookEndpoints"].as_array().unwrap().len(), 1);
        assert!(!endpoints.to_string().contains(endpoint_token));
        registry
            .execute(
                "integration",
                "set_webhook_endpoint_enabled",
                serde_json::json!({"endpointId":endpoint_id,"enabled":false}),
                &tenant,
            )
            .unwrap();
        let inspection = registry
            .execute(
                "integration",
                "inspect_binding",
                serde_json::json!({"capability":"payment.refund"}),
                &tenant,
            )
            .unwrap();
        assert!(
            !inspection
                .to_string()
                .contains("keystore://fixture/payment-a")
        );
        assert_eq!(inspection["providerBindingId"], "binding-a");
        let instances = registry
            .execute(
                "integration",
                "list_instances",
                serde_json::json!({}),
                &tenant,
            )
            .unwrap();
        assert_eq!(instances["instances"].as_array().unwrap().len(), 1);
        assert!(
            instances["instances"][0]["configuredSecretNames"]
                .as_array()
                .unwrap()
                .iter()
                .any(|name| name == "api_key")
        );
        assert!(
            !instances
                .to_string()
                .contains("keystore://fixture/payment-a")
        );
        let bindings = registry
            .execute(
                "integration",
                "list_bindings",
                serde_json::json!({}),
                &tenant,
            )
            .unwrap();
        assert_eq!(bindings["bindings"].as_array().unwrap().len(), 1);
        let planned = registry
            .execute(
                "integration",
                "plan_operation",
                serde_json::json!({"capability":"payment.refund","operationType":"refund","idempotencyKey":"refund-a","requestHash":"hash-a"}),
                &tenant,
            )
            .unwrap();
        assert_eq!(planned["state"], "ready");
    }

    #[test]
    fn invalid_manifest_and_preview_write_are_rejected_before_persistence() {
        let (registry, pool) = registry();
        let mut invalid = manifest();
        invalid["capabilities"] = serde_json::json!(["payment.refund", "payment.refund"]);
        assert!(
            registry
                .execute(
                    "integration",
                    "register_manifest",
                    invalid,
                    &platform_context()
                )
                .is_err()
        );
        let manifest_count: i64 = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM provider_manifests WHERE provider_id = 'fixture-payment'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(manifest_count, 0);

        registry
            .execute(
                "integration",
                "register_manifest",
                manifest(),
                &platform_context(),
            )
            .unwrap();
        let missing_secret = registry
            .execute(
                "integration",
                "upsert_instance",
                serde_json::json!({
                    "id": "missing-secret", "providerId": "fixture-payment", "manifestVersion": "1.0.0",
                    "configRevision": "revision-1", "config": {"endpoint":"https://fixture.invalid"},
                    "secretRefs": {}, "lifecycle":"active", "health":"ready", "readiness":"fixture"
                }),
                &tenant_context(ExecutionMode::Normal),
            )
            .unwrap_err();
        assert!(missing_secret.contains("required secret reference is missing"));
        let plaintext_config = registry
            .execute(
                "integration",
                "upsert_instance",
                serde_json::json!({
                    "id": "plaintext-config", "providerId": "fixture-payment", "manifestVersion": "1.0.0",
                    "configRevision": "revision-1", "config": {"endpoint":"https://fixture.invalid", "api_key":"not-a-secret-ref"},
                    "secretRefs": {"api_key":"keystore://fixture/payment-a"}, "lifecycle":"active", "health":"ready", "readiness":"fixture"
                }),
                &tenant_context(ExecutionMode::Normal),
        )
        .unwrap_err();
        assert!(plaintext_config.contains("VAL_INTEGRATION_INPUT"));
        assert!(!plaintext_config.contains("not-a-secret-ref"));
        let persisted_plaintext_config: i64 = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM provider_instances WHERE config_json LIKE ?1",
                ["%not-a-secret-ref%"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_plaintext_config, 0);

        let raw_secret = "not-a-keystore-reference";
        let raw_secret_error = registry
            .execute(
                "integration",
                "upsert_instance",
                serde_json::json!({
                    "id": "raw-secret", "providerId": "fixture-payment", "manifestVersion": "1.0.0",
                    "configRevision": "revision-1", "config": {"endpoint":"https://fixture.invalid"},
                    "secretRefs": {"api_key": raw_secret}, "lifecycle":"active", "health":"ready", "readiness":"fixture"
                }),
                &tenant_context(ExecutionMode::Normal),
            )
            .unwrap_err();
        assert!(raw_secret_error.contains("secret reference must be a KeyStore URI"));
        assert!(!raw_secret_error.contains(raw_secret));
        let persisted_raw_secret: i64 = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM provider_instances WHERE secret_refs_json LIKE ?1",
                [format!("%{raw_secret}%")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(persisted_raw_secret, 0);

        let unregistered_reference = registry
            .execute(
                "integration",
                "upsert_instance",
                serde_json::json!({
                    "id": "unregistered-reference", "providerId": "fixture-payment", "manifestVersion": "1.0.0",
                    "configRevision": "revision-1", "config": {"endpoint":"https://fixture.invalid"},
                    "secretRefs": {"api_key": "keystore://fixture/not-registered"}, "lifecycle":"active", "health":"ready", "readiness":"fixture"
                }),
                &tenant_context(ExecutionMode::Normal),
            )
            .unwrap_err();
        assert!(unregistered_reference.contains("secret reference is unavailable"));

        let error = registry
            .execute(
                "integration",
                "create_deposit",
                serde_json::json!({"authorityKind":"order","authorityId":"order-a","amountMinor":100,"currency":"CNY"}),
                &preview_context(),
            )
            .unwrap_err();
        assert!(error.contains("PREVIEW_WRITE_BLOCKED"));
    }

    #[tokio::test]
    async fn r3_simulation_effects_never_enter_production_queue_or_credentials() {
        use crate::integration::keystore::{
            KeyStore, SecretAccessAudit, SecretAccessScope, SecretMetadata,
        };
        use crate::integration::runtime::{
            ClaimedOperation, DispatchResult, OperationDispatcher, OperationRuntime,
            OperationRuntimePolicy, RunOnceResult,
        };
        use crate::integration::transport::TransportFailure;
        use crate::integration::types::IntegrationError;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use system_core::{Namespace, SimulationId};
        struct TrapKeyStore(Arc<AtomicUsize>);
        impl KeyStore for TrapKeyStore {
            fn resolve(
                &self,
                _: &SecretRef,
                _: &SecretAccessScope,
            ) -> Result<Secret<Vec<u8>>, IntegrationError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(IntegrationError::SecretAccessDenied)
            }
            fn metadata(&self, _: &SecretRef) -> Option<SecretMetadata> {
                self.0.fetch_add(1, Ordering::SeqCst);
                None
            }
            fn access_audit(&self) -> Vec<SecretAccessAudit> {
                vec![]
            }
        }
        struct TrapDispatcher(Arc<AtomicUsize>);
        #[async_trait::async_trait]
        impl OperationDispatcher for TrapDispatcher {
            async fn dispatch(
                &self,
                _: &ClaimedOperation,
            ) -> Result<DispatchResult, TransportFailure> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(DispatchResult::Succeeded {
                    provider_result_ref: None,
                })
            }
        }
        let (_, pool) = registry();
        pool.get().unwrap().execute("INSERT INTO pricing_configs (baseWeekdayPrice,baseWeekendPrice,holidayRulesJson,createdAt,updatedAt,tenant_id)
            VALUES (8.5,14,'[]','now','now','tenant-a')",[]).unwrap();
        let secrets = Arc::new(AtomicUsize::new(0));
        let dispatches = Arc::new(AtomicUsize::new(0));
        let store =
            IntegrationStore::with_key_store(pool.clone(), Arc::new(TrapKeyStore(secrets.clone())));
        let registry =
            ModuleRegistry::assemble(pool.clone(), Arc::new(NoopHttpClient), store.clone())
                .unwrap();
        let session=registry.execute("tenant_simulation","session.create",serde_json::json!({
            "tenantId":"tenant-a","scenarioName":"R3 isolation","changeIntent":"internal fixture proof",
            "ttlMinutes":30,"plannedAbsentKeys":["r3-proof"],"idempotencyKey":"r3-create-123456789"
        }),&platform_context()).unwrap();
        let id = session["id"].as_str().unwrap();
        let simulation = SimulationId::new(id).unwrap();
        let tenant = TenantId::new("tenant-a").unwrap();
        let revision = session["baseRevision"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| session["baseRevision"].to_string());
        let ctx = ExecutionContext::new(
            platform_context().actor().clone(),
            TenantScope::tenant(tenant.clone()),
            DataScope::new(
                tenant,
                Namespace::Simulation(simulation.clone()),
                Revision::new(revision).unwrap(),
            )
            .unwrap(),
            ExecutionMode::Simulation(simulation),
            RequestId::new("r3-simulation-proof").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap();
        let payload = serde_json::json!({"id":id,"key":"r3-proof",
            "value":{"enabled":true,"threshold":1,"label":"fixture"},
            "effect":{"kind":"reference","note":"fixture-only, no provider"},
            "idempotencyKey":"r3-effect-123456789"});
        let first = registry
            .execute(
                "tenant_simulation",
                "reference_config.record_effect",
                payload.clone(),
                &ctx,
            )
            .unwrap();
        let replay = registry
            .execute(
                "tenant_simulation",
                "reference_config.record_effect",
                payload,
                &ctx,
            )
            .unwrap();
        assert_eq!(first, replay);
        // Every integration command remains unavailable even to a platform
        // owner when executing inside Simulation. The gate precedes payload
        // parsing, secret lookup and effect admission.
        for command in registry.get("integration").unwrap().commands() {
            let error = registry
                .execute(
                    "integration",
                    command.name,
                    serde_json::json!({
                        "secretRefs":{"api_key":"keystore://deployment/production"},
                        "operationType":"payment","idempotencyKey":"forged-simulation"
                    }),
                    &ctx,
                )
                .unwrap_err();
            assert!(error.contains("EXEC_SIMULATION_UNSUPPORTED"), "{error}");
        }
        let conn = pool.get().unwrap();
        let production:i64=conn.query_row("SELECT count(*) FROM simulation_reference_configs WHERE tenant_id='tenant-a' AND key='r3-proof'",[],|r|r.get(0)).unwrap();
        assert_eq!(production, 0);
        let operations: i64 = conn
            .query_row("SELECT count(*) FROM external_operations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(operations, 0);
        drop(conn);
        let runtime = OperationRuntime::new(store, OperationRuntimePolicy::default()).unwrap();
        assert_eq!(
            runtime
                .run_once("tenant-a", &TrapDispatcher(dispatches.clone()))
                .await
                .unwrap(),
            RunOnceResult::Idle
        );
        assert_eq!(secrets.load(Ordering::SeqCst), 0);
        assert_eq!(dispatches.load(Ordering::SeqCst), 0);
        // Preview cannot make even a simulated effect record.
        assert!(
            registry
                .execute(
                    "tenant_simulation",
                    "reference_config.record_effect",
                    serde_json::json!({}),
                    &preview_context()
                )
                .unwrap_err()
                .contains("EXEC_PREVIEW_WRITE_BLOCKED")
        );
    }
}

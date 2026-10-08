use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use super::catalog::ProviderCatalog;
use super::catalog_repository::{
    enum_health, enum_lifecycle, enum_readiness, health_value, lifecycle_value, readiness_value,
};
use super::types::{
    CapabilityId, IntegrationError, ProviderBinding, ProviderBindingId, ProviderId,
    ProviderInstance, ProviderInstanceId, ProviderManifest,
};

#[derive(Clone)]
pub(crate) struct SqliteIntegrationCatalogRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteIntegrationCatalogRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn save_manifest(
        &self,
        manifest: &ProviderManifest,
    ) -> Result<(), IntegrationError> {
        manifest.validate()?;
        let mut connection = self.pool.get().map_err(persistence)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT version FROM provider_manifests
                 WHERE provider_id = ?1 AND version = ?2",
                params![manifest.provider_id.as_str(), manifest.version],
                |row| row.get(0),
            )
            .optional()
            .map_err(persistence)?;
        if existing.is_some() {
            transaction.commit().map_err(persistence)?;
            return Err(IntegrationError::InvalidManifest(
                "provider manifest versions are immutable; register a new version".into(),
            ));
        }

        transaction
            .execute(
                "INSERT INTO provider_manifests
                 (provider_id, version, capabilities_json, config_schema_json,
                  secret_schema_json, api_versions_json, webhook_types_json,
                  simulation_capabilities_json, readiness, compatibility_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    manifest.provider_id.as_str(),
                    manifest.version,
                    serde_json::to_string(&manifest.capabilities).map_err(persistence)?,
                    serde_json::to_string(&manifest.config_schema).map_err(persistence)?,
                    serde_json::to_string(&manifest.secret_schema).map_err(persistence)?,
                    serde_json::to_string(&manifest.api_versions).map_err(persistence)?,
                    serde_json::to_string(&manifest.webhook_types).map_err(persistence)?,
                    serde_json::to_string(&manifest.simulation_capabilities)
                        .map_err(persistence)?,
                    readiness_value(&manifest.readiness),
                    serde_json::to_string(&manifest.compatibility).map_err(persistence)?,
                    now(),
                ],
            )
            .map_err(persistence)?;
        transaction.commit().map_err(persistence)?;
        Ok(())
    }

    pub(crate) fn save_instance(
        &self,
        instance: &ProviderInstance,
    ) -> Result<(), IntegrationError> {
        instance.validate()?;
        let connection = self.pool.get().map_err(persistence)?;
        connection
            .execute(
                "INSERT INTO provider_instances
                 (id, tenant_id, provider_id, manifest_version, config_revision,
                  config_json, secret_refs_json, lifecycle, readiness, health,
                  created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
                 ON CONFLICT(tenant_id, id) DO UPDATE SET
                   provider_id = excluded.provider_id,
                   manifest_version = excluded.manifest_version,
                   config_revision = excluded.config_revision,
                   config_json = excluded.config_json,
                   secret_refs_json = excluded.secret_refs_json,
                   lifecycle = excluded.lifecycle,
                   readiness = excluded.readiness,
                   health = excluded.health,
                   updated_at = excluded.updated_at",
                params![
                    instance.id.as_str(),
                    instance.tenant_id,
                    instance.provider_id.as_str(),
                    instance.manifest_version,
                    instance.config_revision,
                    serde_json::to_string(&instance.config).map_err(persistence)?,
                    serde_json::to_string(&instance.secret_refs).map_err(persistence)?,
                    lifecycle_value(&instance.lifecycle),
                    readiness_value(&instance.readiness),
                    health_value(&instance.health),
                    now(),
                ],
            )
            .map_err(persistence)?;
        Ok(())
    }

    pub(crate) fn save_binding(
        &self,
        binding: &ProviderBinding,
        actor_ref: &str,
    ) -> Result<(), IntegrationError> {
        binding.validate()?;
        let mut connection = self.pool.get().map_err(persistence)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;

        let instance: Option<(String, String, String)> = transaction
            .query_row(
                "SELECT provider_id, manifest_version, config_revision
                 FROM provider_instances
                 WHERE tenant_id = ?1 AND id = ?2",
                params![binding.tenant_id, binding.provider_instance_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(persistence)?;
        let Some((provider_id, manifest_version, current_revision)) = instance else {
            return Err(IntegrationError::BindingUnavailable);
        };
        if current_revision != binding.config_revision {
            return Err(IntegrationError::BindingRevisionStale);
        }

        let capabilities_json: Option<String> = transaction
            .query_row(
                "SELECT capabilities_json
                 FROM provider_manifests
                 WHERE provider_id = ?1 AND version = ?2",
                params![provider_id, manifest_version],
                |row| row.get(0),
            )
            .optional()
            .map_err(persistence)?;
        let capabilities = capabilities_json
            .ok_or(IntegrationError::BindingUnavailable)
            .and_then(|json| {
                serde_json::from_str::<std::collections::BTreeSet<CapabilityId>>(&json)
                    .map_err(persistence)
            })?;
        if !capabilities.contains(&binding.capability) {
            return Err(IntegrationError::CapabilityNotDeclared);
        }

        let timestamp = now();
        transaction
            .execute(
                "INSERT INTO provider_bindings
                 (id, tenant_id, provider_instance_id, capability_id, config_revision,
                  enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(tenant_id, id) DO UPDATE SET
                   provider_instance_id = excluded.provider_instance_id,
                   capability_id = excluded.capability_id,
                   config_revision = excluded.config_revision,
                   enabled = excluded.enabled,
                   updated_at = excluded.updated_at",
                params![
                    binding.id.as_str(),
                    binding.tenant_id,
                    binding.provider_instance_id.as_str(),
                    binding.capability.as_str(),
                    binding.config_revision,
                    binding.enabled,
                    timestamp,
                ],
            )
            .map_err(persistence)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO provider_binding_history
                 (id, tenant_id, binding_id, revision, action, occurred_at, actor_ref)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    binding.tenant_id,
                    binding.id.as_str(),
                    binding.config_revision,
                    if binding.enabled {
                        "updated"
                    } else {
                        "disabled"
                    },
                    timestamp,
                    actor_ref,
                ],
            )
            .map_err(persistence)?;
        transaction.commit().map_err(persistence)
    }

    pub(crate) fn catalog(&self) -> Result<ProviderCatalog, IntegrationError> {
        let connection = self.pool.get().map_err(persistence)?;
        let mut catalog = ProviderCatalog::default();

        let mut manifests = connection
            .prepare(
                "SELECT provider_id, version, capabilities_json, config_schema_json,
                        secret_schema_json, api_versions_json, webhook_types_json,
                        simulation_capabilities_json, readiness, compatibility_json
                 FROM provider_manifests",
            )
            .map_err(persistence)?;
        let manifest_rows = manifests
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })
            .map_err(persistence)?;
        for row in manifest_rows {
            let (
                provider_id,
                version,
                capabilities,
                config_schema,
                secret_schema,
                api_versions,
                webhook_types,
                simulation_capabilities,
                readiness,
                compatibility,
            ) = row.map_err(persistence)?;
            catalog.register_manifest(ProviderManifest {
                provider_id: ProviderId::new(provider_id)?,
                version,
                capabilities: serde_json::from_str(&capabilities).map_err(persistence)?,
                config_schema: serde_json::from_str(&config_schema).map_err(persistence)?,
                secret_schema: serde_json::from_str(&secret_schema).map_err(persistence)?,
                api_versions: serde_json::from_str(&api_versions).map_err(persistence)?,
                webhook_types: serde_json::from_str(&webhook_types).map_err(persistence)?,
                simulation_capabilities: serde_json::from_str(&simulation_capabilities)
                    .map_err(persistence)?,
                readiness: enum_readiness(&readiness)?,
                compatibility: serde_json::from_str(&compatibility).map_err(persistence)?,
            })?;
        }

        let mut instances = connection
            .prepare(
                "SELECT id, tenant_id, provider_id, manifest_version, config_revision,
                        config_json, secret_refs_json, lifecycle, readiness, health
                 FROM provider_instances",
            )
            .map_err(persistence)?;
        let instance_rows = instances
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })
            .map_err(persistence)?;
        for row in instance_rows {
            let (
                id,
                tenant_id,
                provider_id,
                manifest_version,
                config_revision,
                config,
                secret_refs,
                lifecycle,
                readiness,
                health,
            ) = row.map_err(persistence)?;
            catalog.register_instance(ProviderInstance {
                id: ProviderInstanceId::new(id)?,
                tenant_id,
                provider_id: ProviderId::new(provider_id)?,
                manifest_version,
                config_revision,
                config: serde_json::from_str(&config).map_err(persistence)?,
                secret_refs: serde_json::from_str(&secret_refs).map_err(persistence)?,
                lifecycle: enum_lifecycle(&lifecycle)?,
                readiness: enum_readiness(&readiness)?,
                health: enum_health(&health)?,
            })?;
        }

        let mut bindings = connection
            .prepare(
                "SELECT id, tenant_id, provider_instance_id, capability_id,
                        config_revision, enabled
                 FROM provider_bindings",
            )
            .map_err(persistence)?;
        let binding_rows = bindings
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                ))
            })
            .map_err(persistence)?;
        for row in binding_rows {
            let (id, tenant_id, provider_instance_id, capability, config_revision, enabled) =
                row.map_err(persistence)?;
            catalog.register_binding(ProviderBinding {
                id: ProviderBindingId::new(id)?,
                tenant_id,
                provider_instance_id: ProviderInstanceId::new(provider_instance_id)?,
                capability: CapabilityId::new(capability)?,
                config_revision,
                enabled,
            })?;
        }

        Ok(catalog)
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn persistence<T>(_error: T) -> IntegrationError {
    IntegrationError::Persistence
}

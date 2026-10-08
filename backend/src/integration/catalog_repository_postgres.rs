use std::future::Future;

use sqlx::postgres::PgPool;
use sqlx::{Executor, Row};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::catalog::ProviderCatalog;
use super::catalog_repository::{
    enum_health, enum_lifecycle, enum_readiness, health_value, lifecycle_value, readiness_value,
};
use super::types::{
    CapabilityId, IntegrationError, ProviderBinding, ProviderBindingId, ProviderId,
    ProviderInstance, ProviderInstanceId, ProviderManifest,
};

#[derive(Clone)]
pub(crate) struct PostgresIntegrationCatalogRepository {
    pool: PgPool,
}

impl PostgresIntegrationCatalogRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn save_manifest(
        &self,
        manifest: &ProviderManifest,
    ) -> Result<(), IntegrationError> {
        manifest.validate()?;
        let pool = self.pool.clone();
        let manifest = manifest.clone();
        run_pg_catalog(async move {
            let affected = sqlx::query(
                "INSERT INTO provider_manifests
                 (provider_id, version, capabilities_json, config_schema_json,
                  secret_schema_json, api_versions_json, webhook_types_json,
                  simulation_capabilities_json, readiness, compatibility_json, created_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
                 ON CONFLICT (provider_id, version) DO NOTHING",
            )
            .bind(manifest.provider_id.as_str())
            .bind(&manifest.version)
            .bind(serde_json::to_string(&manifest.capabilities).map_err(persistence)?)
            .bind(serde_json::to_string(&manifest.config_schema).map_err(persistence)?)
            .bind(serde_json::to_string(&manifest.secret_schema).map_err(persistence)?)
            .bind(serde_json::to_string(&manifest.api_versions).map_err(persistence)?)
            .bind(serde_json::to_string(&manifest.webhook_types).map_err(persistence)?)
            .bind(serde_json::to_string(&manifest.simulation_capabilities).map_err(persistence)?)
            .bind(readiness_value(&manifest.readiness))
            .bind(serde_json::to_string(&manifest.compatibility).map_err(persistence)?)
            .bind(now())
            .execute(&pool)
            .await
            .map_err(persistence)?
            .rows_affected();

            if affected != 1 {
                return Err(IntegrationError::InvalidManifest(
                    "provider manifest versions are immutable; register a new version".into(),
                ));
            }
            Ok(())
        })
    }

    pub(crate) fn save_instance(
        &self,
        instance: &ProviderInstance,
    ) -> Result<(), IntegrationError> {
        instance.validate()?;
        let pool = self.pool.clone();
        let instance = instance.clone();
        run_pg_catalog(async move {
            sqlx::query(
                "INSERT INTO provider_instances
                 (id,tenant_id,provider_id,manifest_version,config_revision,
                  config_json,secret_refs_json,lifecycle,readiness,health,
                  created_at,updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$11)
                 ON CONFLICT (tenant_id,id) DO UPDATE SET
                   provider_id=EXCLUDED.provider_id,
                   manifest_version=EXCLUDED.manifest_version,
                   config_revision=EXCLUDED.config_revision,
                   config_json=EXCLUDED.config_json,
                   secret_refs_json=EXCLUDED.secret_refs_json,
                   lifecycle=EXCLUDED.lifecycle,
                   readiness=EXCLUDED.readiness,
                   health=EXCLUDED.health,
                   updated_at=EXCLUDED.updated_at",
            )
            .bind(instance.id.as_str())
            .bind(&instance.tenant_id)
            .bind(instance.provider_id.as_str())
            .bind(&instance.manifest_version)
            .bind(&instance.config_revision)
            .bind(serde_json::to_string(&instance.config).map_err(persistence)?)
            .bind(serde_json::to_string(&instance.secret_refs).map_err(persistence)?)
            .bind(lifecycle_value(&instance.lifecycle))
            .bind(readiness_value(&instance.readiness))
            .bind(health_value(&instance.health))
            .bind(now())
            .execute(&pool)
            .await
            .map_err(persistence)?;
            Ok(())
        })
    }

    pub(crate) fn save_binding(
        &self,
        binding: &ProviderBinding,
        actor_ref: &str,
    ) -> Result<(), IntegrationError> {
        binding.validate()?;
        let pool = self.pool.clone();
        let binding = binding.clone();
        let actor_ref = actor_ref.to_owned();

        run_pg_catalog(async move {
            let mut transaction = pool.begin().await.map_err(persistence)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;

            let instance = sqlx::query(
                "SELECT provider_id,manifest_version,config_revision
                 FROM provider_instances
                 WHERE tenant_id=$1 AND id=$2
                 FOR UPDATE",
            )
            .bind(&binding.tenant_id)
            .bind(binding.provider_instance_id.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?;

            let Some(instance) = instance else {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::BindingUnavailable);
            };
            let provider_id: String = instance.try_get(0).map_err(persistence)?;
            let manifest_version: String = instance.try_get(1).map_err(persistence)?;
            let current_revision: String = instance.try_get(2).map_err(persistence)?;
            if current_revision != binding.config_revision {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::BindingRevisionStale);
            }

            let capabilities_json = sqlx::query_scalar::<_, String>(
                "SELECT capabilities_json
                 FROM provider_manifests
                 WHERE provider_id=$1 AND version=$2",
            )
            .bind(&provider_id)
            .bind(&manifest_version)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?
            .ok_or(IntegrationError::BindingUnavailable)?;
            let capabilities = serde_json::from_str::<std::collections::BTreeSet<CapabilityId>>(
                &capabilities_json,
            )
            .map_err(persistence)?;
            if !capabilities.contains(&binding.capability) {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::CapabilityNotDeclared);
            }

            let timestamp = now();
            sqlx::query(
                "INSERT INTO provider_bindings
                 (id,tenant_id,provider_instance_id,capability_id,config_revision,
                  enabled,created_at,updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$7)
                 ON CONFLICT (tenant_id,id) DO UPDATE SET
                   provider_instance_id=EXCLUDED.provider_instance_id,
                   capability_id=EXCLUDED.capability_id,
                   config_revision=EXCLUDED.config_revision,
                   enabled=EXCLUDED.enabled,
                   updated_at=EXCLUDED.updated_at",
            )
            .bind(binding.id.as_str())
            .bind(&binding.tenant_id)
            .bind(binding.provider_instance_id.as_str())
            .bind(binding.capability.as_str())
            .bind(&binding.config_revision)
            .bind(binding.enabled)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            sqlx::query(
                "INSERT INTO provider_binding_history
                 (id,tenant_id,binding_id,revision,action,occurred_at,actor_ref)
                 VALUES ($1,$2,$3,$4,$5,$6,$7)
                 ON CONFLICT (tenant_id,binding_id,revision,action) DO NOTHING",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&binding.tenant_id)
            .bind(binding.id.as_str())
            .bind(&binding.config_revision)
            .bind(if binding.enabled {
                "updated"
            } else {
                "disabled"
            })
            .bind(&timestamp)
            .bind(actor_ref)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)?;
            Ok(())
        })
    }

    pub(crate) fn catalog(&self) -> Result<ProviderCatalog, IntegrationError> {
        let pool = self.pool.clone();
        run_pg_catalog(async move {
            let mut transaction = pool.begin().await.map_err(persistence)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY")
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;

            let mut catalog = ProviderCatalog::default();

            let manifest_rows = sqlx::query(
                "SELECT provider_id,version,capabilities_json,config_schema_json,
                        secret_schema_json,api_versions_json,webhook_types_json,
                        simulation_capabilities_json,readiness,compatibility_json
                 FROM provider_manifests
                 ORDER BY provider_id,version",
            )
            .fetch_all(&mut *transaction)
            .await
            .map_err(persistence)?;
            for row in manifest_rows {
                catalog.register_manifest(ProviderManifest {
                    provider_id: ProviderId::new(
                        row.try_get::<String, _>(0).map_err(persistence)?,
                    )?,
                    version: row.try_get(1).map_err(persistence)?,
                    capabilities: serde_json::from_str(
                        &row.try_get::<String, _>(2).map_err(persistence)?,
                    )
                    .map_err(persistence)?,
                    config_schema: serde_json::from_str(
                        &row.try_get::<String, _>(3).map_err(persistence)?,
                    )
                    .map_err(persistence)?,
                    secret_schema: serde_json::from_str(
                        &row.try_get::<String, _>(4).map_err(persistence)?,
                    )
                    .map_err(persistence)?,
                    api_versions: serde_json::from_str(
                        &row.try_get::<String, _>(5).map_err(persistence)?,
                    )
                    .map_err(persistence)?,
                    webhook_types: serde_json::from_str(
                        &row.try_get::<String, _>(6).map_err(persistence)?,
                    )
                    .map_err(persistence)?,
                    simulation_capabilities: serde_json::from_str(
                        &row.try_get::<String, _>(7).map_err(persistence)?,
                    )
                    .map_err(persistence)?,
                    readiness: enum_readiness(&row.try_get::<String, _>(8).map_err(persistence)?)?,
                    compatibility: serde_json::from_str(
                        &row.try_get::<String, _>(9).map_err(persistence)?,
                    )
                    .map_err(persistence)?,
                })?;
            }

            let instance_rows = sqlx::query(
                "SELECT id,tenant_id,provider_id,manifest_version,config_revision,
                        config_json,secret_refs_json,lifecycle,readiness,health
                 FROM provider_instances
                 ORDER BY tenant_id,id",
            )
            .fetch_all(&mut *transaction)
            .await
            .map_err(persistence)?;
            for row in instance_rows {
                catalog.register_instance(ProviderInstance {
                    id: ProviderInstanceId::new(row.try_get::<String, _>(0).map_err(persistence)?)?,
                    tenant_id: row.try_get(1).map_err(persistence)?,
                    provider_id: ProviderId::new(
                        row.try_get::<String, _>(2).map_err(persistence)?,
                    )?,
                    manifest_version: row.try_get(3).map_err(persistence)?,
                    config_revision: row.try_get(4).map_err(persistence)?,
                    config: serde_json::from_str(
                        &row.try_get::<String, _>(5).map_err(persistence)?,
                    )
                    .map_err(persistence)?,
                    secret_refs: serde_json::from_str(
                        &row.try_get::<String, _>(6).map_err(persistence)?,
                    )
                    .map_err(persistence)?,
                    lifecycle: enum_lifecycle(&row.try_get::<String, _>(7).map_err(persistence)?)?,
                    readiness: enum_readiness(&row.try_get::<String, _>(8).map_err(persistence)?)?,
                    health: enum_health(&row.try_get::<String, _>(9).map_err(persistence)?)?,
                })?;
            }

            let binding_rows = sqlx::query(
                "SELECT id,tenant_id,provider_instance_id,capability_id,
                        config_revision,enabled
                 FROM provider_bindings
                 ORDER BY tenant_id,id",
            )
            .fetch_all(&mut *transaction)
            .await
            .map_err(persistence)?;
            for row in binding_rows {
                catalog.register_binding(ProviderBinding {
                    id: ProviderBindingId::new(row.try_get::<String, _>(0).map_err(persistence)?)?,
                    tenant_id: row.try_get(1).map_err(persistence)?,
                    provider_instance_id: ProviderInstanceId::new(
                        row.try_get::<String, _>(2).map_err(persistence)?,
                    )?,
                    capability: CapabilityId::new(
                        row.try_get::<String, _>(3).map_err(persistence)?,
                    )?,
                    config_revision: row.try_get(4).map_err(persistence)?,
                    enabled: row.try_get(5).map_err(persistence)?,
                })?;
            }

            transaction.commit().await.map_err(persistence)?;
            Ok(catalog)
        })
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn persistence<T>(_error: T) -> IntegrationError {
    IntegrationError::Persistence
}

fn run_pg_catalog<T, F>(future: F) -> Result<T, IntegrationError>
where
    T: Send,
    F: Future<Output = Result<T, IntegrationError>> + Send,
{
    let handle = Handle::try_current().map_err(persistence)?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(IntegrationError::Persistence);
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

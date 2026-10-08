use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::catalog::ProviderCatalog;
#[cfg(feature = "postgres")]
use super::catalog_repository_postgres::PostgresIntegrationCatalogRepository;
use super::catalog_repository_sqlite::SqliteIntegrationCatalogRepository;
use super::types::{IntegrationError, ProviderBinding, ProviderInstance, ProviderManifest};

#[derive(Clone)]
enum IntegrationCatalogBackend {
    Sqlite(SqliteIntegrationCatalogRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresIntegrationCatalogRepository),
}

#[derive(Clone)]
pub(crate) struct IntegrationCatalogRepository {
    backend: IntegrationCatalogBackend,
}

impl IntegrationCatalogRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: IntegrationCatalogBackend::Sqlite(SqliteIntegrationCatalogRepository::new(
                pool,
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: IntegrationCatalogBackend::Postgres(
                PostgresIntegrationCatalogRepository::new(pool),
            ),
        }
    }

    pub(crate) fn save_manifest(
        &self,
        manifest: &ProviderManifest,
    ) -> Result<(), IntegrationError> {
        match &self.backend {
            IntegrationCatalogBackend::Sqlite(repository) => repository.save_manifest(manifest),
            #[cfg(feature = "postgres")]
            IntegrationCatalogBackend::Postgres(repository) => repository.save_manifest(manifest),
        }
    }

    pub(crate) fn save_instance(
        &self,
        instance: &ProviderInstance,
    ) -> Result<(), IntegrationError> {
        match &self.backend {
            IntegrationCatalogBackend::Sqlite(repository) => repository.save_instance(instance),
            #[cfg(feature = "postgres")]
            IntegrationCatalogBackend::Postgres(repository) => repository.save_instance(instance),
        }
    }

    pub(crate) fn save_binding(
        &self,
        binding: &ProviderBinding,
        actor_ref: &str,
    ) -> Result<(), IntegrationError> {
        match &self.backend {
            IntegrationCatalogBackend::Sqlite(repository) => {
                repository.save_binding(binding, actor_ref)
            }
            #[cfg(feature = "postgres")]
            IntegrationCatalogBackend::Postgres(repository) => {
                repository.save_binding(binding, actor_ref)
            }
        }
    }

    pub(crate) fn catalog(&self) -> Result<ProviderCatalog, IntegrationError> {
        match &self.backend {
            IntegrationCatalogBackend::Sqlite(repository) => repository.catalog(),
            #[cfg(feature = "postgres")]
            IntegrationCatalogBackend::Postgres(repository) => repository.catalog(),
        }
    }
}

pub(crate) fn enum_readiness(
    value: &str,
) -> Result<super::types::ProviderReadiness, IntegrationError> {
    use super::types::ProviderReadiness;
    match value {
        "stub" => Ok(ProviderReadiness::Stub),
        "fixture" => Ok(ProviderReadiness::Fixture),
        "sandbox" => Ok(ProviderReadiness::Sandbox),
        "production" => Ok(ProviderReadiness::Production),
        _ => Err(IntegrationError::Persistence),
    }
}

pub(crate) fn enum_lifecycle(
    value: &str,
) -> Result<super::types::ProviderLifecycle, IntegrationError> {
    use super::types::ProviderLifecycle;
    match value {
        "draft" => Ok(ProviderLifecycle::Draft),
        "active" => Ok(ProviderLifecycle::Active),
        "suspended" => Ok(ProviderLifecycle::Suspended),
        "retired" => Ok(ProviderLifecycle::Retired),
        _ => Err(IntegrationError::Persistence),
    }
}

pub(crate) fn enum_health(value: &str) -> Result<super::types::ProviderHealth, IntegrationError> {
    use super::types::ProviderHealth;
    match value {
        "unknown" => Ok(ProviderHealth::Unknown),
        "ready" => Ok(ProviderHealth::Ready),
        "degraded" => Ok(ProviderHealth::Degraded),
        "unhealthy" => Ok(ProviderHealth::Unhealthy),
        _ => Err(IntegrationError::Persistence),
    }
}

pub(crate) fn readiness_value(value: &super::types::ProviderReadiness) -> &'static str {
    use super::types::ProviderReadiness;
    match value {
        ProviderReadiness::Stub => "stub",
        ProviderReadiness::Fixture => "fixture",
        ProviderReadiness::Sandbox => "sandbox",
        ProviderReadiness::Production => "production",
    }
}

pub(crate) fn lifecycle_value(value: &super::types::ProviderLifecycle) -> &'static str {
    use super::types::ProviderLifecycle;
    match value {
        ProviderLifecycle::Draft => "draft",
        ProviderLifecycle::Active => "active",
        ProviderLifecycle::Suspended => "suspended",
        ProviderLifecycle::Retired => "retired",
    }
}

pub(crate) fn health_value(value: &super::types::ProviderHealth) -> &'static str {
    use super::types::ProviderHealth;
    match value {
        ProviderHealth::Unknown => "unknown",
        ProviderHealth::Ready => "ready",
        ProviderHealth::Degraded => "degraded",
        ProviderHealth::Unhealthy => "unhealthy",
    }
}

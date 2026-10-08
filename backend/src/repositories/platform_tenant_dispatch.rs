use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::RepositoryError;
use super::platform_tenant::{
    ClosePlatformTenant, CreatePlatformTenant, PlatformTenantMutationError,
    PlatformTenantProjection, SqlitePlatformTenantRepository, UpdatePlatformTenant,
    UpdatePlatformTenantStatus,
};
#[cfg(feature = "postgres")]
use super::platform_tenant_postgres::PostgresPlatformTenantRepository;

#[derive(Clone)]
enum PlatformTenantBackend {
    Sqlite(SqlitePlatformTenantRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresPlatformTenantRepository),
}

#[derive(Clone)]
pub(crate) struct PlatformTenantRepository {
    backend: PlatformTenantBackend,
}

impl PlatformTenantRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: PlatformTenantBackend::Sqlite(SqlitePlatformTenantRepository::new(pool)),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: PlatformTenantBackend::Postgres(PostgresPlatformTenantRepository::new(pool)),
        }
    }

    pub(crate) fn list(&self) -> Result<Vec<PlatformTenantProjection>, RepositoryError> {
        match &self.backend {
            PlatformTenantBackend::Sqlite(repository) => repository.list(),
            #[cfg(feature = "postgres")]
            PlatformTenantBackend::Postgres(repository) => repository.list(),
        }
    }

    pub(crate) fn get(
        &self,
        tenant_id: &str,
    ) -> Result<Option<PlatformTenantProjection>, RepositoryError> {
        match &self.backend {
            PlatformTenantBackend::Sqlite(repository) => repository.get(tenant_id),
            #[cfg(feature = "postgres")]
            PlatformTenantBackend::Postgres(repository) => repository.get(tenant_id),
        }
    }

    pub(crate) fn create(
        &self,
        command: CreatePlatformTenant,
    ) -> Result<PlatformTenantProjection, PlatformTenantMutationError> {
        match &self.backend {
            PlatformTenantBackend::Sqlite(repository) => repository.create(command),
            #[cfg(feature = "postgres")]
            PlatformTenantBackend::Postgres(repository) => repository.create(command),
        }
    }

    pub(crate) fn update(
        &self,
        command: UpdatePlatformTenant,
    ) -> Result<PlatformTenantProjection, PlatformTenantMutationError> {
        match &self.backend {
            PlatformTenantBackend::Sqlite(repository) => repository.update(command),
            #[cfg(feature = "postgres")]
            PlatformTenantBackend::Postgres(repository) => repository.update(command),
        }
    }

    pub(crate) fn update_status(
        &self,
        command: UpdatePlatformTenantStatus,
    ) -> Result<PlatformTenantProjection, PlatformTenantMutationError> {
        match &self.backend {
            PlatformTenantBackend::Sqlite(repository) => repository.update_status(command),
            #[cfg(feature = "postgres")]
            PlatformTenantBackend::Postgres(repository) => repository.update_status(command),
        }
    }

    pub(crate) fn close(
        &self,
        command: ClosePlatformTenant,
    ) -> Result<(), PlatformTenantMutationError> {
        match &self.backend {
            PlatformTenantBackend::Sqlite(repository) => repository.close(command),
            #[cfg(feature = "postgres")]
            PlatformTenantBackend::Postgres(repository) => repository.close(command),
        }
    }
}

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::RepositoryError;
use super::tenant_preview::{
    PreviewDashboardProjection, PreviewMutationError, PreviewSessionCreate,
    PreviewSessionProjection, SqliteTenantPreviewRepository,
};

#[cfg(feature = "postgres")]
use super::tenant_preview_postgres::PostgresTenantPreviewRepository;

#[derive(Clone)]
enum TenantPreviewBackend {
    Sqlite(SqliteTenantPreviewRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresTenantPreviewRepository),
}

#[derive(Clone)]
pub(crate) struct TenantPreviewRepository {
    backend: TenantPreviewBackend,
}

impl TenantPreviewRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: TenantPreviewBackend::Sqlite(SqliteTenantPreviewRepository::new(pool)),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: TenantPreviewBackend::Postgres(PostgresTenantPreviewRepository::new(pool)),
        }
    }

    pub(crate) fn create_session(
        &self,
        command: PreviewSessionCreate,
    ) -> Result<PreviewSessionProjection, PreviewMutationError> {
        match &self.backend {
            TenantPreviewBackend::Sqlite(repository) => repository.create_session(command),
            #[cfg(feature = "postgres")]
            TenantPreviewBackend::Postgres(repository) => repository.create_session(command),
        }
    }

    pub(crate) fn load_owned(
        &self,
        id: &str,
        actor_id: &str,
    ) -> Result<Option<PreviewSessionProjection>, RepositoryError> {
        match &self.backend {
            TenantPreviewBackend::Sqlite(repository) => repository.load_owned(id, actor_id),
            #[cfg(feature = "postgres")]
            TenantPreviewBackend::Postgres(repository) => repository.load_owned(id, actor_id),
        }
    }

    pub(crate) fn end_session(
        &self,
        id: &str,
        actor_id: &str,
        ended_at: &str,
    ) -> Result<PreviewSessionProjection, PreviewMutationError> {
        match &self.backend {
            TenantPreviewBackend::Sqlite(repository) => {
                repository.end_session(id, actor_id, ended_at)
            }
            #[cfg(feature = "postgres")]
            TenantPreviewBackend::Postgres(repository) => {
                repository.end_session(id, actor_id, ended_at)
            }
        }
    }

    pub(crate) fn tenant_is_active(&self, tenant_id: &str) -> Result<bool, RepositoryError> {
        match &self.backend {
            TenantPreviewBackend::Sqlite(repository) => repository.tenant_is_active(tenant_id),
            #[cfg(feature = "postgres")]
            TenantPreviewBackend::Postgres(repository) => repository.tenant_is_active(tenant_id),
        }
    }

    pub(crate) fn dashboard_summary(
        &self,
        tenant_id: &str,
    ) -> Result<PreviewDashboardProjection, RepositoryError> {
        match &self.backend {
            TenantPreviewBackend::Sqlite(repository) => repository.dashboard_summary(tenant_id),
            #[cfg(feature = "postgres")]
            TenantPreviewBackend::Postgres(repository) => repository.dashboard_summary(tenant_id),
        }
    }
}

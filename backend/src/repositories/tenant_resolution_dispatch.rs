use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::RepositoryError;
use super::tenant_resolution::{SqliteTenantResolutionRepository, TenantResolutionRecord};
#[cfg(feature = "postgres")]
use super::tenant_resolution_postgres::PostgresTenantResolutionRepository;

#[derive(Clone)]
enum TenantResolutionBackend {
    Sqlite(SqliteTenantResolutionRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresTenantResolutionRepository),
}

#[derive(Clone)]
pub(crate) struct TenantResolutionRepository {
    backend: TenantResolutionBackend,
}

impl TenantResolutionRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: TenantResolutionBackend::Sqlite(SqliteTenantResolutionRepository::new(pool)),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: TenantResolutionBackend::Postgres(PostgresTenantResolutionRepository::new(
                pool,
            )),
        }
    }

    pub(crate) fn resolve_by_slug(
        &self,
        slug: &str,
    ) -> Result<Option<TenantResolutionRecord>, RepositoryError> {
        match &self.backend {
            TenantResolutionBackend::Sqlite(repository) => repository.resolve_by_slug(slug),
            #[cfg(feature = "postgres")]
            TenantResolutionBackend::Postgres(repository) => repository.resolve_by_slug(slug),
        }
    }
}

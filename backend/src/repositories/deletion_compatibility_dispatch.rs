use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_admin::deletion::{DeletionAdminInput, DeletionListInput, DeletionRequestInput};

use super::deletion_compatibility::{
    DeletionCompatibilityError, DeletionCompleteResult, DeletionRecord, DeletionRequestOutcome,
    SqliteDeletionCompatibilityRepository,
};
#[cfg(feature = "postgres")]
use super::deletion_compatibility_postgres::PostgresDeletionCompatibilityRepository;

#[derive(Clone)]
enum DeletionCompatibilityBackend {
    Sqlite(SqliteDeletionCompatibilityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresDeletionCompatibilityRepository),
}

#[derive(Clone)]
pub(crate) struct DeletionCompatibilityRepository {
    backend: DeletionCompatibilityBackend,
}

impl DeletionCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: DeletionCompatibilityBackend::Sqlite(
                SqliteDeletionCompatibilityRepository::new(pool),
            ),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: DeletionCompatibilityBackend::Postgres(
                PostgresDeletionCompatibilityRepository::new(pool),
            ),
        }
    }

    pub(crate) fn request(
        &self,
        tenant_id: &str,
        input: &DeletionRequestInput,
        now: &str,
    ) -> Result<DeletionRequestOutcome, DeletionCompatibilityError> {
        match &self.backend {
            DeletionCompatibilityBackend::Sqlite(repository) => {
                repository.request(tenant_id, input, now)
            }
            #[cfg(feature = "postgres")]
            DeletionCompatibilityBackend::Postgres(repository) => {
                repository.request(tenant_id, input, now)
            }
        }
    }

    pub(crate) fn list(
        &self,
        tenant_id: &str,
        input: &DeletionListInput,
    ) -> Result<Vec<DeletionRecord>, DeletionCompatibilityError> {
        match &self.backend {
            DeletionCompatibilityBackend::Sqlite(repository) => repository.list(tenant_id, input),
            #[cfg(feature = "postgres")]
            DeletionCompatibilityBackend::Postgres(repository) => repository.list(tenant_id, input),
        }
    }

    pub(crate) fn process(
        &self,
        tenant_id: &str,
        input: &DeletionAdminInput,
    ) -> Result<(), DeletionCompatibilityError> {
        match &self.backend {
            DeletionCompatibilityBackend::Sqlite(repository) => {
                repository.process(tenant_id, input)
            }
            #[cfg(feature = "postgres")]
            DeletionCompatibilityBackend::Postgres(repository) => {
                repository.process(tenant_id, input)
            }
        }
    }

    pub(crate) fn reject(
        &self,
        tenant_id: &str,
        input: &DeletionAdminInput,
        now: &str,
    ) -> Result<(), DeletionCompatibilityError> {
        match &self.backend {
            DeletionCompatibilityBackend::Sqlite(repository) => {
                repository.reject(tenant_id, input, now)
            }
            #[cfg(feature = "postgres")]
            DeletionCompatibilityBackend::Postgres(repository) => {
                repository.reject(tenant_id, input, now)
            }
        }
    }

    pub(crate) fn complete(
        &self,
        tenant_id: &str,
        input: &DeletionAdminInput,
        now: &str,
    ) -> Result<DeletionCompleteResult, DeletionCompatibilityError> {
        match &self.backend {
            DeletionCompatibilityBackend::Sqlite(repository) => {
                repository.complete(tenant_id, input, now)
            }
            #[cfg(feature = "postgres")]
            DeletionCompatibilityBackend::Postgres(repository) => {
                repository.complete(tenant_id, input, now)
            }
        }
    }
}

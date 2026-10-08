use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_admin::consent::{ConsentCheckInput, ConsentRecordInput, ConsentRevokeInput};

use super::RepositoryError;
use super::consent_compatibility::{
    ConsentAuditRecord, ConsentRecordResult, ConsentRevokeResult,
    SqliteConsentCompatibilityRepository,
};
#[cfg(feature = "postgres")]
use super::consent_compatibility_postgres::PostgresConsentCompatibilityRepository;

#[derive(Clone)]
enum ConsentCompatibilityBackend {
    Sqlite(SqliteConsentCompatibilityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresConsentCompatibilityRepository),
}

#[derive(Clone)]
pub(crate) struct ConsentCompatibilityRepository {
    backend: ConsentCompatibilityBackend,
}

impl ConsentCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: ConsentCompatibilityBackend::Sqlite(
                SqliteConsentCompatibilityRepository::new(pool),
            ),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: ConsentCompatibilityBackend::Postgres(
                PostgresConsentCompatibilityRepository::new(pool),
            ),
        }
    }

    pub(crate) fn record(
        &self,
        tenant_id: &str,
        input: &ConsentRecordInput,
        now: &str,
    ) -> Result<ConsentRecordResult, RepositoryError> {
        match &self.backend {
            ConsentCompatibilityBackend::Sqlite(repository) => {
                repository.record(tenant_id, input, now)
            }
            #[cfg(feature = "postgres")]
            ConsentCompatibilityBackend::Postgres(repository) => {
                repository.record(tenant_id, input, now)
            }
        }
    }

    pub(crate) fn check(
        &self,
        tenant_id: &str,
        input: &ConsentCheckInput,
    ) -> Result<bool, RepositoryError> {
        match &self.backend {
            ConsentCompatibilityBackend::Sqlite(repository) => repository.check(tenant_id, input),
            #[cfg(feature = "postgres")]
            ConsentCompatibilityBackend::Postgres(repository) => repository.check(tenant_id, input),
        }
    }

    pub(crate) fn revoke(
        &self,
        tenant_id: &str,
        input: &ConsentRevokeInput,
        now: &str,
    ) -> Result<ConsentRevokeResult, RepositoryError> {
        match &self.backend {
            ConsentCompatibilityBackend::Sqlite(repository) => {
                repository.revoke(tenant_id, input, now)
            }
            #[cfg(feature = "postgres")]
            ConsentCompatibilityBackend::Postgres(repository) => {
                repository.revoke(tenant_id, input, now)
            }
        }
    }

    pub(crate) fn audit(
        &self,
        tenant_id: &str,
        user_id: &str,
    ) -> Result<Vec<ConsentAuditRecord>, RepositoryError> {
        match &self.backend {
            ConsentCompatibilityBackend::Sqlite(repository) => repository.audit(tenant_id, user_id),
            #[cfg(feature = "postgres")]
            ConsentCompatibilityBackend::Postgres(repository) => {
                repository.audit(tenant_id, user_id)
            }
        }
    }
}

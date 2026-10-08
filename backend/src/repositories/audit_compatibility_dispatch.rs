use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_admin::audit::{ListAuditLogsInput, ListAuditLogsOutput};

use super::RepositoryError;
use super::audit_compatibility::{
    AuditAuthorityWrite, AuditCompatibilityWrite, SqliteAuditCompatibilityRepository,
};
#[cfg(feature = "postgres")]
use super::audit_compatibility_postgres::PostgresAuditCompatibilityRepository;

#[derive(Clone)]
enum AuditCompatibilityBackend {
    Sqlite(SqliteAuditCompatibilityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresAuditCompatibilityRepository),
}

#[derive(Clone)]
pub(crate) struct AuditCompatibilityRepository {
    backend: AuditCompatibilityBackend,
}

impl AuditCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: AuditCompatibilityBackend::Sqlite(SqliteAuditCompatibilityRepository::new(
                pool,
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: AuditCompatibilityBackend::Postgres(
                PostgresAuditCompatibilityRepository::new(pool),
            ),
        }
    }

    pub(crate) fn append_authority_event(
        &self,
        entry: &AuditAuthorityWrite,
    ) -> Result<(), RepositoryError> {
        match &self.backend {
            AuditCompatibilityBackend::Sqlite(repository) => {
                repository.append_authority_event(entry)
            }
            #[cfg(feature = "postgres")]
            AuditCompatibilityBackend::Postgres(repository) => {
                repository.append_authority_event(entry)
            }
        }
    }

    pub(crate) fn append_audit_log(
        &self,
        entry: &AuditCompatibilityWrite,
    ) -> Result<(), RepositoryError> {
        match &self.backend {
            AuditCompatibilityBackend::Sqlite(repository) => repository.append_audit_log(entry),
            #[cfg(feature = "postgres")]
            AuditCompatibilityBackend::Postgres(repository) => repository.append_audit_log(entry),
        }
    }

    pub(crate) fn list_audit_logs(
        &self,
        tenant_id: &str,
        input: &ListAuditLogsInput,
    ) -> Result<ListAuditLogsOutput, RepositoryError> {
        match &self.backend {
            AuditCompatibilityBackend::Sqlite(repository) => {
                repository.list_audit_logs(tenant_id, input)
            }
            #[cfg(feature = "postgres")]
            AuditCompatibilityBackend::Postgres(repository) => {
                repository.list_audit_logs(tenant_id, input)
            }
        }
    }
}

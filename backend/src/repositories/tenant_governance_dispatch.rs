use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::RepositoryError;
use super::tenant_governance::{
    GovernanceAuditQuery, GovernanceAuditRecord, GovernanceChangeIntent,
    GovernanceHealthProjection, GovernanceMutationError, GovernanceTenantListQuery,
    GovernanceTenantProjection, SqliteTenantGovernanceRepository,
};
#[cfg(feature = "postgres")]
use super::tenant_governance_postgres::PostgresTenantGovernanceRepository;

#[derive(Clone)]
enum TenantGovernanceBackend {
    Sqlite(SqliteTenantGovernanceRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresTenantGovernanceRepository),
}

#[derive(Clone)]
pub(crate) struct TenantGovernanceRepository {
    backend: TenantGovernanceBackend,
}

impl TenantGovernanceRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: TenantGovernanceBackend::Sqlite(SqliteTenantGovernanceRepository::new(pool)),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: TenantGovernanceBackend::Postgres(PostgresTenantGovernanceRepository::new(
                pool,
            )),
        }
    }

    pub(crate) fn tenant_list(
        &self,
        query: GovernanceTenantListQuery,
    ) -> Result<Vec<GovernanceTenantProjection>, RepositoryError> {
        match &self.backend {
            TenantGovernanceBackend::Sqlite(repository) => repository.tenant_list(query),
            #[cfg(feature = "postgres")]
            TenantGovernanceBackend::Postgres(repository) => repository.tenant_list(query),
        }
    }

    pub(crate) fn tenant_get(
        &self,
        tenant_id: &str,
    ) -> Result<Option<GovernanceTenantProjection>, RepositoryError> {
        match &self.backend {
            TenantGovernanceBackend::Sqlite(repository) => repository.tenant_get(tenant_id),
            #[cfg(feature = "postgres")]
            TenantGovernanceBackend::Postgres(repository) => repository.tenant_get(tenant_id),
        }
    }

    pub(crate) fn tenant_health(
        &self,
        tenant_id: &str,
        since: &str,
    ) -> Result<Option<GovernanceHealthProjection>, RepositoryError> {
        match &self.backend {
            TenantGovernanceBackend::Sqlite(repository) => {
                repository.tenant_health(tenant_id, since)
            }
            #[cfg(feature = "postgres")]
            TenantGovernanceBackend::Postgres(repository) => {
                repository.tenant_health(tenant_id, since)
            }
        }
    }

    pub(crate) fn audit_query(
        &self,
        query: GovernanceAuditQuery,
    ) -> Result<Vec<GovernanceAuditRecord>, RepositoryError> {
        match &self.backend {
            TenantGovernanceBackend::Sqlite(repository) => repository.audit_query(query),
            #[cfg(feature = "postgres")]
            TenantGovernanceBackend::Postgres(repository) => repository.audit_query(query),
        }
    }

    pub(crate) fn audit_get(
        &self,
        audit_id: &str,
    ) -> Result<Option<GovernanceAuditRecord>, RepositoryError> {
        match &self.backend {
            TenantGovernanceBackend::Sqlite(repository) => repository.audit_get(audit_id),
            #[cfg(feature = "postgres")]
            TenantGovernanceBackend::Postgres(repository) => repository.audit_get(audit_id),
        }
    }

    pub(crate) fn record_change_intent(
        &self,
        command: GovernanceChangeIntent,
    ) -> Result<(), GovernanceMutationError> {
        match &self.backend {
            TenantGovernanceBackend::Sqlite(repository) => repository.record_change_intent(command),
            #[cfg(feature = "postgres")]
            TenantGovernanceBackend::Postgres(repository) => {
                repository.record_change_intent(command)
            }
        }
    }
}

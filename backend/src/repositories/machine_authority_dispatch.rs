use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use crate::error::AppError;

use super::machine_authority::{
    MachineAdminContext, MachineAuthorization, MachineClientProjection, MachineIssuedCredential,
    MachineProvisionRecord, MachineScopeRecord,
};
#[cfg(feature = "postgres")]
use super::machine_authority_postgres::PostgresMachineAuthorityRepository;
use super::machine_authority_sqlite::SqliteMachineAuthorityRepository;

#[derive(Clone)]
enum MachineAuthorityBackend {
    Sqlite(SqliteMachineAuthorityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresMachineAuthorityRepository),
}

#[derive(Clone)]
pub(crate) struct MachineAuthorityRepository {
    backend: MachineAuthorityBackend,
}

impl MachineAuthorityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: MachineAuthorityBackend::Sqlite(SqliteMachineAuthorityRepository::new(pool)),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: MachineAuthorityBackend::Postgres(PostgresMachineAuthorityRepository::new(
                pool,
            )),
        }
    }

    pub(crate) fn provision(
        &self,
        admin: &MachineAdminContext,
        input: MachineProvisionRecord,
    ) -> Result<MachineIssuedCredential, AppError> {
        match &self.backend {
            MachineAuthorityBackend::Sqlite(repository) => repository.provision(admin, input),
            #[cfg(feature = "postgres")]
            MachineAuthorityBackend::Postgres(repository) => repository.provision(admin, input),
        }
    }

    pub(crate) fn authorize(
        &self,
        token: &str,
        requested_tenant: &str,
        version: &str,
        scope: &MachineScopeRecord,
        correlation: &str,
    ) -> Result<MachineAuthorization, AppError> {
        match &self.backend {
            MachineAuthorityBackend::Sqlite(repository) => {
                repository.authorize(token, requested_tenant, version, scope, correlation)
            }
            #[cfg(feature = "postgres")]
            MachineAuthorityBackend::Postgres(repository) => {
                repository.authorize(token, requested_tenant, version, scope, correlation)
            }
        }
    }

    pub(crate) fn lifecycle(
        &self,
        admin: &MachineAdminContext,
        client_id: &str,
        action: &str,
    ) -> Result<Option<MachineIssuedCredential>, AppError> {
        match &self.backend {
            MachineAuthorityBackend::Sqlite(repository) => {
                repository.lifecycle(admin, client_id, action)
            }
            #[cfg(feature = "postgres")]
            MachineAuthorityBackend::Postgres(repository) => {
                repository.lifecycle(admin, client_id, action)
            }
        }
    }

    pub(crate) fn list(
        &self,
        admin: &MachineAdminContext,
    ) -> Result<Vec<MachineClientProjection>, AppError> {
        match &self.backend {
            MachineAuthorityBackend::Sqlite(repository) => repository.list(admin),
            #[cfg(feature = "postgres")]
            MachineAuthorityBackend::Postgres(repository) => repository.list(admin),
        }
    }

    pub(crate) fn credential_lifecycle(
        &self,
        admin: &MachineAdminContext,
        client_id: &str,
        credential_id: &str,
        action: &str,
    ) -> Result<(), AppError> {
        match &self.backend {
            MachineAuthorityBackend::Sqlite(repository) => {
                repository.credential_lifecycle(admin, client_id, credential_id, action)
            }
            #[cfg(feature = "postgres")]
            MachineAuthorityBackend::Postgres(repository) => {
                repository.credential_lifecycle(admin, client_id, credential_id, action)
            }
        }
    }
}

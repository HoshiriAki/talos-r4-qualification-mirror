use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::RepositoryError;
use super::bootstrap_authority::{
    BootstrapPlatformOwnerCommand, BootstrapPlatformOwnerOutcome,
    SqliteBootstrapAuthorityRepository,
};
#[cfg(feature = "postgres")]
use super::bootstrap_authority_postgres::PostgresBootstrapAuthorityRepository;

#[derive(Clone)]
enum BootstrapAuthorityBackend {
    Sqlite(SqliteBootstrapAuthorityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresBootstrapAuthorityRepository),
}

#[derive(Clone)]
pub(crate) struct BootstrapAuthorityRepository {
    backend: BootstrapAuthorityBackend,
}

impl BootstrapAuthorityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: BootstrapAuthorityBackend::Sqlite(SqliteBootstrapAuthorityRepository::new(
                pool,
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: BootstrapAuthorityBackend::Postgres(
                PostgresBootstrapAuthorityRepository::new(pool),
            ),
        }
    }

    pub(crate) fn has_identities(&self) -> Result<bool, RepositoryError> {
        match &self.backend {
            BootstrapAuthorityBackend::Sqlite(repository) => repository.has_identities(),
            #[cfg(feature = "postgres")]
            BootstrapAuthorityBackend::Postgres(repository) => repository.has_identities(),
        }
    }

    pub(crate) fn ensure_initial_platform_owner(
        &self,
        command: BootstrapPlatformOwnerCommand,
    ) -> Result<BootstrapPlatformOwnerOutcome, RepositoryError> {
        match &self.backend {
            BootstrapAuthorityBackend::Sqlite(repository) => {
                repository.ensure_initial_platform_owner(command)
            }
            #[cfg(feature = "postgres")]
            BootstrapAuthorityBackend::Postgres(repository) => {
                repository.ensure_initial_platform_owner(command)
            }
        }
    }
}

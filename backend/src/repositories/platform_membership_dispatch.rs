use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::RepositoryError;
use super::platform_membership::{
    CreatePlatformMembership, CreatedPlatformMembership, PlatformMembershipMutationError,
    PlatformMembershipProjection, RevokePlatformMembership, SqlitePlatformMembershipRepository,
    UpdatePlatformMembership,
};
#[cfg(feature = "postgres")]
use super::platform_membership_postgres::PostgresPlatformMembershipRepository;

#[derive(Clone)]
enum PlatformMembershipBackend {
    Sqlite(SqlitePlatformMembershipRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresPlatformMembershipRepository),
}

#[derive(Clone)]
pub(crate) struct PlatformMembershipRepository {
    backend: PlatformMembershipBackend,
}

impl PlatformMembershipRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: PlatformMembershipBackend::Sqlite(SqlitePlatformMembershipRepository::new(
                pool,
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: PlatformMembershipBackend::Postgres(
                PostgresPlatformMembershipRepository::new(pool),
            ),
        }
    }

    pub(crate) fn list(&self) -> Result<Vec<PlatformMembershipProjection>, RepositoryError> {
        match &self.backend {
            PlatformMembershipBackend::Sqlite(repository) => repository.list(),
            #[cfg(feature = "postgres")]
            PlatformMembershipBackend::Postgres(repository) => repository.list(),
        }
    }

    pub(crate) fn create(
        &self,
        command: CreatePlatformMembership,
    ) -> Result<CreatedPlatformMembership, PlatformMembershipMutationError> {
        match &self.backend {
            PlatformMembershipBackend::Sqlite(repository) => repository.create(command),
            #[cfg(feature = "postgres")]
            PlatformMembershipBackend::Postgres(repository) => repository.create(command),
        }
    }

    pub(crate) fn update(
        &self,
        command: UpdatePlatformMembership,
    ) -> Result<(), PlatformMembershipMutationError> {
        match &self.backend {
            PlatformMembershipBackend::Sqlite(repository) => repository.update(command),
            #[cfg(feature = "postgres")]
            PlatformMembershipBackend::Postgres(repository) => repository.update(command),
        }
    }

    pub(crate) fn revoke(
        &self,
        command: RevokePlatformMembership,
    ) -> Result<(), PlatformMembershipMutationError> {
        match &self.backend {
            PlatformMembershipBackend::Sqlite(repository) => repository.revoke(command),
            #[cfg(feature = "postgres")]
            PlatformMembershipBackend::Postgres(repository) => repository.revoke(command),
        }
    }
}

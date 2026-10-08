use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_admin::staff::{
    CreateTenantMemberInput, DeleteResult, PublicTenantMember, ResetPasswordInput,
    ResetPasswordResult, RevokeTenantMemberInput, ToggleResult, ToggleTenantMemberInput,
    UpdateUsernameInput,
};

use super::tenant_membership_authority::{
    SqliteTenantMembershipAuthorityRepository, TenantMembershipActor,
    TenantMembershipAuthorityError, TenantOwnershipTransfer,
};
#[cfg(feature = "postgres")]
use super::tenant_membership_authority_postgres::PostgresTenantMembershipAuthorityRepository;

#[derive(Clone)]
enum TenantMembershipAuthorityBackend {
    Sqlite(SqliteTenantMembershipAuthorityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresTenantMembershipAuthorityRepository),
}

#[derive(Clone)]
pub(crate) struct TenantMembershipAuthorityRepository {
    backend: TenantMembershipAuthorityBackend,
}

impl TenantMembershipAuthorityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: TenantMembershipAuthorityBackend::Sqlite(
                SqliteTenantMembershipAuthorityRepository::new(pool),
            ),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: TenantMembershipAuthorityBackend::Postgres(
                PostgresTenantMembershipAuthorityRepository::new(pool),
            ),
        }
    }

    pub(crate) fn list(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<PublicTenantMember>, TenantMembershipAuthorityError> {
        match &self.backend {
            TenantMembershipAuthorityBackend::Sqlite(repository) => repository.list(tenant_id),
            #[cfg(feature = "postgres")]
            TenantMembershipAuthorityBackend::Postgres(repository) => repository.list(tenant_id),
        }
    }

    pub(crate) fn create(
        &self,
        actor: &TenantMembershipActor,
        input: &CreateTenantMemberInput,
        now: &str,
    ) -> Result<PublicTenantMember, TenantMembershipAuthorityError> {
        match &self.backend {
            TenantMembershipAuthorityBackend::Sqlite(repository) => {
                repository.create(actor, input, now)
            }
            #[cfg(feature = "postgres")]
            TenantMembershipAuthorityBackend::Postgres(repository) => {
                repository.create(actor, input, now)
            }
        }
    }

    pub(crate) fn delete(
        &self,
        actor: &TenantMembershipActor,
        input: &RevokeTenantMemberInput,
        now: &str,
    ) -> Result<DeleteResult, TenantMembershipAuthorityError> {
        match &self.backend {
            TenantMembershipAuthorityBackend::Sqlite(repository) => {
                repository.delete(actor, input, now)
            }
            #[cfg(feature = "postgres")]
            TenantMembershipAuthorityBackend::Postgres(repository) => {
                repository.delete(actor, input, now)
            }
        }
    }

    pub(crate) fn toggle(
        &self,
        actor: &TenantMembershipActor,
        input: &ToggleTenantMemberInput,
        now: &str,
    ) -> Result<ToggleResult, TenantMembershipAuthorityError> {
        match &self.backend {
            TenantMembershipAuthorityBackend::Sqlite(repository) => {
                repository.toggle(actor, input, now)
            }
            #[cfg(feature = "postgres")]
            TenantMembershipAuthorityBackend::Postgres(repository) => {
                repository.toggle(actor, input, now)
            }
        }
    }

    pub(crate) fn reset_password(
        &self,
        actor: &TenantMembershipActor,
        input: &ResetPasswordInput,
        now: &str,
    ) -> Result<ResetPasswordResult, TenantMembershipAuthorityError> {
        match &self.backend {
            TenantMembershipAuthorityBackend::Sqlite(repository) => {
                repository.reset_password(actor, input, now)
            }
            #[cfg(feature = "postgres")]
            TenantMembershipAuthorityBackend::Postgres(repository) => {
                repository.reset_password(actor, input, now)
            }
        }
    }

    pub(crate) fn update_username(
        &self,
        actor: &TenantMembershipActor,
        input: &UpdateUsernameInput,
        now: &str,
    ) -> Result<PublicTenantMember, TenantMembershipAuthorityError> {
        match &self.backend {
            TenantMembershipAuthorityBackend::Sqlite(repository) => {
                repository.update_username(actor, input, now)
            }
            #[cfg(feature = "postgres")]
            TenantMembershipAuthorityBackend::Postgres(repository) => {
                repository.update_username(actor, input, now)
            }
        }
    }

    pub(crate) fn transfer_ownership(
        &self,
        actor: &TenantMembershipActor,
        target_membership_id: &str,
        now: &str,
    ) -> Result<TenantOwnershipTransfer, TenantMembershipAuthorityError> {
        match &self.backend {
            TenantMembershipAuthorityBackend::Sqlite(repository) => {
                repository.transfer_ownership(actor, target_membership_id, now)
            }
            #[cfg(feature = "postgres")]
            TenantMembershipAuthorityBackend::Postgres(repository) => {
                repository.transfer_ownership(actor, target_membership_id, now)
            }
        }
    }
}

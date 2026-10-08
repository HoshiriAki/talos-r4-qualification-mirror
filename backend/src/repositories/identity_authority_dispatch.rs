use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::DataScope;

use crate::auth_contract::{AuthUserInfo, IdentityAccount};
use crate::error::AppError;

use super::identity_authority::SqliteIdentityAuthorityRepository;
#[cfg(feature = "postgres")]
use super::identity_authority_postgres::PostgresIdentityAuthorityRepository;

#[derive(Clone)]
enum IdentityAuthorityBackend {
    Sqlite(SqliteIdentityAuthorityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresIdentityAuthorityRepository),
}

#[derive(Clone)]
pub(crate) struct IdentityAuthorityRepository {
    backend: IdentityAuthorityBackend,
}

impl IdentityAuthorityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: IdentityAuthorityBackend::Sqlite(SqliteIdentityAuthorityRepository::new(pool)),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: IdentityAuthorityBackend::Postgres(PostgresIdentityAuthorityRepository::new(
                pool,
            )),
        }
    }

    pub(crate) fn find_user_by_username(
        &self,
        scope: &DataScope,
        username: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        match &self.backend {
            IdentityAuthorityBackend::Sqlite(repository) => {
                repository.find_user_by_username(scope, username)
            }
            #[cfg(feature = "postgres")]
            IdentityAuthorityBackend::Postgres(repository) => {
                repository.find_user_by_username(scope, username)
            }
        }
    }

    pub(crate) fn find_platform_identity_by_username(
        &self,
        username: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        match &self.backend {
            IdentityAuthorityBackend::Sqlite(repository) => {
                repository.find_platform_identity_by_username(username)
            }
            #[cfg(feature = "postgres")]
            IdentityAuthorityBackend::Postgres(repository) => {
                repository.find_platform_identity_by_username(username)
            }
        }
    }

    pub(crate) fn find_user_by_id(
        &self,
        scope: &DataScope,
        user_id: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        match &self.backend {
            IdentityAuthorityBackend::Sqlite(repository) => {
                repository.find_user_by_id(scope, user_id)
            }
            #[cfg(feature = "postgres")]
            IdentityAuthorityBackend::Postgres(repository) => {
                repository.find_user_by_id(scope, user_id)
            }
        }
    }

    pub(crate) fn find_identity_by_id(
        &self,
        identity_id: &str,
    ) -> Result<Option<IdentityAccount>, AppError> {
        match &self.backend {
            IdentityAuthorityBackend::Sqlite(repository) => {
                repository.find_identity_by_id(identity_id)
            }
            #[cfg(feature = "postgres")]
            IdentityAuthorityBackend::Postgres(repository) => {
                repository.find_identity_by_id(identity_id)
            }
        }
    }

    pub(crate) fn record_successful_login(
        &self,
        identity_id: &str,
        at: &str,
    ) -> Result<(), AppError> {
        match &self.backend {
            IdentityAuthorityBackend::Sqlite(repository) => {
                repository.record_successful_login(identity_id, at)
            }
            #[cfg(feature = "postgres")]
            IdentityAuthorityBackend::Postgres(repository) => {
                repository.record_successful_login(identity_id, at)
            }
        }
    }

    pub(crate) fn update_identity_profile(
        &self,
        identity_id: &str,
        display_name: Option<&str>,
        email: Option<&str>,
        phone: Option<&str>,
        updated_at: &str,
    ) -> Result<IdentityAccount, AppError> {
        match &self.backend {
            IdentityAuthorityBackend::Sqlite(repository) => repository.update_identity_profile(
                identity_id,
                display_name,
                email,
                phone,
                updated_at,
            ),
            #[cfg(feature = "postgres")]
            IdentityAuthorityBackend::Postgres(repository) => repository.update_identity_profile(
                identity_id,
                display_name,
                email,
                phone,
                updated_at,
            ),
        }
    }

    pub(crate) fn get_auth_user(
        &self,
        session_token: &str,
        tenant_id: Option<&str>,
    ) -> Result<Option<AuthUserInfo>, AppError> {
        match &self.backend {
            IdentityAuthorityBackend::Sqlite(repository) => {
                repository.get_auth_user(session_token, tenant_id)
            }
            #[cfg(feature = "postgres")]
            IdentityAuthorityBackend::Postgres(repository) => {
                repository.get_auth_user(session_token, tenant_id)
            }
        }
    }
}

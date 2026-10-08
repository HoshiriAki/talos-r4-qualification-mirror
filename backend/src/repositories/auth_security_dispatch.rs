use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use crate::error::AppError;

use super::auth_security::{
    SqliteAuthSecurityRepository, StoredRateState, TotpCredentialRewrap, TotpDisableOutcome,
    TotpEnrollmentOutcome, TotpStatus,
};
#[cfg(feature = "postgres")]
use super::auth_security_postgres::PostgresAuthSecurityRepository;

#[derive(Clone)]
enum AuthSecurityBackend {
    Sqlite(SqliteAuthSecurityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresAuthSecurityRepository),
}

#[derive(Clone)]
pub(crate) struct AuthSecurityRepository {
    backend: AuthSecurityBackend,
}

impl AuthSecurityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: AuthSecurityBackend::Sqlite(SqliteAuthSecurityRepository::new(pool)),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: AuthSecurityBackend::Postgres(PostgresAuthSecurityRepository::new(pool)),
        }
    }

    pub(crate) fn blocked_until_ms(&self, key_hash: &str) -> Result<Option<i64>, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => repository.blocked_until_ms(key_hash),
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => repository.blocked_until_ms(key_hash),
        }
    }

    pub(crate) fn mutate_rate_state<T>(
        &self,
        key_hash: &str,
        now_ms: i64,
        prune_before_ms: i64,
        mutation: impl FnOnce(Option<StoredRateState>) -> (StoredRateState, T),
    ) -> Result<T, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => {
                repository.mutate_rate_state(key_hash, now_ms, prune_before_ms, mutation)
            }
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => {
                repository.mutate_rate_state(key_hash, now_ms, prune_before_ms, mutation)
            }
        }
    }

    pub(crate) fn mutate_rate_states(
        &self,
        key_hashes: &[&str],
        now_ms: i64,
        prune_before_ms: i64,
        mutation: impl FnMut(usize, Option<StoredRateState>) -> StoredRateState,
    ) -> Result<(), AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => {
                repository.mutate_rate_states(key_hashes, now_ms, prune_before_ms, mutation)
            }
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => {
                repository.mutate_rate_states(key_hashes, now_ms, prune_before_ms, mutation)
            }
        }
    }

    pub(crate) fn clear_rate_state(&self, key_hash: &str) -> Result<(), AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => repository.clear_rate_state(key_hash),
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => repository.clear_rate_state(key_hash),
        }
    }

    pub(crate) fn insert_session(
        &self,
        id: &str,
        token_hash: &str,
        identity_id: &str,
        auth_strength: &str,
        created_at: &str,
        expires_at: &str,
    ) -> Result<(), AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => repository.insert_session(
                id,
                token_hash,
                identity_id,
                auth_strength,
                created_at,
                expires_at,
            ),
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => repository.insert_session(
                id,
                token_hash,
                identity_id,
                auth_strength,
                created_at,
                expires_at,
            ),
        }
    }

    pub(crate) fn delete_session_by_token_hash(&self, token_hash: &str) -> Result<(), AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => {
                repository.delete_session_by_token_hash(token_hash)
            }
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => {
                repository.delete_session_by_token_hash(token_hash)
            }
        }
    }

    pub(crate) fn delete_sessions_by_identity(&self, identity_id: &str) -> Result<usize, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => {
                repository.delete_sessions_by_identity(identity_id)
            }
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => {
                repository.delete_sessions_by_identity(identity_id)
            }
        }
    }

    pub(crate) fn cleanup_expired_sessions(&self, expires_at_lte: &str) -> Result<usize, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => {
                repository.cleanup_expired_sessions(expires_at_lte)
            }
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => {
                repository.cleanup_expired_sessions(expires_at_lte)
            }
        }
    }

    pub(crate) fn rotate_password_and_revoke_sessions(
        &self,
        identity_id: &str,
        password_hash: &str,
        updated_at: &str,
    ) -> Result<usize, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => repository
                .rotate_password_and_revoke_sessions(identity_id, password_hash, updated_at),
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => repository
                .rotate_password_and_revoke_sessions(identity_id, password_hash, updated_at),
        }
    }

    pub(crate) fn with_totp_credential<T>(
        &self,
        identity_id: &str,
        policy: impl FnOnce(bool, Option<String>) -> Result<(T, Option<TotpCredentialRewrap>), AppError>,
    ) -> Result<T, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => {
                repository.with_totp_credential(identity_id, policy)
            }
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => {
                repository.with_totp_credential(identity_id, policy)
            }
        }
    }

    pub(crate) fn install_pending_totp_secret(
        &self,
        tenant_id: &str,
        identity_id: &str,
        secret_ciphertext: &str,
        updated_at: &str,
    ) -> Result<TotpEnrollmentOutcome, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => repository.install_pending_totp_secret(
                tenant_id,
                identity_id,
                secret_ciphertext,
                updated_at,
            ),
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => repository.install_pending_totp_secret(
                tenant_id,
                identity_id,
                secret_ciphertext,
                updated_at,
            ),
        }
    }

    pub(crate) fn verify_and_enable_scoped_totp<T>(
        &self,
        tenant_id: &str,
        identity_id: &str,
        updated_at: &str,
        policy: impl FnOnce(String) -> Result<(T, Option<String>), AppError>,
    ) -> Result<T, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => {
                repository.verify_and_enable_scoped_totp(tenant_id, identity_id, updated_at, policy)
            }
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => {
                repository.verify_and_enable_scoped_totp(tenant_id, identity_id, updated_at, policy)
            }
        }
    }

    pub(crate) fn disable_scoped_totp(
        &self,
        tenant_id: &str,
        actor_identity_id: Option<&str>,
        identity_id: &str,
        updated_at: &str,
    ) -> Result<TotpDisableOutcome, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => repository.disable_scoped_totp(
                tenant_id,
                actor_identity_id,
                identity_id,
                updated_at,
            ),
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => repository.disable_scoped_totp(
                tenant_id,
                actor_identity_id,
                identity_id,
                updated_at,
            ),
        }
    }

    pub(crate) fn scoped_totp_status(
        &self,
        tenant_id: &str,
        identity_id: &str,
    ) -> Result<TotpStatus, AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => {
                repository.scoped_totp_status(tenant_id, identity_id)
            }
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => {
                repository.scoped_totp_status(tenant_id, identity_id)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn insert_test_identity(&self, identity_id: &str) -> Result<(), AppError> {
        match &self.backend {
            AuthSecurityBackend::Sqlite(repository) => repository.insert_test_identity(identity_id),
            #[cfg(feature = "postgres")]
            AuthSecurityBackend::Postgres(repository) => {
                repository.insert_test_identity(identity_id)
            }
        }
    }
}

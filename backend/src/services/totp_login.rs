use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use crate::error::AppError;
use crate::repositories::{AuthSecurityRepository, TotpCredentialRewrap};
use crate::services::auth_service::TotpLoginResult;

#[cfg(test)]
use rusqlite::params;

/// Production login-side TOTP authority.
///
/// Enrollment and login share `system_admin::totp_crypto` for envelope parsing,
/// key rotation and code verification. Credential SQL and the immediate
/// read/CAS transaction remain owned by `AuthSecurityRepository`; this service
/// owns only cryptographic and authentication policy decisions.
pub fn verify_login_totp(
    pool: &Pool<SqliteConnectionManager>,
    identity_id: &str,
    code: Option<&str>,
) -> Result<TotpLoginResult, AppError> {
    verify_login_totp_with_repository(
        &AuthSecurityRepository::new(pool.clone()),
        identity_id,
        code,
    )
}

pub(crate) fn verify_login_totp_with_repository(
    repository: &AuthSecurityRepository,
    identity_id: &str,
    code: Option<&str>,
) -> Result<TotpLoginResult, AppError> {
    repository.with_totp_credential(identity_id, |enabled, envelope| {
        if !enabled {
            return Ok((TotpLoginResult::NotEnabled, None));
        }

        let Some(code) = code.map(str::trim).filter(|value| value.len() == 6) else {
            return Ok((TotpLoginResult::Required, None));
        };
        if !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return Ok((TotpLoginResult::Invalid, None));
        }

        let envelope = envelope
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| AppError::Internal("enabled TOTP has no secret".into()))?;
        let decrypted = system_admin::totp_crypto::decrypt_totp_secret(&envelope, identity_id)
            .map_err(|_| AppError::Internal("TOTP secret decryption failed".into()))?;

        if !system_admin::totp_crypto::verify_totp_code(&decrypted.secret, code) {
            return Ok((TotpLoginResult::Invalid, None));
        }

        let rewrap = if decrypted.needs_rewrap {
            let replacement_envelope =
                system_admin::totp_crypto::encrypt_totp_secret(&decrypted.secret, identity_id)
                    .map_err(|_| AppError::Internal("TOTP secret rewrap failed".into()))?;
            Some(TotpCredentialRewrap {
                expected_envelope: envelope,
                replacement_envelope,
                updated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            })
        } else {
            None
        };

        Ok((TotpLoginResult::Verified, rewrap))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        pool.get()
            .unwrap()
            .execute_batch(crate::db::baseline::SQLITE_IDENTITY_AUTHORITY_BASELINE)
            .unwrap();
        pool
    }

    fn seed_identity(pool: &Pool<SqliteConnectionManager>, enabled: bool) {
        pool.get()
            .unwrap()
            .execute(
                "INSERT INTO identities
                 (id, username, password_hash, display_name, status, totp_enabled, created_at, updated_at)
                 VALUES ('identity-a', 'alice', 'hash', 'Alice', 'active', ?1, 'now', 'now')",
                params![if enabled { 1 } else { 0 }],
            )
            .unwrap();
    }

    #[test]
    fn disabled_totp_does_not_require_key_material() {
        let pool = pool();
        seed_identity(&pool, false);
        assert_eq!(
            verify_login_totp(&pool, "identity-a", None).unwrap(),
            TotpLoginResult::NotEnabled
        );
    }

    #[test]
    fn enabled_totp_requires_a_six_digit_code_before_crypto() {
        let pool = pool();
        seed_identity(&pool, true);
        pool.get()
            .unwrap()
            .execute(
                "UPDATE identities SET totp_secret_ciphertext = 'v1:00' WHERE id = 'identity-a'",
                [],
            )
            .unwrap();

        assert_eq!(
            verify_login_totp(&pool, "identity-a", None).unwrap(),
            TotpLoginResult::Required
        );
        assert_eq!(
            verify_login_totp(&pool, "identity-a", Some("abc123")).unwrap(),
            TotpLoginResult::Invalid
        );
    }
}

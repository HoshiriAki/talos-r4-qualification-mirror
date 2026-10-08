use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::AppError;
use crate::repositories::AuthSecurityRepository;
use crate::services::auth_service::{self, Session};
use crate::utils::time::{shanghai_now_epoch_ms, shanghai_now_iso};

pub fn create_session_with_strength(
    pool: &Pool<SqliteConnectionManager>,
    user_id: &str,
    auth_strength: &str,
    ttl_days: i64,
) -> Result<Session, AppError> {
    create_session_with_repository(
        &AuthSecurityRepository::new(pool.clone()),
        user_id,
        auth_strength,
        ttl_days,
    )
}

pub(crate) fn create_session_with_repository(
    repository: &AuthSecurityRepository,
    user_id: &str,
    auth_strength: &str,
    ttl_days: i64,
) -> Result<Session, AppError> {
    if !matches!(auth_strength, "password" | "mfa") {
        return Err(AppError::Internal("invalid authentication strength".into()));
    }
    let expires_at = session_expires_at(ttl_days)?;
    let id = Uuid::new_v4().to_string();
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let token_hash = hex::encode(Sha256::digest(token.as_bytes()));
    let now = shanghai_now_iso();

    repository.insert_session(&id, &token_hash, user_id, auth_strength, &now, &expires_at)?;

    Ok(Session {
        id: token,
        user_id: user_id.to_string(),
    })
}

pub fn rotate_password_and_revoke_sessions(
    pool: &Pool<SqliteConnectionManager>,
    identity_id: &str,
    new_password: &str,
) -> Result<usize, AppError> {
    rotate_password_and_revoke_sessions_with_repository(
        &AuthSecurityRepository::new(pool.clone()),
        identity_id,
        new_password,
    )
}

pub(crate) fn rotate_password_and_revoke_sessions_with_repository(
    repository: &AuthSecurityRepository,
    identity_id: &str,
    new_password: &str,
) -> Result<usize, AppError> {
    let password_hash = auth_service::hash_password(new_password);
    repository.rotate_password_and_revoke_sessions(identity_id, &password_hash, &shanghai_now_iso())
}

fn session_expires_at(ttl_days: i64) -> Result<String, AppError> {
    if !(1..=3650).contains(&ttl_days) {
        return Err(AppError::Internal("invalid session TTL policy".into()));
    }
    let ttl_ms = ttl_days
        .checked_mul(24 * 60 * 60 * 1000)
        .ok_or_else(|| AppError::Internal("session TTL overflow".into()))?;
    let expires_ms = shanghai_now_epoch_ms()
        .checked_add(ttl_ms)
        .ok_or_else(|| AppError::Internal("session expiry overflow".into()))?;
    Ok(crate::utils::time::format_epoch_ms_to_shanghai_iso(
        expires_ms,
    ))
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

    fn seed_tenant_identity(pool: &Pool<SqliteConnectionManager>) {
        pool.get()
            .unwrap()
            .execute_batch(
                "INSERT INTO tenants
                 (id,name,slug,status,plan,settings,created_at,updated_at)
                 VALUES ('tenant-a','A','a','active','free',NULL,'now','now');
                 INSERT INTO identities
                 (id,username,password_hash,display_name,status,created_at,updated_at)
                 VALUES ('identity-a','alice','hash','Alice','active','now','now');
                 INSERT INTO tenant_memberships
                 (id,identity_id,tenant_id,role,status,created_at,updated_at)
                 VALUES ('membership-a','identity-a','tenant-a','owner','active','now','now');",
            )
            .unwrap();
    }

    fn token_hash(token: &str) -> String {
        hex::encode(Sha256::digest(token.as_bytes()))
    }

    #[test]
    fn session_ttl_is_explicit_token_is_fresh_and_only_hash_is_persisted() {
        let pool = pool();
        AuthSecurityRepository::new(pool.clone())
            .insert_test_identity("identity-a")
            .unwrap();

        let before = shanghai_now_epoch_ms();
        let first = create_session_with_strength(&pool, "identity-a", "password", 2).unwrap();
        let second = create_session_with_strength(&pool, "identity-a", "password", 2).unwrap();
        assert_ne!(first.id, second.id);

        let (stored_hash, expires_at): (String, String) = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT token_hash, expires_at FROM auth_sessions ORDER BY created_at LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_ne!(stored_hash, first.id);
        assert_eq!(stored_hash.len(), 64);
        let expires_ms = crate::utils::time::parse_date_time_to_epoch_ms(&expires_at).unwrap();
        let expected = 2 * 24 * 60 * 60 * 1000;
        assert!((expires_ms - before - expected).abs() < 5_000);
    }

    #[test]
    fn active_session_is_absolute_lifetime_not_sliding_expiry() {
        let pool = pool();
        seed_tenant_identity(&pool);
        let session = create_session_with_strength(&pool, "identity-a", "password", 2).unwrap();
        let hash = token_hash(&session.id);

        let original_expiry: String = {
            let conn = pool.get().unwrap();
            conn.execute(
                "UPDATE auth_sessions SET last_seen_at = '2000-01-01T00:00:00+08:00' WHERE token_hash = ?1",
                rusqlite::params![hash],
            )
            .unwrap();
            conn.query_row(
                "SELECT expires_at FROM auth_sessions WHERE token_hash = ?1",
                rusqlite::params![hash],
                |row| row.get(0),
            )
            .unwrap()
        };

        let authenticated =
            crate::services::auth_service::get_auth_user(&pool, &session.id, Some("tenant-a"))
                .unwrap();
        assert!(authenticated.is_some());

        let (last_seen_at, expires_at): (String, String) = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT last_seen_at, expires_at FROM auth_sessions WHERE token_hash = ?1",
                rusqlite::params![hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_ne!(last_seen_at, "2000-01-01T00:00:00+08:00");
        assert_eq!(expires_at, original_expiry);
    }

    #[test]
    fn expired_session_is_rejected_and_removed() {
        let pool = pool();
        seed_tenant_identity(&pool);
        let session = create_session_with_strength(&pool, "identity-a", "password", 2).unwrap();
        let hash = token_hash(&session.id);
        pool.get()
            .unwrap()
            .execute(
                "UPDATE auth_sessions SET expires_at = '2000-01-01T00:00:00+08:00' WHERE token_hash = ?1",
                rusqlite::params![hash],
            )
            .unwrap();

        let authenticated =
            crate::services::auth_service::get_auth_user(&pool, &session.id, Some("tenant-a"))
                .unwrap();
        assert!(authenticated.is_none());
        let remaining: i64 = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM auth_sessions WHERE token_hash = ?1",
                rusqlite::params![hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn password_rotation_atomically_revokes_every_prior_bearer() {
        let pool = pool();
        seed_tenant_identity(&pool);
        let first = create_session_with_strength(&pool, "identity-a", "password", 2).unwrap();
        let second = create_session_with_strength(&pool, "identity-a", "mfa", 2).unwrap();

        let revoked =
            rotate_password_and_revoke_sessions(&pool, "identity-a", "replacement-password")
                .unwrap();
        assert_eq!(revoked, 2);

        let stored_hash: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT password_hash FROM identities WHERE id = 'identity-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(auth_service::verify_password(
            "replacement-password",
            &stored_hash
        ));
        assert!(
            auth_service::get_auth_user(&pool, &first.id, Some("tenant-a"))
                .unwrap()
                .is_none()
        );
        assert!(
            auth_service::get_auth_user(&pool, &second.id, Some("tenant-a"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn invalid_session_ttl_and_strength_fail_closed() {
        let pool = pool();
        assert!(create_session_with_strength(&pool, "identity-a", "password", 0).is_err());
        assert!(create_session_with_strength(&pool, "identity-a", "root", 7).is_err());
    }
}

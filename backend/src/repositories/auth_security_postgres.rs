#[cfg(feature = "postgres")]
use std::future::Future;

use sqlx::Row;
use sqlx::postgres::PgPool;
use tokio::runtime::{Handle, RuntimeFlavor};

use crate::error::AppError;

use super::auth_security::{
    StoredRateState, TotpCredentialRewrap, TotpDisableOutcome, TotpEnrollmentOutcome, TotpStatus,
};

#[derive(Clone)]
pub(crate) struct PostgresAuthSecurityRepository {
    pool: PgPool,
}

impl PostgresAuthSecurityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn blocked_until_ms(&self, key_hash: &str) -> Result<Option<i64>, AppError> {
        let pool = self.pool.clone();
        let key_hash = key_hash.to_owned();
        run_pg_auth(async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT blocked_until_ms FROM auth_rate_limit_state WHERE key_hash = $1",
            )
            .bind(key_hash)
            .fetch_optional(&pool)
            .await
            .map_err(pg_persistence)
        })
    }

    pub(crate) fn mutate_rate_state<T>(
        &self,
        key_hash: &str,
        now_ms: i64,
        prune_before_ms: i64,
        mutation: impl FnOnce(Option<StoredRateState>) -> (StoredRateState, T),
    ) -> Result<T, AppError> {
        let pool = self.pool.clone();
        let key_hash = key_hash.to_owned();
        run_pg_auth(async move {
            let mut tx = pool.begin().await.map_err(pg_persistence)?;
            set_serializable(&mut tx).await?;
            let existing = read_rate_state(&mut tx, &key_hash).await?;
            let (state, output) = mutation(existing);
            upsert_rate_state(&mut tx, &key_hash, state, now_ms).await?;
            prune_rate_state(&mut tx, prune_before_ms, now_ms).await?;
            tx.commit().await.map_err(pg_persistence)?;
            Ok(output)
        })
    }

    pub(crate) fn mutate_rate_states(
        &self,
        key_hashes: &[&str],
        now_ms: i64,
        prune_before_ms: i64,
        mut mutation: impl FnMut(usize, Option<StoredRateState>) -> StoredRateState,
    ) -> Result<(), AppError> {
        if key_hashes.is_empty() || key_hashes.len() > 8 {
            return Err(AppError::Internal(
                "invalid auth rate-state batch width".to_string(),
            ));
        }
        let pool = self.pool.clone();
        let key_hashes = key_hashes
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        run_pg_auth(async move {
            let mut tx = pool.begin().await.map_err(pg_persistence)?;
            set_serializable(&mut tx).await?;
            for (index, key_hash) in key_hashes.iter().enumerate() {
                let existing = read_rate_state(&mut tx, key_hash).await?;
                let state = mutation(index, existing);
                upsert_rate_state(&mut tx, key_hash, state, now_ms).await?;
            }
            prune_rate_state(&mut tx, prune_before_ms, now_ms).await?;
            tx.commit().await.map_err(pg_persistence)?;
            Ok(())
        })
    }

    pub(crate) fn clear_rate_state(&self, key_hash: &str) -> Result<(), AppError> {
        let pool = self.pool.clone();
        let key_hash = key_hash.to_owned();
        run_pg_auth(async move {
            sqlx::query("DELETE FROM auth_rate_limit_state WHERE key_hash = $1")
                .bind(key_hash)
                .execute(&pool)
                .await
                .map_err(pg_persistence)?;
            Ok(())
        })
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
        let pool = self.pool.clone();
        let id = id.to_owned();
        let token_hash = token_hash.to_owned();
        let identity_id = identity_id.to_owned();
        let auth_strength = auth_strength.to_owned();
        let created_at = created_at.to_owned();
        let expires_at = expires_at.to_owned();
        run_pg_auth(async move {
            sqlx::query(
                "INSERT INTO auth_sessions
                 (id, token_hash, identity_id, auth_strength, created_at, last_seen_at, expires_at)
                 VALUES ($1, $2, $3, $4, $5, $5, $6)",
            )
            .bind(id)
            .bind(token_hash)
            .bind(identity_id)
            .bind(auth_strength)
            .bind(created_at)
            .bind(expires_at)
            .execute(&pool)
            .await
            .map_err(pg_persistence)?;
            Ok(())
        })
    }

    pub(crate) fn delete_session_by_token_hash(&self, token_hash: &str) -> Result<(), AppError> {
        let pool = self.pool.clone();
        let token_hash = token_hash.to_owned();
        run_pg_auth(async move {
            sqlx::query("DELETE FROM auth_sessions WHERE token_hash = $1")
                .bind(token_hash)
                .execute(&pool)
                .await
                .map_err(pg_persistence)?;
            Ok(())
        })
    }

    pub(crate) fn delete_sessions_by_identity(&self, identity_id: &str) -> Result<usize, AppError> {
        let pool = self.pool.clone();
        let identity_id = identity_id.to_owned();
        run_pg_auth(async move {
            let deleted = sqlx::query("DELETE FROM auth_sessions WHERE identity_id = $1")
                .bind(identity_id)
                .execute(&pool)
                .await
                .map_err(pg_persistence)?
                .rows_affected();
            usize::try_from(deleted)
                .map_err(|_| AppError::Internal("session revoke count overflow".into()))
        })
    }

    pub(crate) fn cleanup_expired_sessions(&self, expires_at_lte: &str) -> Result<usize, AppError> {
        let pool = self.pool.clone();
        let expires_at_lte = expires_at_lte.to_owned();
        run_pg_auth(async move {
            let deleted = sqlx::query("DELETE FROM auth_sessions WHERE expires_at <= $1")
                .bind(expires_at_lte)
                .execute(&pool)
                .await
                .map_err(pg_persistence)?
                .rows_affected();
            usize::try_from(deleted)
                .map_err(|_| AppError::Internal("session cleanup count overflow".into()))
        })
    }

    pub(crate) fn rotate_password_and_revoke_sessions(
        &self,
        identity_id: &str,
        password_hash: &str,
        updated_at: &str,
    ) -> Result<usize, AppError> {
        let pool = self.pool.clone();
        let identity_id = identity_id.to_owned();
        let password_hash = password_hash.to_owned();
        let updated_at = updated_at.to_owned();
        run_pg_auth(async move {
            let mut tx = pool.begin().await.map_err(pg_persistence)?;
            set_serializable(&mut tx).await?;
            let revoked = sqlx::query_scalar::<_, String>(
                "SELECT id FROM auth_sessions WHERE identity_id = $1 FOR UPDATE",
            )
            .bind(&identity_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(pg_persistence)?
            .len();
            // PostgreSQL migration 070 owns the revocation invariant. Updating
            // password_hash fires the trigger in this same transaction so no
            // password rotation can commit while preserving an old bearer.
            let changed = sqlx::query(
                "UPDATE identities
                 SET password_hash = $1, updated_at = $2
                 WHERE id = $3 AND status = 'active'",
            )
            .bind(password_hash)
            .bind(updated_at)
            .bind(&identity_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_persistence)?
            .rows_affected();
            if changed == 0 {
                let _ = tx.rollback().await;
                return Err(AppError::NotFound("Identity 不存在或不可用".to_string()));
            }
            tx.commit().await.map_err(pg_persistence)?;
            Ok(revoked)
        })
    }

    pub(crate) fn with_totp_credential<T>(
        &self,
        identity_id: &str,
        policy: impl FnOnce(bool, Option<String>) -> Result<(T, Option<TotpCredentialRewrap>), AppError>,
    ) -> Result<T, AppError> {
        let pool = self.pool.clone();
        let identity_id = identity_id.to_owned();
        run_pg_auth(async move {
            let mut tx = pool.begin().await.map_err(pg_persistence)?;
            set_serializable(&mut tx).await?;
            let row = sqlx::query(
                "SELECT totp_enabled, totp_secret_ciphertext
                 FROM identities WHERE id = $1 FOR UPDATE",
            )
            .bind(&identity_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(pg_persistence)?;
            let enabled: bool = row.try_get(0).map_err(pg_persistence)?;
            let envelope: Option<String> = row.try_get(1).map_err(pg_persistence)?;

            let (output, rewrap) = policy(enabled, envelope)?;
            if let Some(rewrap) = rewrap {
                let changed = sqlx::query(
                    "UPDATE identities
                     SET totp_secret_ciphertext = $1, updated_at = $2
                     WHERE id = $3
                       AND totp_enabled = TRUE
                       AND totp_secret_ciphertext = $4",
                )
                .bind(rewrap.replacement_envelope)
                .bind(rewrap.updated_at)
                .bind(&identity_id)
                .bind(rewrap.expected_envelope)
                .execute(&mut *tx)
                .await
                .map_err(pg_persistence)?
                .rows_affected();
                if changed != 1 {
                    let _ = tx.rollback().await;
                    return Err(AppError::Internal(
                        "TOTP credential changed during verification".into(),
                    ));
                }
            }

            tx.commit().await.map_err(pg_persistence)?;
            Ok(output)
        })
    }

    pub(crate) fn install_pending_totp_secret(
        &self,
        tenant_id: &str,
        identity_id: &str,
        secret_ciphertext: &str,
        updated_at: &str,
    ) -> Result<TotpEnrollmentOutcome, AppError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let identity_id = identity_id.to_owned();
        let secret_ciphertext = secret_ciphertext.to_owned();
        let updated_at = updated_at.to_owned();
        run_pg_auth(async move {
            let mut tx = pool.begin().await.map_err(pg_persistence)?;
            set_serializable(&mut tx).await?;
            let enabled = sqlx::query_scalar::<_, bool>(
                "SELECT i.totp_enabled
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE tm.tenant_id = $1 AND tm.status = 'active' AND i.id = $2
                 FOR UPDATE OF i",
            )
            .bind(&tenant_id)
            .bind(&identity_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_persistence)?;
            let Some(enabled) = enabled else {
                tx.commit().await.map_err(pg_persistence)?;
                return Ok(TotpEnrollmentOutcome::IdentityUnavailable);
            };
            if enabled {
                tx.commit().await.map_err(pg_persistence)?;
                return Ok(TotpEnrollmentOutcome::AlreadyEnabled);
            }
            let changed = sqlx::query(
                "UPDATE identities
                 SET totp_secret_ciphertext = $1, updated_at = $2
                 WHERE id = $3 AND totp_enabled = FALSE
                   AND EXISTS (
                     SELECT 1 FROM tenant_memberships tm
                     WHERE tm.identity_id = identities.id
                       AND tm.tenant_id = $4
                       AND tm.status = 'active'
                   )",
            )
            .bind(secret_ciphertext)
            .bind(updated_at)
            .bind(&identity_id)
            .bind(tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_persistence)?
            .rows_affected();
            if changed != 1 {
                tx.commit().await.map_err(pg_persistence)?;
                return Ok(TotpEnrollmentOutcome::IdentityUnavailable);
            }
            tx.commit().await.map_err(pg_persistence)?;
            Ok(TotpEnrollmentOutcome::Installed)
        })
    }

    pub(crate) fn verify_and_enable_scoped_totp<T>(
        &self,
        tenant_id: &str,
        identity_id: &str,
        updated_at: &str,
        policy: impl FnOnce(String) -> Result<(T, Option<String>), AppError>,
    ) -> Result<T, AppError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let identity_id = identity_id.to_owned();
        let updated_at = updated_at.to_owned();
        run_pg_auth(async move {
            let mut tx = pool.begin().await.map_err(pg_persistence)?;
            set_serializable(&mut tx).await?;
            let envelope = sqlx::query_scalar::<_, Option<String>>(
                "SELECT i.totp_secret_ciphertext
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE tm.tenant_id = $1 AND tm.status = 'active' AND i.id = $2
                 FOR UPDATE OF i",
            )
            .bind(&tenant_id)
            .bind(&identity_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_persistence)?
            .flatten()
            .ok_or_else(|| AppError::CodedBadRequest {
                code: "2FA_NO_SECRET".into(),
                message: "尚未生成 TOTP 密钥，请先生成".into(),
            })?;
            let (output, replacement) = policy(envelope.clone())?;
            let changed = sqlx::query(
                "UPDATE identities
                 SET totp_enabled = TRUE,
                     totp_secret_ciphertext = COALESCE($1, totp_secret_ciphertext),
                     updated_at = $2
                 WHERE id = $3
                   AND totp_secret_ciphertext = $4
                   AND EXISTS (
                     SELECT 1 FROM tenant_memberships tm
                     WHERE tm.identity_id = identities.id
                       AND tm.tenant_id = $5
                       AND tm.status = 'active'
                   )",
            )
            .bind(replacement)
            .bind(updated_at)
            .bind(&identity_id)
            .bind(envelope)
            .bind(tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_persistence)?
            .rows_affected();
            if changed != 1 {
                let _ = tx.rollback().await;
                return Err(AppError::CodedBadRequest {
                    code: "2FA_CREDENTIAL_CHANGED".into(),
                    message: "TOTP 凭据在验证期间已发生变化，请重新开始绑定".into(),
                });
            }
            tx.commit().await.map_err(pg_persistence)?;
            Ok(output)
        })
    }

    pub(crate) fn disable_scoped_totp(
        &self,
        tenant_id: &str,
        actor_identity_id: Option<&str>,
        identity_id: &str,
        updated_at: &str,
    ) -> Result<TotpDisableOutcome, AppError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let actor_identity_id = actor_identity_id.map(str::to_owned);
        let identity_id = identity_id.to_owned();
        let updated_at = updated_at.to_owned();
        run_pg_auth(async move {
            let mut tx = pool.begin().await.map_err(pg_persistence)?;
            set_serializable(&mut tx).await?;
            let _ = sqlx::query_scalar::<_, String>(
                "SELECT id FROM identities WHERE id = $1 FOR UPDATE",
            )
            .bind(&identity_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_persistence)?;

            if actor_identity_id.as_deref() != Some(identity_id.as_str()) {
                let tenant_memberships = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM tenant_memberships
                     WHERE identity_id = $1 AND status != 'revoked'
                     FOR UPDATE",
                )
                .bind(&identity_id)
                .fetch_all(&mut *tx)
                .await
                .map_err(pg_persistence)?;
                let platform_memberships = sqlx::query_scalar::<_, String>(
                    "SELECT id FROM platform_memberships
                     WHERE identity_id = $1 AND status != 'revoked'
                     FOR UPDATE",
                )
                .bind(&identity_id)
                .fetch_all(&mut *tx)
                .await
                .map_err(pg_persistence)?;
                if tenant_memberships.len() != 1 || !platform_memberships.is_empty() {
                    tx.commit().await.map_err(pg_persistence)?;
                    return Ok(TotpDisableOutcome::SharedIdentityForbidden);
                }
            }

            sqlx::query(
                "UPDATE identities
                 SET totp_enabled = FALSE, totp_secret_ciphertext = NULL, updated_at = $1
                 WHERE id = $2
                   AND EXISTS (
                     SELECT 1 FROM tenant_memberships tm
                     WHERE tm.identity_id = identities.id
                       AND tm.tenant_id = $3
                       AND tm.status = 'active'
                   )",
            )
            .bind(updated_at)
            .bind(identity_id)
            .bind(tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(pg_persistence)?;
            tx.commit().await.map_err(pg_persistence)?;
            Ok(TotpDisableOutcome::Disabled)
        })
    }

    pub(crate) fn scoped_totp_status(
        &self,
        tenant_id: &str,
        identity_id: &str,
    ) -> Result<TotpStatus, AppError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let identity_id = identity_id.to_owned();
        run_pg_auth(async move {
            let row = sqlx::query(
                "SELECT i.totp_enabled,
                        COALESCE(i.totp_secret_ciphertext, '') != ''
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE tm.tenant_id = $1 AND tm.status = 'active' AND i.id = $2",
            )
            .bind(tenant_id)
            .bind(identity_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_persistence)?;
            match row {
                Some(row) => Ok(TotpStatus {
                    enabled: row.try_get(0).map_err(pg_persistence)?,
                    has_secret: row.try_get(1).map_err(pg_persistence)?,
                }),
                None => Ok(TotpStatus {
                    enabled: false,
                    has_secret: false,
                }),
            }
        })
    }

    #[cfg(test)]
    pub(crate) fn insert_test_identity(&self, identity_id: &str) -> Result<(), AppError> {
        let pool = self.pool.clone();
        let identity_id = identity_id.to_owned();
        run_pg_auth(async move {
            sqlx::query(
                "INSERT INTO identities
                 (id,username,password_hash,display_name,status,created_at,updated_at)
                 VALUES ($1,$1,'hash',$1,'active','now','now')",
            )
            .bind(identity_id)
            .execute(&pool)
            .await
            .map_err(pg_persistence)?;
            Ok(())
        })
    }
}

async fn set_serializable(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), AppError> {
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut **tx)
        .await
        .map_err(pg_persistence)?;
    Ok(())
}

async fn read_rate_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key_hash: &str,
) -> Result<Option<StoredRateState>, AppError> {
    let row = sqlx::query(
        "SELECT failure_count, window_started_at_ms, blocked_until_ms
         FROM auth_rate_limit_state
         WHERE key_hash = $1
         FOR UPDATE",
    )
    .bind(key_hash)
    .fetch_optional(&mut **tx)
    .await
    .map_err(pg_persistence)?;

    row.map(|row| {
        Ok(StoredRateState {
            failure_count: row.try_get(0).map_err(pg_persistence)?,
            window_started_at_ms: row.try_get(1).map_err(pg_persistence)?,
            blocked_until_ms: row.try_get(2).map_err(pg_persistence)?,
        })
    })
    .transpose()
}

async fn upsert_rate_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key_hash: &str,
    state: StoredRateState,
    now_ms: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO auth_rate_limit_state
         (key_hash,failure_count,window_started_at_ms,blocked_until_ms,last_seen_at_ms)
         VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT (key_hash) DO UPDATE SET
           failure_count=EXCLUDED.failure_count,
           window_started_at_ms=EXCLUDED.window_started_at_ms,
           blocked_until_ms=EXCLUDED.blocked_until_ms,
           last_seen_at_ms=EXCLUDED.last_seen_at_ms",
    )
    .bind(key_hash)
    .bind(state.failure_count)
    .bind(state.window_started_at_ms)
    .bind(state.blocked_until_ms)
    .bind(now_ms)
    .execute(&mut **tx)
    .await
    .map_err(pg_persistence)?;
    Ok(())
}

async fn prune_rate_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    prune_before_ms: i64,
    now_ms: i64,
) -> Result<(), AppError> {
    sqlx::query(
        "DELETE FROM auth_rate_limit_state
         WHERE last_seen_at_ms < $1 AND blocked_until_ms <= $2",
    )
    .bind(prune_before_ms)
    .bind(now_ms)
    .execute(&mut **tx)
    .await
    .map_err(pg_persistence)?;
    Ok(())
}

fn pg_persistence<E>(_error: E) -> AppError {
    tracing::error!(
        error_class = "auth_postgres",
        "authentication persistence operation failed"
    );
    AppError::ServiceError {
        code: "SYS_AUTH_PERSISTENCE".into(),
        message: "authentication persistence unavailable".into(),
    }
}

fn run_pg_auth<T, F>(future: F) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    let handle = Handle::try_current().map_err(|_| AppError::ServiceError {
        code: "SYS_AUTH_RUNTIME".into(),
        message: "authentication persistence runtime unavailable".into(),
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(AppError::ServiceError {
            code: "SYS_AUTH_RUNTIME".into(),
            message: "authentication persistence requires the multi-thread runtime".into(),
        });
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

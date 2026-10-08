use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StoredRateState {
    pub failure_count: i64,
    pub window_started_at_ms: i64,
    pub blocked_until_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TotpCredentialRewrap {
    pub expected_envelope: String,
    pub replacement_envelope: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TotpEnrollmentOutcome {
    Installed,
    AlreadyEnabled,
    IdentityUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TotpDisableOutcome {
    Disabled,
    SharedIdentityForbidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TotpStatus {
    pub enabled: bool,
    pub has_secret: bool,
}

#[derive(Clone)]
pub(crate) struct SqliteAuthSecurityRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteAuthSecurityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn blocked_until_ms(&self, key_hash: &str) -> Result<Option<i64>, AppError> {
        let conn = self.pool.get()?;
        Ok(conn
            .query_row(
                "SELECT blocked_until_ms FROM auth_rate_limit_state WHERE key_hash=?1",
                [key_hash],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub(crate) fn mutate_rate_state<T>(
        &self,
        key_hash: &str,
        now_ms: i64,
        prune_before_ms: i64,
        mutation: impl FnOnce(Option<StoredRateState>) -> (StoredRateState, T),
    ) -> Result<T, AppError> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = read_rate_state(&tx, key_hash)?;
        let (state, output) = mutation(existing);
        upsert_rate_state(&tx, key_hash, state, now_ms)?;
        prune_rate_state(&tx, prune_before_ms, now_ms)?;
        tx.commit()?;
        Ok(output)
    }

    /// Atomically mutate a bounded set of independent auth-rate dimensions.
    ///
    /// Login failure accounting uses this path so pair/account/source budgets
    /// cannot be partially persisted if a later dimension fails. The mutation
    /// callback receives the key index so policy widths remain owned by the
    /// auth policy service rather than the repository.
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
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (index, key_hash) in key_hashes.iter().enumerate() {
            let existing = read_rate_state(&tx, key_hash)?;
            let state = mutation(index, existing);
            upsert_rate_state(&tx, key_hash, state, now_ms)?;
        }
        prune_rate_state(&tx, prune_before_ms, now_ms)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn clear_rate_state(&self, key_hash: &str) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "DELETE FROM auth_rate_limit_state WHERE key_hash=?1",
            [key_hash],
        )?;
        Ok(())
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
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO auth_sessions
             (id, token_hash, identity_id, auth_strength, created_at, last_seen_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            params![
                id,
                token_hash,
                identity_id,
                auth_strength,
                created_at,
                expires_at,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn delete_session_by_token_hash(&self, token_hash: &str) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "DELETE FROM auth_sessions WHERE token_hash = ?1",
            [token_hash],
        )?;
        Ok(())
    }

    pub(crate) fn delete_sessions_by_identity(&self, identity_id: &str) -> Result<usize, AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "DELETE FROM auth_sessions WHERE identity_id = ?1",
            [identity_id],
        )
        .map_err(Into::into)
    }

    pub(crate) fn cleanup_expired_sessions(&self, expires_at_lte: &str) -> Result<usize, AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "DELETE FROM auth_sessions WHERE expires_at <= ?1",
            [expires_at_lte],
        )
        .map_err(Into::into)
    }

    pub(crate) fn rotate_password_and_revoke_sessions(
        &self,
        identity_id: &str,
        password_hash: &str,
        updated_at: &str,
    ) -> Result<usize, AppError> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revoked = tx.execute(
            "DELETE FROM auth_sessions WHERE identity_id = ?1",
            [identity_id],
        )?;
        let changed = tx.execute(
            "UPDATE identities
             SET password_hash = ?1, updated_at = ?2
             WHERE id = ?3 AND status = 'active'",
            params![password_hash, updated_at, identity_id],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound("Identity 不存在或不可用".to_string()));
        }
        tx.commit()?;
        Ok(revoked)
    }

    /// Execute login-side TOTP credential policy inside one repository-owned
    /// immediate transaction. The callback owns cryptographic/policy decisions;
    /// this repository remains the sole owner of credential SQL and CAS writes.
    pub(crate) fn with_totp_credential<T>(
        &self,
        identity_id: &str,
        policy: impl FnOnce(bool, Option<String>) -> Result<(T, Option<TotpCredentialRewrap>), AppError>,
    ) -> Result<T, AppError> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (enabled, envelope): (bool, Option<String>) = tx.query_row(
            "SELECT totp_enabled, totp_secret_ciphertext FROM identities WHERE id = ?1",
            params![identity_id],
            |row| Ok((row.get::<_, i32>(0)? == 1, row.get(1)?)),
        )?;

        let (output, rewrap) = policy(enabled, envelope)?;
        if let Some(rewrap) = rewrap {
            let changed = tx.execute(
                "UPDATE identities
                 SET totp_secret_ciphertext = ?1, updated_at = ?2
                 WHERE id = ?3 AND totp_enabled = 1 AND totp_secret_ciphertext = ?4",
                params![
                    rewrap.replacement_envelope,
                    rewrap.updated_at,
                    identity_id,
                    rewrap.expected_envelope,
                ],
            )?;
            if changed != 1 {
                return Err(AppError::Internal(
                    "TOTP credential changed during verification".into(),
                ));
            }
        }

        tx.commit()?;
        Ok(output)
    }

    pub(crate) fn install_pending_totp_secret(
        &self,
        tenant_id: &str,
        identity_id: &str,
        secret_ciphertext: &str,
        updated_at: &str,
    ) -> Result<TotpEnrollmentOutcome, AppError> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let enabled = tx
            .query_row(
                "SELECT i.totp_enabled
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE tm.tenant_id = ?1 AND tm.status = 'active' AND i.id = ?2",
                params![tenant_id, identity_id],
                |row| row.get::<_, i32>(0),
            )
            .optional()?
            .map(|value| value == 1);
        let Some(enabled) = enabled else {
            tx.commit()?;
            return Ok(TotpEnrollmentOutcome::IdentityUnavailable);
        };
        if enabled {
            tx.commit()?;
            return Ok(TotpEnrollmentOutcome::AlreadyEnabled);
        }

        let changed = tx.execute(
            "UPDATE identities
             SET totp_secret_ciphertext = ?1, updated_at = ?2
             WHERE id = ?3 AND totp_enabled = 0
               AND EXISTS (
                 SELECT 1 FROM tenant_memberships tm
                 WHERE tm.identity_id = identities.id
                   AND tm.tenant_id = ?4
                   AND tm.status = 'active'
               )",
            params![secret_ciphertext, updated_at, identity_id, tenant_id],
        )?;
        if changed != 1 {
            tx.commit()?;
            return Ok(TotpEnrollmentOutcome::IdentityUnavailable);
        }
        tx.commit()?;
        Ok(TotpEnrollmentOutcome::Installed)
    }

    pub(crate) fn verify_and_enable_scoped_totp<T>(
        &self,
        tenant_id: &str,
        identity_id: &str,
        updated_at: &str,
        policy: impl FnOnce(String) -> Result<(T, Option<String>), AppError>,
    ) -> Result<T, AppError> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let envelope = tx
            .query_row(
                "SELECT i.totp_secret_ciphertext
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE tm.tenant_id = ?1 AND tm.status = 'active' AND i.id = ?2",
                params![tenant_id, identity_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .ok_or_else(|| AppError::CodedBadRequest {
                code: "2FA_NO_SECRET".into(),
                message: "尚未生成 TOTP 密钥，请先生成".into(),
            })?;
        let (output, replacement) = policy(envelope.clone())?;
        let changed = tx.execute(
            "UPDATE identities
             SET totp_enabled = 1,
                 totp_secret_ciphertext = COALESCE(?1, totp_secret_ciphertext),
                 updated_at = ?2
             WHERE id = ?3
               AND totp_secret_ciphertext = ?4
               AND EXISTS (
                 SELECT 1 FROM tenant_memberships tm
                 WHERE tm.identity_id = identities.id
                   AND tm.tenant_id = ?5
                   AND tm.status = 'active'
               )",
            params![replacement, updated_at, identity_id, envelope, tenant_id],
        )?;
        if changed != 1 {
            return Err(AppError::CodedBadRequest {
                code: "2FA_CREDENTIAL_CHANGED".into(),
                message: "TOTP 凭据在验证期间已发生变化，请重新开始绑定".into(),
            });
        }
        tx.commit()?;
        Ok(output)
    }

    pub(crate) fn disable_scoped_totp(
        &self,
        tenant_id: &str,
        actor_identity_id: Option<&str>,
        identity_id: &str,
        updated_at: &str,
    ) -> Result<TotpDisableOutcome, AppError> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if actor_identity_id != Some(identity_id) {
            let tenant_memberships: i64 = tx.query_row(
                "SELECT COUNT(*) FROM tenant_memberships
                 WHERE identity_id = ?1 AND status != 'revoked'",
                [identity_id],
                |row| row.get(0),
            )?;
            let platform_memberships: i64 = tx.query_row(
                "SELECT COUNT(*) FROM platform_memberships
                 WHERE identity_id = ?1 AND status != 'revoked'",
                [identity_id],
                |row| row.get(0),
            )?;
            if tenant_memberships != 1 || platform_memberships != 0 {
                tx.commit()?;
                return Ok(TotpDisableOutcome::SharedIdentityForbidden);
            }
        }
        tx.execute(
            "UPDATE identities
             SET totp_enabled = 0, totp_secret_ciphertext = NULL, updated_at = ?1
             WHERE id = ?2
               AND EXISTS (
                 SELECT 1 FROM tenant_memberships tm
                 WHERE tm.identity_id = identities.id
                   AND tm.tenant_id = ?3
                   AND tm.status = 'active'
               )",
            params![updated_at, identity_id, tenant_id],
        )?;
        tx.commit()?;
        Ok(TotpDisableOutcome::Disabled)
    }

    pub(crate) fn scoped_totp_status(
        &self,
        tenant_id: &str,
        identity_id: &str,
    ) -> Result<TotpStatus, AppError> {
        let conn = self.pool.get()?;
        Ok(conn
            .query_row(
                "SELECT i.totp_enabled,
                        CASE WHEN COALESCE(i.totp_secret_ciphertext, '') != '' THEN 1 ELSE 0 END
                 FROM identities i
                 JOIN tenant_memberships tm ON tm.identity_id = i.id
                 WHERE tm.tenant_id = ?1 AND tm.status = 'active' AND i.id = ?2",
                params![tenant_id, identity_id],
                |row| {
                    Ok(TotpStatus {
                        enabled: row.get::<_, i32>(0)? == 1,
                        has_secret: row.get::<_, i32>(1)? == 1,
                    })
                },
            )
            .optional()?
            .unwrap_or(TotpStatus {
                enabled: false,
                has_secret: false,
            }))
    }

    #[cfg(test)]
    pub(crate) fn insert_test_identity(&self, identity_id: &str) -> Result<(), AppError> {
        let conn = self.pool.get()?;
        conn.execute(
            "INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES (?1,?1,'hash',?1,'active','now','now')",
            [identity_id],
        )?;
        Ok(())
    }
}

fn read_rate_state(
    tx: &rusqlite::Transaction<'_>,
    key_hash: &str,
) -> Result<Option<StoredRateState>, AppError> {
    Ok(tx
        .query_row(
            "SELECT failure_count, window_started_at_ms, blocked_until_ms
             FROM auth_rate_limit_state WHERE key_hash=?1",
            [key_hash],
            |row| {
                Ok(StoredRateState {
                    failure_count: row.get(0)?,
                    window_started_at_ms: row.get(1)?,
                    blocked_until_ms: row.get(2)?,
                })
            },
        )
        .optional()?)
}

fn upsert_rate_state(
    tx: &rusqlite::Transaction<'_>,
    key_hash: &str,
    state: StoredRateState,
    now_ms: i64,
) -> Result<(), AppError> {
    tx.execute(
        "INSERT INTO auth_rate_limit_state
         (key_hash,failure_count,window_started_at_ms,blocked_until_ms,last_seen_at_ms)
         VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(key_hash) DO UPDATE SET
           failure_count=excluded.failure_count,
           window_started_at_ms=excluded.window_started_at_ms,
           blocked_until_ms=excluded.blocked_until_ms,
           last_seen_at_ms=excluded.last_seen_at_ms",
        params![
            key_hash,
            state.failure_count,
            state.window_started_at_ms,
            state.blocked_until_ms,
            now_ms,
        ],
    )?;
    Ok(())
}

fn prune_rate_state(
    tx: &rusqlite::Transaction<'_>,
    prune_before_ms: i64,
    now_ms: i64,
) -> Result<(), AppError> {
    tx.execute(
        "DELETE FROM auth_rate_limit_state
         WHERE last_seen_at_ms < ?1 AND blocked_until_ms <= ?2",
        params![prune_before_ms, now_ms],
    )?;
    Ok(())
}

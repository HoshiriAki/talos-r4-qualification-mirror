//! R4-P7 durable authentication throttling policy.
//!
//! P4 owns trusted client-IP resolution. This service accepts only the already
//! resolved logical key material, derives SHA-256 digests, and delegates all
//! persistence/transaction ownership to `AuthSecurityRepository`.

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use sha2::{Digest, Sha256};

use crate::config::AppConfig;
use crate::error::AppError;
use crate::repositories::{AuthSecurityRepository, StoredRateState};

const LOGIN_ACCOUNT_ATTEMPT_FACTOR: i64 = 3;
const LOGIN_SOURCE_ATTEMPT_FACTOR: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitResult {
    pub allowed: bool,
    pub retry_after_ms: i64,
}

#[derive(Clone)]
pub struct AuthRateLimiter {
    repository: AuthSecurityRepository,
}

impl AuthRateLimiter {
    pub fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self::from_repository(AuthSecurityRepository::new(pool))
    }

    pub(crate) fn from_repository(repository: AuthSecurityRepository) -> Self {
        Self { repository }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self::from_repository(AuthSecurityRepository::postgres(pool))
    }

    pub fn check_login(
        &self,
        resolved_ip: &str,
        username: &str,
        _config: &AppConfig,
    ) -> Result<RateLimitResult, AppError> {
        let (pair_hash, account_hash, source_hash) = login_key_hashes(resolved_ip, username);
        self.check_hashes([&pair_hash, &account_hash, &source_hash])
    }

    pub fn record_login_failure(
        &self,
        resolved_ip: &str,
        username: &str,
        config: &AppConfig,
    ) -> Result<(), AppError> {
        let (pair_hash, account_hash, source_hash) = login_key_hashes(resolved_ip, username);
        let base = RatePolicy::from_config(config);
        let policies = [
            base,
            base.with_attempt_factor(LOGIN_ACCOUNT_ATTEMPT_FACTOR),
            base.with_attempt_factor(LOGIN_SOURCE_ATTEMPT_FACTOR),
        ];
        let hashes = [
            pair_hash.as_str(),
            account_hash.as_str(),
            source_hash.as_str(),
        ];
        let now = now_ms();
        let prune_before = now.saturating_sub(
            policies
                .iter()
                .map(|policy| policy.retention_ms())
                .max()
                .unwrap_or(60_000),
        );

        self.repository
            .mutate_rate_states(&hashes, now, prune_before, |index, existing| {
                failure_state(existing, now, policies[index])
            })
    }

    pub fn record_login_success(&self, resolved_ip: &str, username: &str) -> Result<(), AppError> {
        let (pair_hash, account_hash, _source_hash) = login_key_hashes(resolved_ip, username);
        // A successful credential proof may clear this pair and account's
        // near-threshold history. It intentionally does not erase source-IP
        // history: one valid account must not reset credential-stuffing abuse
        // against other accounts from the same source.
        self.clear_hash(&pair_hash)?;
        self.clear_hash(&account_hash)
    }

    pub fn check_custom(&self, key: &str, config: &AppConfig) -> Result<RateLimitResult, AppError> {
        let key_hash = rate_key_digest("custom", &[key]);
        self.check_hash(&key_hash, config)
    }

    pub fn record_custom_failure(&self, key: &str, config: &AppConfig) -> Result<(), AppError> {
        let key_hash = rate_key_digest("custom", &[key]);
        self.record_failure_hash(&key_hash, config)
    }

    pub fn record_custom_success(&self, key: &str) -> Result<(), AppError> {
        let key_hash = rate_key_digest("custom", &[key]);
        self.clear_hash(&key_hash)
    }

    /// Atomically consume one operation budget unit. The current request is
    /// admitted up to the configured maximum; reaching the maximum blocks the
    /// next request until the block interval expires.
    pub fn consume_custom_budget(
        &self,
        key: &str,
        config: &AppConfig,
    ) -> Result<RateLimitResult, AppError> {
        let key_hash = rate_key_digest("custom-budget", &[key]);
        self.consume_hash(&key_hash, config)
    }

    fn check_hashes<const N: usize>(&self, hashes: [&str; N]) -> Result<RateLimitResult, AppError> {
        let now = now_ms();
        let mut retry_after_ms = 0;
        for key_hash in hashes {
            if let Some(until) = self.repository.blocked_until_ms(key_hash)?
                && until > now
            {
                retry_after_ms = retry_after_ms.max(until - now);
            }
        }
        Ok(RateLimitResult {
            allowed: retry_after_ms == 0,
            retry_after_ms,
        })
    }

    fn check_hash(&self, key_hash: &str, _config: &AppConfig) -> Result<RateLimitResult, AppError> {
        self.check_hashes([key_hash])
    }

    fn record_failure_hash(&self, key_hash: &str, config: &AppConfig) -> Result<(), AppError> {
        self.record_failure_hash_with_policy(key_hash, RatePolicy::from_config(config))
    }

    fn record_failure_hash_with_policy(
        &self,
        key_hash: &str,
        policy: RatePolicy,
    ) -> Result<(), AppError> {
        let now = now_ms();
        let prune_before = now.saturating_sub(policy.retention_ms());
        self.repository
            .mutate_rate_state(key_hash, now, prune_before, |existing| {
                (failure_state(existing, now, policy), ())
            })
    }

    fn consume_hash(
        &self,
        key_hash: &str,
        config: &AppConfig,
    ) -> Result<RateLimitResult, AppError> {
        let now = now_ms();
        let policy = RatePolicy::from_config(config);
        let prune_before = now.saturating_sub(policy.retention_ms());
        self.repository
            .mutate_rate_state(key_hash, now, prune_before, |existing| {
                if let Some(state) = existing
                    && state.blocked_until_ms > now
                {
                    let retry_after_ms = state.blocked_until_ms - now;
                    return (
                        state,
                        RateLimitResult {
                            allowed: false,
                            retry_after_ms,
                        },
                    );
                }

                let (count, window_started, blocked_until) = advance_failure(existing, now, policy);
                (
                    StoredRateState {
                        failure_count: if blocked_until > now { 0 } else { count },
                        window_started_at_ms: window_started,
                        blocked_until_ms: blocked_until,
                    },
                    RateLimitResult {
                        allowed: true,
                        retry_after_ms: 0,
                    },
                )
            })
    }

    fn clear_hash(&self, key_hash: &str) -> Result<(), AppError> {
        self.repository.clear_rate_state(key_hash)
    }
}

#[derive(Debug, Clone, Copy)]
struct RatePolicy {
    window_ms: i64,
    max_attempts: i64,
    block_ms: i64,
}

impl RatePolicy {
    fn from_config(config: &AppConfig) -> Self {
        Self {
            window_ms: config.auth_login_rate_window_ms.max(1),
            max_attempts: i64::from(config.auth_login_rate_max_attempts.max(1)),
            block_ms: config.auth_login_rate_block_ms.max(1),
        }
    }

    fn with_attempt_factor(self, factor: i64) -> Self {
        Self {
            max_attempts: self.max_attempts.saturating_mul(factor.max(1)),
            ..self
        }
    }

    fn retention_ms(self) -> i64 {
        self.window_ms
            .max(self.block_ms)
            .saturating_mul(4)
            .max(60_000)
    }
}

fn login_key_hashes(resolved_ip: &str, username: &str) -> (String, String, String) {
    let username = username.trim().to_lowercase();
    (
        rate_key_digest("login-pair", &[resolved_ip, username.as_str()]),
        rate_key_digest("login-account", &[username.as_str()]),
        rate_key_digest("login-source", &[resolved_ip]),
    )
}

fn failure_state(
    existing: Option<StoredRateState>,
    now: i64,
    policy: RatePolicy,
) -> StoredRateState {
    let (mut count, window_started_at_ms, blocked_until_ms) =
        advance_failure(existing, now, policy);
    if blocked_until_ms > now {
        count = 0;
    }
    StoredRateState {
        failure_count: count,
        window_started_at_ms,
        blocked_until_ms,
    }
}

fn advance_failure(
    existing: Option<StoredRateState>,
    now: i64,
    policy: RatePolicy,
) -> (i64, i64, i64) {
    if let Some(state) = existing {
        if state.blocked_until_ms > now {
            return (0, state.window_started_at_ms, state.blocked_until_ms);
        }
        let within_window = now.saturating_sub(state.window_started_at_ms) < policy.window_ms;
        let window_started = if within_window {
            state.window_started_at_ms
        } else {
            now
        };
        let count = if within_window {
            state.failure_count.saturating_add(1)
        } else {
            1
        };
        if count >= policy.max_attempts {
            return (0, window_started, now.saturating_add(policy.block_ms));
        }
        return (count, window_started, 0);
    }
    if policy.max_attempts <= 1 {
        (0, now, now.saturating_add(policy.block_ms))
    } else {
        (1, now, 0)
    }
}

fn rate_key_digest(namespace: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    append_segment(&mut hasher, namespace.as_bytes());
    for part in parts {
        append_segment(&mut hasher, part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn append_segment(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AppConfig {
        let mut config = AppConfig::from_env();
        config.auth_login_rate_window_ms = 60_000;
        config.auth_login_rate_max_attempts = 2;
        config.auth_login_rate_block_ms = 60_000;
        config
    }

    fn pool() -> Pool<SqliteConnectionManager> {
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::builder().max_size(1).build(manager).unwrap();
        pool.get()
            .unwrap()
            .execute_batch(include_str!(
                "../db/migrations/069_r4_platform_security_hardening.sql"
            ))
            .unwrap();
        pool
    }

    #[test]
    fn rate_keys_are_digest_only_and_login_block_survives_service_recomposition() {
        let pool = pool();
        let config = config();
        let first = AuthRateLimiter::new(pool.clone());
        first
            .record_login_failure("203.0.113.42", "Alice", &config)
            .unwrap();
        first
            .record_login_failure("203.0.113.42", "alice", &config)
            .unwrap();
        assert!(
            !first
                .check_login("203.0.113.42", "ALICE", &config)
                .unwrap()
                .allowed
        );

        let stored: Vec<String> = {
            let conn = pool.get().unwrap();
            let mut stmt = conn
                .prepare("SELECT key_hash FROM auth_rate_limit_state ORDER BY key_hash")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(stored.len(), 3);
        for digest in stored {
            assert_eq!(digest.len(), 64);
            assert!(!digest.contains("203.0.113.42"));
            assert!(!digest.contains("alice"));
        }

        let recomposed = AuthRateLimiter::new(pool);
        assert!(
            !recomposed
                .check_login("203.0.113.42", "alice", &config)
                .unwrap()
                .allowed
        );
    }

    #[test]
    fn login_failure_dimensions_rollback_together() {
        let pool = pool();
        let config = config();
        let limiter = AuthRateLimiter::new(pool.clone());
        let (_, account_hash, _) = login_key_hashes("203.0.113.42", "alice");
        pool.get()
            .unwrap()
            .execute_batch(&format!(
                "CREATE TRIGGER fail_account_rate_state\n                 BEFORE INSERT ON auth_rate_limit_state\n                 FOR EACH ROW WHEN NEW.key_hash = '{account_hash}'\n                 BEGIN SELECT RAISE(ABORT, 'forced auth rate failure'); END;"
            ))
            .unwrap();

        assert!(
            limiter
                .record_login_failure("203.0.113.42", "alice", &config)
                .is_err()
        );
        let count: i64 = pool
            .get()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM auth_rate_limit_state", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn successful_login_clears_pair_and_account_but_preserves_source_history() {
        let pool = pool();
        let config = config();
        let limiter = AuthRateLimiter::new(pool.clone());
        for _ in 0..2 {
            limiter
                .record_login_failure("203.0.113.42", "alice", &config)
                .unwrap();
            limiter
                .record_login_failure("203.0.113.42", "bob", &config)
                .unwrap();
        }
        limiter
            .record_login_success("203.0.113.42", "alice")
            .unwrap();
        assert!(
            limiter
                .check_login("203.0.113.42", "alice", &config)
                .unwrap()
                .allowed
        );
        assert!(
            !limiter
                .check_login("203.0.113.42", "bob", &config)
                .unwrap()
                .allowed
        );

        let source_hash = rate_key_digest("login-source", &["203.0.113.42"]);
        let source_count: i64 = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT failure_count FROM auth_rate_limit_state WHERE key_hash=?1",
                [source_hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source_count, 4);
    }

    #[test]
    fn distributed_sources_share_account_failure_budget() {
        let pool = pool();
        let config = config();
        let limiter = AuthRateLimiter::new(pool);
        let account_limit =
            i64::from(config.auth_login_rate_max_attempts) * LOGIN_ACCOUNT_ATTEMPT_FACTOR;
        for index in 0..account_limit {
            limiter
                .record_login_failure(&format!("203.0.113.{}", index + 1), "alice", &config)
                .unwrap();
        }
        assert!(
            !limiter
                .check_login("198.51.100.9", "alice", &config)
                .unwrap()
                .allowed
        );
    }

    #[test]
    fn rotating_accounts_share_source_failure_budget() {
        let pool = pool();
        let config = config();
        let limiter = AuthRateLimiter::new(pool);
        let source_limit =
            i64::from(config.auth_login_rate_max_attempts) * LOGIN_SOURCE_ATTEMPT_FACTOR;
        for index in 0..source_limit {
            limiter
                .record_login_failure("203.0.113.42", &format!("candidate-{index}"), &config)
                .unwrap();
        }
        assert!(
            !limiter
                .check_login("203.0.113.42", "new-account", &config)
                .unwrap()
                .allowed
        );
    }
}

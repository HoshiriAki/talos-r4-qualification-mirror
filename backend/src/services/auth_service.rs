use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use hmac::{Hmac, Mac};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use scrypt::scrypt;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use system_core::{
    AuthorityContext, DataScope, PlatformMembershipId, PlatformRole, TenantId, TenantMembershipId,
    TenantRole,
};
use uuid::Uuid;

use crate::auth_contract::{AuthUserInfo, IdentityAccount};
use crate::config::AppConfig;
use crate::error::AppError;
use crate::repositories::{
    AuthSecurityRepository, BootstrapAuthorityRepository, BootstrapPlatformOwnerCommand,
    BootstrapPlatformOwnerOutcome, IdentityAuthorityRepository,
};
use crate::utils::time::{shanghai_now_epoch_ms, shanghai_now_iso};

// ── Password hashing ───────────────────────────────────────────

const SCRYPT_LOG_N: u8 = 14; // N=16384 — must match Node.js crypto.scrypt default
const SCRYPT_R: u32 = 8;
const SCRYPT_P: u32 = 1;
const SCRYPT_KEY_LEN: usize = 64;

fn generate_salt() -> [u8; 16] {
    let mut salt = [0u8; 16];
    // Use OsRng via rand
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut salt);
    salt
}

/// Hash a password. Uses the hex string bytes as the scrypt salt — exactly
/// matching Node.js `crypto.scrypt(password, saltHex, 64)` so both backends
/// can verify each other's hashes.
pub fn hash_password(plain: &str) -> String {
    let raw_salt = generate_salt();
    let salt_hex = hex::encode(raw_salt);
    let mut hash = [0u8; SCRYPT_KEY_LEN];
    let params =
        scrypt::Params::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P).expect("scrypt params invalid");
    // salt_hex.as_bytes() = 32 ASCII bytes — same as Node's hex string
    scrypt(plain.as_bytes(), salt_hex.as_bytes(), &params, &mut hash).expect("scrypt failed");
    format!("scrypt${}${}", salt_hex, hex::encode(hash))
}

/// Verify a password. Uses the stored hex-string bytes as the scrypt salt
/// (NOT hex-decoded) — exactly matching Node.js `crypto.scrypt`.
pub fn verify_password(plain: &str, stored: &str) -> bool {
    let parts: Vec<&str> = stored.split('$').collect();
    if parts.len() != 3 || parts[0] != "scrypt" {
        return false;
    }
    // Node.js passes the hex string directly as bytes — do NOT hex-decode
    let salt_bytes = parts[1].as_bytes();
    let stored_hash = match hex::decode(parts[2]) {
        Ok(h) => h,
        Err(_) => return false,
    };
    if stored_hash.len() != SCRYPT_KEY_LEN {
        return false;
    }
    let mut calculated = [0u8; SCRYPT_KEY_LEN];
    let params =
        scrypt::Params::new(SCRYPT_LOG_N, SCRYPT_R, SCRYPT_P).expect("scrypt params invalid");
    if scrypt(plain.as_bytes(), salt_bytes, &params, &mut calculated).is_err() {
        return false;
    }
    stored_hash.ct_eq(&calculated).into()
}

// ── Session management ─────────────────────────────────────────

pub struct Session {
    /// Raw bearer token returned once to the cookie layer. It is never persisted.
    pub id: String,
    #[allow(dead_code)]
    pub user_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotpLoginResult {
    NotEnabled,
    Required,
    Invalid,
    Verified,
}

fn hash_session_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn session_token_fingerprint(token: &str) -> String {
    hash_session_token(token)[..16].to_string()
}

pub fn get_session_expires_at() -> String {
    let ttl_ms: i64 = 7 * 24 * 60 * 60 * 1000; // SESSION_TTL_DAYS default
    let expires_ms = shanghai_now_epoch_ms() + ttl_ms;
    crate::utils::time::format_epoch_ms_to_shanghai_iso(expires_ms)
}

pub fn create_session(
    pool: &Pool<SqliteConnectionManager>,
    user_id: &str,
) -> Result<Session, AppError> {
    create_session_with_strength(pool, user_id, "password")
}

pub fn create_session_with_strength(
    pool: &Pool<SqliteConnectionManager>,
    user_id: &str,
    auth_strength: &str,
) -> Result<Session, AppError> {
    if !matches!(auth_strength, "password" | "mfa") {
        return Err(AppError::Internal("invalid authentication strength".into()));
    }
    let conn = pool.get()?;
    let id = Uuid::new_v4().to_string();
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let token_hash = hash_session_token(&token);
    let now = shanghai_now_iso();
    let expires_at = get_session_expires_at();
    conn.execute(
        "INSERT INTO auth_sessions
         (id, token_hash, identity_id, auth_strength, created_at, last_seen_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
        params![id, token_hash, user_id, auth_strength, now, expires_at],
    )?;
    Ok(Session {
        id: token,
        user_id: user_id.to_string(),
    })
}

fn configured_totp_encryption_key() -> Result<[u8; 32], AppError> {
    let encoded = std::env::var("TALOS_TOTP_ENCRYPTION_KEY")
        .map_err(|_| AppError::Internal("TALOS_TOTP_ENCRYPTION_KEY is required".into()))?;
    let bytes = hex::decode(encoded.trim())
        .map_err(|_| AppError::Internal("TALOS_TOTP_ENCRYPTION_KEY must be hex".into()))?;
    bytes
        .try_into()
        .map_err(|_| AppError::Internal("TALOS_TOTP_ENCRYPTION_KEY must decode to 32 bytes".into()))
}

fn decrypt_totp_secret(envelope: &str, identity_id: &str) -> Result<Vec<u8>, AppError> {
    let mut parts = envelope.split(':');
    if parts.next() != Some("v1") {
        return Err(AppError::Internal(
            "unsupported TOTP secret envelope".into(),
        ));
    }
    let nonce_bytes = hex::decode(parts.next().unwrap_or_default())
        .map_err(|_| AppError::Internal("invalid TOTP nonce".into()))?;
    let ciphertext = hex::decode(parts.next().unwrap_or_default())
        .map_err(|_| AppError::Internal("invalid TOTP ciphertext".into()))?;
    if parts.next().is_some() || nonce_bytes.len() != 12 {
        return Err(AppError::Internal("invalid TOTP secret envelope".into()));
    }
    let key = configured_totp_encryption_key()?;
    Aes256Gcm::new_from_slice(&key)
        .map_err(|_| AppError::Internal("invalid TOTP encryption key".into()))?
        .decrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: &ciphertext,
                aad: identity_id.as_bytes(),
            },
        )
        .map_err(|_| AppError::Internal("TOTP secret authentication failed".into()))
}

fn generate_totp_code(secret: &[u8], time_step: u64) -> u32 {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(secret).expect("HMAC accepts arbitrary key size");
    mac.update(&time_step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = ((u32::from(digest[offset]) & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    binary % 1_000_000
}

pub fn verify_login_totp(
    pool: &Pool<SqliteConnectionManager>,
    identity_id: &str,
    code: Option<&str>,
) -> Result<TotpLoginResult, AppError> {
    let conn = pool.get()?;
    let (enabled, envelope): (bool, Option<String>) = conn.query_row(
        "SELECT totp_enabled, totp_secret_ciphertext FROM identities WHERE id = ?1",
        params![identity_id],
        |row| Ok((row.get::<_, i32>(0)? == 1, row.get(1)?)),
    )?;
    if !enabled {
        return Ok(TotpLoginResult::NotEnabled);
    }
    let Some(code) = code.map(str::trim).filter(|value| value.len() == 6) else {
        return Ok(TotpLoginResult::Required);
    };
    let Ok(user_code) = code.parse::<u32>() else {
        return Ok(TotpLoginResult::Invalid);
    };
    let secret = decrypt_totp_secret(
        envelope
            .as_deref()
            .ok_or_else(|| AppError::Internal("enabled TOTP has no secret".into()))?,
        identity_id,
    )?;
    let step = (shanghai_now_epoch_ms().max(0) as u64 / 1000) / 30;
    let verified = step.saturating_sub(1)..=step.saturating_add(1);
    if verified
        .into_iter()
        .any(|candidate| generate_totp_code(&secret, candidate) == user_code)
    {
        Ok(TotpLoginResult::Verified)
    } else {
        Ok(TotpLoginResult::Invalid)
    }
}

pub fn delete_session(
    pool: &Pool<SqliteConnectionManager>,
    session_token: &str,
) -> Result<(), AppError> {
    delete_session_with_repository(&AuthSecurityRepository::new(pool.clone()), session_token)
}

pub(crate) fn delete_session_with_repository(
    repository: &AuthSecurityRepository,
    session_token: &str,
) -> Result<(), AppError> {
    repository.delete_session_by_token_hash(&hash_session_token(session_token))
}

pub fn delete_sessions_by_user_id(
    pool: &Pool<SqliteConnectionManager>,
    user_id: &str,
) -> Result<usize, AppError> {
    delete_sessions_by_user_id_with_repository(&AuthSecurityRepository::new(pool.clone()), user_id)
}

pub(crate) fn delete_sessions_by_user_id_with_repository(
    repository: &AuthSecurityRepository,
    user_id: &str,
) -> Result<usize, AppError> {
    repository.delete_sessions_by_identity(user_id)
}

pub fn cleanup_expired_sessions(pool: &Pool<SqliteConnectionManager>) -> Result<(), AppError> {
    cleanup_expired_sessions_with_repository(&AuthSecurityRepository::new(pool.clone()))
}

pub(crate) fn cleanup_expired_sessions_with_repository(
    repository: &AuthSecurityRepository,
) -> Result<(), AppError> {
    repository.cleanup_expired_sessions(&shanghai_now_iso())?;
    Ok(())
}

// ── User queries ───────────────────────────────────────────────

fn parse_tenant_role(value: &str) -> rusqlite::Result<TenantRole> {
    match value {
        "staff" => Ok(TenantRole::Staff),
        "admin" => Ok(TenantRole::Admin),
        "owner" => Ok(TenantRole::Owner),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_platform_role(value: &str) -> rusqlite::Result<PlatformRole> {
    match value {
        "platform_owner" => Ok(PlatformRole::Owner),
        "platform_admin" => Ok(PlatformRole::Admin),
        "platform_operator" => Ok(PlatformRole::Operator),
        "support_engineer" => Ok(PlatformRole::SupportEngineer),
        "business_operator" => Ok(PlatformRole::BusinessOperator),
        "security_auditor" => Ok(PlatformRole::SecurityAuditor),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn platform_role_name(role: PlatformRole) -> &'static str {
    match role {
        PlatformRole::Owner => "platform_owner",
        PlatformRole::Admin => "platform_admin",
        PlatformRole::Operator => "platform_operator",
        PlatformRole::SupportEngineer => "support_engineer",
        PlatformRole::BusinessOperator => "business_operator",
        PlatformRole::SecurityAuditor => "security_auditor",
    }
}

/// Find user by username, optionally filtered by tenant_id.
///
/// # Security
/// - If `tenant_id` is provided, only returns users belonging to that tenant
/// - If `tenant_id` is None, searches across all tenants (single-tenant mode)
///
/// # Multi-tenant security
/// Always pass tenant_id when available to prevent cross-tenant login attacks.
pub fn find_user_by_username(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    username: &str,
) -> Result<Option<IdentityAccount>, AppError> {
    find_user_by_username_with_repository(
        &IdentityAuthorityRepository::new(pool.clone()),
        scope,
        username,
    )
}

pub(crate) fn find_user_by_username_with_repository(
    repository: &IdentityAuthorityRepository,
    scope: &DataScope,
    username: &str,
) -> Result<Option<IdentityAccount>, AppError> {
    repository.find_user_by_username(scope, username)
}

pub fn find_platform_identity_by_username(
    pool: &Pool<SqliteConnectionManager>,
    username: &str,
) -> Result<Option<IdentityAccount>, AppError> {
    find_platform_identity_by_username_with_repository(
        &IdentityAuthorityRepository::new(pool.clone()),
        username,
    )
}

pub(crate) fn find_platform_identity_by_username_with_repository(
    repository: &IdentityAuthorityRepository,
    username: &str,
) -> Result<Option<IdentityAccount>, AppError> {
    repository.find_platform_identity_by_username(username)
}

pub fn find_user_by_id(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    user_id: &str,
) -> Result<Option<IdentityAccount>, AppError> {
    find_user_by_id_with_repository(
        &IdentityAuthorityRepository::new(pool.clone()),
        scope,
        user_id,
    )
}

pub(crate) fn find_user_by_id_with_repository(
    repository: &IdentityAuthorityRepository,
    scope: &DataScope,
    user_id: &str,
) -> Result<Option<IdentityAccount>, AppError> {
    repository.find_user_by_id(scope, user_id)
}

pub fn find_identity_by_id(
    pool: &Pool<SqliteConnectionManager>,
    identity_id: &str,
) -> Result<Option<IdentityAccount>, AppError> {
    find_identity_by_id_with_repository(
        &IdentityAuthorityRepository::new(pool.clone()),
        identity_id,
    )
}

pub(crate) fn find_identity_by_id_with_repository(
    repository: &IdentityAuthorityRepository,
    identity_id: &str,
) -> Result<Option<IdentityAccount>, AppError> {
    repository.find_identity_by_id(identity_id)
}

pub fn update_identity_password(
    pool: &Pool<SqliteConnectionManager>,
    identity_id: &str,
    new_password: &str,
) -> Result<(), AppError> {
    let conn = pool.get()?;
    let changed = conn.execute(
        "UPDATE identities SET password_hash = ?1, updated_at = ?2 WHERE id = ?3 AND status = 'active'",
        params![hash_password(new_password), shanghai_now_iso(), identity_id],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound("Identity 不存在或不可用".into()));
    }
    Ok(())
}

pub fn update_identity_profile(
    pool: &Pool<SqliteConnectionManager>,
    identity_id: &str,
    display_name: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
) -> Result<IdentityAccount, AppError> {
    update_identity_profile_with_repository(
        &IdentityAuthorityRepository::new(pool.clone()),
        identity_id,
        display_name,
        email,
        phone,
    )
}

pub(crate) fn update_identity_profile_with_repository(
    repository: &IdentityAuthorityRepository,
    identity_id: &str,
    display_name: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
) -> Result<IdentityAccount, AppError> {
    let display_name = display_name.map(|value| value.trim().chars().take(64).collect::<String>());
    let email = email.map(|value| value.trim().chars().take(128).collect::<String>());
    let phone = phone.map(|value| value.trim().chars().take(32).collect::<String>());
    repository.update_identity_profile(
        identity_id,
        display_name.as_deref(),
        email.as_deref(),
        phone.as_deref(),
        &shanghai_now_iso(),
    )
}

pub(crate) fn record_successful_login_with_repository(
    repository: &IdentityAuthorityRepository,
    identity_id: &str,
) -> Result<(), AppError> {
    repository.record_successful_login(identity_id, &shanghai_now_iso())
}

pub fn update_user_password(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    user_id: &str,
    new_password: &str,
) -> Result<(), AppError> {
    let conn = pool.get()?;
    let hash = hash_password(new_password);
    let now = shanghai_now_iso();
    conn.execute(
        "UPDATE identities SET password_hash = ?1, updated_at = ?2
         WHERE id = ?3 AND EXISTS (
             SELECT 1 FROM tenant_memberships tm
             WHERE tm.identity_id = identities.id AND tm.tenant_id = ?4 AND tm.status = 'active'
         )",
        params![hash, now, user_id, scope.tenant_id().as_str()],
    )?;
    Ok(())
}

// ── Profile update ─────────────────────────────────────────────

pub fn update_user_profile(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    user_id: &str,
    display_name: Option<&str>,
    email: Option<&str>,
    phone: Option<&str>,
) -> Result<IdentityAccount, AppError> {
    let conn = pool.get()?;
    let now = shanghai_now_iso();

    let dn: Option<String> = display_name.map(|s| s.trim().chars().take(64).collect());
    let em: Option<String> = email.map(|s| s.trim().chars().take(128).collect());
    let ph: Option<String> = phone.map(|s| s.trim().chars().take(32).collect());

    match (&dn, &em, &ph) {
        (Some(dn), None, None) => {
            conn.execute(
                "UPDATE identities SET display_name = ?1, updated_at = ?2 WHERE id = ?3 AND EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id = identities.id AND tenant_id = ?4)",
                params![dn, now, user_id, scope.tenant_id().as_str()],
            )?;
        }
        (None, Some(em), None) => {
            conn.execute(
                "UPDATE identities SET email = ?1, updated_at = ?2 WHERE id = ?3 AND EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id = identities.id AND tenant_id = ?4)",
                params![em, now, user_id, scope.tenant_id().as_str()],
            )?;
        }
        (None, None, Some(ph)) => {
            conn.execute(
                "UPDATE identities SET phone = ?1, updated_at = ?2 WHERE id = ?3 AND EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id = identities.id AND tenant_id = ?4)",
                params![ph, now, user_id, scope.tenant_id().as_str()],
            )?;
        }
        (Some(dn), Some(em), None) => {
            conn.execute(
                "UPDATE identities SET display_name = ?1, email = ?2, updated_at = ?3 WHERE id = ?4 AND EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id = identities.id AND tenant_id = ?5)",
                params![dn, em, now, user_id, scope.tenant_id().as_str()],
            )?;
        }
        (Some(dn), None, Some(ph)) => {
            conn.execute(
                "UPDATE identities SET display_name = ?1, phone = ?2, updated_at = ?3 WHERE id = ?4 AND EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id = identities.id AND tenant_id = ?5)",
                params![dn, ph, now, user_id, scope.tenant_id().as_str()],
            )?;
        }
        (None, Some(em), Some(ph)) => {
            conn.execute(
                "UPDATE identities SET email = ?1, phone = ?2, updated_at = ?3 WHERE id = ?4 AND EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id = identities.id AND tenant_id = ?5)",
                params![em, ph, now, user_id, scope.tenant_id().as_str()],
            )?;
        }
        (Some(dn), Some(em), Some(ph)) => {
            conn.execute(
                "UPDATE identities SET display_name = ?1, email = ?2, phone = ?3, updated_at = ?4 WHERE id = ?5 AND EXISTS (SELECT 1 FROM tenant_memberships WHERE identity_id = identities.id AND tenant_id = ?6)",
                params![dn, em, ph, now, user_id, scope.tenant_id().as_str()],
            )?;
        }
        (None, None, None) => {
            return find_user_by_id(pool, scope, user_id)?
                .ok_or_else(|| AppError::NotFound("用户不存在".to_string()));
        }
    }

    find_user_by_id(pool, scope, user_id)?
        .ok_or_else(|| AppError::NotFound("用户不存在".to_string()))
}

// ── Auth user from request ─────────────────────────────────────

pub fn get_auth_user(
    pool: &Pool<SqliteConnectionManager>,
    session_token: &str,
    tenant_id: Option<&str>,
) -> Result<Option<AuthUserInfo>, AppError> {
    get_auth_user_with_repository(
        &IdentityAuthorityRepository::new(pool.clone()),
        session_token,
        tenant_id,
    )
}

pub(crate) fn get_auth_user_with_repository(
    repository: &IdentityAuthorityRepository,
    session_token: &str,
    tenant_id: Option<&str>,
) -> Result<Option<AuthUserInfo>, AppError> {
    repository.get_auth_user(session_token, tenant_id)
}

// ── Admin bootstrap ────────────────────────────────────────────

pub fn ensure_initial_admin(pool: &Pool<SqliteConnectionManager>, config: &AppConfig) {
    ensure_initial_admin_with_repository(&BootstrapAuthorityRepository::new(pool.clone()), config);
}

pub(crate) fn ensure_initial_admin_with_repository(
    repository: &BootstrapAuthorityRepository,
    config: &AppConfig,
) {
    let has_identities = match repository.has_identities() {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(
                "[auth] Failed to inspect identity state for admin bootstrap: {}",
                error
            );
            return;
        }
    };
    if has_identities {
        return;
    }

    if !config.auth_bootstrap_on_start {
        tracing::warn!(
            "[auth] identities is empty, platform owner bootstrap skipped (AUTH_BOOTSTRAP_ON_START not enabled)"
        );
        return;
    }

    let username = config.auth_bootstrap_admin_username.as_str();
    let password = config.auth_bootstrap_admin_password.as_str();

    if username.is_empty() || password.is_empty() {
        tracing::warn!(
            "[auth] Admin bootstrap failed: missing AUTH_BOOTSTRAP_ADMIN_USERNAME or AUTH_BOOTSTRAP_ADMIN_PASSWORD"
        );
        return;
    }

    let command = BootstrapPlatformOwnerCommand {
        username: username.to_string(),
        password_hash: hash_password(password),
        now: shanghai_now_iso(),
    };
    match repository.ensure_initial_platform_owner(command) {
        Ok(BootstrapPlatformOwnerOutcome::Created) => {
            tracing::info!("[auth] Platform owner bootstrapped: {}", username)
        }
        Ok(BootstrapPlatformOwnerOutcome::AlreadyInitialized) => tracing::info!(
            "[auth] Platform owner bootstrap skipped: identity authority initialized concurrently"
        ),
        Err(error) => tracing::error!("[auth] Platform owner bootstrap failed: {}", error),
    }
}

// ── Login rate limiter ─────────────────────────────────────────

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

struct RateState {
    failures: Vec<i64>,
    blocked_until: i64,
    last_seen_at: i64,
}

static RATE_STORE: OnceLock<Mutex<HashMap<String, RateState>>> = OnceLock::new();

fn rate_store() -> &'static Mutex<HashMap<String, RateState>> {
    RATE_STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn build_rate_key(ip: &str, username: &str) -> String {
    let u = username.trim().to_lowercase();
    format!("{}::{}", ip, if u.is_empty() { "-" } else { &u })
}

pub struct RateLimitResult {
    pub allowed: bool,
    pub retry_after_ms: i64,
}

pub fn check_login_rate_limit(ip: &str, username: &str, config: &AppConfig) -> RateLimitResult {
    let key = build_rate_key(ip, username);
    let mut store = rate_store().lock().unwrap();
    let state = store.entry(key).or_insert(RateState {
        failures: Vec::new(),
        blocked_until: 0,
        last_seen_at: now_ms(),
    });

    let now = now_ms();
    state.last_seen_at = now;

    // Prune old failures
    let window_start = now - config.auth_login_rate_window_ms;
    state.failures.retain(|ts| *ts >= window_start);
    if state.blocked_until < now {
        state.blocked_until = 0;
    }

    if state.blocked_until > now {
        RateLimitResult {
            allowed: false,
            retry_after_ms: state.blocked_until - now,
        }
    } else {
        RateLimitResult {
            allowed: true,
            retry_after_ms: 0,
        }
    }
}

pub fn record_login_failure(ip: &str, username: &str, config: &AppConfig) {
    let key = build_rate_key(ip, username);
    let mut store = rate_store().lock().unwrap();
    let state = store.entry(key).or_insert(RateState {
        failures: Vec::new(),
        blocked_until: 0,
        last_seen_at: now_ms(),
    });

    let now = now_ms();
    let window_start = now - config.auth_login_rate_window_ms;
    state.failures.retain(|ts| *ts >= window_start);
    state.failures.push(now);

    if state.failures.len() >= config.auth_login_rate_max_attempts as usize {
        state.blocked_until = now + config.auth_login_rate_block_ms;
        state.failures.clear();
    }
}

pub fn record_login_success(ip: &str, username: &str) {
    let key = build_rate_key(ip, username);
    let mut store = rate_store().lock().unwrap();
    store.remove(&key);
}

// ── Custom-key rate limiter (for non-login endpoints) ────────────

pub fn check_custom_rate_limit(key: &str, config: &AppConfig) -> RateLimitResult {
    let mut store = rate_store().lock().unwrap();
    let now = now_ms();
    let state = store.entry(key.to_string()).or_insert(RateState {
        failures: Vec::new(),
        blocked_until: 0,
        last_seen_at: now,
    });
    state.last_seen_at = now;

    let window_start = now - config.auth_login_rate_window_ms;
    state.failures.retain(|ts| *ts >= window_start);
    if state.blocked_until < now {
        state.blocked_until = 0;
    }

    if state.blocked_until > now {
        RateLimitResult {
            allowed: false,
            retry_after_ms: state.blocked_until - now,
        }
    } else {
        RateLimitResult {
            allowed: true,
            retry_after_ms: 0,
        }
    }
}

pub fn record_custom_failure(key: &str, config: &AppConfig) {
    let mut store = rate_store().lock().unwrap();
    let now = now_ms();
    let state = store.entry(key.to_string()).or_insert(RateState {
        failures: Vec::new(),
        blocked_until: 0,
        last_seen_at: now,
    });
    let window_start = now - config.auth_login_rate_window_ms;
    state.failures.retain(|ts| *ts >= window_start);
    state.failures.push(now);

    if state.failures.len() >= config.auth_login_rate_max_attempts as usize {
        state.blocked_until = now + config.auth_login_rate_block_ms;
        state.failures.clear();
    }
}

/// Atomically check and consume one custom operation budget unit.
/// The current request is allowed up to the configured maximum; subsequent
/// requests remain blocked until the block window expires.
pub fn consume_custom_budget(key: &str, config: &AppConfig) -> RateLimitResult {
    let mut store = rate_store().lock().unwrap();
    let now = now_ms();
    let state = store.entry(key.to_string()).or_insert(RateState {
        failures: Vec::new(),
        blocked_until: 0,
        last_seen_at: now,
    });
    state.last_seen_at = now;
    let window_start = now - config.auth_login_rate_window_ms;
    state.failures.retain(|ts| *ts >= window_start);
    if state.blocked_until > now {
        return RateLimitResult {
            allowed: false,
            retry_after_ms: state.blocked_until - now,
        };
    }
    state.blocked_until = 0;
    state.failures.push(now);
    if state.failures.len() >= config.auth_login_rate_max_attempts as usize {
        state.blocked_until = now + config.auth_login_rate_block_ms;
        state.failures.clear();
    }
    RateLimitResult {
        allowed: true,
        retry_after_ms: 0,
    }
}

pub fn record_custom_success(key: &str) {
    let mut store = rate_store().lock().unwrap();
    store.remove(key);
}

#[cfg(test)]
mod identity_session_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use system_admin::FeatureStaff;
    use system_core::{
        ActorIdentity, ExecutionContext, ExecutionMode, Namespace, NoopHttpClient, RequestId,
        Revision, SystemModule, TenantScope,
    };

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

    #[test]
    fn session_persists_only_hash_and_resolves_requested_tenant_authority() {
        let pool = pool();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "INSERT INTO tenants VALUES ('tenant-a','A','a','active','free',NULL,'now','now');
             INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES ('identity-a','alice','hash','Alice','active','now','now');
             INSERT INTO tenant_memberships
             (id,identity_id,tenant_id,role,status,created_at,updated_at)
             VALUES ('membership-a','identity-a','tenant-a','owner','active','now','now');",
        )
        .unwrap();
        drop(conn);

        let session = create_session(&pool, "identity-a").unwrap();
        let stored: String = pool
            .get()
            .unwrap()
            .query_row("SELECT token_hash FROM auth_sessions", [], |row| row.get(0))
            .unwrap();
        assert_ne!(stored, session.id);
        assert_eq!(stored, hash_session_token(&session.id));

        let identity = get_auth_user(&pool, &session.id, Some("tenant-a"))
            .unwrap()
            .unwrap();
        assert!(matches!(
            identity.authority,
            AuthorityContext::Tenant {
                role: TenantRole::Owner,
                ..
            }
        ));
        assert!(
            get_auth_user(&pool, &session.id, Some("tenant-b"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn staff_management_creates_an_identity_that_runtime_auth_can_verify() {
        let pool = pool();
        pool.get()
            .unwrap()
            .execute_batch(
                "INSERT INTO tenants VALUES
                 ('tenant-a','A','a','active','free',NULL,'now','now');",
            )
            .unwrap();

        let tenant_id = TenantId::new("tenant-a").unwrap();
        let ctx = ExecutionContext::new(
            ActorIdentity::with_authority(
                "owner-identity",
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new("owner-membership").unwrap(),
                    tenant_id: tenant_id.clone(),
                    role: TenantRole::Owner,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::new(
                tenant_id,
                Namespace::Production,
                Revision::new("staff-create-test").unwrap(),
            )
            .unwrap(),
            ExecutionMode::Normal,
            RequestId::new("staff-create-request").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap();
        let module = FeatureStaff {
            pool: Mutex::new(Some(pool.clone())),
        };
        module
            .execute(
                "create_tenant_member",
                serde_json::json!({
                    "username": "new-staff",
                    "passwordHash": hash_password("correct-horse-battery-staple"),
                    "role": "staff"
                }),
                &ctx,
            )
            .unwrap();

        let scope = DataScope::production(
            TenantId::new("tenant-a").unwrap(),
            Revision::new("auth-read-test").unwrap(),
        )
        .unwrap();
        let account = find_user_by_username(&pool, &scope, "new-staff")
            .unwrap()
            .unwrap();
        assert_eq!(account.authority_role, "staff");
        assert!(verify_password(
            "correct-horse-battery-staple",
            &account.password_hash
        ));
    }

    #[test]
    fn custom_budget_is_consumed_before_expensive_work() {
        let mut config = AppConfig::from_env();
        config.auth_login_rate_max_attempts = 2;
        config.auth_login_rate_window_ms = 60_000;
        config.auth_login_rate_block_ms = 60_000;
        let key = format!("credential-budget-test:{}", Uuid::new_v4());

        assert!(consume_custom_budget(&key, &config).allowed);
        assert!(consume_custom_budget(&key, &config).allowed);
        let blocked = consume_custom_budget(&key, &config);
        assert!(!blocked.allowed);
        assert!(blocked.retry_after_ms > 0);
    }
}

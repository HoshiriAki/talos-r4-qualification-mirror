use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::AppError;
use crate::utils::time::shanghai_now_iso;

// ── Types ───────────────────────────────────────────────────────

pub struct ApiKeyCreated {
    pub id: String,
    pub name: String,
    pub key: String,        // full key (pk3_...), only returned once
    pub key_prefix: String, // prefix for display (pk3_...)
    pub scopes: String,
    pub enabled: bool,
    pub rate_limit_rpm: i32,
    pub expires_at: Option<String>,
    pub created_at: String,
}

pub struct ApiKeyInfo {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub scopes: String,
    pub rate_limit_rpm: i32,
    pub enabled: bool,
    pub expires_at: Option<String>,
    pub created_by: String,
}

pub struct ApiKeyRow {
    pub id: String,
    pub name: String,
    pub key_prefix: String,
    pub scopes: String,
    pub rate_limit_rpm: i32,
    pub enabled: bool,
    pub expires_at: Option<String>,
    pub last_used_at: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

// ── Key generation ──────────────────────────────────────────────

fn generate_api_key() -> (String, String) {
    // Format: pk3_<32 hex chars>. Prefix = first 12 chars for lookup (pk3_ + 8 hex).
    let random_part: String = (0..16)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect();
    let full_key = format!("pk3_{}", random_part);
    let key_prefix = full_key.chars().take(12).collect::<String>();
    (full_key, key_prefix)
}

fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

// ── CRUD ────────────────────────────────────────────────────────

pub fn create_api_key(
    pool: &Pool<SqliteConnectionManager>,
    name: &str,
    scopes: &str,
    rate_limit_rpm: i32,
    expires_at: Option<&str>,
    created_by: &str,
) -> Result<ApiKeyCreated, AppError> {
    let conn = pool.get()?;
    let id = Uuid::new_v4().to_string();
    let (full_key, key_prefix) = generate_api_key();
    let key_hash = hash_api_key(&full_key);
    let now = shanghai_now_iso();

    let rate_limit = if rate_limit_rpm <= 0 {
        60
    } else {
        rate_limit_rpm
    };

    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, scopes, rate_limit_rpm, expires_at, enabled, created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10)",
        params![
            id,
            name,
            key_hash,
            key_prefix,
            scopes,
            rate_limit,
            expires_at.map(|s| s.to_string()),
            created_by,
            now,
            now,
        ],
    )?;

    Ok(ApiKeyCreated {
        id,
        name: name.to_string(),
        key: full_key,
        key_prefix,
        scopes: scopes.to_string(),
        enabled: true,
        rate_limit_rpm: rate_limit,
        expires_at: expires_at.map(|s| s.to_string()),
        created_at: now,
    })
}

pub fn validate_api_key(
    pool: &Pool<SqliteConnectionManager>,
    token: &str,
) -> Result<Option<ApiKeyInfo>, AppError> {
    let key_hash = hash_api_key(token);
    let conn = pool.get()?;

    let mut stmt = conn.prepare(
        "SELECT id, name, key_prefix, scopes, rate_limit_rpm, enabled, expires_at, created_by
         FROM api_keys WHERE key_hash = ?1 LIMIT 1",
    )?;

    let mut rows = stmt.query_map(params![key_hash], |row| {
        Ok(ApiKeyInfo {
            id: row.get(0)?,
            name: row.get(1)?,
            key_prefix: row.get(2)?,
            scopes: row.get(3)?,
            rate_limit_rpm: row.get::<_, i32>(4)?,
            enabled: row.get::<_, i32>(5)? != 0,
            expires_at: row.get(6)?,
            created_by: row.get(7)?,
        })
    })?;

    match rows.next() {
        Some(Ok(info)) => {
            // Check if enabled
            if !info.enabled {
                return Ok(None);
            }
            // Check if expired
            if let Some(ref exp) = info.expires_at {
                let now = shanghai_now_iso();
                if now >= *exp {
                    return Ok(None);
                }
            }
            // Update last_used_at
            let now = shanghai_now_iso();
            conn.execute(
                "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2",
                params![now, info.id],
            )?;
            Ok(Some(info))
        }
        Some(Err(e)) => Err(AppError::from(e)),
        None => Ok(None),
    }
}

pub fn list_api_keys(pool: &Pool<SqliteConnectionManager>) -> Result<Vec<ApiKeyRow>, AppError> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, key_prefix, scopes, rate_limit_rpm, enabled, expires_at, last_used_at, created_by, created_at, updated_at
         FROM api_keys ORDER BY created_at DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(ApiKeyRow {
            id: row.get(0)?,
            name: row.get(1)?,
            key_prefix: row.get(2)?,
            scopes: row.get(3)?,
            rate_limit_rpm: row.get::<_, i32>(4)?,
            enabled: row.get::<_, i32>(5)? != 0,
            expires_at: row.get(6)?,
            last_used_at: row.get(7)?,
            created_by: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn delete_api_key(
    pool: &Pool<SqliteConnectionManager>,
    id: &str,
    requested_by: &str,
) -> Result<bool, AppError> {
    let conn = pool.get()?;
    let affected = conn.execute(
        "DELETE FROM api_keys WHERE id = ?1 AND created_by = ?2",
        params![id, requested_by],
    )?;
    Ok(affected > 0)
}

pub fn toggle_api_key(
    pool: &Pool<SqliteConnectionManager>,
    id: &str,
    enabled: bool,
    requested_by: &str,
) -> Result<bool, AppError> {
    let conn = pool.get()?;
    let now = shanghai_now_iso();
    let enabled_int: i32 = if enabled { 1 } else { 0 };
    let affected = conn.execute(
        "UPDATE api_keys SET enabled = ?1, updated_at = ?2 WHERE id = ?3 AND created_by = ?4",
        params![enabled_int, now, id, requested_by],
    )?;
    Ok(affected > 0)
}

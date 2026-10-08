use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use regex::Regex;
use rusqlite::params;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::Duration;
use system_core::{ALL_PLATFORM_CAPABILITIES, AuthorityContext, DataScope};
use uuid::Uuid;

use crate::auth_contract::AuthUserInfo;
use crate::error::AppError;
use crate::repositories::{AuditAuthorityWrite, AuditCompatibilityRepository};
use crate::utils::time;

/// Persist a control-plane or tenant-governance event on the caller's current
/// SQLite transaction. Critical mutations use this instead of the buffered
/// compatibility writer so a failed audit insert rolls the mutation back.
pub fn write_authority_event(
    conn: &rusqlite::Connection,
    actor: &AuthUserInfo,
    tenant_id: Option<&str>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    detail: &serde_json::Value,
) -> Result<(), AppError> {
    let (authority_kind, tenant_membership_id, platform_membership_id, roles, capabilities) =
        audit_authority_snapshot(actor, tenant_id)?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO audit_events
         (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
          platform_membership_id, roles_snapshot, capabilities_snapshot, action,
          resource_type, resource_id, correlation_id, detail_json, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            id,
            actor.id,
            authority_kind,
            tenant_id,
            tenant_membership_id,
            platform_membership_id,
            roles,
            capabilities,
            action,
            resource_type,
            resource_id,
            id,
            safe_detail_json(detail),
            time::shanghai_now_iso(),
        ],
    )?;
    Ok(())
}

fn audit_authority_snapshot<'a>(
    actor: &'a AuthUserInfo,
    tenant_id: Option<&str>,
) -> Result<
    (
        &'static str,
        Option<&'a str>,
        Option<&'a str>,
        String,
        String,
    ),
    AppError,
> {
    match &actor.authority {
        AuthorityContext::Tenant {
            membership_id,
            tenant_id: authority_tenant_id,
            role,
        } => {
            if tenant_id != Some(authority_tenant_id.as_str()) {
                return Err(AppError::Forbidden);
            }
            Ok((
                "tenant",
                Some(membership_id.as_str()),
                None,
                serde_json::to_string(&[*role])?,
                "[]".to_string(),
            ))
        }
        authority @ AuthorityContext::Platform {
            membership_id,
            roles,
        } => {
            let capabilities: Vec<_> = ALL_PLATFORM_CAPABILITIES
                .iter()
                .copied()
                .filter(|capability| authority.allows(*capability))
                .collect();
            Ok((
                "platform",
                None,
                Some(membership_id.as_str()),
                serde_json::to_string(roles)?,
                serde_json::to_string(&capabilities)?,
            ))
        }
    }
}

// ── Regex ──────────────────────────────────────────────────────────

static DATE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap());

// ── Audit buffer channel (lazy init) ───────────────────────────────

const MAX_BATCH_SIZE: usize = 50;
const FLUSH_INTERVAL_SECS: u64 = 5;
const CHANNEL_CAPACITY: usize = 2000;

struct BufferedAuditEntry {
    id: String,
    actor_identity_id: String,
    actor_username: String,
    action_type: String,
    entity_type: String,
    entity_id: String,
    entity_label: String,
    detail_json: String,
    ip: String,
    user_agent: String,
    created_at: String,
}

/// Global sender wrapped in a Mutex<Option> so shutdown can drop the sender
/// to signal the flusher thread to exit.
static AUDIT_TX: LazyLock<Mutex<Option<SyncSender<BufferedAuditEntry>>>> =
    LazyLock::new(|| Mutex::new(None));

fn ensure_sender(pool: &Pool<SqliteConnectionManager>) {
    let mut guard = AUDIT_TX.lock().expect("audit tx lock poisoned");
    if guard.is_none() {
        let (tx, rx) = mpsc::sync_channel::<BufferedAuditEntry>(CHANNEL_CAPACITY);
        let flusher_pool = pool.clone();

        thread::Builder::new()
            .name("audit-flusher".into())
            .spawn(move || {
                audit_flusher_thread(&flusher_pool, rx);
            })
            .expect("failed to spawn audit flusher thread");

        tracing::info!("[audit-buffer] background flusher started");
        *guard = Some(tx);
    }
}

fn try_send_entry(entry: BufferedAuditEntry) {
    let guard = AUDIT_TX.lock().expect("audit tx lock poisoned");
    if let Some(ref tx) = *guard
        && tx.try_send(entry).is_err()
    {
        tracing::warn!("[audit-buffer] channel full — entry dropped");
    }
    // If sender is None (shutdown), silently drop
}

fn audit_flusher_thread(
    pool: &Pool<SqliteConnectionManager>,
    rx: mpsc::Receiver<BufferedAuditEntry>,
) {
    let mut buffer: Vec<BufferedAuditEntry> = Vec::with_capacity(MAX_BATCH_SIZE);

    loop {
        match rx.recv_timeout(Duration::from_secs(FLUSH_INTERVAL_SECS)) {
            Ok(entry) => {
                buffer.push(entry);
                if buffer.len() >= MAX_BATCH_SIZE {
                    flush_batch(pool, &mut buffer);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !buffer.is_empty() {
                    flush_batch(pool, &mut buffer);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if !buffer.is_empty() {
                    flush_batch(pool, &mut buffer);
                }
                tracing::info!("[audit-buffer] flusher thread exiting (channel closed)");
                break;
            }
        }
    }
}

fn flush_batch(pool: &Pool<SqliteConnectionManager>, buffer: &mut Vec<BufferedAuditEntry>) {
    let batch: Vec<BufferedAuditEntry> = std::mem::take(buffer);

    let conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[audit-buffer] failed to get connection: {}", e);
            return;
        }
    };

    if let Err(e) = conn.execute_batch("BEGIN IMMEDIATE") {
        tracing::error!("[audit-buffer] BEGIN failed: {}", e);
        return;
    }

    let mut stmt = match conn.prepare(
        "INSERT INTO audit_events
         (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
          platform_membership_id, roles_snapshot, capabilities_snapshot, action,
          resource_type, resource_id, correlation_id, detail_json, occurred_at)
         VALUES (?1, NULL, 'system', NULL, NULL, NULL, '[]', '[]',
                 ?2, ?3, NULLIF(?4, ''), ?1, ?5, ?6)",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("[audit-buffer] prepare failed: {}", e);
            let _ = conn.execute_batch("ROLLBACK");
            return;
        }
    };

    for entry in &batch {
        if let Err(e) = stmt.execute(params![
            entry.id,
            entry.action_type,
            entry.entity_type,
            entry.entity_id,
            entry.detail_json,
            entry.created_at,
        ]) {
            tracing::error!("[audit-buffer] insert failed: {}", e);
        }
    }

    if let Err(e) = conn.execute_batch("COMMIT") {
        tracing::error!("[audit-buffer] COMMIT failed: {}", e);
        let _ = conn.execute_batch("ROLLBACK");
        return;
    }

    tracing::debug!("[audit-buffer] flushed {} entries", batch.len());
}

// ── Safe JSON helpers ─────────────────────────────────────────────

const AUDIT_DETAIL_BYTES_MAX: usize = 20_000;
const REDACTED_SECRET: &str = "[REDACTED_SECRET]";
const REDACTED_PII: &str = "[REDACTED_PII]";

fn normalized_audit_key(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_opaque_secret_reference_key(key: &str) -> bool {
    key.ends_with("credentialid")
        || key.ends_with("credentialref")
        || key.ends_with("credentialreference")
        || key.ends_with("secretid")
        || key.ends_with("secretref")
        || key.ends_with("secretreference")
}

fn is_secret_audit_key(key: &str) -> bool {
    let key = normalized_audit_key(key);
    let opaque_reference = is_opaque_secret_reference_key(&key);
    let secret_material = key.contains("secret") && !opaque_reference;
    let credential_material = key.contains("credential") && !opaque_reference;
    key.contains("password")
        || secret_material
        || key.contains("session")
        || key.contains("bearer")
        || credential_material
        || key.contains("token")
        || key == "authorization"
        || key == "cookie"
        || key == "setcookie"
        || key == "apikey"
        || key.ends_with("apikey")
        || key == "totpcode"
        || key == "otpcode"
        || key == "mfacode"
        || key == "otpauthurl"
        || key == "privatekey"
        || key == "signingkey"
}

fn is_restricted_pii_audit_key(key: &str) -> bool {
    matches!(
        normalized_audit_key(key).as_str(),
        "email"
            | "phone"
            | "phonenumber"
            | "mobile"
            | "mobilephone"
            | "customerphone"
            | "customeremail"
            | "recipientphone"
            | "address"
            | "shippingaddress"
            | "billingaddress"
            | "recipientaddress"
            | "customername"
            | "recipientname"
            | "contactname"
            | "idcard"
            | "idnumber"
            | "passport"
            | "bankaccount"
            | "accountnumber"
            | "cardnumber"
            | "notes"
            | "adminnotes"
    )
}

fn secret_shaped_string(value: &str) -> bool {
    let trimmed = value.trim();
    let lowercase = trimmed.to_ascii_lowercase();
    lowercase.starts_with("bearer ")
        || lowercase.starts_with("otpauth://")
        || (trimmed.starts_with("mapi_") && trimmed.contains('.'))
}

fn redact_audit_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(key, value)| {
                    let redacted = if is_secret_audit_key(key) {
                        serde_json::Value::String(REDACTED_SECRET.to_string())
                    } else if is_restricted_pii_audit_key(key) {
                        serde_json::Value::String(REDACTED_PII.to_string())
                    } else {
                        redact_audit_value(value)
                    };
                    (key.clone(), redacted)
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(redact_audit_value).collect())
        }
        serde_json::Value::String(value) if secret_shaped_string(value) => {
            serde_json::Value::String(REDACTED_SECRET.to_string())
        }
        _ => value.clone(),
    }
}

pub fn safe_detail_json(detail: &serde_json::Value) -> String {
    if !detail.is_object() && !detail.is_array() {
        return "{}".to_string();
    }

    let redacted = redact_audit_value(detail);
    let text = serde_json::to_string(&redacted).unwrap_or_else(|_| "{}".to_string());
    if text.len() <= AUDIT_DETAIL_BYTES_MAX {
        return text;
    }

    serde_json::json!({
        "warning": "detail_too_large",
        "redactedBytes": text.len(),
        "redactedSha256": hex::encode(Sha256::digest(text.as_bytes())),
    })
    .to_string()
}

pub fn try_parse_json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::Object(Default::default()))
}

// ── Write audit log (buffered) ─────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLog {
    pub id: String,
    pub actor_identity_id: String,
    pub actor_username: String,
    pub action_type: String,
    pub entity_type: String,
    pub entity_id: String,
    pub entity_label: String,
    pub detail_json: String,
    pub ip: String,
    pub user_agent: String,
    pub created_at: String,
    #[serde(default)]
    pub detail: serde_json::Value,
}

/// Writes a structured event using the request's already-resolved authority.
/// This compatibility name remains while route call sites are consolidated;
/// it never infers authority from database memberships.
#[allow(clippy::too_many_arguments)]
pub fn write_audit_log(
    pool: &Pool<SqliteConnectionManager>,
    action_type: &str,
    entity_type: &str,
    entity_id: &str,
    entity_label: &str,
    detail: &serde_json::Value,
    actor: &AuthUserInfo,
) -> Result<(), AppError> {
    if action_type.is_empty() || entity_type.is_empty() {
        return Ok(());
    }
    let conn = pool.get()?;
    let detail = serde_json::json!({
        "label": entity_label,
        "detail": detail,
    });
    write_authority_event(
        &conn,
        actor,
        actor.tenant_id(),
        action_type,
        entity_type,
        (!entity_id.is_empty()).then_some(entity_id),
        &detail,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn write_audit_log_with_repository(
    repository: &AuditCompatibilityRepository,
    action_type: &str,
    entity_type: &str,
    entity_id: &str,
    entity_label: &str,
    detail: &serde_json::Value,
    actor: &AuthUserInfo,
) -> Result<(), AppError> {
    if action_type.is_empty() || entity_type.is_empty() {
        return Ok(());
    }
    let tenant_id = actor.tenant_id();
    let (authority_kind, tenant_membership_id, platform_membership_id, roles, capabilities) =
        audit_authority_snapshot(actor, tenant_id)?;
    let id = Uuid::new_v4().to_string();
    let detail = serde_json::json!({
        "label": entity_label,
        "detail": detail,
    });
    repository
        .append_authority_event(&AuditAuthorityWrite {
            id: id.clone(),
            actor_identity_id: actor.id.clone(),
            authority_kind: authority_kind.to_string(),
            tenant_id: tenant_id.map(str::to_string),
            tenant_membership_id: tenant_membership_id.map(str::to_string),
            platform_membership_id: platform_membership_id.map(str::to_string),
            roles_snapshot: roles,
            capabilities_snapshot: capabilities,
            action: action_type.to_string(),
            resource_type: entity_type.to_string(),
            resource_id: (!entity_id.is_empty()).then(|| entity_id.to_string()),
            correlation_id: id,
            detail_json: safe_detail_json(&detail),
            occurred_at: time::shanghai_now_iso(),
        })
        .map_err(|error| {
            tracing::error!(
                repository_error_code = error.code(),
                "authority audit repository write failed"
            );
            AppError::Internal("audit repository unavailable".into())
        })
}

// ── Query audit logs ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AuditLogQueryResult {
    pub logs: Vec<AuditLog>,
    pub limit: i64,
    pub total: i64,
}

pub struct AuditLogFilter {
    pub action_type: String,
    pub entity_type: String,
    pub actor_username: String,
    pub keyword: String,
    pub date: String,
    pub limit: i64,
    pub offset: i64,
}

impl Default for AuditLogFilter {
    fn default() -> Self {
        Self {
            action_type: String::new(),
            entity_type: String::new(),
            actor_username: String::new(),
            keyword: String::new(),
            date: String::new(),
            limit: 100,
            offset: 0,
        }
    }
}

pub fn query_audit_logs(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    filter: &AuditLogFilter,
) -> Result<AuditLogQueryResult, AppError> {
    let action_type = filter.action_type.trim();
    let entity_type = filter.entity_type.trim();
    let actor_username = filter.actor_username.trim();
    let keyword = filter.keyword.trim();
    let date = filter.date.trim();
    let limit = filter.limit;

    let mut where_clauses: Vec<String> = vec!["tenant_id = ?".to_string()];
    let mut values: Vec<String> = vec![scope.tenant_id().as_str().to_owned()];

    if !action_type.is_empty() {
        where_clauses.push("actionType = ?".to_string());
        values.push(action_type.to_string());
    }
    if !entity_type.is_empty() {
        where_clauses.push("entityType = ?".to_string());
        values.push(entity_type.to_string());
    }
    if !actor_username.is_empty() {
        where_clauses.push("actorUsername LIKE ?".to_string());
        values.push(format!("%{}%", actor_username));
    }
    if !keyword.is_empty() {
        let like = format!("%{}%", keyword);
        where_clauses
            .push("(entityLabel LIKE ? OR actorUsername LIKE ? OR detailJson LIKE ?)".to_string());
        values.push(like.clone());
        values.push(like.clone());
        values.push(like);
    }
    if DATE_REGEX.is_match(date) {
        where_clauses.push("createdAt LIKE ?".to_string());
        values.push(format!("{}%", date));
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let offset = filter.offset;

    // Count total (before moving `values`)
    let count_sql = format!("SELECT COUNT(1) FROM audit_logs {}", where_sql);
    let conn = pool.get()?;
    let mut count_stmt = conn.prepare(&count_sql)?;
    let count_boxed: Vec<Box<dyn rusqlite::types::ToSql>> = values
        .iter()
        .map(|v| Box::new(v.clone()) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    let count_refs: Vec<&dyn rusqlite::types::ToSql> =
        count_boxed.iter().map(|b| b.as_ref()).collect();
    let total: i64 = count_stmt
        .query_row(count_refs.as_slice(), |row| row.get(0))
        .unwrap_or(0);

    // Data query
    let sql = format!(
        "SELECT id, actorIdentityId, actorUsername, actionType, entityType, entityId,
                entityLabel, detailJson, ip, userAgent, createdAt
         FROM audit_logs
         {}
         ORDER BY createdAt DESC
         LIMIT ? OFFSET ?",
        where_sql
    );

    let mut stmt = conn.prepare(&sql)?;

    let mut boxed: Vec<Box<dyn rusqlite::types::ToSql>> = values
        .into_iter()
        .map(|v| Box::new(v) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    boxed.push(Box::new(limit));
    boxed.push(Box::new(offset));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = boxed.iter().map(|b| b.as_ref()).collect();

    let mut logs: Vec<AuditLog> = stmt
        .query_map(param_refs.as_slice(), |row| {
            let detail_json: String = row.get::<_, String>(7).unwrap_or_default();
            let detail = try_parse_json(&detail_json);
            Ok(AuditLog {
                id: row.get(0)?,
                actor_identity_id: row.get::<_, String>(1).unwrap_or_default(),
                actor_username: row.get::<_, String>(2).unwrap_or_default(),
                action_type: row.get(3)?,
                entity_type: row.get(4)?,
                entity_id: row.get::<_, String>(5).unwrap_or_default(),
                entity_label: row.get::<_, String>(6).unwrap_or_default(),
                detail_json,
                ip: row.get::<_, String>(8).unwrap_or_default(),
                user_agent: row.get::<_, String>(9).unwrap_or_default(),
                created_at: row.get::<_, String>(10).unwrap_or_default(),
                detail,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    // Parse detail JSON for each log
    for log in &mut logs {
        log.detail = try_parse_json(&log.detail_json);
    }

    Ok(AuditLogQueryResult { logs, limit, total })
}

// ── Shutdown ───────────────────────────────────────────────────────

/// Flush any remaining buffered entries and stop the background thread.
/// Call this before process exit. Drops the sender to signal the flusher
/// thread to drain and exit.
pub fn shutdown_audit_buffer() {
    let mut guard = AUDIT_TX.lock().expect("audit tx lock poisoned");
    if guard.is_some() {
        tracing::info!("[audit-buffer] shutting down — dropping sender to drain flusher");
        // Drop the sender — the flusher thread will receive Disconnected,
        // flush remaining entries, and exit.
        *guard = None;
    }
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;
    use system_core::{Revision, TenantId};

    #[test]
    fn audit_detail_redacts_credentials_and_restricted_pii_recursively() {
        let detail = serde_json::json!({
            "password": "correct-horse-battery-staple",
            "authorization": "Bearer session-secret",
            "credentialId": "cred-123",
            "providerCredentialRef": "provider-credential-ref-456",
            "providerSecretRef": "opaque-ref-789",
            "credential": "raw-credential",
            "credentialValue": "raw-credential-value",
            "credentialMaterial": "raw-credential-material",
            "nested": {
                "sessionToken": "session-token",
                "totpCode": "123456",
                "providerSecret": "provider-secret",
                "apiKey": "api-key-secret",
                "otpauthUrl": "otpauth://totp/Talos:user?secret=ABCDEF",
                "customerName": "Alice Example",
                "phone": "+86 13800000000",
                "address": "sensitive address",
                "notes": "free-form sensitive note",
                "safeStatus": "active"
            }
        });

        let text = safe_detail_json(&detail);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["password"], REDACTED_SECRET);
        assert_eq!(value["authorization"], REDACTED_SECRET);
        assert_eq!(value["credentialId"], "cred-123");
        assert_eq!(
            value["providerCredentialRef"],
            "provider-credential-ref-456"
        );
        assert_eq!(value["providerSecretRef"], "opaque-ref-789");
        assert_eq!(value["credential"], REDACTED_SECRET);
        assert_eq!(value["credentialValue"], REDACTED_SECRET);
        assert_eq!(value["credentialMaterial"], REDACTED_SECRET);
        assert_eq!(value["nested"]["sessionToken"], REDACTED_SECRET);
        assert_eq!(value["nested"]["totpCode"], REDACTED_SECRET);
        assert_eq!(value["nested"]["providerSecret"], REDACTED_SECRET);
        assert_eq!(value["nested"]["apiKey"], REDACTED_SECRET);
        assert_eq!(value["nested"]["otpauthUrl"], REDACTED_SECRET);
        assert_eq!(value["nested"]["customerName"], REDACTED_PII);
        assert_eq!(value["nested"]["phone"], REDACTED_PII);
        assert_eq!(value["nested"]["address"], REDACTED_PII);
        assert_eq!(value["nested"]["notes"], REDACTED_PII);
        assert_eq!(value["nested"]["safeStatus"], "active");
        for forbidden in [
            "correct-horse-battery-staple",
            "session-secret",
            "raw-credential",
            "raw-credential-value",
            "raw-credential-material",
            "session-token",
            "123456",
            "provider-secret",
            "api-key-secret",
            "ABCDEF",
            "Alice Example",
            "13800000000",
            "sensitive address",
            "free-form sensitive note",
        ] {
            assert!(!text.contains(forbidden));
        }
    }

    #[test]
    fn opaque_reference_with_secret_shaped_value_is_still_redacted() {
        let detail = serde_json::json!({ "credentialRef": "Bearer raw-secret" });
        let text = safe_detail_json(&detail);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["credentialRef"], REDACTED_SECRET);
        assert!(!text.contains("raw-secret"));
    }

    #[test]
    fn oversized_audit_detail_keeps_only_redacted_digest_metadata() {
        let detail =
            serde_json::json!({ "safePayload": "x".repeat(AUDIT_DETAIL_BYTES_MAX + 1000) });
        let text = safe_detail_json(&detail);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["warning"], "detail_too_large");
        assert!(value["redactedBytes"].as_u64().unwrap() > AUDIT_DETAIL_BYTES_MAX as u64);
        assert_eq!(value["redactedSha256"].as_str().unwrap().len(), 64);
        assert!(value.get("preview").is_none());
        assert!(!text.contains(&"x".repeat(128)));
    }

    #[test]
    fn legacy_audit_query_never_crosses_data_scope() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .expect("in-memory audit pool");
        pool.get()
            .expect("audit connection")
            .execute_batch(
                "CREATE TABLE audit_logs(
                    id TEXT PRIMARY KEY, actorIdentityId TEXT, actorUsername TEXT,
                    actionType TEXT, entityType TEXT, entityId TEXT,
                    entityLabel TEXT, detailJson TEXT, ip TEXT, userAgent TEXT,
                    createdAt TEXT, tenant_id TEXT NOT NULL
                );
                INSERT INTO audit_logs VALUES
                  ('a','u-a','A','read','device','d-a','','{}','','','2026-07-19T10:00:00+08:00','tenant-a'),
                  ('b','u-b','B','read','device','d-b','','{}','','','2026-07-19T11:00:00+08:00','tenant-b');",
            )
            .expect("audit fixture");
        let scope = DataScope::production(
            TenantId::new("tenant-a").expect("tenant id"),
            Revision::new("audit-test").expect("revision"),
        )
        .expect("production scope");

        let result = query_audit_logs(&pool, &scope, &AuditLogFilter::default())
            .expect("scoped audit query");

        assert_eq!(result.total, 1);
        assert_eq!(result.logs.len(), 1);
        assert_eq!(result.logs[0].id, "a");
    }
}

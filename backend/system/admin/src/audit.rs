//! feature-audit — 审计日志模块（Maxwell 原生）
//!
//! 异步缓冲写入 + 查询。核心移植自 `backend/src/services/audit_service.rs`，
//! 作为独立的 Maxwell `SystemModule` crate 运行。
//!
//! ## 命令
//! - `write_audit_log` — 异步缓冲写入审计日志
//! - `list_audit_logs` — 带可选过滤器的分页查询

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use regex::Regex;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

// ── System Core 类型 ─────────────────────────────────────────────────────
use system_core::{
    ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, Sanitize, SystemModule, Validate,
    ValidationResult,
};

// ══════════════════════════════════════════════════════════════════════════
// 上海时区辅助
// ══════════════════════════════════════════════════════════════════════════

/// 返回上海时间 (UTC+8) 的 ISO 8601 字符串
fn shanghai_now_iso() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y-%m-%dT%H:%M:%S%.3f+08:00").to_string()
}

// ══════════════════════════════════════════════════════════════════════════
// 输入类型
// ══════════════════════════════════════════════════════════════════════════

/// 写入审计日志的输入
/// actor 身份从 ExecutionContext 获取；ip/userAgent 由传输层提供
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteAuditLogInput {
    pub action_type: String,
    pub entity_type: String,
    pub entity_id: String,
    pub entity_label: String,
    #[serde(default)]
    pub detail_json: String,
}

impl Sanitize for WriteAuditLogInput {
    fn sanitize(&mut self) {
        self.action_type = self.action_type.trim().to_string();
        self.entity_type = self.entity_type.trim().to_string();
        self.entity_id = self.entity_id.trim().to_string();
        self.entity_label = self.entity_label.trim().to_string();
        self.detail_json = self.detail_json.trim().to_string();
    }
}

impl Validate for WriteAuditLogInput {
    fn validate(&self) -> ValidationResult {
        use system_core::FieldError;

        let mut errors: Vec<FieldError> = Vec::new();

        if self.action_type.is_empty() {
            errors.push(FieldError {
                field: "actionType".to_string(),
                code: "VAL_REQUIRED".to_string(),
                message: "actionType is required".to_string(),
            });
        }
        if self.entity_type.is_empty() {
            errors.push(FieldError {
                field: "entityType".to_string(),
                code: "VAL_REQUIRED".to_string(),
                message: "entityType is required".to_string(),
            });
        }

        ValidationResult { errors }
    }
}

/// 列出审计日志的查询参数
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListAuditLogsInput {
    #[serde(default)]
    pub actor_identity_id: String,
    #[serde(default)]
    pub actor_username: String,
    #[serde(default)]
    pub action_type: String,
    #[serde(default)]
    pub entity_type: String,
    #[serde(default)]
    pub keyword: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub start_date: String,
    #[serde(default)]
    pub end_date: String,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

impl Sanitize for ListAuditLogsInput {
    fn sanitize(&mut self) {
        self.actor_identity_id = self.actor_identity_id.trim().to_string();
        self.actor_username = self.actor_username.trim().to_string();
        self.action_type = self.action_type.trim().to_string();
        self.entity_type = self.entity_type.trim().to_string();
        self.keyword = self.keyword.trim().to_string();
        self.date = self.date.trim().to_string();
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
        if self.limit <= 0 {
            self.limit = 100;
        }
        if self.limit > 1000 {
            self.limit = 1000;
        }
        if self.offset < 0 {
            self.offset = 0;
        }
        if let Some(value) = &mut self.sort_by {
            *value = value.trim().to_string();
        }
        if let Some(value) = &mut self.sort_order {
            *value = value.trim().to_ascii_lowercase();
        }
    }
}

impl Validate for ListAuditLogsInput {
    fn validate(&self) -> ValidationResult {
        // All fields are optional — always valid
        ValidationResult { errors: vec![] }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// 输出类型
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteAuditLogOutput {
    pub success: bool,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLogEntry {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAuditLogsOutput {
    pub logs: Vec<AuditLogEntry>,
    pub limit: i64,
    pub total: i64,
}

// ══════════════════════════════════════════════════════════════════════════
// 缓冲审计写入
// ══════════════════════════════════════════════════════════════════════════

const MAX_BATCH_SIZE: usize = 50;
const FLUSH_INTERVAL_SECS: u64 = 5;
const CHANNEL_CAPACITY: usize = 2000;

/// 日期格式验证正则: YYYY-MM-DD
static DATE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap());

struct BufferedAuditEntry {
    id: String,
    tenant_id: String,
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

/// 全局发送器，包装在 `Mutex<Option>` 中，关闭时删除发送端以通知刷新线程。
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

        tracing::info!("[feature-audit] background flusher started");
        *guard = Some(tx);
    }
}

fn try_send_entry(entry: BufferedAuditEntry) {
    let guard = AUDIT_TX.lock().expect("audit tx lock poisoned");
    if let Some(ref tx) = *guard
        && tx.try_send(entry).is_err()
    {
        tracing::warn!("[feature-audit] channel full — entry dropped");
    }
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
                tracing::info!("[feature-audit] flusher thread exiting (channel closed)");
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
            tracing::error!("[feature-audit] failed to get connection: {}", e);
            return;
        }
    };

    if let Err(e) = conn.execute_batch("BEGIN IMMEDIATE") {
        tracing::error!("[feature-audit] BEGIN failed: {}", e);
        return;
    }

    let mut stmt = match conn.prepare(
        "INSERT INTO audit_logs (id, actorIdentityId, actorUsername, actionType, entityType, entityId, \
         entityLabel, detailJson, ip, userAgent, createdAt, tenant_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("[feature-audit] prepare failed: {}", e);
            let _ = conn.execute_batch("ROLLBACK");
            return;
        }
    };

    for entry in &batch {
        if let Err(e) = stmt.execute(params![
            entry.id,
            entry.actor_identity_id,
            entry.actor_username,
            entry.action_type,
            entry.entity_type,
            entry.entity_id,
            entry.entity_label,
            entry.detail_json,
            entry.ip,
            entry.user_agent,
            entry.created_at,
            entry.tenant_id,
        ]) {
            tracing::error!("[feature-audit] insert failed: {}", e);
        }
    }

    if let Err(e) = conn.execute_batch("COMMIT") {
        tracing::error!("[feature-audit] COMMIT failed: {}", e);
        let _ = conn.execute_batch("ROLLBACK");
        return;
    }

    tracing::debug!("[feature-audit] flushed {} entries", batch.len());
}

// ══════════════════════════════════════════════════════════════════════════
// Safe JSON helpers
// ══════════════════════════════════════════════════════════════════════════

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

pub fn safe_audit_detail_json(detail: &serde_json::Value) -> String {
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

fn try_parse_json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::Object(Default::default()))
}

fn tenant_filter(ctx: &ExecutionContext) -> (Vec<String>, Vec<String>) {
    (
        vec!["tenant_id = ?".to_string()],
        vec![ctx.data_scope().tenant_id().as_str().to_owned()],
    )
}

// ══════════════════════════════════════════════════════════════════════════
// SystemModule 实现
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureAudit {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureAudit {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureAudit {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureAudit {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "feature-audit".to_string(),
            version: "0.1.0".to_string(),
            description: "审计日志模块 — 异步缓冲写入 + 查询".to_string(),
            author: "Maxwell".to_string(),
            wasm_compatible: false,
            storage: Some("required".to_string()),
        }
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        let database_url = config
            .get("databaseUrl")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_CONFIG".into(),
                    message: "config.databaseUrl is required".into(),
                    field: Some("databaseUrl".into()),
                    context: None,
                })
                .unwrap_or_default()
            })?;

        let manager = SqliteConnectionManager::file(database_url);
        let pool = Pool::builder().max_size(4).build(manager).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_POOL".into(),
                message: format!("failed to create pool: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;

        ensure_sender(&pool);

        let mut guard = self.pool.lock().map_err(|e| e.to_string())?;
        *guard = Some(pool);

        tracing::info!("[feature-audit] initialized");
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "write_audit_log" => {
                // 类型状态管线: Unvalidated → Sanitized → Validated
                let unvalidated: system_core::Unvalidated<WriteAuditLogInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();

                let pool_guard = self.pool.lock().map_err(|e| e.to_string())?;
                let pool = pool_guard.as_ref().ok_or_else(|| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_NOT_INIT".into(),
                        message: "FeatureAudit not initialized. Call init() first.".into(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })?;

                let id = Uuid::new_v4().to_string();
                let detail_json = safe_audit_detail_json(
                    &serde_json::from_str(&input.detail_json)
                        .unwrap_or(serde_json::Value::Object(Default::default())),
                );
                let now = shanghai_now_iso();

                // Actor identity from ExecutionContext (server-side), not client input
                let actor_identity_id = ctx
                    .user_id()
                    .map(str::to_owned)
                    .unwrap_or_else(|| "system".into());
                let actor_username = ctx.actor().id().unwrap_or("system").to_string();

                let entry = BufferedAuditEntry {
                    id: id.clone(),
                    tenant_id: ctx.data_scope().tenant_id().as_str().to_owned(),
                    actor_identity_id,
                    actor_username,
                    action_type: input.action_type,
                    entity_type: input.entity_type,
                    entity_id: input.entity_id,
                    entity_label: input.entity_label,
                    detail_json,
                    ip: String::new(),
                    user_agent: String::new(),
                    created_at: now,
                };

                ensure_sender(pool);
                try_send_entry(entry);

                let output = WriteAuditLogOutput { success: true, id };

                Ok(serde_json::to_value(output).map_err(|e| e.to_string())?)
            }

            "list_audit_logs" => {
                let unvalidated: system_core::Unvalidated<ListAuditLogsInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();

                let pool_guard = self.pool.lock().map_err(|e| e.to_string())?;
                let pool = pool_guard.as_ref().ok_or_else(|| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_NOT_INIT".into(),
                        message: "FeatureAudit not initialized. Call init() first.".into(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })?;

                // 构建 WHERE 子句
                let (mut where_clauses, mut values) = tenant_filter(ctx);

                if !input.actor_identity_id.is_empty() {
                    where_clauses.push("actorIdentityId = ?".to_string());
                    values.push(input.actor_identity_id);
                }
                if !input.actor_username.is_empty() {
                    where_clauses.push("actorUsername LIKE ?".to_string());
                    values.push(format!("%{}%", input.actor_username));
                }
                if !input.action_type.is_empty() {
                    where_clauses.push("actionType = ?".to_string());
                    values.push(input.action_type);
                }
                if !input.entity_type.is_empty() {
                    where_clauses.push("entityType = ?".to_string());
                    values.push(input.entity_type);
                }
                if !input.keyword.is_empty() {
                    where_clauses.push(
                        "(entityLabel LIKE ? OR actorUsername LIKE ? OR detailJson LIKE ?)"
                            .to_string(),
                    );
                    let like = format!("%{}%", input.keyword);
                    values.push(like.clone());
                    values.push(like.clone());
                    values.push(like);
                }
                if DATE_REGEX.is_match(&input.date) {
                    where_clauses.push("createdAt >= ?".to_string());
                    values.push(format!("{}T00:00:00+08:00", input.date));
                    where_clauses.push("createdAt <= ?".to_string());
                    values.push(format!("{}T23:59:59.999+08:00", input.date));
                }
                if DATE_REGEX.is_match(&input.start_date) {
                    where_clauses.push("createdAt >= ?".to_string());
                    values.push(input.start_date);
                }
                if DATE_REGEX.is_match(&input.end_date) {
                    where_clauses.push("createdAt <= ?".to_string());
                    values.push(format!("{}T23:59:59.999+08:00", input.end_date));
                }

                let where_sql = if where_clauses.is_empty() {
                    String::new()
                } else {
                    format!("WHERE {}", where_clauses.join(" AND "))
                };

                // 统计总数
                let count_sql = format!("SELECT COUNT(1) FROM audit_logs {}", where_sql);
                let conn = pool.get().map_err(|e| e.to_string())?;
                let mut count_stmt = conn.prepare(&count_sql).map_err(|e| e.to_string())?;
                let count_boxed: Vec<Box<dyn rusqlite::types::ToSql>> = values
                    .iter()
                    .map(|v| Box::new(v.clone()) as Box<dyn rusqlite::types::ToSql>)
                    .collect();
                let count_refs: Vec<&dyn rusqlite::types::ToSql> =
                    count_boxed.iter().map(|b| b.as_ref()).collect();
                let total: i64 = count_stmt
                    .query_row(count_refs.as_slice(), |row| row.get(0))
                    .unwrap_or(0);

                // 数据查询
                let sort_column = match input.sort_by.as_deref() {
                    Some("actorUsername") => "actorUsername",
                    Some("actionType") => "actionType",
                    Some("entityType") => "entityType",
                    Some("entityLabel") => "entityLabel",
                    _ => "createdAt",
                };
                let sort_order = if input.sort_order.as_deref() == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                };
                let sql = format!(
                    "SELECT id, actorIdentityId, actorUsername, actionType, entityType, entityId,
                            entityLabel, detailJson, ip, userAgent, createdAt
                     FROM audit_logs
                     {}
                     ORDER BY {} {}, id ASC
                     LIMIT ? OFFSET ?",
                    where_sql, sort_column, sort_order
                );

                let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

                let mut boxed: Vec<Box<dyn rusqlite::types::ToSql>> = values
                    .into_iter()
                    .map(|v| Box::new(v) as Box<dyn rusqlite::types::ToSql>)
                    .collect();
                boxed.push(Box::new(input.limit));
                boxed.push(Box::new(input.offset));
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    boxed.iter().map(|b| b.as_ref()).collect();

                let mut logs: Vec<AuditLogEntry> = stmt
                    .query_map(param_refs.as_slice(), |row| {
                        Ok(AuditLogEntry {
                            id: row.get(0)?,
                            actor_identity_id: row.get::<_, String>(1).unwrap_or_default(),
                            actor_username: row.get::<_, String>(2).unwrap_or_default(),
                            action_type: row.get(3)?,
                            entity_type: row.get(4)?,
                            entity_id: row.get::<_, String>(5).unwrap_or_default(),
                            entity_label: row.get::<_, String>(6).unwrap_or_default(),
                            detail_json: row.get::<_, String>(7).unwrap_or_default(),
                            ip: row.get::<_, String>(8).unwrap_or_default(),
                            user_agent: row.get::<_, String>(9).unwrap_or_default(),
                            created_at: row.get::<_, String>(10).unwrap_or_default(),
                        })
                    })
                    .map_err(|e| e.to_string())?
                    .filter_map(|r| r.ok())
                    .collect();

                // 解析 detail JSON
                for log in &mut logs {
                    let _ = try_parse_json(&log.detail_json);
                    // We keep the raw detail_json string; the caller parses if needed
                }

                let output = ListAuditLogsOutput {
                    logs,
                    limit: input.limit,
                    total,
                };

                Ok(serde_json::to_value(output).map_err(|e| e.to_string())?)
            }

            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("unknown command: {}", command),
                field: Some("command".into()),
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        vec![
            system_core::CommandMetadata::new(
                "write_audit_log",
                system_core::AccessRequirement::System,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "list_audit_logs",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        use system_core::CommandSchema;

        ModuleSchema {
            name: "feature-audit".to_string(),
            description: "审计日志模块 — 异步缓冲写入 + 查询".to_string(),
            commands: vec![
                CommandSchema {
                    name: "write_audit_log".to_string(),
                    description: "写入一条审计日志（异步缓冲，非阻塞）".to_string(),
                    version: "0.1.0".to_string(),
                    input_schema: Some(serde_json::json!({
                        "type": "object",
                        "required": ["actionType", "entityType"],
                        "properties": {
                            "actorIdentityId": {"type": "string", "description": "操作者用户 ID"},
                            "actorUsername": {"type": "string", "description": "操作者用户名"},
                            "actionType": {"type": "string", "description": "操作类型，如 create、update、delete"},
                            "entityType": {"type": "string", "description": "实体类型，如 order、device"},
                            "entityId": {"type": "string", "description": "实体 ID"},
                            "entityLabel": {"type": "string", "description": "实体可读标签"},
                            "detailJson": {"type": "string", "description": "JSON 详情"},
                            "ip": {"type": "string", "description": "客户端 IP"},
                            "userAgent": {"type": "string", "description": "客户端 User-Agent"},
                        },
                    })),
                    output_schema: Some(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "success": {"type": "boolean"},
                            "id": {"type": "string", "description": "审计日志 UUID"},
                        },
                    })),
                },
                CommandSchema {
                    name: "list_audit_logs".to_string(),
                    description: "查询审计日志列表（带过滤和分页）".to_string(),
                    version: "0.1.0".to_string(),
                    input_schema: Some(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "actorIdentityId": {"type": "string"},
                            "actorUsername": {"type": "string"},
                            "actionType": {"type": "string"},
                            "entityType": {"type": "string"},
                            "keyword": {"type": "string"},
                            "date": {"type": "string", "description": "YYYY-MM-DD"},
                            "startDate": {"type": "string", "description": "YYYY-MM-DD"},
                            "endDate": {"type": "string", "description": "YYYY-MM-DD"},
                            "limit": {"type": "integer", "default": 100},
                            "offset": {"type": "integer", "default": 0},
                            "sortBy": {"type": ["string", "null"]},
                            "sortOrder": {"type": ["string", "null"]},
                        },
                    })),
                    output_schema: Some(serde_json::json!({
                        "type": "object",
                        "properties": {
                            "logs": {
                                "type": "array",
                                "items": {"$ref": "#/definitions/AuditLogEntry"}
                            },
                            "limit": {"type": "integer"},
                            "total": {"type": "integer"},
                        },
                    })),
                },
            ],
        }
    }

    /// 关闭：删除发送端以通知刷新线程排空退出
    fn shutdown(&mut self) -> Result<(), String> {
        let mut guard = AUDIT_TX.lock().map_err(|e| e.to_string())?;
        if guard.is_some() {
            tracing::info!("[feature-audit] shutting down — dropping sender to drain flusher");
            *guard = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionMode, Namespace, NoopHttpClient,
        PlatformMembershipId, PlatformRole, RequestId, Revision, TenantId, TenantScope,
    };

    fn preview_context(actor_id: &str, effective_tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(effective_tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                actor_id,
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-membership-1").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::new(
                tenant_id,
                Namespace::Production,
                Revision::new("test-revision").unwrap(),
            )
            .unwrap(),
            ExecutionMode::ReadOnlyPreview(
                system_core::PreviewSessionId::new("preview-1").unwrap(),
            ),
            RequestId::new("test-request").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn audit_filter_uses_effective_tenant_without_replacing_actor() {
        let ctx = preview_context("platform-owner-1", "tenant-acme");
        let (clauses, values) = tenant_filter(&ctx);

        assert_eq!(clauses, vec!["tenant_id = ?"]);
        assert_eq!(values, vec!["tenant-acme"]);
        assert_eq!(ctx.user_id(), Some("platform-owner-1"));
    }

    #[test]
    fn safe_detail_redacts_secrets_and_restricted_pii() {
        let detail = serde_json::json!({
            "password": "password-value",
            "accessToken": "token-value",
            "credentialId": "cred-123",
            "providerCredentialRef": "provider-credential-ref-456",
            "providerSecretRef": "opaque-ref-789",
            "credential": "raw-credential",
            "credentialValue": "raw-credential-value",
            "credentialMaterial": "raw-credential-material",
            "nested": {
                "totpCode": "123456",
                "providerSecret": "provider-secret",
                "customerName": "Alice Example",
                "phone": "+86 13800000000",
                "address": "sensitive address",
                "safeStatus": "active"
            }
        });
        let text = safe_audit_detail_json(&detail);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["password"], REDACTED_SECRET);
        assert_eq!(value["accessToken"], REDACTED_SECRET);
        assert_eq!(value["credentialId"], "cred-123");
        assert_eq!(
            value["providerCredentialRef"],
            "provider-credential-ref-456"
        );
        assert_eq!(value["providerSecretRef"], "opaque-ref-789");
        assert_eq!(value["credential"], REDACTED_SECRET);
        assert_eq!(value["credentialValue"], REDACTED_SECRET);
        assert_eq!(value["credentialMaterial"], REDACTED_SECRET);
        assert_eq!(value["nested"]["totpCode"], REDACTED_SECRET);
        assert_eq!(value["nested"]["providerSecret"], REDACTED_SECRET);
        assert_eq!(value["nested"]["customerName"], REDACTED_PII);
        assert_eq!(value["nested"]["phone"], REDACTED_PII);
        assert_eq!(value["nested"]["address"], REDACTED_PII);
        assert_eq!(value["nested"]["safeStatus"], "active");
        for forbidden in [
            "password-value",
            "token-value",
            "raw-credential",
            "raw-credential-value",
            "raw-credential-material",
            "123456",
            "provider-secret",
            "Alice Example",
            "13800000000",
            "sensitive address",
        ] {
            assert!(!text.contains(forbidden));
        }
    }

    #[test]
    fn opaque_reference_with_secret_shaped_value_is_still_redacted() {
        let detail = serde_json::json!({ "secretRef": "otpauth://totp/Talos:user?secret=ABCDEF" });
        let text = safe_audit_detail_json(&detail);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["secretRef"], REDACTED_SECRET);
        assert!(!text.contains("ABCDEF"));
    }

    #[test]
    fn oversized_detail_has_no_plaintext_preview() {
        let detail =
            serde_json::json!({ "safePayload": "x".repeat(AUDIT_DETAIL_BYTES_MAX + 1000) });
        let text = safe_audit_detail_json(&detail);
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["warning"], "detail_too_large");
        assert_eq!(value["redactedSha256"].as_str().unwrap().len(), 64);
        assert!(value.get("preview").is_none());
        assert!(!text.contains(&"x".repeat(128)));
    }

    #[test]
    fn sanitize_trims_all_fields() {
        let mut input = WriteAuditLogInput {
            action_type: "  create  ".into(),
            entity_type: "  order  ".into(),
            entity_id: "  ord-1  ".into(),
            entity_label: "  Order #1  ".into(),
            detail_json: "  {}  ".into(),
        };
        input.sanitize();
        assert_eq!(input.action_type, "create");
        assert_eq!(input.entity_type, "order");
        assert_eq!(input.entity_id, "ord-1");
        assert_eq!(input.entity_label, "Order #1");
        assert_eq!(input.detail_json, "{}");
    }

    #[test]
    fn validate_rejects_empty_required_fields() {
        let input = WriteAuditLogInput {
            action_type: String::new(),
            entity_type: String::new(),
            entity_id: String::new(),
            entity_label: String::new(),
            detail_json: String::new(),
        };
        let result = input.validate();
        assert!(!result.is_valid());
        assert!(result.errors.iter().any(|e| e.field == "actionType"));
        assert!(result.errors.iter().any(|e| e.field == "entityType"));
    }

    #[test]
    fn validate_accepts_valid_input() {
        let input = WriteAuditLogInput {
            action_type: "create".into(),
            entity_type: "order".into(),
            entity_id: "ord-1".into(),
            entity_label: "Order #1".into(),
            detail_json: "{}".into(),
        };
        let result = input.validate();
        assert!(result.is_valid());
    }

    #[test]
    fn list_input_defaults() {
        let mut input = ListAuditLogsInput {
            actor_identity_id: String::new(),
            actor_username: String::new(),
            action_type: String::new(),
            entity_type: String::new(),
            keyword: String::new(),
            date: String::new(),
            start_date: String::new(),
            end_date: String::new(),
            limit: 0,
            offset: -5,
            sort_by: None,
            sort_order: None,
        };
        input.sanitize();
        assert_eq!(input.limit, 100);
        assert_eq!(input.offset, 0);
    }
}

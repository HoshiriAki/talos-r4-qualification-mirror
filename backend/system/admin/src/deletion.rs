//! feature-deletion — 数据删除/匿名化模块（Maxwell 原生）
//!
//! 处理用户数据删除请求，15 工作日 SLA，数据匿名化。
//!
//! ## 命令
//! - `request` — 创建数据删除请求（AuthUser）
//! - `list` — 管理员：列出所有请求（AdminUser）
//! - `process` — 管理员：标记为处理中（AdminUser）
//! - `complete` — 管理员：匿名化用户数据（AdminUser）
//! - `reject` — 管理员：拒绝请求（AdminUser）

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use uuid::Uuid;

use system_core::{
    CommandSchema, DataScope, ErrorPayload, ExecutionContext, FieldError, ModuleMetadata,
    ModuleSchema, Sanitize, SystemModule, Validate, ValidationResult,
};

// ══════════════════════════════════════════════════════════════════════════
// Helpers
// ══════════════════════════════════════════════════════════════════════════

fn shanghai_now_iso() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y-%m-%dT%H:%M:%S%.3f+08:00").to_string()
}

fn err(category: &str, code: &str, message: String) -> String {
    serde_json::to_string(&ErrorPayload {
        category: category.into(),
        code: code.into(),
        message,
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

pub fn require_self_service_actor(
    actor_identity_id: Option<&str>,
    target_user_id: &str,
) -> Result<(), String> {
    if actor_identity_id == Some(target_user_id) {
        Ok(())
    } else {
        Err(err(
            "auth",
            "AUTH_SELF_REQUIRED",
            "deletion self-service request may only target the authenticated identity".into(),
        ))
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionRequestInput {
    pub user_id: String,
    pub request_type: String,
    pub reason: String,
}

impl Validate for DeletionRequestInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.user_id.is_empty() {
            errors.push(FieldError {
                field: "userId".into(),
                message: "用户ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if !matches!(self.request_type.as_str(), "account" | "personal_data") {
            errors.push(FieldError {
                field: "requestType".into(),
                message: "必须是 account 或 personal_data".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for DeletionRequestInput {
    fn sanitize(&mut self) {
        self.user_id = self.user_id.trim().to_string();
        self.request_type = self.request_type.trim().to_string();
        self.reason = self.reason.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionListInput {
    pub status: Option<String>,
}

impl Validate for DeletionListInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for DeletionListInput {
    fn sanitize(&mut self) {
        if let Some(ref mut s) = self.status {
            *s = s.trim().to_string();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionAdminInput {
    pub request_id: String,
    pub admin_notes: String,
}

impl Validate for DeletionAdminInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.request_id.is_empty() {
            errors.push(FieldError {
                field: "requestId".into(),
                message: "请求ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for DeletionAdminInput {
    fn sanitize(&mut self) {
        self.request_id = self.request_id.trim().to_string();
        self.admin_notes = self.admin_notes.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureDeletion {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureDeletion {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureDeletion {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureDeletion {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "deletion".into(),
            version: "0.1.0".into(),
            description: "数据删除/匿名化模块".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let pool_guard = self.pool.lock().map_err(|e| e.to_string())?;
        let pool = pool_guard.as_ref().ok_or_else(|| {
            err(
                "sys",
                "SYS_NOT_INIT",
                "FeatureDeletion not initialized".into(),
            )
        })?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;

        match command {
            "request" => {
                let unvalidated: system_core::Unvalidated<DeletionRequestInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                request_deletion(&conn, ctx.data_scope(), &input)
            }
            "list" => {
                let unvalidated: system_core::Unvalidated<DeletionListInput> =
                    payload.try_into()?;
                list_deletions(
                    &conn,
                    ctx.data_scope(),
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "process" => {
                let unvalidated: system_core::Unvalidated<DeletionAdminInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                process_deletion(&conn, ctx.data_scope(), &validated.into_inner())
            }
            "complete" => {
                let unvalidated: system_core::Unvalidated<DeletionAdminInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                complete_deletion(&conn, ctx.data_scope(), &validated.into_inner())
            }
            "reject" => {
                let unvalidated: system_core::Unvalidated<DeletionAdminInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                reject_deletion(&conn, ctx.data_scope(), &validated.into_inner())
            }
            _ => Err(err(
                "sys",
                "CMD_UNKNOWN",
                format!("Unknown command: {}", command),
            )),
        }
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        vec![
            system_core::CommandMetadata::new(
                "request",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "list",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "process",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "complete",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "reject",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "deletion".into(),
            description: "数据删除/匿名化模块".into(),
            commands: vec![
                CommandSchema {
                    name: "request".into(),
                    description: "创建删除请求".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "list".into(),
                    description: "列出删除请求".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "process".into(),
                    description: "标记处理中".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "complete".into(),
                    description: "完成匿名化".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "reject".into(),
                    description: "拒绝请求".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Command implementations
// ══════════════════════════════════════════════════════════════════════════

fn request_deletion(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &DeletionRequestInput,
) -> Result<Value, String> {
    let existing: Option<String> = conn.query_row(
        "SELECT id FROM data_deletion_requests WHERE tenant_id = ?1 AND user_id = ?2 AND status IN ('pending', 'processing') LIMIT 1",
        params![scope.tenant_id().as_str(), input.user_id],
        |row| row.get(0),
    ).ok();
    if let Some(id) = existing {
        return Ok(serde_json::json!({ "ok": true, "id": id, "message": "已有处理中的删除请求" }));
    }
    let id = Uuid::new_v4().to_string();
    let now = shanghai_now_iso();
    conn.execute(
        "INSERT INTO data_deletion_requests (id, user_id, request_type, status, reason, requested_at, created_at, tenant_id)
         VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7)",
        params![id, input.user_id, input.request_type, input.reason, now, now, scope.tenant_id().as_str()],
    ).map_err(|e| err("db", "DB_INSERT", e.to_string()))?;
    Ok(serde_json::json!({ "ok": true, "id": id, "status": "pending", "requestedAt": now }))
}

fn list_deletions(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &DeletionListInput,
) -> Result<Value, String> {
    let sql = if input.status.is_some() {
        String::from(
            "SELECT id, user_id, request_type, status, reason, requested_at, completed_at, admin_notes, created_at
             FROM data_deletion_requests WHERE tenant_id = ?1 AND status = ?2 ORDER BY created_at DESC"
        )
    } else {
        "SELECT id, user_id, request_type, status, reason, requested_at, completed_at, admin_notes, created_at
         FROM data_deletion_requests WHERE tenant_id = ?1 ORDER BY created_at DESC".to_string()
    };
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
    let rows: Vec<Value> = stmt
        .query_map(
            rusqlite::params_from_iter(
                std::iter::once(scope.tenant_id().as_str()).chain(input.status.as_deref()),
            ),
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "userId": row.get::<_, String>(1)?,
                    "requestType": row.get::<_, String>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "reason": row.get::<_, String>(4)?,
                    "requestedAt": row.get::<_, String>(5)?,
                    "completedAt": row.get::<_, Option<String>>(6)?,
                    "adminNotes": row.get::<_, String>(7)?,
                    "createdAt": row.get::<_, String>(8)?,
                }))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!({ "ok": true, "requests": rows }))
}

fn process_deletion(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &DeletionAdminInput,
) -> Result<Value, String> {
    conn.execute(
        "UPDATE data_deletion_requests SET status = 'processing', admin_notes = ?1 WHERE tenant_id = ?2 AND id = ?3",
        params![input.admin_notes, scope.tenant_id().as_str(), input.request_id],
    )
    .map_err(|e| err("db", "DB_UPDATE", e.to_string()))?;
    Ok(serde_json::json!({ "ok": true, "id": input.request_id, "status": "processing" }))
}

fn complete_deletion(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &DeletionAdminInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;
    let now = shanghai_now_iso();
    let identity_id: String = conn
        .query_row(
            "SELECT user_id FROM data_deletion_requests WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.request_id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;
    let target_role: String = conn
        .query_row(
            "SELECT role FROM tenant_memberships
             WHERE tenant_id = ?1 AND identity_id = ?2 AND status != 'revoked'",
            params![scope.tenant_id().as_str(), identity_id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;
    if target_role == "owner" {
        let owner_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tenant_memberships
                 WHERE tenant_id = ?1 AND role = 'owner' AND status = 'active'",
                params![scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .map_err(|e| {
                let _ = conn.execute("ROLLBACK", []);
                err("db", "DB_QUERY", e.to_string())
            })?;
        if owner_count <= 1 {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err(
                "biz",
                "TENANT_LAST_OWNER",
                "必须先转移租户所有权，不能删除最后一位 Owner".into(),
            ));
        }
    }

    let anon_id = format!("ANONYMIZED-{}", Uuid::new_v4());
    let anon_display = format!("已删除用户-{}", &Uuid::new_v4().to_string()[..8]);
    let revoked = conn
        .execute(
            "UPDATE tenant_memberships
         SET status = 'revoked', updated_at = ?1
         WHERE tenant_id = ?2 AND identity_id = ?3 AND status != 'revoked'",
            params![now, scope.tenant_id().as_str(), identity_id],
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_UPDATE", e.to_string())
        })?;
    if revoked == 0 {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_NOT_FOUND",
            "Tenant membership not found".into(),
        ));
    }

    let identity_anonymized =
        conn.execute(
            "UPDATE identities
         SET username = ?1, display_name = ?2, email = ?3, phone = NULL,
             password_hash = 'deleted', totp_secret_ciphertext = NULL, totp_enabled = 0,
             status = 'disabled', updated_at = ?4
         WHERE id = ?5
           AND NOT EXISTS (
             SELECT 1 FROM tenant_memberships tm
             WHERE tm.identity_id = identities.id AND tm.status != 'revoked'
           )
           AND NOT EXISTS (
             SELECT 1 FROM platform_memberships pm
             WHERE pm.identity_id = identities.id AND pm.status != 'revoked'
           )",
            params![
                anon_id,
                anon_display,
                format!("{}@deleted.local", anon_id),
                now,
                identity_id
            ],
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_UPDATE", e.to_string())
        })? == 1;
    conn.execute(
        "UPDATE data_deletion_requests SET status = 'completed', completed_at = ?1, admin_notes = ?2 WHERE tenant_id = ?3 AND id = ?4",
        params![now, input.admin_notes, scope.tenant_id().as_str(), input.request_id],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;
    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;
    Ok(serde_json::json!({
        "ok": true,
        "id": input.request_id,
        "status": "completed",
        "completedAt": now,
        "identityId": identity_id,
        "identityAnonymized": identity_anonymized,
    }))
}

fn reject_deletion(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &DeletionAdminInput,
) -> Result<Value, String> {
    let now = shanghai_now_iso();
    conn.execute(
        "UPDATE data_deletion_requests SET status = 'rejected', completed_at = ?1, admin_notes = ?2 WHERE tenant_id = ?3 AND id = ?4",
        params![now, input.admin_notes, scope.tenant_id().as_str(), input.request_id],
    ).map_err(|e| err("db", "DB_UPDATE", e.to_string()))?;
    Ok(
        serde_json::json!({ "ok": true, "id": input.request_id, "status": "rejected", "rejectedAt": now }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_core::{Revision, TenantId};

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(TenantId::new(tenant).unwrap(), Revision::new("r1").unwrap()).unwrap()
    }

    #[test]
    fn deletion_request_is_strictly_self_service() {
        assert!(require_self_service_actor(Some("identity-a"), "identity-a").is_ok());
        let error = require_self_service_actor(Some("identity-a"), "identity-b").unwrap_err();
        assert!(error.contains("AUTH_SELF_REQUIRED"));
        assert!(require_self_service_actor(None, "identity-a").is_err());
    }

    #[test]
    fn deletion_admin_operations_cannot_cross_tenants() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE data_deletion_requests (
            id TEXT PRIMARY KEY, user_id TEXT NOT NULL, request_type TEXT NOT NULL,
            status TEXT NOT NULL, reason TEXT NOT NULL, requested_at TEXT NOT NULL,
            completed_at TEXT, admin_notes TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL,
            tenant_id TEXT NOT NULL
        );
        CREATE TABLE identities (
            id TEXT PRIMARY KEY, username TEXT NOT NULL, display_name TEXT NOT NULL,
            email TEXT, phone TEXT, password_hash TEXT NOT NULL,
            totp_secret_ciphertext TEXT, totp_enabled INTEGER NOT NULL,
            status TEXT NOT NULL, updated_at TEXT NOT NULL
        );
        CREATE TABLE tenant_memberships (
            id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
            role TEXT NOT NULL, status TEXT NOT NULL, updated_at TEXT NOT NULL
        );
        CREATE TABLE platform_memberships (
            id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, status TEXT NOT NULL
        );
        INSERT INTO data_deletion_requests VALUES
          ('foreign-request', 'foreign-user', 'account', 'pending', '', 'now', NULL, '', 'now', 'tenant-b');
        INSERT INTO identities VALUES ('foreign-user', 'foreign', 'Foreign User', 'foreign@example.test', '123', 'hash', NULL, 0, 'active', 'now');
        INSERT INTO tenant_memberships VALUES ('foreign-membership', 'foreign-user', 'tenant-b', 'staff', 'active', 'now');").unwrap();
        let tenant_a = scope("tenant-a");
        process_deletion(
            &conn,
            &tenant_a,
            &DeletionAdminInput {
                request_id: "foreign-request".into(),
                admin_notes: "no access".into(),
            },
        )
        .unwrap();
        reject_deletion(
            &conn,
            &tenant_a,
            &DeletionAdminInput {
                request_id: "foreign-request".into(),
                admin_notes: "no access".into(),
            },
        )
        .unwrap();
        assert!(
            complete_deletion(
                &conn,
                &tenant_a,
                &DeletionAdminInput {
                    request_id: "foreign-request".into(),
                    admin_notes: "no access".into(),
                }
            )
            .is_err()
        );
        let status: String = conn
            .query_row(
                "SELECT status FROM data_deletion_requests WHERE id = 'foreign-request'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let display_name: String = conn
            .query_row(
                "SELECT display_name FROM identities WHERE id = 'foreign-user'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "pending");
        assert_eq!(display_name, "Foreign User");
        let list = list_deletions(&conn, &tenant_a, &DeletionListInput { status: None }).unwrap();
        assert_eq!(list["requests"].as_array().unwrap().len(), 0);
    }
}

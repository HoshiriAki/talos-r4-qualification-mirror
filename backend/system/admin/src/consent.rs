//! feature-consent — 隐私同意书模块（Maxwell 原生）
//!
//! 记录和管理用户的隐私政策、服务条款、数据处理同意。
//!
//! ## 命令
//! - `record` — INSERT 同意记录（AuthUser）
//! - `check` — 检查用户是否同意最新版本（AuthUser）
//! - `revoke` — 撤回同意（AuthUser）
//! - `audit` — 审计：列出用户所有同意记录（AdminUser）

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
// Shanghai timezone helper
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
            "consent self-service commands may only target the authenticated identity".into(),
        ))
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentRecordInput {
    pub user_id: String,
    pub consent_type: String,
    pub version: String,
    pub ip_address: String,
    pub user_agent: String,
}

impl Validate for ConsentRecordInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.user_id.is_empty() {
            errors.push(FieldError {
                field: "userId".into(),
                message: "用户ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if !matches!(
            self.consent_type.as_str(),
            "privacy_policy" | "tos" | "data_processing"
        ) {
            errors.push(FieldError {
                field: "consentType".into(),
                message: "必须是 privacy_policy / tos / data_processing".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ConsentRecordInput {
    fn sanitize(&mut self) {
        self.user_id = self.user_id.trim().to_string();
        self.consent_type = self.consent_type.trim().to_string();
        self.version = self.version.trim().to_string();
        self.ip_address = self.ip_address.trim().to_string();
        self.user_agent = self.user_agent.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentCheckInput {
    pub user_id: String,
    pub consent_type: String,
    pub version: String,
}

impl Validate for ConsentCheckInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.user_id.is_empty() {
            errors.push(FieldError {
                field: "userId".into(),
                message: "用户ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ConsentCheckInput {
    fn sanitize(&mut self) {
        self.user_id = self.user_id.trim().to_string();
        self.consent_type = self.consent_type.trim().to_string();
        self.version = self.version.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentRevokeInput {
    pub user_id: String,
    pub consent_type: String,
}

impl Validate for ConsentRevokeInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.user_id.is_empty() {
            errors.push(FieldError {
                field: "userId".into(),
                message: "用户ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ConsentRevokeInput {
    fn sanitize(&mut self) {
        self.user_id = self.user_id.trim().to_string();
        self.consent_type = self.consent_type.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentAuditInput {
    pub user_id: String,
}

impl Validate for ConsentAuditInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.user_id.is_empty() {
            errors.push(FieldError {
                field: "userId".into(),
                message: "用户ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ConsentAuditInput {
    fn sanitize(&mut self) {
        self.user_id = self.user_id.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureConsent {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureConsent {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureConsent {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureConsent {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "consent".into(),
            version: "0.1.0".into(),
            description: "隐私同意书模块 — record/check/revoke/audit".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        // Pool is injected via assembler, not here
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
                "FeatureConsent not initialized".into(),
            )
        })?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;

        match command {
            "record" => {
                let unvalidated: system_core::Unvalidated<ConsentRecordInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                record_consent(&conn, ctx.data_scope(), &input)
            }
            "check" => {
                let unvalidated: system_core::Unvalidated<ConsentCheckInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                check_consent(&conn, ctx.data_scope(), &input)
            }
            "revoke" => {
                let unvalidated: system_core::Unvalidated<ConsentRevokeInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                revoke_consent(&conn, ctx.data_scope(), &input)
            }
            "audit" => {
                let unvalidated: system_core::Unvalidated<ConsentAuditInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                audit_consent(&conn, ctx.data_scope(), &input)
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
                "record",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "check",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "revoke",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "audit",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "consent".into(),
            description: "隐私同意书模块".into(),
            commands: vec![
                CommandSchema {
                    name: "record".into(),
                    description: "记录同意".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "check".into(),
                    description: "检查同意状态".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "revoke".into(),
                    description: "撤回同意".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "audit".into(),
                    description: "审计同意记录".into(),
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

fn record_consent(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ConsentRecordInput,
) -> Result<Value, String> {
    let id = Uuid::new_v4().to_string();
    let now = shanghai_now_iso();
    conn.execute(
        "INSERT INTO privacy_consents (id, user_id, consent_type, version, consented, ip_address, user_agent, created_at, tenant_id)
         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8)",
        params![id, input.user_id, input.consent_type, input.version, input.ip_address, input.user_agent, now, scope.tenant_id().as_str()],
    ).map_err(|e| err("db", "DB_INSERT", e.to_string()))?;
    Ok(serde_json::json!({ "ok": true, "id": id, "createdAt": now }))
}

fn check_consent(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ConsentCheckInput,
) -> Result<Value, String> {
    let has_consented: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM privacy_consents
            WHERE tenant_id = ?1 AND user_id = ?2 AND consent_type = ?3 AND version = ?4 AND consented = 1 AND revoked_at IS NULL
            ORDER BY created_at DESC LIMIT 1
        )",
        params![scope.tenant_id().as_str(), input.user_id, input.consent_type, input.version],
        |row| row.get(0),
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
    Ok(serde_json::json!({ "ok": true, "hasConsented": has_consented }))
}

fn revoke_consent(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ConsentRevokeInput,
) -> Result<Value, String> {
    let now = shanghai_now_iso();
    let affected = conn
        .execute(
            "UPDATE privacy_consents SET consented = 0, revoked_at = ?1
         WHERE tenant_id = ?2 AND user_id = ?3 AND consent_type = ?4 AND revoked_at IS NULL",
            params![
                now,
                scope.tenant_id().as_str(),
                input.user_id,
                input.consent_type
            ],
        )
        .map_err(|e| err("db", "DB_UPDATE", e.to_string()))?;
    Ok(serde_json::json!({ "ok": true, "affected": affected, "revokedAt": now }))
}

fn audit_consent(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ConsentAuditInput,
) -> Result<Value, String> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, consent_type, version, consented, ip_address, user_agent, revoked_at, created_at
         FROM privacy_consents WHERE tenant_id = ?1 AND user_id = ?2 ORDER BY created_at DESC"
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
    let rows: Vec<Value> = stmt
        .query_map(params![scope.tenant_id().as_str(), input.user_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "userId": row.get::<_, String>(1)?,
                "consentType": row.get::<_, String>(2)?,
                "version": row.get::<_, String>(3)?,
                "consented": row.get::<_, i32>(4)? == 1,
                "ipAddress": row.get::<_, String>(5)?,
                "userAgent": row.get::<_, String>(6)?,
                "revokedAt": row.get::<_, Option<String>>(7)?,
                "createdAt": row.get::<_, String>(8)?,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(serde_json::json!({ "ok": true, "records": rows }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_core::{Revision, TenantId};

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(TenantId::new(tenant).unwrap(), Revision::new("r1").unwrap()).unwrap()
    }

    #[test]
    fn consent_self_service_commands_are_actor_bound() {
        assert!(require_self_service_actor(Some("identity-a"), "identity-a").is_ok());
        let error = require_self_service_actor(Some("identity-a"), "identity-b").unwrap_err();
        assert!(error.contains("AUTH_SELF_REQUIRED"));
        assert!(require_self_service_actor(None, "identity-a").is_err());
    }

    #[test]
    fn consent_queries_and_mutations_are_tenant_scoped() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE privacy_consents (
            id TEXT PRIMARY KEY, user_id TEXT NOT NULL, consent_type TEXT NOT NULL,
            version TEXT NOT NULL, consented INTEGER NOT NULL, ip_address TEXT NOT NULL,
            user_agent TEXT NOT NULL, revoked_at TEXT, created_at TEXT NOT NULL,
            tenant_id TEXT NOT NULL
        );
        INSERT INTO privacy_consents VALUES
          ('foreign', 'shared-user', 'tos', 'v1', 1, '', '', NULL, '2026-07-16T10:00:00+08:00', 'tenant-b');").unwrap();
        let tenant_a = scope("tenant-a");
        let check = check_consent(
            &conn,
            &tenant_a,
            &ConsentCheckInput {
                user_id: "shared-user".into(),
                consent_type: "tos".into(),
                version: "v1".into(),
            },
        )
        .unwrap();
        assert_eq!(check["hasConsented"], false);
        revoke_consent(
            &conn,
            &tenant_a,
            &ConsentRevokeInput {
                user_id: "shared-user".into(),
                consent_type: "tos".into(),
            },
        )
        .unwrap();
        let foreign_consented: i32 = conn
            .query_row(
                "SELECT consented FROM privacy_consents WHERE id = 'foreign'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(foreign_consented, 1);
        let audit = audit_consent(
            &conn,
            &tenant_a,
            &ConsentAuditInput {
                user_id: "shared-user".into(),
            },
        )
        .unwrap();
        assert_eq!(audit["records"].as_array().unwrap().len(), 0);
    }
}

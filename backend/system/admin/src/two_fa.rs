//! feature-two-fa — 双因素认证模块（Maxwell 原生）
//!
//! TOTP (Time-based One-Time Password) 实现，使用 HMAC-SHA256。
//! 30 秒时间窗口，支持前/后各1个窗口（共 90 秒容差）。
//!
//! ## 命令
//! - `generate_secret` — 生成 TOTP 密钥，返回 otpauth:// URL（AuthUser）
//! - `verify_and_enable` — 验证 TOTP 码并启用 2FA（AuthUser）
//! - `disable` — 管理员强制禁用 2FA（AdminUser）
//! - `status` — 查询用户 2FA 状态（AuthUser）

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;

use crate::totp_crypto::{
    base32_encode, decrypt_totp_secret, encrypt_totp_secret, generate_totp_secret, verify_totp_code,
};
use system_core::{
    CommandSchema, DataScope, ErrorPayload, ExecutionContext, FieldError, ModuleMetadata,
    ModuleSchema, Sanitize, SystemModule, Validate, ValidationResult,
};

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

fn require_self_service_actor(
    actor_identity_id: Option<&str>,
    target_user_id: &str,
) -> Result<(), String> {
    if actor_identity_id == Some(target_user_id) {
        Ok(())
    } else {
        Err(err(
            "auth",
            "AUTH_SELF_REQUIRED",
            "TOTP enrollment may only modify the authenticated identity".into(),
        ))
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwoFaIdInput {
    pub user_id: String,
}

impl Validate for TwoFaIdInput {
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

impl Sanitize for TwoFaIdInput {
    fn sanitize(&mut self) {
        self.user_id = self.user_id.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TwoFaVerifyInput {
    pub user_id: String,
    pub code: String,
}

impl Validate for TwoFaVerifyInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.user_id.is_empty() {
            errors.push(FieldError {
                field: "userId".into(),
                message: "用户ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.code.len() != 6 || !self.code.chars().all(|c| c.is_ascii_digit()) {
            errors.push(FieldError {
                field: "code".into(),
                message: "验证码必须为6位数字".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for TwoFaVerifyInput {
    fn sanitize(&mut self) {
        self.user_id = self.user_id.trim().to_string();
        self.code = self.code.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureTwoFa {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureTwoFa {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureTwoFa {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureTwoFa {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "two_fa".into(),
            version: "0.1.0".into(),
            description: "双因素认证模块 — TOTP 生成/验证/禁用/查询".into(),
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
        let pool = pool_guard
            .as_ref()
            .ok_or_else(|| err("sys", "SYS_NOT_INIT", "FeatureTwoFa not initialized".into()))?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;

        match command {
            "generate_secret" => {
                let unvalidated: system_core::Unvalidated<TwoFaIdInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                generate_2fa_secret(&conn, ctx.data_scope(), &input)
            }
            "verify_and_enable" => {
                let unvalidated: system_core::Unvalidated<TwoFaVerifyInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                verify_and_enable_2fa(&conn, ctx.data_scope(), &input)
            }
            "disable" => {
                let unvalidated: system_core::Unvalidated<TwoFaIdInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                disable_2fa(
                    &conn,
                    ctx.data_scope(),
                    ctx.actor().id(),
                    &validated.into_inner(),
                )
            }
            "status" => {
                let unvalidated: system_core::Unvalidated<TwoFaIdInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                status_2fa(&conn, ctx.data_scope(), &validated.into_inner())
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
                "generate_secret",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "verify_and_enable",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "disable",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "status",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "two_fa".into(),
            description: "双因素认证模块".into(),
            commands: vec![
                CommandSchema {
                    name: "generate_secret".into(),
                    description: "生成TOTP密钥".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "verify_and_enable".into(),
                    description: "验证并启用2FA".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "disable".into(),
                    description: "禁用2FA".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "status".into(),
                    description: "查询2FA状态".into(),
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

fn generate_2fa_secret(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &TwoFaIdInput,
) -> Result<Value, String> {
    let already_enabled: bool = conn
        .query_row(
            "SELECT i.totp_enabled
             FROM identities i
             JOIN tenant_memberships tm ON tm.identity_id = i.id
             WHERE tm.tenant_id = ?1 AND tm.status = 'active' AND i.id = ?2",
            params![scope.tenant_id().as_str(), input.user_id],
            |row| row.get::<_, i32>(0),
        )
        .map(|v| v == 1)
        .unwrap_or(false);

    if already_enabled {
        return Err(err(
            "rgv",
            "2FA_ALREADY_ENABLED",
            "2FA 已经启用，请先禁用后再重新生成密钥".into(),
        ));
    }

    let secret = generate_totp_secret();
    let secret_b32 = base32_encode(&secret);
    let secret_ciphertext = encrypt_totp_secret(&secret, &input.user_id)
        .map_err(|message| err("sys", "2FA_ENCRYPT", message))?;
    let updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let changed = conn
        .execute(
            "UPDATE identities
             SET totp_secret_ciphertext = ?1, updated_at = ?4
             WHERE id = ?3
               AND EXISTS (
                 SELECT 1 FROM tenant_memberships tm
                 WHERE tm.identity_id = identities.id AND tm.tenant_id = ?2 AND tm.status = 'active'
               )",
            params![
                secret_ciphertext,
                scope.tenant_id().as_str(),
                input.user_id,
                updated_at
            ],
        )
        .map_err(|e| err("db", "DB_UPDATE", e.to_string()))?;
    if changed != 1 {
        return Err(err(
            "rgv",
            "2FA_IDENTITY_UNAVAILABLE",
            "当前租户中不存在可用的目标身份".into(),
        ));
    }

    let otpauth_url = format!(
        "otpauth://totp/Talos:{}?secret={}&issuer=Talos&algorithm=SHA256&digits=6&period=30",
        input.user_id, secret_b32
    );

    Ok(serde_json::json!({
        "ok": true, "secret": secret_b32, "otpauthUrl": otpauth_url,
        "message": "请使用 TOTP 应用（如 Google Authenticator）扫描或手动输入密钥"
    }))
}

fn verify_and_enable_2fa(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &TwoFaVerifyInput,
) -> Result<Value, String> {
    let secret_ciphertext: String = conn
        .query_row(
            "SELECT i.totp_secret_ciphertext
             FROM identities i
             JOIN tenant_memberships tm ON tm.identity_id = i.id
             WHERE tm.tenant_id = ?1 AND tm.status = 'active' AND i.id = ?2",
            params![scope.tenant_id().as_str(), input.user_id],
            |row| row.get(0),
        )
        .map_err(|_| {
            err(
                "rgv",
                "2FA_NO_SECRET",
                "尚未生成 TOTP 密钥，请先生成".into(),
            )
        })?;

    let decrypted = decrypt_totp_secret(&secret_ciphertext, &input.user_id)
        .map_err(|message| err("sys", "2FA_DECRYPT", message))?;

    if !verify_totp_code(&decrypted.secret, &input.code) {
        return Err(err(
            "rgv",
            "2FA_VERIFY_FAIL",
            "验证码错误，请检查 TOTP 应用时间是否正确".into(),
        ));
    }

    let rewrapped = if decrypted.needs_rewrap {
        Some(
            encrypt_totp_secret(&decrypted.secret, &input.user_id)
                .map_err(|message| err("sys", "2FA_ENCRYPT", message))?,
        )
    } else {
        None
    };
    let updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    let changed = conn
        .execute(
            "UPDATE identities
             SET totp_enabled = 1,
                 totp_secret_ciphertext = COALESCE(?4, totp_secret_ciphertext),
                 updated_at = ?3
             WHERE id = ?2
               AND totp_secret_ciphertext = ?5
               AND EXISTS (
                 SELECT 1 FROM tenant_memberships tm
                 WHERE tm.identity_id = identities.id AND tm.tenant_id = ?1 AND tm.status = 'active'
               )",
            params![
                scope.tenant_id().as_str(),
                input.user_id,
                updated_at,
                rewrapped,
                secret_ciphertext
            ],
        )
        .map_err(|e| err("db", "DB_UPDATE", e.to_string()))?;
    if changed != 1 {
        return Err(err(
            "rgv",
            "2FA_CREDENTIAL_CHANGED",
            "TOTP 凭据在验证期间已发生变化，请重新开始绑定".into(),
        ));
    }

    Ok(serde_json::json!({ "ok": true, "enabled": true, "message": "2FA 已成功启用" }))
}

fn disable_2fa(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    actor_identity_id: Option<&str>,
    input: &TwoFaIdInput,
) -> Result<Value, String> {
    if actor_identity_id != Some(input.user_id.as_str()) {
        let tenant_memberships: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tenant_memberships
                 WHERE identity_id = ?1 AND status != 'revoked'",
                params![input.user_id],
                |row| row.get(0),
            )
            .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
        let platform_memberships: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM platform_memberships
                 WHERE identity_id = ?1 AND status != 'revoked'",
                params![input.user_id],
                |row| row.get(0),
            )
            .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
        if tenant_memberships != 1 || platform_memberships != 0 {
            return Err(err(
                "rgv",
                "2FA_SHARED_IDENTITY",
                "共享 Identity 的全局 MFA 只能由本人或平台身份治理修改".into(),
            ));
        }
    }
    let updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    conn.execute(
        "UPDATE identities
         SET totp_enabled = 0, totp_secret_ciphertext = NULL, updated_at = ?3
         WHERE id = ?2
           AND EXISTS (
             SELECT 1 FROM tenant_memberships tm
             WHERE tm.identity_id = identities.id AND tm.tenant_id = ?1 AND tm.status = 'active'
           )",
        params![scope.tenant_id().as_str(), input.user_id, updated_at],
    )
    .map_err(|e| err("db", "DB_UPDATE", e.to_string()))?;
    Ok(serde_json::json!({ "ok": true, "enabled": false, "message": "2FA 已禁用" }))
}

fn status_2fa(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &TwoFaIdInput,
) -> Result<Value, String> {
    let (enabled, has_secret): (bool, bool) = conn
        .query_row(
            "SELECT i.totp_enabled,
                CASE WHEN COALESCE(i.totp_secret_ciphertext, '') != '' THEN 1 ELSE 0 END
         FROM identities i
         JOIN tenant_memberships tm ON tm.identity_id = i.id
         WHERE tm.tenant_id = ?1 AND tm.status = 'active' AND i.id = ?2",
            params![scope.tenant_id().as_str(), input.user_id],
            |row| Ok((row.get::<_, i32>(0)? == 1, row.get::<_, i32>(1)? == 1)),
        )
        .unwrap_or((false, false));
    Ok(serde_json::json!({ "ok": true, "enabled": enabled, "hasSecret": has_secret }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_core::{Revision, TenantId};

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(TenantId::new(tenant).unwrap(), Revision::new("r1").unwrap()).unwrap()
    }

    #[test]
    fn totp_enrollment_is_strictly_self_service() {
        assert!(require_self_service_actor(Some("identity-a"), "identity-a").is_ok());
        let error = require_self_service_actor(Some("identity-a"), "identity-b").unwrap_err();
        assert!(error.contains("AUTH_SELF_REQUIRED"));
        assert!(require_self_service_actor(None, "identity-a").is_err());
    }

    #[test]
    fn two_fa_lookup_and_mutation_are_tenant_scoped() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE identities (
            id TEXT PRIMARY KEY, totp_secret_ciphertext TEXT,
            totp_enabled INTEGER NOT NULL, updated_at TEXT NOT NULL
        );
        CREATE TABLE tenant_memberships (
            id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
            status TEXT NOT NULL
        );
        INSERT INTO identities VALUES ('shared-user', 'abcd', 1, 'now');
        INSERT INTO tenant_memberships VALUES ('membership-b', 'shared-user', 'tenant-b', 'active');",
        )
        .unwrap();
        let tenant_a = scope("tenant-a");
        let status = status_2fa(
            &conn,
            &tenant_a,
            &TwoFaIdInput {
                user_id: "shared-user".into(),
            },
        )
        .unwrap();
        assert_eq!(status["enabled"], false);
        disable_2fa(
            &conn,
            &tenant_a,
            Some("shared-user"),
            &TwoFaIdInput {
                user_id: "shared-user".into(),
            },
        )
        .unwrap();
        let foreign: (String, i32) = conn
            .query_row(
                "SELECT totp_secret_ciphertext, totp_enabled FROM identities WHERE id = 'shared-user'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(foreign, ("abcd".into(), 1));
    }
}

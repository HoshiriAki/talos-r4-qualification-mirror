use serde_json::Value;
use system_admin::totp_crypto::{
    base32_encode, decrypt_totp_secret, encrypt_totp_secret, generate_totp_secret, verify_totp_code,
};
use system_admin::two_fa::{FeatureTwoFa, TwoFaIdInput, TwoFaVerifyInput};
use system_core::{
    AccessRequirement, CommandMetadata, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule, Unvalidated,
};

use crate::error::AppError;
use crate::repositories::{AuthSecurityRepository, TotpDisableOutcome, TotpEnrollmentOutcome};

#[derive(Clone)]
pub(crate) struct TwoFaCompatibilityModule {
    repository: AuthSecurityRepository,
}

impl TwoFaCompatibilityModule {
    pub(crate) fn new(repository: AuthSecurityRepository) -> Self {
        Self { repository }
    }

    fn now() -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    fn error(category: &str, code: &str, message: impl Into<String>) -> String {
        serde_json::to_string(&ErrorPayload {
            category: category.into(),
            code: code.into(),
            message: message.into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn repository_error(error: AppError) -> String {
        match error {
            AppError::CodedBadRequest { code, message }
            | AppError::CodedNotFound { code, message } => Self::error("rgv", &code, message),
            AppError::CodedForbidden { code, message } => Self::error("auth", &code, message),
            AppError::ServiceError { code, message } => Self::error("sys", &code, message),
            AppError::BadRequest(message) | AppError::NotFound(message) => {
                Self::error("rgv", "2FA_REQUEST_FAILED", message)
            }
            AppError::Forbidden => Self::error("auth", "AUTH_FORBIDDEN", "仅管理员可操作"),
            AppError::Unauthorized => Self::error("auth", "AUTH_UNAUTHORIZED", "未登录"),
            _ => Self::error(
                "sys",
                "SYS_AUTH_PERSISTENCE",
                "authentication persistence unavailable",
            ),
        }
    }

    fn require_self_service(
        actor_identity_id: Option<&str>,
        target_identity_id: &str,
    ) -> Result<(), String> {
        if actor_identity_id == Some(target_identity_id) {
            Ok(())
        } else {
            Err(Self::error(
                "auth",
                "AUTH_SELF_REQUIRED",
                "TOTP enrollment may only modify the authenticated identity",
            ))
        }
    }
}

impl SystemModule for TwoFaCompatibilityModule {
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
        match command {
            "generate_secret" => {
                let unvalidated: Unvalidated<TwoFaIdInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                Self::require_self_service(ctx.actor().id(), &input.user_id)?;

                let secret = generate_totp_secret();
                let secret_b32 = base32_encode(&secret);
                let secret_ciphertext = encrypt_totp_secret(&secret, &input.user_id)
                    .map_err(|message| Self::error("sys", "2FA_ENCRYPT", message))?;
                match self
                    .repository
                    .install_pending_totp_secret(
                        ctx.data_scope().tenant_id().as_str(),
                        &input.user_id,
                        &secret_ciphertext,
                        &Self::now(),
                    )
                    .map_err(Self::repository_error)?
                {
                    TotpEnrollmentOutcome::Installed => {
                        let otpauth_url = format!(
                            "otpauth://totp/Talos:{}?secret={}&issuer=Talos&algorithm=SHA256&digits=6&period=30",
                            input.user_id, secret_b32
                        );
                        Ok(serde_json::json!({
                            "ok": true,
                            "secret": secret_b32,
                            "otpauthUrl": otpauth_url,
                            "message": "请使用 TOTP 应用（如 Google Authenticator）扫描或手动输入密钥"
                        }))
                    }
                    TotpEnrollmentOutcome::AlreadyEnabled => Err(Self::error(
                        "rgv",
                        "2FA_ALREADY_ENABLED",
                        "2FA 已经启用，请先禁用后再重新生成密钥",
                    )),
                    TotpEnrollmentOutcome::IdentityUnavailable => Err(Self::error(
                        "rgv",
                        "2FA_IDENTITY_UNAVAILABLE",
                        "当前租户中不存在可用的目标身份",
                    )),
                }
            }
            "verify_and_enable" => {
                let unvalidated: Unvalidated<TwoFaVerifyInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                Self::require_self_service(ctx.actor().id(), &input.user_id)?;
                self.repository
                    .verify_and_enable_scoped_totp(
                        ctx.data_scope().tenant_id().as_str(),
                        &input.user_id,
                        &Self::now(),
                        |secret_ciphertext| {
                            let decrypted = decrypt_totp_secret(&secret_ciphertext, &input.user_id)
                                .map_err(|message| AppError::ServiceError {
                                    code: "2FA_DECRYPT".into(),
                                    message,
                                })?;
                            if !verify_totp_code(&decrypted.secret, &input.code) {
                                return Err(AppError::CodedBadRequest {
                                    code: "2FA_VERIFY_FAIL".into(),
                                    message: "验证码错误，请检查 TOTP 应用时间是否正确".into(),
                                });
                            }
                            let replacement = if decrypted.needs_rewrap {
                                Some(
                                    encrypt_totp_secret(&decrypted.secret, &input.user_id)
                                        .map_err(|message| AppError::ServiceError {
                                            code: "2FA_ENCRYPT".into(),
                                            message,
                                        })?,
                                )
                            } else {
                                None
                            };
                            Ok((
                                serde_json::json!({
                                    "ok": true,
                                    "enabled": true,
                                    "message": "2FA 已成功启用"
                                }),
                                replacement,
                            ))
                        },
                    )
                    .map_err(Self::repository_error)
            }
            "disable" => {
                let unvalidated: Unvalidated<TwoFaIdInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                match self
                    .repository
                    .disable_scoped_totp(
                        ctx.data_scope().tenant_id().as_str(),
                        ctx.actor().id(),
                        &input.user_id,
                        &Self::now(),
                    )
                    .map_err(Self::repository_error)?
                {
                    TotpDisableOutcome::Disabled => Ok(serde_json::json!({
                        "ok": true,
                        "enabled": false,
                        "message": "2FA 已禁用"
                    })),
                    TotpDisableOutcome::SharedIdentityForbidden => Err(Self::error(
                        "rgv",
                        "2FA_SHARED_IDENTITY",
                        "共享 Identity 的全局 MFA 只能由本人或平台身份治理修改",
                    )),
                }
            }
            "status" => {
                let unvalidated: Unvalidated<TwoFaIdInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let status = self
                    .repository
                    .scoped_totp_status(ctx.data_scope().tenant_id().as_str(), &input.user_id)
                    .map_err(Self::repository_error)?;
                Ok(serde_json::json!({
                    "ok": true,
                    "enabled": status.enabled,
                    "hasSecret": status.has_secret
                }))
            }
            _ => Err(Self::error(
                "sys",
                "CMD_UNKNOWN",
                format!("Unknown command: {command}"),
            )),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "generate_secret",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "verify_and_enable",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "disable",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "status",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        FeatureTwoFa::new().schema()
    }
}

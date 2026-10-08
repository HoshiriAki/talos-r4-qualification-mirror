//! 用户设置读写模块 (leaf — 无跨模块依赖)

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use system_core::*;

// ── 输入类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetSettingsInput {
    pub user_id: String,
}

impl Validate for GetSettingsInput {
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveSettingsInput {
    pub user_id: String,
    pub settings: Value,
}

impl Validate for SaveSettingsInput {
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

// ── 输入类型（不需要清理，不实现 Sanitize）──

impl Sanitize for GetSettingsInput {
    fn sanitize(&mut self) {
        self.user_id = self.user_id.trim().to_string();
    }
}

impl Sanitize for SaveSettingsInput {
    fn sanitize(&mut self) {
        self.user_id = self.user_id.trim().to_string();
    }
}

// ── 输出类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SaveSettingsOutput {
    pub success: bool,
}

// ── 模块主体 ──

pub struct FeatureUserSettings;

impl FeatureUserSettings {
    pub fn new() -> Self {
        Self
    }

    fn do_get_settings(&self, _user_id: &str, _scope: &DataScope) -> Result<Value, String> {
        // 返回空设置 — 实际 DB 操作在 talos-backend 的适配器层
        Ok(serde_json::json!({}))
    }

    fn do_save_settings(
        &self,
        _user_id: &str,
        _settings: &Value,
        _scope: &DataScope,
    ) -> Result<SaveSettingsOutput, String> {
        Ok(SaveSettingsOutput { success: true })
    }
}

impl Default for FeatureUserSettings {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureUserSettings {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "user_settings".into(),
            version: "0.1.0".into(),
            description: "用户设置读写模块".into(),
            author: "hoshi".into(),
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
            "get_settings" => {
                DeserializeGuard::default().check_raw(&payload)?;

                let unvalidated: Unvalidated<GetSettingsInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                let result = self.do_get_settings(&input.user_id, ctx.data_scope())?;
                serde_json::to_value(result).map_err(|e| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })
            }
            "save_settings" => {
                DeserializeGuard::default().check_raw(&payload)?;

                let unvalidated: Unvalidated<SaveSettingsInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                let result =
                    self.do_save_settings(&input.user_id, &input.settings, ctx.data_scope())?;
                serde_json::to_value(result).map_err(|e| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("未知命令: {}", command),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        vec![
            system_core::CommandMetadata::new(
                "get_settings",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "save_settings",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "user_settings".into(),
            description: "用户设置读写模块".into(),
            commands: vec![
                CommandSchema {
                    name: "get_settings".into(),
                    description: "读取用户设置".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "save_settings".into(),
                    description: "保存用户设置".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

//! feature-miniapp — 微信小程序登录 Maxwell 模块
//!
//! code2session + 客户订单/设备查询。
//! 依赖: system-core（SystemModule trait）
//!
//! 当前为 STUB 模式：如果未配置环境变量，所有命令返回 stub 响应。

use serde_json::Value;
use std::sync::Mutex;
use system_core::*;

// ── 配置 ──

#[derive(Debug, Clone)]
pub struct MiniappConfig {
    pub appid: String,
    pub secret: String,
}

// ── 模块主体 ──

pub struct FeatureMiniapp {
    config: Mutex<Option<MiniappConfig>>,
}

impl FeatureMiniapp {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(None),
        }
    }
}

impl Default for FeatureMiniapp {
    fn default() -> Self {
        Self::new()
    }
}

// ── SystemModule 实现 ──

impl SystemModule for FeatureMiniapp {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "miniapp".into(),
            version: "0.1.0".into(),
            description: "微信小程序登录 — code2session + 客户订单/设备查询".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: None,
        }
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        let appid = config
            .get("appid")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "MINIAPP_APPID 缺失或为空".to_string())?
            .to_string();
        let secret = config
            .get("secret")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "MINIAPP_SECRET 缺失或为空".to_string())?
            .to_string();

        let cfg = MiniappConfig { appid, secret };

        let mut guard = self.config.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        *guard = Some(cfg);
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let guard = self.config.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let _cfg = guard
            .as_ref()
            .ok_or_else(|| "MINIAPP_NOT_INIT: 小程序模块未初始化".to_string())?;

        match command {
            "code2session" => Self::stub_code2session(payload),
            "get_customer_orders" => Self::stub_get_customer_orders(payload),
            "get_customer_devices" => Self::stub_get_customer_devices(payload),
            "get_config" => Self::get_config(&guard),
            _ => Err(format!("未知命令: {}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        use schemars::schema_for;

        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct Code2SessionInput {
            code: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct Code2SessionOutput {
            ok: bool,
            openid: String,
            session_key: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct CustomerOrdersInput {
            openid: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            page: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            page_size: Option<u32>,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct CustomerOrdersOutput {
            ok: bool,
            orders: Vec<Value>,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct CustomerDevicesInput {
            openid: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct CustomerDevicesOutput {
            ok: bool,
            devices: Vec<Value>,
        }

        ModuleSchema {
            name: "miniapp".into(),
            description: "微信小程序登录 API".into(),
            commands: vec![
                CommandSchema {
                    name: "code2session".into(),
                    description: "用 wx.login 返回的 code 换取 openid 和 session_key".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(Code2SessionInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(Code2SessionOutput)).ok(),
                },
                CommandSchema {
                    name: "get_customer_orders".into(),
                    description: "查询客户订单列表".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(CustomerOrdersInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(CustomerOrdersOutput)).ok(),
                },
                CommandSchema {
                    name: "get_customer_devices".into(),
                    description: "查询客户设备列表".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(CustomerDevicesInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(CustomerDevicesOutput)).ok(),
                },
                CommandSchema {
                    name: "get_config".into(),
                    description: "返回当前配置状态（脱敏）".into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "code2session",
                AccessRequirement::Authenticated,
                &[EffectClass::ExternalHttp],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "get_customer_orders",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_customer_devices",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_config",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }
}

// ── Stub 命令实现 ──

impl FeatureMiniapp {
    fn stub_code2session(payload: Value) -> Result<Value, String> {
        let _code = payload
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "缺少 code".to_string())?;
        Ok(serde_json::json!({
            "ok": true,
            "openid": "stub_openid",
            "sessionKey": "stub_key",
        }))
    }

    fn stub_get_customer_orders(_payload: Value) -> Result<Value, String> {
        Ok(serde_json::json!({
            "ok": true,
            "orders": [],
        }))
    }

    fn stub_get_customer_devices(_payload: Value) -> Result<Value, String> {
        Ok(serde_json::json!({
            "ok": true,
            "devices": [],
        }))
    }

    fn get_config(guard: &std::sync::MutexGuard<Option<MiniappConfig>>) -> Result<Value, String> {
        match guard.as_ref() {
            Some(cfg) => Ok(serde_json::json!({
                "configured": true,
                "appid": mask(&cfg.appid),
            })),
            None => Ok(serde_json::json!({"configured": false})),
        }
    }
}

fn mask(s: &str) -> String {
    if s.len() <= 8 {
        return "***".to_string();
    }
    format!("{}****{}", &s[..4], &s[s.len() - 4..])
}

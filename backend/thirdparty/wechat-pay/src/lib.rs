//! feature-wechat-pay — 微信支付 v3 API Maxwell 模块
//!
//! JSAPI 统一下单 + 订单查询 + 退款 + 回调处理。
//! 依赖: system-core（SystemModule trait）
//!
//! 当前为 STUB 模式：如果未配置环境变量，所有命令返回 stub 响应。

use serde_json::Value;
use std::sync::Mutex;
use system_core::*;

// ── 配置 ──

#[derive(Debug, Clone)]
pub struct WechatPayConfig {
    pub appid: String,
    pub mchid: String,
    pub api_v3_key: String,
    pub serial_no: String,
    pub private_key_pem: String,
    pub notify_url: String,
    pub base_url: String,
}

// ── 模块主体 ──

pub struct FeatureWechatPay {
    config: Mutex<Option<WechatPayConfig>>,
}

impl FeatureWechatPay {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(None),
        }
    }
}

impl Default for FeatureWechatPay {
    fn default() -> Self {
        Self::new()
    }
}

// ── SystemModule 实现 ──

impl SystemModule for FeatureWechatPay {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "wechat_pay".into(),
            version: "0.1.0".into(),
            description: "微信支付 v3 — JSAPI 下单/查询/退款 + 回调验证".into(),
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
            .ok_or_else(|| "WECHAT_PAY_APPID 缺失或为空".to_string())?
            .to_string();
        let mchid = config
            .get("mchid")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "WECHAT_PAY_MCHID 缺失或为空".to_string())?
            .to_string();
        let api_v3_key = config
            .get("apiV3Key")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "WECHAT_PAY_API_V3_KEY 缺失或为空".to_string())?
            .to_string();
        let serial_no = config
            .get("serialNo")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let private_key_pem = config
            .get("privateKeyPem")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let notify_url = config
            .get("notifyUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let base_url = config
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("https://api.mch.weixin.qq.com");

        let cfg = WechatPayConfig {
            appid,
            mchid,
            api_v3_key,
            serial_no: serial_no.to_string(),
            private_key_pem: private_key_pem.to_string(),
            notify_url: notify_url.to_string(),
            base_url: base_url.to_string(),
        };

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
            .ok_or_else(|| "WECHAT_PAY_NOT_INIT: 微信支付模块未初始化".to_string())?;

        match command {
            "create_jsapi_order" => Self::stub_create_jsapi_order(payload),
            "query_order" => Self::stub_query_order(payload),
            "refund" => Self::stub_refund(payload),
            "handle_callback" => Self::stub_handle_callback(payload),
            "get_config" => Self::get_config(&guard),
            _ => Err(format!("未知命令: {}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        use schemars::schema_for;

        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct JsapiInput {
            order_id: String,
            amount: f64,
            openid: String,
            description: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct JsapiOutput {
            ok: bool,
            prepay_id: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct QueryInput {
            out_trade_no: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct QueryOutput {
            ok: bool,
            trade_state: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct RefundInput {
            out_trade_no: String,
            refund_amount: f64,
            out_refund_no: String,
            total_amount: f64,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct RefundOutput {
            ok: bool,
            status: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct CallbackInput {
            headers: Value,
            body: Value,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct CallbackOutput {
            ok: bool,
            verified: bool,
            note: String,
        }

        ModuleSchema {
            name: "wechat_pay".into(),
            description: "微信支付 v3 API".into(),
            commands: vec![
                CommandSchema {
                    name: "create_jsapi_order".into(),
                    description: "JSAPI 统一下单".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(JsapiInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(JsapiOutput)).ok(),
                },
                CommandSchema {
                    name: "query_order".into(),
                    description: "按商户订单号查询订单状态".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(QueryInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(QueryOutput)).ok(),
                },
                CommandSchema {
                    name: "refund".into(),
                    description: "发起退款".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(RefundInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(RefundOutput)).ok(),
                },
                CommandSchema {
                    name: "handle_callback".into(),
                    description: "处理微信支付回调通知".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(CallbackInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(CallbackOutput)).ok(),
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
                "create_jsapi_order",
                AccessRequirement::Authenticated,
                &[EffectClass::Payment],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "query_order",
                AccessRequirement::Authenticated,
                &[EffectClass::Payment],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "refund",
                AccessRequirement::TenantAdmin,
                &[EffectClass::Payment],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "handle_callback",
                AccessRequirement::System,
                &[EffectClass::Webhook, EffectClass::Payment],
                SimulationSupport::Blocked,
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

impl FeatureWechatPay {
    fn stub_create_jsapi_order(payload: Value) -> Result<Value, String> {
        let order_id = payload
            .get("orderId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "缺少 orderId".to_string())?;
        Ok(serde_json::json!({
            "ok": true,
            "prepayId": format!("stub_prepay_{}", order_id),
        }))
    }

    fn stub_query_order(_payload: Value) -> Result<Value, String> {
        Ok(serde_json::json!({
            "ok": true,
            "tradeState": "NOTPAY",
        }))
    }

    fn stub_refund(payload: Value) -> Result<Value, String> {
        let _out_refund_no = payload
            .get("outRefundNo")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "缺少 outRefundNo".to_string())?;
        Ok(serde_json::json!({
            "ok": true,
            "status": "processing",
        }))
    }

    fn stub_handle_callback(_payload: Value) -> Result<Value, String> {
        Ok(serde_json::json!({
            "ok": true,
            "verified": false,
            "note": "webhook stub",
        }))
    }

    fn get_config(guard: &std::sync::MutexGuard<Option<WechatPayConfig>>) -> Result<Value, String> {
        match guard.as_ref() {
            Some(cfg) => Ok(serde_json::json!({
                "configured": true,
                "appid": mask(&cfg.appid),
                "mchid": mask(&cfg.mchid),
                "serial_no": mask(&cfg.serial_no),
                "notify_url": cfg.notify_url,
            })),
            None => Ok(serde_json::json!({"configured": false})),
        }
    }
}

/// 脱敏辅助：仅保留前 4 位 + 后 4 位
fn mask(s: &str) -> String {
    if s.len() <= 8 {
        return "***".to_string();
    }
    format!("{}****{}", &s[..4], &s[s.len() - 4..])
}

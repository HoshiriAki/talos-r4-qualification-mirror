//! feature-alipay — 支付宝页面支付 Maxwell 模块
//!
//! 页面支付 + 查询 + 退款 + 回调处理。
//! 依赖: system-core（SystemModule trait）
//!
//! 当前为 STUB 模式：如果未配置环境变量，所有命令返回 stub 响应。

use serde_json::Value;
use std::sync::Mutex;
use system_core::*;

// ── 配置 ──

#[derive(Debug, Clone)]
pub struct AlipayConfig {
    pub app_id: String,
    pub private_key_pem: String,
    pub alipay_public_key_pem: String,
    pub notify_url: String,
    pub return_url: String,
    pub base_url: String,
}

// ── 模块主体 ──

pub struct FeatureAlipay {
    config: Mutex<Option<AlipayConfig>>,
}

impl FeatureAlipay {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(None),
        }
    }
}

impl Default for FeatureAlipay {
    fn default() -> Self {
        Self::new()
    }
}

// ── SystemModule 实现 ──

impl SystemModule for FeatureAlipay {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "alipay".into(),
            version: "0.1.0".into(),
            description: "支付宝页面支付 — 下单/查询/退款 + 回调验证".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: None,
        }
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        let app_id = config
            .get("appId")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "ALIPAY_APP_ID 缺失或为空".to_string())?
            .to_string();
        let private_key_pem = config
            .get("privateKeyPem")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let alipay_public_key_pem = config
            .get("alipayPublicKeyPem")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let notify_url = config
            .get("notifyUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let return_url = config
            .get("returnUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let base_url = config
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("https://openapi.alipay.com/gateway.do");

        let cfg = AlipayConfig {
            app_id,
            private_key_pem: private_key_pem.to_string(),
            alipay_public_key_pem: alipay_public_key_pem.to_string(),
            notify_url: notify_url.to_string(),
            return_url: return_url.to_string(),
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
            .ok_or_else(|| "ALIPAY_NOT_INIT: 支付宝模块未初始化".to_string())?;

        match command {
            "page_pay" => Self::stub_page_pay(payload),
            "query" => Self::stub_query(payload),
            "refund" => Self::stub_refund(payload),
            "handle_callback" => Self::stub_handle_callback(payload),
            "get_config" => Self::get_config(&guard),
            _ => Err(format!("未知命令: {}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        use schemars::schema_for;

        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct PagePayInput {
            order_id: String,
            subject: String,
            total_amount: f64,
            #[serde(skip_serializing_if = "Option::is_none")]
            body: Option<String>,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct PagePayOutput {
            ok: bool,
            pay_url: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct QueryInput {
            out_trade_no: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct QueryOutput {
            ok: bool,
            trade_status: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct RefundInput {
            out_trade_no: String,
            refund_amount: f64,
            out_request_no: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct RefundOutput {
            ok: bool,
            status: String,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct CallbackInput {
            params: Value,
        }
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct CallbackOutput {
            ok: bool,
            verified: bool,
            note: String,
        }

        ModuleSchema {
            name: "alipay".into(),
            description: "支付宝页面支付 API".into(),
            commands: vec![
                CommandSchema {
                    name: "page_pay".into(),
                    description: "发起页面支付，返回跳转 URL".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(PagePayInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(PagePayOutput)).ok(),
                },
                CommandSchema {
                    name: "query".into(),
                    description: "按商户订单号查询支付状态".into(),
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
                    description: "处理支付宝异步通知".into(),
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
                "page_pay",
                AccessRequirement::Authenticated,
                &[EffectClass::Payment],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "query",
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

impl FeatureAlipay {
    fn stub_page_pay(payload: Value) -> Result<Value, String> {
        let order_id = payload
            .get("orderId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "缺少 orderId".to_string())?;
        Ok(serde_json::json!({
            "ok": true,
            "payUrl": format!("stub_alipay_redirect_url?order={}", order_id),
        }))
    }

    fn stub_query(_payload: Value) -> Result<Value, String> {
        Ok(serde_json::json!({
            "ok": true,
            "tradeStatus": "WAIT_BUYER_PAY",
        }))
    }

    fn stub_refund(payload: Value) -> Result<Value, String> {
        let _out_request_no = payload
            .get("outRequestNo")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "缺少 outRequestNo".to_string())?;
        Ok(serde_json::json!({
            "ok": true,
            "status": "success",
        }))
    }

    fn stub_handle_callback(_payload: Value) -> Result<Value, String> {
        Ok(serde_json::json!({
            "ok": true,
            "verified": false,
            "note": "webhook stub",
        }))
    }

    fn get_config(guard: &std::sync::MutexGuard<Option<AlipayConfig>>) -> Result<Value, String> {
        match guard.as_ref() {
            Some(cfg) => Ok(serde_json::json!({
                "configured": true,
                "app_id": mask(&cfg.app_id),
                "notify_url": cfg.notify_url,
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

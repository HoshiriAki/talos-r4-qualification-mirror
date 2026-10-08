//! feature-sf-express — 顺丰丰桥 API 原生 Maxwell 模块
//!
//! 6 个 API 命令 + 2 个 webhook 命令。
//! 依赖: system-core（HttpClient trait, SystemModule trait, Sanitize, Validate）

pub mod sf_client;
pub mod signer;
pub mod types;
pub mod webhook_handler;

// ── 错误辅助函数 ──

/// 将错误码 + 消息序列化为 ErrorPayload JSON 字符串。
/// 所有命令和 SfClient 统一使用此函数构建错误。
#[allow(dead_code)]
pub(crate) fn err_payload(code: &str, message: &str) -> String {
    serde_json::to_string(&system_core::ErrorPayload {
        category: if code.starts_with("SYS") {
            "sys".into()
        } else {
            "biz".into()
        },
        code: code.into(),
        message: message.into(),
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

// ── 模块主体依赖 ──

use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use system_core::transport::http_client::HttpClient;
use system_core::*;

use crate::types::cancel_order::*;
use crate::types::cloud_print::*;
use crate::types::create_order::*;
use crate::types::query_delivery_time::*;
use crate::types::query_routes::*;
use crate::types::register_route::*;

// ── 配置 ──

#[derive(Debug, Clone)]
pub struct SfExpressConfig {
    pub partner_id: String,
    pub check_word: String,
    pub monthly_card: String,
    pub base_url: String,
}

// ── 模块主体 ──

pub struct FeatureSfExpress {
    config: Mutex<Option<SfExpressConfig>>,
    webhook_dedup: Mutex<HashSet<String>>,
}

impl FeatureSfExpress {
    pub fn new() -> Self {
        Self {
            config: Mutex::new(None),
            webhook_dedup: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for FeatureSfExpress {
    fn default() -> Self {
        Self::new()
    }
}

// ── SystemModule 实现 ──

impl SystemModule for FeatureSfExpress {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "sf_express".into(),
            version: "0.1.0".into(),
            description: "顺丰丰桥 API — 运单创建/取消/路由查询/面单打印/时效预估 + webhook 处理"
                .into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: None,
        }
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        let partner_id = config
            .get("partnerId")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| err_payload("SF_CONFIG_ERROR", "SF_PARTNER_ID 环境变量缺失或为空"))?
            .to_string();
        let check_word = config
            .get("checkWord")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| err_payload("SF_CONFIG_ERROR", "SF_CHECK_WORD 环境变量缺失或为空"))?
            .to_string();

        let cfg = SfExpressConfig {
            partner_id,
            check_word,
            monthly_card: config
                .get("monthlyCard")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            base_url: config
                .get("baseUrl")
                .and_then(|v| v.as_str())
                .unwrap_or("https://sfapi-sbox.sf-express.com/std/service")
                .to_string(),
        };

        let mut guard = self
            .config
            .lock()
            .map_err(|e| err_payload("SYS_LOCK", &e.to_string()))?;
        *guard = Some(cfg);
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let http = ctx.http_client();
        let guard = self
            .config
            .lock()
            .map_err(|e| err_payload("SYS_LOCK", &e.to_string()))?;
        let cfg = guard
            .as_ref()
            .ok_or_else(|| err_payload("SYS_NOT_INIT", "SF 模块未初始化"))?;

        match command {
            "create_order" => self.do_create_order(&payload, http, cfg, ctx),
            "cancel_order" => self.do_cancel_order(&payload, http, cfg, ctx),
            "register_route" => self.do_register_route(&payload, http, cfg, ctx),
            "query_routes" => self.do_query_routes(&payload, http, cfg, ctx),
            "cloud_print_waybill" => self.do_cloud_print(&payload, http, cfg, ctx),
            "query_delivery_time" => self.do_delivery_time(&payload, http, cfg, ctx),
            "verify_webhook" => self.do_verify_webhook(&payload),
            "process_webhook" => self.do_process_webhook(&payload),
            _ => Err(err_payload(
                "SYS_UNKNOWN_COMMAND",
                &format!("未知命令: {}", command),
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        use schemars::schema_for;
        ModuleSchema {
            name: "sf_express".into(),
            description: "顺丰丰桥 API".into(),
            commands: vec![
                CommandSchema {
                    name: "create_order".into(),
                    description: "下订单 → 分配运单号 + 筛单".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(CreateOrderInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(CreateOrderOutput)).ok(),
                },
                CommandSchema {
                    name: "cancel_order".into(),
                    description: "确认/取消运单".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(CancelOrderInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(CancelOrderOutput)).ok(),
                },
                CommandSchema {
                    name: "register_route".into(),
                    description: "注册运单路由推送".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(RegisterRouteInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(RegisterRouteOutput)).ok(),
                },
                CommandSchema {
                    name: "query_routes".into(),
                    description: "物流轨迹查询（3个月内）".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(QueryRoutesInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(RouteResps)).ok(),
                },
                CommandSchema {
                    name: "cloud_print_waybill".into(),
                    description: "同步/异步生成面单 PDF".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(CloudPrintInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(CloudPrintOutput)).ok(),
                },
                CommandSchema {
                    name: "query_delivery_time".into(),
                    description: "时效预估".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schema_for!(QueryDeliveryTimeInput)).ok(),
                    output_schema: serde_json::to_value(schema_for!(DeliveryTimeOutput)).ok(),
                },
                CommandSchema {
                    name: "verify_webhook".into(),
                    description: "验证 SF 回调签名".into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "process_webhook".into(),
                    description: "解析 SF 回调事件".into(),
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
                "create_order",
                AccessRequirement::Authenticated,
                &[EffectClass::Logistics],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "cancel_order",
                AccessRequirement::Authenticated,
                &[EffectClass::Logistics],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "register_route",
                AccessRequirement::TenantAdmin,
                &[EffectClass::Logistics],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "query_routes",
                AccessRequirement::Authenticated,
                &[EffectClass::Logistics],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "cloud_print_waybill",
                AccessRequirement::Authenticated,
                &[EffectClass::Logistics],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "query_delivery_time",
                AccessRequirement::Authenticated,
                &[EffectClass::Logistics],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "verify_webhook",
                AccessRequirement::System,
                &[EffectClass::Webhook, EffectClass::Logistics],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "process_webhook",
                AccessRequirement::System,
                &[EffectClass::Webhook, EffectClass::Logistics],
                SimulationSupport::Blocked,
            ),
        ]
    }
}

// ── 前向声明：命令方法（实现在 Task 7-8）──
// These are impl FeatureSfExpress methods called from execute() above.
// We declare them as stub methods so the module compiles.

impl FeatureSfExpress {
    fn do_create_order(
        &self,
        payload: &Value,
        http: &Arc<dyn HttpClient>,
        cfg: &SfExpressConfig,
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        DeserializeGuard::default().check_raw(payload)?;
        let input: CreateOrderInput = serde_json::from_value(payload.clone())
            .map_err(|e| err_payload("SF_INPUT_PARSE_ERROR", &format!("输入解析失败: {}", e)))?;
        let mut input = input;
        input.sanitize();
        let validation = input.validate();
        if !validation.is_valid() {
            return Err(validation.to_json());
        }

        let msg_data = serde_json::json!({
            "language": input.language,
            "orderId": input.order_id,
            "expressTypeId": input.express_type_id,
            "isGenWaybillNo": input.is_gen_waybill_no,
            "isUnifiedWaybillNo": input.is_unified_waybill_no,
            "cargoDetails": input.cargo_details,
            "contactInfoList": input.contact_info_list,
            "monthlyCard": &cfg.monthly_card,
        });

        let response = crate::sf_client::SfClient::call(
            http.as_ref(),
            &cfg.partner_id,
            &cfg.check_word,
            &cfg.base_url,
            "EXP_RECE_CREATE_ORDER",
            &msg_data,
        )?;

        let data_str = response
            .get("apiResultData")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err_payload("SF_RESPONSE_MALFORMED", "缺少 apiResultData"))?;
        let output: CreateOrderOutput = serde_json::from_str(data_str)
            .map_err(|e| err_payload("SF_RESPONSE_PARSE_ERROR", &e.to_string()))?;

        match output.filter_result {
            2 => serde_json::to_value(output)
                .map_err(|e| err_payload("SYS_SERIALIZE_ERROR", &e.to_string())),
            1 => Err(err_payload("SF_NEED_MANUAL_REVIEW", "筛单需人工确认")),
            3 => Err(err_payload("SF_REJECTED", "不可收派")),
            4 => Err(err_payload(
                "SF_UNDETERMINED",
                "筛单结果无法确定，请稍后重试",
            )),
            _ => Err(err_payload(
                "SF_UNKNOWN_FILTER",
                &format!("未知筛单结果: {}", output.filter_result),
            )),
        }
    }
    fn do_cancel_order(
        &self,
        payload: &Value,
        http: &Arc<dyn HttpClient>,
        cfg: &SfExpressConfig,
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        DeserializeGuard::default().check_raw(payload)?;
        let input: CancelOrderInput = serde_json::from_value(payload.clone())
            .map_err(|e| err_payload("SF_INPUT_PARSE_ERROR", &format!("输入解析失败: {}", e)))?;
        let mut input = input;
        input.sanitize();
        let validation = input.validate();
        if !validation.is_valid() {
            return Err(validation.to_json());
        }

        let msg_data = serde_json::json!({
            "language": input.language,
            "orderId": input.order_id,
            "dealType": input.deal_type,
            "remark": input.remark,
        });

        let response = crate::sf_client::SfClient::call(
            http.as_ref(),
            &cfg.partner_id,
            &cfg.check_word,
            &cfg.base_url,
            "EXP_RECE_UPDATE_ORDER",
            &msg_data,
        )?;

        let data_str = response
            .get("apiResultData")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err_payload("SF_RESPONSE_MALFORMED", "缺少 apiResultData"))?;
        let output: CancelOrderOutput = serde_json::from_str(data_str)
            .map_err(|e| err_payload("SF_RESPONSE_PARSE_ERROR", &e.to_string()))?;

        serde_json::to_value(output).map_err(|e| err_payload("SYS_SERIALIZE_ERROR", &e.to_string()))
    }
    fn do_register_route(
        &self,
        payload: &Value,
        http: &Arc<dyn HttpClient>,
        cfg: &SfExpressConfig,
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        DeserializeGuard::default().check_raw(payload)?;
        let input: RegisterRouteInput = serde_json::from_value(payload.clone())
            .map_err(|e| err_payload("SF_INPUT_PARSE_ERROR", &format!("输入解析失败: {}", e)))?;
        let mut input = input;
        input.sanitize();
        let validation = input.validate();
        if !validation.is_valid() {
            return Err(validation.to_json());
        }

        let msg_data = serde_json::json!({
            "language": input.language,
            "trackingType": input.tracking_type,
            "trackingNumber": input.tracking_number,
            "callbackUrl": input.callback_url,
        });

        let response = crate::sf_client::SfClient::call(
            http.as_ref(),
            &cfg.partner_id,
            &cfg.check_word,
            &cfg.base_url,
            "EXP_RECE_REGISTER_ROUTE",
            &msg_data,
        )?;

        let data_str = response
            .get("apiResultData")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err_payload("SF_RESPONSE_MALFORMED", "缺少 apiResultData"))?;
        let output: RegisterRouteOutput = serde_json::from_str(data_str)
            .map_err(|e| err_payload("SF_RESPONSE_PARSE_ERROR", &e.to_string()))?;

        serde_json::to_value(output).map_err(|e| err_payload("SYS_SERIALIZE_ERROR", &e.to_string()))
    }
    fn do_query_routes(
        &self,
        payload: &Value,
        http: &Arc<dyn HttpClient>,
        cfg: &SfExpressConfig,
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        DeserializeGuard::default().check_raw(payload)?;
        let input: QueryRoutesInput = serde_json::from_value(payload.clone())
            .map_err(|e| err_payload("SF_INPUT_PARSE_ERROR", &format!("输入解析失败: {}", e)))?;
        let mut input = input;
        input.sanitize();
        let validation = input.validate();
        if !validation.is_valid() {
            return Err(validation.to_json());
        }

        let msg_data = serde_json::json!({
            "language": input.language,
            "trackingType": input.tracking_type,
            "trackingNumber": input.tracking_number,
            "methodType": input.method_type,
        });

        let response = crate::sf_client::SfClient::call(
            http.as_ref(),
            &cfg.partner_id,
            &cfg.check_word,
            &cfg.base_url,
            "EXP_RECE_SEARCH_ROUTES",
            &msg_data,
        )?;

        let data_str = response
            .get("apiResultData")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err_payload("SF_RESPONSE_MALFORMED", "缺少 apiResultData"))?;
        let output: RouteResps = serde_json::from_str(data_str)
            .map_err(|e| err_payload("SF_RESPONSE_PARSE_ERROR", &e.to_string()))?;

        serde_json::to_value(output).map_err(|e| err_payload("SYS_SERIALIZE_ERROR", &e.to_string()))
    }
    fn do_cloud_print(
        &self,
        payload: &Value,
        http: &Arc<dyn HttpClient>,
        cfg: &SfExpressConfig,
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        DeserializeGuard::default().check_raw(payload)?;
        let input: CloudPrintInput = serde_json::from_value(payload.clone())
            .map_err(|e| err_payload("SF_INPUT_PARSE_ERROR", &format!("输入解析失败: {}", e)))?;
        let mut input = input;
        input.sanitize();
        let validation = input.validate();
        if !validation.is_valid() {
            return Err(validation.to_json());
        }

        let msg_data = serde_json::json!({
            "templateCode": input.template_code,
            "version": input.version,
            "sync": input.sync,
            "fileType": input.file_type,
            "documents": input.documents,
        });

        let response = crate::sf_client::SfClient::call(
            http.as_ref(),
            &cfg.partner_id,
            &cfg.check_word,
            &cfg.base_url,
            "COM_RECE_CLOUD_PRINT_WAYBILLS",
            &msg_data,
        )?;

        let data_str = response
            .get("apiResultData")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err_payload("SF_RESPONSE_MALFORMED", "缺少 apiResultData"))?;
        let output: CloudPrintOutput = serde_json::from_str(data_str)
            .map_err(|e| err_payload("SF_RESPONSE_PARSE_ERROR", &e.to_string()))?;

        serde_json::to_value(output).map_err(|e| err_payload("SYS_SERIALIZE_ERROR", &e.to_string()))
    }
    fn do_delivery_time(
        &self,
        payload: &Value,
        http: &Arc<dyn HttpClient>,
        cfg: &SfExpressConfig,
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        DeserializeGuard::default().check_raw(payload)?;
        let input: QueryDeliveryTimeInput = serde_json::from_value(payload.clone())
            .map_err(|e| err_payload("SF_INPUT_PARSE_ERROR", &format!("输入解析失败: {}", e)))?;
        let mut input = input;
        input.sanitize();
        let validation = input.validate();
        if !validation.is_valid() {
            return Err(validation.to_json());
        }

        let msg_data = serde_json::json!({
            "language": input.language,
            "origin": input.origin,
            "dest": input.dest,
            "expressTypeId": input.express_type_id,
        });

        let response = crate::sf_client::SfClient::call(
            http.as_ref(),
            &cfg.partner_id,
            &cfg.check_word,
            &cfg.base_url,
            "EXP_RECE_QUERY_DELIVERTM",
            &msg_data,
        )?;

        let data_str = response
            .get("apiResultData")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err_payload("SF_RESPONSE_MALFORMED", "缺少 apiResultData"))?;
        let output: DeliveryTimeOutput = serde_json::from_str(data_str)
            .map_err(|e| err_payload("SF_RESPONSE_PARSE_ERROR", &e.to_string()))?;

        serde_json::to_value(output).map_err(|e| err_payload("SYS_SERIALIZE_ERROR", &e.to_string()))
    }

    // ── webhook 方法（Task 8 实现）──
    fn do_verify_webhook(&self, payload: &Value) -> Result<Value, String> {
        let body = payload
            .get("body")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err_payload("SF_WEBHOOK_INVALID_INPUT", "缺少 body 参数"))?;

        let guard = self
            .config
            .lock()
            .map_err(|e| err_payload("SYS_LOCK", &e.to_string()))?;
        let cfg = guard
            .as_ref()
            .ok_or_else(|| err_payload("SYS_NOT_INIT", "SF 模块未初始化"))?;

        let processor = crate::webhook_handler::SfWebhookProcessor::new(cfg.check_word.clone());
        let valid = processor.verify_signature(body).unwrap_or(false);

        Ok(serde_json::json!({"valid": valid}))
    }

    fn do_process_webhook(&self, payload: &Value) -> Result<Value, String> {
        let body = payload
            .get("body")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err_payload("SF_WEBHOOK_INVALID_INPUT", "缺少 body 参数"))?;

        // Idempotency: extract requestID, skip if already processed
        let params = crate::webhook_handler::parse_form(body)?;
        let request_id = params.get("requestID").cloned().unwrap_or_default();

        {
            let mut dedup = self
                .webhook_dedup
                .lock()
                .map_err(|e| err_payload("SYS_LOCK", &e.to_string()))?;
            if dedup.contains(&request_id) {
                return Ok(serde_json::json!({"processed": true, "duplicate": true}));
            }
            dedup.insert(request_id);
        }

        let guard = self
            .config
            .lock()
            .map_err(|e| err_payload("SYS_LOCK", &e.to_string()))?;
        let cfg = guard
            .as_ref()
            .ok_or_else(|| err_payload("SYS_NOT_INIT", "SF 模块未初始化"))?;

        let processor = crate::webhook_handler::SfWebhookProcessor::new(cfg.check_word.clone());

        // 纵深防御：即使路由层已验签，模块内部也二次验证
        if !processor.verify_signature(body).unwrap_or(false) {
            return Err(err_payload("SF_WEBHOOK_INVALID_SIGNATURE", "签名验证失败"));
        }

        let event_data = processor.parse_webhook(body)?;

        // Track event for future shipment status updates (handled by caller)
        Ok(serde_json::json!({
            "processed": true,
            "duplicate": false,
            "event": event_data,
        }))
    }
}

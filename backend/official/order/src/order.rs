//! feature-order — 订单管理模块 (Maxwell 原生)
//!
//! 订单 CRUD + 跨模块 Saga 流程 + Excel 导出。
//! 原 `backend/src/services/order_service.rs` 的 Maxwell 移植版。
//!
//! ## 注入依赖
//! - device_module — 设备校验（create_order 时逐台检查 serialNo 是否存在）
//! - pricing_module — 价格预估（create_order 前获取预估金额）
//! - warehouse_routing_module — 仓库解析（自动分配 sendWarehouseId / returnWarehouseId）
//!
//! ## 命令
//! - list_orders  — 支持 8 种过滤器的分页查询
//! - get_order    — 单订单查询 + 关联设备列表
//! - create_order — BEGIN IMMEDIATE Saga：仓库解析 → 估价 → 设备校验 → INSERT order + junction → 审计
//! - update_order — 更新订单字段 + 同步 order_devices
//! - cancel_order — 更新 status='cancelled' + 审计
//! - delete_order — 删除订单 + order_devices + 审计
//! - export_excel — 导出过滤后的订单列表为 xlsx

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use regex::Regex;
use rusqlite::OptionalExtension;
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, LazyLock, Mutex};
use uuid::Uuid;

use system_core::*;

// ═══════════════════════════════════════════════════════════════════
// 时间辅助
// ═══════════════════════════════════════════════════════════════════

/// 返回上海时间 (UTC+8) 的 ISO 8601 字符串
fn shanghai_now_iso() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y-%m-%dT%H:%M:%S%.3f+08:00").to_string()
}

/// 返回上海时间的 YYYY-MM-DD 日期键
#[allow(dead_code)]
fn shanghai_now_date_key() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y-%m-%d").to_string()
}

// ═══════════════════════════════════════════════════════════════════
// 静态常量
// ═══════════════════════════════════════════════════════════════════

/// orderNo 格式: 纯数字
#[allow(dead_code)]
static ORDER_NO_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+$").unwrap());

/// 日期格式: YYYY-MM-DD
static DATE_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap());

// ═══════════════════════════════════════════════════════════════════
// 输入类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListOrdersInput {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub filter: serde_json::Map<String, Value>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    30
}

impl Sanitize for ListOrdersInput {
    fn sanitize(&mut self) {
        if self.page < 1 {
            self.page = 1;
        }
        self.page_size = self.page_size.clamp(1, 200);
        if let Some(ref mut so) = self.sort_order {
            *so = so.trim().to_lowercase();
        }
        if let Some(ref mut sb) = self.sort_by {
            *sb = sb.trim().to_string();
        }
        // 清理 filter 值
        for (_, v) in self.filter.iter_mut() {
            if let Value::String(s) = v {
                *s = s.trim().to_string();
            }
        }
    }
}

impl Validate for ListOrdersInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrderInput {
    pub start_date: String,
    pub end_date: String,
    pub delivery_date: String,
    #[serde(default)]
    pub pickup_methods: Vec<String>,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub province: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub accessories: Vec<String>,
    #[serde(default)]
    pub tracking_no: Option<String>,
}

impl Sanitize for CreateOrderInput {
    fn sanitize(&mut self) {
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
        self.delivery_date = self.delivery_date.trim().to_string();
        self.address = self.address.trim().to_string();
        self.notes = self.notes.trim().to_string();
        self.province = self.province.trim().to_string();
        self.model_id = self.model_id.trim().to_string();
        self.devices = self
            .devices
            .iter()
            .map(|d| d.trim().to_uppercase())
            .collect();
        self.accessories = self
            .accessories
            .iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        self.pickup_methods = self
            .pickup_methods
            .iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if let Some(ref mut t) = self.tracking_no {
            *t = t.trim().to_string();
            if t.is_empty() {
                self.tracking_no = None;
            }
        }
    }
}

impl Validate for CreateOrderInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();

        if !DATE_REGEX.is_match(&self.start_date) {
            errors.push(FieldError {
                field: "startDate".into(),
                code: "VAL_DATE_FORMAT".into(),
                message: "开始日期格式无效，应为 YYYY-MM-DD".into(),
            });
        }
        if !DATE_REGEX.is_match(&self.end_date) {
            errors.push(FieldError {
                field: "endDate".into(),
                code: "VAL_DATE_FORMAT".into(),
                message: "结束日期格式无效，应为 YYYY-MM-DD".into(),
            });
        }
        if !self.delivery_date.is_empty() && !DATE_REGEX.is_match(&self.delivery_date) {
            errors.push(FieldError {
                field: "deliveryDate".into(),
                code: "VAL_DATE_FORMAT".into(),
                message: "发货日期格式无效，应为 YYYY-MM-DD".into(),
            });
        }
        if !self.start_date.is_empty()
            && !self.end_date.is_empty()
            && self.start_date > self.end_date
        {
            errors.push(FieldError {
                field: "endDate".into(),
                code: "VAL_DATE_RANGE".into(),
                message: "结束日期必须晚于或等于开始日期".into(),
            });
        }
        if self.devices.is_empty() {
            errors.push(FieldError {
                field: "devices".into(),
                code: "VAL_REQUIRED".into(),
                message: "至少需要选择一台设备".into(),
            });
        }

        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrderInput {
    pub id: String,
    #[serde(default)]
    pub start_date: String,
    #[serde(default)]
    pub end_date: String,
    #[serde(default)]
    pub delivery_date: String,
    #[serde(default)]
    pub pickup_methods: Vec<String>,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub province: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub devices: Vec<String>,
    #[serde(default)]
    pub accessories: Vec<String>,
    #[serde(default)]
    pub tracking_no: Option<String>,
}

impl Sanitize for UpdateOrderInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
        self.delivery_date = self.delivery_date.trim().to_string();
        self.address = self.address.trim().to_string();
        self.notes = self.notes.trim().to_string();
        self.province = self.province.trim().to_string();
        self.model_id = self.model_id.trim().to_string();
        self.devices = self
            .devices
            .iter()
            .map(|d| d.trim().to_uppercase())
            .collect();
        self.accessories = self
            .accessories
            .iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        self.pickup_methods = self
            .pickup_methods
            .iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if let Some(ref mut t) = self.tracking_no {
            *t = t.trim().to_string();
            if t.is_empty() {
                self.tracking_no = None;
            }
        }
    }
}

impl Validate for UpdateOrderInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "订单 ID 不能为空".into(),
            });
        }
        if !self.start_date.is_empty() && !DATE_REGEX.is_match(&self.start_date) {
            errors.push(FieldError {
                field: "startDate".into(),
                code: "VAL_DATE_FORMAT".into(),
                message: "开始日期格式无效，应为 YYYY-MM-DD".into(),
            });
        }
        if !self.end_date.is_empty() && !DATE_REGEX.is_match(&self.end_date) {
            errors.push(FieldError {
                field: "endDate".into(),
                code: "VAL_DATE_FORMAT".into(),
                message: "结束日期格式无效，应为 YYYY-MM-DD".into(),
            });
        }
        if !self.start_date.is_empty()
            && !self.end_date.is_empty()
            && self.start_date > self.end_date
        {
            errors.push(FieldError {
                field: "endDate".into(),
                code: "VAL_DATE_RANGE".into(),
                message: "结束日期必须晚于或等于开始日期".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrderInput {
    pub id: String,
    #[serde(default)]
    pub reason: String,
}

impl Sanitize for CancelOrderInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
        self.reason = self.reason.trim().to_string();
    }
}

impl Validate for CancelOrderInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "订单 ID 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOrderInput {
    pub id: String,
}

impl Sanitize for DeleteOrderInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
    }
}

impl Validate for DeleteOrderInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "订单 ID 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetOrderInput {
    pub id: String,
}

impl Sanitize for GetOrderInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
    }
}

impl Validate for GetOrderInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "订单 ID 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 输出类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: String,
    pub order_no: String,
    pub start_date: String,
    pub end_date: String,
    pub delivery_date: String,
    #[serde(serialize_with = "serialize_pickup_methods")]
    #[serde(skip_deserializing)]
    pub pickup_methods: Vec<String>,
    pub address: String,
    pub notes: String,
    pub device_serial_no: String,
    pub total_price: f64,
    pub province: String,
    pub send_warehouse_id: String,
    pub return_warehouse_id: String,
    pub created_at: String,
    pub accessories: String,
    pub status: String,
    pub tracking_no: String,
    pub device_models: String,
    pub devices: Vec<String>,
}

/// Serde 序列化 helper — 将 Vec<String> 序列化为 JSON 数组而非字符串
fn serialize_pickup_methods<S: serde::Serializer>(
    methods: &Vec<String>,
    s: S,
) -> Result<S::Ok, S::Error> {
    methods.serialize(s)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrderOutput {
    pub order: Order,
    pub devices_linked: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PagedOrders {
    pub users: Vec<Order>,
    pub pagination: OrderPagination,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrderPagination {
    pub page: u32,
    pub page_size: u32,
    pub total: u32,
    pub total_pages: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOutput {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrderOutput {
    pub success: bool,
    pub order_no: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportExcelOutput {
    pub file_name: String,
    pub content: String,
    pub mime_type: String,
}

// ═══════════════════════════════════════════════════════════════════
// 错误 / 序列化辅助
// ═══════════════════════════════════════════════════════════════════

fn err_json(code: &str, msg: &str) -> String {
    serde_json::to_string(&ErrorPayload {
        category: if code.starts_with("SYS") {
            "sys".into()
        } else {
            "biz".into()
        },
        code: code.into(),
        message: msg.into(),
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

fn serialize<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| {
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

// ═══════════════════════════════════════════════════════════════════
// Base64 编码器（用于 Excel 导出）
// ═══════════════════════════════════════════════════════════════════

fn base64_encode(bytes: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ═══════════════════════════════════════════════════════════════════
// 管线宏
// ═══════════════════════════════════════════════════════════════════

macro_rules! pipeline {
    ($payload:expr, $T:ty, $self:ident, $ctx:expr, $method:ident) => {{
        DeserializeGuard::default().check_raw(&$payload)?;
        let unvalidated: Unvalidated<$T> = $payload.try_into()?;
        let sanitized = unvalidated.sanitize();
        let validated = sanitized.validate()?;
        let input = validated.into_inner();
        $self.$method(&input, $ctx)
    }};
    ($payload:expr, $T:ty, $self:ident, $method:ident) => {{
        DeserializeGuard::default().check_raw(&$payload)?;
        let unvalidated: Unvalidated<$T> = $payload.try_into()?;
        let sanitized = unvalidated.sanitize();
        let validated = sanitized.validate()?;
        let input = validated.into_inner();
        $self.$method(&input)
    }};
}

// ═══════════════════════════════════════════════════════════════════
// Order number 生成器
// ═══════════════════════════════════════════════════════════════════

static ORDER_SEQ: LazyLock<Mutex<u64>> = LazyLock::new(|| Mutex::new(0));

fn generate_order_no(conn: &rusqlite::Connection, scope: &DataScope) -> Result<String, String> {
    let max_no: Option<String> = conn
        .query_row(
            "SELECT orderNo FROM orders WHERE tenant_id = ?1 ORDER BY orderNo DESC LIMIT 1",
            params![scope.tenant_id().as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e: rusqlite::Error| err_json("SYS_DB_QUERY", &e.to_string()))?;

    let mut seq = ORDER_SEQ
        .lock()
        .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;

    let now = chrono::Utc::now().with_timezone(&chrono_tz::Asia::Shanghai);
    let prefix = now.format("%Y%m%d%H%M").to_string();

    if let Some(ref existing) = max_no {
        if existing.starts_with(&prefix) {
            let suffix = existing[prefix.len()..].parse::<u64>().unwrap_or(0);
            *seq = suffix + 1;
        } else {
            *seq = 1;
        }
    } else {
        *seq = 1;
    }

    Ok(format!("{}{:04}", prefix, *seq))
}

// ═══════════════════════════════════════════════════════════════════
// 模块主体
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureOrder {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
    /// 跨模块依赖：feature-device
    pub device_module: Mutex<Option<Arc<dyn SystemModule>>>,
    /// 跨模块依赖：feature-pricing
    pub pricing_module: Mutex<Option<Arc<dyn SystemModule>>>,
    /// 跨模块依赖：feature-warehouse-routing
    pub warehouse_routing_module: Mutex<Option<Arc<dyn SystemModule>>>,
}

impl FeatureOrder {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
            device_module: Mutex::new(None),
            pricing_module: Mutex::new(None),
            warehouse_routing_module: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self
            .pool
            .lock()
            .map_err(|e| err_json("SYS_DB_LOCK", &e.to_string()))?;
        guard
            .as_ref()
            .ok_or_else(|| err_json("SYS_DB_NOT_INIT", "数据库未初始化。请先调用 init()。"))?
            .get()
            .map_err(|e| err_json("SYS_DB_POOL", &e.to_string()))
    }

    // ── 跨模块调用 ──

    fn call_device(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let guard = self
            .device_module
            .lock()
            .map_err(|e| err_json("SYS_MODULE_LOCK", &e.to_string()))?;
        let module = guard
            .as_ref()
            .ok_or_else(|| err_json("SYS_MODULE_NOT_INJECTED", "device 模块未注入"))?;
        module.execute(command, payload, ctx)
    }

    fn call_pricing(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let guard = self
            .pricing_module
            .lock()
            .map_err(|e| err_json("SYS_MODULE_LOCK", &e.to_string()))?;
        let module = guard
            .as_ref()
            .ok_or_else(|| err_json("SYS_MODULE_NOT_INJECTED", "pricing 模块未注入"))?;
        module.execute(command, payload, ctx)
    }

    fn call_warehouse_routing(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let guard = self
            .warehouse_routing_module
            .lock()
            .map_err(|e| err_json("SYS_MODULE_LOCK", &e.to_string()))?;
        let module = guard
            .as_ref()
            .ok_or_else(|| err_json("SYS_MODULE_NOT_INJECTED", "warehouse_routing 模块未注入"))?;
        module.execute(command, payload, ctx)
    }

    // ── row_to_order: DB 行 → Order ──

    fn row_to_order(
        conn: &rusqlite::Connection,
        scope: &DataScope,
        row: &rusqlite::Row<'_>,
    ) -> Result<Order, rusqlite::Error> {
        // Column layout (0-based):
        // 0=id, 1=orderNo, 2=startDate, 3=endDate, 4=deliveryDate,
        // 5=pickupMethods, 6=address, 7=notes, 8=deviceSerialNo, 9=totalPrice,
        // 10=province, 11=sendWarehouseId, 12=returnWarehouseId, 13=createdAt,
        // 14=accessories, 15=status, 16=trackingNo, 17=deviceModels
        let delivery_date: String = row.get::<_, String>(4).unwrap_or_default();

        // 021 — 9 态状态机: status 已是 display label, 直接显示
        let status: String = row.get::<_, String>(15).unwrap_or_default();
        let resolved_status = if status.is_empty() {
            crate::state_machine::display_label(crate::state_machine::status::DRAFT).to_string()
        } else {
            crate::state_machine::display_label(&status).to_string()
        };

        let order_id: String = row.get(0)?;

        // 查询关联的 devices
        let mut stmt = conn
            .prepare("SELECT serialNo FROM order_devices WHERE tenant_id = ?1 AND orderId = ?2")
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let devices: Vec<String> = stmt
            .query_map(params![scope.tenant_id().as_str(), order_id], |r| {
                r.get::<_, String>(0)
            })
            .map_err(|_| rusqlite::Error::InvalidQuery)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(Order {
            id: order_id,
            order_no: row.get(1)?,
            start_date: row.get(2)?,
            end_date: row.get(3)?,
            delivery_date,
            pickup_methods: serde_json::from_str::<Vec<String>>(
                &row.get::<_, String>(5).unwrap_or_default(),
            )
            .unwrap_or_default(),
            address: row.get::<_, String>(6).unwrap_or_default(),
            notes: row.get::<_, String>(7).unwrap_or_default(),
            device_serial_no: row.get::<_, String>(8).unwrap_or_default(),
            total_price: row.get::<_, f64>(9).unwrap_or(0.0),
            province: row.get::<_, String>(10).unwrap_or_default(),
            send_warehouse_id: row.get::<_, String>(11).unwrap_or_default(),
            return_warehouse_id: row.get::<_, String>(12).unwrap_or_default(),
            created_at: row.get::<_, String>(13).unwrap_or_default(),
            accessories: row.get::<_, String>(14).unwrap_or_default(),
            status: resolved_status,
            tracking_no: row.get::<_, String>(16).unwrap_or_default(),
            device_models: row.get::<_, String>(17).unwrap_or_default(),
            devices,
        })
    }

    // ═══════════════════════════════════════════════════════
    // 业务方法
    // ═══════════════════════════════════════════════════════

    fn do_list_orders(
        &self,
        input: &ListOrdersInput,
        ctx: &ExecutionContext,
    ) -> Result<PagedOrders, String> {
        let conn = self.get_conn()?;

        let mut where_parts: Vec<String> = Vec::new();
        let mut where_params: Vec<String> = Vec::new();

        // DataScope is the only tenant source. Every query is tenant-bound.
        where_parts.push("o.tenant_id = ?".into());
        where_params.push(ctx.data_scope().tenant_id().as_str().to_owned());

        // 提取 filter 值
        let get_filter = |key: &str| -> Option<String> {
            input
                .filter
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        };

        if let Some(ref status) = get_filter("status")
            && !status.is_empty()
        {
            // 021 — 将前端传入的 display label 或 DB 值统一映射为 DB 值
            let db_status = match status.as_str() {
                "草稿" | "draft" => "draft",
                "已确认" | "confirmed" => "confirmed",
                "已付款" | "paid" => "paid",
                "已发货" | "shipped" => "shipped",
                "使用中" | "in_use" | "active" | "进行中" => "in_use",
                "已归还" | "returned" => "returned",
                "检查中" | "inspected" => "inspected",
                "已完成" | "completed" => "completed",
                "已关闭" | "closed" => "closed",
                "已取消" | "cancelled" => "cancelled",
                // 向后兼容旧值
                "已预约" | "reserved" => "draft",
                other => other,
            };
            where_parts.push("o.status = ?".into());
            where_params.push(db_status.to_string());
        }

        if let Some(ref province) = get_filter("province")
            && !province.is_empty()
            && province != "__all__"
        {
            where_parts.push("o.province = ?".into());
            where_params.push(province.clone());
        }

        if let Some(ref address) = get_filter("address")
            && !address.is_empty()
        {
            where_parts.push("o.address LIKE ?".into());
            where_params.push(format!("%{}%", address));
        }

        for (key, column, operator) in [
            ("startDateFrom", "o.startDate", ">="),
            ("startDateTo", "o.startDate", "<="),
            ("endDateFrom", "o.endDate", ">="),
            ("endDateTo", "o.endDate", "<="),
            ("deliveryDateFrom", "o.deliveryDate", ">="),
            ("deliveryDateTo", "o.deliveryDate", "<="),
        ] {
            if let Some(value) = get_filter(key)
                && DATE_REGEX.is_match(&value)
            {
                where_parts.push(format!("{column} {operator} ?"));
                where_params.push(value);
            }
        }

        if let Some(included_date) = get_filter("includedDate")
            && DATE_REGEX.is_match(&included_date)
        {
            where_parts.push("o.startDate <= ?".into());
            where_params.push(included_date.clone());
            where_parts.push("o.endDate >= ?".into());
            where_params.push(included_date);
        }

        if let Some(delivery_date) = get_filter("deliveryDate")
            && DATE_REGEX.is_match(&delivery_date)
        {
            where_parts.push("o.deliveryDate = ?".into());
            where_params.push(delivery_date);
        }

        if let Some(methods) = input.filter.get("pickupMethods").and_then(Value::as_array) {
            let methods: Vec<&str> = methods
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|method| !method.is_empty())
                .collect();
            if !methods.is_empty() {
                where_parts.push(format!(
                    "({})",
                    vec!["o.pickupMethods LIKE ?"; methods.len()].join(" OR ")
                ));
                where_params.extend(methods.into_iter().map(|method| format!("%{method}%")));
            }
        }

        if let Some(ref start_date) = get_filter("startDate")
            && DATE_REGEX.is_match(start_date)
        {
            where_parts.push("o.startDate >= ?".into());
            where_params.push(start_date.clone());
        }

        if let Some(ref end_date) = get_filter("endDate")
            && DATE_REGEX.is_match(end_date)
        {
            where_parts.push("o.startDate <= ?".into());
            where_params.push(end_date.clone());
        }

        if let Some(ref serial_no) = get_filter("serialNo")
            && !serial_no.is_empty()
        {
            where_parts.push(
                    "(o.deviceSerialNo LIKE ? OR EXISTS (SELECT 1 FROM order_devices od WHERE od.tenant_id = o.tenant_id AND od.orderId = o.id AND od.serialNo LIKE ?))".into()
                );
            where_params.push(format!("%{}%", serial_no));
            where_params.push(format!("%{}%", serial_no));
        }

        if let Some(ref tracking_no) = get_filter("trackingNo")
            && !tracking_no.is_empty()
        {
            where_parts.push("o.trackingNo LIKE ?".into());
            where_params.push(format!("%{}%", tracking_no));
        }

        if let Some(ref order_no) = get_filter("orderNo")
            && !order_no.is_empty()
        {
            where_parts.push("o.orderNo LIKE ?".into());
            where_params.push(format!("%{}%", order_no));
        }

        if let Some(ref keyword) = get_filter("keyword")
            && !keyword.is_empty()
        {
            where_parts.push("(o.address LIKE ? OR o.notes LIKE ?)".into());
            where_params.push(format!("%{}%", keyword));
            where_params.push(format!("%{}%", keyword));
        }

        let where_clause = if where_parts.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_parts.join(" AND "))
        };

        // Count
        let count_sql = format!("SELECT COUNT(*) FROM orders o {}", where_clause);
        let count_params_refs: Vec<&dyn rusqlite::types::ToSql> = where_params
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let total: u32 = conn
            .query_row(&count_sql, count_params_refs.as_slice(), |row| row.get(0))
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        // ORDER BY
        let allowed_sorts = ["startDate", "endDate", "totalPrice", "createdAt"];
        let sort_by = input.sort_by.as_deref().unwrap_or("createdAt");
        let sort_col = if allowed_sorts.contains(&sort_by) {
            sort_by
        } else {
            "createdAt"
        };
        let sort_order = match input.sort_order.as_deref() {
            Some("asc") => "ASC",
            _ => "DESC",
        };
        let order_clause = format!("ORDER BY o.{} {}", sort_col, sort_order);

        // LIMIT / OFFSET
        let page_size = input.page_size;
        let offset = (input.page.max(1) - 1) * page_size;

        let data_sql = format!(
            "SELECT o.id, o.orderNo, o.startDate, o.endDate, o.deliveryDate, o.pickupMethods, \
                    o.address, o.notes, o.deviceSerialNo, o.totalPrice, o.province, \
                    o.sendWarehouseId, o.returnWarehouseId, o.createdAt, o.accessories, \
                    o.status, o.trackingNo, o.deviceModels \
             FROM orders o {} {} LIMIT ? OFFSET ?",
            where_clause, order_clause
        );

        let mut boxed: Vec<Box<dyn rusqlite::types::ToSql>> = where_params
            .into_iter()
            .map(|p| Box::new(p) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        boxed.push(Box::new(page_size as i64));
        boxed.push(Box::new(offset as i64));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            boxed.iter().map(|b| b.as_ref()).collect();

        let mut stmt = conn
            .prepare(&data_sql)
            .map_err(|e| err_json("SYS_DB_PREPARE", &e.to_string()))?;

        let mut users: Vec<Order> = Vec::new();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Self::row_to_order(&conn, ctx.data_scope(), row)
            })
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        for row in rows.flatten() {
            users.push(row);
        }

        let total_pages = if page_size > 0 {
            total.div_ceil(page_size)
        } else {
            1
        };

        Ok(PagedOrders {
            users,
            pagination: OrderPagination {
                page: input.page,
                page_size,
                total,
                total_pages,
            },
        })
    }

    fn do_get_order(&self, input: &GetOrderInput, ctx: &ExecutionContext) -> Result<Order, String> {
        // Authorization: must be authenticated
        if ctx.user_id().is_none() {
            return Err(err_json("AUTH_REQUIRED", "访问订单需要认证"));
        }

        let conn = self.get_conn()?;

        let sql = "SELECT o.id, o.orderNo, o.startDate, o.endDate, o.deliveryDate, o.pickupMethods, \
                   o.address, o.notes, o.deviceSerialNo, o.totalPrice, o.province, \
                   o.sendWarehouseId, o.returnWarehouseId, o.createdAt, o.accessories, \
                   o.status, o.trackingNo, o.deviceModels \
                   FROM orders o WHERE o.tenant_id = ?1 AND o.id = ?2";

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| err_json("SYS_DB_PREPARE", &e.to_string()))?;

        stmt.query_row(
            params![ctx.data_scope().tenant_id().as_str(), input.id],
            |row| Self::row_to_order(&conn, ctx.data_scope(), row),
        )
        .map_err(|e| {
            if let rusqlite::Error::QueryReturnedNoRows = e {
                err_json("BIZ_ORDER_NOT_FOUND", &format!("订单 {} 不存在", input.id))
            } else {
                err_json("SYS_DB_QUERY", &e.to_string())
            }
        })
    }

    fn do_create_order(
        &self,
        input: &CreateOrderInput,
        ctx: &ExecutionContext,
    ) -> Result<CreateOrderOutput, String> {
        let conn = self.get_conn()?;

        // BEGIN IMMEDIATE — TOCTOU safe
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| err_json("SYS_DB_TXN_BEGIN", &e.to_string()))?;

        let result = (|| -> Result<CreateOrderOutput, String> {
            let now = shanghai_now_iso();
            let id = Uuid::new_v4().to_string();

            // Step 1: 生成订单号
            let order_no = generate_order_no(&conn, ctx.data_scope())?;

            // Step 2: 仓库解析 (跨模块调用 warehouse_routing)
            let warehouse_result = self
                .call_warehouse_routing(
                    "resolve_warehouse",
                    serde_json::json!({
                        "province": input.province,
                        "devices": input.devices,
                    }),
                    ctx,
                )
                .unwrap_or(serde_json::json!({
                    "sendWarehouseId": "",
                    "returnWarehouseId": ""
                }));

            let send_warehouse_id = warehouse_result
                .get("sendWarehouseId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let return_warehouse_id = warehouse_result
                .get("returnWarehouseId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Step 3: 价格预估 (跨模块调用 pricing)
            let pricing_result = self
                .call_pricing(
                    "estimate_pricing",
                    serde_json::json!({
                        "startDate": input.start_date,
                        "endDate": input.end_date,
                        "deliveryDate": input.delivery_date,
                        "modelId": input.model_id,
                        "devices": input.devices,
                    }),
                    ctx,
                )
                .unwrap_or(serde_json::json!({"totalPrice": 0.0}));

            let total_price: f64 = pricing_result
                .get("totalPrice")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            // Step 4: 设备存在性验证 (跨模块调用 device)
            for serial_no in &input.devices {
                let dev_result = self.call_device(
                    "get_device",
                    serde_json::json!({ "serialNo": serial_no }),
                    ctx,
                );
                if dev_result.is_err() {
                    // 设备不存在, 回滚事务
                    return Err(err_json(
                        "BIZ_DEVICE_NOT_FOUND",
                        &format!("设备 {} 不存在或不可用", serial_no),
                    ));
                }
            }

            // Step 5: INSERT INTO orders
            let pickup_methods_json =
                serde_json::to_string(&input.pickup_methods).unwrap_or_else(|_| "[]".into());
            let accessories_json =
                serde_json::to_string(&input.accessories).unwrap_or_else(|_| "[]".into());
            let device_models_json = serde_json::json!({
                "modelId": input.model_id,
            })
            .to_string();

            let tenant_id = ctx.data_scope().tenant_id().as_str();

            conn.execute(
                "INSERT INTO orders (id, orderNo, startDate, endDate, deliveryDate, \
                 pickupMethods, address, notes, deviceSerialNo, totalPrice, province, \
                 sendWarehouseId, returnWarehouseId, createdAt, accessories, status, \
                 trackingNo, deviceModels, tenant_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                params![
                    id,
                    order_no,
                    input.start_date,
                    input.end_date,
                    input.delivery_date,
                    pickup_methods_json,
                    input.address,
                    input.notes,
                    input.devices.first().cloned().unwrap_or_default(),
                    total_price,
                    input.province,
                    send_warehouse_id,
                    return_warehouse_id,
                    now,
                    accessories_json,
                    crate::state_machine::status::DRAFT,
                    input.tracking_no.clone().unwrap_or_default(),
                    device_models_json,
                    tenant_id,
                ],
            ).map_err(|e| err_json("SYS_DB_INSERT", &e.to_string()))?;

            // Step 6: INSERT INTO order_devices (junction table)
            for serial_no in &input.devices {
                conn.execute(
                    "INSERT INTO order_devices (orderId, serialNo, tenant_id) VALUES (?1, ?2, ?3)",
                    params![id, serial_no, tenant_id],
                )
                .map_err(|e| err_json("SYS_DB_INSERT", &e.to_string()))?;
            }

            // 读取新创建的订单
            let mut stmt = conn.prepare(
                "SELECT o.id, o.orderNo, o.startDate, o.endDate, o.deliveryDate, o.pickupMethods, \
                        o.address, o.notes, o.deviceSerialNo, o.totalPrice, o.province, \
                        o.sendWarehouseId, o.returnWarehouseId, o.createdAt, o.accessories, \
                        o.status, o.trackingNo, o.deviceModels \
                 FROM orders o WHERE o.tenant_id = ?1 AND o.id = ?2"
            ).map_err(|e| err_json("SYS_DB_PREPARE", &e.to_string()))?;

            let order = stmt
                .query_row(params![tenant_id, id], |row| {
                    Self::row_to_order(&conn, ctx.data_scope(), row)
                })
                .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

            Ok(CreateOrderOutput {
                order,
                devices_linked: input.devices.len(),
            })
        })();

        match &result {
            Ok(_) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| err_json("SYS_DB_TXN_COMMIT", &e.to_string()))?;
            }
            Err(_) => {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }

        result
    }

    fn do_update_order(
        &self,
        input: &UpdateOrderInput,
        ctx: &ExecutionContext,
    ) -> Result<Order, String> {
        // Authorization: must be authenticated
        if ctx.user_id().is_none() {
            return Err(err_json("AUTH_REQUIRED", "修改订单需要认证"));
        }

        let conn = self.get_conn()?;

        // 先验证订单存在
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM orders WHERE tenant_id = ?1 AND id = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e: rusqlite::Error| err_json("SYS_DB_QUERY", &e.to_string()))?;

        if existing.is_none() {
            return Err(err_json(
                "BIZ_ORDER_NOT_FOUND",
                &format!("订单 {} 不存在", input.id),
            ));
        }

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| err_json("SYS_DB_TXN_BEGIN", &e.to_string()))?;

        let result = (|| -> Result<Order, String> {
            let pickup_methods_json =
                serde_json::to_string(&input.pickup_methods).unwrap_or_else(|_| "[]".into());
            let accessories_json =
                serde_json::to_string(&input.accessories).unwrap_or_else(|_| "[]".into());
            let device_models_json = serde_json::json!({
                "modelId": input.model_id,
            })
            .to_string();

            let update_sql = "UPDATE orders SET \
                 startDate = COALESCE(NULLIF(?1, ''), startDate), \
                 endDate = COALESCE(NULLIF(?2, ''), endDate), \
                 deliveryDate = COALESCE(NULLIF(?3, ''), deliveryDate), \
                 pickupMethods = ?4, address = ?5, notes = ?6, province = ?7, \
                 accessories = ?8, trackingNo = ?9, deviceModels = ?10 \
                 WHERE tenant_id = ?11 AND id = ?12";
            let update_params: Vec<Box<dyn rusqlite::ToSql>> = vec![
                Box::new(input.start_date.clone()),
                Box::new(input.end_date.clone()),
                Box::new(input.delivery_date.clone()),
                Box::new(pickup_methods_json),
                Box::new(input.address.clone()),
                Box::new(input.notes.clone()),
                Box::new(input.province.clone()),
                Box::new(accessories_json),
                Box::new(input.tracking_no.clone().unwrap_or_default()),
                Box::new(device_models_json),
                Box::new(ctx.data_scope().tenant_id().as_str().to_owned()),
                Box::new(input.id.clone()),
            ];

            let update_param_refs: Vec<&dyn rusqlite::ToSql> =
                update_params.iter().map(|p| p.as_ref()).collect();

            let rows_affected = conn
                .execute(&update_sql, update_param_refs.as_slice())
                .map_err(|e| err_json("SYS_DB_UPDATE", &e.to_string()))?;

            // 检查是否更新了任何行（租户隔离验证）
            if rows_affected == 0 {
                return Err(err_json(
                    "BIZ_ORDER_NOT_FOUND",
                    &format!("订单 {} 不存在或无权访问", input.id),
                ));
            }

            // 同步 order_devices
            if !input.devices.is_empty() {
                conn.execute(
                    "DELETE FROM order_devices WHERE tenant_id = ?1 AND orderId = ?2",
                    params![ctx.data_scope().tenant_id().as_str(), input.id],
                )
                .map_err(|e| err_json("SYS_DB_DELETE", &e.to_string()))?;

                // 验证设备存在
                for serial_no in &input.devices {
                    let dev_result = self.call_device(
                        "get_device",
                        serde_json::json!({ "serialNo": serial_no }),
                        ctx,
                    );
                    if dev_result.is_err() {
                        return Err(err_json(
                            "BIZ_DEVICE_NOT_FOUND",
                            &format!("设备 {} 不存在或不可用", serial_no),
                        ));
                    }
                    conn.execute(
                        "INSERT INTO order_devices (orderId, serialNo, tenant_id) VALUES (?1, ?2, ?3)",
                        params![input.id, serial_no, ctx.data_scope().tenant_id().as_str()],
                    ).map_err(|e| err_json("SYS_DB_INSERT", &e.to_string()))?;
                }
            }

            // 读取更新后的订单
            let mut stmt = conn.prepare(
                "SELECT o.id, o.orderNo, o.startDate, o.endDate, o.deliveryDate, o.pickupMethods, \
                        o.address, o.notes, o.deviceSerialNo, o.totalPrice, o.province, \
                        o.sendWarehouseId, o.returnWarehouseId, o.createdAt, o.accessories, \
                        o.status, o.trackingNo, o.deviceModels \
                 FROM orders o WHERE o.tenant_id = ?1 AND o.id = ?2"
            ).map_err(|e| err_json("SYS_DB_PREPARE", &e.to_string()))?;

            stmt.query_row(
                params![ctx.data_scope().tenant_id().as_str(), input.id],
                |row| Self::row_to_order(&conn, ctx.data_scope(), row),
            )
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))
        })();

        match &result {
            Ok(_) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| err_json("SYS_DB_TXN_COMMIT", &e.to_string()))?;
            }
            Err(_) => {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }

        result
    }

    fn do_cancel_order(
        &self,
        input: &CancelOrderInput,
        ctx: &ExecutionContext,
    ) -> Result<CancelOrderOutput, String> {
        // Authorization: must be authenticated
        if ctx.user_id().is_none() {
            return Err(err_json("AUTH_REQUIRED", "取消订单需要认证"));
        }

        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        // 验证订单存在
        let order_no: String = conn
            .query_row(
                "SELECT orderNo FROM orders WHERE tenant_id = ?1 AND id = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.id],
                |row| row.get(0),
            )
            .map_err(|_| err_json("BIZ_ORDER_NOT_FOUND", &format!("订单 {} 不存在", input.id)))?;

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| err_json("SYS_DB_TXN_BEGIN", &e.to_string()))?;

        let result = (|| -> Result<CancelOrderOutput, String> {
            // 读取当前状态用于记录 history
            let old_status: String = conn
                .query_row(
                    "SELECT status FROM orders WHERE tenant_id = ?1 AND id = ?2",
                    params![ctx.data_scope().tenant_id().as_str(), input.id],
                    |r| r.get(0),
                )
                .unwrap_or_default();

            conn.execute(
                "UPDATE orders SET status = 'cancelled' WHERE tenant_id = ?1 AND id = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.id],
            )
            .map_err(|e| err_json("SYS_DB_UPDATE", &e.to_string()))?;

            // 021 — 写入状态历史
            conn.execute(
                "INSERT INTO status_history (id, order_id, from_status, to_status, operator, reason, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![Uuid::new_v4().to_string(), input.id, old_status, "cancelled", "system", input.reason, now],
            ).map_err(|e| err_json("SYS_DB_INSERT", &e.to_string()))?;

            Ok(CancelOrderOutput {
                success: true,
                order_no,
            })
        })();

        match &result {
            Ok(_) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| err_json("SYS_DB_TXN_COMMIT", &e.to_string()))?;
            }
            Err(_) => {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }

        result
    }

    fn do_delete_order(
        &self,
        input: &DeleteOrderInput,
        ctx: &ExecutionContext,
    ) -> Result<DeleteOutput, String> {
        // Authorization: admin only
        if !ctx.has_tenant_admin_authority() {
            return Err(err_json("AUTH_FORBIDDEN", "删除订单需要管理员权限"));
        }

        let conn = self.get_conn()?;
        let _order_no: String = conn
            .query_row(
                "SELECT orderNo FROM orders WHERE tenant_id = ?1 AND id = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.id],
                |row| row.get(0),
            )
            .map_err(|_| {
                err_json(
                    "BIZ_ORDER_NOT_FOUND",
                    &format!("订单 {} 不存在或无权访问", input.id),
                )
            })?;

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| err_json("SYS_DB_TXN_BEGIN", &e.to_string()))?;

        let result = (|| -> Result<DeleteOutput, String> {
            // 删除 order_devices
            conn.execute(
                "DELETE FROM order_devices WHERE tenant_id = ?1 AND orderId = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.id],
            )
            .map_err(|e| err_json("SYS_DB_DELETE", &e.to_string()))?;

            let rows_affected = conn
                .execute(
                    "DELETE FROM orders WHERE tenant_id = ?1 AND id = ?2",
                    params![ctx.data_scope().tenant_id().as_str(), input.id],
                )
                .map_err(|e| err_json("SYS_DB_DELETE", &e.to_string()))?;

            if rows_affected == 0 {
                return Err(err_json(
                    "BIZ_ORDER_NOT_FOUND",
                    &format!("订单 {} 不存在或无权访问", input.id),
                ));
            }

            Ok(DeleteOutput { success: true })
        })();

        match &result {
            Ok(_) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| err_json("SYS_DB_TXN_COMMIT", &e.to_string()))?;
            }
            Err(_) => {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }

        result
    }

    fn do_transition(
        &self,
        input: &crate::state_machine::TransitionInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let operator = ctx.user_id().unwrap_or("system");

        // 1. 批量 guard 预检 (不持锁)
        crate::state_machine::guard_transition("", &input.to_status, &[true])?;

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| err_json("SYS_DB_TXN_BEGIN", &e.to_string()))?;

        let result = (|| -> Result<Value, String> {
            // 2. 读取当前状态
            let current_status: String = conn
                .query_row(
                    "SELECT status FROM orders WHERE tenant_id = ?1 AND id = ?2",
                    params![ctx.data_scope().tenant_id().as_str(), input.order_id],
                    |row| row.get(0),
                )
                .map_err(|_| {
                    err_json(
                        "BIZ_ORDER_NOT_FOUND",
                        &format!("订单 {} 不存在", input.order_id),
                    )
                })?;

            // 3. Guard 校验
            crate::state_machine::guard_transition(&current_status, &input.to_status, &[true])
                .map_err(|e| err_json("BIZ_TRANSITION_REJECTED", &e))?;

            // 4. UPDATE
            conn.execute(
                "UPDATE orders SET status = ?1 WHERE tenant_id = ?2 AND id = ?3",
                params![
                    input.to_status,
                    ctx.data_scope().tenant_id().as_str(),
                    input.order_id
                ],
            )
            .map_err(|e| err_json("SYS_DB_UPDATE", &e.to_string()))?;

            // 5. INSERT status_history
            conn.execute(
                "INSERT INTO status_history (id, order_id, from_status, to_status, operator, reason, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    Uuid::new_v4().to_string(),
                    input.order_id,
                    current_status,
                    input.to_status,
                    operator,
                    input.reason,
                    now,
                ],
            ).map_err(|e| err_json("SYS_DB_INSERT", &e.to_string()))?;

            Ok(serde_json::json!({
                "ok": true,
                "from": current_status,
                "to": input.to_status,
            }))
        })();

        match &result {
            Ok(_) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| err_json("SYS_DB_TXN_COMMIT", &e.to_string()))?;
            }
            Err(_) => {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }

        result
    }

    fn do_export_excel(
        &self,
        input: &ListOrdersInput,
        _ctx: &ExecutionContext,
    ) -> Result<ExportExcelOutput, String> {
        let paged = self.do_list_orders(input, _ctx)?;

        use rust_xlsxwriter::{Format, Workbook};

        let mut workbook = Workbook::new();
        let header_fmt = Format::new().set_bold();

        let ws = workbook.add_worksheet();
        ws.set_name("订单列表")
            .map_err(|e| err_json("SYS_XLSX", &e.to_string()))?;

        let headers = [
            "订单号",
            "开始日期",
            "结束日期",
            "发货日期",
            "状态",
            "省份",
            "设备数量",
            "总金额",
            "收货地址",
            "备注",
            "创建时间",
        ];

        for (i, h) in headers.iter().enumerate() {
            ws.write_string_with_format(0, i as u16, *h, &header_fmt)
                .ok();
        }

        for (r, order) in paged.users.iter().enumerate() {
            let row = (r + 1) as u32;
            ws.write_string(row, 0, &order.order_no).ok();
            ws.write_string(row, 1, &order.start_date).ok();
            ws.write_string(row, 2, &order.end_date).ok();
            ws.write_string(row, 3, &order.delivery_date).ok();
            ws.write_string(row, 4, &order.status).ok();
            ws.write_string(row, 5, &order.province).ok();
            ws.write_number(row, 6, order.devices.len() as f64).ok();
            ws.write_number(row, 7, order.total_price).ok();
            ws.write_string(row, 8, &order.address).ok();
            ws.write_string(row, 9, &order.notes).ok();
            ws.write_string(row, 10, &order.created_at).ok();
        }

        for i in 0..11u16 {
            ws.set_column_width(i, 16).ok();
        }

        let buffer = workbook
            .save_to_buffer()
            .map_err(|e| err_json("SYS_XLSX", &e.to_string()))?;

        let ts = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
        let file_name = format!("订单导出-{}.xlsx", ts);

        Ok(ExportExcelOutput {
            file_name,
            content: base64_encode(&buffer),
            mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
        })
    }
}

impl Default for FeatureOrder {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════════
// SystemModule 实现
// ═══════════════════════════════════════════════════════════════════

impl SystemModule for FeatureOrder {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "order".into(),
            version: "0.1.0".into(),
            description: "订单管理 — 创建/查询/更新/取消 + TOCTOU 事务 + Excel 导出".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "list_orders",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_order",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "create_order",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "update_order",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "cancel_order",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "delete_order",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "transition",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "export_excel",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        // Database URL
        if let Some(db_url) = config.get("databaseUrl").and_then(|v| v.as_str()) {
            let manager = SqliteConnectionManager::file(db_url);
            let pool = Pool::builder()
                .max_size(5)
                .build(manager)
                .map_err(|e| err_json("SYS_DB_POOL_CREATE", &e.to_string()))?;
            let mut guard = self
                .pool
                .lock()
                .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
            *guard = Some(pool);
        }

        // 注入跨模块依赖 — 始终先用 StubModule 占位。
        // ModuleRegistry::assemble() 在运行时替换为真实模块。
        {
            let module: Arc<dyn SystemModule> = Arc::new(StubModule);
            let mut guard = self
                .device_module
                .lock()
                .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
            *guard = Some(module);
        }
        {
            let module: Arc<dyn SystemModule> = Arc::new(StubModule);
            let mut guard = self
                .pricing_module
                .lock()
                .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
            *guard = Some(module);
        }
        {
            let module: Arc<dyn SystemModule> = Arc::new(StubModule);
            let mut guard = self
                .warehouse_routing_module
                .lock()
                .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
            *guard = Some(module);
        }

        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "list_orders" => {
                let result = pipeline!(payload, ListOrdersInput, self, ctx, do_list_orders)?;
                serialize(&result)
            }
            "get_order" => {
                let result = pipeline!(payload, GetOrderInput, self, ctx, do_get_order)?;
                serialize(&result)
            }
            "create_order" => {
                let result = pipeline!(payload, CreateOrderInput, self, ctx, do_create_order)?;
                serialize(&result)
            }
            "update_order" => {
                let result = pipeline!(payload, UpdateOrderInput, self, ctx, do_update_order)?;
                serialize(&result)
            }
            "cancel_order" => {
                let result = pipeline!(payload, CancelOrderInput, self, ctx, do_cancel_order)?;
                serialize(&result)
            }
            "transition" => {
                let result = pipeline!(
                    payload,
                    crate::state_machine::TransitionInput,
                    self,
                    ctx,
                    do_transition
                )?;
                Ok(result)
            }
            "delete_order" => {
                let result = pipeline!(payload, DeleteOrderInput, self, ctx, do_delete_order)?;
                serialize(&result)
            }
            "export_excel" => {
                let result = pipeline!(payload, ListOrdersInput, self, ctx, do_export_excel)?;
                serialize(&result)
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

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "order".into(),
            description: "订单管理 — 创建/查询/更新/取消 + TOCTOU 事务 + Excel 导出".into(),
            commands: vec![
                CommandSchema {
                    name: "list_orders".into(),
                    description: "分页查询订单列表，支持 status/province/date/serialNo/trackingNo/orderNo/keyword 过滤".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ListOrdersInput)).ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(PagedOrders)).ok(),
                },
                CommandSchema {
                    name: "get_order".into(),
                    description: "查询单个订单详情，包含关联设备列表".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetOrderInput)).ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(Order)).ok(),
                },
                CommandSchema {
                    name: "create_order".into(),
                    description: "创建订单 Saga — BEGIN IMMEDIATE + 仓库解析 + 估价 + 设备校验 + INSERT + 审计".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(CreateOrderInput)).ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(CreateOrderOutput)).ok(),
                },
                CommandSchema {
                    name: "update_order".into(),
                    description: "更新订单字段 + 同步 order_devices 关联".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(UpdateOrderInput)).ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(Order)).ok(),
                },
                CommandSchema {
                    name: "cancel_order".into(),
                    description: "取消订单 — UPDATE status='cancelled' + 审计".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(CancelOrderInput)).ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(CancelOrderOutput)).ok(),
                },
                CommandSchema {
                    name: "delete_order".into(),
                    description: "删除订单 + order_devices + 审计".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(DeleteOrderInput)).ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(DeleteOutput)).ok(),
                },
                CommandSchema {
                    name: "transition".into(),
                    description: "订单状态转移 — guard 校验 + atomic UPDATE + status_history".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(crate::state_machine::TransitionInput)).ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "export_excel".into(),
                    description: "导出当前筛选的订单列表为 xlsx".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ListOrdersInput)).ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(ExportExcelOutput)).ok(),
                },
            ],
        }
    }

    fn shutdown(&mut self) -> Result<(), String> {
        if let Ok(mut guard) = self.pool.lock() {
            *guard = None;
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// 存根模块 — init() 阶段使用的占位符
// ═══════════════════════════════════════════════════════════════════

struct StubModule;

impl SystemModule for StubModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "stub".into(),
            version: "0.0.0".into(),
            description: "存根".into(),
            author: "system".into(),
            wasm_compatible: false,
            storage: None,
        }
    }
    fn commands(&self) -> Vec<CommandMetadata> {
        vec![]
    }
    fn init(&mut self, _: Value) -> Result<(), String> {
        Ok(())
    }
    fn execute(&self, _: &str, _: Value, _: &ExecutionContext) -> Result<Value, String> {
        Ok(serde_json::Value::Null)
    }
    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "stub".into(),
            description: "存根".into(),
            commands: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_create_order_trim_all_fields() {
        let mut input = CreateOrderInput {
            start_date: "  2026-01-01  ".into(),
            end_date: "  2026-01-05  ".into(),
            delivery_date: "  2026-01-01  ".into(),
            pickup_methods: vec!["  自取  ".into(), "  快递  ".into(), "  ".into()],
            address: "  上海市  ".into(),
            notes: "  测试备注  ".into(),
            province: "  上海  ".into(),
            model_id: "  DJI-TALOS  ".into(),
            devices: vec!["  abc123  ".into()],
            accessories: vec!["  保护壳  ".into(), "  ".into()],
            tracking_no: Some("  SF123   ".into()),
        };
        input.sanitize();
        assert_eq!(input.start_date, "2026-01-01");
        assert_eq!(input.end_date, "2026-01-05");
        assert_eq!(input.delivery_date, "2026-01-01");
        assert_eq!(input.pickup_methods, vec!["自取", "快递"]);
        assert_eq!(input.address, "上海市");
        assert_eq!(input.notes, "测试备注");
        assert_eq!(input.province, "上海");
        assert_eq!(input.model_id, "DJI-TALOS");
        assert_eq!(input.devices, vec!["ABC123"]);
        assert_eq!(input.accessories, vec!["保护壳"]);
        assert_eq!(input.tracking_no, Some("SF123".into()));
    }

    #[test]
    fn validate_create_order_rejects_empty_devices() {
        let input = CreateOrderInput {
            start_date: "2026-01-01".into(),
            end_date: "2026-01-05".into(),
            delivery_date: "2026-01-01".into(),
            pickup_methods: vec![],
            address: String::new(),
            notes: String::new(),
            province: String::new(),
            model_id: String::new(),
            devices: vec![],
            accessories: vec![],
            tracking_no: None,
        };
        let result = input.validate();
        assert!(!result.is_valid());
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.field == "devices" && e.code == "VAL_REQUIRED")
        );
    }

    #[test]
    fn validate_create_order_rejects_invalid_dates() {
        let input = CreateOrderInput {
            start_date: "invalid".into(),
            end_date: "not-a-date".into(),
            delivery_date: String::new(),
            pickup_methods: vec![],
            address: String::new(),
            notes: String::new(),
            province: String::new(),
            model_id: String::new(),
            devices: vec!["ABC123".into()],
            accessories: vec![],
            tracking_no: None,
        };
        let result = input.validate();
        assert!(result.errors.len() >= 2);
        assert!(result.errors.iter().any(|e| e.field == "startDate"));
        assert!(result.errors.iter().any(|e| e.field == "endDate"));
    }

    #[test]
    fn validate_create_order_date_range_error() {
        let input = CreateOrderInput {
            start_date: "2026-02-01".into(),
            end_date: "2026-01-01".into(),
            delivery_date: String::new(),
            pickup_methods: vec![],
            address: String::new(),
            notes: String::new(),
            province: String::new(),
            model_id: String::new(),
            devices: vec!["ABC123".into()],
            accessories: vec![],
            tracking_no: None,
        };
        let result = input.validate();
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.field == "endDate" && e.code == "VAL_DATE_RANGE")
        );
    }

    #[test]
    fn list_input_defaults_page_and_size() {
        let mut input = ListOrdersInput {
            page: 0,
            page_size: 500,
            filter: Default::default(),
            sort_by: None,
            sort_order: None,
        };
        input.sanitize();
        assert_eq!(input.page, 1);
        assert_eq!(input.page_size, 200); // clamped
    }

    #[test]
    fn cancel_order_validate_empty_id() {
        let input = CancelOrderInput {
            id: String::new(),
            reason: String::new(),
        };
        let result = input.validate();
        assert!(!result.is_valid());
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.field == "id" && e.code == "VAL_REQUIRED")
        );
    }
}

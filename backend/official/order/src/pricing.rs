//! 定价引擎 (Maxwell 原生模块)
//!
//! 优先级链：动态日价 → 假日价格(+includePreviousDay) → 型号基础价 → 全局基础价
//! 占用系数：shipping 0.2 / normal 1.0 / return 0.2
//! 15 天滚动窗口限制动态调价。
//!
//! 注入依赖：
//! - logistics_module: estimate_shipping / estimate_return
//! - warehouse_routing_module: resolve_warehouse / get_occupancy_coefficients
//!
//! 命令：
//! - get_pricing_config, update_pricing_config, save_pricing_config
//! - upsert_dynamic_price, delete_dynamic_price, get_dynamic_prices
//! - estimate_pricing, get_model_pricing_config

use chrono::{Datelike, NaiveDate, TimeZone};
use chrono_tz::Asia::Shanghai;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use system_core::*;
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════
// 常量
// ═══════════════════════════════════════════════════════════════════

pub const DYNAMIC_ROLLING_DAYS: i64 = 15;

const DEFAULT_BASE_WEEKDAY_PRICE: f64 = 8.5;
const DEFAULT_BASE_WEEKEND_PRICE: f64 = 14.0;

const DEFAULT_RECEIVE_SHIPPING_FEES_JSON: &str = r#"{"area1":7,"area4":18,"area2":7,"area3":7}"#;

const DEFAULT_REC_AREA_KEYS: &[&str] = &["area1", "area2", "area3", "area4"];

const OCCUPANCY_COEFF_SHIPPING: f64 = 0.2;
const OCCUPANCY_COEFF_NORMAL: f64 = 1.0;
const OCCUPANCY_COEFF_RETURN: f64 = 0.2;

// ═══════════════════════════════════════════════════════════════════
// 数值与日期辅助
// ═══════════════════════════════════════════════════════════════════

fn is_positive_number(v: f64) -> bool {
    v.is_finite() && v > 0.0
}

fn is_non_negative_number(v: f64) -> bool {
    v.is_finite() && v >= 0.0
}

fn normalize_price(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn normalize_date_str(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() != 10 || &trimmed[4..5] != "-" || &trimmed[7..8] != "-" {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let d: u32 = parts[2].parse().ok()?;
    let _date = NaiveDate::from_ymd_opt(y, m, d)?;
    let _dt = Shanghai.with_ymd_and_hms(y, m, d, 0, 0, 0).single()?;
    Some(format!("{:04}-{:02}-{:02}", y, m, d))
}

fn shanghai_now_iso() -> String {
    let now = Shanghai.from_utc_datetime(&chrono::Utc::now().naive_utc());
    now.format("%Y-%m-%dT%H:%M:%S%.3f+08:00").to_string()
}

fn shanghai_now_date_key() -> String {
    let now = Shanghai.from_utc_datetime(&chrono::Utc::now().naive_utc());
    now.format("%Y-%m-%d").to_string()
}

fn add_days_to_date_key(date_key: &str, days: i64) -> Option<String> {
    let normalized = normalize_date_str(date_key)?;
    let parts: Vec<&str> = normalized.split('-').collect();
    let y: i32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let d: u32 = parts[2].parse().ok()?;
    let base = NaiveDate::from_ymd_opt(y, m, d)?;
    let shifted = if days >= 0 {
        base + chrono::Duration::days(days)
    } else {
        base - chrono::Duration::days(-days)
    };
    Some(shifted.format("%Y-%m-%d").to_string())
}

fn get_date_diff_in_days(start_key: &str, end_key: &str) -> i64 {
    let sp: Vec<&str> = start_key.split('-').collect();
    let ep: Vec<&str> = end_key.split('-').collect();
    if sp.len() != 3 || ep.len() != 3 {
        return 0;
    }
    let sy: i32 = sp[0].parse().unwrap_or(0);
    let sm: u32 = sp[1].parse().unwrap_or(0);
    let sd: u32 = sp[2].parse().unwrap_or(0);
    let ey: i32 = ep[0].parse().unwrap_or(0);
    let em: u32 = ep[1].parse().unwrap_or(0);
    let ed: u32 = ep[2].parse().unwrap_or(0);
    let start = NaiveDate::from_ymd_opt(sy, sm, sd);
    let end = NaiveDate::from_ymd_opt(ey, em, ed);
    match (start, end) {
        (Some(s), Some(e)) => e.signed_duration_since(s).num_days(),
        _ => 0,
    }
}

fn is_weekend(date_key: &str) -> bool {
    let parts: Vec<&str> = date_key.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    let y: i32 = parts[0].parse().unwrap_or(0);
    let m: u32 = parts[1].parse().unwrap_or(0);
    let d: u32 = parts[2].parse().unwrap_or(0);
    if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
        let dow = date.weekday().num_days_from_monday();
        // Monday=0 ... Sunday=6.  Weekend = Fri(4), Sat(5), Sun(6)
        matches!(dow, 4..=6)
    } else {
        false
    }
}

fn get_rolling_date_keys(days: i64, from: &str) -> Vec<String> {
    let size = days.max(1);
    let mut keys = Vec::with_capacity(size as usize);
    for i in 0..size {
        if let Some(key) = add_days_to_date_key(from, i) {
            keys.push(key);
        }
    }
    keys
}

// ═══════════════════════════════════════════════════════════════════
// 输入类型
// ═══════════════════════════════════════════════════════════════════

// ── get_pricing_config ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetPricingConfigInput {}

impl Validate for GetPricingConfigInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}
impl Sanitize for GetPricingConfigInput {
    fn sanitize(&mut self) {}
}

// ── HolidayRuleInput ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HolidayRuleInput {
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    pub price: f64,
    #[serde(default = "default_include_previous_day")]
    pub include_previous_day: bool,
}

fn default_include_previous_day() -> bool {
    true
}

// ── update_pricing_config ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePricingConfigInput {
    pub base_weekday_price: f64,
    pub base_weekend_price: f64,
    #[serde(default)]
    pub holiday_rules: Vec<HolidayRuleInput>,
    #[serde(default)]
    pub updated_by: String,
}

impl Validate for UpdatePricingConfigInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if !is_positive_number(self.base_weekday_price) {
            errors.push(FieldError {
                field: "baseWeekdayPrice".into(),
                message: "baseWeekdayPrice 必须大于 0".into(),
                code: "PRICING_INVALID_BASE_WEEKDAY_PRICE".into(),
            });
        }
        if !is_positive_number(self.base_weekend_price) {
            errors.push(FieldError {
                field: "baseWeekendPrice".into(),
                message: "baseWeekendPrice 必须大于 0".into(),
                code: "PRICING_INVALID_BASE_WEEKEND_PRICE".into(),
            });
        }
        for (i, rule) in self.holiday_rules.iter().enumerate() {
            if rule.name.trim().is_empty() {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].name", i),
                    message: "假日名称不能为空".into(),
                    code: "VAL_REQUIRED".into(),
                });
            }
            if normalize_date_str(&rule.start_date).is_none() {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].startDate", i),
                    message: "startDate 格式无效".into(),
                    code: "VAL_INVALID_DATE".into(),
                });
            }
            if normalize_date_str(&rule.end_date).is_none() {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].endDate", i),
                    message: "endDate 格式无效".into(),
                    code: "VAL_INVALID_DATE".into(),
                });
            }
            if !is_positive_number(rule.price) {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].price", i),
                    message: "price 必须大于 0".into(),
                    code: "PRICING_INVALID_HOLIDAY_PRICE".into(),
                });
            }
            let sd = normalize_date_str(&rule.start_date);
            let ed = normalize_date_str(&rule.end_date);
            if let (Some(s), Some(e)) = (sd, ed)
                && s > e
            {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].endDate", i),
                    message: "startDate 不能晚于 endDate".into(),
                    code: "VAL_DATE_ORDER".into(),
                });
            }
        }
        ValidationResult { errors }
    }
}
impl Sanitize for UpdatePricingConfigInput {
    fn sanitize(&mut self) {
        self.updated_by = self.updated_by.trim().to_string();
        for rule in &mut self.holiday_rules {
            rule.name = rule.name.trim().to_string();
            rule.start_date = rule.start_date.trim().to_string();
            rule.end_date = rule.end_date.trim().to_string();
        }
    }
}

// ── save_pricing_config ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SavePricingConfigInput {
    pub base_weekday_price: f64,
    pub base_weekend_price: f64,
    #[serde(default)]
    pub holiday_rules: Vec<HolidayRuleInput>,
    #[serde(default)]
    pub dynamic_price_map: HashMap<String, f64>,
    #[serde(default)]
    pub updated_by: String,
    pub receive_shipping_fees: Option<HashMap<String, f64>>,
}

impl Validate for SavePricingConfigInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if !is_positive_number(self.base_weekday_price) {
            errors.push(FieldError {
                field: "baseWeekdayPrice".into(),
                message: "baseWeekdayPrice 必须大于 0".into(),
                code: "PRICING_INVALID_BASE_WEEKDAY_PRICE".into(),
            });
        }
        if !is_positive_number(self.base_weekend_price) {
            errors.push(FieldError {
                field: "baseWeekendPrice".into(),
                message: "baseWeekendPrice 必须大于 0".into(),
                code: "PRICING_INVALID_BASE_WEEKEND_PRICE".into(),
            });
        }

        for (i, rule) in self.holiday_rules.iter().enumerate() {
            if rule.name.trim().is_empty() {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].name", i),
                    message: "假日名称不能为空".into(),
                    code: "VAL_REQUIRED".into(),
                });
            }
            if normalize_date_str(&rule.start_date).is_none() {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].startDate", i),
                    message: "startDate 格式无效".into(),
                    code: "VAL_INVALID_DATE".into(),
                });
            }
            if normalize_date_str(&rule.end_date).is_none() {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].endDate", i),
                    message: "endDate 格式无效".into(),
                    code: "VAL_INVALID_DATE".into(),
                });
            }
            if !is_positive_number(rule.price) {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].price", i),
                    message: "price 必须大于 0".into(),
                    code: "PRICING_INVALID_HOLIDAY_PRICE".into(),
                });
            }
            let sd = normalize_date_str(&rule.start_date);
            let ed = normalize_date_str(&rule.end_date);
            if let (Some(s), Some(e)) = (sd, ed)
                && s > e
            {
                errors.push(FieldError {
                    field: format!("holidayRules[{}].endDate", i),
                    message: "startDate 不能晚于 endDate".into(),
                    code: "VAL_DATE_ORDER".into(),
                });
            }
        }

        let rolling_set: std::collections::HashSet<String> =
            get_rolling_date_keys(DYNAMIC_ROLLING_DAYS, &shanghai_now_date_key())
                .into_iter()
                .collect();

        for (raw_date_key, raw_price) in &self.dynamic_price_map {
            let normalized = normalize_date_str(raw_date_key);
            if normalized.is_none() {
                errors.push(FieldError {
                    field: format!("dynamicPriceMap[{}]", raw_date_key),
                    message: format!("动态调价日期格式无效：{}", raw_date_key),
                    code: "PRICING_INVALID_DYNAMIC_DATE".into(),
                });
            } else if let Some(n) = &normalized
                && !rolling_set.contains(n)
            {
                errors.push(FieldError {
                    field: format!("dynamicPriceMap[{}]", raw_date_key),
                    message: format!("动态调价仅支持未来 {} 天：{}", DYNAMIC_ROLLING_DAYS, n),
                    code: "PRICING_DYNAMIC_DATE_OUT_OF_RANGE".into(),
                });
            }
            if !is_positive_number(*raw_price) {
                errors.push(FieldError {
                    field: format!("dynamicPriceMap[{}]", raw_date_key),
                    message: format!("动态调价必须大于 0：{}", raw_date_key),
                    code: "PRICING_INVALID_DYNAMIC_PRICE".into(),
                });
            }
        }

        if let Some(ref fees) = self.receive_shipping_fees {
            for area_key in DEFAULT_REC_AREA_KEYS {
                if let Some(fee) = fees.get(*area_key)
                    && !is_non_negative_number(*fee)
                {
                    errors.push(FieldError {
                        field: format!("receiveShippingFees.{}", area_key),
                        message: format!("收到区域邮费必须 >= 0：{}", area_key),
                        code: "PRICING_INVALID_RECEIVE_SHIPPING_FEE".into(),
                    });
                }
            }
        }

        ValidationResult { errors }
    }
}
impl Sanitize for SavePricingConfigInput {
    fn sanitize(&mut self) {
        self.updated_by = self.updated_by.trim().to_string();
        for rule in &mut self.holiday_rules {
            rule.name = rule.name.trim().to_string();
            rule.start_date = rule.start_date.trim().to_string();
            rule.end_date = rule.end_date.trim().to_string();
        }
        let mut cleaned: HashMap<String, f64> = HashMap::new();
        for (k, v) in self.dynamic_price_map.drain() {
            if let Some(nk) = normalize_date_str(&k) {
                cleaned.insert(nk, normalize_price(v));
            }
        }
        self.dynamic_price_map = cleaned;
    }
}

// ── upsert_dynamic_price ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertDynamicPriceInput {
    pub date_key: String,
    pub price: f64,
    #[serde(default)]
    pub updated_by: String,
}

impl Validate for UpsertDynamicPriceInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        let normalized = normalize_date_str(&self.date_key);
        if let Some(ref n) = normalized {
            let rolling_set: std::collections::HashSet<String> =
                get_rolling_date_keys(DYNAMIC_ROLLING_DAYS, &shanghai_now_date_key())
                    .into_iter()
                    .collect();
            if !rolling_set.contains(n.as_str()) {
                errors.push(FieldError {
                    field: "dateKey".into(),
                    message: format!("动态调价仅支持未来 {} 天：{}", DYNAMIC_ROLLING_DAYS, n),
                    code: "PRICING_DYNAMIC_DATE_OUT_OF_RANGE".into(),
                });
            }
        } else {
            errors.push(FieldError {
                field: "dateKey".into(),
                message: "dateKey 格式无效".into(),
                code: "VAL_INVALID_DATE".into(),
            });
        }
        if !is_positive_number(self.price) {
            errors.push(FieldError {
                field: "price".into(),
                message: "price 必须大于 0".into(),
                code: "PRICING_INVALID_DYNAMIC_PRICE".into(),
            });
        }
        ValidationResult { errors }
    }
}
impl Sanitize for UpsertDynamicPriceInput {
    fn sanitize(&mut self) {
        self.date_key = self.date_key.trim().to_string();
        self.updated_by = self.updated_by.trim().to_string();
        if let Some(n) = normalize_date_str(&self.date_key) {
            self.date_key = n;
        }
        self.price = normalize_price(self.price);
    }
}

// ── delete_dynamic_price ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDynamicPriceInput {
    pub date_key: String,
}

impl Validate for DeleteDynamicPriceInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if normalize_date_str(&self.date_key).is_none() {
            errors.push(FieldError {
                field: "dateKey".into(),
                message: "dateKey 格式无效".into(),
                code: "VAL_INVALID_DATE".into(),
            });
        }
        ValidationResult { errors }
    }
}
impl Sanitize for DeleteDynamicPriceInput {
    fn sanitize(&mut self) {
        self.date_key = self.date_key.trim().to_string();
        if let Some(n) = normalize_date_str(&self.date_key) {
            self.date_key = n;
        }
    }
}

// ── estimate_pricing ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EstimatePricingInput {
    pub start_date: String,
    pub end_date: String,
    #[serde(default)]
    pub province: String,
    #[serde(default)]
    pub model_id: String,
}

impl Validate for EstimatePricingInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if normalize_date_str(&self.start_date).is_none() {
            errors.push(FieldError {
                field: "startDate".into(),
                message: "startDate 格式无效".into(),
                code: "VAL_INVALID_DATE".into(),
            });
        }
        if normalize_date_str(&self.end_date).is_none() {
            errors.push(FieldError {
                field: "endDate".into(),
                message: "endDate 格式无效".into(),
                code: "VAL_INVALID_DATE".into(),
            });
        }
        if let (Some(s), Some(e)) = (
            normalize_date_str(&self.start_date),
            normalize_date_str(&self.end_date),
        ) && s > e
        {
            errors.push(FieldError {
                field: "endDate".into(),
                message: "startDate 不能晚于 endDate".into(),
                code: "VAL_DATE_ORDER".into(),
            });
        }
        ValidationResult { errors }
    }
}
impl Sanitize for EstimatePricingInput {
    fn sanitize(&mut self) {
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
        self.province = self.province.trim().to_string();
        self.model_id = self.model_id.trim().to_string();
        if let Some(n) = normalize_date_str(&self.start_date) {
            self.start_date = n;
        }
        if let Some(n) = normalize_date_str(&self.end_date) {
            self.end_date = n;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// 输出类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HolidayRule {
    pub name: String,
    pub start_date: String,
    pub end_date: String,
    pub price: f64,
    #[serde(default = "default_holiday_include_previous_day")]
    pub include_previous_day: bool,
}

fn default_holiday_include_previous_day() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DynamicPriceEntry {
    pub date_key: String,
    pub price: f64,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DailyPriceEntry {
    pub date_key: String,
    pub final_daily_price: f64,
    pub price_source: String,
    pub occupy_type: String,
    pub occupy_factor: f64,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelBasePrice {
    pub weekday_price: f64,
    pub weekend_price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PricingConfig {
    pub base_weekday_price: f64,
    pub base_weekend_price: f64,
    pub holiday_rules: Vec<HolidayRule>,
    pub receive_shipping_fees: HashMap<String, f64>,
    pub dynamic_price_map: HashMap<String, f64>,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseRoute {
    pub send_warehouse_id: String,
    pub send_warehouse_name: String,
    pub return_warehouse_id: String,
    pub return_warehouse_name: String,
    pub shipping_days: i64,
    pub return_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PricingEstimate {
    pub breakdown: Vec<DailyPriceEntry>,
    pub total_price: f64,
    pub route: Option<WarehouseRoute>,
    pub shipping_estimate: Option<serde_json::Value>,
    pub return_estimate: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DynamicPriceUpsertResult {
    pub success: bool,
    pub date_key: String,
    pub price: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DynamicPriceDeleteResult {
    pub success: bool,
    pub date_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SavePricingConfigResult {
    pub success: bool,
    pub config: PricingConfig,
}

// ═══════════════════════════════════════════════════════════════════
// 模块主体
// ═══════════════════════════════════════════════════════════════════

pub struct FeaturePricing {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
    pub logistics_module: Mutex<Option<Arc<dyn SystemModule>>>,
    pub warehouse_routing_module: Mutex<Option<Arc<dyn SystemModule>>>,
}

impl FeaturePricing {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
            logistics_module: Mutex::new(None),
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
            .ok_or_else(|| err_json("SYS_DB_NOT_INIT", "数据库未初始化"))?
            .get()
            .map_err(|e| err_json("SYS_DB_POOL", &e.to_string()))
    }

    // ── 跨模块调用 ──

    fn call_logistics(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let guard = self
            .logistics_module
            .lock()
            .map_err(|e| err_json("SYS_MODULE_LOCK", &e.to_string()))?;
        match guard.as_ref() {
            Some(m) => m.execute(command, payload, ctx),
            None => Ok(Value::Null),
        }
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
        match guard.as_ref() {
            Some(m) => m.execute(command, payload, ctx),
            None => Ok(Value::Null),
        }
    }

    // ── 数据库业务逻辑 ──

    fn ensure_pricing_config_row(
        &self,
        scope: &DataScope,
        conn: &rusqlite::Connection,
    ) -> Result<PricingConfig, String> {
        let existing: Option<PricingConfig> = conn
            .query_row(
                "SELECT baseWeekdayPrice, baseWeekendPrice, holidayRulesJson, receiveShippingFeesJson, updatedBy, createdAt, updatedAt FROM pricing_configs WHERE tenant_id = ?1 LIMIT 1",
                params![scope.tenant_id().as_str()],
                |row| {
                    let hrj: String = row.get(2)?;
                    let sfj: String = row.get(3)?;
                    Ok(PricingConfig {
                        base_weekday_price: row.get(0)?,
                        base_weekend_price: row.get(1)?,
                        holiday_rules: parse_holiday_rules_json(&hrj),
                        receive_shipping_fees: parse_receive_shipping_fees_json(&sfj),
                        dynamic_price_map: HashMap::new(),
                        updated_by: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                },
            )
            .ok();

        if let Some(config) = existing {
            return Ok(config);
        }

        let now = shanghai_now_iso();
        let default_fees: HashMap<String, f64> =
            serde_json::from_str(DEFAULT_RECEIVE_SHIPPING_FEES_JSON).unwrap_or_default();

        conn.execute(
            "INSERT INTO pricing_configs (baseWeekdayPrice, baseWeekendPrice, holidayRulesJson, receiveShippingFeesJson, updatedBy, createdAt, updatedAt, tenant_id)
             VALUES (?1, ?2, '[]', ?3, '', ?4, ?5, ?6)",
            params![
                DEFAULT_BASE_WEEKDAY_PRICE,
                DEFAULT_BASE_WEEKEND_PRICE,
                DEFAULT_RECEIVE_SHIPPING_FEES_JSON,
                now,
                now,
                scope.tenant_id().as_str()
            ],
        )
        .map_err(|e| err_json("SYS_DB_INSERT", &e.to_string()))?;

        Ok(PricingConfig {
            base_weekday_price: DEFAULT_BASE_WEEKDAY_PRICE,
            base_weekend_price: DEFAULT_BASE_WEEKEND_PRICE,
            holiday_rules: vec![],
            receive_shipping_fees: default_fees,
            dynamic_price_map: HashMap::new(),
            updated_by: String::new(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    fn get_dynamic_price_map_db(
        &self,
        scope: &DataScope,
        conn: &rusqlite::Connection,
    ) -> Result<HashMap<String, f64>, String> {
        let rolling_keys: std::collections::HashSet<String> =
            get_rolling_date_keys(DYNAMIC_ROLLING_DAYS, &shanghai_now_date_key())
                .into_iter()
                .collect();

        let mut stmt = conn
            .prepare("SELECT dateKey, price FROM dynamic_daily_prices WHERE tenant_id = ?1 ORDER BY dateKey ASC")
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        let rows: Vec<(String, f64)> = stmt
            .query_map(params![scope.tenant_id().as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let mut map = HashMap::new();
        for (date_key, price) in rows {
            if rolling_keys.contains(&date_key) && is_positive_number(price) {
                map.insert(date_key, normalize_price(price));
            }
        }
        Ok(map)
    }

    fn get_model_base_price(
        &self,
        scope: &DataScope,
        model_id: &str,
        conn: &rusqlite::Connection,
    ) -> Result<ModelBasePrice, String> {
        if model_id.is_empty() {
            let config = self.ensure_pricing_config_row(scope, conn)?;
            return Ok(ModelBasePrice {
                weekday_price: config.base_weekday_price,
                weekend_price: config.base_weekend_price,
            });
        }

        let row = conn
            .query_row(
                "SELECT weekdayPrice, weekendPrice FROM model_base_prices WHERE tenant_id = ?1 AND modelId = ?2",
                params![scope.tenant_id().as_str(), model_id],
                |row| {
                    Ok(ModelBasePrice {
                        weekday_price: row.get(0)?,
                        weekend_price: row.get(1)?,
                    })
                },
            )
            .ok();

        match row {
            Some(r) => Ok(r),
            None => {
                let config = self.ensure_pricing_config_row(scope, conn)?;
                Ok(ModelBasePrice {
                    weekday_price: config.base_weekday_price,
                    weekend_price: config.base_weekend_price,
                })
            }
        }
    }

    fn resolve_daily_price(date_key: &str, config: &PricingConfig) -> (f64, String) {
        // Priority 1: dynamic price
        if let Some(&price) = config.dynamic_price_map.get(date_key) {
            return (price, "dynamic".to_string());
        }

        // Priority 2: holiday (with includePreviousDay)
        for rule in &config.holiday_rules {
            if rule.include_previous_day
                && let Some(prev) = add_days_to_date_key(&rule.start_date, -1)
                && date_key == prev
            {
                return (rule.price, "holiday".to_string());
            }
            if rule.start_date.as_str() <= date_key && date_key <= rule.end_date.as_str() {
                return (rule.price, "holiday".to_string());
            }
        }

        // Priority 3: base (weekend vs weekday)
        if is_weekend(date_key) {
            (config.base_weekend_price, "base_weekend".to_string())
        } else {
            (config.base_weekday_price, "base_weekday".to_string())
        }
    }

    // ── do_* command handlers ──

    fn do_get_pricing_config(&self, scope: &DataScope) -> Result<PricingConfig, String> {
        let conn = self.get_conn()?;
        let mut config = self.ensure_pricing_config_row(scope, &conn)?;
        config.dynamic_price_map = self.get_dynamic_price_map_db(scope, &conn)?;
        Ok(config)
    }

    fn do_update_pricing_config(
        &self,
        input: &UpdatePricingConfigInput,
        scope: &DataScope,
    ) -> Result<PricingConfig, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let normalized_rules: Vec<HolidayRule> = input
            .holiday_rules
            .iter()
            .map(|r| HolidayRule {
                name: r.name.clone(),
                start_date: normalize_date_str(&r.start_date).unwrap_or_default(),
                end_date: normalize_date_str(&r.end_date).unwrap_or_default(),
                price: normalize_price(r.price),
                include_previous_day: r.include_previous_day,
            })
            .collect();

        let rules_json = serde_json::to_string(&normalized_rules)
            .map_err(|e| err_json("SYS_SERIALIZE", &e.to_string()))?;

        let _ = self.ensure_pricing_config_row(scope, &conn)?;

        conn.execute(
            "UPDATE pricing_configs SET baseWeekdayPrice = ?1, baseWeekendPrice = ?2, holidayRulesJson = ?3, updatedBy = ?4, updatedAt = ?5 WHERE tenant_id = ?6",
            params![
                normalize_price(input.base_weekday_price),
                normalize_price(input.base_weekend_price),
                rules_json,
                input.updated_by.clone(),
                now,
                scope.tenant_id().as_str(),
            ],
        )
        .map_err(|e| err_json("SYS_DB_UPDATE", &e.to_string()))?;

        let mut config = self.ensure_pricing_config_row(scope, &conn)?;
        config.dynamic_price_map = self.get_dynamic_price_map_db(scope, &conn)?;
        Ok(config)
    }

    fn do_save_pricing_config(
        &self,
        input: &SavePricingConfigInput,
        scope: &DataScope,
    ) -> Result<SavePricingConfigResult, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let normalized_rules: Vec<HolidayRule> = input
            .holiday_rules
            .iter()
            .map(|r| HolidayRule {
                name: r.name.clone(),
                start_date: normalize_date_str(&r.start_date).unwrap_or_default(),
                end_date: normalize_date_str(&r.end_date).unwrap_or_default(),
                price: normalize_price(r.price),
                include_previous_day: r.include_previous_day,
            })
            .collect();

        let rules_json = serde_json::to_string(&normalized_rules)
            .map_err(|e| err_json("SYS_SERIALIZE", &e.to_string()))?;

        let default_fees: HashMap<String, f64> =
            serde_json::from_str(DEFAULT_RECEIVE_SHIPPING_FEES_JSON).unwrap_or_default();
        let mut normalized_fees = HashMap::new();
        for area_key in DEFAULT_REC_AREA_KEYS {
            if let Some(ref input_fees) = input.receive_shipping_fees {
                let fee = input_fees
                    .get(*area_key)
                    .copied()
                    .unwrap_or(*default_fees.get(*area_key).unwrap_or(&0.0));
                normalized_fees.insert(area_key.to_string(), normalize_price(fee));
            } else {
                normalized_fees.insert(
                    area_key.to_string(),
                    *default_fees.get(*area_key).unwrap_or(&0.0),
                );
            }
        }
        let fees_json = serde_json::to_string(&normalized_fees)
            .map_err(|e| err_json("SYS_SERIALIZE", &e.to_string()))?;

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| err_json("SYS_DB_TXN_BEGIN", &e.to_string()))?;

        let result = (|| -> Result<(), String> {
            let _ = self.ensure_pricing_config_row(scope, &conn)?;

            conn.execute(
                "UPDATE pricing_configs SET baseWeekdayPrice = ?1, baseWeekendPrice = ?2, holidayRulesJson = ?3, receiveShippingFeesJson = ?4, updatedBy = ?5, updatedAt = ?6 WHERE tenant_id = ?7",
                params![
                    normalize_price(input.base_weekday_price),
                    normalize_price(input.base_weekend_price),
                    rules_json,
                    fees_json,
                    input.updated_by.clone(),
                    now,
                    scope.tenant_id().as_str(),
                ],
            )
            .map_err(|e| err_json("SYS_DB_UPDATE", &e.to_string()))?;

            conn.execute(
                "DELETE FROM dynamic_daily_prices WHERE tenant_id = ?1",
                params![scope.tenant_id().as_str()],
            )
            .map_err(|e| err_json("SYS_DB_DELETE", &e.to_string()))?;

            let mut sorted_entries: Vec<(&String, &f64)> = input.dynamic_price_map.iter().collect();
            sorted_entries.sort_by(|a, b| a.0.cmp(b.0));

            let mut insert_stmt = conn
                .prepare(
                    "INSERT INTO dynamic_daily_prices (id, dateKey, price, updatedBy, createdAt, updatedAt, tenant_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )
                .map_err(|e| err_json("SYS_DB_PREPARE", &e.to_string()))?;

            for (date_key, price) in sorted_entries {
                insert_stmt
                    .execute(params![
                        Uuid::new_v4().to_string(),
                        date_key,
                        normalize_price(*price),
                        input.updated_by.clone(),
                        now,
                        scope.tenant_id().as_str(),
                        now,
                    ])
                    .map_err(|e| err_json("SYS_DB_INSERT", &e.to_string()))?;
            }

            Ok(())
        })();

        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| err_json("SYS_DB_TXN_COMMIT", &e.to_string()))?;
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }

        let mut config = self.ensure_pricing_config_row(scope, &conn)?;
        config.dynamic_price_map = self.get_dynamic_price_map_db(scope, &conn)?;

        Ok(SavePricingConfigResult {
            success: true,
            config,
        })
    }

    fn do_upsert_dynamic_price(
        &self,
        input: &UpsertDynamicPriceInput,
        scope: &DataScope,
    ) -> Result<DynamicPriceUpsertResult, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let id = Uuid::new_v4().to_string();

        conn.execute(
            "DELETE FROM dynamic_daily_prices WHERE tenant_id = ?1 AND dateKey = ?2",
            params![scope.tenant_id().as_str(), input.date_key],
        )
        .map_err(|e| err_json("SYS_DB_DELETE", &e.to_string()))?;

        conn.execute(
            "INSERT INTO dynamic_daily_prices (id, dateKey, price, updatedBy, createdAt, updatedAt, tenant_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, input.date_key, input.price, input.updated_by, now, now, scope.tenant_id().as_str()],
        )
        .map_err(|e| err_json("SYS_DB_INSERT", &e.to_string()))?;

        Ok(DynamicPriceUpsertResult {
            success: true,
            date_key: input.date_key.clone(),
            price: input.price,
        })
    }

    fn do_delete_dynamic_price(
        &self,
        input: &DeleteDynamicPriceInput,
        scope: &DataScope,
    ) -> Result<DynamicPriceDeleteResult, String> {
        let conn = self.get_conn()?;
        conn.execute(
            "DELETE FROM dynamic_daily_prices WHERE tenant_id = ?1 AND dateKey = ?2",
            params![scope.tenant_id().as_str(), input.date_key],
        )
        .map_err(|e| err_json("SYS_DB_DELETE", &e.to_string()))?;

        Ok(DynamicPriceDeleteResult {
            success: true,
            date_key: input.date_key.clone(),
        })
    }

    fn do_get_dynamic_prices(&self, scope: &DataScope) -> Result<Vec<DynamicPriceEntry>, String> {
        let conn = self.get_conn()?;
        let rolling_keys: std::collections::HashSet<String> =
            get_rolling_date_keys(DYNAMIC_ROLLING_DAYS, &shanghai_now_date_key())
                .into_iter()
                .collect();

        let mut stmt = conn
            .prepare(
                "SELECT dateKey, price, updatedBy, createdAt, updatedAt
                 FROM dynamic_daily_prices WHERE tenant_id = ?1 ORDER BY dateKey ASC",
            )
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        let entries: Vec<DynamicPriceEntry> = stmt
            .query_map(params![scope.tenant_id().as_str()], |row| {
                Ok(DynamicPriceEntry {
                    date_key: row.get(0)?,
                    price: row.get(1)?,
                    updated_by: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?
            .filter_map(|r| r.ok())
            .filter(|e| rolling_keys.contains(&e.date_key))
            .collect();

        Ok(entries)
    }

    fn do_estimate_pricing(
        &self,
        input: &EstimatePricingInput,
        ctx: &ExecutionContext,
    ) -> Result<PricingEstimate, String> {
        let conn = self.get_conn()?;
        let scope = ctx.data_scope();
        let mut config = self.ensure_pricing_config_row(scope, &conn)?;
        config.dynamic_price_map = self.get_dynamic_price_map_db(scope, &conn)?;

        if !input.model_id.is_empty() {
            let model_base = self.get_model_base_price(scope, &input.model_id, &conn)?;
            config.base_weekday_price = model_base.weekday_price;
            config.base_weekend_price = model_base.weekend_price;
        }

        let mut route: Option<WarehouseRoute> = None;
        let mut shipping_days: i64 = 1;
        let mut return_days: i64 = 1;

        if !input.province.is_empty() {
            let route_result = self
                .call_warehouse_routing(
                    "resolve_warehouse",
                    serde_json::json!({"province": input.province}),
                    ctx,
                )
                .ok();

            if let Some(val) = route_result
                && !val.is_null()
                && let Ok(parsed) = serde_json::from_value::<WarehouseRoute>(val.clone())
            {
                shipping_days = parsed.shipping_days.max(1);
                return_days = parsed.return_days.max(1);
                route = Some(parsed);
            }

            if route.is_none() {
                let logistics_result = self
                    .call_logistics(
                        "estimate_shipping",
                        serde_json::json!({"province": input.province, "warehouseName": "上海仓"}),
                        ctx,
                    )
                    .ok();

                if let Some(val) = logistics_result
                    && !val.is_null()
                {
                    if let Some(sd) = val.get("shippingDays").and_then(|v| v.as_i64()) {
                        shipping_days = sd.max(1);
                    }
                    if let Some(rd) = val.get("returnDays").and_then(|v| v.as_i64()) {
                        return_days = rd.max(1);
                    }
                }
            }
        }

        let total_days = get_date_diff_in_days(&input.start_date, &input.end_date) + 1;
        let mut breakdown = Vec::with_capacity(total_days as usize);
        let mut current = input.start_date.clone();
        let mut day_index: i64 = 0;

        while normalize_date_str(&current).is_some() {
            if day_index >= total_days {
                break;
            }

            let occupy_type = if day_index < shipping_days {
                "shipping"
            } else if day_index >= total_days - return_days {
                "return"
            } else {
                "normal"
            };

            let factor = match occupy_type {
                "shipping" => OCCUPANCY_COEFF_SHIPPING,
                "return" => OCCUPANCY_COEFF_RETURN,
                _ => OCCUPANCY_COEFF_NORMAL,
            };

            let (price, source) = Self::resolve_daily_price(&current, &config);
            let amount = normalize_price(price * factor);

            breakdown.push(DailyPriceEntry {
                date_key: current.clone(),
                final_daily_price: price,
                price_source: source,
                occupy_type: occupy_type.to_string(),
                occupy_factor: factor,
                amount,
            });

            if let Some(next_key) = add_days_to_date_key(&current, 1) {
                current = next_key;
            } else {
                break;
            }
            day_index += 1;
        }

        let total_price = normalize_price(breakdown.iter().map(|d| d.amount).sum());

        let shipping_estimate = if !input.province.is_empty() {
            self.call_logistics(
                "estimate_shipping",
                serde_json::json!({
                    "province": input.province,
                    "warehouseName": route.as_ref().map(|r| &r.send_warehouse_name).unwrap_or(&"上海仓".to_string())
                }),
                ctx,
            )
            .ok()
        } else {
            None
        };

        let return_estimate = if !input.province.is_empty() {
            self.call_logistics(
                "estimate_return",
                serde_json::json!({
                    "province": input.province,
                    "warehouseName": route.as_ref().map(|r| &r.return_warehouse_name).unwrap_or(&"上海仓".to_string())
                }),
                ctx,
            )
            .ok()
        } else {
            None
        };

        Ok(PricingEstimate {
            breakdown,
            total_price,
            route,
            shipping_estimate,
            return_estimate,
        })
    }

    fn do_get_model_pricing_config(
        &self,
        scope: &DataScope,
        model_id: &str,
    ) -> Result<PricingConfig, String> {
        let conn = self.get_conn()?;
        let mut config = self.ensure_pricing_config_row(scope, &conn)?;
        config.dynamic_price_map = self.get_dynamic_price_map_db(scope, &conn)?;

        if !model_id.is_empty() {
            let model_base = self.get_model_base_price(scope, model_id, &conn)?;
            config.base_weekday_price = model_base.weekday_price;
            config.base_weekend_price = model_base.weekend_price;
        }

        Ok(config)
    }
}

impl Default for FeaturePricing {
    fn default() -> Self {
        Self::new()
    }
}

// ── 管线辅助宏 ──

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(
            TenantId::new(tenant).expect("tenant"),
            Revision::new("r1").expect("revision"),
        )
        .expect("production scope")
    }

    #[test]
    fn pricing_config_and_dynamic_prices_are_tenant_scoped() {
        let db_path = std::env::temp_dir().join(format!("talos-pricing-{}.db", Uuid::new_v4()));
        let manager = SqliteConnectionManager::file(&db_path);
        let pool = Pool::builder().max_size(4).build(manager).expect("pool");
        let feature = FeaturePricing {
            pool: Mutex::new(Some(pool.clone())),
            logistics_module: Mutex::new(None),
            warehouse_routing_module: Mutex::new(None),
        };

        {
            let conn = pool.get().expect("setup connection");
            conn.execute_batch(
                "CREATE TABLE pricing_configs (
                   id INTEGER PRIMARY KEY AUTOINCREMENT,
                   baseWeekdayPrice REAL NOT NULL,
                   baseWeekendPrice REAL NOT NULL,
                   holidayRulesJson TEXT NOT NULL,
                   receiveShippingFeesJson TEXT NOT NULL,
                   updatedBy TEXT NOT NULL,
                   createdAt TEXT NOT NULL,
                   updatedAt TEXT NOT NULL,
                   tenant_id TEXT NOT NULL UNIQUE
                 );
                 CREATE TABLE dynamic_daily_prices (
                   id TEXT PRIMARY KEY,
                   dateKey TEXT NOT NULL,
                   price REAL NOT NULL,
                   updatedBy TEXT NOT NULL,
                   createdAt TEXT NOT NULL,
                   updatedAt TEXT NOT NULL,
                   tenant_id TEXT NOT NULL,
                   UNIQUE(tenant_id, dateKey)
                 );
                 CREATE TABLE model_base_prices (
                   modelId TEXT PRIMARY KEY,
                   weekdayPrice REAL NOT NULL,
                   weekendPrice REAL NOT NULL,
                   tenant_id TEXT NOT NULL
                 );",
            )
            .expect("schema");
        }

        let tenant_a = scope("tenant-a");
        let tenant_b = scope("tenant-b");
        feature
            .do_update_pricing_config(
                &UpdatePricingConfigInput {
                    base_weekday_price: 10.0,
                    base_weekend_price: 20.0,
                    holiday_rules: vec![],
                    updated_by: "admin-a".into(),
                },
                &tenant_a,
            )
            .expect("tenant A config");
        feature
            .do_update_pricing_config(
                &UpdatePricingConfigInput {
                    base_weekday_price: 30.0,
                    base_weekend_price: 40.0,
                    holiday_rules: vec![],
                    updated_by: "admin-b".into(),
                },
                &tenant_b,
            )
            .expect("tenant B config");

        let date_key = shanghai_now_date_key();
        feature
            .do_upsert_dynamic_price(
                &UpsertDynamicPriceInput {
                    date_key: date_key.clone(),
                    price: 111.0,
                    updated_by: "admin-a".into(),
                },
                &tenant_a,
            )
            .expect("tenant A dynamic price");
        feature
            .do_upsert_dynamic_price(
                &UpsertDynamicPriceInput {
                    date_key: date_key.clone(),
                    price: 222.0,
                    updated_by: "admin-b".into(),
                },
                &tenant_b,
            )
            .expect("tenant B dynamic price");

        let config_a = feature.do_get_pricing_config(&tenant_a).expect("tenant A");
        let config_b = feature.do_get_pricing_config(&tenant_b).expect("tenant B");
        assert_eq!(config_a.base_weekday_price, 10.0);
        assert_eq!(config_b.base_weekday_price, 30.0);
        assert_eq!(config_a.dynamic_price_map.get(&date_key), Some(&111.0));
        assert_eq!(config_b.dynamic_price_map.get(&date_key), Some(&222.0));

        feature
            .do_delete_dynamic_price(
                &DeleteDynamicPriceInput {
                    date_key: date_key.clone(),
                },
                &tenant_a,
            )
            .expect("delete tenant A price");
        assert!(
            feature
                .do_get_dynamic_prices(&tenant_a)
                .expect("tenant A prices")
                .is_empty()
        );
        assert_eq!(
            feature
                .do_get_dynamic_prices(&tenant_b)
                .expect("tenant B prices")[0]
                .price,
            222.0
        );

        drop(feature);
        drop(pool);
        let _ = std::fs::remove_file(db_path);
    }
}

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

// ── 错误辅助 ──

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

// ── JSON 解析辅助 ──

fn parse_holiday_rules_json(text: &str) -> Vec<HolidayRule> {
    if text.is_empty() || text == "[]" {
        return vec![];
    }
    serde_json::from_str::<Vec<HolidayRule>>(text).unwrap_or_default()
}

fn parse_receive_shipping_fees_json(text: &str) -> HashMap<String, f64> {
    if text.is_empty() {
        return serde_json::from_str(DEFAULT_RECEIVE_SHIPPING_FEES_JSON).unwrap_or_default();
    }
    serde_json::from_str(text).unwrap_or_else(|_| {
        serde_json::from_str(DEFAULT_RECEIVE_SHIPPING_FEES_JSON).unwrap_or_default()
    })
}

// ── SystemModule impl ──

impl SystemModule for FeaturePricing {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "pricing".into(),
            version: "0.1.0".into(),
            description: "定价引擎 — 优先级链(动态→假日→型号→全局) + 占用系数 + 15天滚动窗口"
                .into(),
            author: "hoshi".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "get_pricing_config",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "update_pricing_config",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "save_pricing_config",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "upsert_dynamic_price",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "delete_dynamic_price",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_dynamic_prices",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "estimate_pricing",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_model_pricing_config",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        if let Some(url) = config.get("databaseUrl").and_then(|v| v.as_str()) {
            let manager = SqliteConnectionManager::file(url);
            let pool = Pool::builder()
                .max_size(4)
                .build(manager)
                .map_err(|e| err_json("SYS_DB_POOL_CREATE", &e.to_string()))?;
            let mut guard = self
                .pool
                .lock()
                .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
            *guard = Some(pool);
        }
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let scope = ctx.data_scope();
        match command {
            "get_pricing_config" => {
                let result = self.do_get_pricing_config(scope)?;
                serialize(&result)
            }
            "update_pricing_config" => {
                let result = pipeline!(
                    payload,
                    UpdatePricingConfigInput,
                    self,
                    scope,
                    do_update_pricing_config
                )?;
                serialize(&result)
            }
            "save_pricing_config" => {
                let result = pipeline!(
                    payload,
                    SavePricingConfigInput,
                    self,
                    scope,
                    do_save_pricing_config
                )?;
                serialize(&result)
            }
            "upsert_dynamic_price" => {
                let result = pipeline!(
                    payload,
                    UpsertDynamicPriceInput,
                    self,
                    scope,
                    do_upsert_dynamic_price
                )?;
                serialize(&result)
            }
            "delete_dynamic_price" => {
                let result = pipeline!(
                    payload,
                    DeleteDynamicPriceInput,
                    self,
                    scope,
                    do_delete_dynamic_price
                )?;
                serialize(&result)
            }
            "get_dynamic_prices" => {
                let result = self.do_get_dynamic_prices(scope)?;
                serialize(&result)
            }
            "estimate_pricing" => {
                let result = pipeline!(
                    payload,
                    EstimatePricingInput,
                    self,
                    ctx,
                    do_estimate_pricing
                )?;
                serialize(&result)
            }
            "get_model_pricing_config" => {
                let model_id = payload
                    .get("modelId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let result = self.do_get_model_pricing_config(scope, model_id)?;
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
            name: "pricing".into(),
            description: "定价引擎".into(),
            commands: vec![
                CommandSchema {
                    name: "get_pricing_config".into(),
                    description: "获取完整定价配置（基础价+假日规则+动态价+区域邮费）".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        GetPricingConfigInput
                    ))
                    .ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(PricingConfig)).ok(),
                },
                CommandSchema {
                    name: "update_pricing_config".into(),
                    description: "更新基础价+假日规则".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        UpdatePricingConfigInput
                    ))
                    .ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(PricingConfig)).ok(),
                },
                CommandSchema {
                    name: "save_pricing_config".into(),
                    description: "全量保存定价配置（含动态价+区域邮费）".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        SavePricingConfigInput
                    ))
                    .ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(
                        SavePricingConfigResult
                    ))
                    .ok(),
                },
                CommandSchema {
                    name: "upsert_dynamic_price".into(),
                    description: "插入或更新单日动态价格".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        UpsertDynamicPriceInput
                    ))
                    .ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(
                        DynamicPriceUpsertResult
                    ))
                    .ok(),
                },
                CommandSchema {
                    name: "delete_dynamic_price".into(),
                    description: "删除单日动态价格".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        DeleteDynamicPriceInput
                    ))
                    .ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(
                        DynamicPriceDeleteResult
                    ))
                    .ok(),
                },
                CommandSchema {
                    name: "get_dynamic_prices".into(),
                    description: "获取15天窗口内所有动态价格".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: Some(
                        serde_json::json!({"type": "array", "items": {"$ref": "#/definitions/DynamicPriceEntry"}}),
                    ),
                },
                CommandSchema {
                    name: "estimate_pricing".into(),
                    description: "根据日期范围+省份+型号ID估算订单价格".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(EstimatePricingInput))
                        .ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(PricingEstimate))
                        .ok(),
                },
                CommandSchema {
                    name: "get_model_pricing_config".into(),
                    description: "获取特定型号的定价配置（回退到全局）".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: serde_json::to_value(schemars::schema_for!(PricingConfig)).ok(),
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

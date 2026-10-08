#![allow(dead_code)]
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use system_core::DataScope;
use uuid::Uuid;

use crate::error::AppError;
use crate::services::logistics_service;
use crate::services::warehouse_routing_service;
use crate::utils::time;

pub const DYNAMIC_ROLLING_DAYS: i64 = 15;
pub const DEFAULT_BASE_WEEKDAY_PRICE: f64 = 8.5;
pub const DEFAULT_BASE_WEEKEND_PRICE: f64 = 14.0;

pub const DEFAULT_RECEIVE_SHIPPING_FEES: &str = r#"{"area1":7,"area4":18,"area2":7,"area3":7}"#;
const RECEIVE_AREA_KEYS: &[&str] = &["area1", "area2", "area3", "area4"];

const OCCUPANCY_COEFFICIENTS_SHIPPING: f64 = 0.2;
const OCCUPANCY_COEFFICIENTS_NORMAL: f64 = 1.0;
const OCCUPANCY_COEFFICIENTS_RETURN: f64 = 0.2;

// ── Helpers ──────────────────────────────────────────────────────

/// Check if a number is positive and finite.
pub fn is_positive_number(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

pub fn is_positive_number_helper(value: f64) -> bool {
    is_positive_number(value)
}

fn normalize_price(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn normalize_date_str(s: &str) -> Result<String, String> {
    let v = serde_json::Value::String(s.trim().to_string());
    time::normalize_business_date(&v)
}

fn normalize_include_previous_day(value: Option<&serde_json::Value>) -> Option<bool> {
    match value {
        None | Some(serde_json::Value::Null) => Some(true),
        Some(serde_json::Value::Bool(b)) => Some(*b),
        Some(serde_json::Value::Number(n)) => n.as_f64().map(|f| (f - 0.0).abs() > f64::EPSILON),
        Some(serde_json::Value::String(s)) => {
            let lower = s.trim().to_lowercase();
            match lower.as_str() {
                "true" | "1" | "yes" | "on" => Some(true),
                "false" | "0" | "no" | "off" => Some(false),
                _ => None,
            }
        }
        _ => None,
    }
}

// ── Holiday rule types ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HolidayRule {
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

/// Raw input from API — uses flexible types for validation.
#[derive(Debug, Clone, Deserialize)]
pub struct HolidayRuleInput {
    pub name: Option<String>,
    #[serde(alias = "startDate")]
    pub start_date: Option<String>,
    #[serde(alias = "endDate")]
    pub end_date: Option<String>,
    pub price: Option<f64>,
    #[serde(alias = "includePreviousDay")]
    pub include_previous_day: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct NormalizeHolidayRuleResult {
    pub ok: bool,
    pub error: Option<String>,
    pub value: Option<HolidayRule>,
}

pub fn normalize_holiday_rule_internal(
    rule: &HolidayRuleInput,
    index: usize,
) -> NormalizeHolidayRuleResult {
    normalize_holiday_rule(rule, index)
}

fn normalize_holiday_rule(rule: &HolidayRuleInput, index: usize) -> NormalizeHolidayRuleResult {
    let name = rule
        .name
        .as_deref()
        .map(|s| s.trim())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return NormalizeHolidayRuleResult {
            ok: false,
            error: Some(format!("holidayRules[{}] 缺少 name", index)),
            value: None,
        };
    }

    let start_date_str = rule.start_date.as_deref().map(|s| s.trim()).unwrap_or("");
    let start_date = match normalize_date_str(start_date_str) {
        Ok(d) => d,
        Err(_) => {
            return NormalizeHolidayRuleResult {
                ok: false,
                error: Some(format!("holidayRules[{}] startDate 格式无效", index)),
                value: None,
            };
        }
    };

    let end_date_str = rule.end_date.as_deref().map(|s| s.trim()).unwrap_or("");
    let end_date = match normalize_date_str(end_date_str) {
        Ok(d) => d,
        Err(_) => {
            return NormalizeHolidayRuleResult {
                ok: false,
                error: Some(format!("holidayRules[{}] endDate 格式无效", index)),
                value: None,
            };
        }
    };

    let price = match rule.price {
        Some(p) if is_positive_number(p) => p,
        _ => {
            return NormalizeHolidayRuleResult {
                ok: false,
                error: Some(format!("holidayRules[{}] price 必须大于 0", index)),
                value: None,
            };
        }
    };

    let include_previous_day_bl =
        normalize_include_previous_day(rule.include_previous_day.as_ref());
    let include_previous_day = match include_previous_day_bl {
        Some(b) => b,
        None => {
            return NormalizeHolidayRuleResult {
                ok: false,
                error: Some(format!(
                    "holidayRules[{}] includePreviousDay 必须是布尔值",
                    index
                )),
                value: None,
            };
        }
    };

    if start_date > end_date {
        return NormalizeHolidayRuleResult {
            ok: false,
            error: Some(format!(
                "holidayRules[{}] startDate 不能晚于 endDate",
                index
            )),
            value: None,
        };
    }

    NormalizeHolidayRuleResult {
        ok: true,
        error: None,
        value: Some(HolidayRule {
            name,
            start_date,
            end_date,
            price: normalize_price(price),
            include_previous_day,
        }),
    }
}

pub fn normalize_holiday_rules(rules: &[HolidayRuleInput]) -> Vec<HolidayRule> {
    let mut normalized = Vec::new();
    for (index, rule) in rules.iter().enumerate() {
        let result = normalize_holiday_rule(rule, index);
        if result.ok
            && let Some(v) = result.value
        {
            normalized.push(v);
        }
    }
    normalized
}

fn parse_holiday_rules_json(text: &str) -> Vec<HolidayRule> {
    if text.is_empty() {
        return Vec::new();
    }
    let parsed: Result<Vec<HolidayRule>, _> = serde_json::from_str(text);
    match parsed {
        Ok(rules) => rules,
        Err(_) => {
            // Try parsing as HolidayRuleInput array and normalize
            let raw: Result<Vec<HolidayRuleInput>, _> = serde_json::from_str(text);
            match raw {
                Ok(inputs) => normalize_holiday_rules(&inputs),
                Err(_) => Vec::new(),
            }
        }
    }
}

fn parse_receive_shipping_fees_json(text: &str) -> HashMap<String, f64> {
    // Default values
    let defaults: Vec<(&str, f64)> = vec![
        ("area1", 7.0),
        ("area2", 7.0),
        ("area3", 7.0),
        ("area4", 18.0),
    ];
    let mut map: HashMap<String, f64> = defaults.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    if text.is_empty() {
        return map;
    }
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text)
        && let Some(obj) = parsed.as_object()
    {
        for key in RECEIVE_AREA_KEYS {
            if let Some(val) = obj.get(*key).and_then(|v| v.as_f64())
                && val >= 0.0
            {
                map.insert(key.to_string(), normalize_price(val));
            }
        }
    }
    map
}

// ── Configuration ────────────────────────────────────────────────

fn ensure_pricing_config_row(
    conn: &rusqlite::Connection,
    scope: &DataScope,
) -> Result<PricingConfigRow, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, baseWeekdayPrice, baseWeekendPrice, holidayRulesJson, receiveShippingFeesJson, updatedBy, createdAt, updatedAt
         FROM pricing_configs WHERE tenant_id = ?1 LIMIT 1",
    )?;
    let existing = stmt.query_row(params![scope.tenant_id().as_str()], |row| {
        Ok(PricingConfigRow {
            id: row.get(0)?,
            base_weekday_price: row.get(1)?,
            base_weekend_price: row.get(2)?,
            holiday_rules_json: row.get::<_, String>(3).unwrap_or_default(),
            receive_shipping_fees_json: row.get::<_, String>(4).unwrap_or_default(),
            updated_by: row.get::<_, String>(5).unwrap_or_default(),
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    });

    if let Ok(row) = existing {
        return Ok(row);
    }

    let now = time::shanghai_now_iso();
    conn.execute(
        "INSERT INTO pricing_configs (baseWeekdayPrice, baseWeekendPrice, holidayRulesJson, receiveShippingFeesJson, updatedBy, createdAt, updatedAt, tenant_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            DEFAULT_BASE_WEEKDAY_PRICE,
            DEFAULT_BASE_WEEKEND_PRICE,
            "[]",
            DEFAULT_RECEIVE_SHIPPING_FEES,
            "",
            now,
            now,
            scope.tenant_id().as_str()
        ],
    )?;

    conn.query_row(
        "SELECT id, baseWeekdayPrice, baseWeekendPrice, holidayRulesJson, receiveShippingFeesJson, updatedBy, createdAt, updatedAt
         FROM pricing_configs WHERE tenant_id = ?1 LIMIT 1",
        params![scope.tenant_id().as_str()],
        |row| {
            Ok(PricingConfigRow {
                id: row.get(0)?,
                base_weekday_price: row.get(1)?,
                base_weekend_price: row.get(2)?,
                holiday_rules_json: row.get::<_, String>(3).unwrap_or_default(),
                receive_shipping_fees_json: row.get::<_, String>(4).unwrap_or_default(),
                updated_by: row.get::<_, String>(5).unwrap_or_default(),
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        },
    )
    .map_err(AppError::from)
}

struct PricingConfigRow {
    id: i64,
    base_weekday_price: f64,
    base_weekend_price: f64,
    holiday_rules_json: String,
    receive_shipping_fees_json: String,
    updated_by: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingConfig {
    pub base_weekday_price: f64,
    pub base_weekend_price: f64,
    pub holiday_rules: Vec<HolidayRule>,
    #[serde(rename = "dynamicPriceMap")]
    pub dynamic_price_map: HashMap<String, f64>,
    #[serde(rename = "receiveShippingFees")]
    pub receive_shipping_fees: HashMap<String, f64>,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

fn get_rolling_date_keys(days: i64, from_date_key: &str) -> Vec<String> {
    let size = std::cmp::max(1, days);
    let mut keys = Vec::new();
    for i in 0..size {
        keys.push(time::add_days_to_date_key(from_date_key, i));
    }
    keys
}

fn get_dynamic_price_map(
    conn: &rusqlite::Connection,
    scope: &DataScope,
) -> Result<HashMap<String, f64>, AppError> {
    let rolling_set: std::collections::HashSet<String> =
        get_rolling_date_keys(DYNAMIC_ROLLING_DAYS, &time::shanghai_now_date_key())
            .into_iter()
            .collect();

    let mut stmt = conn.prepare(
        "SELECT dateKey, price FROM dynamic_daily_prices WHERE tenant_id = ?1 ORDER BY dateKey ASC",
    )?;
    let rows = stmt.query_map(params![scope.tenant_id().as_str()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
    })?;

    let mut map = HashMap::new();
    for (date_key, price) in rows.into_iter().flatten() {
        let dk = date_key.trim().to_string();
        if !rolling_set.contains(&dk) {
            continue;
        }
        if !is_positive_number(price) {
            continue;
        }
        map.insert(dk, normalize_price(price));
    }
    Ok(map)
}

// ── Public API ───────────────────────────────────────────────────

/// Get or create the pricing configuration.
pub fn get_pricing_config(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
) -> Result<PricingConfig, AppError> {
    let conn = pool.get()?;
    let row = ensure_pricing_config_row(&conn, scope)?;

    let base_weekday_price = if is_positive_number(row.base_weekday_price) {
        normalize_price(row.base_weekday_price)
    } else {
        DEFAULT_BASE_WEEKDAY_PRICE
    };
    let base_weekend_price = if is_positive_number(row.base_weekend_price) {
        normalize_price(row.base_weekend_price)
    } else {
        DEFAULT_BASE_WEEKEND_PRICE
    };

    let holiday_rules = parse_holiday_rules_json(&row.holiday_rules_json);
    let dynamic_price_map = get_dynamic_price_map(&conn, scope)?;
    let receive_shipping_fees = parse_receive_shipping_fees_json(&row.receive_shipping_fees_json);

    Ok(PricingConfig {
        base_weekday_price,
        base_weekend_price,
        holiday_rules,
        dynamic_price_map,
        receive_shipping_fees,
        updated_by: row.updated_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Update the pricing configuration (base prices and holiday rules only, not dynamic prices).
pub fn update_pricing_config(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    base_weekday: f64,
    base_weekend: f64,
    holiday_rules: &[HolidayRule],
    updated_by: &str,
) -> Result<PricingConfig, AppError> {
    if !is_positive_number(base_weekday) {
        return Err(AppError::BadRequest(
            "baseWeekdayPrice 必须大于 0".to_string(),
        ));
    }
    if !is_positive_number(base_weekend) {
        return Err(AppError::BadRequest(
            "baseWeekendPrice 必须大于 0".to_string(),
        ));
    }

    let conn = pool.get()?;
    let now = time::shanghai_now_iso();
    let normalized_weekday = normalize_price(base_weekday);
    let normalized_weekend = normalize_price(base_weekend);
    let holiday_json = serde_json::to_string(holiday_rules).unwrap_or_else(|_| "[]".to_string());

    // Ensure the config row exists
    ensure_pricing_config_row(&conn, scope)?;

    conn.execute(
        "UPDATE pricing_configs
         SET baseWeekdayPrice = ?1, baseWeekendPrice = ?2, holidayRulesJson = ?3, updatedBy = ?4, updatedAt = ?5
         WHERE tenant_id = ?6",
        params![
            normalized_weekday,
            normalized_weekend,
            holiday_json,
            updated_by,
            now,
            scope.tenant_id().as_str()
        ],
    )?;

    // Return updated config
    get_pricing_config(pool, scope)
}

/// Full config save including dynamic prices (matches JS savePricingConfig).
pub fn save_pricing_config(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    base_weekday: f64,
    base_weekend: f64,
    holiday_rules: &[HolidayRule],
    dynamic_price_map: &HashMap<String, f64>,
    updated_by: &str,
    receive_shipping_fees_json: Option<&str>,
) -> Result<PricingConfig, AppError> {
    if !is_positive_number(base_weekday) {
        return Err(AppError::BadRequest(
            "baseWeekdayPrice 必须大于 0".to_string(),
        ));
    }
    if !is_positive_number(base_weekend) {
        return Err(AppError::BadRequest(
            "baseWeekendPrice 必须大于 0".to_string(),
        ));
    }

    let mut conn = pool.get()?;
    let now = time::shanghai_now_iso();
    let normalized_weekday = normalize_price(base_weekday);
    let normalized_weekend = normalize_price(base_weekend);
    let holiday_json = serde_json::to_string(holiday_rules).unwrap_or_else(|_| "[]".to_string());
    let receive_fees_json = receive_shipping_fees_json.unwrap_or(DEFAULT_RECEIVE_SHIPPING_FEES);

    // Use a transaction
    let tx = conn.transaction()?;

    ensure_pricing_config_row(&tx, scope)?;

    tx.execute(
        "UPDATE pricing_configs
         SET baseWeekdayPrice = ?1, baseWeekendPrice = ?2, holidayRulesJson = ?3, receiveShippingFeesJson = ?4, updatedBy = ?5, updatedAt = ?6
         WHERE tenant_id = ?7",
        params![
            normalized_weekday,
            normalized_weekend,
            holiday_json,
            receive_fees_json,
            updated_by,
            now,
            scope.tenant_id().as_str()
        ],
    )?;

    // Delete all existing dynamic prices and re-insert
    tx.execute(
        "DELETE FROM dynamic_daily_prices WHERE tenant_id = ?1",
        params![scope.tenant_id().as_str()],
    )?;

    // Sort date keys ascending
    let mut sorted_keys: Vec<&String> = dynamic_price_map.keys().collect();
    sorted_keys.sort();

    for date_key in sorted_keys {
        let price = dynamic_price_map[date_key];
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO dynamic_daily_prices (id, dateKey, price, updatedBy, createdAt, updatedAt, tenant_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, date_key, price, updated_by, now, now, scope.tenant_id().as_str()],
        )?;
    }

    tx.commit()?;

    get_pricing_config(pool, scope)
}

// ── Model-based pricing ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ModelBasePrice {
    pub weekday_price: f64,
    pub weekend_price: f64,
}

pub fn get_model_base_price(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    model_id: &str,
) -> Result<ModelBasePrice, AppError> {
    if model_id.is_empty() {
        let config = get_pricing_config(pool, scope)?;
        return Ok(ModelBasePrice {
            weekday_price: config.base_weekday_price,
            weekend_price: config.base_weekend_price,
        });
    }

    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT weekdayPrice, weekendPrice FROM model_base_prices WHERE tenant_id = ?1 AND modelId = ?2 LIMIT 1",
    )?;
    let result = stmt.query_row(params![scope.tenant_id().as_str(), model_id], |row| {
        Ok(ModelBasePrice {
            weekday_price: row.get(0)?,
            weekend_price: row.get(1)?,
        })
    });

    match result {
        Ok(price) => Ok(price),
        Err(_) => {
            // Fallback to global pricing
            let config = get_pricing_config(pool, scope)?;
            Ok(ModelBasePrice {
                weekday_price: config.base_weekday_price,
                weekend_price: config.base_weekend_price,
            })
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingConfig {
    pub base_weekday_price: f64,
    pub base_weekend_price: f64,
    pub holiday_rules: Vec<HolidayRule>,
    pub dynamic_price_map: HashMap<String, f64>,
    #[serde(rename = "receiveShippingFees")]
    pub receive_shipping_fees: HashMap<String, f64>,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub model_id: Option<String>,
}

pub fn get_model_pricing_config(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    model_id: &str,
) -> Result<ModelPricingConfig, AppError> {
    let global_config = get_pricing_config(pool, scope)?;
    let base_price = get_model_base_price(pool, scope, model_id)?;

    Ok(ModelPricingConfig {
        base_weekday_price: base_price.weekday_price,
        base_weekend_price: base_price.weekend_price,
        holiday_rules: global_config.holiday_rules,
        dynamic_price_map: global_config.dynamic_price_map,
        receive_shipping_fees: global_config.receive_shipping_fees,
        updated_by: global_config.updated_by,
        created_at: global_config.created_at,
        updated_at: global_config.updated_at,
        model_id: if model_id.is_empty() {
            None
        } else {
            Some(model_id.to_string())
        },
    })
}

// ── Occupancy coefficients ───────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct OccupancyCoefficients {
    pub shipping: f64,
    pub normal: f64,
    #[serde(rename = "return")]
    pub r#return: f64,
}

pub fn get_occupancy_coefficients() -> OccupancyCoefficients {
    OccupancyCoefficients {
        shipping: OCCUPANCY_COEFFICIENTS_SHIPPING,
        normal: OCCUPANCY_COEFFICIENTS_NORMAL,
        r#return: OCCUPANCY_COEFFICIENTS_RETURN,
    }
}

fn occupancy_factor(occupy_type: &str) -> f64 {
    match occupy_type {
        "shipping" => OCCUPANCY_COEFFICIENTS_SHIPPING,
        "return" => OCCUPANCY_COEFFICIENTS_RETURN,
        _ => OCCUPANCY_COEFFICIENTS_NORMAL,
    }
}

// ── Daily price resolution ───────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DailyPriceResult {
    pub price: f64,
    pub source: String,
}

pub fn resolve_daily_price(
    date_key: &str,
    pricing_config: &ModelPricingConfig,
) -> DailyPriceResult {
    // Priority: dynamic > holiday > base
    if let Some(&price) = pricing_config.dynamic_price_map.get(date_key) {
        return DailyPriceResult {
            price,
            source: "dynamic".to_string(),
        };
    }

    for rule in &pricing_config.holiday_rules {
        if rule.include_previous_day {
            let prev = time::add_days_to_date_key(&rule.start_date, -1);
            if date_key == prev {
                return DailyPriceResult {
                    price: rule.price,
                    source: "holiday".to_string(),
                };
            }
        }
        if *rule.start_date <= *date_key && *date_key <= *rule.end_date {
            return DailyPriceResult {
                price: rule.price,
                source: "holiday".to_string(),
            };
        }
    }

    if time::is_weekend(date_key) {
        DailyPriceResult {
            price: pricing_config.base_weekend_price,
            source: "base_weekend".to_string(),
        }
    } else {
        DailyPriceResult {
            price: pricing_config.base_weekday_price,
            source: "base_weekday".to_string(),
        }
    }
}

// ── Order price snapshot / breakdown ─────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPriceEntry {
    pub date_key: String,
    pub final_daily_price: f64,
    pub price_source: String,
    pub occupy_type: String,
    pub occupy_factor: f64,
    pub amount: f64,
}

pub fn calculate_order_price_snapshot(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    start_date: &str,
    end_date: &str,
    model_id: &str,
    province: &str,
) -> Result<Vec<DailyPriceEntry>, AppError> {
    if start_date.is_empty() || end_date.is_empty() {
        return Ok(Vec::new());
    }

    let pricing_config = get_model_pricing_config(pool, scope, model_id)?;

    // Resolve warehouse route for occupancy types
    let route = warehouse_routing_service::resolve_warehouse_route(pool, province)?;
    let shipping_days = route
        .as_ref()
        .map(|r| r.shipping_days as usize)
        .unwrap_or(1);
    let return_days = route.as_ref().map(|r| r.return_days as usize).unwrap_or(1);

    let start_normalized =
        normalize_date_str(start_date).unwrap_or_else(|_| start_date.to_string());
    let end_normalized = normalize_date_str(end_date).unwrap_or_else(|_| end_date.to_string());

    let total_days = time::get_date_diff_in_days(&start_normalized, &end_normalized) as usize + 1;

    let mut breakdown = Vec::new();
    let mut current = start_normalized.clone();

    let mut day_index: usize = 0;
    while current <= end_normalized {
        // Determine occupy type
        let occupy_type = if day_index < shipping_days {
            "shipping"
        } else if day_index >= total_days.saturating_sub(return_days) {
            "return"
        } else {
            "normal"
        };

        let factor = occupancy_factor(occupy_type);
        let daily = resolve_daily_price(&current, &pricing_config);
        let amount = normalize_price(daily.price * factor);

        breakdown.push(DailyPriceEntry {
            date_key: current.clone(),
            final_daily_price: daily.price,
            price_source: daily.source,
            occupy_type: occupy_type.to_string(),
            occupy_factor: factor,
            amount,
        });

        current = time::add_days_to_date_key(&current, 1);
        day_index += 1;
    }

    Ok(breakdown)
}

pub fn calculate_order_total(breakdown: &[DailyPriceEntry]) -> f64 {
    let sum: f64 = breakdown.iter().map(|d| d.amount).sum();
    normalize_price(sum)
}

// ── High-level estimate pricing ──────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingEstimate {
    pub breakdown: Vec<DailyPriceEntry>,
    pub total_price: f64,
    pub route: Option<warehouse_routing_service::WarehouseRoute>,
    pub shipping_estimate: Option<logistics_service::LogisticsEstimate>,
    pub return_estimate: Option<logistics_service::LogisticsEstimate>,
}

/// Estimate pricing for a rental order.
/// Calculates daily breakdown with occupancy coefficients from warehouse routing,
/// and includes shipping/return logistics estimates.
pub fn estimate_pricing(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    start_date: &str,
    end_date: &str,
    province: &str,
    model_id: &str,
) -> Result<PricingEstimate, AppError> {
    let breakdown =
        calculate_order_price_snapshot(pool, scope, start_date, end_date, model_id, province)?;
    let total_price = calculate_order_total(&breakdown);
    let route = warehouse_routing_service::resolve_warehouse_route(pool, province)?;

    let (shipping_estimate, return_estimate) = if let Some(ref r) = route {
        let ship = logistics_service::estimate_shipping(province, &r.send_warehouse_name);
        let ret =
            logistics_service::estimate_return(province, &r.return_warehouse_name).or_else(|| {
                if r.send_warehouse_name != r.return_warehouse_name {
                    logistics_service::estimate_return(province, &r.send_warehouse_name)
                } else {
                    None
                }
            });
        (ship, ret)
    } else {
        (None, None)
    };

    Ok(PricingEstimate {
        breakdown,
        total_price,
        route,
        shipping_estimate,
        return_estimate,
    })
}

// ── Dynamic price CRUD ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicPriceEntry {
    pub id: String,
    pub date_key: String,
    pub price: f64,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

/// List all dynamic daily prices.
pub fn get_dynamic_prices(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
) -> Result<Vec<DynamicPriceEntry>, AppError> {
    let conn = pool.get()?;
    let mut stmt = conn.prepare(
        "SELECT id, dateKey, price, updatedBy, createdAt, updatedAt
         FROM dynamic_daily_prices WHERE tenant_id = ?1
         ORDER BY dateKey ASC",
    )?;
    let rows = stmt.query_map(params![scope.tenant_id().as_str()], |row| {
        Ok(DynamicPriceEntry {
            id: row.get(0)?,
            date_key: row.get(1)?,
            price: row.get(2)?,
            updated_by: row.get::<_, String>(3).unwrap_or_default(),
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}

/// Insert or update a dynamic daily price for a specific date.
pub fn upsert_dynamic_price(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    date_key: &str,
    price: f64,
    updated_by: &str,
) -> Result<DynamicPriceEntry, AppError> {
    if !is_positive_number(price) {
        return Err(AppError::BadRequest("price 必须大于 0".to_string()));
    }

    let normalized_date = normalize_date_str(date_key)
        .map_err(|e| AppError::BadRequest(format!("日期格式无效: {}", e)))?;

    // Validate within rolling window
    let rolling_set: std::collections::HashSet<String> =
        get_rolling_date_keys(DYNAMIC_ROLLING_DAYS, &time::shanghai_now_date_key())
            .into_iter()
            .collect();
    if !rolling_set.contains(&normalized_date) {
        return Err(AppError::BadRequest(format!(
            "动态调价仅支持未来 {} 天：{}",
            DYNAMIC_ROLLING_DAYS, normalized_date
        )));
    }

    let conn = pool.get()?;
    let now = time::shanghai_now_iso();
    let normalized_price = normalize_price(price);

    // Check if entry already exists
    let mut stmt = conn.prepare(
        "SELECT id FROM dynamic_daily_prices WHERE tenant_id = ?1 AND dateKey = ?2 LIMIT 1",
    )?;
    let existing: Option<String> = stmt
        .query_row(
            params![scope.tenant_id().as_str(), normalized_date],
            |row| row.get(0),
        )
        .ok();

    if let Some(id) = existing {
        conn.execute(
            "UPDATE dynamic_daily_prices SET price = ?1, updatedBy = ?2, updatedAt = ?3 WHERE tenant_id = ?4 AND id = ?5",
            params![normalized_price, updated_by, now, scope.tenant_id().as_str(), id],
        )?;
        return Ok(DynamicPriceEntry {
            id,
            date_key: normalized_date,
            price: normalized_price,
            updated_by: updated_by.to_string(),
            created_at: now.clone(),
            updated_at: now,
        });
    }

    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO dynamic_daily_prices (id, dateKey, price, updatedBy, createdAt, updatedAt, tenant_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, normalized_date, normalized_price, updated_by, now, now, scope.tenant_id().as_str()],
    )?;

    Ok(DynamicPriceEntry {
        id,
        date_key: normalized_date,
        price: normalized_price,
        updated_by: updated_by.to_string(),
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Delete a dynamic daily price entry by date key.
pub fn delete_dynamic_price(
    pool: &Pool<SqliteConnectionManager>,
    scope: &DataScope,
    date_key: &str,
) -> Result<bool, AppError> {
    let normalized_date = normalize_date_str(date_key)
        .map_err(|e| AppError::BadRequest(format!("日期格式无效: {}", e)))?;

    let conn = pool.get()?;
    let affected = conn.execute(
        "DELETE FROM dynamic_daily_prices WHERE tenant_id = ?1 AND dateKey = ?2",
        params![scope.tenant_id().as_str(), normalized_date],
    )?;
    Ok(affected > 0)
}

/// Validate a config payload (matches JS validateConfigPayload).
/// Returns normalized HolidayRules and dynamicPriceMap, or an error.
#[derive(Debug)]
pub struct ValidatedConfigPayload {
    pub base_weekday_price: f64,
    pub base_weekend_price: f64,
    pub holiday_rules: Vec<HolidayRule>,
    pub dynamic_price_map: HashMap<String, f64>,
    pub receive_shipping_fees: HashMap<String, f64>,
}

pub fn validate_config_payload(
    base_weekday_price: f64,
    base_weekend_price: f64,
    holiday_rules: &[HolidayRuleInput],
    dynamic_price_map: &HashMap<String, f64>,
    receive_shipping_fees: Option<&HashMap<String, f64>>,
) -> Result<ValidatedConfigPayload, AppError> {
    if !is_positive_number(base_weekday_price) {
        return Err(AppError::BadRequest(
            "baseWeekdayPrice 必须大于 0".to_string(),
        ));
    }
    if !is_positive_number(base_weekend_price) {
        return Err(AppError::BadRequest(
            "baseWeekendPrice 必须大于 0".to_string(),
        ));
    }

    let mut normalized_holiday_rules = Vec::new();
    for (i, item) in holiday_rules.iter().enumerate() {
        let result = normalize_holiday_rule(item, i);
        if !result.ok {
            return Err(AppError::BadRequest(result.error.unwrap_or_default()));
        }
        if let Some(v) = result.value {
            normalized_holiday_rules.push(v);
        }
    }

    let rolling_set: std::collections::HashSet<String> =
        get_rolling_date_keys(DYNAMIC_ROLLING_DAYS, &time::shanghai_now_date_key())
            .into_iter()
            .collect();

    let mut normalized_dynamic_price_map = HashMap::new();
    for (raw_date_key, raw_price) in dynamic_price_map {
        let date_result = normalize_date_str(raw_date_key)
            .map_err(|_| AppError::BadRequest(format!("动态调价日期格式无效：{}", raw_date_key)))?;

        if !rolling_set.contains(&date_result) {
            return Err(AppError::BadRequest(format!(
                "动态调价仅支持未来 {} 天：{}",
                DYNAMIC_ROLLING_DAYS, date_result
            )));
        }

        if !is_positive_number(*raw_price) {
            return Err(AppError::BadRequest(format!(
                "动态调价必须大于 0：{}",
                date_result
            )));
        }

        normalized_dynamic_price_map.insert(date_result, normalize_price(*raw_price));
    }

    let normalized_receive_shipping_fees = if let Some(fees) = receive_shipping_fees {
        let mut normalized = HashMap::new();
        for key in RECEIVE_AREA_KEYS {
            let val = fees.get(*key).copied().unwrap_or(7.0);
            if !val.is_finite() || val < 0.0 {
                return Err(AppError::BadRequest(format!(
                    "receiveShippingFees.{} 必须为非负数",
                    key
                )));
            }
            normalized.insert(key.to_string(), normalize_price(val));
        }
        normalized
    } else {
        parse_receive_shipping_fees_json(DEFAULT_RECEIVE_SHIPPING_FEES)
    };

    Ok(ValidatedConfigPayload {
        base_weekday_price: normalize_price(base_weekday_price),
        base_weekend_price: normalize_price(base_weekend_price),
        holiday_rules: normalized_holiday_rules,
        dynamic_price_map: normalized_dynamic_price_map,
        receive_shipping_fees: normalized_receive_shipping_fees,
    })
}

#[cfg(test)]
mod tenant_isolation_tests {
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{DataScope, Revision, TenantId};

    use super::{get_dynamic_prices, get_pricing_config, upsert_dynamic_price};

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(
            TenantId::new(tenant).unwrap(),
            Revision::new("pricing-test-revision").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn pricing_config_and_dynamic_prices_are_tenant_isolated() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "CREATE TABLE pricing_configs (
                id INTEGER PRIMARY KEY, baseWeekdayPrice REAL NOT NULL,
                baseWeekendPrice REAL NOT NULL, holidayRulesJson TEXT NOT NULL DEFAULT '[]',
                receiveShippingFeesJson TEXT NOT NULL DEFAULT '{}', updatedBy TEXT DEFAULT '',
                createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL, tenant_id TEXT NOT NULL UNIQUE
             );
             CREATE TABLE dynamic_daily_prices (
                id TEXT PRIMARY KEY, dateKey TEXT NOT NULL, price REAL NOT NULL,
                updatedBy TEXT DEFAULT '', createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL,
                tenant_id TEXT NOT NULL, UNIQUE(tenant_id, dateKey)
             );
             INSERT INTO pricing_configs
                (baseWeekdayPrice, baseWeekendPrice, holidayRulesJson, receiveShippingFeesJson, updatedBy, createdAt, updatedAt, tenant_id)
             VALUES
                (10, 20, '[]', '{}', '', 'now', 'now', 'tenant-a'),
                (30, 40, '[]', '{}', '', 'now', 'now', 'tenant-b');",
        )
        .unwrap();
        drop(conn);

        let tenant_a = scope("tenant-a");
        let tenant_b = scope("tenant-b");
        assert_eq!(
            get_pricing_config(&pool, &tenant_a)
                .unwrap()
                .base_weekday_price,
            10.0
        );
        assert_eq!(
            get_pricing_config(&pool, &tenant_b)
                .unwrap()
                .base_weekday_price,
            30.0
        );

        let date = crate::utils::time::shanghai_now_date_key();
        upsert_dynamic_price(&pool, &tenant_a, &date, 88.0, "admin-a").unwrap();
        assert_eq!(get_dynamic_prices(&pool, &tenant_a).unwrap().len(), 1);
        assert!(get_dynamic_prices(&pool, &tenant_b).unwrap().is_empty());
    }
}

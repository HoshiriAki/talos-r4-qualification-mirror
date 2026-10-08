//! feature-booking — 在线预订系统模块（Maxwell 原生）
//!
//! 设备可用性查询、价格估算、预订、确认。
//!
//! ## 命令
//! - `availability` — 查询设备可用性（AuthUser）
//! - `estimate` — 价格估算（AuthUser）
//! - `reserve` — 预订（AdminUser）
//! - `confirm` — 确认预订关联到订单（AdminUser）
//! - `device_search` — 设备搜索（AuthUser）

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;

use system_core::{
    CommandSchema, DataScope, ErrorPayload, ExecutionContext, FieldError, ModuleMetadata,
    ModuleSchema, Sanitize, SystemModule, Validate, ValidationResult,
};

// ══════════════════════════════════════════════════════════════════════════
// Helpers
// ══════════════════════════════════════════════════════════════════════════

fn shanghai_now() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y-%m-%d %H:%M:%S").to_string()
}

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

fn require_admin(ctx: &ExecutionContext) -> Result<(), String> {
    if !ctx.has_tenant_admin_authority() {
        return Err(err("auth", "AUTH_FORBIDDEN", "仅管理员可操作".into()));
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Availability
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityInput {
    pub device_serial_no: Option<String>,
    pub start_date: String,
    pub end_date: String,
}

impl Validate for AvailabilityInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.start_date.is_empty() {
            errors.push(FieldError {
                field: "startDate".into(),
                message: "开始日期不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.end_date.is_empty() {
            errors.push(FieldError {
                field: "endDate".into(),
                message: "结束日期不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for AvailabilityInput {
    fn sanitize(&mut self) {
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
        if let Some(ref mut s) = self.device_serial_no {
            *s = s.trim().to_string();
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Estimate
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateInput {
    pub device_serial_no: String,
    pub start_date: String,
    pub end_date: String,
}

impl Validate for EstimateInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.device_serial_no.is_empty() {
            errors.push(FieldError {
                field: "deviceSerialNo".into(),
                message: "设备编号不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.start_date.is_empty() {
            errors.push(FieldError {
                field: "startDate".into(),
                message: "开始日期不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.end_date.is_empty() {
            errors.push(FieldError {
                field: "endDate".into(),
                message: "结束日期不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for EstimateInput {
    fn sanitize(&mut self) {
        self.device_serial_no = self.device_serial_no.trim().to_string();
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Reserve
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveInput {
    pub device_serial_no: String,
    pub start_date: String,
    pub end_date: String,
    pub customer_name: String,
    pub customer_phone: String,
}

impl Validate for ReserveInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.device_serial_no.is_empty() {
            errors.push(FieldError {
                field: "deviceSerialNo".into(),
                message: "设备编号不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.start_date.is_empty() {
            errors.push(FieldError {
                field: "startDate".into(),
                message: "开始日期不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.end_date.is_empty() {
            errors.push(FieldError {
                field: "endDate".into(),
                message: "结束日期不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.customer_name.is_empty() {
            errors.push(FieldError {
                field: "customerName".into(),
                message: "客户姓名不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.customer_phone.is_empty() {
            errors.push(FieldError {
                field: "customerPhone".into(),
                message: "客户电话不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ReserveInput {
    fn sanitize(&mut self) {
        self.device_serial_no = self.device_serial_no.trim().to_string();
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
        self.customer_name = self.customer_name.trim().to_string();
        self.customer_phone = self.customer_phone.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Confirm
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmInput {
    pub reservation_id: i64,
    pub order_id: i64,
}

impl Validate for ConfirmInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.reservation_id <= 0 {
            errors.push(FieldError {
                field: "reservationId".into(),
                message: "预订ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.order_id <= 0 {
            errors.push(FieldError {
                field: "orderId".into(),
                message: "订单ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ConfirmInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Device Search
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSearchInput {
    pub query: Option<String>,
    pub model: Option<String>,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
}

impl Validate for DeviceSearchInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for DeviceSearchInput {
    fn sanitize(&mut self) {
        if let Some(ref mut q) = self.query {
            *q = q.trim().to_string();
        }
        if let Some(ref mut m) = self.model {
            *m = m.trim().to_string();
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureBooking {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureBooking {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureBooking {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureBooking {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "booking".into(),
            version: "0.1.0".into(),
            description: "在线预订系统 — 设备可用性/价格估算/预订/确认".into(),
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
        let pool = pool_guard.as_ref().ok_or_else(|| {
            err(
                "sys",
                "SYS_NOT_INIT",
                "FeatureBooking not initialized".into(),
            )
        })?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;

        match command {
            "availability" => {
                let unvalidated: system_core::Unvalidated<AvailabilityInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                availability(&conn, ctx.data_scope(), &validated.into_inner())
            }
            "estimate" => {
                let unvalidated: system_core::Unvalidated<EstimateInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                estimate(&conn, ctx.data_scope(), &validated.into_inner())
            }
            "reserve" => Err(err(
                "biz",
                "BIZ_LEGACY_BOOKING_WRITE_RETIRED",
                "Use reservation_v2.create_legacy_hold".into(),
            )),
            "confirm" => Err(err(
                "biz",
                "BIZ_LEGACY_BOOKING_WRITE_RETIRED",
                "Use reservation_v2.confirm_reservation".into(),
            )),
            "device_search" => {
                let unvalidated: system_core::Unvalidated<DeviceSearchInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                device_search(&conn, ctx.data_scope(), &validated.into_inner())
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
                "availability",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Blocked,
            ),
            system_core::CommandMetadata::new(
                "estimate",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Blocked,
            ),
            system_core::CommandMetadata::new(
                "device_search",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Blocked,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "booking".into(),
            description: "在线预订系统模块".into(),
            commands: vec![
                CommandSchema {
                    name: "availability".into(),
                    description: "查询设备可用性".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "estimate".into(),
                    description: "价格估算".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "device_search".into(),
                    description: "设备搜索".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Availability
// ══════════════════════════════════════════════════════════════════════════

fn availability(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &AvailabilityInput,
) -> Result<Value, String> {
    // Get devices
    let devices_sql = if input.device_serial_no.is_some() {
        "SELECT d.serialNo, COALESCE(d.modelId, '') as model_id FROM devices d WHERE d.tenant_id = ?1 AND d.serialNo = ?2"
    } else {
        "SELECT d.serialNo, COALESCE(d.modelId, '') as model_id FROM devices d WHERE d.tenant_id = ?1"
    };

    let mut stmt = conn
        .prepare(devices_sql)
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
    let device_params: Vec<Box<dyn rusqlite::types::ToSql>> =
        if let Some(ref serial) = input.device_serial_no {
            vec![
                Box::new(scope.tenant_id().as_str().to_string()),
                Box::new(serial.clone()),
            ]
        } else {
            vec![Box::new(scope.tenant_id().as_str().to_string())]
        };
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        device_params.iter().map(|p| p.as_ref()).collect();

    let devices: Vec<(String, String)> = stmt
        .query_map(rusqlite::params_from_iter(param_refs), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    // For each date in range, check availability
    // Generate dates between start_date and end_date (inclusive)
    let start_dt = chrono::NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d")
        .map_err(|e| err("val", "VAL_INVALID", format!("开始日期格式无效: {}", e)))?;
    let end_dt = chrono::NaiveDate::parse_from_str(&input.end_date, "%Y-%m-%d")
        .map_err(|e| err("val", "VAL_INVALID", format!("结束日期格式无效: {}", e)))?;

    let mut dates: Vec<Value> = Vec::new();
    let mut current = start_dt;
    while current <= end_dt {
        let date_str = current.format("%Y-%m-%d").to_string();

        // For each device, check if there's an availability record for this date
        let mut stmt2 = conn.prepare(
            "SELECT device_serial_no, is_available FROM booking_availability WHERE tenant_id = ?1 AND date = ?2"
        ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

        let avail_map: std::collections::HashMap<String, bool> = stmt2
            .query_map(params![scope.tenant_id().as_str(), date_str], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)? == 1))
            })
            .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        for (serial, model_id) in &devices {
            // If no availability record exists, device is available by default
            let available = avail_map.get(serial).copied().unwrap_or(true);
            dates.push(serde_json::json!({
                "date": date_str,
                "available": available,
                "deviceSerialNo": serial,
                "deviceModel": model_id,
            }));
        }

        current += chrono::Duration::days(1);
    }

    Ok(serde_json::json!({
        "ok": true,
        "dates": dates,
        "devices": devices.iter().map(|(s, m)| serde_json::json!({
            "serialNo": s,
            "model": m,
        })).collect::<Vec<Value>>(),
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// Estimate
// ══════════════════════════════════════════════════════════════════════════

fn estimate(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &EstimateInput,
) -> Result<Value, String> {
    let start_dt = chrono::NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d")
        .map_err(|e| err("val", "VAL_INVALID", format!("开始日期格式无效: {}", e)))?;
    let end_dt = chrono::NaiveDate::parse_from_str(&input.end_date, "%Y-%m-%d")
        .map_err(|e| err("val", "VAL_INVALID", format!("结束日期格式无效: {}", e)))?;

    let days = (end_dt - start_dt).num_days().max(1) as i64;

    // Get device model to look up base price
    let model_info: Option<(String, Option<f64>, Option<f64>)> = conn
        .query_row(
            "SELECT COALESCE(m.name, '') as model_name, bp.weekdayPrice, bp.weekendPrice
         FROM devices d
         LEFT JOIN device_models m ON d.modelId = m.id AND m.tenant_id = d.tenant_id
         LEFT JOIN model_base_prices bp ON d.modelId = bp.modelId
         WHERE d.tenant_id = ?1 AND d.serialNo = ?2",
            params![scope.tenant_id().as_str(), input.device_serial_no],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<f64>>(1)?,
                    row.get::<_, Option<f64>>(2)?,
                ))
            },
        )
        .ok();

    // Fallback pricing: weekday 8.5, weekend 14 as per CLAUDE.md pricing engine
    let (model_name, weekday_price, weekend_price) =
        model_info.unwrap_or((String::new(), Some(8.5), Some(14.0)));

    let weekday_rate = weekday_price.unwrap_or(8.5);
    let weekend_rate = weekend_price.unwrap_or(14.0);

    // Calculate: iterate days, weekends are Fri(5), Sat(6), Sun(0)
    let mut total_rate = 0.0;
    let mut current = start_dt;
    let mut weekday_count = 0_i64;
    let mut weekend_count = 0_i64;
    while current <= end_dt {
        let dow = current.format("%u").to_string().parse::<u32>().unwrap_or(1);
        // chrono %u: Mon=1..Sun=7. Weekend = Fri(5), Sat(6), Sun(7)
        if dow >= 5 {
            total_rate += weekend_rate;
            weekend_count += 1;
        } else {
            total_rate += weekday_rate;
            weekday_count += 1;
        }
        current += chrono::Duration::days(1);
    }

    let daily_rate = total_rate / days as f64;
    let subtotal = total_rate;
    let deposit = subtotal; // deposit = subtotal as simple default
    let total = subtotal + deposit;

    Ok(serde_json::json!({
        "ok": true,
        "deviceSerialNo": input.device_serial_no,
        "deviceModel": model_name,
        "startDate": input.start_date,
        "endDate": input.end_date,
        "days": days,
        "weekdayCount": weekday_count,
        "weekendCount": weekend_count,
        "weekdayRate": weekday_rate,
        "weekendRate": weekend_rate,
        "dailyRate": (daily_rate * 100.0).round() / 100.0,
        "subtotal": (subtotal * 100.0).round() / 100.0,
        "deposit": (deposit * 100.0).round() / 100.0,
        "total": (total * 100.0).round() / 100.0,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// Reserve
// ══════════════════════════════════════════════════════════════════════════

fn reserve(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ReserveInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let owns_device = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM devices WHERE tenant_id = ?1 AND serialNo = ?2)",
            params![scope.tenant_id().as_str(), input.device_serial_no],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
    if !owns_device {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err("biz", "BIZ_NOT_FOUND", "Device not found".into()));
    }

    let start_dt =
        chrono::NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d").map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("val", "VAL_INVALID", format!("开始日期格式无效: {}", e))
        })?;
    let end_dt = chrono::NaiveDate::parse_from_str(&input.end_date, "%Y-%m-%d").map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("val", "VAL_INVALID", format!("结束日期格式无效: {}", e))
    })?;

    // Check availability for all dates in range
    let mut current = start_dt;
    while current <= end_dt {
        let date_str = current.format("%Y-%m-%d").to_string();
        let existing: Option<i32> = conn.query_row(
            "SELECT is_available FROM booking_availability WHERE tenant_id = ?1 AND device_serial_no = ?2 AND date = ?3",
            params![scope.tenant_id().as_str(), input.device_serial_no, date_str],
            |row| row.get(0),
        ).ok();

        if existing == Some(0) {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err(
                "biz",
                "BIZ_UNAVAILABLE",
                format!("设备 {} 在 {} 不可用", input.device_serial_no, date_str),
            ));
        }

        current += chrono::Duration::days(1);
    }

    // Mark all dates as reserved
    let now = shanghai_now();
    let mut current = start_dt;
    while current <= end_dt {
        let date_str = current.format("%Y-%m-%d").to_string();
        conn.execute(
            "INSERT INTO booking_availability (device_serial_no, date, is_available, created_at, tenant_id)
             VALUES (?1, ?2, 0, ?3, ?4)
             ON CONFLICT(device_serial_no, date) DO UPDATE SET is_available = 0, created_at = ?3
             WHERE booking_availability.tenant_id = ?4",
            params![input.device_serial_no, date_str, now, scope.tenant_id().as_str()],
        ).map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_INSERT", e.to_string())
        })?;

        current += chrono::Duration::days(1);
    }

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true,
        "deviceSerialNo": input.device_serial_no,
        "startDate": input.start_date,
        "endDate": input.end_date,
        "customerName": input.customer_name,
        "customerPhone": input.customer_phone,
        "reservedAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// Confirm
// ══════════════════════════════════════════════════════════════════════════

fn confirm(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ConfirmInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let now = shanghai_now();
    // Update all booking_availability records for this reservation's device and dates
    // to link to the order. Since we don't store reservation_id directly,
    // we update any unlinked reservations for that device.
    let updated = conn
        .execute(
            "UPDATE booking_availability SET reserved_order_id = ?1
         WHERE device_serial_no = (
             SELECT device_serial_no FROM booking_availability WHERE id = ?2 AND tenant_id = ?3
         ) AND tenant_id = ?3 AND is_available = 0 AND reserved_order_id IS NULL
           AND EXISTS (SELECT 1 FROM orders WHERE id = ?1 AND tenant_id = ?3)",
            params![
                input.order_id,
                input.reservation_id,
                scope.tenant_id().as_str()
            ],
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_UPDATE", e.to_string())
        })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true,
        "reservationId": input.reservation_id,
        "orderId": input.order_id,
        "updatedRows": updated,
        "confirmedAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// Device Search
// ══════════════════════════════════════════════════════════════════════════

fn device_search(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &DeviceSearchInput,
) -> Result<Value, String> {
    let mut conditions: Vec<String> = vec!["d.tenant_id = ?1".into()];
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(scope.tenant_id().as_str().to_string())];

    if let Some(ref query) = input.query {
        if !query.is_empty() {
            let idx = conditions.len() + 1;
            conditions.push(format!(
                "(d.serialNo LIKE ?{} OR m.name LIKE ?{} OR COALESCE(d.modelId,'') LIKE ?{})",
                idx, idx, idx
            ));
            all_params.push(Box::new(format!("%{}%", query)));
        }
    }
    if let Some(ref model) = input.model {
        if !model.is_empty() {
            let idx = conditions.len() + 1;
            conditions.push(format!("COALESCE(d.modelId, '') = ?{}", idx));
            all_params.push(Box::new(model.clone()));
        }
    }

    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT d.serialNo, COALESCE(d.modelId, '') as model_id, COALESCE(m.name, '') as model_name,
                COALESCE(bp.weekdayPrice, 8.5) as weekday_price, COALESCE(bp.weekendPrice, 14.0) as weekend_price
         FROM devices d
         LEFT JOIN device_models m ON d.modelId = m.id AND m.tenant_id = d.tenant_id
         LEFT JOIN model_base_prices bp ON d.modelId = bp.modelId
         {}
         ORDER BY d.serialNo",
        where_sql
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let mut items: Vec<Value> = if all_params.is_empty() {
        stmt.query_map([], |row| {
            let wd: f64 = row.get(3)?;
            let we: f64 = row.get(4)?;
            Ok(serde_json::json!({
                "serialNo": row.get::<_, String>(0)?,
                "modelId": row.get::<_, String>(1)?,
                "modelName": row.get::<_, String>(2)?,
                "weekdayPrice": wd,
                "weekendPrice": we,
                "dailyRate": ((wd + we) / 2.0 * 100.0).round() / 100.0,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            all_params.iter().map(|p| p.as_ref()).collect();
        stmt.query_map(rusqlite::params_from_iter(param_refs), |row| {
            let wd: f64 = row.get(3)?;
            let we: f64 = row.get(4)?;
            Ok(serde_json::json!({
                "serialNo": row.get::<_, String>(0)?,
                "modelId": row.get::<_, String>(1)?,
                "modelName": row.get::<_, String>(2)?,
                "weekdayPrice": wd,
                "weekendPrice": we,
                "dailyRate": ((wd + we) / 2.0 * 100.0).round() / 100.0,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    // Apply price range filter client-side (optional filters for min/max)
    if let Some(min_price) = input.min_price {
        items.retain(|item| {
            item.get("dailyRate")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                >= min_price
        });
    }
    if let Some(max_price) = input.max_price {
        items.retain(|item| {
            item.get("dailyRate")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                <= max_price
        });
    }

    Ok(serde_json::json!({ "ok": true, "items": items, "total": items.len() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_core::{DataScope, Revision, TenantId};

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(
            TenantId::new(tenant).expect("tenant id"),
            Revision::new("r1").expect("revision"),
        )
        .expect("resolved production scope")
    }

    #[test]
    fn booking_reads_and_writes_are_limited_to_data_scope() {
        let conn = rusqlite::Connection::open_in_memory().expect("database");
        conn.execute_batch(
            "CREATE TABLE devices (
                serialNo TEXT PRIMARY KEY,
                modelId TEXT,
                tenant_id TEXT NOT NULL
             );
             CREATE TABLE booking_availability (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_serial_no TEXT NOT NULL,
                date TEXT NOT NULL,
                is_available INTEGER NOT NULL DEFAULT 1,
                reserved_order_id INTEGER,
                created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL,
                UNIQUE(device_serial_no, date)
             );
             INSERT INTO devices VALUES ('A-001', 'MODEL-A', 'tenant-a');
             INSERT INTO devices VALUES ('B-001', 'MODEL-B', 'tenant-b');
             INSERT INTO booking_availability
                (device_serial_no, date, is_available, created_at, tenant_id)
             VALUES ('B-001', '2026-07-20', 0, '2026-07-16 12:00:00', 'tenant-b');",
        )
        .expect("schema and fixtures");

        let tenant_a = scope("tenant-a");
        let result = availability(
            &conn,
            &tenant_a,
            &AvailabilityInput {
                device_serial_no: None,
                start_date: "2026-07-20".into(),
                end_date: "2026-07-20".into(),
            },
        )
        .expect("tenant-scoped availability");
        assert_eq!(result["devices"].as_array().expect("devices").len(), 1);
        assert_eq!(result["devices"][0]["serialNo"], "A-001");

        reserve(
            &conn,
            &tenant_a,
            &ReserveInput {
                device_serial_no: "A-001".into(),
                start_date: "2026-07-20".into(),
                end_date: "2026-07-20".into(),
                customer_name: "Alice".into(),
                customer_phone: "13800000000".into(),
            },
        )
        .expect("tenant-scoped reservation");
        let stored_tenant: String = conn
            .query_row(
                "SELECT tenant_id FROM booking_availability WHERE device_serial_no = 'A-001'",
                [],
                |row| row.get(0),
            )
            .expect("stored tenant");
        assert_eq!(stored_tenant, "tenant-a");
    }
}

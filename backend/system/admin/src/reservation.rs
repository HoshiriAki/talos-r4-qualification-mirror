//! feature-reservation — 多仓库库存预留模块（Maxwell 原生）
//!
//! 设备预留、冲突检测、释放、确认、过期扫描。
//!
//! ## 命令
//! - `reserve` — 预留设备（AuthUser）
//! - `release` — 释放预留（AuthUser）
//! - `confirm` — 确认预留→订单（AuthUser）
//! - `expire` — 批量过期（AdminUser）
//! - `list` — 分页列表（AuthUser）
//! - `get` — 单条查询（AuthUser）
//! - `check_availability` — 检查设备可用性（AuthUser）
//! - `rule_get` — 获取预留规则（AuthUser）
//! - `rule_update` — 更新预留规则（AdminUser）

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;

use system_core::{
    CommandSchema, DataScope, ErrorPayload, ExecutionContext, FieldError, ModuleMetadata,
    ModuleSchema, Sanitize, SystemModule, Unvalidated, Validate, ValidationResult,
};
#[cfg(test)]
use system_core::{Revision, TenantId};

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

/// Check if two date ranges [a_start, a_end) and [b_start, b_end) overlap.
/// Returns true if they conflict.
fn date_range_overlap(a_start: &str, a_end: &str, b_start: &str, b_end: &str) -> bool {
    a_start < b_end && b_start < a_end
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Reserve
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveInput {
    pub device_serial_no: String,
    pub warehouse_id: Option<i64>,
    pub order_id: Option<i64>,
    pub customer_name: String,
    pub customer_phone: String,
    pub start_date: String,
    pub end_date: String,
    pub notes: Option<String>,
}

impl Validate for ReserveInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.device_serial_no.is_empty() {
            errors.push(FieldError {
                field: "deviceSerialNo".into(),
                message: "设备序列号不能为空".into(),
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
        if !self.start_date.is_empty()
            && !self.end_date.is_empty()
            && self.start_date >= self.end_date
        {
            errors.push(FieldError {
                field: "dateRange".into(),
                message: "开始日期必须在结束日期之前".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ReserveInput {
    fn sanitize(&mut self) {
        self.device_serial_no = self.device_serial_no.trim().to_string();
        self.customer_name = self.customer_name.trim().to_string();
        self.customer_phone = self.customer_phone.trim().to_string();
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
        if let Some(ref mut n) = self.notes {
            *n = n.trim().to_string();
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Release
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInput {
    pub id: i64,
}

impl Validate for ReleaseInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "预留ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ReleaseInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Confirm
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmInput {
    pub id: i64,
    pub order_id: i64,
}

impl Validate for ConfirmInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "预留ID无效".into(),
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
// Input types — Expire (empty — batch scan)
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpireInput {}

impl Validate for ExpireInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for ExpireInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — List
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationListInput {
    pub status: Option<String>,
    pub device_serial_no: Option<String>,
    pub customer_phone: Option<String>,
    pub warehouse_id: Option<i64>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl Validate for ReservationListInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for ReservationListInput {
    fn sanitize(&mut self) {
        if let Some(ref mut s) = self.status {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.device_serial_no {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.customer_phone {
            *s = s.trim().to_string();
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Get
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationGetInput {
    pub id: i64,
}

impl Validate for ReservationGetInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "预留ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ReservationGetInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Check Availability
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckAvailabilityInput {
    pub device_serial_no: String,
    pub start_date: String,
    pub end_date: String,
}

impl Validate for CheckAvailabilityInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.device_serial_no.is_empty() {
            errors.push(FieldError {
                field: "deviceSerialNo".into(),
                message: "设备序列号不能为空".into(),
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
        if !self.start_date.is_empty()
            && !self.end_date.is_empty()
            && self.start_date >= self.end_date
        {
            errors.push(FieldError {
                field: "dateRange".into(),
                message: "开始日期必须在结束日期之前".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for CheckAvailabilityInput {
    fn sanitize(&mut self) {
        self.device_serial_no = self.device_serial_no.trim().to_string();
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Rule Get (empty)
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleGetInput {}

impl Validate for RuleGetInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for RuleGetInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Rule Update
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleUpdateInput {
    pub max_days_ahead: Option<i64>,
    pub max_concurrent_per_customer: Option<i64>,
    pub auto_release_minutes: Option<i64>,
    pub is_active: Option<bool>,
}

impl Validate for RuleUpdateInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if let Some(d) = self.max_days_ahead
            && !(1..=365).contains(&d)
        {
            errors.push(FieldError {
                field: "maxDaysAhead".into(),
                message: "最大提前天数必须在 1-365 之间".into(),
                code: "VAL_INVALID".into(),
            });
        }
        if let Some(c) = self.max_concurrent_per_customer
            && !(0..=100).contains(&c)
        {
            errors.push(FieldError {
                field: "maxConcurrentPerCustomer".into(),
                message: "每客户最大并发数必须在 0-100 之间".into(),
                code: "VAL_INVALID".into(),
            });
        }
        if let Some(m) = self.auto_release_minutes
            && !(1..=10080).contains(&m)
        {
            errors.push(FieldError {
                field: "autoReleaseMinutes".into(),
                message: "自动释放分钟数必须在 1-10080 之间".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for RuleUpdateInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureReservation {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureReservation {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureReservation {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureReservation {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "reservation".into(),
            version: "0.1.0".into(),
            description: "多仓库库存预留 — 预留/释放/确认/过期扫描/可用性检查".into(),
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
                "FeatureReservation not initialized".into(),
            )
        })?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;
        let scope = ctx.data_scope();

        match command {
            "reserve" => {
                let unvalidated: Unvalidated<ReserveInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let staff_id = ctx
                    .user_id()
                    .and_then(|id| id.parse::<i64>().ok())
                    .ok_or_else(|| err("auth", "AUTH_REQUIRED", "需要登录".into()))?;
                reserve(&conn, scope, &input, staff_id)
            }
            "release" => {
                let unvalidated: Unvalidated<ReleaseInput> = payload.try_into()?;
                release(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "confirm" => {
                let unvalidated: Unvalidated<ConfirmInput> = payload.try_into()?;
                confirm(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "expire" => {
                require_admin(ctx)?;
                let unvalidated: Unvalidated<ExpireInput> = payload.try_into()?;
                expire_batch(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "list" => {
                let unvalidated: Unvalidated<ReservationListInput> = payload.try_into()?;
                list(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "get" => {
                let unvalidated: Unvalidated<ReservationGetInput> = payload.try_into()?;
                get(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "check_availability" => {
                let unvalidated: Unvalidated<CheckAvailabilityInput> = payload.try_into()?;
                check_availability(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "rule_get" => {
                let unvalidated: Unvalidated<RuleGetInput> = payload.try_into()?;
                rule_get(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "rule_update" => {
                require_admin(ctx)?;
                let unvalidated: Unvalidated<RuleUpdateInput> = payload.try_into()?;
                rule_update(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
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
                "list",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Blocked,
            ),
            system_core::CommandMetadata::new(
                "get",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Blocked,
            ),
            system_core::CommandMetadata::new(
                "check_availability",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Blocked,
            ),
            system_core::CommandMetadata::new(
                "rule_get",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Blocked,
            ),
            system_core::CommandMetadata::new(
                "rule_update",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Blocked,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "reservation".into(),
            description: "Legacy Reservation compatibility surface; writes are canonicalized in reservation_v2".into(),
            commands: vec![
                CommandSchema {
                    name: "list".into(),
                    description: "预留分页列表（legacy compatibility）".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get".into(),
                    description: "查询预留（legacy compatibility）".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "check_availability".into(),
                    description: "检查 legacy 预留记录冲突".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "rule_get".into(),
                    description: "获取预留兼容规则".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "rule_update".into(),
                    description: "更新预留兼容规则".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// reserve — 预留设备（含冲突检测）
// ══════════════════════════════════════════════════════════════════════════

fn reserve(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ReserveInput,
    staff_id: i64,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Check concurrent per-customer limit
    let rule = get_active_rule(conn, scope)?;
    let concurrent_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM inventory_reservations WHERE tenant_id = ?1 AND customer_phone = ?2 AND status IN ('reserved', 'confirmed')",
        params![scope.tenant_id().as_str(), input.customer_phone],
        |row| row.get(0),
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_QUERY", e.to_string())
    })?;

    if concurrent_count >= rule.max_concurrent_per_customer {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_LIMIT_EXCEEDED",
            format!(
                "客户已有 {} 个有效预留，达到上限 {}",
                concurrent_count, rule.max_concurrent_per_customer
            ),
        ));
    }

    // Check date range conflict: any active reservation for the same device with overlapping dates
    let mut stmt = conn
        .prepare(
            "SELECT id, device_serial_no, customer_name, start_date, end_date, status
         FROM inventory_reservations
         WHERE tenant_id = ?1 AND device_serial_no = ?2 AND status IN ('reserved', 'confirmed')",
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    let conflicts: Vec<Value> = stmt
        .query_map(
            params![scope.tenant_id().as_str(), input.device_serial_no],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?
        .filter_map(|r| r.ok())
        .filter(|(_, _, _, s, e, _)| date_range_overlap(&input.start_date, &input.end_date, s, e))
        .map(|(id, _dsn, cn, s, e, st)| {
            serde_json::json!({
                "id": id,
                "customerName": cn,
                "startDate": s,
                "endDate": e,
                "status": st,
            })
        })
        .collect();

    if !conflicts.is_empty() {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_CONFLICT",
            format!(
                "设备 {} 在指定时段已被预留，存在 {} 个冲突",
                input.device_serial_no,
                conflicts.len()
            ),
        ));
    }

    let now = shanghai_now();
    conn.execute(
        "INSERT INTO inventory_reservations (device_serial_no, warehouse_id, order_id, customer_name, customer_phone, start_date, end_date, status, notes, reserved_by, created_at, updated_at, tenant_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'reserved', ?8, ?9, ?10, ?10, ?11)",
        params![
            input.device_serial_no, input.warehouse_id, input.order_id,
            input.customer_name, input.customer_phone,
            input.start_date, input.end_date,
            input.notes, staff_id, now, scope.tenant_id().as_str()
        ],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    let id = conn.last_insert_rowid();
    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": id, "createdAt": now }))
}

// ══════════════════════════════════════════════════════════════════════════
// release — 释放一个预留
// ══════════════════════════════════════════════════════════════════════════

fn release(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ReleaseInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let current_status: String = conn
        .query_row(
            "SELECT status FROM inventory_reservations WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if current_status != "reserved" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_STATUS_INVALID",
            format!(
                "当前状态 '{}' 不可释放，仅 'reserved' 状态的预留可释放",
                current_status
            ),
        ));
    }

    let now = shanghai_now();
    conn.execute(
        "UPDATE inventory_reservations SET status = 'released', updated_at = ?1 WHERE tenant_id = ?2 AND id = ?3",
        params![now, scope.tenant_id().as_str(), input.id],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": input.id, "status": "released", "releasedAt": now }))
}

// ══════════════════════════════════════════════════════════════════════════
// confirm — 确认预留 → 关联订单
// ══════════════════════════════════════════════════════════════════════════

fn confirm(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ConfirmInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let current_status: String = conn
        .query_row(
            "SELECT status FROM inventory_reservations WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if current_status != "reserved" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_STATUS_INVALID",
            format!(
                "当前状态 '{}' 不可确认，仅 'reserved' 状态的预留可确认",
                current_status
            ),
        ));
    }

    let now = shanghai_now();
    conn.execute(
        "UPDATE inventory_reservations SET status = 'confirmed', order_id = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4",
        params![input.order_id, now, scope.tenant_id().as_str(), input.id],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(
        serde_json::json!({ "ok": true, "id": input.id, "orderId": input.order_id, "status": "confirmed", "confirmedAt": now }),
    )
}

// ══════════════════════════════════════════════════════════════════════════
// expire_batch — 批量过期超过 auto_release_minutes 的预留
// ══════════════════════════════════════════════════════════════════════════

fn expire_batch(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    _input: &ExpireInput,
) -> Result<Value, String> {
    let rule = get_active_rule(conn, scope)?;
    let now = shanghai_now();

    conn.execute(
        "UPDATE inventory_reservations SET status = 'expired', updated_at = ?1
         WHERE tenant_id = ?1 AND status = 'reserved'
           AND datetime(created_at, printf('+%d minutes', ?2)) <= ?3",
        params![scope.tenant_id().as_str(), rule.auto_release_minutes, now],
    )
    .map_err(|e| err("db", "DB_UPDATE", e.to_string()))?;

    let expired_count = conn.changes() as i64;

    Ok(serde_json::json!({
        "ok": true,
        "expiredCount": expired_count,
        "autoReleaseMinutes": rule.auto_release_minutes,
        "expiredAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// list — 分页列表（参数化 SQL）
// ══════════════════════════════════════════════════════════════════════════

fn list(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ReservationListInput,
) -> Result<Value, String> {
    let page = input.page.unwrap_or(1).max(1);
    let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let mut conditions: Vec<String> = vec!["tenant_id = ?1".into()];
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(scope.tenant_id().as_str().to_string())];

    if let Some(ref status) = input.status {
        let idx = conditions.len() + 1;
        conditions.push(format!("status = ?{}", idx));
        all_params.push(Box::new(status.clone()));
    }
    if let Some(ref dsn) = input.device_serial_no {
        let idx = conditions.len() + 1;
        conditions.push(format!("device_serial_no = ?{}", idx));
        all_params.push(Box::new(dsn.clone()));
    }
    if let Some(ref phone) = input.customer_phone {
        let idx = conditions.len() + 1;
        conditions.push(format!("customer_phone = ?{}", idx));
        all_params.push(Box::new(phone.clone()));
    }
    if let Some(wid) = input.warehouse_id {
        let idx = conditions.len() + 1;
        conditions.push(format!("warehouse_id = ?{}", idx));
        all_params.push(Box::new(wid));
    }

    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Count
    let count_sql = format!("SELECT COUNT(*) FROM inventory_reservations {}", where_sql);
    let total: i64 = if all_params.is_empty() {
        conn.query_row(&count_sql, [], |row| row.get(0))
    } else {
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            all_params.iter().map(|p| p.as_ref()).collect();
        conn.query_row(&count_sql, rusqlite::params_from_iter(param_refs), |row| {
            row.get(0)
        })
    }
    .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    // Query
    let query_sql = format!(
        "SELECT id, device_serial_no, warehouse_id, order_id, customer_name, customer_phone, start_date, end_date, status, notes, reserved_by, created_at, updated_at
         FROM inventory_reservations {} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
        where_sql,
        all_params.len() + 1,
        all_params.len() + 2,
    );

    let mut stmt = conn
        .prepare(&query_sql)
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let mut final_params: Vec<Box<dyn rusqlite::types::ToSql>> = all_params;
    final_params.push(Box::new(page_size));
    final_params.push(Box::new(offset));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        final_params.iter().map(|p| p.as_ref()).collect();

    let items: Vec<Value> = stmt
        .query_map(rusqlite::params_from_iter(param_refs), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "deviceSerialNo": row.get::<_, String>(1)?,
                "warehouseId": row.get::<_, Option<i64>>(2)?,
                "orderId": row.get::<_, Option<i64>>(3)?,
                "customerName": row.get::<_, String>(4)?,
                "customerPhone": row.get::<_, String>(5)?,
                "startDate": row.get::<_, String>(6)?,
                "endDate": row.get::<_, String>(7)?,
                "status": row.get::<_, String>(8)?,
                "notes": row.get::<_, Option<String>>(9)?,
                "reservedBy": row.get::<_, i64>(10)?,
                "createdAt": row.get::<_, String>(11)?,
                "updatedAt": row.get::<_, String>(12)?,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(
        serde_json::json!({ "ok": true, "items": items, "total": total, "page": page, "pageSize": page_size }),
    )
}

// ══════════════════════════════════════════════════════════════════════════
// get — 单条查询
// ══════════════════════════════════════════════════════════════════════════

fn get(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ReservationGetInput,
) -> Result<Value, String> {
    let record = conn.query_row(
        "SELECT id, device_serial_no, warehouse_id, order_id, customer_name, customer_phone, start_date, end_date, status, notes, reserved_by, created_at, updated_at
         FROM inventory_reservations WHERE tenant_id = ?1 AND id = ?2",
        params![scope.tenant_id().as_str(), input.id],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "deviceSerialNo": row.get::<_, String>(1)?,
                "warehouseId": row.get::<_, Option<i64>>(2)?,
                "orderId": row.get::<_, Option<i64>>(3)?,
                "customerName": row.get::<_, String>(4)?,
                "customerPhone": row.get::<_, String>(5)?,
                "startDate": row.get::<_, String>(6)?,
                "endDate": row.get::<_, String>(7)?,
                "status": row.get::<_, String>(8)?,
                "notes": row.get::<_, Option<String>>(9)?,
                "reservedBy": row.get::<_, i64>(10)?,
                "createdAt": row.get::<_, String>(11)?,
                "updatedAt": row.get::<_, String>(12)?,
            }))
        },
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "record": record }))
}

// ══════════════════════════════════════════════════════════════════════════
// check_availability — 检查设备在指定日期的可用性
// ══════════════════════════════════════════════════════════════════════════

fn check_availability(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &CheckAvailabilityInput,
) -> Result<Value, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, customer_name, customer_phone, start_date, end_date, status
         FROM inventory_reservations
         WHERE tenant_id = ?1 AND device_serial_no = ?2 AND status IN ('reserved', 'confirmed')",
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let conflicts: Vec<Value> = stmt
        .query_map(
            params![scope.tenant_id().as_str(), input.device_serial_no],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .filter(|(_, _, _, s, e, _)| date_range_overlap(&input.start_date, &input.end_date, s, e))
        .map(|(id, cn, cp, s, e, st)| {
            serde_json::json!({
                "id": id,
                "customerName": cn,
                "customerPhone": cp,
                "startDate": s,
                "endDate": e,
                "status": st,
            })
        })
        .collect();

    let available = conflicts.is_empty();

    Ok(serde_json::json!({
        "ok": true,
        "deviceSerialNo": input.device_serial_no,
        "startDate": input.start_date,
        "endDate": input.end_date,
        "available": available,
        "conflicts": conflicts,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// Rule helpers
// ══════════════════════════════════════════════════════════════════════════

struct ReservationRule {
    max_concurrent_per_customer: i64,
    auto_release_minutes: i64,
}

fn get_active_rule(
    conn: &rusqlite::Connection,
    scope: &DataScope,
) -> Result<ReservationRule, String> {
    conn.query_row(
        "SELECT max_concurrent_per_customer, auto_release_minutes FROM reservation_rules WHERE tenant_id = ?1 AND is_active = 1 LIMIT 1",
        params![scope.tenant_id().as_str()],
        |row| Ok(ReservationRule {
            max_concurrent_per_customer: row.get(0)?,
            auto_release_minutes: row.get(1)?,
        }),
    ).or_else(|_| {
        // Auto-seed if missing
        let now = shanghai_now();
        conn.execute(
            "INSERT OR IGNORE INTO reservation_rules (rule_name, max_days_ahead, max_concurrent_per_customer, auto_release_minutes, is_active, created_at, updated_at, tenant_id)
             VALUES ('default', 90, 2, 30, 1, ?1, ?1, ?2)",
            params![now, scope.tenant_id().as_str()],
        ).map_err(|e| err("db", "DB_INSERT", e.to_string()))?;
        Ok(ReservationRule {
            max_concurrent_per_customer: 2,
            auto_release_minutes: 30,
        })
    })
}

// ══════════════════════════════════════════════════════════════════════════
// rule_get
// ══════════════════════════════════════════════════════════════════════════

fn rule_get(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    _input: &RuleGetInput,
) -> Result<Value, String> {
    let rule = conn.query_row(
        "SELECT id, rule_name, max_days_ahead, max_concurrent_per_customer, auto_release_minutes, is_active, created_at, updated_at
         FROM reservation_rules WHERE tenant_id = ?1 AND is_active = 1 LIMIT 1",
        params![scope.tenant_id().as_str()],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "ruleName": row.get::<_, String>(1)?,
                "maxDaysAhead": row.get::<_, i64>(2)?,
                "maxConcurrentPerCustomer": row.get::<_, i64>(3)?,
                "autoReleaseMinutes": row.get::<_, i64>(4)?,
                "isActive": row.get::<_, i32>(5)? == 1,
                "createdAt": row.get::<_, String>(6)?,
                "updatedAt": row.get::<_, String>(7)?,
            }))
        },
    ).or_else(|_| -> Result<Value, String> {
        // Auto-seed
        let now = shanghai_now();
        conn.execute(
            "INSERT OR IGNORE INTO reservation_rules (rule_name, max_days_ahead, max_concurrent_per_customer, auto_release_minutes, is_active, created_at, updated_at, tenant_id)
             VALUES ('default', 90, 2, 30, 1, ?1, ?1, ?2)",
            params![now, scope.tenant_id().as_str()],
        ).map_err(|e| err("db", "DB_INSERT", e.to_string()))?;
        Ok(serde_json::json!({
            "id": 1,
            "ruleName": "default",
            "maxDaysAhead": 90,
            "maxConcurrentPerCustomer": 2,
            "autoReleaseMinutes": 30,
            "isActive": true,
            "createdAt": now,
            "updatedAt": now,
        }))
    })?;

    Ok(serde_json::json!({ "ok": true, "rule": rule }))
}

// ══════════════════════════════════════════════════════════════════════════
// rule_update
// ══════════════════════════════════════════════════════════════════════════

fn rule_update(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &RuleUpdateInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let now = shanghai_now();

    // Ensure rule exists
    let existing: Option<(i64, i64, i64, i64, i64)> = conn.query_row(
        "SELECT id, max_days_ahead, max_concurrent_per_customer, auto_release_minutes, is_active FROM reservation_rules WHERE tenant_id = ?1 AND is_active = 1 LIMIT 1",
        params![scope.tenant_id().as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).ok();

    if let Some((_id, mda, mcc, arm, ia)) = existing {
        let new_mda = input.max_days_ahead.unwrap_or(mda);
        let new_mcc = input.max_concurrent_per_customer.unwrap_or(mcc);
        let new_arm = input.auto_release_minutes.unwrap_or(arm);
        let new_ia = match input.is_active {
            Some(true) => 1,
            Some(false) => 0,
            None => ia,
        };

        conn.execute(
            "UPDATE reservation_rules SET max_days_ahead = ?1, max_concurrent_per_customer = ?2, auto_release_minutes = ?3, is_active = ?4, updated_at = ?5 WHERE tenant_id = ?6 AND is_active = 1",
            params![new_mda, new_mcc, new_arm, new_ia, now, scope.tenant_id().as_str()],
        ).map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_UPDATE", e.to_string())
        })?;
    } else {
        // No active rule — insert new
        let new_mda = input.max_days_ahead.unwrap_or(90);
        let new_mcc = input.max_concurrent_per_customer.unwrap_or(2);
        let new_arm = input.auto_release_minutes.unwrap_or(30);
        let new_ia = match input.is_active {
            Some(false) => 0,
            _ => 1,
        };

        conn.execute(
            "INSERT INTO reservation_rules (rule_name, max_days_ahead, max_concurrent_per_customer, auto_release_minutes, is_active, created_at, updated_at, tenant_id)
             VALUES ('default', ?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            params![new_mda, new_mcc, new_arm, new_ia, now, scope.tenant_id().as_str()],
        ).map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_INSERT", e.to_string())
        })?;
    }

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Read back the updated rule
    rule_get(conn, scope, &RuleGetInput {})
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(
            TenantId::new(tenant).expect("tenant"),
            Revision::new("r1").expect("revision"),
        )
        .expect("scope")
    }

    #[test]
    fn reservations_and_rules_are_isolated_between_tenants() {
        let conn = rusqlite::Connection::open_in_memory().expect("database");
        conn.execute_batch(
            "CREATE TABLE inventory_reservations (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               device_serial_no TEXT NOT NULL,
               warehouse_id INTEGER,
               order_id INTEGER,
               customer_name TEXT NOT NULL,
               customer_phone TEXT NOT NULL,
               start_date TEXT NOT NULL,
               end_date TEXT NOT NULL,
               status TEXT NOT NULL,
               notes TEXT,
               reserved_by INTEGER NOT NULL,
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL,
               tenant_id TEXT NOT NULL
             );
             CREATE TABLE reservation_rules (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               rule_name TEXT NOT NULL,
               max_days_ahead INTEGER NOT NULL,
               max_concurrent_per_customer INTEGER NOT NULL,
               auto_release_minutes INTEGER NOT NULL,
               is_active INTEGER NOT NULL,
               created_at TEXT NOT NULL,
               updated_at TEXT NOT NULL,
               tenant_id TEXT NOT NULL UNIQUE
             );",
        )
        .expect("schema");

        let tenant_a = scope("tenant-a");
        let tenant_b = scope("tenant-b");
        let input = ReserveInput {
            device_serial_no: "DEVICE-1".into(),
            warehouse_id: None,
            order_id: None,
            customer_name: "Customer".into(),
            customer_phone: "13800000000".into(),
            start_date: "2026-07-16".into(),
            end_date: "2026-07-18".into(),
            notes: None,
        };

        let created_a = reserve(&conn, &tenant_a, &input, 1).expect("tenant A reserve");
        let created_b = reserve(&conn, &tenant_b, &input, 2).expect("tenant B reserve");
        let id_a = created_a["id"].as_i64().unwrap();
        let id_b = created_b["id"].as_i64().unwrap();

        let list_input = ReservationListInput {
            status: None,
            device_serial_no: None,
            customer_phone: None,
            warehouse_id: None,
            page: None,
            page_size: None,
        };
        let list_a = list(&conn, &tenant_a, &list_input).expect("tenant A list");
        let list_b = list(&conn, &tenant_b, &list_input).expect("tenant B list");
        assert_eq!(list_a["total"], 1);
        assert_eq!(list_b["total"], 1);
        assert_eq!(list_a["items"][0]["id"], id_a);
        assert_eq!(list_b["items"][0]["id"], id_b);

        assert!(release(&conn, &tenant_b, &ReleaseInput { id: id_a }).is_err());
        assert_eq!(
            get(&conn, &tenant_a, &ReservationGetInput { id: id_a }).expect("tenant A get")["record"]
                ["status"],
            "reserved"
        );
        assert!(get(&conn, &tenant_a, &ReservationGetInput { id: id_b }).is_err());

        rule_update(
            &conn,
            &tenant_a,
            &RuleUpdateInput {
                max_days_ahead: None,
                max_concurrent_per_customer: Some(7),
                auto_release_minutes: None,
                is_active: None,
            },
        )
        .expect("tenant A rule");
        let rule_a = rule_get(&conn, &tenant_a, &RuleGetInput {}).expect("rule A");
        let rule_b = rule_get(&conn, &tenant_b, &RuleGetInput {}).expect("rule B");
        assert_eq!(rule_a["rule"]["maxConcurrentPerCustomer"], 7);
        assert_eq!(rule_b["rule"]["maxConcurrentPerCustomer"], 2);
    }
}

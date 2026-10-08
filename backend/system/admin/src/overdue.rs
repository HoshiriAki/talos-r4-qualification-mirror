//! feature-overdue — 逾期处理系统模块（Maxwell 原生）
//!
//! 逾期检测、费用计算、升级通知、豁免、统计。
//!
//! ## 命令
//! - `overdue_detect` — 扫描到期未归还订单，创建/更新逾期记录（AdminUser）
//! - `overdue_calc` — 计算单个订单逾期费（AuthUser）
//! - `overdue_apply` — 应用逾期费到订单（AuthUser）
//! - `overdue_waive` — 豁免逾期费（AdminUser）
//! - `overdue_list` — 分页列表（AuthUser）
//! - `overdue_get` — 单条查询（AuthUser）
//! - `overdue_config_get` — 获取逾期费率配置（AuthUser）
//! - `overdue_config_upsert` — 更新配置（AdminUser）
//! - `overdue_escalate` — 检查并升级逾期状态（AdminUser）
//! - `overdue_escalation_history` — 获取升级历史（AuthUser）
//! - `overdue_stats` — 逾期统计（AuthUser）
//! - `overdue_check_before_order` — 下单前检查（AuthUser）

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

// ══════════════════════════════════════════════════════════════════════════
// Helpers
// ══════════════════════════════════════════════════════════════════════════

fn shanghai_now() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn shanghai_today() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y-%m-%d").to_string()
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

fn days_between(earlier: &str, later: &str) -> i64 {
    let a = chrono::NaiveDate::parse_from_str(earlier, "%Y-%m-%d");
    let b = chrono::NaiveDate::parse_from_str(later, "%Y-%m-%d");
    match (a, b) {
        (Ok(a), Ok(b)) => (b - a).num_days(),
        _ => 0,
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Detect
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverdueDetectInput {}

impl Validate for OverdueDetectInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for OverdueDetectInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Calc
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueCalcInput {
    pub order_id: i64,
}

impl Validate for OverdueCalcInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
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

impl Sanitize for OverdueCalcInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Apply
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueApplyInput {
    pub overdue_id: i64,
}

impl Validate for OverdueApplyInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.overdue_id <= 0 {
            errors.push(FieldError {
                field: "overdueId".into(),
                message: "逾期记录ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for OverdueApplyInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Waive
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueWaiveInput {
    pub overdue_id: i64,
    pub waived_reason: String,
}

impl Validate for OverdueWaiveInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.overdue_id <= 0 {
            errors.push(FieldError {
                field: "overdueId".into(),
                message: "逾期记录ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.waived_reason.is_empty() {
            errors.push(FieldError {
                field: "waivedReason".into(),
                message: "豁免原因不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for OverdueWaiveInput {
    fn sanitize(&mut self) {
        self.waived_reason = self.waived_reason.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — List
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueListInput {
    pub status: Option<String>,
    pub customer_phone: Option<String>,
    pub order_id: Option<i64>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl Validate for OverdueListInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for OverdueListInput {
    fn sanitize(&mut self) {
        if let Some(ref mut s) = self.status {
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
pub struct OverdueGetInput {
    pub overdue_id: i64,
}

impl Validate for OverdueGetInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.overdue_id <= 0 {
            errors.push(FieldError {
                field: "overdueId".into(),
                message: "逾期记录ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for OverdueGetInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Config Get (empty)
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverdueConfigGetInput {}

impl Validate for OverdueConfigGetInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for OverdueConfigGetInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Config Upsert
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueConfigUpsertInput {
    pub daily_rate: Option<f64>,
    pub max_days: Option<i64>,
    pub cap_multiplier: Option<f64>,
    pub grace_period_hours: Option<i64>,
}

impl Validate for OverdueConfigUpsertInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if let Some(r) = self.daily_rate
            && (!r.is_finite() || r < 0.0)
        {
            errors.push(FieldError {
                field: "dailyRate".into(),
                message: "日费率必须 >= 0".into(),
                code: "VAL_INVALID".into(),
            });
        }
        if let Some(d) = self.max_days
            && !(1..=365).contains(&d)
        {
            errors.push(FieldError {
                field: "maxDays".into(),
                message: "最大天数必须在 1-365 之间".into(),
                code: "VAL_INVALID".into(),
            });
        }
        if let Some(c) = self.cap_multiplier
            && (!c.is_finite() || !(1.0..=10.0).contains(&c))
        {
            errors.push(FieldError {
                field: "capMultiplier".into(),
                message: "上限乘数必须在 1.0-10.0 之间".into(),
                code: "VAL_INVALID".into(),
            });
        }
        if let Some(g) = self.grace_period_hours
            && !(0..=168).contains(&g)
        {
            errors.push(FieldError {
                field: "gracePeriodHours".into(),
                message: "宽限期必须在 0-168 小时之间".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for OverdueConfigUpsertInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Escalate
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverdueEscalateInput {}

impl Validate for OverdueEscalateInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for OverdueEscalateInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Escalation History
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueEscalationHistoryInput {
    pub overdue_id: i64,
}

impl Validate for OverdueEscalationHistoryInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.overdue_id <= 0 {
            errors.push(FieldError {
                field: "overdueId".into(),
                message: "逾期记录ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for OverdueEscalationHistoryInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Stats (empty)
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverdueStatsInput {}

impl Validate for OverdueStatsInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for OverdueStatsInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Check Before Order
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueCheckBeforeOrderInput {
    pub customer_phone: String,
}

impl Validate for OverdueCheckBeforeOrderInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
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

impl Sanitize for OverdueCheckBeforeOrderInput {
    fn sanitize(&mut self) {
        self.customer_phone = self.customer_phone.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureOverdue {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureOverdue {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureOverdue {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureOverdue {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "overdue".into(),
            version: "0.1.0".into(),
            description: "逾期处理系统 — 检测/计算/豁免/升级通知/统计".into(),
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
                "FeatureOverdue not initialized".into(),
            )
        })?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;
        let scope = ctx.data_scope();

        match command {
            "overdue_detect" => {
                require_admin(ctx)?;
                let unvalidated: Unvalidated<OverdueDetectInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                overdue_detect(&conn, scope, &validated.into_inner())
            }
            "overdue_calc" => {
                let unvalidated: Unvalidated<OverdueCalcInput> = payload.try_into()?;
                overdue_calc(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "overdue_apply" => {
                let unvalidated: Unvalidated<OverdueApplyInput> = payload.try_into()?;
                overdue_apply(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "overdue_waive" => {
                require_admin(ctx)?;
                let unvalidated: Unvalidated<OverdueWaiveInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let admin_id = ctx
                    .user_id()
                    .and_then(|id| id.parse::<i64>().ok())
                    .ok_or_else(|| err("auth", "AUTH_REQUIRED", "需要管理员身份".into()))?;
                overdue_waive(&conn, scope, &input, admin_id)
            }
            "overdue_list" => {
                let unvalidated: Unvalidated<OverdueListInput> = payload.try_into()?;
                overdue_list(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "overdue_get" => {
                let unvalidated: Unvalidated<OverdueGetInput> = payload.try_into()?;
                overdue_get(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "overdue_config_get" => {
                let unvalidated: Unvalidated<OverdueConfigGetInput> = payload.try_into()?;
                overdue_config_get(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "overdue_config_upsert" => {
                require_admin(ctx)?;
                let unvalidated: Unvalidated<OverdueConfigUpsertInput> = payload.try_into()?;
                overdue_config_upsert(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "overdue_escalate" => {
                require_admin(ctx)?;
                let unvalidated: Unvalidated<OverdueEscalateInput> = payload.try_into()?;
                overdue_escalate(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "overdue_escalation_history" => {
                let unvalidated: Unvalidated<OverdueEscalationHistoryInput> = payload.try_into()?;
                overdue_escalation_history(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "overdue_stats" => {
                let unvalidated: Unvalidated<OverdueStatsInput> = payload.try_into()?;
                overdue_stats(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "overdue_check_before_order" => {
                let unvalidated: Unvalidated<OverdueCheckBeforeOrderInput> = payload.try_into()?;
                overdue_check_before_order(
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
                "overdue_detect",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_calc",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_apply",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_waive",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_list",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_get",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_config_get",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_config_upsert",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_escalate",
                system_core::AccessRequirement::TenantAdmin,
                &[
                    system_core::EffectClass::DatabaseWrite,
                    system_core::EffectClass::Notification,
                ],
                system_core::SimulationSupport::Blocked,
            ),
            system_core::CommandMetadata::new(
                "overdue_escalation_history",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_stats",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "overdue_check_before_order",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "overdue".into(),
            description: "逾期处理系统模块".into(),
            commands: vec![
                CommandSchema {
                    name: "overdue_detect".into(),
                    description: "扫描逾期订单".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_calc".into(),
                    description: "计算逾期费".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_apply".into(),
                    description: "应用逾期费".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_waive".into(),
                    description: "豁免逾期费".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_list".into(),
                    description: "逾期记录列表".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_get".into(),
                    description: "查看逾期记录".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_config_get".into(),
                    description: "获取逾期配置".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_config_upsert".into(),
                    description: "更新逾期配置".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_escalate".into(),
                    description: "升级逾期通知".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_escalation_history".into(),
                    description: "升级历史".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_stats".into(),
                    description: "逾期统计".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "overdue_check_before_order".into(),
                    description: "下单前检查".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_detect — 扫描到期未归还的订单
// ══════════════════════════════════════════════════════════════════════════

fn overdue_detect(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    _input: &OverdueDetectInput,
) -> Result<Value, String> {
    let today = shanghai_today();

    // Find orders past expected_return_date that are still active
    let mut stmt = conn
        .prepare(
            "SELECT id, customer_name, customer_phone, expected_return_date
         FROM orders
         WHERE tenant_id = ?1 AND expected_return_date < ?2
           AND expected_return_date != ''
           AND status IN ('in_use', 'shipped')
         ORDER BY expected_return_date ASC",
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let overdue_orders: Vec<(i64, String, String, String)> = stmt
        .query_map(params![scope.tenant_id().as_str(), today], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    // Get config for daily_rate
    let config = get_fee_config(conn, scope)?;
    let mut detected: Vec<Value> = Vec::new();
    let now = shanghai_now();

    for (order_id, customer_name, customer_phone, expected_return_date) in &overdue_orders {
        conn.execute("BEGIN IMMEDIATE", [])
            .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

        let days = days_between(expected_return_date, &today);

        // Check if an active overdue record already exists for this order
        let existing: Option<(i64, i64, String)> = conn.query_row(
            "SELECT id, days_overdue, status FROM overdue_records WHERE tenant_id = ?1 AND order_id = ?2 AND status IN ('active', 'escalated_d1', 'escalated_d3', 'escalated_d7') LIMIT 1",
            params![scope.tenant_id().as_str(), order_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).ok();

        if let Some((rec_id, _old_days, existing_status)) = existing {
            // Update existing record
            let cap_days = (config.max_days as f64 * config.cap_multiplier) as i64;
            let capped_days = days.min(cap_days);
            let total_fee = (capped_days as f64 * config.daily_rate).max(0.0);

            conn.execute(
                "UPDATE overdue_records SET days_overdue = ?1, total_fee = ?2, updated_at = ?3 WHERE tenant_id = ?4 AND id = ?5",
                params![capped_days, total_fee, now, scope.tenant_id().as_str(), rec_id],
            ).map_err(|e| {
                let _ = conn.execute("ROLLBACK", []);
                err("db", "DB_UPDATE", e.to_string())
            })?;

            conn.execute("COMMIT", [])
                .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

            // Check if we need to auto-escalate
            let should_escalate =
                existing_status == "active" && days >= 1 && config.grace_period_hours == 0;

            if should_escalate {
                let _ = try_escalate(conn, scope, rec_id, &config, days);
            }

            detected.push(serde_json::json!({
                "orderId": order_id,
                "customerName": customer_name,
                "customerPhone": customer_phone,
                "expectedReturnDate": expected_return_date,
                "daysOverdue": capped_days,
                "totalFee": total_fee,
                "action": "updated"
            }));
        } else {
            // Create new record
            let cap_days = (config.max_days as f64 * config.cap_multiplier) as i64;
            let capped_days = days.min(cap_days);
            let total_fee = (capped_days as f64 * config.daily_rate).max(0.0);

            conn.execute(
                "INSERT INTO overdue_records (tenant_id, order_id, customer_name, customer_phone, expected_return_date, days_overdue, daily_rate, total_fee, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?9)",
                params![scope.tenant_id().as_str(), order_id, customer_name, customer_phone, expected_return_date, capped_days, config.daily_rate, total_fee, now],
            ).map_err(|e| {
                let _ = conn.execute("ROLLBACK", []);
                err("db", "DB_INSERT", e.to_string())
            })?;

            let new_id = conn.last_insert_rowid();
            conn.execute("COMMIT", [])
                .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

            // Auto-escalate if past grace period
            if days >= 1 && config.grace_period_hours == 0 {
                let _ = try_escalate(conn, scope, new_id, &config, days);
            }

            detected.push(serde_json::json!({
                "orderId": order_id,
                "customerName": customer_name,
                "customerPhone": customer_phone,
                "expectedReturnDate": expected_return_date,
                "daysOverdue": capped_days,
                "totalFee": total_fee,
                "action": "created",
                "id": new_id
            }));
        }
    }

    Ok(serde_json::json!({
        "ok": true,
        "detected": detected.len(),
        "records": detected,
        "scannedAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_calc — 计算单个订单的逾期费用
// ══════════════════════════════════════════════════════════════════════════

fn overdue_calc(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &OverdueCalcInput,
) -> Result<Value, String> {
    let today = shanghai_today();
    let config = get_fee_config(conn, scope)?;

    // Get order info
    let order = conn
        .query_row(
            "SELECT id, customer_name, customer_phone, expected_return_date, status
         FROM orders WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.order_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let expected_date = &order.3;
    if expected_date.is_empty() {
        return Ok(serde_json::json!({
            "ok": true,
            "daysOverdue": 0,
            "dailyRate": config.daily_rate,
            "totalFee": 0.0,
            "capAmount": 0.0,
            "actualFee": 0.0,
            "orderStatus": order.4,
        }));
    }

    let days = days_between(expected_date, &today);
    let raw_fee = days as f64 * config.daily_rate;
    let cap_amount = config.daily_rate * (config.max_days as f64) * config.cap_multiplier;
    let actual_fee = raw_fee.min(cap_amount).max(0.0);

    Ok(serde_json::json!({
        "ok": true,
        "daysOverdue": days.max(0),
        "dailyRate": config.daily_rate,
        "totalFee": raw_fee,
        "capAmount": cap_amount,
        "actualFee": actual_fee,
        "orderStatus": order.4,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_apply — 将逾期费写入 order_price_details
// ══════════════════════════════════════════════════════════════════════════

fn overdue_apply(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &OverdueApplyInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Get overdue record
    let record = conn
        .query_row(
            "SELECT id, order_id, total_fee, waived_amount, days_overdue, status
         FROM overdue_records WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.overdue_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if record.5 == "waived" || record.5 == "paid" || record.5 == "cancelled" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_STATUS_INVALID",
            format!("当前状态 '{}' 不可应用费用", record.5),
        ));
    }

    let net_fee = (record.2 - record.3).max(0.0);
    let now = shanghai_now();

    // Insert into order_price_details
    conn.execute(
        "INSERT INTO order_price_details (order_id, item_type, item_name, amount, quantity, created_at, updated_at, tenant_id)
         VALUES (?1, 'overdue', '逾期费用', ?2, 1, ?3, ?3, ?4)",
        params![record.1, net_fee, now, scope.tenant_id().as_str()],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    // Update overdue record
    conn.execute(
        "UPDATE overdue_records SET paid_amount = paid_amount + ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4",
        params![net_fee, now, scope.tenant_id().as_str(), input.overdue_id],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    // Check if fully paid
    let (total, paid): (f64, f64) = conn
        .query_row(
            "SELECT total_fee, paid_amount FROM overdue_records WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.overdue_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if paid >= total {
        conn.execute(
            "UPDATE overdue_records SET status = 'paid', updated_at = ?1 WHERE tenant_id = ?2 AND id = ?3",
            params![now, scope.tenant_id().as_str(), input.overdue_id],
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_UPDATE", e.to_string())
        })?;
    }

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true,
        "overdueId": input.overdue_id,
        "orderId": record.1,
        "appliedAmount": net_fee,
        "appliedAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_waive — 豁免逾期费
// ══════════════════════════════════════════════════════════════════════════

fn overdue_waive(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &OverdueWaiveInput,
    admin_id: i64,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Check status
    let status: String = conn
        .query_row(
            "SELECT status FROM overdue_records WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.overdue_id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if status == "waived" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_ALREADY_WAIVED",
            "该逾期记录已被豁免".into(),
        ));
    }
    if status == "paid" || status == "cancelled" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_STATUS_INVALID",
            format!("当前状态 '{}' 不可豁免", status),
        ));
    }

    let now = shanghai_now();
    let total_fee: f64 = conn
        .query_row(
            "SELECT total_fee FROM overdue_records WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.overdue_id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    conn.execute(
        "UPDATE overdue_records SET waived_amount = ?1, status = 'waived', waived_by = ?2, waived_reason = ?3, updated_at = ?4 WHERE tenant_id = ?5 AND id = ?6",
        params![total_fee, admin_id, input.waived_reason, now, scope.tenant_id().as_str(), input.overdue_id],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true,
        "overdueId": input.overdue_id,
        "waivedAmount": total_fee,
        "waivedBy": admin_id,
        "waivedAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_list — 分页列表，参数化查询防注入
// ══════════════════════════════════════════════════════════════════════════

fn overdue_list(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &OverdueListInput,
) -> Result<Value, String> {
    let page = input.page.unwrap_or(1).max(1);
    let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let mut conditions: Vec<String> = vec!["tenant_id = ?1".into()];
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(scope.tenant_id().as_str().to_owned())];

    if let Some(ref status) = input.status {
        let idx = conditions.len() + 1;
        conditions.push(format!("status = ?{}", idx));
        all_params.push(Box::new(status.clone()));
    }
    if let Some(ref phone) = input.customer_phone {
        let idx = conditions.len() + 1;
        conditions.push(format!("customer_phone = ?{}", idx));
        all_params.push(Box::new(phone.clone()));
    }
    if let Some(order_id) = input.order_id {
        let idx = conditions.len() + 1;
        conditions.push(format!("order_id = ?{}", idx));
        all_params.push(Box::new(order_id));
    }

    let where_sql = format!("WHERE {}", conditions.join(" AND "));

    // Count
    let count_sql = format!("SELECT COUNT(*) FROM overdue_records {}", where_sql);
    let total: i64 = {
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            all_params.iter().map(|p| p.as_ref()).collect();
        conn.query_row(&count_sql, rusqlite::params_from_iter(param_refs), |row| {
            row.get(0)
        })
    }
    .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    // Query
    let query_sql = format!(
        "SELECT id, order_id, customer_name, customer_phone, expected_return_date, actual_return_date, days_overdue, daily_rate, total_fee, waived_amount, paid_amount, status, escalation_level, last_escalation_at, waived_by, waived_reason, created_at, updated_at
         FROM overdue_records {} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
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
                "orderId": row.get::<_, i64>(1)?,
                "customerName": row.get::<_, String>(2)?,
                "customerPhone": row.get::<_, String>(3)?,
                "expectedReturnDate": row.get::<_, String>(4)?,
                "actualReturnDate": row.get::<_, Option<String>>(5)?,
                "daysOverdue": row.get::<_, i64>(6)?,
                "dailyRate": row.get::<_, f64>(7)?,
                "totalFee": row.get::<_, f64>(8)?,
                "waivedAmount": row.get::<_, f64>(9)?,
                "paidAmount": row.get::<_, f64>(10)?,
                "status": row.get::<_, String>(11)?,
                "escalationLevel": row.get::<_, i64>(12)?,
                "lastEscalationAt": row.get::<_, Option<String>>(13)?,
                "waivedBy": row.get::<_, Option<i64>>(14)?,
                "waivedReason": row.get::<_, Option<String>>(15)?,
                "createdAt": row.get::<_, String>(16)?,
                "updatedAt": row.get::<_, String>(17)?,
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
// overdue_get — 单条查询
// ══════════════════════════════════════════════════════════════════════════

fn overdue_get(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &OverdueGetInput,
) -> Result<Value, String> {
    let record = conn.query_row(
        "SELECT id, order_id, customer_name, customer_phone, expected_return_date, actual_return_date, days_overdue, daily_rate, total_fee, waived_amount, paid_amount, status, escalation_level, last_escalation_at, waived_by, waived_reason, created_at, updated_at
         FROM overdue_records WHERE tenant_id = ?1 AND id = ?2",
        params![scope.tenant_id().as_str(), input.overdue_id],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "orderId": row.get::<_, i64>(1)?,
                "customerName": row.get::<_, String>(2)?,
                "customerPhone": row.get::<_, String>(3)?,
                "expectedReturnDate": row.get::<_, String>(4)?,
                "actualReturnDate": row.get::<_, Option<String>>(5)?,
                "daysOverdue": row.get::<_, i64>(6)?,
                "dailyRate": row.get::<_, f64>(7)?,
                "totalFee": row.get::<_, f64>(8)?,
                "waivedAmount": row.get::<_, f64>(9)?,
                "paidAmount": row.get::<_, f64>(10)?,
                "status": row.get::<_, String>(11)?,
                "escalationLevel": row.get::<_, i64>(12)?,
                "lastEscalationAt": row.get::<_, Option<String>>(13)?,
                "waivedBy": row.get::<_, Option<i64>>(14)?,
                "waivedReason": row.get::<_, Option<String>>(15)?,
                "createdAt": row.get::<_, String>(16)?,
                "updatedAt": row.get::<_, String>(17)?,
            }))
        },
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "record": record }))
}

// ══════════════════════════════════════════════════════════════════════════
// Fee config helper
// ══════════════════════════════════════════════════════════════════════════

struct FeeConfig {
    daily_rate: f64,
    max_days: i64,
    cap_multiplier: f64,
    grace_period_hours: i64,
}

fn get_fee_config(conn: &rusqlite::Connection, scope: &DataScope) -> Result<FeeConfig, String> {
    conn.query_row(
        "SELECT id, daily_rate, max_days, cap_multiplier, grace_period_hours FROM overdue_fee_config WHERE tenant_id = ?1 LIMIT 1",
        params![scope.tenant_id().as_str()],
        |row| Ok(FeeConfig {
            daily_rate: row.get(1)?,
            max_days: row.get(2)?,
            cap_multiplier: row.get(3)?,
            grace_period_hours: row.get(4)?,
        }),
    ).or_else(|_| {
        // Auto-insert defaults on first access
        let now = shanghai_now();
        conn.execute(
            "INSERT INTO overdue_fee_config (tenant_id, daily_rate, max_days, cap_multiplier, grace_period_hours, created_at, updated_at)
             VALUES (?1, 50.0, 30, 3.0, 4, ?2, ?2)",
            params![scope.tenant_id().as_str(), now],
        ).map_err(|e| err("db", "DB_INSERT", e.to_string()))?;
        Ok(FeeConfig {
            daily_rate: 50.0,
            max_days: 30,
            cap_multiplier: 3.0,
            grace_period_hours: 4,
        })
    })
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_config_get
// ══════════════════════════════════════════════════════════════════════════

fn overdue_config_get(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    _input: &OverdueConfigGetInput,
) -> Result<Value, String> {
    let config = get_fee_config(conn, scope)?;
    Ok(serde_json::json!({
        "ok": true,
        "config": {
            "dailyRate": config.daily_rate,
            "maxDays": config.max_days,
            "capMultiplier": config.cap_multiplier,
            "gracePeriodHours": config.grace_period_hours,
        },
        "formula": "min(days * dailyRate, dailyRate * maxDays * capMultiplier)",
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_config_upsert
// ══════════════════════════════════════════════════════════════════════════

fn overdue_config_upsert(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &OverdueConfigUpsertInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let now = shanghai_now();

    // Read existing or create defaults
    let existing: Option<(f64, i64, f64, i64)> = conn.query_row(
        "SELECT daily_rate, max_days, cap_multiplier, grace_period_hours FROM overdue_fee_config WHERE tenant_id = ?1 LIMIT 1",
        params![scope.tenant_id().as_str()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).ok();

    if let Some((dr, md, cm, gph)) = existing {
        let new_dr = input.daily_rate.unwrap_or(dr);
        let new_md = input.max_days.unwrap_or(md);
        let new_cm = input.cap_multiplier.unwrap_or(cm);
        let new_gph = input.grace_period_hours.unwrap_or(gph);

        conn.execute(
            "UPDATE overdue_fee_config SET daily_rate = ?1, max_days = ?2, cap_multiplier = ?3, grace_period_hours = ?4, updated_at = ?5 WHERE tenant_id = ?6",
            params![new_dr, new_md, new_cm, new_gph, now, scope.tenant_id().as_str()],
        ).map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_UPDATE", e.to_string())
        })?;

        conn.execute("COMMIT", [])
            .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

        Ok(serde_json::json!({
            "ok": true,
            "config": {
                "dailyRate": new_dr,
                "maxDays": new_md,
                "capMultiplier": new_cm,
                "gracePeriodHours": new_gph,
            },
            "updatedAt": now,
        }))
    } else {
        // No existing row — insert
        let new_dr = input.daily_rate.unwrap_or(50.0);
        let new_md = input.max_days.unwrap_or(30);
        let new_cm = input.cap_multiplier.unwrap_or(3.0);
        let new_gph = input.grace_period_hours.unwrap_or(4);

        conn.execute(
            "INSERT INTO overdue_fee_config (tenant_id, daily_rate, max_days, cap_multiplier, grace_period_hours, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![scope.tenant_id().as_str(), new_dr, new_md, new_cm, new_gph, now],
        ).map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_INSERT", e.to_string())
        })?;

        conn.execute("COMMIT", [])
            .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

        Ok(serde_json::json!({
            "ok": true,
            "config": {
                "dailyRate": new_dr,
                "maxDays": new_md,
                "capMultiplier": new_cm,
                "gracePeriodHours": new_gph,
            },
            "createdAt": now,
        }))
    }
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_escalate — D+1 SMS, D+3 email, D+7 legal notice
// ══════════════════════════════════════════════════════════════════════════

fn overdue_escalate(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    _input: &OverdueEscalateInput,
) -> Result<Value, String> {
    let config = get_fee_config(conn, scope)?;

    // Find all active overdue records
    let mut stmt = conn.prepare(
        "SELECT id, order_id, customer_name, customer_phone, expected_return_date, days_overdue, status, escalation_level
         FROM overdue_records WHERE tenant_id = ?1 AND status IN ('active', 'escalated_d1', 'escalated_d3')"
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    #[allow(clippy::type_complexity)]
    let records: Vec<(i64, i64, String, String, String, i64, String, i64)> = stmt
        .query_map(params![scope.tenant_id().as_str()], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    let mut results: Vec<Value> = Vec::new();

    for (
        rec_id,
        order_id,
        customer_name,
        customer_phone,
        expected_date,
        days_overdue,
        status,
        esc_level,
    ) in &records
    {
        let result = try_escalate_record(
            conn,
            scope,
            *rec_id,
            *order_id,
            customer_name,
            customer_phone,
            expected_date,
            *days_overdue,
            status,
            *esc_level,
            &config,
        )?;
        results.push(result);
    }

    Ok(serde_json::json!({
        "ok": true,
        "escalated": results.iter().filter(|r| r["action"] != "none").count(),
        "checked": results.len(),
        "results": results,
    }))
}

#[allow(clippy::too_many_arguments)]
fn try_escalate_record(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    rec_id: i64,
    order_id: i64,
    _customer_name: &str,
    _customer_phone: &str,
    _expected_date: &str,
    days_overdue: i64,
    _status: &str,
    esc_level: i64,
    _config: &FeeConfig,
) -> Result<Value, String> {
    let (new_level, new_status, channel) = if days_overdue >= 7 && esc_level < 3 {
        (3, "escalated_d7", "legal_notice")
    } else if days_overdue >= 3 && esc_level < 2 {
        (2, "escalated_d3", "email")
    } else if days_overdue >= 1 && esc_level < 1 {
        (1, "escalated_d1", "sms")
    } else {
        // Not due for escalation yet
        return Ok(serde_json::json!({
            "overdueId": rec_id,
            "orderId": order_id,
            "daysOverdue": days_overdue,
            "previousLevel": esc_level,
            "action": "none",
        }));
    };

    // Anti-duplicate: check within transaction to prevent TOCTOU
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let already_sent: Option<i64> = conn.query_row(
        "SELECT id FROM overdue_notification_log WHERE tenant_id = ?1 AND overdue_id = ?2 AND escalation_level = ?3 LIMIT 1",
        params![scope.tenant_id().as_str(), rec_id, new_level],
        |row| row.get(0),
    ).ok();

    if already_sent.is_some() {
        let _ = conn.execute("ROLLBACK", []);
        return Ok(serde_json::json!({
            "overdueId": rec_id,
            "orderId": order_id,
            "daysOverdue": days_overdue,
            "previousLevel": esc_level,
            "action": "skipped_duplicate",
            "targetLevel": new_level,
        }));
    }

    let now = shanghai_now();

    // Update overdue record
    conn.execute(
        "UPDATE overdue_records SET status = ?1, escalation_level = ?2, last_escalation_at = ?3, updated_at = ?3 WHERE tenant_id = ?4 AND id = ?5",
        params![new_status, new_level, now, scope.tenant_id().as_str(), rec_id],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    // Log notification
    conn.execute(
        "INSERT INTO overdue_notification_log (tenant_id, overdue_id, escalation_level, channel, sent_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![scope.tenant_id().as_str(), rec_id, new_level, channel, now],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Stub notification via tracing (no PII in logs)
    tracing::info!(
        target: "overdue",
        order_id = order_id,
        escalation_level = new_level,
        channel = channel,
        days_overdue = days_overdue,
        "[STUB] 订单 #{} 逾期 {} 天，升级到 level {} ({} 通知)",
        order_id, days_overdue, new_level, channel
    );

    Ok(serde_json::json!({
        "overdueId": rec_id,
        "orderId": order_id,
        "daysOverdue": days_overdue,
        "previousLevel": esc_level,
        "newLevel": new_level,
        "newStatus": new_status,
        "channel": channel,
        "action": "escalated",
        "escalatedAt": now,
    }))
}

// Re-usable escalate helper used by overdue_detect
fn try_escalate(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    rec_id: i64,
    config: &FeeConfig,
    days_overdue: i64,
) -> Result<(), String> {
    // Get record details
    let rec = conn.query_row(
        "SELECT order_id, customer_name, customer_phone, expected_return_date, status, escalation_level
         FROM overdue_records WHERE tenant_id = ?1 AND id = ?2",
        params![scope.tenant_id().as_str(), rec_id],
        |row| Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
        )),
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    try_escalate_record(
        conn,
        scope,
        rec_id,
        rec.0,
        &rec.1,
        &rec.2,
        &rec.3,
        days_overdue,
        &rec.4,
        rec.5,
        config,
    )?;

    Ok(())
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_escalation_history
// ══════════════════════════════════════════════════════════════════════════

fn overdue_escalation_history(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &OverdueEscalationHistoryInput,
) -> Result<Value, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, overdue_id, escalation_level, channel, sent_at
         FROM overdue_notification_log WHERE tenant_id = ?1 AND overdue_id = ?2 ORDER BY escalation_level ASC",
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let history: Vec<Value> = stmt
        .query_map(
            params![scope.tenant_id().as_str(), input.overdue_id],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "overdueId": row.get::<_, i64>(1)?,
                    "escalationLevel": row.get::<_, i64>(2)?,
                    "channel": row.get::<_, String>(3)?,
                    "sentAt": row.get::<_, String>(4)?,
                }))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::json!({ "ok": true, "history": history }))
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_stats
// ══════════════════════════════════════════════════════════════════════════

fn overdue_stats(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    _input: &OverdueStatsInput,
) -> Result<Value, String> {
    // Total active and amount
    let active_row = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(total_fee - waived_amount - paid_amount), 0)
         FROM overdue_records WHERE tenant_id = ?1 AND status IN ('active', 'escalated_d1', 'escalated_d3', 'escalated_d7')",
        params![scope.tenant_id().as_str()],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)),
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    // By escalation level
    let mut stmt = conn.prepare(
        "SELECT escalation_level, COUNT(*), COALESCE(SUM(total_fee - waived_amount - paid_amount), 0)
         FROM overdue_records WHERE tenant_id = ?1 AND status IN ('active', 'escalated_d1', 'escalated_d3', 'escalated_d7')
         GROUP BY escalation_level"
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let by_level: Vec<Value> = stmt
        .query_map(params![scope.tenant_id().as_str()], |row| {
            Ok(serde_json::json!({
                "escalationLevel": row.get::<_, i64>(0)?,
                "count": row.get::<_, i64>(1)?,
                "totalAmount": row.get::<_, f64>(2)?,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    // Top overdue (top 5 by days)
    let mut stmt2 = conn.prepare(
        "SELECT id, order_id, customer_name, customer_phone, days_overdue, total_fee, status
         FROM overdue_records WHERE tenant_id = ?1 AND status IN ('active', 'escalated_d1', 'escalated_d3', 'escalated_d7')
         ORDER BY days_overdue DESC LIMIT 5"
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let top_overdue: Vec<Value> = stmt2
        .query_map(params![scope.tenant_id().as_str()], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "orderId": row.get::<_, i64>(1)?,
                "customerName": row.get::<_, String>(2)?,
                "customerPhone": row.get::<_, String>(3)?,
                "daysOverdue": row.get::<_, i64>(4)?,
                "totalFee": row.get::<_, f64>(5)?,
                "status": row.get::<_, String>(6)?,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    let by_escalation: serde_json::Map<String, Value> = by_level
        .iter()
        .map(|v| {
            let level = v["escalationLevel"].as_i64().unwrap_or(0);
            (format!("level_{}", level), v.clone())
        })
        .collect();

    Ok(serde_json::json!({
        "ok": true,
        "totalActive": active_row.0,
        "totalAmount": active_row.1,
        "byEscalationLevel": by_escalation,
        "topOverdue": top_overdue,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// overdue_check_before_order — 下单前检查客户是否有未处理逾期
// ══════════════════════════════════════════════════════════════════════════

fn overdue_check_before_order(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &OverdueCheckBeforeOrderInput,
) -> Result<Value, String> {
    let mut stmt = conn.prepare(
        "SELECT id, order_id, days_overdue, total_fee, status
         FROM overdue_records
         WHERE tenant_id = ?1 AND customer_phone = ?2 AND status IN ('active', 'escalated_d1', 'escalated_d3', 'escalated_d7')
         ORDER BY days_overdue DESC"
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let active_records: Vec<Value> = stmt
        .query_map(
            params![scope.tenant_id().as_str(), input.customer_phone],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "orderId": row.get::<_, i64>(1)?,
                    "daysOverdue": row.get::<_, i64>(2)?,
                    "totalFee": row.get::<_, f64>(3)?,
                    "status": row.get::<_, String>(4)?,
                }))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    let has_overdue = !active_records.is_empty();

    // Check for severe overdue (escalated_d7) which blocks ordering
    let has_severe = active_records
        .iter()
        .any(|r| r.get("status").and_then(|v| v.as_str()) == Some("escalated_d7"));

    let total_unpaid: f64 = active_records
        .iter()
        .map(|r| r.get("totalFee").and_then(|v| v.as_f64()).unwrap_or(0.0))
        .sum();

    Ok(serde_json::json!({
        "ok": true,
        "hasOverdue": has_overdue,
        "activeRecords": active_records,
        "totalUnpaid": total_unpaid,
        "blockReason": if has_severe { Some("该客户存在严重逾期记录(已进入法律告知阶段), 请先处理逾期再下单") } else { None } as Option<&str>,
        "allowed": !has_severe,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_core::{Revision, TenantId};

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(TenantId::new(tenant).unwrap(), Revision::new("r1").unwrap()).unwrap()
    }

    #[test]
    fn overdue_records_and_config_are_isolated_by_data_scope() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE overdue_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT, tenant_id TEXT NOT NULL,
                order_id INTEGER NOT NULL, customer_name TEXT NOT NULL,
                customer_phone TEXT NOT NULL, expected_return_date TEXT NOT NULL,
                actual_return_date TEXT, days_overdue INTEGER NOT NULL,
                daily_rate REAL NOT NULL, total_fee REAL NOT NULL,
                waived_amount REAL NOT NULL DEFAULT 0, paid_amount REAL NOT NULL DEFAULT 0,
                status TEXT NOT NULL, escalation_level INTEGER NOT NULL DEFAULT 0,
                last_escalation_at TEXT, waived_by INTEGER, waived_reason TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE overdue_fee_config (
                id INTEGER PRIMARY KEY AUTOINCREMENT, tenant_id TEXT,
                daily_rate REAL NOT NULL, max_days INTEGER NOT NULL,
                cap_multiplier REAL NOT NULL, grace_period_hours INTEGER NOT NULL,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            INSERT INTO overdue_records
                (tenant_id, order_id, customer_name, customer_phone, expected_return_date,
                 days_overdue, daily_rate, total_fee, status, created_at, updated_at)
            VALUES
                ('tenant-a', 1, 'A', '13800000000', '2026-01-01', 10, 50, 500, 'active', 'now', 'now'),
                ('tenant-b', 2, 'B', '13800000000', '2026-01-01', 2, 10, 20, 'active', 'now', 'now');
            INSERT INTO overdue_fee_config
                (tenant_id, daily_rate, max_days, cap_multiplier, grace_period_hours, created_at, updated_at)
            VALUES ('tenant-a', 50, 30, 3, 4, 'now', 'now'),
                   ('tenant-b', 10, 7, 1, 0, 'now', 'now');",
        )
        .unwrap();

        let a = scope("tenant-a");
        let b = scope("tenant-b");
        let list_input = OverdueListInput {
            status: None,
            customer_phone: Some("13800000000".into()),
            order_id: None,
            page: None,
            page_size: None,
        };
        assert_eq!(
            overdue_list(&conn, &a, &list_input).unwrap()["items"][0]["orderId"],
            1
        );
        assert_eq!(
            overdue_list(&conn, &b, &list_input).unwrap()["items"][0]["orderId"],
            2
        );
        assert_eq!(get_fee_config(&conn, &a).unwrap().daily_rate, 50.0);
        assert_eq!(get_fee_config(&conn, &b).unwrap().daily_rate, 10.0);

        let check = OverdueCheckBeforeOrderInput {
            customer_phone: "13800000000".into(),
        };
        let b_check = overdue_check_before_order(&conn, &b, &check).unwrap();
        assert_eq!(b_check["activeRecords"][0]["orderId"], 2);
    }
}

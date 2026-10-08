//! feature-optical-sop — 光学检测 SOP 模块（Maxwell 原生）
//!
//! 5 步归还检查清单 + 自动损伤分级 + 联动 damage_report。
//!
//! ## 命令
//! - `inspection_create` — 创建 5 步检查清单（AdminUser）
//! - `inspection_update_step` — 更新单步结果（AdminUser）
//! - `inspection_complete` — 完成检查，非 pass 自动创建 damage_report（AdminUser）
//! - `inspection_get` — 单条查询（AuthUser）
//! - `inspection_list` — 分页列表（AuthUser）
//! - `inspection_stats` — 统计数据（AuthUser）

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

/// Compute overall_grade from 5 boolean step results.
fn compute_overall_grade(
    body_ok: bool,
    lens_ok: bool,
    screen_ok: bool,
    accessory_ok: bool,
    function_ok: bool,
) -> &'static str {
    let failed = [!body_ok, !lens_ok, !screen_ok, !accessory_ok, !function_ok]
        .iter()
        .filter(|&&f| f)
        .count();

    match failed {
        0 => "pass",
        1..=2 => "minor_damage",
        3..=4 => "major_damage",
        _ => "total_loss",
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Create
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionCreateInput {
    pub order_id: i64,
    pub device_serial_no: String,
}

impl Validate for InspectionCreateInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.order_id <= 0 {
            errors.push(FieldError {
                field: "orderId".into(),
                message: "订单ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.device_serial_no.is_empty() {
            errors.push(FieldError {
                field: "deviceSerialNo".into(),
                message: "设备序列号不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for InspectionCreateInput {
    fn sanitize(&mut self) {
        self.device_serial_no = self.device_serial_no.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Update Step
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionUpdateStepInput {
    pub id: i64,
    pub step: String,
    pub ok: bool,
    pub note: Option<String>,
}

impl Validate for InspectionUpdateStepInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "检查清单ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if !matches!(
            self.step.as_str(),
            "body" | "lens" | "screen" | "accessory" | "function"
        ) {
            errors.push(FieldError {
                field: "step".into(),
                message: "步骤必须是 body / lens / screen / accessory / function".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for InspectionUpdateStepInput {
    fn sanitize(&mut self) {
        self.step = self.step.trim().to_string();
        if let Some(ref mut n) = self.note {
            *n = n.trim().to_string();
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Complete
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionCompleteInput {
    pub id: i64,
}

impl Validate for InspectionCompleteInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "检查清单ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for InspectionCompleteInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Get
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionGetInput {
    pub id: i64,
}

impl Validate for InspectionGetInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "检查清单ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for InspectionGetInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — List
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionListInput {
    pub order_id: Option<i64>,
    pub device_serial_no: Option<String>,
    pub overall_grade: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl Validate for InspectionListInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for InspectionListInput {
    fn sanitize(&mut self) {
        if let Some(ref mut d) = self.device_serial_no {
            *d = d.trim().to_string();
        }
        if let Some(ref mut g) = self.overall_grade {
            *g = g.trim().to_string();
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Stats (empty)
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionStatsInput {}

impl Validate for InspectionStatsInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for InspectionStatsInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureOpticalSop {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureOpticalSop {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureOpticalSop {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureOpticalSop {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "optical_sop".into(),
            version: "0.1.0".into(),
            description: "光学检测 SOP — 5 步归还检查清单 + 损伤分级".into(),
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
                "FeatureOpticalSop not initialized".into(),
            )
        })?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;

        match command {
            "inspection_create" => {
                require_admin(ctx)?;
                let unvalidated: Unvalidated<InspectionCreateInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let inspector_id = ctx
                    .user_id()
                    .as_ref()
                    .and_then(|id| id.parse::<i64>().ok())
                    .ok_or_else(|| err("auth", "AUTH_REQUIRED", "需要管理员身份".into()))?;
                inspection_create(&conn, ctx.data_scope(), &input, inspector_id)
            }
            "inspection_update_step" => {
                require_admin(ctx)?;
                let unvalidated: Unvalidated<InspectionUpdateStepInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                inspection_update_step(&conn, ctx.data_scope(), &validated.into_inner())
            }
            "inspection_complete" => {
                require_admin(ctx)?;
                let unvalidated: Unvalidated<InspectionCompleteInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let inspector_id = ctx
                    .user_id()
                    .as_ref()
                    .and_then(|id| id.parse::<i64>().ok())
                    .ok_or_else(|| err("auth", "AUTH_REQUIRED", "需要管理员身份".into()))?;
                inspection_complete(&conn, ctx.data_scope(), &input, inspector_id)
            }
            "inspection_get" => {
                let unvalidated: Unvalidated<InspectionGetInput> = payload.try_into()?;
                inspection_get(
                    &conn,
                    ctx.data_scope(),
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "inspection_list" => {
                let unvalidated: Unvalidated<InspectionListInput> = payload.try_into()?;
                inspection_list(
                    &conn,
                    ctx.data_scope(),
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "inspection_stats" => {
                let unvalidated: Unvalidated<InspectionStatsInput> = payload.try_into()?;
                inspection_stats(
                    &conn,
                    ctx.data_scope(),
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
                "inspection_create",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "inspection_update_step",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "inspection_complete",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "inspection_get",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "inspection_list",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "inspection_stats",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "optical_sop".into(),
            description: "光学检测 SOP 模块".into(),
            commands: vec![
                CommandSchema {
                    name: "inspection_create".into(),
                    description: "创建 5 步检查清单".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "inspection_update_step".into(),
                    description: "更新单步检查结果".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "inspection_complete".into(),
                    description: "完成检查并创建 damage_report".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "inspection_get".into(),
                    description: "查看检查清单".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "inspection_list".into(),
                    description: "检查清单列表".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "inspection_stats".into(),
                    description: "检查统计".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// inspection_create — 创建 5 步检查清单
// ══════════════════════════════════════════════════════════════════════════

fn inspection_create(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &InspectionCreateInput,
    inspector_id: i64,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Verify order status is in_use or shipped
    let order_status: String = conn
        .query_row(
            "SELECT status FROM orders WHERE id = ?1 AND tenant_id = ?2",
            params![input.order_id, scope.tenant_id().as_str()],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if order_status != "in_use" && order_status != "shipped" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_STATUS_INVALID",
            format!(
                "订单状态 '{}' 不可创建检查清单，需为 in_use 或 shipped",
                order_status
            ),
        ));
    }

    // Check for existing checklist for same order+device (avoid duplicates)
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM inspection_checklists WHERE order_id = ?1 AND device_serial_no = ?2 AND tenant_id = ?3 LIMIT 1",
            params![input.order_id, input.device_serial_no, scope.tenant_id().as_str()],
            |row| row.get(0),
        )
        .ok();

    if existing.is_some() {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err("biz", "BIZ_DUPLICATE", "该订单设备已有检查清单".into()));
    }

    let now = shanghai_now();
    let inserted = conn.execute(
        "INSERT INTO inspection_checklists (order_id, device_serial_no, inspector_id, body_ok, lens_ok, screen_ok, accessory_ok, function_ok, overall_grade, created_at, updated_at, tenant_id)
         SELECT ?1, ?2, ?3, 1, 1, 1, 1, 1, 'pass', ?4, ?4, ?5
         FROM order_devices od JOIN devices d ON d.serial_no = od.device_serial_no AND d.tenant_id = od.tenant_id
         WHERE od.order_id = ?1 AND od.device_serial_no = ?2 AND od.tenant_id = ?5",
        params![input.order_id, input.device_serial_no, inspector_id, now, scope.tenant_id().as_str()],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    if inserted == 0 {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err("biz", "BIZ_NOT_FOUND", "订单设备不属于当前租户".into()));
    }

    let id = conn.last_insert_rowid();
    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true,
        "id": id,
        "orderId": input.order_id,
        "deviceSerialNo": input.device_serial_no,
        "overallGrade": "pass",
        "createdAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// inspection_update_step — 更新单步结果并重算 overall_grade
// ══════════════════════════════════════════════════════════════════════════

fn inspection_update_step(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &InspectionUpdateStepInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Read current step values (within transaction to prevent TOCTOU)
    let (body_ok, lens_ok, screen_ok, accessory_ok, function_ok): (i32, i32, i32, i32, i32) = conn
        .query_row(
            "SELECT body_ok, lens_ok, screen_ok, accessory_ok, function_ok FROM inspection_checklists WHERE id = ?1 AND tenant_id = ?2",
            params![input.id, scope.tenant_id().as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    let ok_int: i32 = if input.ok { 1 } else { 0 };
    let (col, note_col) = match input.step.as_str() {
        "body" => ("body_ok", "body_note"),
        "lens" => ("lens_ok", "lens_note"),
        "screen" => ("screen_ok", "screen_note"),
        "accessory" => ("accessory_ok", "accessory_note"),
        "function" => ("function_ok", "function_note"),
        _ => unreachable!(),
    };

    let now = shanghai_now();
    let sql = format!(
        "UPDATE inspection_checklists SET {} = ?1, {} = ?2, updated_at = ?3 WHERE id = ?4 AND tenant_id = ?5",
        col, note_col
    );
    conn.execute(
        &sql,
        params![
            ok_int,
            input.note,
            now,
            input.id,
            scope.tenant_id().as_str()
        ],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    // Recompute overall_grade
    let (b, l, s, a, f) = match input.step.as_str() {
        "body" => (
            ok_int == 1,
            lens_ok == 1,
            screen_ok == 1,
            accessory_ok == 1,
            function_ok == 1,
        ),
        "lens" => (
            body_ok == 1,
            ok_int == 1,
            screen_ok == 1,
            accessory_ok == 1,
            function_ok == 1,
        ),
        "screen" => (
            body_ok == 1,
            lens_ok == 1,
            ok_int == 1,
            accessory_ok == 1,
            function_ok == 1,
        ),
        "accessory" => (
            body_ok == 1,
            lens_ok == 1,
            screen_ok == 1,
            ok_int == 1,
            function_ok == 1,
        ),
        "function" => (
            body_ok == 1,
            lens_ok == 1,
            screen_ok == 1,
            accessory_ok == 1,
            ok_int == 1,
        ),
        _ => unreachable!(),
    };

    let grade = compute_overall_grade(b, l, s, a, f);
    conn.execute(
        "UPDATE inspection_checklists SET overall_grade = ?1, updated_at = ?2 WHERE id = ?3 AND tenant_id = ?4",
        params![grade, now, input.id, scope.tenant_id().as_str()],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true,
        "id": input.id,
        "step": input.step,
        "ok": input.ok,
        "overallGrade": grade,
        "updatedAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// inspection_complete — 完成检查，非 pass 自动创建 damage_report
// ══════════════════════════════════════════════════════════════════════════

fn inspection_complete(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &InspectionCompleteInput,
    inspector_id: i64,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Read checklist with all fields (within transaction)
    let checklist = conn
        .query_row(
            "SELECT id, order_id, device_serial_no, inspector_id, body_ok, lens_ok, screen_ok, accessory_ok, function_ok, overall_grade, body_note, lens_note, screen_note, accessory_note, function_note, notes, photo_urls
             FROM inspection_checklists WHERE id = ?1 AND tenant_id = ?2",
            params![input.id, scope.tenant_id().as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i32>(4)? == 1,
                    row.get::<_, i32>(5)? == 1,
                    row.get::<_, i32>(6)? == 1,
                    row.get::<_, i32>(7)? == 1,
                    row.get::<_, i32>(8)? == 1,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, Option<String>>(16)?,
                ))
            },
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    let (
        checklist_id,
        order_id,
        device_serial_no,
        _inspector_id,
        body_ok,
        lens_ok,
        screen_ok,
        accessory_ok,
        function_ok,
        overall_grade,
        body_note,
        lens_note,
        screen_note,
        accessory_note,
        function_note,
        notes,
        photo_urls,
    ) = checklist;

    let now = shanghai_now();
    let mut damage_report_id: Option<i64> = None;

    // If overall_grade is not pass, auto-create damage_report
    if overall_grade != "pass" {
        // Build damage description from step notes
        let mut damage_parts: Vec<String> = Vec::new();
        if !body_ok {
            damage_parts.push(format!(
                "外观异常: {}",
                body_note.as_deref().unwrap_or("无备注")
            ));
        }
        if !lens_ok {
            damage_parts.push(format!(
                "镜头异常: {}",
                lens_note.as_deref().unwrap_or("无备注")
            ));
        }
        if !screen_ok {
            damage_parts.push(format!(
                "屏幕异常: {}",
                screen_note.as_deref().unwrap_or("无备注")
            ));
        }
        if !accessory_ok {
            damage_parts.push(format!(
                "配件异常: {}",
                accessory_note.as_deref().unwrap_or("无备注")
            ));
        }
        if !function_ok {
            damage_parts.push(format!(
                "功能异常: {}",
                function_note.as_deref().unwrap_or("无备注")
            ));
        }

        let description = damage_parts.join("; ");
        let severity = match overall_grade.as_str() {
            "minor_damage" => "minor",
            "major_damage" => "major",
            "total_loss" => "critical",
            _ => "minor",
        };

        // Create damage_report
        conn.execute(
            "INSERT INTO damage_reports (device_serial_no, order_id, reporter_id, severity, description, status, photo_urls, detected_at, created_at, updated_at, tenant_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, ?7, ?7, ?8)",
            params![device_serial_no, order_id, inspector_id, severity, description, photo_urls, now, scope.tenant_id().as_str()],
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_INSERT", e.to_string())
        })?;

        damage_report_id = Some(conn.last_insert_rowid());
    }

    // Link damage_report_id back to checklist
    if let Some(dr_id) = damage_report_id {
        conn.execute(
            "UPDATE inspection_checklists SET damage_report_id = ?1, notes = ?2, updated_at = ?3 WHERE id = ?4 AND tenant_id = ?5",
            params![dr_id, notes, now, checklist_id, scope.tenant_id().as_str()],
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
        "id": checklist_id,
        "orderId": order_id,
        "deviceSerialNo": device_serial_no,
        "overallGrade": overall_grade,
        "damageReportId": damage_report_id,
        "completedAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// inspection_get — 单条查询
// ══════════════════════════════════════════════════════════════════════════

fn inspection_get(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &InspectionGetInput,
) -> Result<Value, String> {
    let record = conn
        .query_row(
            "SELECT id, order_id, device_serial_no, inspector_id, body_ok, lens_ok, screen_ok, accessory_ok, function_ok, overall_grade, damage_report_id, body_note, lens_note, screen_note, accessory_note, function_note, photo_urls, notes, created_at, updated_at
             FROM inspection_checklists WHERE id = ?1 AND tenant_id = ?2",
            params![input.id, scope.tenant_id().as_str()],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "orderId": row.get::<_, i64>(1)?,
                    "deviceSerialNo": row.get::<_, String>(2)?,
                    "inspectorId": row.get::<_, i64>(3)?,
                    "bodyOk": row.get::<_, i32>(4)? == 1,
                    "lensOk": row.get::<_, i32>(5)? == 1,
                    "screenOk": row.get::<_, i32>(6)? == 1,
                    "accessoryOk": row.get::<_, i32>(7)? == 1,
                    "functionOk": row.get::<_, i32>(8)? == 1,
                    "overallGrade": row.get::<_, String>(9)?,
                    "damageReportId": row.get::<_, Option<i64>>(10)?,
                    "bodyNote": row.get::<_, Option<String>>(11)?,
                    "lensNote": row.get::<_, Option<String>>(12)?,
                    "screenNote": row.get::<_, Option<String>>(13)?,
                    "accessoryNote": row.get::<_, Option<String>>(14)?,
                    "functionNote": row.get::<_, Option<String>>(15)?,
                    "photoUrls": row.get::<_, Option<String>>(16)?,
                    "notes": row.get::<_, Option<String>>(17)?,
                    "createdAt": row.get::<_, String>(18)?,
                    "updatedAt": row.get::<_, String>(19)?,
                }))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "record": record }))
}

// ══════════════════════════════════════════════════════════════════════════
// inspection_list — 分页列表，参数化查询
// ══════════════════════════════════════════════════════════════════════════

fn inspection_list(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &InspectionListInput,
) -> Result<Value, String> {
    let page = input.page.unwrap_or(1).max(1);
    let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let mut conditions: Vec<String> = vec!["tenant_id = ?1".into()];
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(scope.tenant_id().as_str().to_owned())];

    if let Some(order_id) = input.order_id {
        let idx = conditions.len() + 1;
        conditions.push(format!("order_id = ?{}", idx));
        all_params.push(Box::new(order_id));
    }
    if let Some(ref serial_no) = input.device_serial_no {
        let idx = conditions.len() + 1;
        conditions.push(format!("device_serial_no = ?{}", idx));
        all_params.push(Box::new(serial_no.clone()));
    }
    if let Some(ref grade) = input.overall_grade {
        let idx = conditions.len() + 1;
        conditions.push(format!("overall_grade = ?{}", idx));
        all_params.push(Box::new(grade.clone()));
    }

    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Count
    let count_sql = format!("SELECT COUNT(*) FROM inspection_checklists {}", where_sql);
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
        "SELECT id, order_id, device_serial_no, inspector_id, body_ok, lens_ok, screen_ok, accessory_ok, function_ok, overall_grade, damage_report_id, body_note, lens_note, screen_note, accessory_note, function_note, photo_urls, notes, created_at, updated_at
         FROM inspection_checklists {} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
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
                "deviceSerialNo": row.get::<_, String>(2)?,
                "inspectorId": row.get::<_, i64>(3)?,
                "bodyOk": row.get::<_, i32>(4)? == 1,
                "lensOk": row.get::<_, i32>(5)? == 1,
                "screenOk": row.get::<_, i32>(6)? == 1,
                "accessoryOk": row.get::<_, i32>(7)? == 1,
                "functionOk": row.get::<_, i32>(8)? == 1,
                "overallGrade": row.get::<_, String>(9)?,
                "damageReportId": row.get::<_, Option<i64>>(10)?,
                "bodyNote": row.get::<_, Option<String>>(11)?,
                "lensNote": row.get::<_, Option<String>>(12)?,
                "screenNote": row.get::<_, Option<String>>(13)?,
                "accessoryNote": row.get::<_, Option<String>>(14)?,
                "functionNote": row.get::<_, Option<String>>(15)?,
                "photoUrls": row.get::<_, Option<String>>(16)?,
                "notes": row.get::<_, Option<String>>(17)?,
                "createdAt": row.get::<_, String>(18)?,
                "updatedAt": row.get::<_, String>(19)?,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::json!({
        "ok": true,
        "items": items,
        "total": total,
        "page": page,
        "pageSize": page_size,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// inspection_stats — 统计数据
// ══════════════════════════════════════════════════════════════════════════

fn inspection_stats(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    _input: &InspectionStatsInput,
) -> Result<Value, String> {
    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM inspection_checklists WHERE tenant_id = ?1",
            params![scope.tenant_id().as_str()],
            |row| row.get(0),
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let pass_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM inspection_checklists WHERE tenant_id = ?1 AND overall_grade = 'pass'",
            params![scope.tenant_id().as_str()],
            |row| row.get(0),
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let damage_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM inspection_checklists WHERE tenant_id = ?1 AND overall_grade != 'pass'",
            params![scope.tenant_id().as_str()],
            |row| row.get(0),
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let mut stmt = conn
        .prepare("SELECT overall_grade, COUNT(*) FROM inspection_checklists WHERE tenant_id = ?1 GROUP BY overall_grade")
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let by_grade_rows: Vec<(String, i64)> = stmt
        .query_map(params![scope.tenant_id().as_str()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    let by_grade: serde_json::Map<String, Value> = by_grade_rows
        .into_iter()
        .map(|(grade, count)| (grade, serde_json::json!(count)))
        .collect();

    Ok(serde_json::json!({
        "ok": true,
        "total": total,
        "passCount": pass_count,
        "damageCount": damage_count,
        "byGrade": by_grade,
    }))
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;
    use system_core::{Revision, TenantId};

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(TenantId::new(tenant).unwrap(), Revision::new("r1").unwrap()).unwrap()
    }

    #[test]
    fn inspection_queries_and_stats_do_not_cross_tenants() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE inspection_checklists (
                id INTEGER PRIMARY KEY, order_id INTEGER, device_serial_no TEXT,
                inspector_id INTEGER, body_ok INTEGER, lens_ok INTEGER, screen_ok INTEGER,
                accessory_ok INTEGER, function_ok INTEGER, overall_grade TEXT,
                damage_report_id INTEGER, body_note TEXT, lens_note TEXT, screen_note TEXT,
                accessory_note TEXT, function_note TEXT, photo_urls TEXT, notes TEXT,
                created_at TEXT, updated_at TEXT, tenant_id TEXT
             );
             INSERT INTO inspection_checklists VALUES
                (1, 10, 'A', 1, 1,1,1,1,1,'pass',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'2026-01-01','2026-01-01','tenant-a'),
                (2, 20, 'B', 2, 0,0,0,0,0,'major_damage',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'2026-01-01','2026-01-01','tenant-b');",
        )
        .unwrap();

        let stats = inspection_stats(&conn, &scope("tenant-a"), &InspectionStatsInput {}).unwrap();
        assert_eq!(stats["total"], 1);
        assert_eq!(stats["passCount"], 1);
        assert_eq!(stats["damageCount"], 0);
        assert!(inspection_get(&conn, &scope("tenant-a"), &InspectionGetInput { id: 2 }).is_err());
    }
}

//! feature-barcode — 条码/RFID 系统模块（Maxwell 原生）
//!
//! 条码生成、设备查找、扫描事件记录。
//!
//! ## 命令
//! - `generate` — 为单台设备生成条码（AdminUser）
//! - `batch_generate` — 为所有无条码设备生成条码（AdminUser）
//! - `lookup` — 通过条码查找设备（AuthUser）
//! - `scan_event` — 记录扫描事件（AuthUser）
//! - `scan_history` — 扫描历史查询（AuthUser）
//! - `scan_stats` — 扫描统计（AuthUser）

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
// Input types — Generate
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BarcodeGenerateInput {
    pub device_serial_no: String,
}

impl Validate for BarcodeGenerateInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
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

impl Sanitize for BarcodeGenerateInput {
    fn sanitize(&mut self) {
        self.device_serial_no = self.device_serial_no.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Lookup
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BarcodeLookupInput {
    pub barcode_text: String,
}

impl Validate for BarcodeLookupInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.barcode_text.is_empty() {
            errors.push(FieldError {
                field: "barcodeText".into(),
                message: "条码文本不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for BarcodeLookupInput {
    fn sanitize(&mut self) {
        self.barcode_text = self.barcode_text.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Scan Event
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEventInput {
    pub device_serial_no: String,
    pub barcode_text: Option<String>,
    pub scan_type: String,
    pub warehouse_id: Option<String>,
    pub notes: Option<String>,
}

impl Validate for ScanEventInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.device_serial_no.is_empty() {
            errors.push(FieldError {
                field: "deviceSerialNo".into(),
                message: "设备序列号不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if !matches!(
            self.scan_type.as_str(),
            "checkout" | "checkin" | "inventory" | "transfer"
        ) {
            errors.push(FieldError {
                field: "scanType".into(),
                message: "扫描类型无效".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ScanEventInput {
    fn sanitize(&mut self) {
        self.device_serial_no = self.device_serial_no.trim().to_string();
        if let Some(ref mut b) = self.barcode_text {
            *b = b.trim().to_string();
        }
        self.scan_type = self.scan_type.trim().to_string();
        if let Some(ref mut n) = self.notes {
            *n = n.trim().to_string();
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Scan History
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanHistoryInput {
    pub device_serial_no: Option<String>,
    pub scan_type: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl Validate for ScanHistoryInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for ScanHistoryInput {
    fn sanitize(&mut self) {
        if let Some(ref mut d) = self.device_serial_no {
            *d = d.trim().to_string();
        }
        if let Some(ref mut s) = self.scan_type {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.start_date {
            *s = s.trim().to_string();
        }
        if let Some(ref mut e) = self.end_date {
            *e = e.trim().to_string();
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureBarcode {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureBarcode {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureBarcode {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureBarcode {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "barcode".into(),
            version: "0.1.0".into(),
            description: "条码/RFID 系统 — 条码生成/设备查找/扫描事件".into(),
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
                "FeatureBarcode not initialized".into(),
            )
        })?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;

        match command {
            "generate" => {
                require_admin(ctx)?;
                let unvalidated: system_core::Unvalidated<BarcodeGenerateInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                barcode_generate(&conn, ctx.data_scope(), &validated.into_inner())
            }
            "batch_generate" => {
                require_admin(ctx)?;
                barcode_batch_generate(&conn, ctx.data_scope())
            }
            "lookup" => {
                let unvalidated: system_core::Unvalidated<BarcodeLookupInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                barcode_lookup(&conn, ctx.data_scope(), &validated.into_inner())
            }
            "scan_event" => {
                let unvalidated: system_core::Unvalidated<ScanEventInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let scanned_by = ctx
                    .user_id()
                    .ok_or_else(|| err("auth", "AUTH_REQUIRED", "需要登录".into()))?;
                scan_event_record(&conn, ctx.data_scope(), &input, scanned_by)
            }
            "scan_history" => {
                let unvalidated: system_core::Unvalidated<ScanHistoryInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                scan_history(&conn, ctx.data_scope(), &validated.into_inner())
            }
            "scan_stats" => scan_stats(&conn, ctx.data_scope()),
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
                "generate",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "batch_generate",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "lookup",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "scan_event",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "scan_history",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "scan_stats",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "barcode".into(),
            description: "条码/RFID 系统模块".into(),
            commands: vec![
                CommandSchema {
                    name: "generate".into(),
                    description: "生成单台设备条码".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "batch_generate".into(),
                    description: "批量生成所有未标记设备条码".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "lookup".into(),
                    description: "通过条码查找设备".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "scan_event".into(),
                    description: "记录扫描事件".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "scan_history".into(),
                    description: "扫描历史查询".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "scan_stats".into(),
                    description: "扫描统计".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Barcode implementations
// ══════════════════════════════════════════════════════════════════════════

fn barcode_generate(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &BarcodeGenerateInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Check if barcode already exists for this device
    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT bl.id, bl.barcode_text FROM barcode_labels bl
         JOIN devices d ON d.serialNo = bl.device_serial_no
         WHERE d.tenant_id = ?1 AND bl.device_serial_no = ?2 LIMIT 1",
            params![scope.tenant_id().as_str(), input.device_serial_no],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    if let Some((id, text)) = existing {
        let _ = conn.execute("ROLLBACK", []);
        // Return the existing record — not an error
        let generated_at: String = conn
            .query_row(
                "SELECT generated_at FROM barcode_labels WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap_or_default();
        return Ok(serde_json::json!({
            "ok": true, "id": id, "deviceSerialNo": input.device_serial_no,
            "barcodeText": text, "barcodeType": "CODE128", "labelFormat": "50x25mm",
            "generatedAt": generated_at, "alreadyExists": true,
        }));
    }

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

    // Generate barcode: PKT-{serialNo}-{random4hex}
    let hex_chars: String = {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        format!("{:04x}", rng.r#gen::<u16>())
    };
    let barcode_text = format!("PKT-{}-{}", input.device_serial_no, hex_chars);
    let now = shanghai_now();

    conn.execute(
        "INSERT INTO barcode_labels (device_serial_no, barcode_text, barcode_type, label_format, generated_at)
         VALUES (?1, ?2, 'CODE128', '50x25mm', ?3)",
        params![input.device_serial_no, barcode_text, now],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    let id = conn.last_insert_rowid();
    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true, "id": id, "deviceSerialNo": input.device_serial_no,
        "barcodeText": barcode_text, "barcodeType": "CODE128", "labelFormat": "50x25mm",
        "generatedAt": now,
    }))
}

fn barcode_batch_generate(conn: &rusqlite::Connection, scope: &DataScope) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Find all devices without barcodes
    let mut stmt = conn
        .prepare(
            "SELECT serialNo FROM devices
         WHERE tenant_id = ?1 AND serialNo NOT IN (SELECT device_serial_no FROM barcode_labels)",
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    let unmarked: Vec<String> = stmt
        .query_map(params![scope.tenant_id().as_str()], |row| row.get(0))
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?
        .filter_map(|r| r.ok())
        .collect();

    if unmarked.is_empty() {
        conn.execute("COMMIT", [])
            .map_err(|e| err("db", "DB_TXN", e.to_string()))?;
        return Ok(serde_json::json!({ "ok": true, "generated": 0 }));
    }

    let now = shanghai_now();
    let mut generated = 0_i64;

    {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        for serial_no in &unmarked {
            let hex_chars = format!("{:04x}", rng.r#gen::<u16>());
            let barcode_text = format!("PKT-{}-{}", serial_no, hex_chars);

            if let Err(e) = conn.execute(
                "INSERT INTO barcode_labels (device_serial_no, barcode_text, barcode_type, label_format, generated_at)
                 VALUES (?1, ?2, 'CODE128', '50x25mm', ?3)",
                params![serial_no, barcode_text, now],
            ) {
                let _ = conn.execute("ROLLBACK", []);
                return Err(err("db", "DB_INSERT", e.to_string()));
            }
            generated += 1;
        }
    }

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "generated": generated }))
}

fn barcode_lookup(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &BarcodeLookupInput,
) -> Result<Value, String> {
    let record = conn.query_row(
        "SELECT bl.id, bl.device_serial_no, bl.barcode_text, bl.barcode_type, bl.label_format, bl.generated_at,
                d.serialNo, d.modelId, d.rentalStatus, d.status, d.currentWarehouseId
         FROM barcode_labels bl
         JOIN devices d ON d.serialNo = bl.device_serial_no
         WHERE d.tenant_id = ?1 AND bl.barcode_text = ?2",
        params![scope.tenant_id().as_str(), input.barcode_text],
        |row| {
            Ok(serde_json::json!({
                "barcodeId": row.get::<_, i64>(0)?,
                "deviceSerialNo": row.get::<_, String>(1)?,
                "barcodeText": row.get::<_, String>(2)?,
                "barcodeType": row.get::<_, String>(3)?,
                "labelFormat": row.get::<_, String>(4)?,
                "generatedAt": row.get::<_, String>(5)?,
                "deviceInfo": {
                    "serialNo": row.get::<_, Option<String>>(6)?,
                    "modelId": row.get::<_, Option<String>>(7)?,
                    "rentalStatus": row.get::<_, Option<String>>(8)?,
                    "status": row.get::<_, Option<String>>(9)?,
                    "currentWarehouseId": row.get::<_, Option<String>>(10)?,
                },
            }))
        },
    ).map_err(|_| err("biz", "BIZ_NOT_FOUND", "未找到该条码对应的设备".into()))?;

    Ok(serde_json::json!({ "ok": true, "record": record }))
}

fn scan_event_record(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ScanEventInput,
    scanned_by: &str,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let references_belong_to_tenant = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM devices WHERE tenant_id = ?1 AND serialNo = ?2)
          AND EXISTS(
            SELECT 1 FROM tenant_memberships
            WHERE tenant_id = ?1 AND identity_id = ?3 AND status = 'active'
          )
          AND (?4 IS NULL OR EXISTS(SELECT 1 FROM warehouses WHERE tenant_id = ?1 AND id = ?4))",
            params![
                scope.tenant_id().as_str(),
                input.device_serial_no,
                scanned_by,
                input.warehouse_id
            ],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
    if !references_belong_to_tenant {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_NOT_FOUND",
            "Scan references not found".into(),
        ));
    }

    let now = shanghai_now();
    conn.execute(
        "INSERT INTO scan_events (device_serial_no, barcode_text, scan_type, scanned_by, warehouse_id, notes, created_at, tenant_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![input.device_serial_no, input.barcode_text, input.scan_type, scanned_by, input.warehouse_id, input.notes, now, scope.tenant_id().as_str()],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    let id = conn.last_insert_rowid();
    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true, "id": id, "deviceSerialNo": input.device_serial_no,
        "barcodeText": input.barcode_text, "scanType": input.scan_type,
        "scannedBy": scanned_by, "warehouseId": input.warehouse_id,
        "notes": input.notes, "createdAt": now,
    }))
}

fn scan_history(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ScanHistoryInput,
) -> Result<Value, String> {
    let page = input.page.unwrap_or(1).max(1);
    let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    // Parameterized WHERE clauses
    let mut conditions: Vec<String> = vec!["tenant_id = ?1".into()];
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(scope.tenant_id().as_str().to_string())];

    if let Some(ref serial) = input.device_serial_no {
        let idx = conditions.len() + 1;
        conditions.push(format!("device_serial_no = ?{}", idx));
        all_params.push(Box::new(serial.clone()));
    }
    if let Some(ref stype) = input.scan_type {
        let idx = conditions.len() + 1;
        conditions.push(format!("scan_type = ?{}", idx));
        all_params.push(Box::new(stype.clone()));
    }
    if let Some(ref start) = input.start_date {
        let idx = conditions.len() + 1;
        conditions.push(format!("date(created_at) >= ?{}", idx));
        all_params.push(Box::new(start.clone()));
    }
    if let Some(ref end) = input.end_date {
        let idx = conditions.len() + 1;
        conditions.push(format!("date(created_at) <= ?{}", idx));
        all_params.push(Box::new(end.clone()));
    }

    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM scan_events {}", where_sql);
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

    let query_sql = format!(
        "SELECT id, device_serial_no, barcode_text, scan_type, scanned_by, warehouse_id, notes, created_at
         FROM scan_events {} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
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

    let events: Vec<Value> = stmt
        .query_map(rusqlite::params_from_iter(param_refs), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "deviceSerialNo": row.get::<_, String>(1)?,
                "barcodeText": row.get::<_, Option<String>>(2)?,
                "scanType": row.get::<_, String>(3)?,
                "scannedBy": row.get::<_, String>(4)?,
                "warehouseId": row.get::<_, Option<String>>(5)?,
                "notes": row.get::<_, Option<String>>(6)?,
                "createdAt": row.get::<_, String>(7)?,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(
        serde_json::json!({ "ok": true, "events": events, "total": total, "page": page, "pageSize": page_size }),
    )
}

fn scan_stats(conn: &rusqlite::Connection, scope: &DataScope) -> Result<Value, String> {
    let today_scans: i64 = conn.query_row(
        "SELECT COUNT(*) FROM scan_events WHERE tenant_id = ?1 AND date(created_at) = date('now', '+08:00')",
        params![scope.tenant_id().as_str()],
        |row| row.get(0),
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let this_week_scans: i64 = conn.query_row(
        "SELECT COUNT(*) FROM scan_events WHERE tenant_id = ?1 AND created_at >= datetime('now', '+08:00', '-7 days')",
        params![scope.tenant_id().as_str()],
        |row| row.get(0),
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT scan_type, COUNT(*) as cnt FROM scan_events WHERE tenant_id = ?1 AND created_at >= datetime('now', '+08:00', '-7 days') GROUP BY scan_type"
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let by_type_rows: Vec<(String, i64)> = stmt
        .query_map(params![scope.tenant_id().as_str()], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    let mut checkout = 0_i64;
    let mut checkin = 0_i64;
    let mut inventory = 0_i64;
    let mut transfer = 0_i64;

    for (stype, cnt) in &by_type_rows {
        match stype.as_str() {
            "checkout" => checkout = *cnt,
            "checkin" => checkin = *cnt,
            "inventory" => inventory = *cnt,
            "transfer" => transfer = *cnt,
            _ => {}
        }
    }

    Ok(serde_json::json!({
        "ok": true,
        "todayScans": today_scans,
        "thisWeekScans": this_week_scans,
        "byType": {
            "checkout": checkout,
            "checkin": checkin,
            "inventory": inventory,
            "transfer": transfer,
        },
    }))
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
    fn barcode_lookup_and_scan_history_are_limited_to_data_scope() {
        let conn = rusqlite::Connection::open_in_memory().expect("database");
        conn.execute_batch(
            "CREATE TABLE devices (
                serialNo TEXT PRIMARY KEY,
                modelId TEXT,
                rentalStatus TEXT,
                status TEXT,
                currentWarehouseId TEXT,
                tenant_id TEXT NOT NULL
             );
             CREATE TABLE barcode_labels (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_serial_no TEXT NOT NULL UNIQUE,
                barcode_text TEXT NOT NULL UNIQUE,
                barcode_type TEXT NOT NULL,
                label_format TEXT NOT NULL,
                generated_at TEXT NOT NULL
             );
             CREATE TABLE scan_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_serial_no TEXT NOT NULL,
                barcode_text TEXT,
                scan_type TEXT NOT NULL,
                scanned_by TEXT NOT NULL,
                warehouse_id TEXT,
                notes TEXT,
                created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL
             );
             INSERT INTO devices VALUES ('A-001', NULL, 'idle', 'active', NULL, 'tenant-a');
             INSERT INTO devices VALUES ('B-001', NULL, 'idle', 'active', NULL, 'tenant-b');
             INSERT INTO barcode_labels
                (device_serial_no, barcode_text, barcode_type, label_format, generated_at)
             VALUES ('B-001', 'BAR-B', 'CODE128', '50x25mm', '2026-07-16 12:00:00');
             INSERT INTO scan_events
                (device_serial_no, barcode_text, scan_type, scanned_by, created_at, tenant_id)
             VALUES ('A-001', 'BAR-A', 'inventory', 'identity-a', '2026-07-16 12:00:00', 'tenant-a');
             INSERT INTO scan_events
                (device_serial_no, barcode_text, scan_type, scanned_by, created_at, tenant_id)
             VALUES ('B-001', 'BAR-B', 'inventory', 'identity-b', '2026-07-16 12:00:00', 'tenant-b');",
        )
        .expect("schema and fixtures");

        let tenant_a = scope("tenant-a");
        let lookup = barcode_lookup(
            &conn,
            &tenant_a,
            &BarcodeLookupInput {
                barcode_text: "BAR-B".into(),
            },
        );
        assert!(
            lookup.is_err(),
            "tenant A must not resolve tenant B's barcode"
        );

        let history = scan_history(
            &conn,
            &tenant_a,
            &ScanHistoryInput {
                device_serial_no: None,
                scan_type: None,
                start_date: None,
                end_date: None,
                page: None,
                page_size: None,
            },
        )
        .expect("tenant-scoped scan history");
        assert_eq!(history["total"], 1);
        assert_eq!(history["events"][0]["deviceSerialNo"], "A-001");
    }
}

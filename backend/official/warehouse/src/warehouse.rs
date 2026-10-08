//! 仓库管理模块 (storage-bound — 连接池集成)
//!
//! 提供仓库的完整 CRUD、设备统计和关联设备查询。
//! 命令:
//! - list_warehouses: 列出所有仓库，按名称排序
//! - get_warehouse: 按 ID 获取单个仓库
//! - create_warehouse: 创建仓库（UUID v4 生成 ID）
//! - update_warehouse: 按 ID 更新仓库字段
//! - delete_warehouse: 按 ID 删除仓库
//! - get_warehouse_stats: 所有仓库的设备数量统计
//! - get_warehouse_devices: 按仓库 ID 列出关联设备
//!
//! 使用 r2d2 连接池 (SQLite)。独立运行时通过 init() 传入 {"databaseUrl": ":memory:"} 等配置。

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, types::ToSql};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use system_core::*;

// ── 时间辅助 ──

fn shanghai_now_iso() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%.3f+08:00")
        .to_string()
}

// ── 输入类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListWarehousesInput {}

impl Validate for ListWarehousesInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for ListWarehousesInput {
    fn sanitize(&mut self) {}
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetWarehouseInput {
    pub id: String,
}

impl Validate for GetWarehouseInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "仓库 ID 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for GetWarehouseInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWarehouseInput {
    pub name: String,
    #[serde(rename = "type")]
    pub wh_type: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub contact_name: String,
    #[serde(default)]
    pub contact_phone: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub capacity: i32,
}

fn default_enabled() -> bool {
    true
}

impl Validate for CreateWarehouseInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.name.trim().is_empty() {
            errors.push(FieldError {
                field: "name".into(),
                code: "VAL_REQUIRED".into(),
                message: "仓库名称不能为空".into(),
            });
        }
        if self.wh_type.trim().is_empty() {
            errors.push(FieldError {
                field: "type".into(),
                code: "VAL_REQUIRED".into(),
                message: "仓库类型不能为空".into(),
            });
        } else if !["owned", "partner"].contains(&self.wh_type.as_str()) {
            errors.push(FieldError {
                field: "type".into(),
                code: "VAL_INVALID".into(),
                message: "仓库类型无效，必须是 owned 或 partner".into(),
            });
        }
        if self.capacity < 0 {
            errors.push(FieldError {
                field: "capacity".into(),
                code: "VAL_INVALID".into(),
                message: "容量不能为负数".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for CreateWarehouseInput {
    fn sanitize(&mut self) {
        self.name = self.name.trim().to_string();
        self.wh_type = self.wh_type.trim().to_string();
        self.address = self.address.trim().to_string();
        self.contact_name = self.contact_name.trim().to_string();
        self.contact_phone = self.contact_phone.trim().to_string();
        self.notes = self.notes.trim().to_string();
        if self.capacity < 0 {
            self.capacity = 0;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWarehouseInput {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "type", default)]
    pub wh_type: Option<String>,
    pub enabled: Option<bool>,
    pub address: Option<String>,
    pub contact_name: Option<String>,
    pub contact_phone: Option<String>,
    pub notes: Option<String>,
    pub capacity: Option<i32>,
}

impl Validate for UpdateWarehouseInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "仓库 ID 不能为空".into(),
            });
        }
        if let Some(ref name) = self.name
            && name.trim().is_empty()
        {
            errors.push(FieldError {
                field: "name".into(),
                code: "VAL_INVALID".into(),
                message: "仓库名称不能为空".into(),
            });
        }
        if let Some(ref wh_type) = self.wh_type {
            if wh_type.trim().is_empty() {
                errors.push(FieldError {
                    field: "type".into(),
                    code: "VAL_REQUIRED".into(),
                    message: "仓库类型不能为空".into(),
                });
            } else if !["owned", "partner"].contains(&wh_type.as_str()) {
                errors.push(FieldError {
                    field: "type".into(),
                    code: "VAL_INVALID".into(),
                    message: "仓库类型无效，必须是 owned 或 partner".into(),
                });
            }
        }
        if let Some(cap) = self.capacity
            && cap < 0
        {
            errors.push(FieldError {
                field: "capacity".into(),
                code: "VAL_INVALID".into(),
                message: "容量不能为负数".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for UpdateWarehouseInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
        if let Some(ref mut name) = self.name {
            *name = name.trim().to_string();
        }
        if let Some(ref mut wh_type) = self.wh_type {
            *wh_type = wh_type.trim().to_string();
        }
        if let Some(ref mut address) = self.address {
            *address = address.trim().to_string();
        }
        if let Some(ref mut contact_name) = self.contact_name {
            *contact_name = contact_name.trim().to_string();
        }
        if let Some(ref mut contact_phone) = self.contact_phone {
            *contact_phone = contact_phone.trim().to_string();
        }
        if let Some(ref mut notes) = self.notes {
            *notes = notes.trim().to_string();
        }
        if let Some(ref mut cap) = self.capacity
            && *cap < 0
        {
            *cap = 0;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteWarehouseInput {
    pub id: String,
}

impl Validate for DeleteWarehouseInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "仓库 ID 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for DeleteWarehouseInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetWarehouseStatsInput {}

impl Validate for GetWarehouseStatsInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for GetWarehouseStatsInput {
    fn sanitize(&mut self) {}
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetWarehouseDevicesInput {
    pub id: String,
    #[serde(default)]
    pub status: Option<String>,
}

impl Validate for GetWarehouseDevicesInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.trim().is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                code: "VAL_REQUIRED".into(),
                message: "仓库 ID 不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for GetWarehouseDevicesInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
        if let Some(ref mut s) = self.status {
            *s = s.trim().to_string();
        }
    }
}

// ── 输出类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Warehouse {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub wh_type: String,
    pub enabled: bool,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub contact_name: String,
    #[serde(default)]
    pub contact_phone: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub capacity: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseStats {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub wh_type: String,
    pub enabled: bool,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub contact_name: String,
    #[serde(default)]
    pub contact_phone: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub capacity: i32,
    pub total_devices: i64,
    pub available_devices: i64,
    pub rented_devices: i64,
    pub repairing_devices: i64,
    pub utilization_percent: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WarehouseDevice {
    pub id: String,
    pub serial_no: String,
    pub model_name: String,
    pub status: String,
    pub warehouse_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOutput {
    pub success: bool,
}

// ── 数据库行映射 ──

fn row_to_warehouse(row: &rusqlite::Row) -> rusqlite::Result<Warehouse> {
    Ok(Warehouse {
        id: row.get("id")?,
        name: row.get("name")?,
        wh_type: row.get("type")?,
        enabled: row.get::<_, i32>("enabled")? != 0,
        address: row.get::<_, String>("address").unwrap_or_default(),
        contact_name: row.get::<_, String>("contactName").unwrap_or_default(),
        contact_phone: row.get::<_, String>("contactPhone").unwrap_or_default(),
        notes: row.get::<_, String>("notes").unwrap_or_default(),
        capacity: row.get::<_, i32>("capacity").unwrap_or(0),
        created_at: row.get("createdAt")?,
        updated_at: row.get("updatedAt")?,
    })
}

fn row_to_warehouse_stats(row: &rusqlite::Row) -> rusqlite::Result<WarehouseStats> {
    let total: i64 = row.get::<_, i64>("totalDevices").unwrap_or(0);
    let cap: i32 = row.get::<_, i32>("capacity").unwrap_or(0);
    let util = if cap > 0 {
        (total * 100) / (cap as i64)
    } else {
        0
    };
    Ok(WarehouseStats {
        id: row.get("id")?,
        name: row.get("name")?,
        wh_type: row.get("type")?,
        enabled: row.get::<_, i32>("enabled")? != 0,
        address: row.get::<_, String>("address").unwrap_or_default(),
        contact_name: row.get::<_, String>("contactName").unwrap_or_default(),
        contact_phone: row.get::<_, String>("contactPhone").unwrap_or_default(),
        notes: row.get::<_, String>("notes").unwrap_or_default(),
        capacity: cap,
        total_devices: total,
        available_devices: row.get::<_, i64>("availableDevices").unwrap_or(0),
        rented_devices: row.get::<_, i64>("rentedDevices").unwrap_or(0),
        repairing_devices: row.get::<_, i64>("repairingDevices").unwrap_or(0),
        utilization_percent: util,
        created_at: row.get("createdAt")?,
        updated_at: row.get("updatedAt")?,
    })
}

fn err_payload(category: &str, code: &str, message: &str) -> String {
    serde_json::to_string(&ErrorPayload {
        category: category.into(),
        code: code.into(),
        message: message.into(),
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

// ── 模块主体 ──

/// 仓库管理模块
///
/// 通过 r2d2 连接池访问 SQLite 数据库。
/// init() 接受 `{"databaseUrl": ":memory:"}` 或实际文件路径。
pub struct FeatureWarehouse {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureWarehouse {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self
            .pool
            .lock()
            .map_err(|e| err_payload("sys", "SYS_DB_LOCK", &format!("连接池锁错误: {}", e)))?;
        guard
            .as_ref()
            .ok_or_else(|| err_payload("sys", "SYS_DB_NOT_INIT", "数据库未初始化"))?
            .get()
            .map_err(|e| err_payload("sys", "SYS_DB_POOL", &format!("连接池获取失败: {}", e)))
    }

    // ── 命令实现 ──

    fn list_warehouses(
        &self,
        conn: &rusqlite::Connection,
        ctx: &ExecutionContext,
    ) -> Result<Vec<Warehouse>, String> {
        let tenant_id = ctx.data_scope().tenant_id().as_str();
        let sql = "SELECT * FROM warehouses WHERE tenant_id = ? ORDER BY name ASC";

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| err_payload("sys", "SYS_DB_PREPARE", &format!("查询准备失败: {}", e)))?;

        let rows = stmt
            .query_map(params![tenant_id], row_to_warehouse)
            .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("查询失败: {}", e)))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(
                row.map_err(|e| err_payload("sys", "SYS_DB_ROW", &format!("行解析失败: {}", e)))?,
            );
        }
        Ok(results)
    }

    fn get_warehouse(
        &self,
        conn: &rusqlite::Connection,
        id: &str,
        ctx: &ExecutionContext,
    ) -> Result<Warehouse, String> {
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        conn.query_row(
            "SELECT * FROM warehouses WHERE tenant_id = ? AND id = ?",
            params![tenant_id, id],
            row_to_warehouse,
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                err_payload("val", "VAL_NOT_FOUND", "仓库不存在或无权访问")
            }
            _ => err_payload("sys", "SYS_DB_QUERY", &format!("查询失败: {}", e)),
        })
    }

    fn create_warehouse(
        &self,
        conn: &rusqlite::Connection,
        input: &CreateWarehouseInput,
        ctx: &ExecutionContext,
    ) -> Result<Warehouse, String> {
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        // 检查当前数据作用域内的名称唯一性
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(1) > 0 FROM warehouses WHERE tenant_id = ? AND name = ?",
                params![tenant_id, input.name],
                |row| row.get(0),
            )
            .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("查询失败: {}", e)))?;
        if exists {
            return Err(err_payload("val", "VAL_DUPLICATE", "仓库名称已存在"));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = shanghai_now_iso();

        conn.execute(
            "INSERT INTO warehouses (id, name, type, enabled, address, contactName, contactPhone, notes, capacity, createdAt, updatedAt, tenant_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id,
                input.name,
                input.wh_type,
                input.enabled as i32,
                input.address,
                input.contact_name,
                input.contact_phone,
                input.notes,
                input.capacity,
                now,
                now,
                tenant_id,
            ],
        )
        .map_err(|e| err_payload("sys", "SYS_DB_EXECUTE", &format!("插入失败: {}", e)))?;

        self.get_warehouse(conn, &id, ctx)
    }

    fn update_warehouse(
        &self,
        conn: &rusqlite::Connection,
        input: &UpdateWarehouseInput,
        ctx: &ExecutionContext,
    ) -> Result<Warehouse, String> {
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        conn.query_row(
            "SELECT id FROM warehouses WHERE tenant_id = ? AND id = ?",
            params![tenant_id, input.id],
            |_| Ok(()),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                err_payload("val", "VAL_NOT_FOUND", "仓库不存在或无权访问")
            }
            _ => err_payload("sys", "SYS_DB_QUERY", &format!("查询失败: {}", e)),
        })?;

        let mut set_clauses: Vec<String> = Vec::new();
        let mut param_values: Vec<Box<dyn ToSql>> = Vec::new();

        if let Some(ref name) = input.name {
            // 检查名称唯一性（排除自身）
            let dup: bool = conn
                .query_row(
                    "SELECT COUNT(1) > 0 FROM warehouses WHERE tenant_id = ? AND name = ? AND id <> ?",
                    params![tenant_id, name, input.id],
                    |row| row.get(0),
                )
                .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("查询失败: {}", e)))?;
            if dup {
                return Err(err_payload("val", "VAL_DUPLICATE", "仓库名称已存在"));
            }
            set_clauses.push("name = ?".into());
            param_values.push(Box::new(name.clone()));
        }
        if let Some(ref wh_type) = input.wh_type {
            set_clauses.push("type = ?".into());
            param_values.push(Box::new(wh_type.clone()));
        }
        if let Some(enabled) = input.enabled {
            set_clauses.push("enabled = ?".into());
            param_values.push(Box::new(enabled as i32));
        }
        if let Some(ref address) = input.address {
            set_clauses.push("address = ?".into());
            param_values.push(Box::new(address.clone()));
        }
        if let Some(ref contact_name) = input.contact_name {
            set_clauses.push("contactName = ?".into());
            param_values.push(Box::new(contact_name.clone()));
        }
        if let Some(ref contact_phone) = input.contact_phone {
            set_clauses.push("contactPhone = ?".into());
            param_values.push(Box::new(contact_phone.clone()));
        }
        if let Some(ref notes) = input.notes {
            set_clauses.push("notes = ?".into());
            param_values.push(Box::new(notes.clone()));
        }
        if let Some(capacity) = input.capacity {
            set_clauses.push("capacity = ?".into());
            param_values.push(Box::new(capacity));
        }

        if !set_clauses.is_empty() {
            let now = shanghai_now_iso();
            set_clauses.push("updatedAt = ?".into());
            param_values.push(Box::new(now));

            param_values.push(Box::new(tenant_id.to_owned()));
            param_values.push(Box::new(input.id.clone()));
            let sql = format!(
                "UPDATE warehouses SET {} WHERE tenant_id = ? AND id = ?",
                set_clauses.join(", ")
            );

            let refs: Vec<&dyn ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

            let rows_affected = conn
                .execute(&sql, refs.as_slice())
                .map_err(|e| err_payload("sys", "SYS_DB_EXECUTE", &format!("更新失败: {}", e)))?;

            // 检查租户隔离
            if rows_affected == 0 {
                return Err(err_payload("val", "VAL_NOT_FOUND", "仓库不存在或无权访问"));
            }
        }

        self.get_warehouse(conn, &input.id, ctx)
    }

    fn delete_warehouse(
        &self,
        conn: &rusqlite::Connection,
        id: &str,
        ctx: &ExecutionContext,
    ) -> Result<DeleteOutput, String> {
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        conn.query_row(
            "SELECT id FROM warehouses WHERE tenant_id = ? AND id = ?",
            params![tenant_id, id],
            |_| Ok(()),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                err_payload("val", "VAL_NOT_FOUND", "仓库不存在或无权访问")
            }
            _ => err_payload("sys", "SYS_DB_QUERY", &format!("查询失败: {}", e)),
        })?;

        // 检查仓库下是否有设备
        let device_count: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM devices WHERE tenant_id = ? AND (currentWarehouseId = ? OR expectedWarehouseId = ?)",
                params![tenant_id, id, id],
                |row| row.get(0),
            )
            .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("查询失败: {}", e)))?;

        if device_count > 0 {
            return Err(err_payload(
                "val",
                "VAL_CONFLICT",
                &format!("该仓库下有 {} 台设备，无法删除", device_count),
            ));
        }

        let rows_affected = conn
            .execute(
                "DELETE FROM warehouses WHERE tenant_id = ? AND id = ?",
                params![tenant_id, id],
            )
            .map_err(|e| err_payload("sys", "SYS_DB_EXECUTE", &format!("删除失败: {}", e)))?;

        if rows_affected == 0 {
            return Err(err_payload("val", "VAL_NOT_FOUND", "仓库不存在或无权访问"));
        }

        Ok(DeleteOutput { success: true })
    }

    fn get_warehouse_stats(
        &self,
        conn: &rusqlite::Connection,
        ctx: &ExecutionContext,
    ) -> Result<Vec<WarehouseStats>, String> {
        let tenant_id = ctx.data_scope().tenant_id().as_str();
        let sql = "
            SELECT
                w.*,
                COUNT(d.id) AS totalDevices,
                SUM(CASE WHEN d.rentalStatus = '已入库' THEN 1 ELSE 0 END) AS availableDevices,
                SUM(CASE WHEN d.rentalStatus = '租赁中' THEN 1 ELSE 0 END) AS rentedDevices,
                SUM(CASE WHEN d.rentalStatus = '返厂维修' THEN 1 ELSE 0 END) AS repairingDevices
            FROM warehouses w
            LEFT JOIN devices d ON d.currentWarehouseId = w.id AND d.tenant_id = w.tenant_id
            WHERE w.tenant_id = ?
            GROUP BY w.id
            ORDER BY w.name ASC
        ";
        let mut stmt = conn.prepare(sql).map_err(|e| {
            err_payload("sys", "SYS_DB_PREPARE", &format!("统计查询准备失败: {}", e))
        })?;
        let rows = stmt
            .query_map(params![tenant_id], row_to_warehouse_stats)
            .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("统计查询失败: {}", e)))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(
                row.map_err(|e| err_payload("sys", "SYS_DB_ROW", &format!("行解析失败: {}", e)))?,
            );
        }
        Ok(results)
    }

    fn get_warehouse_devices(
        &self,
        conn: &rusqlite::Connection,
        id: &str,
        status: Option<&str>,
        ctx: &ExecutionContext,
    ) -> Result<Vec<WarehouseDevice>, String> {
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        // 先检查当前数据作用域内的仓库存在
        conn.query_row(
            "SELECT id FROM warehouses WHERE tenant_id = ? AND id = ?",
            params![tenant_id, id],
            |_| Ok(()),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                err_payload("val", "VAL_NOT_FOUND", "仓库不存在")
            }
            _ => err_payload("sys", "SYS_DB_QUERY", &format!("查询失败: {}", e)),
        })?;

        let (sql, param_vec): (String, Vec<Box<dyn ToSql>>) = if let Some(ref s) = status {
            if s.is_empty() {
                (
                    "SELECT d.id, d.serialNo, COALESCE(dm.name, '') AS modelName, d.rentalStatus, \
                     COALESCE(d.currentWarehouseId, '') AS warehouseId, d.createdAt \
                     FROM devices d \
                     LEFT JOIN device_models dm ON dm.id = d.modelId AND dm.tenant_id = d.tenant_id \
                     WHERE d.tenant_id = ?1 AND d.currentWarehouseId = ?2 \
                     ORDER BY d.createdAt DESC"
                        .into(),
                    vec![Box::new(tenant_id.to_owned()), Box::new(id.to_string())],
                )
            } else {
                (
                    "SELECT d.id, d.serialNo, COALESCE(dm.name, '') AS modelName, d.rentalStatus, \
                     COALESCE(d.currentWarehouseId, '') AS warehouseId, d.createdAt \
                     FROM devices d \
                     LEFT JOIN device_models dm ON dm.id = d.modelId AND dm.tenant_id = d.tenant_id \
                     WHERE d.tenant_id = ?1 AND d.currentWarehouseId = ?2 AND d.rentalStatus = ?3 \
                     ORDER BY d.createdAt DESC"
                        .into(),
                    vec![Box::new(tenant_id.to_owned()), Box::new(id.to_string()), Box::new(s.to_string())],
                )
            }
        } else {
            (
                "SELECT d.id, d.serialNo, COALESCE(dm.name, '') AS modelName, d.rentalStatus, \
                 COALESCE(d.currentWarehouseId, '') AS warehouseId, d.createdAt \
                 FROM devices d \
                 LEFT JOIN device_models dm ON dm.id = d.modelId AND dm.tenant_id = d.tenant_id \
                 WHERE d.tenant_id = ?1 AND d.currentWarehouseId = ?2 \
                 ORDER BY d.createdAt DESC"
                    .into(),
                vec![Box::new(tenant_id.to_owned()), Box::new(id.to_string())],
            )
        };

        let refs: Vec<&dyn ToSql> = param_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql).map_err(|e| {
            err_payload("sys", "SYS_DB_PREPARE", &format!("设备查询准备失败: {}", e))
        })?;

        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(WarehouseDevice {
                    id: row.get("id")?,
                    serial_no: row.get("serialNo")?,
                    model_name: row.get("modelName")?,
                    status: row.get("rentalStatus")?,
                    warehouse_id: row.get("warehouseId")?,
                    created_at: row.get("createdAt")?,
                })
            })
            .map_err(|e| err_payload("sys", "SYS_DB_QUERY", &format!("设备查询失败: {}", e)))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(
                row.map_err(|e| err_payload("sys", "SYS_DB_ROW", &format!("行解析失败: {}", e)))?,
            );
        }
        Ok(results)
    }
}

impl Default for FeatureWarehouse {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureWarehouse {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "warehouse".into(),
            version: "0.1.0".into(),
            description: "仓库管理模块 — 仓库 CRUD + 设备统计 + 关联设备查询".into(),
            author: "hoshi".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "list_warehouses",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_warehouse",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "create_warehouse",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "update_warehouse",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "delete_warehouse",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_warehouse_stats",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_warehouse_devices",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        if let Some(db_url) = config.get("databaseUrl").and_then(|v| v.as_str()) {
            let manager = SqliteConnectionManager::file(db_url);
            let pool = Pool::builder().max_size(5).build(manager).map_err(|e| {
                err_payload("sys", "SYS_DB_POOL", &format!("创建连接池失败: {}", e))
            })?;
            let mut guard = self
                .pool
                .lock()
                .map_err(|e| err_payload("sys", "SYS_LOCK", &format!("锁获取失败: {}", e)))?;
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
        match command {
            "list_warehouses" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ListWarehousesInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let _validated = sanitized.validate()?;
                let conn = self.get_conn()?;
                let results = self.list_warehouses(&conn, ctx)?;
                serde_json::to_value(results)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "get_warehouse" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<GetWarehouseInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let result = self.get_warehouse(&conn, &input.id, ctx)?;
                serde_json::to_value(result)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "create_warehouse" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<CreateWarehouseInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let result = self.create_warehouse(&conn, &input, ctx)?;
                serde_json::to_value(result)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "update_warehouse" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<UpdateWarehouseInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let result = self.update_warehouse(&conn, &input, ctx)?;
                serde_json::to_value(result)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "delete_warehouse" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<DeleteWarehouseInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let result = self.delete_warehouse(&conn, &input.id, ctx)?;
                serde_json::to_value(result)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "get_warehouse_stats" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<GetWarehouseStatsInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let _validated = sanitized.validate()?;
                let conn = self.get_conn()?;
                let results = self.get_warehouse_stats(&conn, ctx)?;
                serde_json::to_value(results)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            "get_warehouse_devices" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<GetWarehouseDevicesInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let conn = self.get_conn()?;
                let results =
                    self.get_warehouse_devices(&conn, &input.id, input.status.as_deref(), ctx)?;
                serde_json::to_value(results)
                    .map_err(|e| err_payload("sys", "SYS_SERIALIZE", &e.to_string()))
            }
            _ => Err(err_payload(
                "sys",
                "SYS_UNKNOWN_COMMAND",
                &format!("未知命令: {}", command),
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "warehouse".into(),
            description: "仓库管理模块 — 仓库 CRUD + 设备统计 + 关联设备查询".into(),
            commands: vec![
                CommandSchema {
                    name: "list_warehouses".into(),
                    description: "列出所有仓库，按名称排序".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_warehouse".into(),
                    description: "按 ID 获取单个仓库".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "create_warehouse".into(),
                    description: "创建仓库，UUID v4 生成 ID".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "update_warehouse".into(),
                    description: "按 ID 更新仓库字段".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "delete_warehouse".into(),
                    description: "按 ID 删除仓库，有设备时拒绝删除".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_warehouse_stats".into(),
                    description: "所有仓库的设备数量及利用率统计".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_warehouse_devices".into(),
                    description: "按仓库 ID 列出关联设备，可选状态过滤".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_for(tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).expect("tenant ID is valid");
        let data_scope = DataScope::production(
            tenant_id.clone(),
            Revision::new("warehouse-test-revision").expect("revision is valid"),
        )
        .expect("production data scope is valid");

        ExecutionContext::new(
            ActorIdentity::system(),
            TenantScope::tenant(tenant_id),
            data_scope,
            ExecutionMode::Normal,
            RequestId::new("warehouse-test-request").expect("request ID is valid"),
            None,
            std::sync::Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn initialized_warehouse_module() -> FeatureWarehouse {
        let mut module = FeatureWarehouse::new();
        module
            .init(serde_json::json!({ "databaseUrl": ":memory:" }))
            .expect("in-memory database initializes");
        let conn = module.get_conn().expect("database connection");
        conn.execute_batch(
            "CREATE TABLE warehouses (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                type TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                address TEXT NOT NULL,
                contactName TEXT NOT NULL,
                contactPhone TEXT NOT NULL,
                notes TEXT NOT NULL,
                capacity INTEGER NOT NULL,
                createdAt TEXT NOT NULL,
                updatedAt TEXT NOT NULL,
                tenant_id TEXT NOT NULL
            );",
        )
        .expect("warehouse fixtures created");
        module
    }

    #[test]
    fn warehouse_names_are_unique_within_the_execution_context_data_scope() {
        let module = initialized_warehouse_module();
        let conn = module.get_conn().expect("database connection");
        let input = CreateWarehouseInput {
            name: "Shared name".into(),
            wh_type: "Main".into(),
            enabled: true,
            address: "".into(),
            contact_name: "".into(),
            contact_phone: "".into(),
            notes: "".into(),
            capacity: 0,
        };

        module
            .create_warehouse(&conn, &input, &context_for("tenant-a"))
            .expect("first tenant can create warehouse");
        module
            .create_warehouse(&conn, &input, &context_for("tenant-b"))
            .expect("second tenant can use the same warehouse name");
    }
}

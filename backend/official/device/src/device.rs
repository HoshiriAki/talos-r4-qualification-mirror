//! feature-device — 设备管理模块 (Maxwell native)
//!
//! 设备 CRUD + 序列号解析 + 模糊匹配
//! 4-layer pipeline: DeserializeGuard → serde → Sanitize → Validate → execute

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{LazyLock, Mutex};
use system_core::*;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OptionalExtension;
use rusqlite::params;

// ── 静态正则 ──

static SERIAL_NO_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9]+$").unwrap());

// ── 辅助函数：序列号净化 ──

fn sanitize_serial_no(s: &mut String) {
    // trim
    *s = s.trim().to_string();
    // strip control characters
    s.retain(|c| !c.is_control());
    // uppercase
    *s = s.to_uppercase();
    // keep only ASCII alphanumeric
    s.retain(|c| c.is_ascii_alphanumeric());
}

// ── 输入类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDevicesInput {
    pub status: Option<String>,
    pub model_id: Option<String>,
    pub warehouse_id: Option<String>,
    pub serial_no: Option<String>,
}

fn err_json(code: &str, message: &str) -> String {
    serde_json::to_string(&ErrorPayload {
        category: "sys".into(),
        code: code.into(),
        message: message.into(),
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDevicesPagedInput {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    pub keyword: Option<String>,
    pub rental_status: Option<String>,
    pub notes: Option<String>,
    pub warning_status: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    30
}

impl Sanitize for ListDevicesPagedInput {
    fn sanitize(&mut self) {
        self.page = self.page.max(1);
        self.page_size = self.page_size.clamp(1, 200);
        for value in [
            &mut self.keyword,
            &mut self.rental_status,
            &mut self.notes,
            &mut self.warning_status,
            &mut self.sort_by,
            &mut self.sort_order,
        ]
        .into_iter()
        .flatten()
        {
            *value = value.trim().to_string();
        }
        if let Some(value) = &mut self.sort_order {
            *value = value.to_ascii_lowercase();
        }
    }
}

impl Validate for ListDevicesPagedInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for ListDevicesInput {
    fn sanitize(&mut self) {
        if let Some(ref mut s) = self.status {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.model_id {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.warehouse_id {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.serial_no {
            sanitize_serial_no(s);
        }
    }
}

impl Validate for ListDevicesInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDeviceInput {
    pub serial_no: String,
}

impl Sanitize for GetDeviceInput {
    fn sanitize(&mut self) {
        sanitize_serial_no(&mut self.serial_no);
    }
}

impl Validate for GetDeviceInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.serial_no.is_empty() {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_REQUIRED".into(),
                message: "序列号不能为空".into(),
            });
        }
        if !self.serial_no.is_empty() && !SERIAL_NO_REGEX.is_match(&self.serial_no) {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_FORMAT".into(),
                message: "序列号只能包含字母和数字".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeviceInput {
    pub serial_no: String,
    pub model_id: String,
    pub warehouse_id: String,
    pub status: Option<String>,
}

impl Sanitize for CreateDeviceInput {
    fn sanitize(&mut self) {
        sanitize_serial_no(&mut self.serial_no);
        self.model_id = self.model_id.trim().to_string();
        self.warehouse_id = self.warehouse_id.trim().to_string();
        if let Some(ref mut s) = self.status {
            *s = s.trim().to_string();
        }
    }
}

impl Validate for CreateDeviceInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.serial_no.is_empty() {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_REQUIRED".into(),
                message: "序列号不能为空".into(),
            });
        }
        if !self.serial_no.is_empty() && !SERIAL_NO_REGEX.is_match(&self.serial_no) {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_FORMAT".into(),
                message: "序列号只能包含字母和数字".into(),
            });
        }
        if self.model_id.is_empty() {
            errors.push(FieldError {
                field: "modelId".into(),
                code: "VAL_REQUIRED".into(),
                message: "型号ID不能为空".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeviceInput {
    pub serial_no: String,
    pub model_id: Option<String>,
    pub warehouse_id: Option<String>,
    pub status: Option<String>,
}

impl Sanitize for UpdateDeviceInput {
    fn sanitize(&mut self) {
        sanitize_serial_no(&mut self.serial_no);
        if let Some(ref mut s) = self.model_id {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.warehouse_id {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.status {
            *s = s.trim().to_string();
        }
    }
}

impl Validate for UpdateDeviceInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.serial_no.is_empty() {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_REQUIRED".into(),
                message: "序列号不能为空".into(),
            });
        }
        if !self.serial_no.is_empty() && !SERIAL_NO_REGEX.is_match(&self.serial_no) {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_FORMAT".into(),
                message: "序列号只能包含字母和数字".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDeviceInput {
    pub serial_no: String,
}

impl Sanitize for DeleteDeviceInput {
    fn sanitize(&mut self) {
        sanitize_serial_no(&mut self.serial_no);
    }
}

impl Validate for DeleteDeviceInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.serial_no.is_empty() {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_REQUIRED".into(),
                message: "序列号不能为空".into(),
            });
        }
        if !self.serial_no.is_empty() && !SERIAL_NO_REGEX.is_match(&self.serial_no) {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_FORMAT".into(),
                message: "序列号只能包含字母和数字".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveSerialInput {
    pub serial_no: String,
}

impl Sanitize for ResolveSerialInput {
    fn sanitize(&mut self) {
        sanitize_serial_no(&mut self.serial_no);
    }
}

impl Validate for ResolveSerialInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.serial_no.is_empty() {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_REQUIRED".into(),
                message: "序列号不能为空".into(),
            });
        }
        if !self.serial_no.is_empty() && !SERIAL_NO_REGEX.is_match(&self.serial_no) {
            errors.push(FieldError {
                field: "serialNo".into(),
                code: "VAL_FORMAT".into(),
                message: "序列号只能包含字母和数字".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDeviceMatchCandidatesInput {}

impl Sanitize for GetDeviceMatchCandidatesInput {
    fn sanitize(&mut self) {}
}

impl Validate for GetDeviceMatchCandidatesInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateSerialNoInput {
    pub serial_no: String,
}

impl Sanitize for ValidateSerialNoInput {
    fn sanitize(&mut self) {
        self.serial_no = self.serial_no.trim().to_string();
        self.serial_no.retain(|c| !c.is_control());
        self.serial_no = self.serial_no.to_uppercase();
    }
}

impl Validate for ValidateSerialNoInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

// ── 输出类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub serial_no: String,
    pub model_id: String,
    pub model_name: Option<String>,
    pub warehouse_id: Option<String>,
    pub warehouse_name: Option<String>,
    pub status: String,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevicePageRow {
    serial_no: String,
    rental_status: String,
    notes: String,
    warning_status: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevicePagination {
    page: u32,
    page_size: u32,
    total: u32,
    total_pages: u32,
}

#[derive(Debug, Clone, Serialize)]
struct PagedDevices {
    devices: Vec<DevicePageRow>,
    pagination: DevicePagination,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceMatchCandidate {
    pub serial_no: String,
    pub normalized_serial_no: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResult {
    pub matched: bool,
    pub serial_no: Option<String>,
    pub candidates: Vec<DeviceMatchCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOutput {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidateSerialNoOutput {
    pub valid: bool,
}

// ── 模块主体 ──

pub struct FeatureDevice {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureDevice {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    /// 从连接池获取一个连接
    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_LOCK".into(),
                message: format!("获取连接池锁失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        let pool = guard.as_ref().ok_or_else(|| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_NO_DB".into(),
                message: "数据库未初始化，请先调用 init()".into(),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        pool.get().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_CONN".into(),
                message: format!("获取数据库连接失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })
    }

    // ── 数据范围辅助函数 ──

    /// Returns the resolved repository scope for this command.
    ///
    /// A device query must never fall back to an unscoped or default tenant.
    fn data_scope(ctx: &ExecutionContext) -> Result<&DataScope, String> {
        let scope = ctx.data_scope();
        if scope.is_resolved() {
            Ok(scope)
        } else {
            Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNRESOLVED_DATA_SCOPE".into(),
                message: "设备操作需要已解析的数据范围".into(),
                field: None,
                context: None,
            })
            .unwrap_or_default())
        }
    }

    fn scoped_tenant_id(ctx: &ExecutionContext) -> Result<&str, String> {
        Ok(Self::data_scope(ctx)?.tenant_id().as_str())
    }

    // ── 业务方法 ──

    fn do_list_devices(
        &self,
        input: &ListDevicesInput,
        ctx: &ExecutionContext,
    ) -> Result<Vec<Device>, String> {
        let conn = self.get_conn()?;
        let tenant_id = Self::scoped_tenant_id(ctx)?;
        let mut sql = String::from(
            "SELECT d.serialNo, d.modelId, dm.name as modelName, d.currentWarehouseId, w.name as warehouseName, \
             d.rentalStatus, d.notes, d.createdAt, d.createdAt AS updatedAt \
             FROM devices d \
             LEFT JOIN device_models dm ON dm.id = d.modelId AND dm.tenant_id = d.tenant_id \
             LEFT JOIN warehouses w ON w.id = d.currentWarehouseId AND w.tenant_id = d.tenant_id \
             WHERE d.tenant_id = ?",
        );
        let mut param_values = vec![tenant_id.to_string()];

        if let Some(ref status) = input.status {
            sql.push_str(" AND d.rentalStatus = ?");
            param_values.push(status.clone());
        }
        if let Some(ref model_id) = input.model_id {
            sql.push_str(" AND d.modelId = ?");
            param_values.push(model_id.clone());
        }
        if let Some(ref warehouse_id) = input.warehouse_id {
            sql.push_str(" AND d.currentWarehouseId = ?");
            param_values.push(warehouse_id.clone());
        }
        if let Some(ref serial_no) = input.serial_no {
            sql.push_str(" AND d.serialNo = ?");
            param_values.push(serial_no.clone());
        }

        sql.push_str(" ORDER BY d.serialNo");

        let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();

        let mut stmt = conn.prepare(&sql).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_QUERY".into(),
                message: format!("准备查询失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;

        let rows = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(Device {
                    serial_no: row.get(0)?,
                    model_id: row.get(1)?,
                    model_name: row.get(2)?,
                    warehouse_id: row.get(3)?,
                    warehouse_name: row.get(4)?,
                    status: row.get(5)?,
                    notes: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

        let mut devices = Vec::new();
        for row in rows {
            devices.push(row.map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("读取行失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?);
        }
        Ok(devices)
    }

    fn do_list_devices_paged(
        &self,
        input: &ListDevicesPagedInput,
        ctx: &ExecutionContext,
    ) -> Result<PagedDevices, String> {
        let conn = self.get_conn()?;
        let tenant_id = Self::scoped_tenant_id(ctx)?;
        let mut where_parts = vec!["tenant_id = ?".to_string()];
        let mut values = vec![tenant_id.to_string()];

        if let Some(value) = input.keyword.as_deref().filter(|value| !value.is_empty()) {
            where_parts.push("(serialNo LIKE ? OR notes LIKE ?)".to_string());
            let like = format!("%{value}%");
            values.push(like.clone());
            values.push(like);
        }
        if let Some(value) = input
            .rental_status
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            where_parts.push("rentalStatus = ?".to_string());
            values.push(value.to_string());
        }
        if let Some(value) = input.notes.as_deref().filter(|value| !value.is_empty()) {
            where_parts.push("notes LIKE ?".to_string());
            values.push(format!("%{value}%"));
        }
        if let Some(value) = input
            .warning_status
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            where_parts.push("warning_status = ?".to_string());
            values.push(value.to_string());
        }

        let where_sql = format!("WHERE {}", where_parts.join(" AND "));
        let refs: Vec<&dyn rusqlite::types::ToSql> = values
            .iter()
            .map(|value| value as &dyn rusqlite::types::ToSql)
            .collect();
        let total: u32 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM devices {where_sql}"),
                refs.as_slice(),
                |row| row.get(0),
            )
            .map_err(|error| err_json("SYS_DB_QUERY", &error.to_string()))?;

        let sort_column = match input.sort_by.as_deref() {
            Some("rentalStatus") => "rentalStatus",
            Some("notes") => "notes",
            Some("warningStatus") => "warning_status",
            Some("createdAt") => "createdAt",
            _ => "serialNo",
        };
        let sort_order = if input.sort_order.as_deref() == Some("desc") {
            "DESC"
        } else {
            "ASC"
        };
        let offset = (input.page - 1) * input.page_size;
        let sql = format!(
            "SELECT serialNo, rentalStatus, COALESCE(notes, ''), warning_status, createdAt \
             FROM devices {where_sql} ORDER BY {sort_column} {sort_order}, serialNo ASC LIMIT ? OFFSET ?"
        );

        let mut boxed: Vec<Box<dyn rusqlite::types::ToSql>> = values
            .into_iter()
            .map(|value| Box::new(value) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        boxed.push(Box::new(input.page_size));
        boxed.push(Box::new(offset));
        let refs: Vec<&dyn rusqlite::types::ToSql> =
            boxed.iter().map(|value| value.as_ref()).collect();
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|error| err_json("SYS_DB_PREPARE", &error.to_string()))?;
        let rows = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(DevicePageRow {
                    serial_no: row.get(0)?,
                    rental_status: row.get(1)?,
                    notes: row.get(2)?,
                    warning_status: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|error| err_json("SYS_DB_QUERY", &error.to_string()))?;
        let devices = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| err_json("SYS_DB_QUERY", &error.to_string()))?;

        Ok(PagedDevices {
            devices,
            pagination: DevicePagination {
                page: input.page,
                page_size: input.page_size,
                total,
                total_pages: total.div_ceil(input.page_size),
            },
        })
    }

    fn do_get_device(
        &self,
        serial_no: &str,
        ctx: &ExecutionContext,
    ) -> Result<Option<Device>, String> {
        let conn = self.get_conn()?;

        let tenant_id = Self::scoped_tenant_id(ctx)?;
        let sql = "SELECT d.serialNo, d.modelId, dm.name as modelName, d.currentWarehouseId, w.name as warehouseName, \
                   d.rentalStatus, d.notes, d.createdAt, d.createdAt AS updatedAt \
                   FROM devices d \
                   LEFT JOIN device_models dm ON dm.id = d.modelId AND dm.tenant_id = d.tenant_id \
                   LEFT JOIN warehouses w ON w.id = d.currentWarehouseId AND w.tenant_id = d.tenant_id \
                   WHERE d.tenant_id = ? AND d.serialNo = ?";
        let params = [tenant_id.to_string(), serial_no.to_string()];

        let mut stmt = conn.prepare(sql).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_QUERY".into(),
                message: format!("准备查询失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;

        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

        let result = stmt
            .query_row(param_refs.as_slice(), |row| {
                Ok(Device {
                    serial_no: row.get(0)?,
                    model_id: row.get(1)?,
                    model_name: row.get(2)?,
                    warehouse_id: row.get(3)?,
                    warehouse_name: row.get(4)?,
                    status: row.get(5)?,
                    notes: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })
            .optional()
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

        Ok(result)
    }

    fn do_create_device(
        &self,
        input: &CreateDeviceInput,
        ctx: &ExecutionContext,
    ) -> Result<Device, String> {
        let conn = self.get_conn()?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now()
            .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let status = input.status.as_deref().unwrap_or("available");

        let tenant_id = Self::scoped_tenant_id(ctx)?;

        conn.execute(
            "INSERT INTO devices (id, serialNo, modelId, currentWarehouseId, rentalStatus, createdAt, tenant_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, input.serial_no, input.model_id, input.warehouse_id, status, now, tenant_id],
        )
        .map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_INSERT".into(),
                message: format!("创建设备失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;

        // 读取刚创建的记录以获取 JOIN 数据
        drop(conn);
        self.do_get_device(&input.serial_no, ctx)?.ok_or_else(|| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_INSERT".into(),
                message: "创建设备成功但读取失败".into(),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })
    }

    fn do_update_device(
        &self,
        input: &UpdateDeviceInput,
        ctx: &ExecutionContext,
    ) -> Result<Device, String> {
        let existing = self.do_get_device(&input.serial_no, ctx)?;
        if existing.is_none() {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NOT_FOUND".into(),
                message: format!("设备不存在: {}", input.serial_no),
                field: Some("serialNo".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let mut set_clauses: Vec<String> = Vec::new();
        let mut param_values: Vec<String> = Vec::new();

        if let Some(ref model_id) = input.model_id {
            set_clauses.push("modelId = ?".to_string());
            param_values.push(model_id.clone());
        }
        if let Some(ref warehouse_id) = input.warehouse_id {
            set_clauses.push("currentWarehouseId = ?".to_string());
            param_values.push(warehouse_id.clone());
        }
        if let Some(ref status) = input.status {
            set_clauses.push("rentalStatus = ?".to_string());
            param_values.push(status.clone());
        }

        if set_clauses.is_empty() {
            return self.do_get_device(&input.serial_no, ctx)?.ok_or_else(|| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_UPDATE".into(),
                    message: "更新失败".into(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            });
        }

        let conn = self.get_conn()?;
        let tenant_id = Self::scoped_tenant_id(ctx)?;
        param_values.push(tenant_id.to_string());
        param_values.push(input.serial_no.clone());
        let sql = format!(
            "UPDATE devices SET {} WHERE tenant_id = ? AND serialNo = ?",
            set_clauses.join(", ")
        );

        let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();

        let rows_affected = conn.execute(&sql, params_refs.as_slice()).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_UPDATE".into(),
                message: format!("更新设备失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;

        // 检查租户隔离
        if rows_affected == 0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NOT_FOUND".into(),
                message: format!("设备 {} 不存在或无权访问", input.serial_no),
                field: Some("serialNo".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        drop(conn);
        self.do_get_device(&input.serial_no, ctx)?.ok_or_else(|| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_UPDATE".into(),
                message: "更新成功但读取失败".into(),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })
    }

    fn do_delete_device(
        &self,
        serial_no: &str,
        ctx: &ExecutionContext,
    ) -> Result<DeleteOutput, String> {
        let conn = self.get_conn()?;

        let tenant_id = Self::scoped_tenant_id(ctx)?;
        let sql = "DELETE FROM devices WHERE tenant_id = ? AND serialNo = ?";
        let params = [tenant_id.to_string(), serial_no.to_string()];

        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();

        let affected = conn.execute(sql, params_refs.as_slice()).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_DELETE".into(),
                message: format!("删除设备失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;

        if affected == 0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NOT_FOUND".into(),
                message: format!("设备 {} 不存在或无权访问", serial_no),
                field: None,
                context: None,
            })
            .unwrap_or_default());
        }

        Ok(DeleteOutput { success: true })
    }

    fn do_resolve_serial(
        &self,
        input: &ResolveSerialInput,
        ctx: &ExecutionContext,
    ) -> Result<ResolveResult, String> {
        let normalized_input = &input.serial_no;

        // 获取所有序列号
        let conn = self.get_conn()?;
        let mut stmt = conn
            .prepare("SELECT serialNo FROM devices WHERE tenant_id = ?")
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("准备查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

        let all_serials: Vec<String> = stmt
            .query_map([Self::scoped_tenant_id(ctx)?], |row| row.get(0))
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?
            .filter_map(|r| r.ok())
            .collect();

        // 构建候选列表（所有已存在的序列号及其归一化形式）
        let candidates: Vec<DeviceMatchCandidate> = all_serials
            .iter()
            .map(|s| {
                let mut n = s.clone();
                sanitize_serial_no(&mut n);
                DeviceMatchCandidate {
                    serial_no: s.clone(),
                    normalized_serial_no: n,
                }
            })
            .collect();

        // 1. 精确匹配
        for c in &candidates {
            if c.serial_no == *normalized_input {
                return Ok(ResolveResult {
                    matched: true,
                    serial_no: Some(c.serial_no.clone()),
                    candidates,
                });
            }
        }

        // 2. 大小写不敏感匹配 (normalized)
        for c in &candidates {
            if c.normalized_serial_no == *normalized_input {
                return Ok(ResolveResult {
                    matched: true,
                    serial_no: Some(c.serial_no.clone()),
                    candidates,
                });
            }
        }

        // 3. 前缀匹配 (模糊)
        let prefix_matches: Vec<DeviceMatchCandidate> = candidates
            .iter()
            .filter(|c| {
                c.normalized_serial_no
                    .starts_with(normalized_input.as_str())
            })
            .cloned()
            .collect();

        if prefix_matches.len() == 1 {
            return Ok(ResolveResult {
                matched: true,
                serial_no: Some(prefix_matches[0].serial_no.clone()),
                candidates,
            });
        }

        // 无匹配或多匹配
        Ok(ResolveResult {
            matched: false,
            serial_no: None,
            candidates,
        })
    }

    fn do_get_match_candidates(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<Vec<DeviceMatchCandidate>, String> {
        let conn = self.get_conn()?;
        let mut stmt = conn
            .prepare("SELECT serialNo FROM devices WHERE tenant_id = ?")
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("准备查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

        let candidates: Vec<DeviceMatchCandidate> = stmt
            .query_map([Self::scoped_tenant_id(ctx)?], |row| {
                let raw: String = row.get(0)?;
                let mut normalized = raw.clone();
                sanitize_serial_no(&mut normalized);
                Ok(DeviceMatchCandidate {
                    serial_no: raw,
                    normalized_serial_no: normalized,
                })
            })
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(candidates)
    }

    fn do_validate_serial_no(&self, input: &ValidateSerialNoInput) -> ValidateSerialNoOutput {
        let valid = !input.serial_no.is_empty() && SERIAL_NO_REGEX.is_match(&input.serial_no);
        ValidateSerialNoOutput { valid }
    }
}

impl Default for FeatureDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureDevice {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "feature-device".into(),
            version: "0.1.0".into(),
            description: "设备管理模块 — 设备 CRUD + 序列号解析 + 模糊匹配".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "list_devices",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "list_devices_paged",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_device",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "create_device",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "update_device",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "delete_device",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "resolve_serial",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_match_candidates",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "validate_serial_no",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        let database_url = config
            .get("databaseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or(":memory:");
        let manager = SqliteConnectionManager::file(database_url);
        let pool = Pool::builder().max_size(4).build(manager).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_POOL".into(),
                message: format!("创建连接池失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        let mut guard = self.pool.lock().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_LOCK".into(),
                message: format!("获取连接池锁失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        *guard = Some(pool);
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "list_devices" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ListDevicesInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_list_devices(&input, ctx)?;
                serde_json::to_value(result).map_err(|e| {
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
            "list_devices_paged" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ListDevicesPagedInput> = payload.try_into()?;
                let input = unvalidated.sanitize().validate()?.into_inner();
                let result = self.do_list_devices_paged(&input, ctx)?;
                serde_json::to_value(result)
                    .map_err(|error| err_json("SYS_SERIALIZE", &error.to_string()))
            }
            "get_device" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<GetDeviceInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_get_device(&input.serial_no, ctx)?;
                serde_json::to_value(result).map_err(|e| {
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
            "create_device" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<CreateDeviceInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_create_device(&input, ctx)?;
                serde_json::to_value(result).map_err(|e| {
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
            "update_device" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<UpdateDeviceInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_update_device(&input, ctx)?;
                serde_json::to_value(result).map_err(|e| {
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
            "delete_device" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<DeleteDeviceInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_delete_device(&input.serial_no, ctx)?;
                serde_json::to_value(result).map_err(|e| {
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
            "resolve_serial" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ResolveSerialInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_resolve_serial(&input, ctx)?;
                serde_json::to_value(result).map_err(|e| {
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
            "get_match_candidates" => {
                let result = self.do_get_match_candidates(ctx)?;
                serde_json::to_value(result).map_err(|e| {
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
            "validate_serial_no" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ValidateSerialNoInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();
                let result = self.do_validate_serial_no(&input);
                serde_json::to_value(result).map_err(|e| {
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
            name: "feature-device".into(),
            description: "设备管理模块".into(),
            commands: vec![
                CommandSchema {
                    name: "list_devices".into(),
                    description: "列出设备，支持按状态/型号/仓库/序列号过滤".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "list_devices_paged".into(),
                    description: "Paginated tenant-scoped device query".into(),
                    version: "1.0.0".into(),
                    input_schema: Some(
                        serde_json::to_value(schemars::schema_for!(ListDevicesPagedInput))
                            .unwrap_or_default(),
                    ),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_device".into(),
                    description: "根据序列号获取单个设备".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "create_device".into(),
                    description: "创建设备".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "update_device".into(),
                    description: "更新设备信息".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "delete_device".into(),
                    description: "删除设备".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "resolve_serial".into(),
                    description: "序列号解析与模糊匹配".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_match_candidates".into(),
                    description: "获取所有序列号匹配候选".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "validate_serial_no".into(),
                    description: "验证序列号格式".into(),
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

    fn initialized_device_module() -> FeatureDevice {
        let mut module = FeatureDevice::new();
        module
            .init(serde_json::json!({ "databaseUrl": ":memory:" }))
            .expect("in-memory database initializes");
        let conn = module.get_conn().expect("database connection");
        conn.execute_batch(
            "CREATE TABLE devices (
                id TEXT PRIMARY KEY,
                serialNo TEXT NOT NULL,
                modelId TEXT NOT NULL,
                currentWarehouseId TEXT,
                rentalStatus TEXT NOT NULL,
                notes TEXT,
                warning_status TEXT NOT NULL DEFAULT '正常',
                createdAt TEXT NOT NULL,
                tenant_id TEXT NOT NULL
            );
            CREATE TABLE device_models (id TEXT PRIMARY KEY, name TEXT, tenant_id TEXT NOT NULL);
            CREATE TABLE warehouses (id TEXT PRIMARY KEY, name TEXT, tenant_id TEXT NOT NULL);
            INSERT INTO devices (id, serialNo, modelId, currentWarehouseId, rentalStatus, createdAt, tenant_id)
            VALUES ('device-a', 'ALPHA1', 'model-a', NULL, 'available', '2026-07-16', 'test-tenant');
            INSERT INTO devices (id, serialNo, modelId, currentWarehouseId, rentalStatus, createdAt, tenant_id)
            VALUES ('device-b', 'BRAVO2', 'model-b', NULL, 'available', '2026-07-16', 'other-tenant');",
        )
        .expect("device fixtures created");
        module
    }

    #[test]
    fn match_candidates_are_limited_to_execution_context_data_scope() {
        let module = initialized_device_module();
        let candidates = module
            .do_get_match_candidates(&crate::test_context())
            .expect("scoped candidates query succeeds");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].serial_no, "ALPHA1");
    }

    #[test]
    fn list_devices_uses_the_migrated_sqlite_column_contract() {
        let module = initialized_device_module();
        let devices = module
            .do_list_devices(
                &ListDevicesInput {
                    status: None,
                    model_id: None,
                    warehouse_id: None,
                    serial_no: None,
                },
                &crate::test_context(),
            )
            .expect("scoped device list succeeds against the migrated schema");

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].serial_no, "ALPHA1");
        assert_eq!(devices[0].status, "available");
        assert_eq!(devices[0].updated_at, "2026-07-16");
    }

    #[test]
    fn paged_device_query_is_bounded_and_tenant_scoped() {
        let module = initialized_device_module();
        let page = module
            .do_list_devices_paged(
                &ListDevicesPagedInput {
                    page: 1,
                    page_size: 1,
                    keyword: Some("ALPHA".into()),
                    rental_status: Some("available".into()),
                    notes: None,
                    warning_status: Some("正常".into()),
                    sort_by: Some("serialNo".into()),
                    sort_order: Some("asc".into()),
                },
                &crate::test_context(),
            )
            .expect("paged device query succeeds");

        assert_eq!(page.devices.len(), 1);
        assert_eq!(page.devices[0].serial_no, "ALPHA1");
        assert_eq!(page.pagination.total, 1);
        assert_eq!(page.pagination.page_size, 1);
        assert_eq!(page.pagination.total_pages, 1);
    }

    #[test]
    fn device_writes_use_the_migrated_sqlite_column_contract() {
        let module = initialized_device_module();
        let ctx = crate::test_context();
        let created = module
            .do_create_device(
                &CreateDeviceInput {
                    serial_no: "CHARLIE3".into(),
                    model_id: "model-c".into(),
                    warehouse_id: "warehouse-c".into(),
                    status: Some("available".into()),
                },
                &ctx,
            )
            .expect("create uses currentWarehouseId and rentalStatus");
        assert_eq!(created.status, "available");

        let updated = module
            .do_update_device(
                &UpdateDeviceInput {
                    serial_no: "CHARLIE3".into(),
                    model_id: None,
                    warehouse_id: None,
                    status: Some("rented".into()),
                },
                &ctx,
            )
            .expect("update does not require a nonexistent updatedAt column");
        assert_eq!(updated.status, "rented");
    }
}

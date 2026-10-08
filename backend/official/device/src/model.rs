//! 设备型号管理模块 — 型号 CRUD + 基础定价
//!
//! 封装 device_models + model_base_prices 两张表的操作。
//! 自包含：通过 init() 接收 Pool，不依赖 talos-backend。

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use system_core::*;

// ── 辅助 ──────────────────────────────────────────────────────────

fn shanghai_now_iso() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

// ── 输入类型 ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListModelsInput {
    /// 可选筛选关键字（当前版本保留但未使用）
    #[serde(default)]
    pub keyword: Option<String>,
}

impl Sanitize for ListModelsInput {
    fn sanitize(&mut self) {
        if let Some(ref mut kw) = self.keyword {
            *kw = kw.trim().to_string();
        }
    }
}

impl Validate for ListModelsInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetModelInput {
    pub id: String,
}

impl Sanitize for GetModelInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
    }
}

impl Validate for GetModelInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                message: "型号ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateModelInput {
    pub name: String,
    pub category: String,
    pub prefix: String,
    pub enabled: bool,
    #[serde(default)]
    pub weekday_price: Option<f64>,
    #[serde(default)]
    pub weekend_price: Option<f64>,
}

impl Sanitize for CreateModelInput {
    fn sanitize(&mut self) {
        self.name = self.name.trim().to_string();
        self.category = self.category.trim().to_string();
        self.prefix = self.prefix.trim().to_string();
    }
}

impl Validate for CreateModelInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.name.is_empty() {
            errors.push(FieldError {
                field: "name".into(),
                message: "型号名称不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if let Some(wp) = self.weekday_price
            && (wp <= 0.0 || !wp.is_finite())
        {
            errors.push(FieldError {
                field: "weekdayPrice".into(),
                message: "weekdayPrice 必须大于 0".into(),
                code: "VAL_INVALID".into(),
            });
        }
        if let Some(wep) = self.weekend_price
            && (wep <= 0.0 || !wep.is_finite())
        {
            errors.push(FieldError {
                field: "weekendPrice".into(),
                message: "weekendPrice 必须大于 0".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModelInput {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub weekday_price: Option<f64>,
    #[serde(default)]
    pub weekend_price: Option<f64>,
}

impl Sanitize for UpdateModelInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
        if let Some(ref mut n) = self.name {
            *n = n.trim().to_string();
        }
        if let Some(ref mut c) = self.category {
            *c = c.trim().to_string();
        }
        if let Some(ref mut p) = self.prefix {
            *p = p.trim().to_string();
        }
    }
}

impl Validate for UpdateModelInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                message: "型号ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if let Some(ref n) = self.name
            && n.is_empty()
        {
            errors.push(FieldError {
                field: "name".into(),
                message: "型号名称不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if let Some(wp) = self.weekday_price
            && (wp <= 0.0 || !wp.is_finite())
        {
            errors.push(FieldError {
                field: "weekdayPrice".into(),
                message: "weekdayPrice 必须大于 0".into(),
                code: "VAL_INVALID".into(),
            });
        }
        if let Some(wep) = self.weekend_price
            && (wep <= 0.0 || !wep.is_finite())
        {
            errors.push(FieldError {
                field: "weekendPrice".into(),
                message: "weekendPrice 必须大于 0".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteModelInput {
    pub id: String,
}

impl Sanitize for DeleteModelInput {
    fn sanitize(&mut self) {
        self.id = self.id.trim().to_string();
    }
}

impl Validate for DeleteModelInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id.is_empty() {
            errors.push(FieldError {
                field: "id".into(),
                message: "型号ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

// ── 输出类型 ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceModel {
    pub id: String,
    pub name: String,
    pub category: String,
    pub prefix: String,
    pub enabled: bool,
    pub weekday_price: Option<f64>,
    pub weekend_price: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteOutput {
    pub success: bool,
}

// ── 行映射 ────────────────────────────────────────────────────────

fn row_to_model(row: &rusqlite::Row) -> rusqlite::Result<DeviceModel> {
    Ok(DeviceModel {
        id: row.get(0)?,
        name: row.get(1)?,
        category: row.get(2)?,
        prefix: row.get(3)?,
        enabled: row.get::<_, i32>(4)? != 0,
        weekday_price: row.get::<_, Option<f64>>(5)?,
        weekend_price: row.get::<_, Option<f64>>(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

const MODEL_COLUMNS: &str = "dm.id, dm.name, dm.category, dm.prefix, dm.enabled, \
     mbp.weekdayPrice, mbp.weekendPrice, dm.createdAt, dm.updatedAt";

const MODEL_FROM_JOIN: &str = "FROM device_models dm LEFT JOIN model_base_prices mbp ON mbp.modelId = dm.id AND mbp.tenant_id = dm.tenant_id";

// ── 模块主体 ──────────────────────────────────────────────────────

pub struct FeatureModel {
    pub pool: std::sync::Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureModel {
    pub fn new() -> Self {
        Self {
            pool: std::sync::Mutex::new(None),
        }
    }

    fn conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_LOCK".into(),
                message: e.to_string(),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        guard
            .as_ref()
            .ok_or_else(|| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_NOT_INITIALIZED".into(),
                    message: "模块尚未初始化 Pool".into(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?
            .get()
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_POOL".into(),
                    message: e.to_string(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })
    }

    // ── 业务方法 ─────────────────────────────────────────────

    fn do_list_models(&self, scope: &DataScope) -> Result<Vec<DeviceModel>, String> {
        let conn = self.conn()?;
        let sql = format!(
            "SELECT {} {} WHERE dm.tenant_id = ?1 ORDER BY dm.createdAt ASC",
            MODEL_COLUMNS, MODEL_FROM_JOIN
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows: Vec<DeviceModel> = stmt
            .query_map(params![scope.tenant_id().as_str()], row_to_model)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    fn do_get_model(&self, scope: &DataScope, id: &str) -> Result<Option<DeviceModel>, String> {
        let conn = self.conn()?;
        let sql = format!(
            "SELECT {} {} WHERE dm.id = ?1 AND dm.tenant_id = ?2 LIMIT 1",
            MODEL_COLUMNS, MODEL_FROM_JOIN
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query_map(params![id, scope.tenant_id().as_str()], row_to_model)
            .map_err(|e| e.to_string())?;
        rows.next().transpose().map_err(|e| e.to_string())
    }

    fn do_create_model(
        &self,
        scope: &DataScope,
        input: &CreateModelInput,
    ) -> Result<DeviceModel, String> {
        let conn = self.conn()?;

        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM device_models WHERE name = ?1 AND tenant_id = ?2 LIMIT 1",
                params![input.name, scope.tenant_id().as_str()],
                |_| Ok(()),
            )
            .is_ok();
        if exists {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_DUPLICATE".into(),
                message: "型号名称已存在".into(),
                field: Some("name".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let now = shanghai_now_iso();
        let id = uuid::Uuid::new_v4().to_string();

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| e.to_string())?;

        let tx_result: Result<(), String> = (|| {
            conn.execute(
                "INSERT INTO device_models (id, name, category, prefix, enabled, createdAt, updatedAt, tenant_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    input.name,
                    input.category,
                    input.prefix,
                    input.enabled as i32,
                    now,
                    now,
                    scope.tenant_id().as_str()
                ],
            )
            .map_err(|e| e.to_string())?;

            if let (Some(wd), Some(we)) = (input.weekday_price, input.weekend_price) {
                conn.execute(
                    "INSERT INTO model_base_prices (modelId, weekdayPrice, weekendPrice, createdAt, updatedAt, tenant_id) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, wd, we, now, now, scope.tenant_id().as_str()],
                )
                .map_err(|e| e.to_string())?;
            }

            conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
            Ok(())
        })();

        if let Err(e) = tx_result {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }

        self.do_get_model(scope, &id)?
            .ok_or_else(|| "创建后查找型号失败".to_string())
    }

    fn do_update_model(
        &self,
        scope: &DataScope,
        input: &UpdateModelInput,
    ) -> Result<DeviceModel, String> {
        let conn = self.conn()?;

        let existing: bool = conn
            .query_row(
                "SELECT 1 FROM device_models WHERE id = ?1 AND tenant_id = ?2 LIMIT 1",
                params![input.id, scope.tenant_id().as_str()],
                |_| Ok(()),
            )
            .is_ok();
        if !existing {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NOT_FOUND".into(),
                message: "型号不存在".into(),
                field: Some("id".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let now = shanghai_now_iso();

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| e.to_string())?;

        let tx_result: Result<(), String> = (|| {
            let mut fields: Vec<String> = Vec::new();
            let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(ref name) = input.name {
                let dup: bool = conn
                    .query_row(
                        "SELECT 1 FROM device_models WHERE name = ?1 AND id <> ?2 AND tenant_id = ?3 LIMIT 1",
                        params![name, input.id, scope.tenant_id().as_str()],
                        |_| Ok(()),
                    )
                    .is_ok();
                if dup {
                    return Err(serde_json::to_string(&ErrorPayload {
                        category: "val".into(),
                        code: "VAL_DUPLICATE".into(),
                        message: "型号名称已存在".into(),
                        field: Some("name".into()),
                        context: None,
                    })
                    .unwrap_or_default());
                }
                fields.push("name = ?".to_string());
                values.push(Box::new(name.clone()));
            }
            if let Some(ref category) = input.category {
                fields.push("category = ?".to_string());
                values.push(Box::new(category.clone()));
            }
            if let Some(ref prefix) = input.prefix {
                fields.push("prefix = ?".to_string());
                values.push(Box::new(prefix.clone()));
            }
            if let Some(enabled) = input.enabled {
                fields.push("enabled = ?".to_string());
                values.push(Box::new(enabled as i32));
            }

            if !fields.is_empty() {
                fields.push("updatedAt = ?".to_string());
                values.push(Box::new(now.clone()));
                values.push(Box::new(input.id.clone()));
                values.push(Box::new(scope.tenant_id().as_str().to_owned()));
                let sql = format!(
                    "UPDATE device_models SET {} WHERE id = ? AND tenant_id = ?",
                    fields.join(", ")
                );
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    values.iter().map(|v| v.as_ref()).collect();
                conn.execute(&sql, rusqlite::params_from_iter(param_refs))
                    .map_err(|e| e.to_string())?;
            }

            if let (Some(wd), Some(we)) = (input.weekday_price, input.weekend_price) {
                let existing_price: bool = conn
                    .query_row(
                        "SELECT 1 FROM model_base_prices WHERE modelId = ?1 AND tenant_id = ?2 LIMIT 1",
                        params![input.id, scope.tenant_id().as_str()],
                        |_| Ok(()),
                    )
                    .is_ok();
                if existing_price {
                    conn.execute(
                        "UPDATE model_base_prices SET weekdayPrice = ?1, weekendPrice = ?2, updatedAt = ?3 WHERE modelId = ?4 AND tenant_id = ?5",
                        params![wd, we, now, input.id, scope.tenant_id().as_str()],
                    )
                    .map_err(|e| e.to_string())?;
                } else {
                    conn.execute(
                        "INSERT INTO model_base_prices (modelId, weekdayPrice, weekendPrice, createdAt, updatedAt, tenant_id) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![input.id, wd, we, now, now, scope.tenant_id().as_str()],
                    )
                    .map_err(|e| e.to_string())?;
                }
            }

            conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
            Ok(())
        })();

        if let Err(e) = tx_result {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(e);
        }

        self.do_get_model(scope, &input.id)?
            .ok_or_else(|| "更新后查找型号失败".to_string())
    }

    fn do_delete_model(&self, scope: &DataScope, id: &str) -> Result<DeleteOutput, String> {
        let conn = self.conn()?;

        let existing: bool = conn
            .query_row(
                "SELECT 1 FROM device_models WHERE id = ?1 AND tenant_id = ?2 LIMIT 1",
                params![id, scope.tenant_id().as_str()],
                |_| Ok(()),
            )
            .is_ok();
        if !existing {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NOT_FOUND".into(),
                message: "型号不存在".into(),
                field: Some("id".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let device_count: i64 = conn
            .query_row(
                "SELECT COUNT(1) AS cnt FROM devices WHERE modelId = ?1 AND tenant_id = ?2",
                params![id, scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;

        if device_count > 0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_REFERENCED".into(),
                message: format!("该型号已被 {} 台设备使用，无法删除", device_count),
                field: Some("id".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute(
            "DELETE FROM device_models WHERE id = ?1 AND tenant_id = ?2",
            params![id, scope.tenant_id().as_str()],
        )
        .map_err(|e| e.to_string())?;

        Ok(DeleteOutput { success: true })
    }
}

impl Default for FeatureModel {
    fn default() -> Self {
        Self::new()
    }
}

// ── SystemModule 实现 ─────────────────────────────────────────────

impl SystemModule for FeatureModel {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "model".into(),
            version: "0.1.0".into(),
            description: "设备型号管理模块 — 型号 CRUD + 基础定价".into(),
            author: "hoshi".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "list_models",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_model",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "create_model",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "update_model",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "delete_model",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        let database_url = config
            .get("databaseUrl")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_CONFIG".into(),
                    message: "config.databaseUrl 缺失".into(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

        let manager = SqliteConnectionManager::file(database_url);
        let pool = Pool::builder().max_size(5).build(manager).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_POOL".into(),
                message: e.to_string(),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;

        let mut guard = self.pool.lock().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_LOCK".into(),
                message: e.to_string(),
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
            "list_models" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ListModelsInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let _validated = sanitized.validate()?;

                let models = self.do_list_models(ctx.data_scope())?;
                serde_json::to_value(models).map_err(|e| {
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
            "get_model" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<GetModelInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                let model = self
                    .do_get_model(ctx.data_scope(), &input.id)?
                    .ok_or_else(|| {
                        serde_json::to_string(&ErrorPayload {
                            category: "val".into(),
                            code: "VAL_NOT_FOUND".into(),
                            message: "型号不存在".into(),
                            field: Some("id".into()),
                            context: None,
                        })
                        .unwrap_or_default()
                    })?;
                serde_json::to_value(model).map_err(|e| {
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
            "create_model" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<CreateModelInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                let model = self.do_create_model(ctx.data_scope(), &input)?;
                serde_json::to_value(model).map_err(|e| {
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
            "update_model" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<UpdateModelInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                let model = self.do_update_model(ctx.data_scope(), &input)?;
                serde_json::to_value(model).map_err(|e| {
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
            "delete_model" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<DeleteModelInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                let result = self.do_delete_model(ctx.data_scope(), &input.id)?;
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
            name: "model".into(),
            description: "设备型号管理模块 — 型号 CRUD + 基础定价".into(),
            commands: vec![
                CommandSchema {
                    name: "list_models".into(),
                    description: "列出全部设备型号".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_model".into(),
                    description: "按 ID 获取单个型号".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "create_model".into(),
                    description: "新建型号（含基础定价）".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "update_model".into(),
                    description: "更新型号字段 + 基础定价".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "delete_model".into(),
                    description: "删除型号（仅当无关联设备时）".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;

    fn initialized_module() -> FeatureModel {
        let module = FeatureModel::new();
        let pool = Pool::new(SqliteConnectionManager::memory()).expect("in-memory pool");
        pool.get()
            .expect("connection")
            .execute_batch(
                "CREATE TABLE device_models (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, category TEXT NOT NULL,
                    prefix TEXT NOT NULL, enabled INTEGER NOT NULL, createdAt TEXT NOT NULL,
                    updatedAt TEXT NOT NULL, tenant_id TEXT NOT NULL
                );
                CREATE TABLE model_base_prices (
                    modelId TEXT NOT NULL, weekdayPrice REAL, weekendPrice REAL,
                    createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL, tenant_id TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, modelId)
                );
                CREATE TABLE devices (modelId TEXT, tenant_id TEXT NOT NULL);
                INSERT INTO device_models VALUES
                    ('local', 'Shared', 'camera', 'L', 1, '', '', 'test-tenant'),
                    ('foreign', 'Foreign', 'camera', 'F', 1, '', '', 'other-tenant');",
            )
            .expect("fixtures");
        *module.pool.lock().expect("pool lock") = Some(pool);
        module
    }

    #[test]
    fn model_crud_is_limited_to_data_scope() {
        let module = initialized_module();
        let ctx = crate::test_context();
        let scope = ctx.data_scope();

        let listed = module.do_list_models(scope).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "local");
        assert!(
            module
                .do_get_model(scope, "foreign")
                .expect("get")
                .is_none()
        );

        let created = module
            .do_create_model(
                scope,
                &CreateModelInput {
                    name: "Tenant Local".into(),
                    category: "camera".into(),
                    prefix: "TL".into(),
                    enabled: true,
                    weekday_price: None,
                    weekend_price: None,
                },
            )
            .expect("create");
        let tenant: String = module
            .conn()
            .expect("connection")
            .query_row(
                "SELECT tenant_id FROM device_models WHERE id = ?1",
                params![created.id],
                |row| row.get(0),
            )
            .expect("tenant");
        assert_eq!(tenant, "test-tenant");

        assert!(module.do_delete_model(scope, "foreign").is_err());
        let foreign_count: i64 = module
            .conn()
            .expect("connection")
            .query_row(
                "SELECT COUNT(*) FROM device_models WHERE id = 'foreign'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(foreign_count, 1);
    }
}

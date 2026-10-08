//! feature-contract — 合同/电子签系统模块（Maxwell 原生）
//!
//! 租赁合同模板管理 + 合同生成/签署/验证。
//!
//! ## 命令
//! - `template_create` — 创建合同模板（AdminUser）
//! - `template_update` — 更新合同模板（AdminUser）
//! - `template_list` — 模板列表（AuthUser）
//! - `template_get` — 获取模板详情（AuthUser）
//! - `contract_generate` — 生成合同（AuthUser，设备价值>¥15k 自动触发）
//! - `contract_sign` — 记录电子签名（AuthUser）
//! - `contract_verify` — 验证合同（AuthUser）
//! - `contract_list` — 合同列表（AuthUser）
//! - `contract_get` — 合同详情（AuthUser）

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

fn auto_trigger_threshold() -> f64 {
    15000.0
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Template
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateCreateInput {
    pub name: String,
    pub content_json: Value,
}

impl Validate for TemplateCreateInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.name.is_empty() {
            errors.push(FieldError {
                field: "name".into(),
                message: "模板名称不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for TemplateCreateInput {
    fn sanitize(&mut self) {
        self.name = self.name.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateUpdateInput {
    pub id: i64,
    pub name: Option<String>,
    pub content_json: Option<Value>,
    pub is_active: Option<bool>,
}

impl Validate for TemplateUpdateInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "模板ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if let Some(ref name) = self.name
            && name.is_empty()
        {
            errors.push(FieldError {
                field: "name".into(),
                message: "模板名称不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for TemplateUpdateInput {
    fn sanitize(&mut self) {
        if let Some(ref mut name) = self.name {
            *name = name.trim().to_string();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateGetInput {
    pub id: i64,
}

impl Validate for TemplateGetInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "模板ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for TemplateGetInput {
    fn sanitize(&mut self) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateListInput {
    pub is_active: Option<bool>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl Validate for TemplateListInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for TemplateListInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Contract
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractGenerateInput {
    pub order_id: String,
    pub template_id: Option<i64>,
    pub customer_name: String,
    pub customer_phone: String,
    pub device_value: f64,
    pub variables: Option<Value>,
}

impl Validate for ContractGenerateInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.order_id.trim().is_empty() {
            errors.push(FieldError {
                field: "orderId".into(),
                message: "订单ID不能为空".into(),
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
        if !self.device_value.is_finite() || self.device_value < 0.0 {
            errors.push(FieldError {
                field: "deviceValue".into(),
                message: "设备价值必须是非负数值".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ContractGenerateInput {
    fn sanitize(&mut self) {
        self.order_id = self.order_id.trim().to_string();
        self.customer_name = self.customer_name.trim().to_string();
        self.customer_phone = self.customer_phone.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractSignInput {
    pub id: i64,
    pub signer_name: String,
    pub signer_phone: String,
    pub signature_data: String,
}

impl Validate for ContractSignInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "合同ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.signer_name.is_empty() {
            errors.push(FieldError {
                field: "signerName".into(),
                message: "签署人姓名不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.signer_phone.is_empty() {
            errors.push(FieldError {
                field: "signerPhone".into(),
                message: "签署人电话不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.signature_data.is_empty() {
            errors.push(FieldError {
                field: "signatureData".into(),
                message: "签名数据不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ContractSignInput {
    fn sanitize(&mut self) {
        self.signer_name = self.signer_name.trim().to_string();
        self.signer_phone = self.signer_phone.trim().to_string();
        self.signature_data = self.signature_data.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractVerifyInput {
    pub id: i64,
}

impl Validate for ContractVerifyInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "合同ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ContractVerifyInput {
    fn sanitize(&mut self) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractListInput {
    pub order_id: Option<String>,
    pub customer_phone: Option<String>,
    pub status: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl Validate for ContractListInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for ContractListInput {
    fn sanitize(&mut self) {
        if let Some(ref mut s) = self.order_id {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.customer_phone {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.status {
            *s = s.trim().to_string();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractGetInput {
    pub id: i64,
}

impl Validate for ContractGetInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "合同ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ContractGetInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureContract {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureContract {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureContract {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureContract {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "contract".into(),
            version: "0.1.0".into(),
            description: "合同/电子签系统 — 模板管理/合同生成/电子签名/合同验证".into(),
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
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let pool_guard = self.pool.lock().map_err(|e| e.to_string())?;
        let pool = pool_guard.as_ref().ok_or_else(|| {
            err(
                "sys",
                "SYS_NOT_INIT",
                "FeatureContract not initialized".into(),
            )
        })?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;

        match command {
            // Templates
            "template_create" => {
                require_admin(_ctx)?;
                let unvalidated: system_core::Unvalidated<TemplateCreateInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                template_create(&conn, _ctx.data_scope(), &validated.into_inner())
            }
            "template_update" => {
                require_admin(_ctx)?;
                let unvalidated: system_core::Unvalidated<TemplateUpdateInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                template_update(&conn, _ctx.data_scope(), &validated.into_inner())
            }
            "template_list" => {
                let unvalidated: system_core::Unvalidated<TemplateListInput> =
                    payload.try_into()?;
                template_list(
                    &conn,
                    _ctx.data_scope(),
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "template_get" => {
                let unvalidated: system_core::Unvalidated<TemplateGetInput> = payload.try_into()?;
                template_get(
                    &conn,
                    _ctx.data_scope(),
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            // Contracts
            "contract_generate" => {
                let unvalidated: system_core::Unvalidated<ContractGenerateInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                contract_generate(&conn, _ctx.data_scope(), &validated.into_inner())
            }
            "contract_sign" => {
                let unvalidated: system_core::Unvalidated<ContractSignInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                contract_sign(&conn, _ctx.data_scope(), &validated.into_inner())
            }
            "contract_verify" => {
                let unvalidated: system_core::Unvalidated<ContractVerifyInput> =
                    payload.try_into()?;
                contract_verify(
                    &conn,
                    _ctx.data_scope(),
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "contract_list" => {
                let unvalidated: system_core::Unvalidated<ContractListInput> =
                    payload.try_into()?;
                contract_list(
                    &conn,
                    _ctx.data_scope(),
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "contract_get" => {
                let unvalidated: system_core::Unvalidated<ContractGetInput> = payload.try_into()?;
                contract_get(
                    &conn,
                    _ctx.data_scope(),
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
                "template_create",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "template_update",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "template_list",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "template_get",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "contract_generate",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "contract_sign",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "contract_verify",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "contract_list",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "contract_get",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "contract".into(),
            description: "合同/电子签系统模块".into(),
            commands: vec![
                CommandSchema {
                    name: "template_create".into(),
                    description: "创建合同模板".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "template_update".into(),
                    description: "更新合同模板".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "template_list".into(),
                    description: "模板列表".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "template_get".into(),
                    description: "模板详情".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "contract_generate".into(),
                    description: "生成合同".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "contract_sign".into(),
                    description: "电子签名".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "contract_verify".into(),
                    description: "验证合同".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "contract_list".into(),
                    description: "合同列表".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "contract_get".into(),
                    description: "合同详情".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Template implementations
// ══════════════════════════════════════════════════════════════════════════

fn template_create(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &TemplateCreateInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let now = shanghai_now();
    let content_str = serde_json::to_string(&input.content_json)
        .map_err(|e| err("val", "VAL_JSON", e.to_string()))?;

    conn.execute(
        "INSERT INTO contract_templates (name, content_json, is_active, created_at, tenant_id) VALUES (?1, ?2, 1, ?3, ?4)",
        params![input.name, content_str, now, scope.tenant_id().as_str()],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    let id = conn.last_insert_rowid();
    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": id, "createdAt": now }))
}

fn template_update(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &TemplateUpdateInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Verify template exists (within transaction to prevent TOCTOU)
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM contract_templates WHERE id = ?1 AND tenant_id = ?2",
            params![input.id, scope.tenant_id().as_str()],
            |_| Ok(()),
        )
        .is_ok();

    if !exists {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err("biz", "BIZ_NOT_FOUND", "模板不存在".into()));
    }

    let now = shanghai_now();

    if let Some(ref name) = input.name {
        conn.execute(
            "UPDATE contract_templates SET name = ?1, updated_at = ?2 WHERE id = ?3 AND tenant_id = ?4",
            params![name, now, input.id, scope.tenant_id().as_str()],
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_UPDATE", e.to_string())
        })?;
    }

    if let Some(ref content) = input.content_json {
        let content_str =
            serde_json::to_string(content).map_err(|e| err("val", "VAL_JSON", e.to_string()))?;
        conn.execute(
            "UPDATE contract_templates SET content_json = ?1, updated_at = ?2 WHERE id = ?3 AND tenant_id = ?4",
            params![content_str, now, input.id, scope.tenant_id().as_str()],
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_UPDATE", e.to_string())
        })?;
    }

    if let Some(is_active) = input.is_active {
        conn.execute(
            "UPDATE contract_templates SET is_active = ?1, updated_at = ?2 WHERE id = ?3 AND tenant_id = ?4",
            params![is_active as i32, now, input.id, scope.tenant_id().as_str()],
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_UPDATE", e.to_string())
        })?;
    }

    // Always update the timestamp
    conn.execute(
        "UPDATE contract_templates SET updated_at = ?1 WHERE id = ?2 AND tenant_id = ?3",
        params![now, input.id, scope.tenant_id().as_str()],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": input.id, "updatedAt": now }))
}

fn template_list(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &TemplateListInput,
) -> Result<Value, String> {
    let page = input.page.unwrap_or(1).max(1);
    let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let filter_sql = match input.is_active {
        Some(true) => "WHERE tenant_id = ?1 AND is_active = 1",
        Some(false) => "WHERE tenant_id = ?1 AND is_active = 0",
        None => "WHERE tenant_id = ?1",
    };

    let count_sql = format!("SELECT COUNT(*) FROM contract_templates {}", filter_sql);
    let total: i64 = conn
        .query_row(&count_sql, params![scope.tenant_id().as_str()], |row| {
            row.get(0)
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let query_sql = format!(
        "SELECT id, name, content_json, is_active, created_at FROM contract_templates {} ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
        filter_sql
    );

    let mut stmt = conn
        .prepare(&query_sql)
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
    let items: Vec<Value> = stmt
        .query_map(
            params![scope.tenant_id().as_str(), page_size, offset],
            |row| {
                let content_str: String = row.get(2)?;
                let content_json: Value = serde_json::from_str(&content_str).unwrap_or_default();
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "contentJson": content_json,
                    "isActive": row.get::<_, i32>(3)? == 1,
                    "createdAt": row.get::<_, String>(4)?,
                }))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(
        serde_json::json!({ "ok": true, "items": items, "total": total, "page": page, "pageSize": page_size }),
    )
}

fn template_get(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &TemplateGetInput,
) -> Result<Value, String> {
    let record = conn
        .query_row(
            "SELECT id, name, content_json, is_active, created_at FROM contract_templates WHERE id = ?1 AND tenant_id = ?2",
            params![input.id, scope.tenant_id().as_str()],
            |row| {
                let content_str: String = row.get(2)?;
                let content_json: Value = serde_json::from_str(&content_str).unwrap_or_default();
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "contentJson": content_json,
                    "isActive": row.get::<_, i32>(3)? == 1,
                    "createdAt": row.get::<_, String>(4)?,
                }))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "record": record }))
}

// ══════════════════════════════════════════════════════════════════════════
// Contract implementations
// ══════════════════════════════════════════════════════════════════════════

fn render_template(template_json: &Value, variables: &Value) -> String {
    let mut rendered = serde_json::to_string(template_json).unwrap_or_default();

    if let Some(obj) = variables.as_object() {
        for (key, val) in obj {
            let placeholder = format!("{{{}}}", key);
            let raw = match val {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            // HTML-escape to prevent stored XSS in contract previews
            let escaped = raw
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&#x27;");
            rendered = rendered.replace(&placeholder, &escaped);
        }
    }
    rendered
}

fn contract_generate(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ContractGenerateInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // If template_id is provided, load the template; otherwise use default template (id=1)
    let template_id = input.template_id.unwrap_or(1);

    let template_row: Result<(String, String), _> = conn.query_row(
        "SELECT name, content_json FROM contract_templates WHERE id = ?1 AND tenant_id = ?2 AND is_active = 1",
        params![template_id, scope.tenant_id().as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    );

    let (_template_name, template_content_str) = match template_row {
        Ok(row) => row,
        Err(_) => {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err("biz", "BIZ_NOT_FOUND", "合同模板不存在或已禁用".into()));
        }
    };

    let template_json: Value = serde_json::from_str(&template_content_str).unwrap_or_default();
    let variables = input.variables.clone().unwrap_or_default();

    // Render template with variables
    let rendered_content = render_template(&template_json, &variables);
    let content_json = serde_json::json!({
        "templateId": template_id,
        "renderedContent": rendered_content,
        "variables": variables,
    });

    let content_str =
        serde_json::to_string(&content_json).map_err(|e| err("val", "VAL_JSON", e.to_string()))?;

    let now = shanghai_now();
    let auto_triggered = input.device_value > auto_trigger_threshold();
    let initial_status = if auto_triggered { "generated" } else { "draft" };

    let inserted = conn.execute(
        "INSERT INTO contracts (order_id, template_id, customer_name, customer_phone, device_value, content_json, status, created_at, updated_at, tenant_id)
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9 FROM orders WHERE id = ?1 AND tenant_id = ?9",
        params![
            input.order_id,
            template_id,
            input.customer_name,
            input.customer_phone,
            input.device_value,
            content_str,
            initial_status,
            now,
            scope.tenant_id().as_str()
        ],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    if inserted == 0 {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err("biz", "BIZ_NOT_FOUND", "订单不属于当前租户".into()));
    }

    let contract_id = conn.last_insert_rowid();
    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true,
        "id": contract_id,
        "status": initial_status,
        "autoTriggered": auto_triggered,
        "createdAt": now,
    }))
}

fn contract_sign(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ContractSignInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Verify contract exists and is in signable state (within transaction to prevent TOCTOU)
    let current_status: String = conn
        .query_row(
            "SELECT status FROM contracts WHERE id = ?1 AND tenant_id = ?2",
            params![input.id, scope.tenant_id().as_str()],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if current_status != "draft" && current_status != "generated" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_STATUS_INVALID",
            format!("当前状态 '{}' 不可签署", current_status),
        ));
    }

    let now = shanghai_now();

    // Insert e-signature record
    conn.execute(
        "INSERT INTO e_signatures (contract_id, signer_name, signer_phone, signature_data, signed_at, tenant_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            input.id,
            input.signer_name,
            input.signer_phone,
            input.signature_data,
            now,
            scope.tenant_id().as_str()
        ],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    // Update contract status to signed
    conn.execute(
        "UPDATE contracts SET status = 'signed', signed_at = ?1, updated_at = ?1 WHERE id = ?2 AND tenant_id = ?3",
        params![now, input.id, scope.tenant_id().as_str()],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": input.id, "status": "signed", "signedAt": now }))
}

fn contract_verify(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ContractVerifyInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Verify contract exists and is signed (within transaction to prevent TOCTOU)
    let current_status: String = conn
        .query_row(
            "SELECT status FROM contracts WHERE id = ?1 AND tenant_id = ?2",
            params![input.id, scope.tenant_id().as_str()],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if current_status != "signed" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_STATUS_INVALID",
            format!("当前状态 '{}' 不可验证，需要已签署状态", current_status),
        ));
    }

    // Check that at least one signature exists
    let sig_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM e_signatures WHERE contract_id = ?1 AND tenant_id = ?2",
            params![input.id, scope.tenant_id().as_str()],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if sig_count == 0 {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err("biz", "BIZ_NO_SIGNATURE", "合同无有效签名记录".into()));
    }

    let now = shanghai_now();
    conn.execute(
        "UPDATE contracts SET status = 'verified', updated_at = ?1 WHERE id = ?2 AND tenant_id = ?3",
        params![now, input.id, scope.tenant_id().as_str()],
    )
    .map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": input.id, "status": "verified", "verifiedAt": now }))
}

fn contract_list(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ContractListInput,
) -> Result<Value, String> {
    let page = input.page.unwrap_or(1).max(1);
    let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    // Parameterized WHERE clauses
    let mut conditions: Vec<String> = vec!["c.tenant_id = ?1".into()];
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(scope.tenant_id().as_str().to_owned())];

    if let Some(ref order_id) = input.order_id {
        let idx = conditions.len() + 1;
        conditions.push(format!("c.order_id = ?{}", idx));
        all_params.push(Box::new(order_id.clone()));
    }
    if let Some(ref phone) = input.customer_phone {
        let idx = conditions.len() + 1;
        conditions.push(format!("c.customer_phone = ?{}", idx));
        all_params.push(Box::new(phone.clone()));
    }
    if let Some(ref status) = input.status {
        let idx = conditions.len() + 1;
        conditions.push(format!("c.status = ?{}", idx));
        all_params.push(Box::new(status.clone()));
    }

    let where_sql = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM contracts c {}", where_sql);
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
        "SELECT c.id, c.order_id, c.template_id, c.customer_name, c.customer_phone, c.device_value, c.content_json, c.status, c.signed_at, c.created_at, c.updated_at
         FROM contracts c {} ORDER BY c.created_at DESC LIMIT ?{} OFFSET ?{}",
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
            let content_str: String = row.get(6)?;
            let content_json: Value = serde_json::from_str(&content_str).unwrap_or_default();
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "orderId": row.get::<_, i64>(1)?,
                "templateId": row.get::<_, Option<i64>>(2)?,
                "customerName": row.get::<_, String>(3)?,
                "customerPhone": row.get::<_, String>(4)?,
                "deviceValue": row.get::<_, f64>(5)?,
                "contentJson": content_json,
                "status": row.get::<_, String>(7)?,
                "signedAt": row.get::<_, Option<String>>(8)?,
                "createdAt": row.get::<_, String>(9)?,
                "updatedAt": row.get::<_, String>(10)?,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(
        serde_json::json!({ "ok": true, "items": items, "total": total, "page": page, "pageSize": page_size }),
    )
}

fn contract_get(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ContractGetInput,
) -> Result<Value, String> {
    let record = conn
        .query_row(
            "SELECT id, order_id, template_id, customer_name, customer_phone, device_value, content_json, status, signed_at, created_at, updated_at
             FROM contracts WHERE id = ?1 AND tenant_id = ?2",
            params![input.id, scope.tenant_id().as_str()],
            |row| {
                let content_str: String = row.get(6)?;
                let content_json: Value = serde_json::from_str(&content_str).unwrap_or_default();
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "orderId": row.get::<_, i64>(1)?,
                    "templateId": row.get::<_, Option<i64>>(2)?,
                    "customerName": row.get::<_, String>(3)?,
                    "customerPhone": row.get::<_, String>(4)?,
                    "deviceValue": row.get::<_, f64>(5)?,
                    "contentJson": content_json,
                    "status": row.get::<_, String>(7)?,
                    "signedAt": row.get::<_, Option<String>>(8)?,
                    "createdAt": row.get::<_, String>(9)?,
                    "updatedAt": row.get::<_, String>(10)?,
                }))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    // Also fetch signatures for this contract
    let mut sig_stmt = conn
        .prepare(
            "SELECT id, signer_name, signer_phone, signature_data, signed_at
             FROM e_signatures WHERE contract_id = ?1 AND tenant_id = ?2 ORDER BY signed_at ASC",
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let signatures: Vec<Value> = sig_stmt
        .query_map(params![input.id, scope.tenant_id().as_str()], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "signerName": row.get::<_, String>(1)?,
                "signerPhone": row.get::<_, String>(2)?,
                "signatureData": row.get::<_, String>(3)?,
                "signedAt": row.get::<_, String>(4)?,
            }))
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(serde_json::json!({ "ok": true, "record": record, "signatures": signatures }))
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;
    use system_core::{Revision, TenantId};

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(TenantId::new(tenant).unwrap(), Revision::new("r1").unwrap()).unwrap()
    }

    #[test]
    fn contract_list_and_get_do_not_cross_tenants() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE contracts (
                id INTEGER PRIMARY KEY, order_id INTEGER, template_id INTEGER,
                customer_name TEXT, customer_phone TEXT, device_value REAL,
                content_json TEXT, status TEXT, signed_at TEXT,
                created_at TEXT, updated_at TEXT, tenant_id TEXT
             );
             CREATE TABLE e_signatures (
                id INTEGER PRIMARY KEY, contract_id INTEGER, signer_name TEXT,
                signer_phone TEXT, signature_data TEXT, signed_at TEXT, tenant_id TEXT
             );
             INSERT INTO contracts VALUES
                (1, 10, NULL, 'A', '1', 1, '{}', 'draft', NULL, '2026-01-01', '2026-01-01', 'tenant-a'),
                (2, 20, NULL, 'B', '2', 2, '{}', 'draft', NULL, '2026-01-01', '2026-01-01', 'tenant-b');",
        )
        .unwrap();

        let listed = contract_list(
            &conn,
            &scope("tenant-a"),
            &ContractListInput {
                order_id: None,
                customer_phone: None,
                status: None,
                page: None,
                page_size: None,
            },
        )
        .unwrap();
        assert_eq!(listed["total"], 1);
        assert_eq!(listed["items"][0]["id"], 1);

        assert!(contract_get(&conn, &scope("tenant-a"), &ContractGetInput { id: 2 }).is_err());
    }
}

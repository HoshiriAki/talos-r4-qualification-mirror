//! feature-credit — 客户信用系统模块（Maxwell 原生）
//!
//! 黑名单、违规记录、信用评分三个子模块。
//!
//! ## 命令
//! - `blacklist_add` — 添加黑名单（AdminUser）
//! - `blacklist_remove` — 移除黑名单（AdminUser）
//! - `blacklist_check` — 检查客户是否在黑名单（AuthUser）
//! - `blacklist_list` — 分页列表（AuthUser）
//! - `violation_record` — 记录违规（AuthUser）
//! - `violation_appeal` — 客户申诉（AuthUser）
//! - `violation_review` — 审核申诉（AdminUser）
//! - `violation_list` — 分页列表（AuthUser）
//! - `violation_get` — 单条查询（AuthUser）
//! - `credit_get` — 获取信用评分（AuthUser）
//! - `credit_history` — 评分变动历史（AuthUser）
//! - `credit_recalculate` — 手动重算（AdminUser）
//! - `check_before_order` — 下单前检查（AuthUser）

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
// Input types — Blacklist
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlacklistAddInput {
    pub customer_name: String,
    pub customer_phone: String,
    pub id_number: Option<String>,
    pub reason: String,
    pub severity: String,
}

impl Validate for BlacklistAddInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
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
        if self.reason.is_empty() {
            errors.push(FieldError {
                field: "reason".into(),
                message: "拉黑原因不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if !matches!(
            self.severity.as_str(),
            "low" | "medium" | "high" | "critical"
        ) {
            errors.push(FieldError {
                field: "severity".into(),
                message: "必须是 low / medium / high / critical".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for BlacklistAddInput {
    fn sanitize(&mut self) {
        self.customer_name = self.customer_name.trim().to_string();
        self.customer_phone = self.customer_phone.trim().to_string();
        if let Some(ref mut id) = self.id_number {
            *id = id.trim().to_string();
        }
        self.reason = self.reason.trim().to_string();
        self.severity = self.severity.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlacklistRemoveInput {
    pub id: i64,
    pub removal_reason: String,
}

impl Validate for BlacklistRemoveInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "黑名单ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.removal_reason.is_empty() {
            errors.push(FieldError {
                field: "removalReason".into(),
                message: "移除原因不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for BlacklistRemoveInput {
    fn sanitize(&mut self) {
        self.removal_reason = self.removal_reason.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlacklistCheckInput {
    pub customer_name: String,
    pub customer_phone: String,
}

impl Validate for BlacklistCheckInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
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

impl Sanitize for BlacklistCheckInput {
    fn sanitize(&mut self) {
        self.customer_name = self.customer_name.trim().to_string();
        self.customer_phone = self.customer_phone.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlacklistListInput {
    pub is_active: Option<bool>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl Validate for BlacklistListInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for BlacklistListInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Violation
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViolationRecordInput {
    pub customer_name: String,
    pub customer_phone: String,
    pub order_id: Option<i64>,
    pub violation_type: String,
    pub severity: String,
    pub description: String,
    pub evidence: Option<String>,
    pub financial_penalty: Option<f64>,
}

impl Validate for ViolationRecordInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
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
        if !matches!(
            self.violation_type.as_str(),
            "late_return" | "damage" | "lost_device" | "fake_info" | "payment_default" | "other"
        ) {
            errors.push(FieldError {
                field: "violationType".into(),
                message: "违规类型无效".into(),
                code: "VAL_INVALID".into(),
            });
        }
        if !matches!(
            self.severity.as_str(),
            "minor" | "moderate" | "major" | "critical"
        ) {
            errors.push(FieldError {
                field: "severity".into(),
                message: "严重程度无效".into(),
                code: "VAL_INVALID".into(),
            });
        }
        if self.description.is_empty() {
            errors.push(FieldError {
                field: "description".into(),
                message: "违规描述不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if let Some(p) = self.financial_penalty
            && (!p.is_finite() || !(0.0..=1_000_000.0).contains(&p))
        {
            errors.push(FieldError {
                field: "financialPenalty".into(),
                message: "罚款金额必须在 0-1000000 之间".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ViolationRecordInput {
    fn sanitize(&mut self) {
        self.customer_name = self.customer_name.trim().to_string();
        self.customer_phone = self.customer_phone.trim().to_string();
        self.violation_type = self.violation_type.trim().to_string();
        self.severity = self.severity.trim().to_string();
        self.description = self.description.trim().to_string();
        if let Some(ref mut ev) = self.evidence {
            *ev = ev.trim().to_string();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViolationAppealInput {
    pub id: i64,
    pub appeal_reason: String,
}

impl Validate for ViolationAppealInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "违规ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.appeal_reason.is_empty() {
            errors.push(FieldError {
                field: "appealReason".into(),
                message: "申诉原因不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ViolationAppealInput {
    fn sanitize(&mut self) {
        self.appeal_reason = self.appeal_reason.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViolationReviewInput {
    pub id: i64,
    pub status: String,
    pub review_notes: String,
}

impl Validate for ViolationReviewInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "违规ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if !matches!(self.status.as_str(), "upheld" | "dismissed") {
            errors.push(FieldError {
                field: "status".into(),
                message: "必须是 upheld 或 dismissed".into(),
                code: "VAL_INVALID".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ViolationReviewInput {
    fn sanitize(&mut self) {
        self.status = self.status.trim().to_string();
        self.review_notes = self.review_notes.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViolationListInput {
    pub customer_phone: Option<String>,
    pub status: Option<String>,
    pub violation_type: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl Validate for ViolationListInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for ViolationListInput {
    fn sanitize(&mut self) {
        if let Some(ref mut s) = self.customer_phone {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.status {
            *s = s.trim().to_string();
        }
        if let Some(ref mut s) = self.violation_type {
            *s = s.trim().to_string();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViolationGetInput {
    pub id: i64,
}

impl Validate for ViolationGetInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.id <= 0 {
            errors.push(FieldError {
                field: "id".into(),
                message: "违规ID无效".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ViolationGetInput {
    fn sanitize(&mut self) {}
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Credit
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditGetInput {
    pub customer_phone: String,
}

impl Validate for CreditGetInput {
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

impl Sanitize for CreditGetInput {
    fn sanitize(&mut self) {
        self.customer_phone = self.customer_phone.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditHistoryInput {
    pub customer_phone: String,
}

impl Validate for CreditHistoryInput {
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

impl Sanitize for CreditHistoryInput {
    fn sanitize(&mut self) {
        self.customer_phone = self.customer_phone.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditRecalculateInput {
    pub customer_phone: String,
}

impl Validate for CreditRecalculateInput {
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

impl Sanitize for CreditRecalculateInput {
    fn sanitize(&mut self) {
        self.customer_phone = self.customer_phone.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Input types — Pre-order check
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckBeforeOrderInput {
    pub customer_name: String,
    pub customer_phone: String,
}

impl Validate for CheckBeforeOrderInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
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

impl Sanitize for CheckBeforeOrderInput {
    fn sanitize(&mut self) {
        self.customer_name = self.customer_name.trim().to_string();
        self.customer_phone = self.customer_phone.trim().to_string();
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Module
// ══════════════════════════════════════════════════════════════════════════

pub struct FeatureCredit {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureCredit {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }
}

impl Default for FeatureCredit {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureCredit {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "credit".into(),
            version: "0.1.0".into(),
            description: "客户信用系统 — 黑名单/违规记录/信用评分".into(),
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
                "FeatureCredit not initialized".into(),
            )
        })?;
        let conn = pool
            .get()
            .map_err(|e| err("sys", "DB_CONN", e.to_string()))?;
        let scope = _ctx.data_scope();

        match command {
            // Blacklist
            "blacklist_add" => {
                require_admin(_ctx)?;
                let unvalidated: system_core::Unvalidated<BlacklistAddInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let admin_id = _ctx
                    .user_id()
                    .and_then(|id| id.parse::<i64>().ok())
                    .ok_or_else(|| err("auth", "AUTH_REQUIRED", "需要管理员身份".into()))?;
                blacklist_add(&conn, scope, &input, admin_id)
            }
            "blacklist_remove" => {
                require_admin(_ctx)?;
                let unvalidated: system_core::Unvalidated<BlacklistRemoveInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let admin_id = _ctx
                    .user_id()
                    .and_then(|id| id.parse::<i64>().ok())
                    .ok_or_else(|| err("auth", "AUTH_REQUIRED", "需要管理员身份".into()))?;
                blacklist_remove(&conn, scope, &input, admin_id)
            }
            "blacklist_check" => {
                let unvalidated: system_core::Unvalidated<BlacklistCheckInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                blacklist_check(&conn, scope, &validated.into_inner())
            }
            "blacklist_list" => {
                let unvalidated: system_core::Unvalidated<BlacklistListInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                blacklist_list(&conn, scope, &validated.into_inner())
            }
            // Violation
            "violation_record" => {
                let unvalidated: system_core::Unvalidated<ViolationRecordInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let reported_by = _ctx
                    .user_id()
                    .and_then(|id| id.parse::<i64>().ok())
                    .ok_or_else(|| err("auth", "AUTH_REQUIRED", "需要登录".into()))?;
                violation_record(&conn, scope, &input, reported_by)
            }
            "violation_appeal" => {
                let unvalidated: system_core::Unvalidated<ViolationAppealInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                violation_appeal(&conn, scope, &validated.into_inner())
            }
            "violation_review" => {
                require_admin(_ctx)?;
                let unvalidated: system_core::Unvalidated<ViolationReviewInput> =
                    payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let reviewer_id = _ctx
                    .user_id()
                    .and_then(|id| id.parse::<i64>().ok())
                    .ok_or_else(|| err("auth", "AUTH_REQUIRED", "需要管理员身份".into()))?;
                violation_review(&conn, scope, &input, reviewer_id)
            }
            "violation_list" => {
                let unvalidated: system_core::Unvalidated<ViolationListInput> =
                    payload.try_into()?;
                violation_list(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "violation_get" => {
                let unvalidated: system_core::Unvalidated<ViolationGetInput> =
                    payload.try_into()?;
                violation_get(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            // Credit
            "credit_get" => {
                let unvalidated: system_core::Unvalidated<CreditGetInput> = payload.try_into()?;
                credit_get(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "credit_history" => {
                let unvalidated: system_core::Unvalidated<CreditHistoryInput> =
                    payload.try_into()?;
                credit_history(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            "credit_recalculate" => {
                require_admin(_ctx)?;
                let unvalidated: system_core::Unvalidated<CreditRecalculateInput> =
                    payload.try_into()?;
                credit_recalculate(
                    &conn,
                    scope,
                    &unvalidated.sanitize().validate()?.into_inner(),
                )
            }
            // Pre-order check
            "check_before_order" => {
                let unvalidated: system_core::Unvalidated<CheckBeforeOrderInput> =
                    payload.try_into()?;
                check_before_order(
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
                "blacklist_add",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "blacklist_remove",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "blacklist_check",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "blacklist_list",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "violation_record",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "violation_appeal",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "violation_review",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "violation_list",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "violation_get",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "credit_get",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "credit_history",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "credit_recalculate",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "check_before_order",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "credit".into(),
            description: "客户信用系统模块".into(),
            commands: vec![
                CommandSchema {
                    name: "blacklist_add".into(),
                    description: "添加黑名单".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "blacklist_remove".into(),
                    description: "移除黑名单".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "blacklist_check".into(),
                    description: "检查黑名单".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "blacklist_list".into(),
                    description: "黑名单列表".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "violation_record".into(),
                    description: "记录违规".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "violation_appeal".into(),
                    description: "违规申诉".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "violation_review".into(),
                    description: "审核申诉".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "violation_list".into(),
                    description: "违规列表".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "violation_get".into(),
                    description: "查看违规".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "credit_get".into(),
                    description: "信用评分".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "credit_history".into(),
                    description: "评分历史".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "credit_recalculate".into(),
                    description: "重算评分".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "check_before_order".into(),
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
// Blacklist implementations
// ══════════════════════════════════════════════════════════════════════════

fn blacklist_add(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &BlacklistAddInput,
    admin_id: i64,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Check if already active
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM blacklist WHERE tenant_id = ?1 AND customer_phone = ?2 AND is_active = 1 LIMIT 1",
            params![scope.tenant_id().as_str(), input.customer_phone],
            |row| row.get(0),
        )
        .ok();

    if existing.is_some() {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err("biz", "BIZ_DUPLICATE", "该客户已在黑名单中".into()));
    }

    let now = shanghai_now();
    conn.execute(
        "INSERT INTO blacklist (tenant_id, customer_name, customer_phone, id_number, reason, severity, created_by, is_active, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)",
        params![scope.tenant_id().as_str(), input.customer_name, input.customer_phone, input.id_number, input.reason, input.severity, admin_id, now],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    let id = conn.last_insert_rowid();
    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": id, "createdAt": now }))
}

fn blacklist_remove(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &BlacklistRemoveInput,
    admin_id: i64,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Check exists and is active
    let record: Option<(i64, bool)> = conn
        .query_row(
            "SELECT id, is_active FROM blacklist WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.id],
            |row| Ok((row.get(0)?, row.get::<_, i32>(1)? == 1)),
        )
        .ok();

    match record {
        None => {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err("biz", "BIZ_NOT_FOUND", "黑名单记录不存在".into()));
        }
        Some((_, false)) => {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err(
                "biz",
                "BIZ_ALREADY_REMOVED",
                "该黑名单记录已被移除".into(),
            ));
        }
        _ => {}
    }

    let now = shanghai_now();
    conn.execute(
        "UPDATE blacklist SET is_active = 0, removed_at = ?1, removed_by = ?2, removal_reason = ?3, updated_at = ?1 WHERE tenant_id = ?4 AND id = ?5",
        params![now, admin_id, input.removal_reason, scope.tenant_id().as_str(), input.id],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": input.id, "removedAt": now }))
}

fn blacklist_check(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &BlacklistCheckInput,
) -> Result<Value, String> {
    let mut stmt = conn.prepare(
        "SELECT id, customer_name, customer_phone, id_number, reason, severity, created_by, is_active, removed_at, removed_by, removal_reason, created_at, updated_at
         FROM blacklist WHERE tenant_id = ?1 AND customer_phone = ?2 AND is_active = 1 ORDER BY created_at DESC"
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let records: Vec<Value> = stmt
        .query_map(
            params![scope.tenant_id().as_str(), input.customer_phone],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "customerName": row.get::<_, String>(1)?,
                    "customerPhone": row.get::<_, String>(2)?,
                    "idNumber": row.get::<_, Option<String>>(3)?,
                    "reason": row.get::<_, String>(4)?,
                    "severity": row.get::<_, String>(5)?,
                    "createdBy": row.get::<_, i64>(6)?,
                    "isActive": row.get::<_, i32>(7)? == 1,
                    "removedAt": row.get::<_, Option<String>>(8)?,
                    "removedBy": row.get::<_, Option<i64>>(9)?,
                    "removalReason": row.get::<_, Option<String>>(10)?,
                    "createdAt": row.get::<_, String>(11)?,
                    "updatedAt": row.get::<_, String>(12)?,
                }))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    let blacklisted = !records.is_empty();
    Ok(serde_json::json!({ "ok": true, "blacklisted": blacklisted, "records": records }))
}

fn blacklist_list(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &BlacklistListInput,
) -> Result<Value, String> {
    let page = input.page.unwrap_or(1).max(1);
    let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    // Bool-based filter: safe because the value is always a literal 0 or 1 (not user string input)
    let filter_sql = match input.is_active {
        Some(true) => "AND is_active = 1",
        Some(false) => "AND is_active = 0",
        None => "",
    };

    let count_sql = format!(
        "SELECT COUNT(*) FROM blacklist WHERE tenant_id = ?1 {}",
        filter_sql
    );
    let total: i64 = conn
        .query_row(&count_sql, params![scope.tenant_id().as_str()], |row| {
            row.get(0)
        })
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let query_sql = format!(
        "SELECT id, customer_name, customer_phone, id_number, reason, severity, created_by, is_active, removed_at, removed_by, removal_reason, created_at, updated_at
         FROM blacklist WHERE tenant_id = ?1 {} ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
        filter_sql
    );

    let mut stmt = conn
        .prepare(&query_sql)
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
    let items: Vec<Value> = stmt
        .query_map(
            params![scope.tenant_id().as_str(), page_size, offset],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "customerName": row.get::<_, String>(1)?,
                    "customerPhone": row.get::<_, String>(2)?,
                    "idNumber": row.get::<_, Option<String>>(3)?,
                    "reason": row.get::<_, String>(4)?,
                    "severity": row.get::<_, String>(5)?,
                    "createdBy": row.get::<_, i64>(6)?,
                    "isActive": row.get::<_, i32>(7)? == 1,
                    "removedAt": row.get::<_, Option<String>>(8)?,
                    "removedBy": row.get::<_, Option<i64>>(9)?,
                    "removalReason": row.get::<_, Option<String>>(10)?,
                    "createdAt": row.get::<_, String>(11)?,
                    "updatedAt": row.get::<_, String>(12)?,
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

// ══════════════════════════════════════════════════════════════════════════
// Violation implementations
// ══════════════════════════════════════════════════════════════════════════

fn violation_record(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ViolationRecordInput,
    reported_by: i64,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    let now = shanghai_now();
    conn.execute(
        "INSERT INTO violations (tenant_id, customer_name, customer_phone, order_id, violation_type, severity, description, evidence, financial_penalty, reported_by, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'recorded', ?11, ?11)",
        params![
            scope.tenant_id().as_str(), input.customer_name, input.customer_phone, input.order_id,
            input.violation_type, input.severity, input.description,
            input.evidence, input.financial_penalty.unwrap_or(0.0), reported_by, now
        ],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_INSERT", e.to_string())
    })?;

    let violation_id = conn.last_insert_rowid();

    // Update credit scores: damage/lost_device with major/critical => -20, otherwise -10
    let penalty: i64 = if (input.violation_type == "damage"
        || input.violation_type == "lost_device")
        && (input.severity == "major" || input.severity == "critical")
    {
        20
    } else {
        10
    };

    // Upsert credit score
    ensure_credit_score(conn, scope, &input.customer_name, &input.customer_phone)?;
    conn.execute(
        "UPDATE credit_scores SET damage_incidents = damage_incidents + 1, score = MAX(0, score - ?1), updated_at = ?2, last_calculated_at = ?2 WHERE tenant_id = ?3 AND customer_phone = ?4",
        params![penalty, now, scope.tenant_id().as_str(), input.customer_phone],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": violation_id, "penalty": penalty, "createdAt": now }))
}

fn violation_appeal(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ViolationAppealInput,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Check status within transaction to prevent TOCTOU
    let current_status: String = conn
        .query_row(
            "SELECT status FROM violations WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if current_status != "recorded" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_STATUS_INVALID",
            format!("当前状态 '{}' 不可申诉", current_status),
        ));
    }

    let now = shanghai_now();
    conn.execute(
        "UPDATE violations SET status = 'appealed', appeal_reason = ?1, appeal_at = ?2, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4",
        params![input.appeal_reason, now, scope.tenant_id().as_str(), input.id],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": input.id, "status": "appealed", "appealAt": now }))
}

fn violation_review(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ViolationReviewInput,
    reviewer_id: i64,
) -> Result<Value, String> {
    conn.execute("BEGIN IMMEDIATE", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    // Check current status — must be appealed or under_review (within transaction to prevent TOCTOU)
    let current_status: String = conn
        .query_row(
            "SELECT status FROM violations WHERE tenant_id = ?1 AND id = ?2",
            params![scope.tenant_id().as_str(), input.id],
            |row| row.get(0),
        )
        .map_err(|e| {
            let _ = conn.execute("ROLLBACK", []);
            err("db", "DB_QUERY", e.to_string())
        })?;

    if current_status != "appealed" && current_status != "under_review" {
        let _ = conn.execute("ROLLBACK", []);
        return Err(err(
            "biz",
            "BIZ_STATUS_INVALID",
            format!("当前状态 '{}' 不可审核", current_status),
        ));
    }

    let now = shanghai_now();
    conn.execute(
        "UPDATE violations SET status = ?1, reviewed_by = ?2, reviewed_at = ?3, review_notes = ?4, updated_at = ?3 WHERE tenant_id = ?5 AND id = ?6",
        params![input.status, reviewer_id, now, input.review_notes, scope.tenant_id().as_str(), input.id],
    ).map_err(|e| {
        let _ = conn.execute("ROLLBACK", []);
        err("db", "DB_UPDATE", e.to_string())
    })?;

    conn.execute("COMMIT", [])
        .map_err(|e| err("db", "DB_TXN", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "id": input.id, "status": input.status, "reviewedAt": now }))
}

fn violation_list(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ViolationListInput,
) -> Result<Value, String> {
    let page = input.page.unwrap_or(1).max(1);
    let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    // Parameterized WHERE clauses to prevent SQL injection
    let mut conditions: Vec<String> = vec!["tenant_id = ?1".into()];
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(scope.tenant_id().as_str().to_owned())];

    if let Some(ref phone) = input.customer_phone {
        let idx = conditions.len() + 1;
        conditions.push(format!("customer_phone = ?{}", idx));
        all_params.push(Box::new(phone.clone()));
    }
    if let Some(ref status) = input.status {
        let idx = conditions.len() + 1;
        conditions.push(format!("status = ?{}", idx));
        all_params.push(Box::new(status.clone()));
    }
    if let Some(ref vtype) = input.violation_type {
        let idx = conditions.len() + 1;
        conditions.push(format!("violation_type = ?{}", idx));
        all_params.push(Box::new(vtype.clone()));
    }

    let where_sql = format!("WHERE {}", conditions.join(" AND "));

    let count_sql = format!("SELECT COUNT(*) FROM violations {}", where_sql);
    let total: i64 = {
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            all_params.iter().map(|p| p.as_ref()).collect();
        conn.query_row(&count_sql, rusqlite::params_from_iter(param_refs), |row| {
            row.get(0)
        })
    }
    .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let query_sql = format!(
        "SELECT id, customer_name, customer_phone, order_id, violation_type, severity, description, evidence, financial_penalty, reported_by, status, appeal_reason, appeal_at, reviewed_by, reviewed_at, review_notes, created_at, updated_at
         FROM violations {} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
        where_sql,
        all_params.len() + 1,
        all_params.len() + 2,
    );

    let mut stmt = conn
        .prepare(&query_sql)
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    // Build final param list: filter params + page_size + offset
    let mut final_params: Vec<Box<dyn rusqlite::types::ToSql>> = all_params;
    final_params.push(Box::new(page_size));
    final_params.push(Box::new(offset));
    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        final_params.iter().map(|p| p.as_ref()).collect();

    let items: Vec<Value> = stmt
        .query_map(rusqlite::params_from_iter(param_refs), |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "customerName": row.get::<_, String>(1)?,
                "customerPhone": row.get::<_, String>(2)?,
                "orderId": row.get::<_, Option<i64>>(3)?,
                "violationType": row.get::<_, String>(4)?,
                "severity": row.get::<_, String>(5)?,
                "description": row.get::<_, String>(6)?,
                "evidence": row.get::<_, Option<String>>(7)?,
                "financialPenalty": row.get::<_, f64>(8)?,
                "reportedBy": row.get::<_, i64>(9)?,
                "status": row.get::<_, String>(10)?,
                "appealReason": row.get::<_, Option<String>>(11)?,
                "appealAt": row.get::<_, Option<String>>(12)?,
                "reviewedBy": row.get::<_, Option<i64>>(13)?,
                "reviewedAt": row.get::<_, Option<String>>(14)?,
                "reviewNotes": row.get::<_, Option<String>>(15)?,
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

fn violation_get(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &ViolationGetInput,
) -> Result<Value, String> {
    let record = conn.query_row(
        "SELECT id, customer_name, customer_phone, order_id, violation_type, severity, description, evidence, financial_penalty, reported_by, status, appeal_reason, appeal_at, reviewed_by, reviewed_at, review_notes, created_at, updated_at
         FROM violations WHERE tenant_id = ?1 AND id = ?2",
        params![scope.tenant_id().as_str(), input.id],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "customerName": row.get::<_, String>(1)?,
                "customerPhone": row.get::<_, String>(2)?,
                "orderId": row.get::<_, Option<i64>>(3)?,
                "violationType": row.get::<_, String>(4)?,
                "severity": row.get::<_, String>(5)?,
                "description": row.get::<_, String>(6)?,
                "evidence": row.get::<_, Option<String>>(7)?,
                "financialPenalty": row.get::<_, f64>(8)?,
                "reportedBy": row.get::<_, i64>(9)?,
                "status": row.get::<_, String>(10)?,
                "appealReason": row.get::<_, Option<String>>(11)?,
                "appealAt": row.get::<_, Option<String>>(12)?,
                "reviewedBy": row.get::<_, Option<i64>>(13)?,
                "reviewedAt": row.get::<_, Option<String>>(14)?,
                "reviewNotes": row.get::<_, Option<String>>(15)?,
                "createdAt": row.get::<_, String>(16)?,
                "updatedAt": row.get::<_, String>(17)?,
            }))
        },
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "record": record }))
}

// ══════════════════════════════════════════════════════════════════════════
// Credit score implementations
// ══════════════════════════════════════════════════════════════════════════

fn ensure_credit_score(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    name: &str,
    phone: &str,
) -> Result<(), String> {
    // INSERT OR IGNORE is atomic — no TOCTOU between SELECT and INSERT
    let now = shanghai_now();
    conn.execute(
        "INSERT OR IGNORE INTO credit_scores (tenant_id, customer_name, customer_phone, score, total_orders, on_time_returns, late_returns, damage_incidents, last_calculated_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, 100, 0, 0, 0, 0, ?4, ?4, ?4)",
        params![scope.tenant_id().as_str(), name, phone, now],
    ).map_err(|e| err("db", "DB_INSERT", e.to_string()))?;
    Ok(())
}

fn score_label(score: i64) -> &'static str {
    match score {
        150..=200 => "优秀",
        120..=149 => "良好",
        90..=119 => "一般",
        60..=89 => "较差",
        _ => "差",
    }
}

fn credit_get(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &CreditGetInput,
) -> Result<Value, String> {
    // INSERT OR IGNORE handles the auto-create atomically (no TOCTOU)
    let now = shanghai_now();
    conn.execute(
        "INSERT OR IGNORE INTO credit_scores (tenant_id, customer_name, customer_phone, score, total_orders, on_time_returns, late_returns, damage_incidents, last_calculated_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, 100, 0, 0, 0, 0, ?4, ?4, ?4)",
        params![scope.tenant_id().as_str(), input.customer_phone, input.customer_phone, now],
    ).map_err(|e| err("db", "DB_INSERT", e.to_string()))?;

    let record = conn.query_row(
        "SELECT id, customer_name, customer_phone, score, total_orders, on_time_returns, late_returns, damage_incidents, last_calculated_at, created_at, updated_at
         FROM credit_scores WHERE tenant_id = ?1 AND customer_phone = ?2",
        params![scope.tenant_id().as_str(), input.customer_phone],
        |row| {
            let score_val: i64 = row.get(3)?;
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "customerName": row.get::<_, String>(1)?,
                "customerPhone": row.get::<_, String>(2)?,
                "score": score_val,
                "totalOrders": row.get::<_, i64>(4)?,
                "onTimeReturns": row.get::<_, i64>(5)?,
                "lateReturns": row.get::<_, i64>(6)?,
                "damageIncidents": row.get::<_, i64>(7)?,
                "lastCalculatedAt": row.get::<_, String>(8)?,
                "createdAt": row.get::<_, String>(9)?,
                "updatedAt": row.get::<_, String>(10)?,
                "label": score_label(score_val),
            }))
        },
    ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    Ok(serde_json::json!({ "ok": true, "record": record }))
}

fn credit_history(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &CreditHistoryInput,
) -> Result<Value, String> {
    // Simplified history: violations as credit-impact events
    let mut stmt = conn
        .prepare(
            "SELECT id, violation_type, severity, description, status, created_at
         FROM violations WHERE tenant_id = ?1 AND customer_phone = ?2 ORDER BY created_at DESC",
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?;

    let history: Vec<Value> = stmt
        .query_map(
            params![scope.tenant_id().as_str(), input.customer_phone],
            |row| {
                let vtype: String = row.get(1)?;
                let severity: String = row.get(2)?;
                let impact: i64 = if (vtype == "damage" || vtype == "lost_device")
                    && (severity == "major" || severity == "critical")
                {
                    -20
                } else {
                    -10
                };

                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "eventType": "violation",
                    "violationType": vtype,
                    "severity": severity,
                    "description": row.get::<_, String>(3)?,
                    "scoreImpact": impact,
                    "status": row.get::<_, String>(4)?,
                    "createdAt": row.get::<_, String>(5)?,
                }))
            },
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    // Get current score
    let score_info = conn.query_row(
        "SELECT score, on_time_returns, late_returns, damage_incidents FROM credit_scores WHERE tenant_id = ?1 AND customer_phone = ?2",
        params![scope.tenant_id().as_str(), input.customer_phone],
        |row| {
            Ok(serde_json::json!({
                "score": row.get::<_, i64>(0)?,
                "onTimeReturns": row.get::<_, i64>(1)?,
                "lateReturns": row.get::<_, i64>(2)?,
                "damageIncidents": row.get::<_, i64>(3)?,
            }))
        },
    ).unwrap_or(serde_json::json!({ "score": 100, "onTimeReturns": 0, "lateReturns": 0, "damageIncidents": 0 }));

    Ok(serde_json::json!({ "ok": true, "currentScore": score_info, "history": history }))
}

fn credit_recalculate(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &CreditRecalculateInput,
) -> Result<Value, String> {
    // Get current counters
    let counters = conn.query_row(
        "SELECT on_time_returns, late_returns, damage_incidents FROM credit_scores WHERE tenant_id = ?1 AND customer_phone = ?2",
        params![scope.tenant_id().as_str(), input.customer_phone],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
    ).map_err(|_| err("db", "DB_QUERY", "信用档案不存在".into()))?;

    let (on_time, late, damage) = counters;
    let score = (100_i64 + on_time * 2 - late * 5 - damage * 10).clamp(0, 200);

    let now = shanghai_now();
    conn.execute(
        "UPDATE credit_scores SET score = ?1, last_calculated_at = ?2, updated_at = ?2 WHERE tenant_id = ?3 AND customer_phone = ?4",
        params![score, now, scope.tenant_id().as_str(), input.customer_phone],
    ).map_err(|e| err("db", "DB_UPDATE", e.to_string()))?;

    Ok(serde_json::json!({
        "ok": true,
        "customerPhone": input.customer_phone,
        "score": score,
        "label": score_label(score),
        "formula": "100 + onTime*2 - late*5 - damage*10",
        "components": { "onTimeReturns": on_time, "lateReturns": late, "damageIncidents": damage },
        "recalculatedAt": now,
    }))
}

// ══════════════════════════════════════════════════════════════════════════
// Pre-order check
// ══════════════════════════════════════════════════════════════════════════

fn check_before_order(
    conn: &rusqlite::Connection,
    scope: &DataScope,
    input: &CheckBeforeOrderInput,
) -> Result<Value, String> {
    let mut warnings: Vec<String> = Vec::new();

    // 1. Check blacklist
    let blacklist_records: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT severity, reason FROM blacklist WHERE tenant_id = ?1 AND customer_phone = ?2 AND is_active = 1"
        ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
        stmt.query_map(
            params![scope.tenant_id().as_str(), input.customer_phone],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    for (severity, reason) in &blacklist_records {
        if severity == "critical" || severity == "high" {
            return Ok(serde_json::json!({
                "ok": true,
                "allowed": false,
                "blockReason": format!("黑名单({}): {}", severity, reason),
                "creditScore": null,
                "creditWarning": null,
                "warnings": [format!("该客户在黑名单中，严重程度: {}，原因: {}", severity, reason)],
            }));
        }
        warnings.push(format!(
            "该客户在黑名单中（{}）, 原因: {}",
            severity, reason
        ));
    }

    // 2. Check credit score
    let score_info: Option<(i64, String)> = conn
        .query_row(
            "SELECT score, customer_name FROM credit_scores WHERE tenant_id = ?1 AND customer_phone = ?2",
            params![scope.tenant_id().as_str(), input.customer_phone],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let (credit_score, credit_warning) = match score_info {
        Some((score, _)) if score < 50 => {
            return Ok(serde_json::json!({
                "ok": true,
                "allowed": false,
                "blockReason": format!("信用评分过低({})，拒绝下单", score),
                "creditScore": score,
                "creditWarning": format!("信用评分极低({}分)，建议人工审核", score),
                "warnings": [format!("信用评分 {} 分，已低于最低标准 50 分", score)],
            }));
        }
        Some((score, _)) if score < 80 => {
            (score, Some(format!("信用评分较低({}分)，建议关注", score)))
        }
        Some((score, _)) => (score, None),
        None => (100, None),
    };

    // 3. Check unresolved major/critical violations
    let pending_major: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT violation_type, severity FROM violations
             WHERE tenant_id = ?1 AND customer_phone = ?2 AND severity IN ('major', 'critical') AND status IN ('recorded', 'appealed', 'under_review')"
        ).map_err(|e| err("db", "DB_QUERY", e.to_string()))?;
        stmt.query_map(
            params![scope.tenant_id().as_str(), input.customer_phone],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|e| err("db", "DB_QUERY", e.to_string()))?
        .filter_map(|r| r.ok())
        .collect()
    };

    for (vtype, sev) in &pending_major {
        warnings.push(format!("未处理的严重违规: {} ({})", vtype, sev));
    }

    Ok(serde_json::json!({
        "ok": true,
        "allowed": true,
        "blockReason": null,
        "creditScore": credit_score,
        "creditWarning": credit_warning,
        "warnings": warnings,
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
    fn credit_reads_and_writes_are_isolated_by_data_scope() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE credit_scores (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tenant_id TEXT NOT NULL,
                customer_name TEXT NOT NULL,
                customer_phone TEXT NOT NULL,
                score INTEGER NOT NULL DEFAULT 100,
                total_orders INTEGER NOT NULL DEFAULT 0,
                on_time_returns INTEGER NOT NULL DEFAULT 0,
                late_returns INTEGER NOT NULL DEFAULT 0,
                damage_incidents INTEGER NOT NULL DEFAULT 0,
                last_calculated_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(tenant_id, customer_phone)
            );
            CREATE TABLE blacklist (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                tenant_id TEXT NOT NULL,
                customer_name TEXT NOT NULL,
                customer_phone TEXT NOT NULL,
                id_number TEXT,
                reason TEXT NOT NULL,
                severity TEXT NOT NULL,
                created_by INTEGER NOT NULL,
                is_active INTEGER NOT NULL,
                removed_at TEXT,
                removed_by INTEGER,
                removal_reason TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .unwrap();

        let tenant_a = scope("tenant-a");
        let tenant_b = scope("tenant-b");
        let phone = "13800000000";
        ensure_credit_score(&conn, &tenant_a, "A", phone).unwrap();
        ensure_credit_score(&conn, &tenant_b, "B", phone).unwrap();
        conn.execute(
            "UPDATE credit_scores SET score = 20 WHERE tenant_id = 'tenant-a'",
            [],
        )
        .unwrap();

        let input = CreditGetInput {
            customer_phone: phone.into(),
        };
        let a = credit_get(&conn, &tenant_a, &input).unwrap();
        let b = credit_get(&conn, &tenant_b, &input).unwrap();
        assert_eq!(a["record"]["score"], 20);
        assert_eq!(b["record"]["score"], 100);

        conn.execute(
            "INSERT INTO blacklist (tenant_id, customer_name, customer_phone, reason, severity, created_by, is_active, created_at, updated_at)
             VALUES ('tenant-a', 'A', ?1, 'blocked', 'critical', 1, 1, 'now', 'now')",
            params![phone],
        )
        .unwrap();
        let check = BlacklistCheckInput {
            customer_name: "B".into(),
            customer_phone: phone.into(),
        };
        assert_eq!(
            blacklist_check(&conn, &tenant_b, &check).unwrap()["blacklisted"],
            false
        );
    }
}

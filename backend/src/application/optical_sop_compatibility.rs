use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use system_admin::optical_sop::FeatureOpticalSop;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{
    OpticalSopMutationError, RepositoryError, RepositoryProvider, ScopedRepositories,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionCreateInput {
    order_id: String,
    device_serial_no: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionUpdateStepInput {
    id: i64,
    step: String,
    ok: bool,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InspectionCompleteInput {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct InspectionGetInput {
    id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectionListInput {
    order_id: Option<String>,
    device_serial_no: Option<String>,
    overall_grade: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct InspectionStatsInput {}

#[derive(Clone)]
pub(crate) struct OpticalSopCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl OpticalSopCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    fn scoped(&self, ctx: &ExecutionContext) -> Result<ScopedRepositories, String> {
        self.repository_provider
            .bind(ctx)
            .map_err(Self::repository_error)
    }

    fn parse<T: serde::de::DeserializeOwned>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload).map_err(|error| {
            Self::error(
                "val",
                "VAL_DESERIALIZE",
                format!("invalid optical SOP payload: {error}"),
                None,
            )
        })
    }

    fn error(category: &str, code: &str, message: String, field: Option<&str>) -> String {
        serde_json::to_string(&ErrorPayload {
            category: category.into(),
            code: code.into(),
            message,
            field: field.map(str::to_owned),
            context: None,
        })
        .unwrap_or_default()
    }

    fn repository_error(error: RepositoryError) -> String {
        Self::error(
            "sys",
            error.code(),
            "optical SOP persistence unavailable".into(),
            None,
        )
    }

    fn mutation_error(error: OpticalSopMutationError) -> String {
        match error {
            OpticalSopMutationError::ChecklistNotFound | OpticalSopMutationError::OrderNotFound => {
                Self::error("biz", "BIZ_NOT_FOUND", "检查清单或订单不存在".into(), None)
            }
            OpticalSopMutationError::InvalidOrderStatus(status) => Self::error(
                "biz",
                "BIZ_STATUS_INVALID",
                format!("订单状态 '{status}' 不可创建检查清单，需为 in_use 或 shipped"),
                Some("orderId"),
            ),
            OpticalSopMutationError::DeviceNotInOrder => Self::error(
                "biz",
                "BIZ_NOT_FOUND",
                "订单设备不属于当前租户或未分配给该订单".into(),
                Some("deviceSerialNo"),
            ),
            OpticalSopMutationError::Duplicate => Self::error(
                "biz",
                "BIZ_DUPLICATE",
                "该订单设备已有检查清单".into(),
                None,
            ),
            OpticalSopMutationError::Completed => Self::error(
                "biz",
                "BIZ_STATUS_INVALID",
                "检查清单已完成，不可继续修改".into(),
                Some("id"),
            ),
            OpticalSopMutationError::Storage(error) => Self::repository_error(error),
        }
    }

    fn require_admin(ctx: &ExecutionContext) -> Result<(), String> {
        if ctx.has_tenant_admin_authority() {
            Ok(())
        } else {
            Err(Self::error(
                "auth",
                "AUTH_FORBIDDEN",
                "仅管理员可操作".into(),
                None,
            ))
        }
    }

    fn actor(ctx: &ExecutionContext) -> Result<&str, String> {
        ctx.user_id()
            .ok_or_else(|| Self::error("auth", "AUTH_REQUIRED", "需要管理员身份".into(), None))
    }

    fn create(
        &self,
        ctx: &ExecutionContext,
        mut input: InspectionCreateInput,
    ) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        input.order_id = input.order_id.trim().to_owned();
        input.device_serial_no = input.device_serial_no.trim().to_owned();
        if input.order_id.is_empty() {
            return Err(Self::error(
                "val",
                "VAL_REQUIRED",
                "订单ID不能为空".into(),
                Some("orderId"),
            ));
        }
        if input.device_serial_no.is_empty() {
            return Err(Self::error(
                "val",
                "VAL_REQUIRED",
                "设备序列号不能为空".into(),
                Some("deviceSerialNo"),
            ));
        }

        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .optical_sops()
            .create(
                &input.order_id,
                &input.device_serial_no,
                Self::actor(ctx)?,
                &shanghai_now_iso(),
            )
            .map_err(Self::mutation_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "id": outcome.id,
            "orderId": outcome.order_id,
            "deviceSerialNo": outcome.device_serial_no,
            "overallGrade": outcome.overall_grade,
            "createdAt": outcome.created_at,
        }))
    }

    fn update_step(
        &self,
        ctx: &ExecutionContext,
        mut input: InspectionUpdateStepInput,
    ) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        if input.id <= 0 {
            return Err(Self::error(
                "val",
                "VAL_REQUIRED",
                "检查清单ID无效".into(),
                Some("id"),
            ));
        }

        input.step = normalize_step(&input.step).ok_or_else(|| {
            Self::error(
                "val",
                "VAL_INVALID",
                "步骤必须是 body / lens / screen / accessory / function".into(),
                Some("step"),
            )
        })?;
        input.note = input
            .note
            .map(|note| note.trim().to_owned())
            .filter(|note| !note.is_empty());

        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .optical_sops()
            .update_step(
                input.id,
                &input.step,
                input.ok,
                input.note.as_deref(),
                &shanghai_now_iso(),
            )
            .map_err(Self::mutation_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "id": outcome.id,
            "step": outcome.step,
            "ok": outcome.ok,
            "overallGrade": outcome.overall_grade,
            "updatedAt": outcome.updated_at,
        }))
    }

    fn complete(
        &self,
        ctx: &ExecutionContext,
        input: InspectionCompleteInput,
    ) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        if input.id <= 0 {
            return Err(Self::error(
                "val",
                "VAL_REQUIRED",
                "检查清单ID无效".into(),
                Some("id"),
            ));
        }

        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .optical_sops()
            .complete(input.id, Self::actor(ctx)?, &shanghai_now_iso())
            .map_err(Self::mutation_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "id": outcome.id,
            "orderId": outcome.order_id,
            "deviceSerialNo": outcome.device_serial_no,
            "overallGrade": outcome.overall_grade,
            "damageReportId": outcome.damage_report_id,
            "completedAt": outcome.completed_at,
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: InspectionGetInput) -> Result<Value, String> {
        if input.id <= 0 {
            return Err(Self::error(
                "val",
                "VAL_REQUIRED",
                "检查清单ID无效".into(),
                Some("id"),
            ));
        }
        let scoped = self.scoped(ctx)?;
        let record = scoped
            .optical_sops()
            .get(input.id)
            .map_err(Self::repository_error)?
            .ok_or_else(|| {
                Self::error("biz", "BIZ_NOT_FOUND", "检查清单不存在".into(), Some("id"))
            })?;
        Ok(serde_json::json!({ "ok": true, "record": record }))
    }

    fn list(
        &self,
        ctx: &ExecutionContext,
        mut input: InspectionListInput,
    ) -> Result<Value, String> {
        input.order_id = trim_option(input.order_id);
        input.device_serial_no = trim_option(input.device_serial_no);
        input.overall_grade = trim_option(input.overall_grade);

        let scoped = self.scoped(ctx)?;
        let result = scoped
            .optical_sops()
            .list(
                input.order_id.as_deref(),
                input.device_serial_no.as_deref(),
                input.overall_grade.as_deref(),
                input.page.unwrap_or(1).max(1),
                input.page_size.unwrap_or(20).clamp(1, 100),
            )
            .map_err(Self::repository_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "items": result.items,
            "total": result.total,
            "page": result.page,
            "pageSize": result.page_size,
        }))
    }

    fn stats(&self, ctx: &ExecutionContext, _input: InspectionStatsInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let stats = scoped
            .optical_sops()
            .stats()
            .map_err(Self::repository_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "total": stats.total,
            "passCount": stats.pass_count,
            "damageCount": stats.damage_count,
            "byGrade": stats.by_grade,
        }))
    }
}

impl SystemModule for OpticalSopCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureOpticalSop::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureOpticalSop::new().commands()
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
        match command {
            "inspection_create" => self.create(ctx, Self::parse(payload)?),
            "inspection_update_step" => self.update_step(ctx, Self::parse(payload)?),
            "inspection_complete" => self.complete(ctx, Self::parse(payload)?),
            "inspection_get" => self.get(ctx, Self::parse(payload)?),
            "inspection_list" => self.list(ctx, Self::parse(payload)?),
            "inspection_stats" => self.stats(ctx, Self::parse(payload)?),
            _ => Err(Self::error(
                "sys",
                "CMD_UNKNOWN",
                format!("Unknown command: {command}"),
                None,
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureOpticalSop::new().schema()
    }
}

fn normalize_step(value: &str) -> Option<String> {
    let normalized = value.trim().strip_suffix("_ok").unwrap_or(value.trim());
    match normalized {
        "body" | "lens" | "screen" | "accessory" | "function" => Some(normalized.to_owned()),
        _ => None,
    }
}

fn trim_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

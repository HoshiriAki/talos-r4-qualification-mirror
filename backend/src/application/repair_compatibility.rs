use std::sync::Arc;

use official_device::repair::{
    CreateRepairOrderInput, FeatureRepair, GetDeviceStatsInput, GetRepairInput, ListRepairInput,
    UpdateRepairStatusInput,
};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{RepairMutationError, RepositoryError, RepositoryProvider};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct RepairCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl RepairCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    fn scoped(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<crate::repositories::ScopedRepositories, String> {
        self.repository_provider
            .bind(ctx)
            .map_err(Self::repository_error)
    }

    fn parse<T: serde::de::DeserializeOwned>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload).map_err(|error| format!("VAL_DESERIALIZE: {error}"))
    }

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "repair persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: RepairMutationError, repair_id: &str, damage_id: &str) -> String {
        let payload = match error {
            RepairMutationError::DamageNotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_FOUND".into(),
                message: format!("损坏报告 {damage_id} 不存在"),
                field: Some("damageReportId".into()),
                context: None,
            },
            RepairMutationError::DamageNotAdjudicated(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_ADJUDICATED".into(),
                message: format!("损坏报告尚未责任认定 (当前: {status})"),
                field: Some("damageReportId".into()),
                context: None,
            },
            RepairMutationError::AlreadyExists(existing) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REPAIR_ALREADY_EXISTS".into(),
                message: format!("损坏报告已有维修工单 {existing}"),
                field: Some("damageReportId".into()),
                context: None,
            },
            RepairMutationError::NotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REPAIR_NOT_FOUND".into(),
                message: format!("维修工单 {repair_id} 不存在"),
                field: Some("repairId".into()),
                context: None,
            },
            RepairMutationError::NotPending(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REPAIR_NOT_PENDING".into(),
                message: format!("工单状态为 {status}，不可开始维修"),
                field: Some("repairId".into()),
                context: None,
            },
            RepairMutationError::NotInProgress(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REPAIR_NOT_IN_PROGRESS".into(),
                message: format!("工单状态为 {status}，不可完成"),
                field: Some("repairId".into()),
                context: None,
            },
            RepairMutationError::NotCompleted(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REPAIR_NOT_COMPLETED".into(),
                message: if status == "changed" {
                    "维修状态已变化，返库事务已取消".into()
                } else {
                    format!("工单状态为 {status}，不可返库")
                },
                field: Some("repairId".into()),
                context: None,
            },
            RepairMutationError::DeviceNotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DEVICE_NOT_FOUND".into(),
                message: "返库设备不存在或不属于当前租户".into(),
                field: Some("deviceSerialNo".into()),
                context: None,
            },
            RepairMutationError::Storage(error) => return Self::repository_error(error),
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn create(
        &self,
        ctx: &ExecutionContext,
        input: CreateRepairOrderInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let operator = ctx.user_id().unwrap_or("system");
        let outcome = scoped
            .repairs()
            .create(
                &input.damage_report_id,
                &input.repair_description,
                &input.vendor,
                operator,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, "", &input.damage_report_id))?;

        Ok(serde_json::json!({
            "ok": true,
            "repairId": outcome.repair_id,
            "damageReportId": outcome.damage_report_id,
            "deviceSerialNo": outcome.device_serial_no,
            "status": "pending",
            "depositForfeited": false,
            "settlementAuthority": "r3_settlement",
        }))
    }

    fn start(
        &self,
        ctx: &ExecutionContext,
        input: UpdateRepairStatusInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .repairs()
            .start(&input.repair_id, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.repair_id, ""))?;

        Ok(serde_json::json!({
            "ok": true,
            "repairId": outcome.repair_id,
            "status": outcome.status,
        }))
    }

    fn complete(
        &self,
        ctx: &ExecutionContext,
        input: UpdateRepairStatusInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let operator = ctx.user_id().unwrap_or("system");
        let repair_cost = input.repair_cost.unwrap_or(0.0);
        let outcome = scoped
            .repairs()
            .complete(&input.repair_id, repair_cost, operator, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.repair_id, ""))?;

        Ok(serde_json::json!({
            "ok": true,
            "repairId": outcome.repair_id,
            "status": outcome.status,
            "repairCost": repair_cost,
        }))
    }

    fn return_to_stock(
        &self,
        ctx: &ExecutionContext,
        input: UpdateRepairStatusInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let operator = ctx.user_id().unwrap_or("system");
        let outcome = scoped
            .repairs()
            .return_to_stock(&input.repair_id, operator, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.repair_id, ""))?;

        Ok(serde_json::json!({
            "ok": true,
            "repairId": outcome.repair_id,
            "deviceSerialNo": outcome.device_serial_no,
            "status": outcome.status,
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: GetRepairInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let repair = scoped
            .repairs()
            .get(&input.repair_id)
            .map_err(Self::repository_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "repairOrder": repair,
        }))
    }

    fn list(&self, ctx: &ExecutionContext, input: ListRepairInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let result = scoped
            .repairs()
            .list(
                input.status.as_deref(),
                input.device_serial_no.as_deref(),
                page,
                page_size,
            )
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "repairOrders": result.repairs,
            "pagination": {
                "page": result.page,
                "pageSize": result.page_size,
                "total": result.total,
            },
        }))
    }

    fn device_stats(
        &self,
        ctx: &ExecutionContext,
        input: GetDeviceStatsInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let stats = scoped
            .repairs()
            .device_stats(&input.device_serial_no)
            .map_err(Self::repository_error)?;

        let replacement_value = scoped
            .procurements()
            .get(&input.device_serial_no)
            .ok()
            .flatten()
            .map(|record| record.replacement_value)
            .unwrap_or(0.0);

        let exceeds_threshold =
            replacement_value > 0.0 && stats.total_repair_cost > replacement_value * 0.75;
        let threshold_ratio = if replacement_value > 0.0 {
            stats.total_repair_cost / replacement_value
        } else {
            0.0
        };

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": input.device_serial_no,
            "totalRepairs": stats.total_repairs,
            "totalRepairCost": stats.total_repair_cost,
            "replacementValue": replacement_value,
            "thresholdRatio": threshold_ratio,
            "exceedsReplacementThreshold": exceeds_threshold,
            "warning": if exceeds_threshold {
                "累计维修费用超过重置价值的75%，建议替换设备"
            } else {
                ""
            },
            "recentRepairs": stats.recent_repairs,
        }))
    }
}

impl SystemModule for RepairCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureRepair::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureRepair::new().commands()
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
            "create_repair_order" => self.create(ctx, Self::parse(payload)?),
            "start_repair" => self.start(ctx, Self::parse(payload)?),
            "complete_repair" => self.complete(ctx, Self::parse(payload)?),
            "return_to_stock" => self.return_to_stock(ctx, Self::parse(payload)?),
            "get" => self.get(ctx, Self::parse(payload)?),
            "list" => self.list(ctx, Self::parse(payload)?),
            "get_device_stats" => self.device_stats(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: repair.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureRepair::new().schema()
    }
}

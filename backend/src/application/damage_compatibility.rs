use std::sync::Arc;

use official_device::damage::{
    AdjudicateDamageInput, AssessDamageInput, FeatureDamage, GetDamageInput, ListDamageInput,
    ReportDamageInput,
};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{DamageMutationError, RepositoryError, RepositoryProvider};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct DamageCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl DamageCompatibilityModule {
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
            message: "damage persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: DamageMutationError, damage_id: &str) -> String {
        let payload = match error {
            DamageMutationError::ResourceNotFound => {
                return "BIZ_DAMAGE_RESOURCE_NOT_FOUND: order or device is outside data scope"
                    .into();
            }
            DamageMutationError::NotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_FOUND".into(),
                message: format!("损坏报告 {damage_id} 不存在"),
                field: Some("damageId".into()),
                context: None,
            },
            DamageMutationError::NotReported(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_REPORTED".into(),
                message: format!("损坏报告状态为 {status}，不可定损"),
                field: Some("damageId".into()),
                context: None,
            },
            DamageMutationError::NotAssessed(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_ASSESSED".into(),
                message: format!("损坏报告状态为 {status}，不可认定"),
                field: Some("damageId".into()),
                context: None,
            },
            DamageMutationError::Storage(error) => return Self::repository_error(error),
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn report(&self, ctx: &ExecutionContext, input: ReportDamageInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        if input.appearance_ok && input.accessories_ok && input.function_ok {
            return Ok(serde_json::json!({
                "ok": true,
                "damageId": null,
                "noDamage": true,
            }));
        }

        let operator = ctx.user_id().unwrap_or("system");
        let outcome = scoped
            .damages()
            .report(
                &input.order_id,
                &input.device_serial_no,
                input.appearance_ok,
                input.accessories_ok,
                input.function_ok,
                &input.damage_description,
                operator,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, ""))?;

        Ok(serde_json::json!({
            "ok": true,
            "damageId": outcome.damage_id,
            "orderId": outcome.order_id,
            "deviceSerialNo": outcome.device_serial_no,
            "status": "reported",
            "noDamage": false,
        }))
    }

    fn assess(&self, ctx: &ExecutionContext, input: AssessDamageInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let valid_liabilities = ["customer", "logistics", "warehouse", "unknown"];
        if !valid_liabilities.contains(&input.liability.as_str()) {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_INVALID_LIABILITY".into(),
                message: format!("责任类型无效: {}", input.liability),
                field: Some("liability".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let operator = ctx.user_id().unwrap_or("system");
        let outcome = scoped
            .damages()
            .assess(
                &input.damage_id,
                input.estimated_damage_amount,
                &input.liability,
                &input.notes,
                operator,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, &input.damage_id))?;

        Ok(serde_json::json!({
            "ok": true,
            "damageId": outcome.damage_id,
            "orderId": outcome.order_id,
            "deviceSerialNo": outcome.device_serial_no,
            "estimatedAmount": input.estimated_damage_amount,
            "liability": input.liability,
            "status": outcome.status,
        }))
    }

    fn adjudicate(
        &self,
        ctx: &ExecutionContext,
        input: AdjudicateDamageInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let operator = ctx.user_id().unwrap_or("system");
        let outcome = scoped
            .damages()
            .adjudicate(&input.damage_id, operator, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.damage_id))?;

        Ok(serde_json::json!({
            "ok": true,
            "damageId": outcome.damage_id,
            "orderId": outcome.order_id,
            "deviceSerialNo": outcome.device_serial_no,
            "status": outcome.status,
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: GetDamageInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let reports = if let Some(order_id) = input.order_id {
            scoped
                .damages()
                .get_by_order(&order_id)
                .map_err(Self::repository_error)?
        } else if let Some(device_serial_no) = input.device_serial_no {
            scoped
                .damages()
                .get_by_device(&device_serial_no)
                .map_err(Self::repository_error)?
        } else {
            return Err("VAL_MISSING_FILTER: orderId or deviceSerialNo required".into());
        };

        Ok(serde_json::json!({
            "ok": true,
            "damageReports": reports,
        }))
    }

    fn list(&self, ctx: &ExecutionContext, input: ListDamageInput) -> Result<Value, String> {
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let scoped = self.scoped(ctx)?;
        let result = scoped
            .damages()
            .list(input.status.as_deref(), page, page_size)
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "damageReports": result.reports,
            "pagination": {
                "page": result.page,
                "pageSize": result.page_size,
                "total": result.total,
            },
        }))
    }
}

impl SystemModule for DamageCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureDamage::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureDamage::new().commands()
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
            "report" => self.report(ctx, Self::parse(payload)?),
            "assess" => self.assess(ctx, Self::parse(payload)?),
            "adjudicate" => self.adjudicate(ctx, Self::parse(payload)?),
            "get" => self.get(ctx, Self::parse(payload)?),
            "list" => self.list(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: damage.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureDamage::new().schema()
    }
}

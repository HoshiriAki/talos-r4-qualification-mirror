use std::sync::Arc;

use official_finance::deposit::{
    CalculateDepositInput, CollectDepositInput, FeatureDeposit, ForfeitDepositInput,
    GetDepositInput, ReleaseDepositInput,
};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{DepositMutationError, RepositoryError, RepositoryProvider};
use crate::utils::time::shanghai_now_iso;

const DEFAULT_DEPOSIT_PER_DEVICE: f64 = 2000.0;

#[derive(Clone)]
pub(crate) struct DepositCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl DepositCompatibilityModule {
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
            message: "deposit persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: DepositMutationError, order_id: &str) -> String {
        let payload = match error {
            DepositMutationError::OrderNotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ORDER_NOT_FOUND".into(),
                message: format!("订单 {order_id} 不存在"),
                field: Some("orderId".into()),
                context: None,
            },
            DepositMutationError::AlreadyPaid => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DEPOSIT_ALREADY_PAID".into(),
                message: "押金已缴纳或已释放，不可重复收款".into(),
                field: Some("orderId".into()),
                context: None,
            },
            DepositMutationError::NotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DEPOSIT_NOT_FOUND".into(),
                message: format!("订单 {order_id} 没有押金记录"),
                field: Some("orderId".into()),
                context: None,
            },
            DepositMutationError::AlreadyReleased => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DEPOSIT_ALREADY_RELEASED".into(),
                message: "押金已释放，不可重复操作".into(),
                field: Some("orderId".into()),
                context: None,
            },
            DepositMutationError::NotPaid(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DEPOSIT_NOT_PAID".into(),
                message: format!("押金状态为 {status}，不可执行该操作"),
                field: Some("orderId".into()),
                context: None,
            },
            DepositMutationError::ExceedsAvailable {
                requested,
                available,
            } => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_EXCEEDS_AVAILABLE".into(),
                message: format!("罚没金额 ¥{requested:.2} 超过可用余额 ¥{available:.2}"),
                field: Some("amount".into()),
                context: None,
            },
            DepositMutationError::Storage(error) => return Self::repository_error(error),
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn calculate(
        &self,
        ctx: &ExecutionContext,
        input: CalculateDepositInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .deposits()
            .calculate(
                &input.order_id,
                DEFAULT_DEPOSIT_PER_DEVICE,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, &input.order_id))?;

        let mut response = serde_json::json!({
            "ok": true,
            "depositId": outcome.deposit.id,
            "orderId": outcome.deposit.order_id,
            "amount": outcome.deposit.amount,
            "status": outcome.deposit.status,
            "existing": outcome.existing,
        });
        if let Some(device_count) = outcome.device_count {
            response["deviceCount"] = serde_json::json!(device_count);
        }
        Ok(response)
    }

    fn collect(&self, ctx: &ExecutionContext, input: CollectDepositInput) -> Result<Value, String> {
        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "押金金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let operator = ctx.user_id().unwrap_or("system");
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .deposits()
            .collect(&input.order_id, input.amount, operator, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.order_id))?;

        Ok(serde_json::json!({
            "ok": true,
            "depositId": outcome.deposit_id,
            "orderId": outcome.order_id,
            "amount": outcome.amount,
            "status": "paid",
        }))
    }

    fn release(&self, ctx: &ExecutionContext, input: ReleaseDepositInput) -> Result<Value, String> {
        let operator = ctx.user_id().unwrap_or("system");
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .deposits()
            .release(
                &input.order_id,
                &input.reason,
                operator,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, &input.order_id))?;

        Ok(serde_json::json!({
            "ok": true,
            "depositId": outcome.deposit_id,
            "orderId": outcome.order_id,
            "releasedAmount": outcome.released_amount,
            "forfeitedAmount": outcome.forfeited_amount,
            "status": "released",
        }))
    }

    fn forfeit(&self, ctx: &ExecutionContext, input: ForfeitDepositInput) -> Result<Value, String> {
        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "罚没金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let operator = ctx.user_id().unwrap_or("system");
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .deposits()
            .forfeit(
                &input.order_id,
                input.amount,
                &input.reason,
                operator,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, &input.order_id))?;

        Ok(serde_json::json!({
            "ok": true,
            "depositId": outcome.deposit_id,
            "orderId": outcome.order_id,
            "forfeitedAmount": outcome.forfeited_amount,
            "totalForfeited": outcome.total_forfeited,
            "remainingBalance": outcome.remaining_balance,
            "status": outcome.status,
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: GetDepositInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let detail = scoped
            .deposits()
            .get(&input.order_id)
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "deposit": detail.deposit,
            "ledger": detail.ledger,
        }))
    }
}

impl SystemModule for DepositCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureDeposit::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureDeposit::new().commands()
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
            "calculate" => self.calculate(ctx, Self::parse(payload)?),
            "collect" => self.collect(ctx, Self::parse(payload)?),
            "release" => self.release(ctx, Self::parse(payload)?),
            "forfeit" => self.forfeit(ctx, Self::parse(payload)?),
            "get" => self.get(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: deposit.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureDeposit::new().schema()
    }
}

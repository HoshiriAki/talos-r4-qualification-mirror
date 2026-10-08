use std::sync::Arc;

use official_finance::refund::{
    ApproveRefundInput, ExecuteRefundInput, FeatureRefund, ListRefundsInput, RejectRefundInput,
    RequestRefundInput,
};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{RefundMutationError, RepositoryError, RepositoryProvider};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct RefundCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl RefundCompatibilityModule {
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
            message: "refund persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: RefundMutationError, subject_id: &str) -> String {
        let payload = match error {
            RefundMutationError::DepositNotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DEPOSIT_NOT_FOUND".into(),
                message: format!("订单 {subject_id} 没有押金记录"),
                field: Some("orderId".into()),
                context: None,
            },
            RefundMutationError::NotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REFUND_NOT_FOUND".into(),
                message: format!("退款申请 {subject_id} 不存在"),
                field: Some("refundId".into()),
                context: None,
            },
            RefundMutationError::NotPending(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REFUND_NOT_PENDING".into(),
                message: format!("退款申请状态为 {status}，不可执行该操作"),
                field: Some("refundId".into()),
                context: None,
            },
            RefundMutationError::NotApproved(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REFUND_NOT_APPROVED".into(),
                message: format!("退款申请状态为 {status}，不可执行"),
                field: Some("refundId".into()),
                context: None,
            },
            RefundMutationError::Storage(error) => return Self::repository_error(error),
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn request(&self, ctx: &ExecutionContext, input: RequestRefundInput) -> Result<Value, String> {
        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "退款金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let operator = ctx.user_id().unwrap_or("system");
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .refunds()
            .request(
                &input.order_id,
                input.amount,
                &input.reason,
                operator,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, &input.order_id))?;

        Ok(serde_json::json!({
            "ok": true,
            "refundId": outcome.refund_id,
            "depositId": outcome.deposit_id,
            "orderId": outcome.order_id,
            "amount": outcome.amount,
            "status": "pending",
        }))
    }

    fn approve(&self, ctx: &ExecutionContext, input: ApproveRefundInput) -> Result<Value, String> {
        let operator = ctx.user_id().unwrap_or("system");
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .refunds()
            .approve(&input.refund_id, operator, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.refund_id))?;
        Ok(serde_json::json!({
            "ok": true,
            "refundId": outcome.refund_id,
            "status": outcome.status,
        }))
    }

    fn reject(&self, ctx: &ExecutionContext, input: RejectRefundInput) -> Result<Value, String> {
        let operator = ctx.user_id().unwrap_or("system");
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .refunds()
            .reject(
                &input.refund_id,
                &input.reason,
                operator,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, &input.refund_id))?;
        Ok(serde_json::json!({
            "ok": true,
            "refundId": outcome.refund_id,
            "status": outcome.status,
        }))
    }

    fn execute(&self, ctx: &ExecutionContext, input: ExecuteRefundInput) -> Result<Value, String> {
        let operator = ctx.user_id().unwrap_or("system");
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .refunds()
            .execute_refund(&input.refund_id, operator, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.refund_id))?;
        Ok(serde_json::json!({
            "ok": true,
            "refundId": outcome.refund_id,
            "status": "executed",
            "amount": outcome.amount,
        }))
    }

    fn list(&self, ctx: &ExecutionContext, input: ListRefundsInput) -> Result<Value, String> {
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let scoped = self.scoped(ctx)?;
        let result = scoped
            .refunds()
            .list(
                input.order_id.as_deref(),
                input.status.as_deref(),
                page,
                page_size,
            )
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "refunds": result.refunds,
            "pagination": {
                "page": result.page,
                "pageSize": result.page_size,
                "total": result.total,
            },
        }))
    }
}

impl SystemModule for RefundCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureRefund::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureRefund::new().commands()
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
            "request" => self.request(ctx, Self::parse(payload)?),
            "approve" => self.approve(ctx, Self::parse(payload)?),
            "reject" => self.reject(ctx, Self::parse(payload)?),
            "execute" => self.execute(ctx, Self::parse(payload)?),
            "list" => self.list(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: refund.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureRefund::new().schema()
    }
}

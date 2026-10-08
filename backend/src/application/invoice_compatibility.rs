use std::sync::Arc;

use official_finance::invoice::{
    FeatureInvoice, GetInvoiceInput, IssueInvoiceInput, ListInvoicesInput, RedFlushInvoiceInput,
    VoidInvoiceInput,
};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{InvoiceMutationError, RepositoryError, RepositoryProvider};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct InvoiceCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl InvoiceCompatibilityModule {
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
            message: "invoice persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: InvoiceMutationError, invoice_id: &str, action: &str) -> String {
        let payload = match error {
            InvoiceMutationError::NotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_INVOICE_NOT_FOUND".into(),
                message: format!("发票 {invoice_id} 不存在"),
                field: Some("invoiceId".into()),
                context: None,
            },
            InvoiceMutationError::NotIssued(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_INVOICE_NOT_ISSUED".into(),
                message: match action {
                    "void" => format!("发票状态为 {status}，不可作废"),
                    "red_flush" => format!("发票状态为 {status}，不可冲销"),
                    _ => format!("发票状态为 {status}，不可执行该操作"),
                },
                field: Some("invoiceId".into()),
                context: None,
            },
            InvoiceMutationError::Storage(error) => return Self::repository_error(error),
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn issue(&self, ctx: &ExecutionContext, input: IssueInvoiceInput) -> Result<Value, String> {
        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "发票金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }
        if input.invoice_type != "普通发票" && input.invoice_type != "专用发票" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_INVALID_INVOICE_TYPE".into(),
                message: "发票类型必须为'普通发票'或'专用发票'".into(),
                field: Some("invoiceType".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .invoices()
            .issue(
                &input.order_id,
                input.amount,
                &input.invoice_type,
                input.tax_rate,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, "", "issue"))?;

        Ok(serde_json::json!({
            "ok": true,
            "invoiceId": outcome.invoice_id,
            "invoiceNo": outcome.invoice_no,
            "orderId": outcome.order_id,
            "amount": outcome.amount,
            "taxRate": outcome.tax_rate,
            "taxAmount": outcome.tax_amount,
            "status": "issued",
        }))
    }

    fn void(&self, ctx: &ExecutionContext, input: VoidInvoiceInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .invoices()
            .void(&input.invoice_id, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.invoice_id, "void"))?;

        Ok(serde_json::json!({
            "ok": true,
            "invoiceId": outcome.invoice_id,
            "status": "voided",
        }))
    }

    fn red_flush(
        &self,
        ctx: &ExecutionContext,
        input: RedFlushInvoiceInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .invoices()
            .red_flush(&input.invoice_id, &input.reason, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.invoice_id, "red_flush"))?;

        Ok(serde_json::json!({
            "ok": true,
            "originalInvoiceId": outcome.original_invoice_id,
            "redInvoiceId": outcome.red_invoice_id,
            "redInvoiceNo": outcome.red_invoice_no,
            "redAmount": outcome.red_amount,
            "status": "red_flushed",
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: GetInvoiceInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let invoice = if let Some(invoice_id) = input.invoice_id {
            scoped
                .invoices()
                .get_by_id(&invoice_id)
                .map_err(Self::repository_error)?
        } else if let Some(order_id) = input.order_id {
            scoped
                .invoices()
                .get_by_order(&order_id)
                .map_err(Self::repository_error)?
        } else {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_MISSING_PARAM".into(),
                message: "必须提供 invoiceId 或 orderId".into(),
                field: None,
                context: None,
            })
            .unwrap_or_default());
        };

        Ok(serde_json::json!({
            "ok": true,
            "invoice": invoice,
        }))
    }

    fn list(&self, ctx: &ExecutionContext, input: ListInvoicesInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let result = scoped
            .invoices()
            .list(
                input.status.as_deref(),
                input.date_from.as_deref(),
                input.date_to.as_deref(),
                page,
                page_size,
            )
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "invoices": result.invoices,
            "pagination": {
                "page": result.page,
                "pageSize": result.page_size,
                "total": result.total,
            },
        }))
    }
}

impl SystemModule for InvoiceCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureInvoice::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureInvoice::new().commands()
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
            "issue" => self.issue(ctx, Self::parse(payload)?),
            "void" => self.void(ctx, Self::parse(payload)?),
            "red_flush" => self.red_flush(ctx, Self::parse(payload)?),
            "get" => self.get(ctx, Self::parse(payload)?),
            "list" => self.list(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: invoice.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureInvoice::new().schema()
    }
}

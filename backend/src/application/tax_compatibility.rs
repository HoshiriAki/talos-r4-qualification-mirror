use std::sync::Arc;

use official_finance::tax::{
    CalculateTaxInput, ExportTaxCsvInput, FeatureTax, GetTaxConfigInput, UpsertTaxConfigInput,
};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{RepositoryError, RepositoryProvider};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct TaxCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl TaxCompatibilityModule {
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
            message: "tax persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn get_config(
        &self,
        ctx: &ExecutionContext,
        input: GetTaxConfigInput,
    ) -> Result<Value, String> {
        let tax_type = input.tax_type.as_deref().unwrap_or("vat");
        let scoped = self.scoped(ctx)?;
        let configs = scoped
            .taxes()
            .get_configs(tax_type)
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "configs": configs,
        }))
    }

    fn upsert_config(
        &self,
        ctx: &ExecutionContext,
        input: UpsertTaxConfigInput,
    ) -> Result<Value, String> {
        if input.rate <= 0.0 || input.rate > 1.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_INVALID_TAX_RATE".into(),
                message: "税率必须在 0.01 ~ 1.00 之间".into(),
                field: Some("rate".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let now = shanghai_now_iso();
        let effective_from = if input.effective_from.is_empty() {
            now[..10].to_owned()
        } else {
            input.effective_from.clone()
        };
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .taxes()
            .upsert_config(&input.tax_type, input.rate, &effective_from, &now)
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "configId": outcome.config_id,
            "taxType": outcome.tax_type,
            "rate": outcome.rate,
            "effectiveFrom": outcome.effective_from,
            "isActive": true,
        }))
    }

    fn calculate(&self, ctx: &ExecutionContext, input: CalculateTaxInput) -> Result<Value, String> {
        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let tax_rate = if let Some(rate) = input.tax_rate {
            rate
        } else {
            self.scoped(ctx)?
                .taxes()
                .active_rate("vat")
                .map_err(Self::repository_error)?
                .unwrap_or(0.13)
        };
        let pre_tax = (input.amount / (1.0 + tax_rate) * 100.0).round() / 100.0;
        let tax_amount = (input.amount - pre_tax).max(0.0);

        Ok(serde_json::json!({
            "ok": true,
            "totalAmount": input.amount,
            "preTaxAmount": pre_tax,
            "taxAmount": tax_amount,
            "taxRate": tax_rate,
        }))
    }

    fn export(&self, ctx: &ExecutionContext, input: ExportTaxCsvInput) -> Result<Value, String> {
        let period = if input.period_key.is_empty() {
            shanghai_now_iso()[..7].to_owned()
        } else {
            input.period_key
        };
        let scoped = self.scoped(ctx)?;
        let snapshot = scoped
            .taxes()
            .export_snapshot(&period)
            .map_err(Self::repository_error)?;

        let mut csv = String::new();
        csv.push_str("销项税额申报辅助表\n");
        csv.push_str(&format!("期间: {period}\n\n"));
        csv.push_str("发票号码,订单ID,票种,金额(含税),税率,税额,状态,开票日期\n");

        let mut total_tax = 0.0_f64;
        let mut total_amount = 0.0_f64;
        for row in &snapshot.invoices {
            csv.push_str(&format!(
                "{},{},{},{:.2},{:.2}%,{:.2},{},{}\n",
                row.invoice_no,
                row.order_id,
                row.invoice_type,
                row.amount,
                row.tax_rate * 100.0,
                row.tax_amount,
                row.status,
                row.issued_at
            ));
            total_amount += row.amount;
            total_tax += row.tax_amount;
        }

        csv.push('\n');
        csv.push_str(&format!(
            "合计,,,{:.2},,,{:.2},,\n",
            total_amount, total_tax
        ));
        csv.push_str("\n当前活跃税率配置\n");
        csv.push_str("税种,税率,生效日期\n");
        for config in &snapshot.configs {
            csv.push_str(&format!(
                "{},{:.2}%,{}\n",
                config.tax_type,
                config.rate * 100.0,
                config.effective_from
            ));
        }

        Ok(serde_json::json!({
            "ok": true,
            "csv": csv,
            "period": period,
            "invoiceCount": snapshot.invoices.len(),
            "totalTax": total_tax,
            "totalAmount": total_amount,
        }))
    }
}

impl SystemModule for TaxCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureTax::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureTax::new().commands()
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
            "get_config" => self.get_config(ctx, Self::parse(payload)?),
            "upsert_config" => self.upsert_config(ctx, Self::parse(payload)?),
            "calculate" => self.calculate(ctx, Self::parse(payload)?),
            "export" => self.export(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: tax.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureTax::new().schema()
    }
}

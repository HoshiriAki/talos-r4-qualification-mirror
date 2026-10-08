use std::sync::Arc;

use official_device::procurement::{
    FeatureProcurement, GetPurchaseInput, RecordPurchaseInput, SetReplacementValueInput,
};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};
use uuid::Uuid;

use crate::repositories::{
    NewProcurementRecord, ProcurementMutationError, RepositoryError, RepositoryProvider,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct ProcurementCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl ProcurementCompatibilityModule {
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

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "procurement persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: ProcurementMutationError, device_serial_no: &str) -> String {
        let payload = match error {
            ProcurementMutationError::Duplicate => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ASSET_ALREADY_EXISTS".into(),
                message: format!("设备 {device_serial_no} 已有采购记录"),
                field: Some("deviceSerialNo".into()),
                context: None,
            },
            ProcurementMutationError::NotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ASSET_NOT_FOUND".into(),
                message: format!("设备 {device_serial_no} 没有采购记录，请先录入"),
                field: Some("deviceSerialNo".into()),
                context: None,
            },
            ProcurementMutationError::Storage(error) => ErrorPayload {
                category: "sys".into(),
                code: error.code().into(),
                message: "procurement persistence unavailable".into(),
                field: None,
                context: None,
            },
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn parse<T: serde::de::DeserializeOwned>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload).map_err(|error| format!("VAL_DESERIALIZE: {error}"))
    }

    fn record_purchase(
        &self,
        ctx: &ExecutionContext,
        input: RecordPurchaseInput,
    ) -> Result<Value, String> {
        let now = shanghai_now_iso();
        let purchase_date = if input.purchase_date.is_empty() {
            now.clone()
        } else {
            input.purchase_date.clone()
        };
        let replacement_value = if input.replacement_value > 0.0 {
            input.replacement_value
        } else {
            input.purchase_price
        };
        let serial = input.device_serial_no.clone();

        let scoped = self.scoped(ctx)?;
        let record = scoped
            .procurements()
            .create(&NewProcurementRecord {
                id: Uuid::new_v4().to_string(),
                device_serial_no: input.device_serial_no,
                purchase_price: input.purchase_price,
                purchase_date,
                vendor: input.vendor,
                invoice_no: input.invoice_no,
                replacement_value,
                notes: input.notes,
                created_at: now,
            })
            .map_err(|error| Self::mutation_error(error, &serial))?;

        Ok(serde_json::json!({
            "ok": true,
            "id": record.id,
            "deviceSerialNo": record.device_serial_no,
            "purchasePrice": record.purchase_price,
            "replacementValue": record.replacement_value,
        }))
    }

    fn set_replacement_value(
        &self,
        ctx: &ExecutionContext,
        input: SetReplacementValueInput,
    ) -> Result<Value, String> {
        let serial = input.device_serial_no.clone();
        let scoped = self.scoped(ctx)?;
        let record = scoped
            .procurements()
            .set_replacement_value(&input.device_serial_no, input.replacement_value)
            .map_err(|error| Self::mutation_error(error, &serial))?;

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": record.device_serial_no,
            "replacementValue": record.replacement_value,
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: GetPurchaseInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let purchase = scoped
            .procurements()
            .get(&input.device_serial_no)
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "purchase": purchase,
        }))
    }
}

impl SystemModule for ProcurementCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureProcurement::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureProcurement::new().commands()
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
            "record_purchase" => self.record_purchase(ctx, Self::parse(payload)?),
            "set_replacement_value" => self.set_replacement_value(ctx, Self::parse(payload)?),
            "get" => self.get(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: procurement.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureProcurement::new().schema()
    }
}

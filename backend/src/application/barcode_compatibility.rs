use std::sync::Arc;

use chrono::Duration;
use chrono_tz::Asia::Shanghai;
use serde_json::Value;
use system_admin::barcode::{
    BarcodeGenerateInput, BarcodeLookupInput, FeatureBarcode, ScanEventInput, ScanHistoryInput,
};
use system_core::{
    ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule, Unvalidated,
};

use crate::repositories::{
    BarcodeMutationError, RepositoryError, RepositoryProvider, ScopedRepositories,
};

#[derive(Clone)]
pub(crate) struct BarcodeCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl BarcodeCompatibilityModule {
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

    fn repository_error(error: RepositoryError) -> String {
        Self::error(
            "sys",
            error.code(),
            "barcode persistence unavailable".into(),
            None,
        )
    }

    fn mutation_error(error: BarcodeMutationError) -> String {
        match error {
            BarcodeMutationError::DeviceNotFound => {
                Self::error("biz", "BIZ_NOT_FOUND", "Device not found".into(), None)
            }
            BarcodeMutationError::ScanReferencesNotFound => Self::error(
                "biz",
                "BIZ_NOT_FOUND",
                "Scan references not found".into(),
                None,
            ),
            BarcodeMutationError::Storage(error) => Self::repository_error(error),
        }
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

    fn generate(
        &self,
        ctx: &ExecutionContext,
        input: BarcodeGenerateInput,
    ) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        let scoped = self.scoped(ctx)?;
        let now = barcode_now();
        let (label, already_exists) = scoped
            .barcodes()
            .generate(&input.device_serial_no, &now)
            .map_err(Self::mutation_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "id": label.id,
            "deviceSerialNo": label.device_serial_no,
            "barcodeText": label.barcode_text,
            "barcodeType": label.barcode_type,
            "labelFormat": label.label_format,
            "generatedAt": label.generated_at,
            "alreadyExists": already_exists,
        }))
    }

    fn batch_generate(&self, ctx: &ExecutionContext) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        let scoped = self.scoped(ctx)?;
        let generated = scoped
            .barcodes()
            .batch_generate(&barcode_now())
            .map_err(Self::mutation_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "generated": generated,
        }))
    }

    fn lookup(&self, ctx: &ExecutionContext, input: BarcodeLookupInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let record = scoped
            .barcodes()
            .lookup(&input.barcode_text)
            .map_err(Self::repository_error)?
            .ok_or_else(|| {
                Self::error(
                    "biz",
                    "BIZ_NOT_FOUND",
                    "未找到该条码对应的设备".into(),
                    None,
                )
            })?;

        Ok(serde_json::json!({
            "ok": true,
            "record": record,
        }))
    }

    fn scan_event(&self, ctx: &ExecutionContext, input: ScanEventInput) -> Result<Value, String> {
        let scanned_by = ctx
            .user_id()
            .ok_or_else(|| Self::error("auth", "AUTH_REQUIRED", "需要登录".into(), None))?;
        let scoped = self.scoped(ctx)?;
        let event = scoped
            .barcodes()
            .record_scan(
                &input.device_serial_no,
                input.barcode_text.as_deref(),
                &input.scan_type,
                scanned_by,
                input.warehouse_id.as_deref(),
                input.notes.as_deref(),
                &barcode_now(),
            )
            .map_err(Self::mutation_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "id": event.id,
            "deviceSerialNo": event.device_serial_no,
            "barcodeText": event.barcode_text,
            "scanType": event.scan_type,
            "scannedBy": event.scanned_by,
            "warehouseId": event.warehouse_id,
            "notes": event.notes,
            "createdAt": event.created_at,
        }))
    }

    fn scan_history(
        &self,
        ctx: &ExecutionContext,
        input: ScanHistoryInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let result = scoped
            .barcodes()
            .history(
                input.device_serial_no.as_deref(),
                input.scan_type.as_deref(),
                input.start_date.as_deref(),
                input.end_date.as_deref(),
                input.page.unwrap_or(1).max(1),
                input.page_size.unwrap_or(20).clamp(1, 100),
            )
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "events": result.events,
            "total": result.total,
            "page": result.page,
            "pageSize": result.page_size,
        }))
    }

    fn scan_stats(&self, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let now = chrono::Utc::now().with_timezone(&Shanghai);
        let today = now.format("%Y-%m-%d").to_string();
        let week_start = (now - Duration::days(7))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let stats = scoped
            .barcodes()
            .stats(&today, &week_start)
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "todayScans": stats.today_scans,
            "thisWeekScans": stats.this_week_scans,
            "byType": stats.by_type,
        }))
    }
}

impl SystemModule for BarcodeCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureBarcode::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureBarcode::new().commands()
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
            "generate" => {
                let input: Unvalidated<BarcodeGenerateInput> = payload.try_into()?;
                self.generate(ctx, input.sanitize().validate()?.into_inner())
            }
            "batch_generate" => self.batch_generate(ctx),
            "lookup" => {
                let input: Unvalidated<BarcodeLookupInput> = payload.try_into()?;
                self.lookup(ctx, input.sanitize().validate()?.into_inner())
            }
            "scan_event" => {
                let input: Unvalidated<ScanEventInput> = payload.try_into()?;
                self.scan_event(ctx, input.sanitize().validate()?.into_inner())
            }
            "scan_history" => {
                let input: Unvalidated<ScanHistoryInput> = payload.try_into()?;
                self.scan_history(ctx, input.sanitize().validate()?.into_inner())
            }
            "scan_stats" => self.scan_stats(ctx),
            _ => Err(Self::error(
                "sys",
                "CMD_UNKNOWN",
                format!("Unknown command: {command}"),
                None,
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureBarcode::new().schema()
    }
}

fn barcode_now() -> String {
    chrono::Utc::now()
        .with_timezone(&Shanghai)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

use std::collections::HashSet;
use std::sync::Arc;

use official_device::excel_import::{
    FeatureExcelImport, ImportDevicesInput, ImportError, ImportResult,
};
use serde_json::Value;
use system_core::{
    DeserializeGuard, ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, Sanitize,
    SystemModule, Unvalidated, Validate,
};

use crate::repositories::{RepositoryError, RepositoryProvider};

use super::DeviceCompatibilityModule;

pub(crate) struct ExcelImportCompatibilityModule {
    legacy_parser: FeatureExcelImport,
    repository_provider: Arc<dyn RepositoryProvider>,
    device_module: DeviceCompatibilityModule,
}

impl ExcelImportCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            legacy_parser: FeatureExcelImport::new(),
            device_module: DeviceCompatibilityModule::new(repository_provider.clone()),
            repository_provider,
        }
    }

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: format!("excel import repository unavailable: {error}"),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn serialize<T: serde::Serialize>(value: &T) -> Result<Value, String> {
        serde_json::to_value(value).map_err(|error| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_SERIALIZE".into(),
                message: error.to_string(),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })
    }

    fn import_devices(
        &self,
        input: &ImportDevicesInput,
        ctx: &ExecutionContext,
    ) -> Result<ImportResult, String> {
        let scoped = self
            .repository_provider
            .bind(ctx)
            .map_err(Self::repository_error)?;

        let total = input.rows.len();
        let mut created = 0usize;
        let mut skipped = 0usize;
        let mut errors = Vec::new();
        let mut file_seen = HashSet::new();

        for (index, row) in input.rows.iter().enumerate() {
            let serial_no = &row.serial_no;
            if !file_seen.insert(serial_no.clone()) {
                skipped += 1;
                errors.push(ImportError {
                    row_index: index + 1,
                    serial_no: serial_no.clone(),
                    reason: "duplicate serialNo within file".into(),
                });
                continue;
            }

            match self.device_module.execute(
                "get_device",
                serde_json::json!({ "serialNo": serial_no }),
                ctx,
            ) {
                Ok(value) if !value.is_null() => {
                    skipped += 1;
                    errors.push(ImportError {
                        row_index: index + 1,
                        serial_no: serial_no.clone(),
                        reason: "device already exists".into(),
                    });
                    continue;
                }
                Ok(_) => {}
                Err(error) => return Err(error),
            }

            let model_id = match row.model_name.as_deref().filter(|value| !value.is_empty()) {
                Some(name) => scoped
                    .models()
                    .find_by_name(name)
                    .map_err(Self::repository_error)?
                    .map(|model| model.id)
                    .unwrap_or_default(),
                None => String::new(),
            };

            let warehouse_id = match row
                .warehouse_name
                .as_deref()
                .filter(|value| !value.is_empty())
            {
                Some(name) => scoped
                    .warehouses()
                    .find_by_name(name)
                    .map_err(Self::repository_error)?
                    .map(|warehouse| warehouse.id)
                    .unwrap_or_default(),
                None => String::new(),
            };

            // Preserve the established Registry ABI: DeviceImportRow.notes was historically
            // accepted but not forwarded by FeatureExcelImport into create_device.
            let create_payload = serde_json::json!({
                "serialNo": serial_no,
                "modelId": model_id,
                "warehouseId": warehouse_id,
                "status": row.status.clone().unwrap_or_else(|| "available".into()),
            });

            match self
                .device_module
                .execute("create_device", create_payload, ctx)
            {
                Ok(_) => created += 1,
                Err(error) => {
                    skipped += 1;
                    errors.push(ImportError {
                        row_index: index + 1,
                        serial_no: serial_no.clone(),
                        reason: format!("create failed: {error}"),
                    });
                }
            }
        }

        Ok(ImportResult {
            total,
            created,
            skipped,
            errors,
        })
    }

    fn parse_import(payload: Value) -> Result<ImportDevicesInput, String> {
        DeserializeGuard::default().check_raw(&payload)?;
        let unvalidated: Unvalidated<ImportDevicesInput> = payload.try_into()?;
        Ok(unvalidated.sanitize().validate()?.into_inner())
    }
}

impl SystemModule for ExcelImportCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        self.legacy_parser.metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        self.legacy_parser.commands()
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
            "parse_excel" | "validate_excel" => self.legacy_parser.execute(command, payload, ctx),
            "import_devices" => {
                let input = Self::parse_import(payload)?;
                Self::serialize(&self.import_devices(&input, ctx)?)
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("unknown command: {command}"),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn schema(&self) -> ModuleSchema {
        self.legacy_parser.schema()
    }
}

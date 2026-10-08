use std::sync::{Arc, Mutex};

use official_warehouse::warehouse_advanced::{
    CancelTransferInput, CancelTransferOutput, CapacityStats, FeatureWarehouseAdvanced,
    GetCapacityStatsInput, GetLowStockInput, LowStockAlert, SetAlertThresholdInput,
    SetThresholdOutput, TransferDeviceInput, TransferOutput,
};
use serde_json::Value;
use system_core::{
    DeserializeGuard, ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, Sanitize,
    SystemModule, Unvalidated, Validate,
};

use crate::repositories::{RepositoryError, RepositoryProvider, WarehouseDeviceMoveOutcome};
use crate::utils::time::shanghai_now_iso;

pub(crate) struct WarehouseAdvancedCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
    alert_threshold: Mutex<i64>,
}

impl WarehouseAdvancedCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
            alert_threshold: Mutex::new(5),
        }
    }

    fn parse<T>(payload: Value) -> Result<T, String>
    where
        T: serde::de::DeserializeOwned + Sanitize + Validate,
    {
        DeserializeGuard::default().check_raw(&payload)?;
        let unvalidated: Unvalidated<T> = payload.try_into()?;
        Ok(unvalidated.sanitize().validate()?.into_inner())
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
            message: "warehouse advanced persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn business_error(code: &str, message: String, field: Option<&str>) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "biz".into(),
            code: code.into(),
            message,
            field: field.map(str::to_owned),
            context: None,
        })
        .unwrap_or_default()
    }

    fn serialize<T: serde::Serialize>(value: T) -> Result<Value, String> {
        serde_json::to_value(value).map_err(|_| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_SERIALIZE".into(),
                message: "warehouse advanced response serialization failed".into(),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })
    }

    fn get_low_stock(
        &self,
        input: GetLowStockInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let threshold = *self
            .alert_threshold
            .lock()
            .map_err(|error| Self::business_error("SYS_LOCK", error.to_string(), None))?;
        let scoped = self.scoped(ctx)?;
        let rows = scoped
            .warehouses()
            .advanced_low_stock(input.warehouse_id.as_deref(), threshold)
            .map_err(Self::repository_error)?;
        Self::serialize(
            rows.into_iter()
                .map(|row| LowStockAlert {
                    warehouse_id: row.warehouse_id,
                    warehouse_name: row.warehouse_name,
                    available_count: row.available_count,
                    threshold,
                    shortage: threshold - row.available_count,
                })
                .collect::<Vec<_>>(),
        )
    }

    fn set_alert_threshold(&self, input: SetAlertThresholdInput) -> Result<Value, String> {
        let mut threshold = self
            .alert_threshold
            .lock()
            .map_err(|error| Self::business_error("SYS_LOCK", error.to_string(), None))?;
        *threshold = input.threshold;
        Self::serialize(SetThresholdOutput {
            success: true,
            threshold: input.threshold,
        })
    }

    fn transfer_device(
        &self,
        input: TransferDeviceInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let now = shanghai_now_iso();
        match scoped
            .warehouses()
            .move_device_between_warehouses(
                &input.serial_no,
                &input.from_warehouse_id,
                &input.to_warehouse_id,
            )
            .map_err(Self::repository_error)?
        {
            WarehouseDeviceMoveOutcome::Moved => Self::serialize(TransferOutput {
                success: true,
                serial_no: input.serial_no,
                from_warehouse_id: input.from_warehouse_id,
                to_warehouse_id: input.to_warehouse_id,
                transferred_at: now,
            }),
            WarehouseDeviceMoveOutcome::DeviceNotInSource => Err(Self::business_error(
                "BIZ_DEVICE_NOT_IN_SOURCE",
                format!(
                    "设备 {} 不在源仓库 {}",
                    input.serial_no, input.from_warehouse_id
                ),
                Some("serialNo"),
            )),
            WarehouseDeviceMoveOutcome::TargetWarehouseNotFound => Err(Self::business_error(
                "BIZ_WAREHOUSE_NOT_FOUND",
                format!("目标仓库 {} 不存在", input.to_warehouse_id),
                Some("toWarehouseId"),
            )),
        }
    }

    fn cancel_transfer(
        &self,
        input: CancelTransferInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let now = shanghai_now_iso();
        match scoped
            .warehouses()
            .move_device_between_warehouses(
                &input.serial_no,
                &input.original_to_warehouse_id,
                &input.original_from_warehouse_id,
            )
            .map_err(Self::repository_error)?
        {
            WarehouseDeviceMoveOutcome::Moved => Self::serialize(CancelTransferOutput {
                success: true,
                serial_no: input.serial_no,
                reverted_to_warehouse_id: input.original_from_warehouse_id,
                cancelled_at: now,
            }),
            WarehouseDeviceMoveOutcome::DeviceNotInSource => Err(Self::business_error(
                "BIZ_DEVICE_NOT_FOUND",
                "设备未找到，无法取消调拨".into(),
                Some("serialNo"),
            )),
            WarehouseDeviceMoveOutcome::TargetWarehouseNotFound => Err(Self::business_error(
                "BIZ_WAREHOUSE_NOT_FOUND",
                format!("原仓库 {} 不存在", input.original_from_warehouse_id),
                Some("originalFromWarehouseId"),
            )),
        }
    }

    fn capacity_stats(
        &self,
        _input: GetCapacityStatsInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let rows = scoped
            .warehouses()
            .advanced_capacity_stats()
            .map_err(Self::repository_error)?;
        Self::serialize(
            rows.into_iter()
                .map(|row| CapacityStats {
                    warehouse_id: row.warehouse_id,
                    warehouse_name: row.warehouse_name,
                    capacity: row.capacity,
                    total_devices: row.total_devices,
                    available_devices: row.available_devices,
                    rented_devices: row.rented_devices,
                    utilization_percent: if row.capacity > 0 {
                        (row.rented_devices as f64 / row.capacity as f64) * 100.0
                    } else {
                        0.0
                    },
                })
                .collect::<Vec<_>>(),
        )
    }
}

impl SystemModule for WarehouseAdvancedCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureWarehouseAdvanced::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureWarehouseAdvanced::new().commands()
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
            "get_low_stock" => self.get_low_stock(Self::parse(payload)?, ctx),
            "set_alert_threshold" => self.set_alert_threshold(Self::parse(payload)?),
            "transfer_device" => self.transfer_device(Self::parse(payload)?, ctx),
            "cancel_transfer" => self.cancel_transfer(Self::parse(payload)?, ctx),
            "get_capacity_stats" => self.capacity_stats(Self::parse(payload)?, ctx),
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
        FeatureWarehouseAdvanced::new().schema()
    }
}

use std::sync::Arc;

use official_warehouse::warehouse::{
    CreateWarehouseInput, DeleteOutput, DeleteWarehouseInput, FeatureWarehouse,
    GetWarehouseDevicesInput, GetWarehouseInput, GetWarehouseStatsInput, ListWarehousesInput,
    UpdateWarehouseInput,
};
use serde_json::Value;
use system_core::{
    DeserializeGuard, ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, Sanitize,
    SystemModule, Unvalidated, Validate,
};
use uuid::Uuid;

use crate::repositories::{
    NewWarehouse, RepositoryError, RepositoryProvider, WarehouseMutationError, WarehousePatch,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct WarehouseCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl WarehouseCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
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
            message: "warehouse persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: WarehouseMutationError) -> String {
        let payload = match error {
            WarehouseMutationError::NotFound => ErrorPayload {
                category: "val".into(),
                code: "VAL_NOT_FOUND".into(),
                message: "仓库不存在或无权访问".into(),
                field: Some("id".into()),
                context: None,
            },
            WarehouseMutationError::DuplicateName => ErrorPayload {
                category: "val".into(),
                code: "VAL_DUPLICATE".into(),
                message: "仓库名称已存在".into(),
                field: Some("name".into()),
                context: None,
            },
            WarehouseMutationError::Referenced(count) => ErrorPayload {
                category: "val".into(),
                code: "VAL_CONFLICT".into(),
                message: format!("该仓库下有 {count} 台设备，无法删除"),
                field: Some("id".into()),
                context: None,
            },
            WarehouseMutationError::RegionRuleNotFound => ErrorPayload {
                category: "val".into(),
                code: "VAL_NOT_FOUND".into(),
                message: "区域规则不存在".into(),
                field: None,
                context: None,
            },
            WarehouseMutationError::Storage(error) => ErrorPayload {
                category: "sys".into(),
                code: error.code().into(),
                message: "warehouse persistence unavailable".into(),
                field: None,
                context: None,
            },
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn serialize<T: serde::Serialize>(value: T) -> Result<Value, String> {
        serde_json::to_value(value).map_err(|_| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_SERIALIZE".into(),
                message: "warehouse response serialization failed".into(),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })
    }

    fn list(&self, _input: ListWarehousesInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        Self::serialize(scoped.warehouses().list().map_err(Self::repository_error)?)
    }

    fn get(&self, input: GetWarehouseInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let warehouse = scoped
            .warehouses()
            .get(&input.id)
            .map_err(Self::repository_error)?
            .ok_or_else(|| Self::mutation_error(WarehouseMutationError::NotFound))?;
        Self::serialize(warehouse)
    }

    fn create(&self, input: CreateWarehouseInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let warehouse = scoped
            .warehouses()
            .create(&NewWarehouse {
                id: Uuid::new_v4().to_string(),
                name: input.name,
                wh_type: input.wh_type,
                enabled: input.enabled,
                address: input.address,
                contact_name: input.contact_name,
                contact_phone: input.contact_phone,
                notes: input.notes,
                capacity: input.capacity,
                now: shanghai_now_iso(),
            })
            .map_err(Self::mutation_error)?;
        Self::serialize(warehouse)
    }

    fn update(&self, input: UpdateWarehouseInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let warehouse = scoped
            .warehouses()
            .update(
                &input.id,
                &WarehousePatch {
                    name: input.name,
                    wh_type: input.wh_type,
                    enabled: input.enabled,
                    address: input.address,
                    contact_name: input.contact_name,
                    contact_phone: input.contact_phone,
                    notes: input.notes,
                    capacity: input.capacity,
                    now: shanghai_now_iso(),
                },
            )
            .map_err(Self::mutation_error)?;
        Self::serialize(warehouse)
    }

    fn delete(&self, input: DeleteWarehouseInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        scoped
            .warehouses()
            .delete(&input.id)
            .map_err(Self::mutation_error)?;
        Self::serialize(DeleteOutput { success: true })
    }

    fn stats(
        &self,
        _input: GetWarehouseStatsInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        Self::serialize(
            scoped
                .warehouses()
                .stats()
                .map_err(Self::repository_error)?,
        )
    }

    fn devices(
        &self,
        input: GetWarehouseDevicesInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        Self::serialize(
            scoped
                .warehouses()
                .devices(&input.id, input.status.as_deref())
                .map_err(Self::mutation_error)?,
        )
    }
}

impl SystemModule for WarehouseCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureWarehouse::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureWarehouse::new().commands()
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
            "list_warehouses" => self.list(Self::parse(payload)?, ctx),
            "get_warehouse" => self.get(Self::parse(payload)?, ctx),
            "create_warehouse" => self.create(Self::parse(payload)?, ctx),
            "update_warehouse" => self.update(Self::parse(payload)?, ctx),
            "delete_warehouse" => self.delete(Self::parse(payload)?, ctx),
            "get_warehouse_stats" => self.stats(Self::parse(payload)?, ctx),
            "get_warehouse_devices" => self.devices(Self::parse(payload)?, ctx),
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
        FeatureWarehouse::new().schema()
    }
}

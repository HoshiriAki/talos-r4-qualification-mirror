use std::sync::Arc;

use official_device::device::{
    CreateDeviceInput, DeleteDeviceInput, DeleteOutput, DeviceMatchCandidate, FeatureDevice,
    GetDeviceInput, GetDeviceMatchCandidatesInput, ListDevicesInput, ListDevicesPagedInput,
    ResolveResult, ResolveSerialInput, UpdateDeviceInput, ValidateSerialNoInput,
    ValidateSerialNoOutput,
};
use serde_json::Value;
use system_core::{
    DeserializeGuard, ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, Sanitize,
    SystemModule, Unvalidated, Validate,
};
use uuid::Uuid;

use crate::repositories::{
    DeviceListRequest, DevicePagedRequest, DevicePatch, DeviceSortDirection, DeviceSortField,
    NewDevice, RepositoryError, RepositoryProvider,
};

#[derive(Clone)]
pub(crate) struct DeviceCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl DeviceCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    fn repository_error(code: &str, error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: code.into(),
            message: format!("device repository unavailable: {error}"),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn not_found(serial_no: &str) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "val".into(),
            code: "VAL_NOT_FOUND".into(),
            message: format!("设备不存在: {serial_no}"),
            field: Some("serialNo".into()),
            context: None,
        })
        .unwrap_or_default()
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
            .map_err(|error| Self::repository_error("SYS_DB_QUERY", error))
    }

    fn normalize_candidate(serial_no: &str) -> String {
        let mut normalized = serial_no.trim().to_string();
        normalized.retain(|character| !character.is_control());
        normalized = normalized.to_uppercase();
        normalized.retain(|character| character.is_ascii_alphanumeric());
        normalized
    }

    fn candidates(&self, ctx: &ExecutionContext) -> Result<Vec<DeviceMatchCandidate>, String> {
        let scoped = self.scoped(ctx)?;
        scoped
            .devices()
            .list_serials()
            .map_err(|error| Self::repository_error("SYS_DB_QUERY", error))
            .map(|serials| {
                serials
                    .into_iter()
                    .map(|serial_no| DeviceMatchCandidate {
                        normalized_serial_no: Self::normalize_candidate(&serial_no),
                        serial_no,
                    })
                    .collect()
            })
    }

    fn list(&self, input: ListDevicesInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let devices = scoped
            .devices()
            .list(&DeviceListRequest {
                status: input.status,
                model_id: input.model_id,
                warehouse_id: input.warehouse_id,
                serial_no: input.serial_no,
            })
            .map_err(|error| Self::repository_error("SYS_DB_QUERY", error))?;
        serde_json::to_value(devices).map_err(|error| {
            Self::repository_error(
                "SYS_SERIALIZE",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn list_paged(
        &self,
        input: ListDevicesPagedInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let sort_by = match input.sort_by.as_deref() {
            Some("rentalStatus") => DeviceSortField::RentalStatus,
            Some("notes") => DeviceSortField::Notes,
            Some("warningStatus") => DeviceSortField::WarningStatus,
            Some("createdAt") => DeviceSortField::CreatedAt,
            _ => DeviceSortField::SerialNo,
        };
        let sort_direction = if input.sort_order.as_deref() == Some("desc") {
            DeviceSortDirection::Desc
        } else {
            DeviceSortDirection::Asc
        };
        let page = scoped
            .devices()
            .list_paged(&DevicePagedRequest {
                page: input.page,
                page_size: input.page_size,
                keyword: input.keyword,
                rental_status: input.rental_status,
                notes: input.notes,
                warning_status: input.warning_status,
                sort_by,
                sort_direction,
            })
            .map_err(|error| Self::repository_error("SYS_DB_QUERY", error))?;
        serde_json::to_value(page).map_err(|error| {
            Self::repository_error(
                "SYS_SERIALIZE",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn get(&self, input: GetDeviceInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let device = scoped
            .devices()
            .get(&input.serial_no)
            .map_err(|error| Self::repository_error("SYS_DB_QUERY", error))?;
        serde_json::to_value(device).map_err(|error| {
            Self::repository_error(
                "SYS_SERIALIZE",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn create(&self, input: CreateDeviceInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let device = scoped
            .devices()
            .create(&NewDevice {
                id: Uuid::new_v4().to_string(),
                serial_no: input.serial_no,
                model_id: input.model_id,
                warehouse_id: input.warehouse_id,
                status: input.status.unwrap_or_else(|| "available".into()),
                created_at: chrono::Utc::now()
                    .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).expect("valid +08:00"))
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string(),
            })
            .map_err(|error| Self::repository_error("SYS_DB_INSERT", error))?;
        serde_json::to_value(device).map_err(|error| {
            Self::repository_error(
                "SYS_SERIALIZE",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn update(&self, input: UpdateDeviceInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let serial_no = input.serial_no;
        let device = scoped
            .devices()
            .update(
                &serial_no,
                &DevicePatch {
                    model_id: input.model_id,
                    warehouse_id: input.warehouse_id,
                    status: input.status,
                },
            )
            .map_err(|error| Self::repository_error("SYS_DB_UPDATE", error))?
            .ok_or_else(|| Self::not_found(&serial_no))?;
        serde_json::to_value(device).map_err(|error| {
            Self::repository_error(
                "SYS_SERIALIZE",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn delete(&self, input: DeleteDeviceInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let deleted = scoped
            .devices()
            .delete(&input.serial_no)
            .map_err(|error| Self::repository_error("SYS_DB_DELETE", error))?;
        if !deleted {
            return Err(Self::not_found(&input.serial_no));
        }
        serde_json::to_value(DeleteOutput { success: true }).map_err(|error| {
            Self::repository_error(
                "SYS_SERIALIZE",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn resolve(&self, input: ResolveSerialInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let normalized_input = input.serial_no;
        let candidates = self.candidates(ctx)?;

        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.serial_no == normalized_input)
        {
            return serde_json::to_value(ResolveResult {
                matched: true,
                serial_no: Some(candidate.serial_no.clone()),
                candidates,
            })
            .map_err(|error| {
                Self::repository_error(
                    "SYS_SERIALIZE",
                    RepositoryError::ContractViolation(error.to_string()),
                )
            });
        }
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.normalized_serial_no == normalized_input)
        {
            return serde_json::to_value(ResolveResult {
                matched: true,
                serial_no: Some(candidate.serial_no.clone()),
                candidates,
            })
            .map_err(|error| {
                Self::repository_error(
                    "SYS_SERIALIZE",
                    RepositoryError::ContractViolation(error.to_string()),
                )
            });
        }
        let prefix_matches = candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .normalized_serial_no
                    .starts_with(&normalized_input)
            })
            .collect::<Vec<_>>();
        let serial_no = (prefix_matches.len() == 1).then(|| prefix_matches[0].serial_no.clone());
        serde_json::to_value(ResolveResult {
            matched: serial_no.is_some(),
            serial_no,
            candidates,
        })
        .map_err(|error| {
            Self::repository_error(
                "SYS_SERIALIZE",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }
}

impl SystemModule for DeviceCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureDevice::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureDevice::new().commands()
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
            "list_devices" => self.list(Self::parse(payload)?, ctx),
            "list_devices_paged" => self.list_paged(Self::parse(payload)?, ctx),
            "get_device" => self.get(Self::parse(payload)?, ctx),
            "create_device" => self.create(Self::parse(payload)?, ctx),
            "update_device" => self.update(Self::parse(payload)?, ctx),
            "delete_device" => self.delete(Self::parse(payload)?, ctx),
            "resolve_serial" => self.resolve(Self::parse(payload)?, ctx),
            "get_match_candidates" => {
                let _: GetDeviceMatchCandidatesInput = Self::parse(payload)?;
                serde_json::to_value(self.candidates(ctx)?).map_err(|error| {
                    Self::repository_error(
                        "SYS_SERIALIZE",
                        RepositoryError::ContractViolation(error.to_string()),
                    )
                })
            }
            "validate_serial_no" => {
                let input: ValidateSerialNoInput = Self::parse(payload)?;
                serde_json::to_value(ValidateSerialNoOutput {
                    valid: !input.serial_no.is_empty()
                        && input
                            .serial_no
                            .chars()
                            .all(|character| character.is_ascii_alphanumeric()),
                })
                .map_err(|error| {
                    Self::repository_error(
                        "SYS_SERIALIZE",
                        RepositoryError::ContractViolation(error.to_string()),
                    )
                })
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
        FeatureDevice::new().schema()
    }
}

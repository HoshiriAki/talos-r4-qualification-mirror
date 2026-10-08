use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::domain::{AllocationId, ReservationId};
use crate::repositories::{RepositoryError, RepositoryProvider};

const MODULE_NAME: &str = "reservation_v2";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateFromOrderInput {
    order_id: String,
    #[serde(default = "default_hold_minutes")]
    hold_minutes: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateLegacyHoldInput {
    device_serial_no: String,
    start_date: String,
    end_date: String,
    #[serde(default = "default_hold_minutes")]
    hold_minutes: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReservationIdInput {
    reservation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfirmInput {
    reservation_id: String,
    order_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AllocateInput {
    reservation_id: String,
    device_serial_no: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AllocationIdInput {
    allocation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CapacityInput {
    model_id: String,
    start_date: String,
    end_date: String,
    #[serde(default = "default_quantity")]
    quantity: u32,
    device_serial_no: Option<String>,
}

fn default_hold_minutes() -> i64 {
    30
}

fn default_quantity() -> u32 {
    1
}

#[derive(Debug)]
enum ReservationError {
    InvalidInput(String),
    Repository(RepositoryError),
    Serialize(String),
    UnknownCommand(String),
}

impl ReservationError {
    fn payload(&self) -> ErrorPayload {
        let (category, code, message) = match self {
            Self::InvalidInput(detail) => ("val", "VAL_RESERVATION_INPUT", detail.clone()),
            Self::Repository(error) => {
                let category = if matches!(error, RepositoryError::ContractViolation(_)) {
                    "biz"
                } else {
                    "sys"
                };
                (category, error.code(), error.to_string())
            }
            Self::Serialize(detail) => ("sys", "SYS_SERIALIZE", detail.clone()),
            Self::UnknownCommand(command) => (
                "sys",
                "SYS_UNKNOWN_COMMAND",
                format!("unknown command: {command}"),
            ),
        };
        ErrorPayload {
            category: category.into(),
            code: code.into(),
            message,
            field: None,
            context: None,
        }
    }

    fn into_json(self) -> String {
        serde_json::to_string(&self.payload()).unwrap_or_else(|_| {
            r#"{"category":"sys","code":"SYS_SERIALIZE","message":"failed to serialize reservation error","field":null,"context":null}"#.into()
        })
    }
}

impl From<RepositoryError> for ReservationError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

#[derive(Clone)]
struct ReservationService {
    repositories: Arc<dyn RepositoryProvider>,
}

impl ReservationService {
    fn new(repositories: Arc<dyn RepositoryProvider>) -> Self {
        Self { repositories }
    }

    fn create_from_order(
        &self,
        ctx: &ExecutionContext,
        input: CreateFromOrderInput,
    ) -> Result<Value, ReservationError> {
        let scoped = self.repositories.bind(ctx)?;
        let reservation = scoped.reservations().create_from_order(
            ReservationId::new(),
            &input.order_id,
            input.hold_minutes,
        )?;
        serialize(reservation)
    }

    fn create_legacy_hold(
        &self,
        ctx: &ExecutionContext,
        input: CreateLegacyHoldInput,
    ) -> Result<Value, ReservationError> {
        let scoped = self.repositories.bind(ctx)?;
        let reservation = scoped.reservations().create_legacy_device_hold(
            ReservationId::new(),
            &input.device_serial_no,
            &input.start_date,
            &input.end_date,
            input.hold_minutes,
        )?;
        serialize(reservation)
    }

    fn get(
        &self,
        ctx: &ExecutionContext,
        input: ReservationIdInput,
    ) -> Result<Value, ReservationError> {
        let id = parse_reservation_id(&input.reservation_id)?;
        let scoped = self.repositories.bind(ctx)?;
        let value = scoped
            .reservations()
            .get(&id)?
            .ok_or_else(|| ReservationError::InvalidInput("reservation not found".into()))?;
        serialize(value)
    }

    fn capacity(
        &self,
        ctx: &ExecutionContext,
        input: CapacityInput,
    ) -> Result<Value, ReservationError> {
        if input.quantity == 0 {
            return Err(ReservationError::InvalidInput(
                "quantity must be greater than zero".into(),
            ));
        }
        let scoped = self.repositories.bind(ctx)?;
        let value = scoped.reservations().capacity(
            &input.model_id,
            &input.start_date,
            &input.end_date,
            input.quantity,
            input.device_serial_no.as_deref(),
        )?;
        serialize(value)
    }

    fn confirm(
        &self,
        ctx: &ExecutionContext,
        input: ConfirmInput,
    ) -> Result<Value, ReservationError> {
        let id = parse_reservation_id(&input.reservation_id)?;
        let scoped = self.repositories.bind(ctx)?;
        serialize(
            scoped
                .reservations()
                .confirm(&id, input.order_id.as_deref())?,
        )
    }

    fn expire_due(&self, ctx: &ExecutionContext) -> Result<Value, ReservationError> {
        let scoped = self.repositories.bind(ctx)?;
        let expired = scoped.reservations().expire_due()?;
        Ok(serde_json::json!({ "expired": expired }))
    }

    fn allocate(
        &self,
        ctx: &ExecutionContext,
        input: AllocateInput,
    ) -> Result<Value, ReservationError> {
        let reservation_id = parse_reservation_id(&input.reservation_id)?;
        let scoped = self.repositories.bind(ctx)?;
        serialize(scoped.reservations().allocate_device(
            &reservation_id,
            AllocationId::new(),
            &input.device_serial_no,
        )?)
    }

    fn release_allocation(
        &self,
        ctx: &ExecutionContext,
        input: AllocationIdInput,
    ) -> Result<Value, ReservationError> {
        let id = AllocationId::parse(input.allocation_id.trim())
            .map_err(|error| ReservationError::InvalidInput(error.to_string()))?;
        let scoped = self.repositories.bind(ctx)?;
        serialize(scoped.reservations().release_allocation(&id)?)
    }

    fn list_migration_exceptions(&self, ctx: &ExecutionContext) -> Result<Value, ReservationError> {
        let scoped = self.repositories.bind(ctx)?;
        serialize(scoped.reservations().list_migration_exceptions()?)
    }
}

fn parse_reservation_id(value: &str) -> Result<ReservationId, ReservationError> {
    ReservationId::parse(value.trim())
        .map_err(|error| ReservationError::InvalidInput(error.to_string()))
}

fn serialize<T: serde::Serialize>(value: T) -> Result<Value, ReservationError> {
    serde_json::to_value(value).map_err(|error| ReservationError::Serialize(error.to_string()))
}

pub struct ReservationV2Module {
    service: ReservationService,
}

impl ReservationV2Module {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            service: ReservationService::new(repository_provider),
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload)
            .map_err(|error| ReservationError::InvalidInput(error.to_string()).into_json())
    }
}

impl SystemModule for ReservationV2Module {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "1.0.0".into(),
            description: "Canonical tenant-scoped Reservation and Allocation authority".into(),
            author: "TALOS".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            read_command("availability"),
            read_command("get_reservation"),
            read_command("list_migration_exceptions"),
            write_command("create_from_order"),
            write_command("create_legacy_hold"),
            write_command("confirm_reservation"),
            write_command("expire_due"),
            write_command("allocate_device"),
            write_command("release_allocation"),
        ]
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let result = match command {
            "availability" => self.service.capacity(ctx, Self::parse(payload)?),
            "get_reservation" => self.service.get(ctx, Self::parse(payload)?),
            "list_migration_exceptions" => self.service.list_migration_exceptions(ctx),
            "create_from_order" => self.service.create_from_order(ctx, Self::parse(payload)?),
            "create_legacy_hold" => self.service.create_legacy_hold(ctx, Self::parse(payload)?),
            "confirm_reservation" => self.service.confirm(ctx, Self::parse(payload)?),
            "expire_due" => self.service.expire_due(ctx),
            "allocate_device" => self.service.allocate(ctx, Self::parse(payload)?),
            "release_allocation" => self.service.release_allocation(ctx, Self::parse(payload)?),
            other => Err(ReservationError::UnknownCommand(other.into())),
        };
        result.map_err(ReservationError::into_json)
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: MODULE_NAME.into(),
            description: "Canonical Reservation / Allocation V2 authority".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|command| CommandSchema {
                    name: command.name.to_string(),
                    description: "Reservation / Allocation V2 command".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                })
                .collect(),
        }
    }
}

fn read_command(name: &'static str) -> CommandMetadata {
    CommandMetadata::new(
        name,
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseRead],
        SimulationSupport::Blocked,
    )
}

fn write_command(name: &'static str) -> CommandMetadata {
    CommandMetadata::new(
        name,
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseWrite],
        SimulationSupport::Blocked,
    )
}

#[cfg(test)]
mod tests {
    use super::{CapacityInput, CreateFromOrderInput, default_hold_minutes, default_quantity};

    #[test]
    fn defaults_keep_short_holds_and_positive_capacity_requests() {
        assert_eq!(default_hold_minutes(), 30);
        assert_eq!(default_quantity(), 1);
        let input = CreateFromOrderInput {
            order_id: "order".into(),
            hold_minutes: default_hold_minutes(),
        };
        assert_eq!(input.hold_minutes, 30);
        let capacity = CapacityInput {
            model_id: "model".into(),
            start_date: "2026-08-08".into(),
            end_date: "2026-08-09".into(),
            quantity: default_quantity(),
            device_serial_no: None,
        };
        assert_eq!(capacity.quantity, 1);
    }
}

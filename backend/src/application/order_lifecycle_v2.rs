use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::repositories::{RepositoryError, RepositoryProvider};

const MODULE_NAME: &str = "order_lifecycle_v2";

const WRITE_COMMANDS: &[&str] = &[
    "submit_order",
    "confirm_order",
    "cancel_order",
    "require_contract",
    "generate_contract",
    "send_contract",
    "sign_contract",
    "verify_contract",
    "mark_awaiting_payment",
    "record_partial_payment",
    "record_paid",
    "mark_reserved",
    "mark_allocated",
    "mark_ready_to_ship",
    "mark_shipped",
    "mark_in_use",
    "mark_return_pending",
    "mark_returned",
    "mark_inspected",
    "begin_settlement",
    "settle_order",
    "mark_completed",
    "close_order",
    "require_review",
    "mark_overdue",
    "mark_damage_review",
    "mark_repairing",
    "mark_disputed",
    "resolve_risk",
];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrderIdInput {
    order_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActionInput {
    order_id: String,
    expected_version: i64,
    #[serde(default)]
    reason: String,
}

#[derive(Debug)]
enum LifecycleError {
    InvalidInput(String),
    Repository(RepositoryError),
    Serialize(String),
    UnknownCommand(String),
}

impl From<RepositoryError> for LifecycleError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

impl LifecycleError {
    fn payload(&self) -> ErrorPayload {
        let (category, code, message) = match self {
            Self::InvalidInput(detail) => ("val", "VAL_ORDER_LIFECYCLE_INPUT", detail.clone()),
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
                format!("unknown lifecycle command: {command}"),
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
            r#"{"category":"sys","code":"SYS_SERIALIZE","message":"failed to serialize lifecycle error","field":null,"context":null}"#.into()
        })
    }
}

#[derive(Clone)]
struct OrderLifecycleService {
    repositories: Arc<dyn RepositoryProvider>,
}

impl OrderLifecycleService {
    fn new(repositories: Arc<dyn RepositoryProvider>) -> Self {
        Self { repositories }
    }

    fn get(&self, ctx: &ExecutionContext, input: OrderIdInput) -> Result<Value, LifecycleError> {
        let order_id = normalized_order_id(&input.order_id)?;
        let scoped = self.repositories.bind(ctx)?;
        serialize(scoped.lifecycles().operational_view(order_id)?)
    }

    fn history(
        &self,
        ctx: &ExecutionContext,
        input: OrderIdInput,
    ) -> Result<Value, LifecycleError> {
        let order_id = normalized_order_id(&input.order_id)?;
        let scoped = self.repositories.bind(ctx)?;
        serialize(scoped.lifecycles().history(order_id)?)
    }

    fn migration_exceptions(&self, ctx: &ExecutionContext) -> Result<Value, LifecycleError> {
        let scoped = self.repositories.bind(ctx)?;
        serialize(scoped.lifecycles().list_migration_exceptions()?)
    }

    fn apply(
        &self,
        ctx: &ExecutionContext,
        command: &str,
        input: ActionInput,
    ) -> Result<Value, LifecycleError> {
        if !WRITE_COMMANDS.contains(&command) {
            return Err(LifecycleError::UnknownCommand(command.into()));
        }
        let order_id = normalized_order_id(&input.order_id)?;
        if input.expected_version < 1 {
            return Err(LifecycleError::InvalidInput(
                "expectedVersion must be positive".into(),
            ));
        }
        let scoped = self.repositories.bind(ctx)?;
        let actor = ctx.user_id().unwrap_or("system");
        serialize(scoped.lifecycles().apply_action(
            order_id,
            command,
            input.expected_version,
            actor,
            &input.reason,
        )?)
    }
}

fn normalized_order_id(value: &str) -> Result<&str, LifecycleError> {
    let value = value.trim();
    if value.is_empty() {
        Err(LifecycleError::InvalidInput(
            "orderId must not be empty".into(),
        ))
    } else {
        Ok(value)
    }
}

fn serialize<T: serde::Serialize>(value: T) -> Result<Value, LifecycleError> {
    serde_json::to_value(value).map_err(|error| LifecycleError::Serialize(error.to_string()))
}

pub struct OrderLifecycleV2Module {
    service: OrderLifecycleService,
}

impl OrderLifecycleV2Module {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            service: OrderLifecycleService::new(repository_provider),
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload)
            .map_err(|error| LifecycleError::InvalidInput(error.to_string()).into_json())
    }
}

impl SystemModule for OrderLifecycleV2Module {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "1.0.0".into(),
            description: "Canonical multi-dimensional Order lifecycle authority".into(),
            author: "TALOS".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        let mut commands = vec![
            read_command("get_lifecycle"),
            read_command("list_history"),
            read_command("list_migration_exceptions"),
        ];
        commands.extend(WRITE_COMMANDS.iter().copied().map(write_command));
        commands
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let result = match command {
            "get_lifecycle" => self.service.get(ctx, Self::parse(payload)?),
            "list_history" => self.service.history(ctx, Self::parse(payload)?),
            "list_migration_exceptions" => self.service.migration_exceptions(ctx),
            command if WRITE_COMMANDS.contains(&command) => {
                self.service.apply(ctx, command, Self::parse(payload)?)
            }
            other => Err(LifecycleError::UnknownCommand(other.into())),
        };
        result.map_err(LifecycleError::into_json)
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: MODULE_NAME.into(),
            description: "Order Lifecycle V2 named-guard authority".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|command| CommandSchema {
                    name: command.name.to_string(),
                    description: "Order Lifecycle V2 command".into(),
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
    use super::WRITE_COMMANDS;

    #[test]
    fn named_commands_are_explicit_and_no_generic_transition_exists() {
        assert!(WRITE_COMMANDS.contains(&"mark_ready_to_ship"));
        assert!(WRITE_COMMANDS.contains(&"close_order"));
        assert!(!WRITE_COMMANDS.contains(&"transition"));
        assert!(!WRITE_COMMANDS.contains(&"update_status"));
    }
}

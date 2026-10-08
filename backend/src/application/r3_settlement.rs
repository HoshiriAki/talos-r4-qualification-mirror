//! Registry-facing R3 Damage / Repair / Settlement commands.
//!
//! HTTP adapters may only call these named commands through `ModuleRegistry`.
//! The module binds the trusted `ExecutionContext` to scoped repositories; no
//! caller-supplied tenant ID or provider client participates in the workflow.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::repositories::{
    AdditionalChargeInput, DamageFindingInput, DepositDeductionInput, InspectionCompletionInput,
    LiabilityDecisionInput, OpenDisputeInput, RepairDecisionInput, RepairTransitionInput,
    RepositoryError, RepositoryProvider, ResolveDisputeInput, SettlementIdInput,
    SettlementProposalInput,
};

const MODULE_NAME: &str = "r3_settlement";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigureTimezoneInput {
    time_zone_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InspectionBeginInput {
    inspection_id: String,
    expected_version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClosureFactsInput {
    order_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CloseOrderInput {
    order_id: String,
    expected_version: i64,
}

#[derive(Debug)]
enum R3Error {
    Input(String),
    Repository(RepositoryError),
    Serialization(String),
    UnknownCommand(String),
    ClosureBlocked(String),
}

impl From<RepositoryError> for R3Error {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

impl R3Error {
    fn payload(&self) -> ErrorPayload {
        let (category, code, message) = match self {
            Self::Input(message) => ("val", "VAL_R3_SETTLEMENT_INPUT", message.clone()),
            Self::Repository(error) => (
                if matches!(error, RepositoryError::ContractViolation(_)) {
                    "biz"
                } else {
                    "sys"
                },
                error.code(),
                error.to_string(),
            ),
            Self::Serialization(message) => ("sys", "SYS_R3_SETTLEMENT_SERIALIZE", message.clone()),
            Self::UnknownCommand(command) => (
                "sys",
                "SYS_R3_SETTLEMENT_COMMAND",
                format!("unknown R3 settlement command: {command}"),
            ),
            Self::ClosureBlocked(detail) => ("biz", "R3_TERMINAL_CLOSURE_BLOCKED", detail.clone()),
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
        serde_json::to_string(&self.payload()).unwrap_or_else(|_| "{\"category\":\"sys\",\"code\":\"SYS_R3_SETTLEMENT_SERIALIZE\",\"message\":\"failed to serialize R3 error\"}".into())
    }
}

#[derive(Clone)]
struct R3SettlementService {
    repositories: Arc<dyn RepositoryProvider>,
}

impl R3SettlementService {
    fn bind(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<crate::repositories::ScopedRepositories, R3Error> {
        Ok(self.repositories.bind(ctx)?)
    }
    fn actor(ctx: &ExecutionContext) -> &str {
        ctx.user_id().unwrap_or("system")
    }
    fn value<T: serde::Serialize>(&self, value: T) -> Result<Value, R3Error> {
        serde_json::to_value(value).map_err(|error| R3Error::Serialization(error.to_string()))
    }

    fn configure_timezone(
        &self,
        ctx: &ExecutionContext,
        input: ConfigureTimezoneInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(serde_json::json!({ "timeZoneId": scoped.r3_settlements().configure_business_timezone(&input.time_zone_id, Self::actor(ctx))? }))
    }

    fn business_timezone(&self, ctx: &ExecutionContext) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            serde_json::json!({ "timeZoneId": scoped.r3_settlements().business_timezone()? }),
        )
    }

    fn begin_inspection(
        &self,
        ctx: &ExecutionContext,
        input: InspectionBeginInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(scoped.rental_closure().transition_inspection(
            &input.inspection_id,
            "in_progress",
            input.expected_version,
        )?)
    }

    fn complete_inspection(
        &self,
        ctx: &ExecutionContext,
        input: InspectionCompletionInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .complete_inspection(input, Self::actor(ctx))?,
        )
    }

    fn create_damage_finding(
        &self,
        ctx: &ExecutionContext,
        input: DamageFindingInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .create_damage_finding(input, Self::actor(ctx))?,
        )
    }

    fn decide_liability(
        &self,
        ctx: &ExecutionContext,
        input: LiabilityDecisionInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .decide_liability(input, Self::actor(ctx))?,
        )
    }

    fn decide_repair(
        &self,
        ctx: &ExecutionContext,
        input: RepairDecisionInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .decide_repair(input, Self::actor(ctx))?,
        )
    }

    fn transition_repair(
        &self,
        ctx: &ExecutionContext,
        input: RepairTransitionInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(scoped.r3_settlements().transition_repair(input)?)
    }

    fn propose_settlement(
        &self,
        ctx: &ExecutionContext,
        input: SettlementProposalInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .propose_settlement(input, Self::actor(ctx))?,
        )
    }

    fn accept_settlement(
        &self,
        ctx: &ExecutionContext,
        input: SettlementIdInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .accept_settlement(&input.settlement_case_id)?,
        )
    }

    fn deduct_deposit(
        &self,
        ctx: &ExecutionContext,
        input: DepositDeductionInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .deduct_deposit(input, Self::actor(ctx))?,
        )
    }

    fn admit_additional_charge(
        &self,
        ctx: &ExecutionContext,
        input: AdditionalChargeInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .admit_additional_charge(input, Self::actor(ctx))?,
        )
    }

    fn open_dispute(
        &self,
        ctx: &ExecutionContext,
        input: OpenDisputeInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .open_dispute(input, Self::actor(ctx))?,
        )
    }

    fn resolve_dispute(
        &self,
        ctx: &ExecutionContext,
        input: ResolveDisputeInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .resolve_dispute(input, Self::actor(ctx))?,
        )
    }

    fn complete_settlement(
        &self,
        ctx: &ExecutionContext,
        input: SettlementIdInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(
            scoped
                .r3_settlements()
                .complete_settlement(&input.settlement_case_id)?,
        )
    }

    fn closure_facts(
        &self,
        ctx: &ExecutionContext,
        input: ClosureFactsInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        self.value(scoped.r3_settlements().closure_facts(&input.order_id)?)
    }

    fn close_order(
        &self,
        ctx: &ExecutionContext,
        input: CloseOrderInput,
    ) -> Result<Value, R3Error> {
        let scoped = self.bind(ctx)?;
        scoped.r3_settlements().close_order_atomic(
            &input.order_id,
            input.expected_version,
            Self::actor(ctx),
        )?;
        self.value(scoped.lifecycles().operational_view(&input.order_id)?)
    }
}

pub struct R3SettlementModule {
    service: R3SettlementService,
}

impl R3SettlementModule {
    pub fn new(repositories: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            service: R3SettlementService { repositories },
        }
    }
    fn parse<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, R3Error> {
        serde_json::from_value(payload).map_err(|error| R3Error::Input(error.to_string()))
    }
}

impl SystemModule for R3SettlementModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "1.0.0".into(),
            description: "R3 authoritative Damage, Repair, Dispute, and Settlement workflow".into(),
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
            read("get_business_timezone"),
            read("get_closure_facts"),
            admin_write("configure_business_timezone"),
            write("begin_inspection"),
            write("complete_inspection"),
            write("create_damage_finding"),
            admin_write("decide_liability"),
            write("decide_repair"),
            write("transition_repair"),
            write("propose_settlement"),
            admin_write("accept_settlement"),
            admin_write("deduct_deposit"),
            admin_write("admit_additional_charge"),
            write("open_dispute"),
            admin_write("resolve_dispute"),
            admin_write("complete_settlement"),
            admin_write("close_order"),
        ]
    }
    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let result: Result<Value, R3Error> = (|| match command {
            "configure_business_timezone" => {
                self.service.configure_timezone(ctx, Self::parse(payload)?)
            }
            "get_business_timezone" => self.service.business_timezone(ctx),
            "begin_inspection" => self.service.begin_inspection(ctx, Self::parse(payload)?),
            "complete_inspection" => self.service.complete_inspection(ctx, Self::parse(payload)?),
            "create_damage_finding" => self
                .service
                .create_damage_finding(ctx, Self::parse(payload)?),
            "decide_liability" => self.service.decide_liability(ctx, Self::parse(payload)?),
            "decide_repair" => self.service.decide_repair(ctx, Self::parse(payload)?),
            "transition_repair" => self.service.transition_repair(ctx, Self::parse(payload)?),
            "propose_settlement" => self.service.propose_settlement(ctx, Self::parse(payload)?),
            "accept_settlement" => self.service.accept_settlement(ctx, Self::parse(payload)?),
            "deduct_deposit" => self.service.deduct_deposit(ctx, Self::parse(payload)?),
            "admit_additional_charge" => self
                .service
                .admit_additional_charge(ctx, Self::parse(payload)?),
            "open_dispute" => self.service.open_dispute(ctx, Self::parse(payload)?),
            "resolve_dispute" => self.service.resolve_dispute(ctx, Self::parse(payload)?),
            "complete_settlement" => self.service.complete_settlement(ctx, Self::parse(payload)?),
            "get_closure_facts" => self.service.closure_facts(ctx, Self::parse(payload)?),
            "close_order" => self.service.close_order(ctx, Self::parse(payload)?),
            other => Err(R3Error::UnknownCommand(other.into())),
        })();
        result.map_err(R3Error::into_json)
    }
    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: MODULE_NAME.into(),
            description: "R3 named damage, settlement, and closure commands".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|command| CommandSchema {
                    name: command.name.to_string(),
                    description: "R3 settlement command".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                })
                .collect(),
        }
    }
}

fn read(name: &'static str) -> CommandMetadata {
    CommandMetadata::new(
        name,
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseRead],
        SimulationSupport::Blocked,
    )
}
fn write(name: &'static str) -> CommandMetadata {
    CommandMetadata::new(
        name,
        AccessRequirement::Authenticated,
        &[EffectClass::DatabaseWrite],
        SimulationSupport::Blocked,
    )
}
fn admin_write(name: &'static str) -> CommandMetadata {
    CommandMetadata::new(
        name,
        AccessRequirement::TenantAdmin,
        &[EffectClass::DatabaseWrite],
        SimulationSupport::Blocked,
    )
}

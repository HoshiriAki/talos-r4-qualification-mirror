use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::domain::{
    ContactKind, CustomerId, CustomerRiskStatus, CustomerStatus, normalize_contact_value,
};
use crate::repositories::{
    NewCustomerContact, NewCustomerRecord, RepositoryError, RepositoryProvider,
};

const MODULE_NAME: &str = "customer";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContactInput {
    kind: ContactKind,
    value: String,
    #[serde(default)]
    is_primary: bool,
}

impl ContactInput {
    fn normalize(self) -> Result<NewCustomerContact, CustomerError> {
        let normalized_value =
            normalize_contact_value(self.kind, &self.value).map_err(CustomerError::InvalidInput)?;
        Ok(NewCustomerContact {
            kind: self.kind,
            raw_value: self.value.trim().to_string(),
            normalized_value,
            is_primary: self.is_primary,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCustomerInput {
    legal_name: String,
    display_name: Option<String>,
    #[serde(default)]
    contacts: Vec<ContactInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListCustomersInput {
    #[serde(default = "default_limit")]
    limit: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CustomerIdInput {
    customer_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddContactInput {
    customer_id: String,
    contact: ContactInput,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DuplicateCandidateInput {
    kind: ContactKind,
    value: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddExternalIdentityInput {
    customer_id: String,
    provider: String,
    external_subject: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveMigrationExceptionInput {
    exception_id: String,
    customer_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateCustomerOutput {
    customer: crate::repositories::CustomerProjection,
    duplicate_candidates: Vec<crate::repositories::DuplicateCustomerCandidate>,
}

fn default_limit() -> u32 {
    100
}

#[derive(Debug)]
enum CustomerError {
    InvalidInput(String),
    NotFound(String),
    Repository(RepositoryError),
    Serialize(String),
    UnknownCommand(String),
}

impl CustomerError {
    fn payload(&self) -> ErrorPayload {
        match self {
            Self::InvalidInput(detail) => ErrorPayload {
                category: "val".into(),
                code: "VAL_CUSTOMER_INPUT".into(),
                message: detail.clone(),
                field: None,
                context: None,
            },
            Self::NotFound(id) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_CUSTOMER_NOT_FOUND".into(),
                message: format!("customer {id} does not exist in the active tenant"),
                field: None,
                context: None,
            },
            Self::Repository(error) => ErrorPayload {
                category: "sys".into(),
                code: error.code().into(),
                message: error.to_string(),
                field: None,
                context: None,
            },
            Self::Serialize(detail) => ErrorPayload {
                category: "sys".into(),
                code: "SYS_SERIALIZE".into(),
                message: detail.clone(),
                field: None,
                context: None,
            },
            Self::UnknownCommand(command) => ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("unknown command: {command}"),
                field: None,
                context: None,
            },
        }
    }

    fn into_json(self) -> String {
        serde_json::to_string(&self.payload()).unwrap_or_else(|_| {
            r#"{"category":"sys","code":"SYS_SERIALIZE","message":"failed to serialize customer error","field":null,"context":null}"#.into()
        })
    }
}

impl From<RepositoryError> for CustomerError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

#[derive(Clone)]
struct CustomerService {
    repositories: Arc<dyn RepositoryProvider>,
}

impl CustomerService {
    fn new(repositories: Arc<dyn RepositoryProvider>) -> Self {
        Self { repositories }
    }

    fn create(
        &self,
        ctx: &ExecutionContext,
        input: CreateCustomerInput,
    ) -> Result<CreateCustomerOutput, CustomerError> {
        let legal_name = input.legal_name.trim().to_string();
        if legal_name.is_empty() {
            return Err(CustomerError::InvalidInput(
                "legalName must not be blank".into(),
            ));
        }
        let display_name = input
            .display_name
            .unwrap_or_else(|| legal_name.clone())
            .trim()
            .to_string();
        if display_name.is_empty() {
            return Err(CustomerError::InvalidInput(
                "displayName must not be blank".into(),
            ));
        }

        let contacts = input
            .contacts
            .into_iter()
            .map(ContactInput::normalize)
            .collect::<Result<Vec<_>, _>>()?;
        let scoped = self.repositories.bind(ctx)?;
        let repo = scoped.customers();
        let mut duplicate_candidates = Vec::new();
        for contact in &contacts {
            duplicate_candidates
                .extend(repo.duplicate_candidates(contact.kind, &contact.normalized_value)?);
        }
        duplicate_candidates.sort_by(|left, right| {
            left.customer_id
                .as_str()
                .cmp(right.customer_id.as_str())
                .then_with(|| left.matched_contact.cmp(&right.matched_contact))
        });
        duplicate_candidates.dedup_by(|left, right| {
            left.customer_id == right.customer_id && left.matched_contact == right.matched_contact
        });

        let customer = repo.create(&NewCustomerRecord {
            id: CustomerId::new(),
            legal_name,
            display_name,
            status: CustomerStatus::Active,
            risk_status: CustomerRiskStatus::Clear,
            contacts,
            actor_identity_id: ctx.user_id().map(str::to_owned),
        })?;
        Ok(CreateCustomerOutput {
            customer,
            duplicate_candidates,
        })
    }

    fn list(
        &self,
        ctx: &ExecutionContext,
        input: ListCustomersInput,
    ) -> Result<Vec<crate::repositories::CustomerProjection>, CustomerError> {
        Ok(self.repositories.bind(ctx)?.customers().list(input.limit)?)
    }

    fn get(
        &self,
        ctx: &ExecutionContext,
        input: CustomerIdInput,
    ) -> Result<crate::repositories::CustomerProjection, CustomerError> {
        let id = customer_id(&input.customer_id)?;
        self.repositories
            .bind(ctx)?
            .customers()
            .get(&id)?
            .ok_or_else(|| CustomerError::NotFound(id.as_str().into()))
    }

    fn add_contact(
        &self,
        ctx: &ExecutionContext,
        input: AddContactInput,
    ) -> Result<crate::repositories::CustomerProjection, CustomerError> {
        let id = customer_id(&input.customer_id)?;
        let contact = input.contact.normalize()?;
        Ok(self
            .repositories
            .bind(ctx)?
            .customers()
            .add_contact(&id, &contact, ctx.user_id())?)
    }

    fn duplicate_candidates(
        &self,
        ctx: &ExecutionContext,
        input: DuplicateCandidateInput,
    ) -> Result<Vec<crate::repositories::DuplicateCustomerCandidate>, CustomerError> {
        let normalized = normalize_contact_value(input.kind, &input.value)
            .map_err(CustomerError::InvalidInput)?;
        Ok(self
            .repositories
            .bind(ctx)?
            .customers()
            .duplicate_candidates(input.kind, &normalized)?)
    }

    fn add_external_identity(
        &self,
        ctx: &ExecutionContext,
        input: AddExternalIdentityInput,
    ) -> Result<Value, CustomerError> {
        let id = customer_id(&input.customer_id)?;
        self.repositories
            .bind(ctx)?
            .customers()
            .add_external_identity(&id, &input.provider, &input.external_subject, ctx.user_id())?;
        Ok(serde_json::json!({"ok": true}))
    }

    fn list_migration_exceptions(
        &self,
        ctx: &ExecutionContext,
        input: ListCustomersInput,
    ) -> Result<Vec<crate::repositories::CustomerMigrationExceptionProjection>, CustomerError> {
        Ok(self
            .repositories
            .bind(ctx)?
            .customers()
            .list_migration_exceptions(input.limit)?)
    }

    fn resolve_migration_exception(
        &self,
        ctx: &ExecutionContext,
        input: ResolveMigrationExceptionInput,
    ) -> Result<Value, CustomerError> {
        let id = customer_id(&input.customer_id)?;
        self.repositories
            .bind(ctx)?
            .customers()
            .resolve_migration_exception(&input.exception_id, &id, ctx.user_id())?;
        Ok(serde_json::json!({"ok": true, "customerId": id.as_str()}))
    }

    fn anonymize(
        &self,
        ctx: &ExecutionContext,
        input: CustomerIdInput,
    ) -> Result<crate::repositories::CustomerProjection, CustomerError> {
        let id = customer_id(&input.customer_id)?;
        Ok(self
            .repositories
            .bind(ctx)?
            .customers()
            .anonymize(&id, ctx.user_id())?)
    }
}

fn customer_id(value: &str) -> Result<CustomerId, CustomerError> {
    CustomerId::parse(value.trim()).map_err(|error| CustomerError::InvalidInput(error.to_string()))
}

pub struct CustomerModule {
    service: CustomerService,
}

impl CustomerModule {
    pub fn new(repositories: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            service: CustomerService::new(repositories),
        }
    }

    fn serialize<T: Serialize>(value: T) -> Result<Value, CustomerError> {
        serde_json::to_value(value).map_err(|error| CustomerError::Serialize(error.to_string()))
    }

    fn input<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, CustomerError> {
        serde_json::from_value(payload)
            .map_err(|error| CustomerError::InvalidInput(error.to_string()))
    }
}

impl SystemModule for CustomerModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "0.1.0".into(),
            description: "Tenant-scoped Customer domain with masked PII projections".into(),
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
            CommandMetadata::new(
                "list_customers",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "get_customer",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "find_duplicate_candidates",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "create_customer",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "add_contact",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "add_external_identity",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "list_migration_exceptions",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "resolve_migration_exception",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "anonymize_customer",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
        ]
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let result: Result<Value, CustomerError> = (|| match command {
            "list_customers" => Self::serialize(self.service.list(ctx, Self::input(payload)?)?),
            "get_customer" => Self::serialize(self.service.get(ctx, Self::input(payload)?)?),
            "find_duplicate_candidates" => Self::serialize(
                self.service
                    .duplicate_candidates(ctx, Self::input(payload)?)?,
            ),
            "create_customer" => Self::serialize(self.service.create(ctx, Self::input(payload)?)?),
            "add_contact" => Self::serialize(self.service.add_contact(ctx, Self::input(payload)?)?),
            "add_external_identity" => self
                .service
                .add_external_identity(ctx, Self::input(payload)?),
            "list_migration_exceptions" => Self::serialize(
                self.service
                    .list_migration_exceptions(ctx, Self::input(payload)?)?,
            ),
            "resolve_migration_exception" => self
                .service
                .resolve_migration_exception(ctx, Self::input(payload)?),
            "anonymize_customer" => {
                Self::serialize(self.service.anonymize(ctx, Self::input(payload)?)?)
            }
            _ => Err(CustomerError::UnknownCommand(command.to_string())),
        })();
        result.map_err(CustomerError::into_json)
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: MODULE_NAME.into(),
            description: "Tenant-scoped Customer domain".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|command| CommandSchema {
                    name: command.name.into(),
                    description: command.name.replace('_', " "),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                })
                .collect(),
        }
    }
}

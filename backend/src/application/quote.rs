use std::sync::Arc;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::domain::{CustomerId, Money, QuoteId, QuoteLineId, QuoteLineKind};
use crate::repositories::{
    NewQuote, NewQuoteLine, OrderFromQuoteProjection, QuoteProjection, RepositoryError,
    RepositoryProvider, ScopedQuoteRepository,
};

const MODULE_NAME: &str = "quote";
const DEFAULT_CURRENCY: &str = "CNY";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ModelLineInput {
    model_id: String,
    quantity: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccessoryLineInput {
    accessory_id: String,
    quantity: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateQuoteInput {
    customer_id: String,
    start_date: String,
    end_date: String,
    region: String,
    #[serde(default)]
    model_lines: Vec<ModelLineInput>,
    #[serde(default)]
    accessory_lines: Vec<AccessoryLineInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuoteIdInput {
    quote_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListQuotesInput {
    #[serde(default = "default_limit")]
    limit: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateOrderFromQuoteInput {
    quote_id: String,
    #[serde(default)]
    remark: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpsertAccessoryInput {
    id: String,
    sku: String,
    name: String,
    unit_price_minor: i64,
    #[serde(default = "default_currency")]
    currency: String,
    #[serde(default = "default_true")]
    active: bool,
}

fn default_limit() -> u32 {
    100
}

fn default_currency() -> String {
    DEFAULT_CURRENCY.into()
}

fn default_true() -> bool {
    true
}

#[derive(Debug)]
enum QuoteError {
    InvalidInput(String),
    NotFound(String),
    Pricing(String),
    Repository(RepositoryError),
    Serialize(String),
    UnknownCommand(String),
}

impl QuoteError {
    fn payload(&self) -> ErrorPayload {
        let (category, code, message) = match self {
            Self::InvalidInput(detail) => ("val", "VAL_QUOTE_INPUT", detail.clone()),
            Self::NotFound(detail) => ("biz", "BIZ_QUOTE_NOT_FOUND", detail.clone()),
            Self::Pricing(detail) => ("biz", "BIZ_SERVER_PRICING_FAILED", detail.clone()),
            Self::Repository(error) => ("sys", error.code(), error.to_string()),
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
            r#"{"category":"sys","code":"SYS_SERIALIZE","message":"failed to serialize quote error","field":null,"context":null}"#.into()
        })
    }
}

impl From<RepositoryError> for QuoteError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

#[derive(Clone)]
struct QuoteService {
    repositories: Arc<dyn RepositoryProvider>,
    pricing: Arc<dyn SystemModule>,
}

impl QuoteService {
    fn new(repositories: Arc<dyn RepositoryProvider>, pricing: Arc<dyn SystemModule>) -> Self {
        Self {
            repositories,
            pricing,
        }
    }

    fn create(
        &self,
        ctx: &ExecutionContext,
        input: CreateQuoteInput,
    ) -> Result<QuoteProjection, QuoteError> {
        validate_dates(&input.start_date, &input.end_date)?;
        if input.region.trim().is_empty() {
            return Err(QuoteError::InvalidInput("region must not be blank".into()));
        }
        if input.model_lines.is_empty() && input.accessory_lines.is_empty() {
            return Err(QuoteError::InvalidInput(
                "quote must contain at least one model or accessory line".into(),
            ));
        }
        let customer_id = CustomerId::parse(input.customer_id.trim())
            .map_err(|error| QuoteError::InvalidInput(error.to_string()))?;
        let scoped = self.repositories.bind(ctx)?;
        let quote_repo = scoped.quotes();
        let mut lines = Vec::new();

        for line in input.model_lines {
            validate_quantity(line.quantity)?;
            let model_id = line.model_id.trim();
            if model_id.is_empty() {
                return Err(QuoteError::InvalidInput("modelId must not be blank".into()));
            }
            let pricing_payload = serde_json::json!({
                "startDate": input.start_date,
                "endDate": input.end_date,
                "province": input.region,
                "modelId": model_id
            });
            let server_result = self
                .pricing
                .execute("estimate_pricing", pricing_payload, ctx)
                .map_err(QuoteError::Pricing)?;
            let total_major = server_result
                .get("totalPrice")
                .and_then(Value::as_f64)
                .ok_or_else(|| {
                    QuoteError::Pricing(
                        "pricing authority did not return numeric totalPrice".into(),
                    )
                })?;
            let unit_price = Money::from_major_f64(total_major, DEFAULT_CURRENCY)
                .map_err(QuoteError::Pricing)?;
            let subtotal = unit_price
                .checked_mul(line.quantity)
                .map_err(QuoteError::Pricing)?;
            lines.push(NewQuoteLine {
                id: QuoteLineId::new(),
                kind: QuoteLineKind::Model,
                reference_id: model_id.to_string(),
                description: format!("model:{model_id}"),
                quantity: line.quantity,
                unit_price,
                subtotal,
                price_snapshot_json: serde_json::json!({
                    "authority": "pricing.estimate_pricing",
                    "currency": DEFAULT_CURRENCY,
                    "input": {
                        "startDate": input.start_date,
                        "endDate": input.end_date,
                        "region": input.region,
                        "modelId": model_id
                    },
                    "result": server_result
                })
                .to_string(),
            });
        }

        for line in input.accessory_lines {
            validate_quantity(line.quantity)?;
            let accessory = quote_repo
                .get_accessory(line.accessory_id.trim())?
                .ok_or_else(|| {
                    QuoteError::NotFound(format!(
                        "accessory {} does not exist in the active tenant",
                        line.accessory_id
                    ))
                })?;
            if !accessory.active {
                return Err(QuoteError::InvalidInput(format!(
                    "accessory {} is inactive",
                    accessory.id
                )));
            }
            let subtotal = accessory
                .unit_price
                .checked_mul(line.quantity)
                .map_err(QuoteError::InvalidInput)?;
            lines.push(NewQuoteLine {
                id: QuoteLineId::new(),
                kind: QuoteLineKind::Accessory,
                reference_id: accessory.id.clone(),
                description: accessory.name.clone(),
                quantity: line.quantity,
                unit_price: accessory.unit_price.clone(),
                subtotal,
                price_snapshot_json: serde_json::json!({
                    "authority": "accessory_catalog",
                    "accessoryId": accessory.id,
                    "sku": accessory.sku,
                    "name": accessory.name,
                    "version": accessory.version,
                    "unitPrice": accessory.unit_price
                })
                .to_string(),
            });
        }

        Ok(quote_repo.create(&NewQuote {
            id: QuoteId::new(),
            customer_id,
            start_date: input.start_date,
            end_date: input.end_date,
            region: input.region.trim().to_string(),
            currency: DEFAULT_CURRENCY.into(),
            expires_at: ScopedQuoteRepository::default_expiry(),
            lines,
        })?)
    }

    fn list(
        &self,
        ctx: &ExecutionContext,
        input: ListQuotesInput,
    ) -> Result<Vec<QuoteProjection>, QuoteError> {
        Ok(self.repositories.bind(ctx)?.quotes().list(input.limit)?)
    }

    fn get(
        &self,
        ctx: &ExecutionContext,
        input: QuoteIdInput,
    ) -> Result<QuoteProjection, QuoteError> {
        let id = quote_id(&input.quote_id)?;
        self.repositories
            .bind(ctx)?
            .quotes()
            .get(&id)?
            .ok_or_else(|| QuoteError::NotFound(format!("quote {} not found", id.as_str())))
    }

    fn confirm(
        &self,
        ctx: &ExecutionContext,
        input: QuoteIdInput,
    ) -> Result<QuoteProjection, QuoteError> {
        let id = quote_id(&input.quote_id)?;
        Ok(self.repositories.bind(ctx)?.quotes().confirm(&id)?)
    }

    fn expire(
        &self,
        ctx: &ExecutionContext,
        input: QuoteIdInput,
    ) -> Result<QuoteProjection, QuoteError> {
        let id = quote_id(&input.quote_id)?;
        Ok(self.repositories.bind(ctx)?.quotes().expire(&id)?)
    }

    fn create_order(
        &self,
        ctx: &ExecutionContext,
        input: CreateOrderFromQuoteInput,
    ) -> Result<OrderFromQuoteProjection, QuoteError> {
        let id = quote_id(&input.quote_id)?;
        Ok(self
            .repositories
            .bind(ctx)?
            .quotes()
            .create_order_from_quote(&id, input.remark.trim())?)
    }

    fn upsert_accessory(
        &self,
        ctx: &ExecutionContext,
        input: UpsertAccessoryInput,
    ) -> Result<crate::repositories::AccessoryProjection, QuoteError> {
        let price =
            Money::new(input.unit_price_minor, input.currency).map_err(QuoteError::InvalidInput)?;
        Ok(self.repositories.bind(ctx)?.quotes().upsert_accessory(
            &input.id,
            &input.sku,
            &input.name,
            &price,
            input.active,
        )?)
    }
}

fn validate_dates(start: &str, end: &str) -> Result<(), QuoteError> {
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .map_err(|_| QuoteError::InvalidInput("startDate must be YYYY-MM-DD".into()))?;
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .map_err(|_| QuoteError::InvalidInput("endDate must be YYYY-MM-DD".into()))?;
    if end_date < start_date {
        return Err(QuoteError::InvalidInput(
            "endDate must not precede startDate".into(),
        ));
    }
    Ok(())
}

fn validate_quantity(quantity: u32) -> Result<(), QuoteError> {
    if quantity == 0 || quantity > 10_000 {
        return Err(QuoteError::InvalidInput(
            "line quantity must be between 1 and 10000".into(),
        ));
    }
    Ok(())
}

fn quote_id(value: &str) -> Result<QuoteId, QuoteError> {
    QuoteId::parse(value.trim()).map_err(|error| QuoteError::InvalidInput(error.to_string()))
}

pub struct QuoteModule {
    service: QuoteService,
}

impl QuoteModule {
    pub fn new(repositories: Arc<dyn RepositoryProvider>, pricing: Arc<dyn SystemModule>) -> Self {
        Self {
            service: QuoteService::new(repositories, pricing),
        }
    }

    fn input<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, QuoteError> {
        serde_json::from_value(payload).map_err(|error| QuoteError::InvalidInput(error.to_string()))
    }

    fn serialize<T: Serialize>(value: T) -> Result<Value, QuoteError> {
        serde_json::to_value(value).map_err(|error| QuoteError::Serialize(error.to_string()))
    }
}

impl SystemModule for QuoteModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "0.1.0".into(),
            description: "Server-authoritative Quote, Pricing snapshot and OrderLine boundary"
                .into(),
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
                "list_quotes",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "get_quote",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "create_quote",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead, EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "confirm_quote",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "expire_quote",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "create_order_from_quote",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "upsert_accessory",
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
        let result: Result<Value, QuoteError> = (|| match command {
            "list_quotes" => Self::serialize(self.service.list(ctx, Self::input(payload)?)?),
            "get_quote" => Self::serialize(self.service.get(ctx, Self::input(payload)?)?),
            "create_quote" => Self::serialize(self.service.create(ctx, Self::input(payload)?)?),
            "confirm_quote" => Self::serialize(self.service.confirm(ctx, Self::input(payload)?)?),
            "expire_quote" => Self::serialize(self.service.expire(ctx, Self::input(payload)?)?),
            "create_order_from_quote" => {
                Self::serialize(self.service.create_order(ctx, Self::input(payload)?)?)
            }
            "upsert_accessory" => {
                Self::serialize(self.service.upsert_accessory(ctx, Self::input(payload)?)?)
            }
            _ => Err(QuoteError::UnknownCommand(command.to_string())),
        })();
        result.map_err(QuoteError::into_json)
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: MODULE_NAME.into(),
            description: "Server-authoritative Quote and Pricing V2".into(),
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

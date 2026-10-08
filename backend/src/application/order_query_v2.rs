use std::collections::HashMap;
use std::sync::Arc;

use official_order::state_machine::status;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::repositories::{
    LifecycleAllowedAction, OrderListRequest, OrderReadPage, OrderReadProjection,
    OrderSortDirection, OrderSortField, RepositoryError, RepositoryProvider, ScopedRepositories,
};

use super::OrderQueryService;

const MODULE_NAME: &str = "order_query_v2";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrderV2ListInput {
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: u32,
    #[serde(default)]
    filter: Map<String, Value>,
    sort_by: Option<String>,
    sort_order: Option<String>,
}

fn default_page() -> u32 {
    1
}
fn default_page_size() -> u32 {
    30
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OrderV2GetInput {
    id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderLifecycleSummary {
    pub commercial_status: String,
    pub contract_status: String,
    pub financial_status: String,
    pub fulfilment_status: String,
    pub risk_status: String,
    pub version: i64,
    pub updated_at: String,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderSummary {
    pub id: String,
    pub order_no: String,
    pub start_date: String,
    pub end_date: String,
    pub delivery_date: String,
    pub pickup_methods: Vec<String>,
    pub address: String,
    pub notes: String,
    pub devices: Vec<String>,
    pub device_serial_no: String,
    pub device_models: HashMap<String, i64>,
    pub accessories: Vec<String>,
    pub total_price: f64,
    pub province: String,
    pub send_warehouse_id: String,
    pub return_warehouse_id: String,
    pub tracking_no: String,
    pub created_at: String,
    /// Legacy compatibility projection only. Lifecycle V2 is authoritative.
    pub status: String,
    pub lifecycle: OrderLifecycleSummary,
    pub allowed_actions: Vec<LifecycleAllowedAction>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderDetail {
    #[serde(flatten)]
    pub summary: OrderSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderV2Pagination {
    pub page: u32,
    pub page_size: u32,
    pub total: u32,
    pub total_pages: u32,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderV2Page {
    pub items: Vec<OrderSummary>,
    pub pagination: OrderV2Pagination,
}

#[derive(Debug)]
enum OrderV2Error {
    InvalidInput(String),
    InvalidStatus(String),
    UnknownPersistedStatus(String),
    NotFound(String),
    Repository(RepositoryError),
    Serialize(String),
}

impl OrderV2Error {
    fn payload(&self) -> ErrorPayload {
        match self {
            Self::InvalidInput(detail) => ErrorPayload {
                category: "val".into(),
                code: "VAL_ORDER_QUERY_V2_INPUT".into(),
                message: detail.clone(),
                field: None,
                context: None,
            },
            Self::InvalidStatus(value) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ORDER_STATUS_INVALID".into(),
                message: format!("unsupported order status filter: {value}"),
                field: Some("status".into()),
                context: None,
            },
            Self::UnknownPersistedStatus(value) => ErrorPayload {
                category: "sys".into(),
                code: "SYS_ORDER_STATUS_UNKNOWN".into(),
                message: format!("unknown persisted order status: {value}"),
                field: None,
                context: None,
            },
            Self::NotFound(id) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ORDER_NOT_FOUND".into(),
                message: format!("order {id} not found"),
                field: None,
                context: None,
            },
            Self::Repository(error) => ErrorPayload {
                category: if matches!(error, RepositoryError::ContractViolation(_)) {
                    "biz".into()
                } else {
                    "sys".into()
                },
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
        }
    }

    fn into_json(self) -> String {
        serde_json::to_string(&self.payload()).unwrap_or_else(|_| {
            r#"{"category":"sys","code":"SYS_SERIALIZE","message":"failed to serialize Order Query V2 error","field":null,"context":null}"#.into()
        })
    }
}

impl From<RepositoryError> for OrderV2Error {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

#[derive(Clone)]
pub(crate) struct OrderQueryV2Service {
    queries: OrderQueryService,
    repositories: Arc<dyn RepositoryProvider>,
}

impl OrderQueryV2Service {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            queries: OrderQueryService::new(repository_provider.clone()),
            repositories: repository_provider,
        }
    }

    fn list(
        &self,
        ctx: &ExecutionContext,
        input: OrderV2ListInput,
    ) -> Result<OrderV2Page, OrderV2Error> {
        let request = build_request(input)?;
        let page = self.queries.list(ctx, &request)?;
        let scoped = self.repositories.bind(ctx)?;
        project_page(page, &scoped)
    }

    fn get(
        &self,
        ctx: &ExecutionContext,
        input: OrderV2GetInput,
    ) -> Result<OrderDetail, OrderV2Error> {
        let id = input.id.trim();
        if id.is_empty() {
            return Err(OrderV2Error::InvalidInput(
                "order id must not be empty".into(),
            ));
        }
        let projection = self
            .queries
            .get_by_id(ctx, id)?
            .ok_or_else(|| OrderV2Error::NotFound(id.into()))?;
        let scoped = self.repositories.bind(ctx)?;
        Ok(OrderDetail {
            summary: project_order(projection, &scoped)?,
        })
    }
}

fn build_request(input: OrderV2ListInput) -> Result<OrderListRequest, OrderV2Error> {
    Ok(OrderListRequest {
        page: input.page.max(1),
        page_size: input.page_size.clamp(1, 200),
        offset: None,
        order_no: string_filter(&input.filter, "orderNo"),
        status: normalize_status(string_ref(&input.filter, "status"))?,
        keyword: string_filter(&input.filter, "keyword"),
        address: string_filter(&input.filter, "address"),
        province: string_filter(&input.filter, "province"),
        start_date_from: date_filter(&input.filter, "startDateFrom"),
        start_date_to: date_filter(&input.filter, "startDateTo"),
        end_date_from: date_filter(&input.filter, "endDateFrom"),
        end_date_to: date_filter(&input.filter, "endDateTo"),
        included_date: date_filter(&input.filter, "includedDate"),
        delivery_date_from: date_filter(&input.filter, "deliveryDateFrom"),
        delivery_date_to: date_filter(&input.filter, "deliveryDateTo"),
        delivery_date: date_filter(&input.filter, "deliveryDate"),
        pickup_methods: string_array_filter(&input.filter, "pickupMethods"),
        start_date: date_filter(&input.filter, "startDate"),
        end_date: date_filter(&input.filter, "endDate"),
        serial_no: string_filter(&input.filter, "serialNo"),
        tracking_no: string_filter(&input.filter, "trackingNo"),
        sort_by: match input.sort_by.as_deref().map(str::trim) {
            Some("startDate") => OrderSortField::StartDate,
            Some("endDate") => OrderSortField::EndDate,
            Some("totalPrice") => OrderSortField::TotalPrice,
            _ => OrderSortField::CreatedAt,
        },
        sort_direction: match input.sort_order.as_deref().map(str::trim) {
            Some(value) if value.eq_ignore_ascii_case("asc") => OrderSortDirection::Asc,
            _ => OrderSortDirection::Desc,
        },
    })
}

fn normalize_status(value: Option<&str>) -> Result<Option<String>, OrderV2Error> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let persisted = match value {
        "草稿" | "draft" | "已预约" | "reserved" => status::DRAFT,
        "已确认" | "confirmed" => status::CONFIRMED,
        "已付款" | "paid" => status::PAID,
        "已发货" | "shipped" => status::SHIPPED,
        "使用中" | "in_use" | "active" | "进行中" => status::IN_USE,
        "已归还" | "returned" => status::RETURNED,
        "检查中" | "inspected" => status::INSPECTED,
        "维修中" | "repairing" => status::REPAIRING,
        "已完成" | "completed" => status::COMPLETED,
        "已关闭" | "closed" => status::CLOSED,
        "已取消" | "cancelled" => status::CANCELLED,
        other => return Err(OrderV2Error::InvalidStatus(other.into())),
    };
    Ok(Some(persisted.into()))
}

fn project_page(
    page: OrderReadPage,
    scoped: &ScopedRepositories,
) -> Result<OrderV2Page, OrderV2Error> {
    let items = page
        .orders
        .into_iter()
        .map(|projection| project_order(projection, scoped))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(OrderV2Page {
        items,
        pagination: OrderV2Pagination {
            page: page.page,
            page_size: page.page_size,
            total: page.total,
            total_pages: page.total_pages,
            has_more: page.page < page.total_pages,
        },
    })
}

fn project_order(
    projection: OrderReadProjection,
    scoped: &ScopedRepositories,
) -> Result<OrderSummary, OrderV2Error> {
    if !is_known_status(&projection.status) {
        return Err(OrderV2Error::UnknownPersistedStatus(projection.status));
    }
    let operational = scoped.lifecycles().operational_view(&projection.id)?;
    let lifecycle = OrderLifecycleSummary {
        commercial_status: operational.lifecycle.commercial_status,
        contract_status: operational.lifecycle.contract_status,
        financial_status: operational.lifecycle.financial_status,
        fulfilment_status: operational.lifecycle.fulfilment_status,
        risk_status: operational.lifecycle.risk_status,
        version: operational.lifecycle.version,
        updated_at: operational.lifecycle.updated_at,
        blockers: operational.blockers,
    };
    Ok(OrderSummary {
        id: projection.id,
        order_no: projection.order_no,
        start_date: projection.start_date,
        end_date: projection.end_date,
        delivery_date: projection.delivery_date,
        pickup_methods: projection.pickup_methods,
        address: projection.address,
        notes: projection.notes,
        devices: projection.devices,
        device_serial_no: projection.device_serial_no,
        device_models: parse_map(&projection.device_models),
        accessories: parse_vec(&projection.accessories),
        total_price: projection.total_price,
        province: projection.province,
        send_warehouse_id: projection.send_warehouse_id,
        return_warehouse_id: projection.return_warehouse_id,
        tracking_no: projection.tracking_no,
        created_at: projection.created_at,
        status: projection.status,
        lifecycle,
        allowed_actions: operational.allowed_actions,
    })
}

fn is_known_status(value: &str) -> bool {
    matches!(
        value,
        status::DRAFT
            | status::CONFIRMED
            | status::PAID
            | status::SHIPPED
            | status::IN_USE
            | status::RETURNED
            | status::INSPECTED
            | status::REPAIRING
            | status::COMPLETED
            | status::CLOSED
            | status::CANCELLED
    )
}

fn parse_vec(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}
fn parse_map(raw: &str) -> HashMap<String, i64> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn string_ref<'a>(filter: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    filter
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
fn string_filter(filter: &Map<String, Value>, key: &str) -> Option<String> {
    string_ref(filter, key).map(str::to_owned)
}
fn date_filter(filter: &Map<String, Value>, key: &str) -> Option<String> {
    string_ref(filter, key)
        .filter(|value| is_date_key(value))
        .map(str::to_owned)
}
fn string_array_filter(filter: &Map<String, Value>, key: &str) -> Vec<String> {
    filter
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}
fn is_date_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

pub struct OrderQueryV2Module {
    service: OrderQueryV2Service,
}

impl OrderQueryV2Module {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            service: OrderQueryV2Service::new(repository_provider),
        }
    }
    fn serialize<T: Serialize>(value: &T) -> Result<Value, String> {
        serde_json::to_value(value)
            .map_err(|error| OrderV2Error::Serialize(error.to_string()).into_json())
    }
    fn invalid_payload(error: serde_json::Error) -> String {
        OrderV2Error::InvalidInput(error.to_string()).into_json()
    }
}

impl SystemModule for OrderQueryV2Module {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "1.1.0".into(),
            description: "Canonical tenant-scoped Order Query/API V2 with Lifecycle V2 projection"
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
                "list_orders",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "get_order",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
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
        match command {
            "list_orders" => {
                let input = serde_json::from_value(payload).map_err(Self::invalid_payload)?;
                Self::serialize(
                    &self
                        .service
                        .list(ctx, input)
                        .map_err(OrderV2Error::into_json)?,
                )
            }
            "get_order" => {
                let input = serde_json::from_value(payload).map_err(Self::invalid_payload)?;
                Self::serialize(
                    &self
                        .service
                        .get(ctx, input)
                        .map_err(OrderV2Error::into_json)?,
                )
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
        ModuleSchema {
            name: MODULE_NAME.into(),
            description: "Canonical Order Query/API V2 with server-owned lifecycle actions".into(),
            commands: vec![
                CommandSchema {
                    name: "list_orders".into(),
                    description: "Return canonical OrderSummary page with Lifecycle V2 actions"
                        .into(),
                    version: "1.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_order".into(),
                    description: "Return canonical OrderDetail with Lifecycle V2 actions".into(),
                    version: "1.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OrderV2ListInput, build_request};
    use serde_json::{Map, json};

    #[test]
    fn canonical_filter_normalizes_legacy_status_aliases_without_offset() {
        let mut filter = Map::new();
        filter.insert("status".into(), json!("进行中"));
        let request = build_request(OrderV2ListInput {
            page: 2,
            page_size: 25,
            filter,
            sort_by: None,
            sort_order: None,
        })
        .unwrap();
        assert_eq!(request.status.as_deref(), Some("in_use"));
        assert_eq!(request.page, 2);
        assert_eq!(request.page_size, 25);
        assert_eq!(request.offset, None);
    }
}

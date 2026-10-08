use std::sync::Arc;

use official_order::state_machine::{self, status};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::repositories::{
    OrderListRequest, OrderReadPage, OrderReadProjection, OrderSortDirection, OrderSortField,
    RepositoryError, RepositoryProvider,
};

use super::OrderQueryService;

const MODULE_NAME: &str = "order_read_compatibility";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityListInput {
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    page_size: u32,
    #[serde(default)]
    filter: Map<String, Value>,
    sort_by: Option<String>,
    sort_order: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityOffsetListInput {
    #[serde(default = "default_offset_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
    #[serde(default)]
    filter: Map<String, Value>,
    sort_by: Option<String>,
    sort_order: Option<String>,
}

fn default_offset_limit() -> i64 {
    50
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    30
}

#[derive(Debug, Clone, Deserialize)]
struct CompatibilityGetInput {
    id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityPagination {
    page: u32,
    page_size: u32,
    total: u32,
    total_pages: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct CompatibilityOrderPage {
    users: Vec<OrderReadProjection>,
    pagination: CompatibilityPagination,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct CompatibilityOffsetPage {
    users: Vec<OrderReadProjection>,
    limit: u32,
    total: u32,
}

#[derive(Debug)]
enum CompatibilityError {
    InvalidInput(String),
    InvalidStatus(String),
    UnknownPersistedStatus(String),
    OrderNotFound(String),
    Repository(RepositoryError),
    Serialize(String),
}

impl CompatibilityError {
    fn payload(&self) -> ErrorPayload {
        match self {
            Self::InvalidInput(detail) => ErrorPayload {
                category: "val".into(),
                code: "VAL_ORDER_READ_INPUT".into(),
                message: detail.clone(),
                field: None,
                context: None,
            },
            Self::InvalidStatus(status) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ORDER_STATUS_INVALID".into(),
                message: format!("unsupported order status filter: {status}"),
                field: Some("status".into()),
                context: None,
            },
            Self::UnknownPersistedStatus(status) => ErrorPayload {
                category: "sys".into(),
                code: "SYS_ORDER_STATUS_UNKNOWN".into(),
                message: format!("unknown persisted order status: {status}"),
                field: None,
                context: None,
            },
            Self::OrderNotFound(id) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ORDER_NOT_FOUND".into(),
                message: format!("订单 {id} 不存在"),
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
        }
    }

    fn into_json(self) -> String {
        serde_json::to_string(&self.payload()).unwrap_or_else(|_| {
            r#"{"category":"sys","code":"SYS_SERIALIZE","message":"failed to serialize compatibility error","field":null,"context":null}"#.into()
        })
    }
}

impl From<RepositoryError> for CompatibilityError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

#[derive(Debug, Clone, Copy)]
struct OrderStatusCodec;

impl OrderStatusCodec {
    fn normalize_filter(value: Option<&str>) -> Result<Option<String>, CompatibilityError> {
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
            other => return Err(CompatibilityError::InvalidStatus(other.into())),
        };
        Ok(Some(persisted.into()))
    }

    fn display_label(value: &str) -> Result<&str, CompatibilityError> {
        let persisted = if value.is_empty() {
            status::DRAFT
        } else {
            value
        };
        if !Self::is_persisted(persisted) {
            return Err(CompatibilityError::UnknownPersistedStatus(persisted.into()));
        }
        Ok(state_machine::display_label(persisted))
    }

    fn is_persisted(value: &str) -> bool {
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
}

#[derive(Clone)]
struct OrderReadCompatibilityService {
    queries: OrderQueryService,
}

impl OrderReadCompatibilityService {
    fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            queries: OrderQueryService::new(repository_provider),
        }
    }

    fn list(
        &self,
        ctx: &ExecutionContext,
        input: CompatibilityListInput,
    ) -> Result<CompatibilityOrderPage, CompatibilityError> {
        let request = self.list_request(input)?;
        let page = self.queries.list(ctx, &request)?;
        self.compatibility_page(page)
    }

    fn list_offset(
        &self,
        ctx: &ExecutionContext,
        input: CompatibilityOffsetListInput,
    ) -> Result<CompatibilityOffsetPage, CompatibilityError> {
        let limit = u32::try_from(input.limit.clamp(1, 200)).unwrap_or(50);
        let offset = u64::try_from(input.offset.max(0)).unwrap_or_default();
        let mut request = self.list_request(CompatibilityListInput {
            page: 1,
            page_size: limit,
            filter: input.filter,
            sort_by: input.sort_by,
            sort_order: input.sort_order,
        })?;
        request.offset = Some(offset);
        let page = self.queries.list(ctx, &request)?;
        let users = page
            .orders
            .into_iter()
            .map(Self::compatibility_projection)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CompatibilityOffsetPage {
            users,
            limit: page.page_size,
            total: page.total,
        })
    }

    fn get(
        &self,
        ctx: &ExecutionContext,
        input: CompatibilityGetInput,
    ) -> Result<OrderReadProjection, CompatibilityError> {
        let id = input.id.trim();
        if id.is_empty() {
            return Err(CompatibilityError::InvalidInput(
                "order id must not be empty".into(),
            ));
        }
        let projection = self
            .queries
            .get_by_id(ctx, id)?
            .ok_or_else(|| CompatibilityError::OrderNotFound(id.into()))?;
        Self::compatibility_projection(projection)
    }

    fn list_request(
        &self,
        input: CompatibilityListInput,
    ) -> Result<OrderListRequest, CompatibilityError> {
        let status = OrderStatusCodec::normalize_filter(string_filter(&input.filter, "status"))?;
        Ok(OrderListRequest {
            page: input.page.max(1),
            page_size: input.page_size.clamp(1, 200),
            offset: None,
            order_no: owned_string_filter(&input.filter, "orderNo"),
            status,
            keyword: owned_string_filter(&input.filter, "keyword"),
            address: owned_string_filter(&input.filter, "address"),
            province: owned_string_filter(&input.filter, "province"),
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
            serial_no: owned_string_filter(&input.filter, "serialNo"),
            tracking_no: owned_string_filter(&input.filter, "trackingNo"),
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

    fn compatibility_page(
        &self,
        page: OrderReadPage,
    ) -> Result<CompatibilityOrderPage, CompatibilityError> {
        let users = page
            .orders
            .into_iter()
            .map(Self::compatibility_projection)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CompatibilityOrderPage {
            users,
            pagination: CompatibilityPagination {
                page: page.page,
                page_size: page.page_size,
                total: page.total,
                total_pages: page.total_pages,
            },
        })
    }

    fn compatibility_projection(
        mut projection: OrderReadProjection,
    ) -> Result<OrderReadProjection, CompatibilityError> {
        projection.status = OrderStatusCodec::display_label(&projection.status)?.into();
        Ok(projection)
    }
}

fn string_filter<'a>(filter: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    filter
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn owned_string_filter(filter: &Map<String, Value>, key: &str) -> Option<String> {
    string_filter(filter, key).map(str::to_owned)
}

fn date_filter(filter: &Map<String, Value>, key: &str) -> Option<String> {
    string_filter(filter, key)
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

pub struct OrderReadCompatibilityModule {
    service: OrderReadCompatibilityService,
}

impl OrderReadCompatibilityModule {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            service: OrderReadCompatibilityService::new(repository_provider),
        }
    }

    fn serialize<T: Serialize>(value: &T) -> Result<Value, String> {
        serde_json::to_value(value)
            .map_err(|error| CompatibilityError::Serialize(error.to_string()).into_json())
    }

    fn invalid_payload(error: serde_json::Error) -> String {
        CompatibilityError::InvalidInput(error.to_string()).into_json()
    }
}

impl SystemModule for OrderReadCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "0.1.0".into(),
            description: "Registry adapter for scoped Order read compatibility".into(),
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
                "list_orders_offset",
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
                let result = self
                    .service
                    .list(ctx, input)
                    .map_err(CompatibilityError::into_json)?;
                Self::serialize(&result)
            }
            "list_orders_offset" => {
                let input = serde_json::from_value(payload).map_err(Self::invalid_payload)?;
                let result = self
                    .service
                    .list_offset(ctx, input)
                    .map_err(CompatibilityError::into_json)?;
                Self::serialize(&result)
            }
            "get_order" => {
                let input = serde_json::from_value(payload).map_err(Self::invalid_payload)?;
                let result = self
                    .service
                    .get(ctx, input)
                    .map_err(CompatibilityError::into_json)?;
                Self::serialize(&result)
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
            description: "Registry adapter for scoped Order read compatibility".into(),
            commands: vec![
                CommandSchema {
                    name: "list_orders".into(),
                    description: "Return the legacy users + pagination envelope from scoped reads"
                        .into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "list_orders_offset".into(),
                    description: "Return the legacy users + limit + total offset envelope from exact scoped reads"
                        .into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_order".into(),
                    description: "Return one scoped Order with legacy status and error semantics"
                        .into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;
    use serde_json::json;
    use system_core::audit::AuditResult;
    use system_core::{
        ActorIdentity, AuditPayloadPolicy, AuthorityContext, DataScope, ExecutionContext,
        ExecutionMode, Namespace, NoopHttpClient, PlatformMembershipId, PlatformRole,
        PreviewSessionId, RequestId, Revision, SimulationId, SystemModule, TenantId,
        TenantMembershipId, TenantRole, TenantScope,
    };

    use crate::registry::ModuleRegistry;
    use crate::registry::audit_sink::InMemoryAuditSink;
    use crate::repositories::SqliteRepositoryProvider;

    use super::{OrderReadCompatibilityModule, OrderStatusCodec};

    fn pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        {
            let connection = pool.get().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE orders (\
                     id TEXT NOT NULL, orderNo TEXT NOT NULL, startDate TEXT NOT NULL, \
                     endDate TEXT NOT NULL, deliveryDate TEXT NOT NULL DEFAULT '', \
                     pickupMethods TEXT NOT NULL DEFAULT '[]', address TEXT NOT NULL DEFAULT '', \
                     notes TEXT NOT NULL DEFAULT '', deviceSerialNo TEXT NOT NULL DEFAULT '', \
                     totalPrice REAL NOT NULL DEFAULT 0, province TEXT NOT NULL DEFAULT '', \
                     sendWarehouseId TEXT NOT NULL DEFAULT '', returnWarehouseId TEXT NOT NULL DEFAULT '', \
                     createdAt TEXT NOT NULL, accessories TEXT NOT NULL DEFAULT '', \
                     status TEXT NOT NULL, trackingNo TEXT NOT NULL DEFAULT '', \
                     deviceModels TEXT NOT NULL DEFAULT '', tenant_id TEXT NOT NULL, \
                     PRIMARY KEY (tenant_id, id));\
                     CREATE TABLE order_devices (\
                     tenant_id TEXT NOT NULL, orderId TEXT NOT NULL, serialNo TEXT NOT NULL);",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO orders (\
                     id, orderNo, startDate, endDate, deliveryDate, pickupMethods, address, notes, \
                     deviceSerialNo, totalPrice, province, sendWarehouseId, returnWarehouseId, \
                     createdAt, accessories, status, trackingNo, deviceModels, tenant_id) \
                     VALUES ('order-a', '1001', '2026-08-01', '2026-08-10', '2026-08-02', \
                     '[\"delivery\"]', '浦东新区', 'notes', '', 100, '上海', 'send', 'return', \
                     '2026-08-02', '', 'paid', 'SF-A', '', 'tenant-a')",
                    [],
                )
                .unwrap();
            connection
                .execute_batch(
                    "INSERT INTO orders (id, orderNo, startDate, endDate, deliveryDate, pickupMethods, address, notes, deviceSerialNo, totalPrice, province, sendWarehouseId, returnWarehouseId, createdAt, accessories, status, trackingNo, deviceModels, tenant_id) VALUES
                     ('order-b', '1002', '2026-08-01', '2026-08-10', '2026-08-02', '[\"delivery\"]', '浦东新区', '', '', 100, '上海', 'send', 'return', '2026-08-03', '', 'paid', '', '', 'tenant-a');
                     INSERT INTO orders (id, orderNo, startDate, endDate, deliveryDate, pickupMethods, address, notes, deviceSerialNo, totalPrice, province, sendWarehouseId, returnWarehouseId, createdAt, accessories, status, trackingNo, deviceModels, tenant_id) VALUES
                     ('order-c', '1003', '2026-08-01', '2026-08-10', '2026-08-02', '[\"delivery\"]', '浦东新区', '', '', 100, '上海', 'send', 'return', '2026-08-04', '', 'paid', '', '', 'tenant-a');
                     INSERT INTO orders (id, orderNo, startDate, endDate, deliveryDate, pickupMethods, address, notes, deviceSerialNo, totalPrice, province, sendWarehouseId, returnWarehouseId, createdAt, accessories, status, trackingNo, deviceModels, tenant_id) VALUES
                     ('order-d', '1004', '2026-08-01', '2026-08-10', '2026-08-02', '[\"delivery\"]', '浦东新区', '', '', 100, '上海', 'send', 'return', '2026-08-05', '', 'paid', '', '', 'tenant-a');",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO order_devices (tenant_id, orderId, serialNo) VALUES (?1, ?2, ?3)",
                    params!["tenant-a", "order-a", "DEVICE-A"],
                )
                .unwrap();
        }
        pool
    }

    fn normal_context() -> ExecutionContext {
        let tenant = TenantId::new("tenant-a").unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "actor-a",
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new("tenant-membership-a").unwrap(),
                    tenant_id: tenant.clone(),
                    role: TenantRole::Staff,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("revision-a").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new("request-normal").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }
    fn platform_context(mode: ExecutionMode, request: &str) -> ExecutionContext {
        let tenant = TenantId::new("tenant-a").unwrap();
        let data_scope = match &mode {
            ExecutionMode::Simulation(simulation_id) => DataScope::new(
                tenant.clone(),
                Namespace::Simulation(simulation_id.clone()),
                Revision::new("revision-simulation").unwrap(),
            )
            .unwrap(),
            _ => DataScope::production(tenant.clone(), Revision::new("revision-preview").unwrap())
                .unwrap(),
        };
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "platform-owner",
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant),
            data_scope,
            mode,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn registry() -> (ModuleRegistry, Arc<InMemoryAuditSink>) {
        let pool = pool();
        let provider = Arc::new(SqliteRepositoryProvider::new(pool));
        let module: Arc<dyn SystemModule> = Arc::new(OrderReadCompatibilityModule::new(provider));
        let sink = Arc::new(InMemoryAuditSink::default());
        (
            ModuleRegistry::new_with_audit_sink(
                HashMap::from([("order_read_compatibility".into(), module)]),
                sink.clone(),
            )
            .unwrap(),
            sink,
        )
    }

    #[test]
    fn status_codec_accepts_legacy_aliases_and_rejects_unknown_values() {
        assert_eq!(
            OrderStatusCodec::normalize_filter(Some("已预约")).unwrap(),
            Some("draft".into())
        );
        assert_eq!(
            OrderStatusCodec::normalize_filter(Some("active")).unwrap(),
            Some("in_use".into())
        );
        assert_eq!(
            OrderStatusCodec::display_label("repairing").unwrap(),
            "维修中"
        );
        assert!(OrderStatusCodec::normalize_filter(Some("mystery")).is_err());
        assert!(OrderStatusCodec::display_label("mystery").is_err());
    }

    #[test]
    fn compatibility_module_closes_envelope_status_filter_and_not_found_contracts() {
        let (registry, _) = registry();
        let list = registry
            .execute(
                "order_read_compatibility",
                "list_orders",
                serde_json::json!({
                    "page": 1,
                    "pageSize": 20,
                    "filter": {
                        "status": "已付款",
                        "address": "浦东",
                        "serialNo": "DEVICE-A"
                    },
                    "sortBy": "createdAt",
                    "sortOrder": "asc"
                }),
                &normal_context(),
            )
            .unwrap();

        assert_eq!(list["users"].as_array().unwrap().len(), 1);
        assert_eq!(list["users"][0]["status"], "已付款");
        assert_eq!(list["pagination"]["total"], 1);
        assert!(list.get("orders").is_none());

        let error = registry
            .execute(
                "order_read_compatibility",
                "get_order",
                serde_json::json!({"id": "missing"}),
                &normal_context(),
            )
            .unwrap_err();
        assert!(error.contains("BIZ_ORDER_NOT_FOUND"));
    }

    #[test]
    fn offset_command_preserves_exact_non_page_aligned_window_and_legacy_envelope() {
        let (registry, _) = registry();
        let result = registry
            .execute(
                "order_read_compatibility",
                "list_orders_offset",
                serde_json::json!({
                    "limit": 2,
                    "offset": 1,
                    "filter": {},
                    "sortBy": "createdAt",
                    "sortOrder": "desc"
                }),
                &normal_context(),
            )
            .unwrap();

        let ids = result["users"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["order-c", "order-b"]);
        assert_eq!(result["limit"], 2);
        assert_eq!(result["total"], 4);
        assert!(result.get("pagination").is_none());
    }

    #[test]
    fn registry_preserves_preview_simulation_and_attempt_result_audit_semantics() {
        let (registry, sink) = registry();
        registry
            .execute(
                "order_read_compatibility",
                "get_order",
                serde_json::json!({"id": "order-a"}),
                &normal_context(),
            )
            .unwrap();
        registry
            .execute(
                "order_read_compatibility",
                "get_order",
                serde_json::json!({"id": "order-a"}),
                &platform_context(
                    ExecutionMode::ReadOnlyPreview(
                        PreviewSessionId::new("preview-order-read").unwrap(),
                    ),
                    "request-preview",
                ),
            )
            .unwrap();
        let simulation_error = registry
            .execute(
                "order_read_compatibility",
                "get_order",
                serde_json::json!({"id": "order-a"}),
                &platform_context(
                    ExecutionMode::Simulation(SimulationId::new("simulation-order-read").unwrap()),
                    "request-simulation",
                ),
            )
            .unwrap_err();
        assert!(simulation_error.contains("EXEC_SIMULATION_UNSUPPORTED"));

        let events = sink.events().unwrap();
        assert_eq!(events.len(), 6);
        assert_eq!(events[0].result(), &AuditResult::Attempted);
        assert_eq!(events[1].result(), &AuditResult::Succeeded);
        assert_eq!(events[2].result(), &AuditResult::Attempted);
        assert_eq!(events[3].result(), &AuditResult::Succeeded);
        assert_eq!(events[4].result(), &AuditResult::Attempted);
        assert!(matches!(events[5].result(), AuditResult::Failed { .. }));
        assert!(events.iter().all(|event| {
            event.payload_policy() == AuditPayloadPolicy::ReferenceOnly
                && event.command().starts_with("order_read_compatibility.")
        }));
    }
}

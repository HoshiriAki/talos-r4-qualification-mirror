use std::{
    fs::remove_file,
    path::{Path, PathBuf},
    sync::Arc,
};

use official_order::FeatureOrder;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde_json::{Value, json};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    SystemModule, TenantId, TenantScope,
};
use uuid::Uuid;

use crate::application::OrderQueryService;
use crate::repositories::{
    OrderListRequest, OrderReadProjection, OrderSortDirection, OrderSortField, RepositoryProvider,
    SqliteRepositoryProvider,
};

const OPEN_PARITY_GAPS: &[&str] = &[
    "response-envelope",
    "status-normalization",
    "extended-filter-surface",
    "not-found-error-contract",
    "registry-caller-semantics",
];

struct TempDatabase {
    path: PathBuf,
}

impl TempDatabase {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!(
                "talos-sp03d-order-read-parity-{}.sqlite",
                Uuid::new_v4()
            )),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDatabase {
    fn drop(&mut self) {
        let _ = remove_file(&self.path);
    }
}

fn repository_pool(database_path: &Path) -> Pool<SqliteConnectionManager> {
    let pool = Pool::builder()
        .max_size(1)
        .build(SqliteConnectionManager::file(database_path))
        .expect("temporary parity database pool");
    {
        let connection = pool.get().expect("parity connection");
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
            .expect("parity schema");

        insert_order(
            &connection,
            "tenant-a",
            "a-1",
            "1001",
            "2026-08-02",
            "paid",
            "West Depot",
            "alpha note",
        );
        insert_order(
            &connection,
            "tenant-a",
            "a-2",
            "1002",
            "2026-08-03",
            "draft",
            "East Depot",
            "beta note",
        );
        insert_order(
            &connection,
            "tenant-b",
            "b-1",
            "2001",
            "2026-08-04",
            "paid",
            "West Remote",
            "other tenant",
        );

        for (tenant, order_id, serial_no) in [
            ("tenant-a", "a-1", "DEVICE-A"),
            ("tenant-a", "a-2", "DEVICE-B"),
            ("tenant-b", "b-1", "DEVICE-X"),
        ] {
            connection
                .execute(
                    "INSERT INTO order_devices (tenant_id, orderId, serialNo) VALUES (?1, ?2, ?3)",
                    params![tenant, order_id, serial_no],
                )
                .expect("parity device link");
        }
    }
    pool
}

fn insert_order(
    connection: &rusqlite::Connection,
    tenant: &str,
    id: &str,
    order_no: &str,
    created_at: &str,
    status: &str,
    address: &str,
    notes: &str,
) {
    connection
        .execute(
            "INSERT INTO orders (\
             id, orderNo, startDate, endDate, deliveryDate, pickupMethods, address, notes, \
             deviceSerialNo, totalPrice, province, sendWarehouseId, returnWarehouseId, \
             createdAt, accessories, status, trackingNo, deviceModels, tenant_id) \
             VALUES (?1, ?2, '2026-08-01', '2026-08-10', '2026-07-31', \
             '[\"delivery\"]', ?3, ?4, '', 100, 'province', 'send', 'return', ?5, \
             '', ?6, '', '', ?7)",
            params![id, order_no, address, notes, created_at, status, tenant],
        )
        .expect("parity order");
}

fn context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).expect("tenant id");
    ExecutionContext::new(
        ActorIdentity::authenticated("parity-actor", "staff").expect("actor"),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(
            tenant_id,
            Revision::new("parity-revision").expect("revision"),
        )
        .expect("data scope"),
        ExecutionMode::Normal,
        RequestId::new(request).expect("request id"),
        None,
        Arc::new(NoopHttpClient),
    )
    .expect("execution context")
}

fn with_fixture<F>(tenant: &str, request: &str, assertion: F)
where
    F: FnOnce(&FeatureOrder, &OrderQueryService, &ExecutionContext),
{
    let cleanup = TempDatabase::new();
    let database_path = cleanup.path().to_path_buf();
    let repository_pool = repository_pool(&database_path);

    let mut module = FeatureOrder::new();
    module
        .init(json!({
            "databaseUrl": database_path.to_string_lossy().as_ref(),
        }))
        .expect("official Order initialization");

    let provider: Arc<dyn RepositoryProvider> =
        Arc::new(SqliteRepositoryProvider::new(repository_pool.clone()));
    let query_service = OrderQueryService::new(provider);
    let ctx = context(tenant, request);

    assertion(&module, &query_service, &ctx);
}

fn official_list(module: &FeatureOrder, ctx: &ExecutionContext, filter: Value) -> Value {
    module
        .execute(
            "list_orders",
            json!({
                "page": 1,
                "pageSize": 20,
                "filter": filter,
                "sortBy": "createdAt",
                "sortOrder": "asc",
            }),
            ctx,
        )
        .expect("official list")
}

fn repository_request() -> OrderListRequest {
    OrderListRequest {
        page: 1,
        page_size: 20,
        order_no: None,
        status: None,
        keyword: None,
        sort_by: OrderSortField::CreatedAt,
        sort_direction: OrderSortDirection::Asc,
        ..OrderListRequest::default()
    }
}

fn normalize_repository_status(order: &OrderReadProjection) -> Value {
    let mut value = serde_json::to_value(order).expect("repository projection JSON");
    let raw_status = value
        .get("status")
        .and_then(Value::as_str)
        .expect("repository status");
    value["status"] =
        Value::String(official_order::state_machine::display_label(raw_status).to_owned());
    value
}

#[test]
fn projection_fields_match_after_explicit_status_normalization() {
    with_fixture("tenant-a", "parity-projection", |module, service, ctx| {
        let official = official_list(module, ctx, json!({}));
        let repository = service
            .list(ctx, &repository_request())
            .expect("repository list");

        let official_orders = official["users"]
            .as_array()
            .expect("official users envelope");
        let repository_orders: Vec<Value> = repository
            .orders
            .iter()
            .map(normalize_repository_status)
            .collect();

        assert_eq!(official_orders, &repository_orders);
        assert_eq!(official["pagination"]["page"], repository.page);
        assert_eq!(official["pagination"]["pageSize"], repository.page_size);
        assert_eq!(official["pagination"]["total"], repository.total);
        assert_eq!(official["pagination"]["totalPages"], repository.total_pages);
    });
}

#[test]
fn tenant_scope_remains_equivalent_across_both_read_paths() {
    with_fixture("tenant-b", "parity-tenant", |module, service, ctx| {
        let official = official_list(module, ctx, json!({}));
        let repository = service
            .list(ctx, &repository_request())
            .expect("repository list");

        assert_eq!(official["users"].as_array().expect("users").len(), 1);
        assert_eq!(official["users"][0]["id"], "b-1");
        assert_eq!(repository.orders.len(), 1);
        assert_eq!(repository.orders[0].id, "b-1");
    });
}

#[test]
fn status_filter_requires_an_explicit_compatibility_adapter() {
    with_fixture("tenant-a", "parity-status", |module, service, ctx| {
        let official = official_list(module, ctx, json!({"status": "已付款"}));

        let mut localized_request = repository_request();
        localized_request.status = Some("已付款".to_owned());
        let localized_repository = service
            .list(ctx, &localized_request)
            .expect("localized repository list");

        let mut raw_request = repository_request();
        raw_request.status = Some("paid".to_owned());
        let raw_repository = service
            .list(ctx, &raw_request)
            .expect("raw repository list");

        assert_eq!(official["users"].as_array().expect("users").len(), 1);
        assert_eq!(localized_repository.total, 0);
        assert_eq!(raw_repository.total, 1);
        assert_eq!(raw_repository.orders[0].status, "paid");
        assert_eq!(official["users"][0]["status"], "已付款");
    });
}

#[test]
fn extended_filter_surface_blocks_caller_cutover() {
    with_fixture("tenant-a", "parity-filters", |module, service, ctx| {
        let official = official_list(module, ctx, json!({"address": "West"}));
        let repository = service
            .list(ctx, &repository_request())
            .expect("repository list");

        const OFFICIAL_ONLY_FILTERS: &[&str] = &[
            "address",
            "province",
            "startDateFrom",
            "startDateTo",
            "endDateFrom",
            "endDateTo",
            "includedDate",
            "deliveryDateFrom",
            "deliveryDateTo",
            "deliveryDate",
            "pickupMethods",
            "startDate",
            "endDate",
            "serialNo",
            "trackingNo",
        ];

        assert_eq!(official["users"].as_array().expect("users").len(), 1);
        assert_eq!(official["users"][0]["id"], "a-1");
        assert_eq!(repository.total, 2);
        assert_eq!(OFFICIAL_ONLY_FILTERS.len(), 15);
    });
}

#[test]
fn not_found_error_contract_blocks_caller_cutover() {
    with_fixture("tenant-a", "parity-not-found", |module, service, ctx| {
        let official_error = module
            .execute("get_order", json!({"id": "missing"}), ctx)
            .expect_err("official not-found error");
        let official_error: Value =
            serde_json::from_str(&official_error).expect("official error payload");
        let repository = service.get_by_id(ctx, "missing").expect("repository get");

        assert_eq!(official_error["code"], "BIZ_ORDER_NOT_FOUND");
        assert!(repository.is_none());
    });
}

#[test]
fn response_envelope_requires_an_explicit_compatibility_adapter() {
    with_fixture("tenant-a", "parity-envelope", |module, service, ctx| {
        let official = official_list(module, ctx, json!({}));
        let repository = service
            .list(ctx, &repository_request())
            .expect("repository list");
        let repository = serde_json::to_value(repository).expect("repository page JSON");

        assert!(official.get("users").is_some());
        assert!(official.get("pagination").is_some());
        assert!(repository.get("orders").is_some());
        assert!(repository.get("pageSize").is_some());
        assert_ne!(official, repository);
    });
}

#[test]
fn caller_cutover_remains_blocked_until_all_named_gaps_close() {
    assert_eq!(OPEN_PARITY_GAPS.len(), 5);
    assert!(OPEN_PARITY_GAPS.contains(&"response-envelope"));
    assert!(OPEN_PARITY_GAPS.contains(&"status-normalization"));
    assert!(OPEN_PARITY_GAPS.contains(&"extended-filter-surface"));
    assert!(OPEN_PARITY_GAPS.contains(&"not-found-error-contract"));
    assert!(OPEN_PARITY_GAPS.contains(&"registry-caller-semantics"));
}

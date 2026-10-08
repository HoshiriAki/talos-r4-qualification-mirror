use std::collections::HashMap;
use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde_json::Value;
use system_core::{
    AccessRequirement, ActorIdentity, AuthorityContext, CommandMetadata, CommandSchema, DataScope,
    EffectClass, ExecutionContext, ExecutionMode, ModuleMetadata, ModuleSchema, Namespace,
    NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision,
    SimulationId, SimulationSupport, SystemModule, TenantId, TenantMembershipId, TenantRole,
    TenantScope,
};

use crate::application::{CloseTerminalAuthority, CloseTerminalSnapshot, RentalCloseCoordinator};
use crate::domain::{AllocationId, CustomerId, ReservationId};
use crate::registry::ModuleRegistry;
use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};

use super::QuoteModule;

struct FixedPricing;

struct InternalNormalCloseAuthority;

impl CloseTerminalAuthority for InternalNormalCloseAuthority {
    fn terminal_snapshot(&self, _order_id: &str, _ctx: &ExecutionContext) -> CloseTerminalSnapshot {
        CloseTerminalSnapshot {
            deposit_terminal: true,
            shipments_terminal: true,
            damage_repair_terminal: true,
            overdue_terminal: true,
            invoice_terminal: true,
            audit_reconciliation_clear: true,
        }
    }
}

impl SystemModule for FixedPricing {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "pricing".into(),
            version: "test".into(),
            description: "fixed server pricing authority".into(),
            author: "test".into(),
            wasm_compatible: false,
            storage: None,
        }
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![CommandMetadata::new(
            "estimate_pricing",
            AccessRequirement::Authenticated,
            &[EffectClass::DatabaseRead],
            SimulationSupport::Blocked,
        )]
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        _ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        if command != "estimate_pricing" {
            return Err("unknown command".into());
        }
        Ok(serde_json::json!({
            "totalPrice": 12.34,
            "shippingFee": 2.50,
            "modelId": payload.get("modelId").cloned().unwrap_or(Value::Null),
            "dayPrices": [
                {"date": "2026-09-01", "price": 5.00},
                {"date": "2026-09-02", "price": 4.84}
            ]
        }))
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "pricing".into(),
            description: "fixed server pricing authority".into(),
            commands: vec![CommandSchema {
                name: "estimate_pricing".into(),
                description: "test price".into(),
                version: "test".into(),
                input_schema: None,
                output_schema: None,
            }],
        }
    }
}

fn pool() -> Pool<SqliteConnectionManager> {
    let pool = Pool::builder()
        .max_size(1)
        .build(SqliteConnectionManager::memory())
        .unwrap();
    let connection = pool.get().unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE tenants (id TEXT PRIMARY KEY);
             INSERT INTO tenants VALUES ('tenant-a'), ('tenant-b');
             CREATE TABLE device_models (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, name TEXT NOT NULL,
               UNIQUE(id, tenant_id)
             );
             CREATE TABLE devices (
               serialNo TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, modelId TEXT,
               rentalStatus TEXT NOT NULL, UNIQUE(serialNo, tenant_id)
             );
             INSERT INTO device_models VALUES ('model-a', 'tenant-a', 'Model A');
             INSERT INTO devices VALUES
               ('DEV-A', 'tenant-a', 'model-a', 'idle'),
               ('DEV-B', 'tenant-a', 'model-a', 'idle');
             CREATE TABLE inventory_reservations (id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL);
             CREATE TABLE booking_availability (
               id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL, is_available INTEGER NOT NULL
             );
             CREATE TABLE customers (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL,
               legal_name TEXT NOT NULL, display_name TEXT NOT NULL,
               status TEXT NOT NULL, risk_status TEXT NOT NULL,
               version INTEGER NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
               UNIQUE(id, tenant_id)
             );
             CREATE TABLE accessory_catalog (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, sku TEXT NOT NULL, name TEXT NOT NULL,
               unit_price_minor INTEGER NOT NULL, currency TEXT NOT NULL, active INTEGER NOT NULL,
               version INTEGER NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
               UNIQUE(tenant_id, sku), UNIQUE(id, tenant_id)
             );
             CREATE TABLE quotes (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, customer_id TEXT NOT NULL,
               status TEXT NOT NULL, start_date TEXT NOT NULL, end_date TEXT NOT NULL,
               region TEXT NOT NULL, currency TEXT NOT NULL, total_minor INTEGER NOT NULL,
               version INTEGER NOT NULL, expires_at TEXT NOT NULL, confirmed_at TEXT,
               converted_order_id TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
               UNIQUE(id, tenant_id)
             );
             CREATE TABLE quote_lines (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, quote_id TEXT NOT NULL,
               line_kind TEXT NOT NULL, reference_id TEXT NOT NULL, description TEXT NOT NULL,
               quantity INTEGER NOT NULL, unit_price_minor INTEGER NOT NULL,
               subtotal_minor INTEGER NOT NULL, currency TEXT NOT NULL,
               price_snapshot_json TEXT NOT NULL, created_at TEXT NOT NULL
             );
             CREATE TABLE orders (
               id TEXT PRIMARY KEY, orderNo TEXT NOT NULL, startDate TEXT NOT NULL,
               endDate TEXT NOT NULL, deliveryDate TEXT NOT NULL, pickupMethods TEXT NOT NULL,
               address TEXT DEFAULT '', notes TEXT DEFAULT '', region TEXT NOT NULL,
               deviceCount INTEGER NOT NULL, remark TEXT NOT NULL DEFAULT '', tenant_id TEXT NOT NULL,
               customer_id TEXT, source_quote_id TEXT, total_minor INTEGER, currency TEXT,
               status TEXT NOT NULL DEFAULT 'draft', createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL,
               UNIQUE(id, tenant_id)
             );
             CREATE TABLE order_lines (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, order_id TEXT NOT NULL,
               source_quote_line_id TEXT NOT NULL, line_kind TEXT NOT NULL,
               reference_id TEXT NOT NULL, description TEXT NOT NULL, quantity INTEGER NOT NULL,
               unit_price_minor INTEGER NOT NULL, subtotal_minor INTEGER NOT NULL,
               currency TEXT NOT NULL, price_snapshot_json TEXT NOT NULL, created_at TEXT NOT NULL,
               UNIQUE(tenant_id, source_quote_line_id)
             );
             CREATE TABLE order_devices (
               id TEXT PRIMARY KEY, orderId TEXT NOT NULL, serialNo TEXT NOT NULL, tenant_id TEXT NOT NULL
             );
             CREATE TRIGGER trg_quote_lines_immutable_update BEFORE UPDATE ON quote_lines
             WHEN EXISTS (
                 SELECT 1 FROM quotes q WHERE q.id = OLD.quote_id
                   AND q.tenant_id = OLD.tenant_id AND q.status <> 'draft'
             ) BEGIN SELECT RAISE(ABORT, 'confirmed quote lines are immutable'); END;
             CREATE TRIGGER trg_quote_lines_immutable_delete BEFORE DELETE ON quote_lines
             WHEN EXISTS (
                 SELECT 1 FROM quotes q WHERE q.id = OLD.quote_id
                   AND q.tenant_id = OLD.tenant_id AND q.status <> 'draft'
             ) BEGIN SELECT RAISE(ABORT, 'confirmed quote lines are immutable'); END;",
        )
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../db/migrations/055_reservation_allocation_v2.sql"
        ))
        .unwrap();
    connection
        .execute_batch(include_str!("../db/migrations/056_order_lifecycle_v2.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../db/migrations/057_durable_rental_workflow.sql"
        ))
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../db/migrations/058_return_inspection_settlement.sql"
        ))
        .unwrap();
    drop(connection);
    pool
}

fn context(tenant: &str, role: TenantRole, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            format!("actor-{tenant}"),
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(format!("membership-{tenant}")).unwrap(),
                tenant_id: tenant_id.clone(),
                role,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("production-current").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn preview_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform-owner",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("preview-revision").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-quote").unwrap()),
        RequestId::new("request-preview-quote").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn simulation_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    let simulation_id = SimulationId::new("simulation-quote").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform-owner",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::new(
            tenant_id,
            Namespace::Simulation(simulation_id.clone()),
            Revision::new("simulation-revision").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Simulation(simulation_id),
        RequestId::new("request-simulation-quote").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn registry(pool: Pool<SqliteConnectionManager>) -> ModuleRegistry {
    let provider: Arc<dyn RepositoryProvider> = Arc::new(SqliteRepositoryProvider::new(pool));
    let quote: Arc<dyn SystemModule> = Arc::new(QuoteModule::new(provider, Arc::new(FixedPricing)));
    ModuleRegistry::new(HashMap::from([("quote".into(), quote)])).unwrap()
}

fn insert_customer(pool: &Pool<SqliteConnectionManager>, tenant: &str) -> CustomerId {
    let id = CustomerId::new();
    pool.get()
        .unwrap()
        .execute(
            "INSERT INTO customers
             (id, tenant_id, legal_name, display_name, status, risk_status,
              version, created_at, updated_at)
             VALUES (?1, ?2, 'Alice', 'Alice', 'active', 'clear', 1, 'now', 'now')",
            rusqlite::params![id.as_str(), tenant],
        )
        .unwrap();
    id
}

fn seed_accessory(registry: &ModuleRegistry, ctx: &ExecutionContext) {
    registry
        .execute(
            "quote",
            "upsert_accessory",
            serde_json::json!({
                "id": "acc-battery",
                "sku": "BAT-01",
                "name": "Extra Battery",
                "unitPriceMinor": 250,
                "currency": "CNY",
                "active": true
            }),
            ctx,
        )
        .unwrap();
}

fn create_quote(
    registry: &ModuleRegistry,
    ctx: &ExecutionContext,
    customer_id: &CustomerId,
) -> Value {
    registry
        .execute(
            "quote",
            "create_quote",
            serde_json::json!({
                "customerId": customer_id.as_str(),
                "startDate": "2026-09-01",
                "endDate": "2026-09-02",
                "region": "Shanghai",
                "modelLines": [{"modelId": "model-a", "quantity": 2}],
                "accessoryLines": [{"accessoryId": "acc-battery", "quantity": 3}]
            }),
            ctx,
        )
        .unwrap()
}

#[test]
fn quote_uses_server_price_minor_units_and_multiple_lines() {
    let pool = pool();
    let registry = registry(pool.clone());
    let admin = context("tenant-a", TenantRole::Admin, "request-quote-a");
    let customer = insert_customer(&pool, "tenant-a");
    seed_accessory(&registry, &admin);

    let quote = create_quote(&registry, &admin, &customer);
    assert_eq!(quote["status"], "draft");
    let lines = quote["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(quote["total"]["minor"], 3218);
    assert_eq!(quote["total"]["currency"], "CNY");

    let model_line = lines
        .iter()
        .find(|line| {
            line["priceSnapshot"]["authority"].as_str() == Some("pricing.estimate_pricing")
        })
        .expect("model pricing line");
    let accessory_line = lines
        .iter()
        .find(|line| line["priceSnapshot"]["authority"].as_str() == Some("accessory_catalog"))
        .expect("accessory catalog line");

    assert_eq!(model_line["unitPrice"]["minor"], 1234);
    assert_eq!(model_line["priceSnapshot"]["result"]["shippingFee"], 2.5);
    assert_eq!(accessory_line["unitPrice"]["minor"], 250);
}

#[test]
fn create_quote_rejects_client_price_fields() {
    let pool = pool();
    let registry = registry(pool.clone());
    let staff = context("tenant-a", TenantRole::Staff, "request-client-total");
    let customer = insert_customer(&pool, "tenant-a");

    let result = registry.execute(
        "quote",
        "create_quote",
        serde_json::json!({
            "customerId": customer.as_str(),
            "startDate": "2026-09-01",
            "endDate": "2026-09-02",
            "region": "Shanghai",
            "total": {"minor": 1, "currency": "CNY"},
            "modelLines": [{"modelId": "model-a", "quantity": 1}]
        }),
        &staff,
    );
    assert!(result.is_err());
}

#[test]
fn confirmed_quote_snapshot_is_immutable_and_converts_once_without_device_binding() {
    let pool = pool();
    let registry = registry(pool.clone());
    let admin = context("tenant-a", TenantRole::Admin, "request-convert");
    let customer = insert_customer(&pool, "tenant-a");
    seed_accessory(&registry, &admin);
    let quote = create_quote(&registry, &admin, &customer);
    let quote_id = quote["id"].as_str().unwrap();

    let confirmed = registry
        .execute(
            "quote",
            "confirm_quote",
            serde_json::json!({"quoteId": quote_id}),
            &admin,
        )
        .unwrap();
    assert_eq!(confirmed["status"], "confirmed");

    assert!(
        pool.get()
            .unwrap()
            .execute(
                "UPDATE quote_lines SET subtotal_minor = 1 WHERE quote_id = ?1",
                [quote_id],
            )
            .is_err()
    );

    let order = registry
        .execute(
            "quote",
            "create_order_from_quote",
            serde_json::json!({"quoteId": quote_id, "remark": "from confirmed quote"}),
            &admin,
        )
        .unwrap();
    assert!(order["orderNo"].as_str().unwrap().starts_with("ORD-"));
    assert_eq!(order["total"]["minor"], 3218);

    let connection = pool.get().unwrap();
    let line_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM order_lines WHERE order_id = ?1",
            [order["orderId"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(line_count, 2);
    let device_bindings: i64 = connection
        .query_row("SELECT COUNT(*) FROM order_devices", [], |row| row.get(0))
        .unwrap();
    assert_eq!(device_bindings, 0);
    let compatibility: (String, String, String, String) = connection
        .query_row(
            "SELECT deliveryDate, pickupMethods, address, notes FROM orders WHERE id = ?1",
            [order["orderId"].as_str().unwrap()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(compatibility.0, "2026-09-01");
    assert_eq!(compatibility.1, "[]");
    assert_eq!(compatibility.2, "");
    assert_eq!(compatibility.3, "from confirmed quote");
    drop(connection);

    assert!(
        registry
            .execute(
                "quote",
                "create_order_from_quote",
                serde_json::json!({"quoteId": quote_id}),
                &admin,
            )
            .is_err()
    );
}

#[test]
fn internal_normal_e2e_closes_quote_order_rental_with_fixture_authorities() {
    let pool = pool();
    let registry = registry(pool.clone());
    let ctx = context("tenant-a", TenantRole::Admin, "internal-normal-e2e");
    let customer = insert_customer(&pool, "tenant-a");
    seed_accessory(&registry, &ctx);
    let quote = create_quote(&registry, &ctx, &customer);
    let quote_id = quote["id"].as_str().unwrap();
    registry
        .execute(
            "quote",
            "confirm_quote",
            serde_json::json!({"quoteId": quote_id}),
            &ctx,
        )
        .unwrap();
    let order = registry
        .execute(
            "quote",
            "create_order_from_quote",
            serde_json::json!({"quoteId": quote_id, "remark": "Internal Normal E2E"}),
            &ctx,
        )
        .unwrap();
    let order_id = order["orderId"].as_str().unwrap();
    let repositories: Arc<dyn RepositoryProvider> =
        Arc::new(SqliteRepositoryProvider::new(pool.clone()));
    let scoped = repositories.bind(&ctx).unwrap();
    let lifecycle = scoped.lifecycles();

    let mut view = lifecycle
        .apply_action(order_id, "submit_order", 1, "fixture-actor", "submit")
        .unwrap();
    for (action, reason) in [
        ("confirm_order", "confirm"),
        ("mark_awaiting_payment", "billing fixture"),
        ("record_paid", "payment fixture"),
    ] {
        view = lifecycle
            .apply_action(
                order_id,
                action,
                view.lifecycle.version,
                "fixture-actor",
                reason,
            )
            .unwrap();
    }

    let reservation = scoped
        .reservations()
        .create_from_order(ReservationId::new(), order_id, 30)
        .unwrap();
    scoped
        .reservations()
        .confirm(&reservation.id, None)
        .unwrap();
    scoped
        .reservations()
        .allocate_device(&reservation.id, AllocationId::new(), "DEV-A")
        .unwrap();
    scoped
        .reservations()
        .allocate_device(&reservation.id, AllocationId::new(), "DEV-B")
        .unwrap();
    for action in [
        "mark_reserved",
        "mark_allocated",
        "mark_ready_to_ship",
        "mark_shipped",
        "mark_in_use",
        "mark_return_pending",
        "mark_returned",
    ] {
        view = lifecycle
            .apply_action(
                order_id,
                action,
                view.lifecycle.version,
                "fixture-actor",
                "deterministic Internal Normal fixture",
            )
            .unwrap();
    }

    let allocations = scoped
        .reservations()
        .find_by_order(order_id)
        .unwrap()
        .unwrap()
        .allocations;
    for (index, allocation) in allocations.iter().enumerate() {
        scoped
            .rental_closure()
            .receive_allocation(
                order_id,
                allocation.id.as_str(),
                &format!("internal-normal-return-{index}"),
            )
            .unwrap();
        let inspection_id: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT id FROM rental_inspections WHERE tenant_id='tenant-a' AND allocation_id=?1",
                [allocation.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        scoped
            .rental_closure()
            .transition_inspection(&inspection_id, "in_progress", 1)
            .unwrap();
        scoped
            .rental_closure()
            .transition_inspection(&inspection_id, "passed", 2)
            .unwrap();
    }
    view = lifecycle
        .apply_action(
            order_id,
            "mark_inspected",
            view.lifecycle.version,
            "fixture-actor",
            "inspection passed",
        )
        .unwrap();
    scoped
        .rental_closure()
        .calculate_settlement(order_id, "CNY", 3218, true)
        .unwrap();
    scoped
        .rental_closure()
        .mark_fixture_settlement_terminal(order_id)
        .unwrap();
    for action in ["begin_settlement", "settle_order", "mark_completed"] {
        view = lifecycle
            .apply_action(
                order_id,
                action,
                view.lifecycle.version,
                "fixture-actor",
                "deterministic settlement fixture",
            )
            .unwrap();
    }
    drop(scoped);

    let coordinator =
        RentalCloseCoordinator::new(repositories, Arc::new(InternalNormalCloseAuthority));
    let closed = coordinator
        .close_order(order_id, view.lifecycle.version, "fixture-actor", &ctx)
        .unwrap();
    assert_eq!(closed.lifecycle.commercial_status, "closed");
    assert_eq!(closed.lifecycle.fulfilment_status, "inspected");
    assert_eq!(closed.lifecycle.financial_status, "settled");
}

#[test]
fn quote_expiry_is_due_only_and_blocks_accept_or_conversion() {
    let pool = pool();
    let registry = registry(pool.clone());
    let admin = context("tenant-a", TenantRole::Admin, "request-expire");
    let customer = insert_customer(&pool, "tenant-a");
    seed_accessory(&registry, &admin);
    let quote = create_quote(&registry, &admin, &customer);
    let quote_id = quote["id"].as_str().unwrap().to_string();

    assert!(
        registry
            .execute(
                "quote",
                "expire_quote",
                serde_json::json!({"quoteId": quote_id}),
                &admin,
            )
            .is_err()
    );

    pool.get()
        .unwrap()
        .execute(
            "UPDATE quotes SET expires_at = '2000-01-01T00:00:00.000Z' WHERE id = ?1",
            [&quote_id],
        )
        .unwrap();

    let expired = registry
        .execute(
            "quote",
            "expire_quote",
            serde_json::json!({"quoteId": quote_id}),
            &admin,
        )
        .unwrap();
    assert_eq!(expired["status"], "expired");

    assert!(
        registry
            .execute(
                "quote",
                "confirm_quote",
                serde_json::json!({"quoteId": quote_id}),
                &admin,
            )
            .is_err()
    );
    assert!(
        registry
            .execute(
                "quote",
                "create_order_from_quote",
                serde_json::json!({"quoteId": quote_id}),
                &admin,
            )
            .is_err()
    );
}

#[test]
fn quote_is_tenant_scoped_preview_read_only_and_simulation_fail_closed() {
    let pool = pool();
    let registry = registry(pool.clone());
    let admin_a = context("tenant-a", TenantRole::Admin, "request-a");
    let customer = insert_customer(&pool, "tenant-a");
    seed_accessory(&registry, &admin_a);
    let quote = create_quote(&registry, &admin_a, &customer);
    let quote_id = quote["id"].as_str().unwrap();

    let tenant_b = context("tenant-b", TenantRole::Staff, "request-b");
    assert!(
        registry
            .execute(
                "quote",
                "get_quote",
                serde_json::json!({"quoteId": quote_id}),
                &tenant_b,
            )
            .is_err()
    );

    let preview = preview_context("tenant-a");
    assert!(
        registry
            .execute(
                "quote",
                "get_quote",
                serde_json::json!({"quoteId": quote_id}),
                &preview,
            )
            .is_ok()
    );
    assert!(
        registry
            .execute(
                "quote",
                "confirm_quote",
                serde_json::json!({"quoteId": quote_id}),
                &preview,
            )
            .is_err()
    );

    let simulation = simulation_context("tenant-a");
    assert!(
        registry
            .execute(
                "quote",
                "get_quote",
                serde_json::json!({"quoteId": quote_id}),
                &simulation,
            )
            .is_err()
    );
}

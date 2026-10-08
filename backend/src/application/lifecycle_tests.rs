use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
    NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision,
    SimulationId, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use crate::application::{CloseTerminalAuthority, CloseTerminalSnapshot, RentalCloseCoordinator};
use crate::domain::{AllocationId, ReservationId};
use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};

struct FixtureCloseAuthority(CloseTerminalSnapshot);
impl CloseTerminalAuthority for FixtureCloseAuthority {
    fn terminal_snapshot(&self, _order_id: &str, _ctx: &ExecutionContext) -> CloseTerminalSnapshot {
        self.0.clone()
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
             CREATE TABLE device_models (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL,
               name TEXT NOT NULL, UNIQUE(id, tenant_id)
             );
             CREATE TABLE devices (
               serialNo TEXT PRIMARY KEY, tenant_id TEXT NOT NULL,
               modelId TEXT, rentalStatus TEXT NOT NULL,
               UNIQUE(serialNo, tenant_id)
             );
             CREATE TABLE orders (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL,
               startDate TEXT NOT NULL, endDate TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'draft', createdAt TEXT NOT NULL,
               UNIQUE(id, tenant_id)
             );
             CREATE TABLE order_lines (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL,
               order_id TEXT NOT NULL, line_kind TEXT NOT NULL,
               reference_id TEXT NOT NULL, quantity INTEGER NOT NULL
             );
             CREATE TABLE inventory_reservations (
               id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL
             );
             CREATE TABLE booking_availability (
               id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL,
               is_available INTEGER NOT NULL
             );
             CREATE TABLE order_devices (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL
             );
             INSERT INTO tenants VALUES ('tenant-a'), ('tenant-b');
             INSERT INTO device_models VALUES ('model-a', 'tenant-a', 'Model A');
             INSERT INTO devices VALUES ('DEV-A', 'tenant-a', 'model-a', 'idle');
             INSERT INTO orders VALUES
               ('order-ready', 'tenant-a', '2026-12-01', '2026-12-03', 'draft', '2026-08-08T00:00:00Z'),
               ('order-version', 'tenant-a', '2026-12-10', '2026-12-12', 'draft', '2026-08-08T00:00:00Z'),
               ('order-legacy', 'tenant-a', '2026-12-20', '2026-12-21', 'shipped', '2026-08-08T00:00:00Z');
             INSERT INTO order_lines VALUES
               ('line-ready', 'tenant-a', 'order-ready', 'model', 'model-a', 1);",
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
    drop(connection);
    pool
}

fn normal_context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            format!("actor-{tenant}"),
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(format!("membership-{tenant}")).unwrap(),
                tenant_id: tenant_id.clone(),
                role: TenantRole::Admin,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("lifecycle-test").unwrap()).unwrap(),
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
        DataScope::production(tenant_id, Revision::new("lifecycle-preview").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("lifecycle-preview-session").unwrap()),
        RequestId::new("lifecycle-preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn simulation_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    let simulation_id = SimulationId::new("lifecycle-simulation").unwrap();
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
            Revision::new("lifecycle-simulation-base").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Simulation(simulation_id),
        RequestId::new("lifecycle-simulation-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[test]
fn migration_seeds_minimum_implied_dimensions_and_records_ambiguity() {
    let provider = SqliteRepositoryProvider::new(pool());
    let scoped = provider
        .bind(&normal_context("tenant-a", "migration"))
        .unwrap();
    let legacy = scoped.lifecycles().get("order-legacy").unwrap().unwrap();
    assert_eq!(legacy.commercial_status, "confirmed");
    assert_eq!(legacy.financial_status, "paid");
    assert_eq!(legacy.fulfilment_status, "shipped");
    assert_eq!(legacy.version, 1);
    let exceptions = scoped.lifecycles().list_migration_exceptions().unwrap();
    assert_eq!(exceptions.len(), 1);
    assert_eq!(exceptions[0].order_id, "order-legacy");
    assert!(exceptions[0].reason.contains("single-axis"));
}

#[test]
fn optimistic_version_conflict_is_fail_closed_and_history_is_append_only() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider
        .bind(&normal_context("tenant-a", "version"))
        .unwrap();
    let lifecycle = scoped.lifecycles();
    let first = lifecycle
        .apply_action("order-version", "submit_order", 1, "actor", "submit")
        .unwrap();
    assert_eq!(first.lifecycle.version, 2);
    assert_eq!(first.lifecycle.commercial_status, "submitted");
    let conflict = lifecycle
        .apply_action("order-version", "confirm_order", 1, "actor", "stale")
        .unwrap_err();
    assert_eq!(conflict.code(), "REPOSITORY_CONTRACT_VIOLATION");
    assert!(conflict.to_string().contains("version conflict"));
    let before: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM domain_outbox WHERE tenant_id='tenant-a' AND message_type='OrderConfirmed' AND source_id='order-version'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(
        before, 0,
        "failed lifecycle transaction must not leak outbox intent"
    );
    lifecycle
        .apply_action("order-version", "confirm_order", 2, "actor", "confirm")
        .unwrap();
    let after: i64 = pool.get().unwrap().query_row(
        "SELECT COUNT(*) FROM domain_outbox WHERE tenant_id='tenant-a' AND message_type='OrderConfirmed' AND source_id='order-version'",
        [], |row| row.get(0)).unwrap();
    assert_eq!(
        after, 1,
        "successful lifecycle and outbox intent commit together"
    );
    let history = lifecycle.history("order-version").unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].guard_name, "can_submit_order");
    assert_eq!(history[0].resulting_version, 2);
}

#[test]
fn ready_to_ship_named_guard_uses_payment_reservation_allocation_and_risk() {
    let provider = SqliteRepositoryProvider::new(pool());
    let scoped = provider.bind(&normal_context("tenant-a", "ready")).unwrap();
    let lifecycle = scoped.lifecycles();

    let current = lifecycle
        .apply_action("order-ready", "confirm_order", 1, "actor", "confirm")
        .unwrap();
    let current = lifecycle
        .apply_action(
            "order-ready",
            "mark_awaiting_payment",
            current.lifecycle.version,
            "actor",
            "bill",
        )
        .unwrap();
    let current = lifecycle
        .apply_action(
            "order-ready",
            "record_paid",
            current.lifecycle.version,
            "actor",
            "paid",
        )
        .unwrap();

    let reservation = scoped
        .reservations()
        .create_from_order(ReservationId::new(), "order-ready", 30)
        .unwrap();
    scoped
        .reservations()
        .confirm(&reservation.id, None)
        .unwrap();
    scoped
        .reservations()
        .allocate_device(&reservation.id, AllocationId::new(), "DEV-A")
        .unwrap();

    let current = lifecycle
        .apply_action(
            "order-ready",
            "mark_allocated",
            current.lifecycle.version,
            "actor",
            "allocation complete",
        )
        .unwrap();
    assert!(
        current
            .allowed_actions
            .iter()
            .any(|action| action.action == "mark_ready_to_ship")
    );

    let blocked = lifecycle
        .apply_action(
            "order-ready",
            "require_review",
            current.lifecycle.version,
            "actor",
            "manual review",
        )
        .unwrap();
    assert!(blocked.blockers.iter().any(|item| item == "blocking_risk"));
    assert!(
        !blocked
            .allowed_actions
            .iter()
            .any(|action| action.action == "mark_ready_to_ship")
    );
    let error = lifecycle
        .apply_action(
            "order-ready",
            "mark_ready_to_ship",
            blocked.lifecycle.version,
            "actor",
            "must fail",
        )
        .unwrap_err();
    assert!(error.to_string().contains("blocking_risk"));

    let cleared = lifecycle
        .apply_action(
            "order-ready",
            "resolve_risk",
            blocked.lifecycle.version,
            "actor",
            "review complete",
        )
        .unwrap();
    let ready = lifecycle
        .apply_action(
            "order-ready",
            "mark_ready_to_ship",
            cleared.lifecycle.version,
            "actor",
            "all guards clear",
        )
        .unwrap();
    assert_eq!(ready.lifecycle.fulfilment_status, "ready_to_ship");
}

#[test]
fn preview_reads_lifecycle_but_cannot_write_and_simulation_fails_closed() {
    let provider = SqliteRepositoryProvider::new(pool());
    let preview = provider.bind(&preview_context("tenant-a")).unwrap();
    assert!(preview.lifecycles().get("order-version").unwrap().is_some());
    let error = preview
        .lifecycles()
        .apply_action("order-version", "submit_order", 1, "platform", "preview")
        .unwrap_err();
    assert_eq!(error.code(), "REPOSITORY_PREVIEW_WRITE_DENIED");

    assert_eq!(
        provider
            .bind(&simulation_context("tenant-a"))
            .err()
            .unwrap()
            .code(),
        "REPOSITORY_SIMULATION_UNSUPPORTED"
    );
}

#[test]
fn r1p7_close_checks_every_external_terminal_and_uses_named_lifecycle_command() {
    let pool = pool();
    pool.get().unwrap().execute("UPDATE order_lifecycle SET commercial_status='completed',contract_status='not_required',financial_status='settled',fulfilment_status='inspected',risk_status='clear',version=9 WHERE tenant_id='tenant-a' AND order_id='order-ready'",[]).unwrap();
    let all = CloseTerminalSnapshot {
        deposit_terminal: true,
        shipments_terminal: true,
        damage_repair_terminal: true,
        overdue_terminal: true,
        invoice_terminal: true,
        audit_reconciliation_clear: true,
    };
    let cases = [
        "deposit",
        "shipments",
        "damage",
        "overdue",
        "invoice",
        "audit",
    ];
    for missing in cases {
        let mut fixture = all.clone();
        match missing {
            "deposit" => fixture.deposit_terminal = false,
            "shipments" => fixture.shipments_terminal = false,
            "damage" => fixture.damage_repair_terminal = false,
            "overdue" => fixture.overdue_terminal = false,
            "invoice" => fixture.invoice_terminal = false,
            _ => fixture.audit_reconciliation_clear = false,
        };
        let coordinator = RentalCloseCoordinator::new(
            Arc::new(SqliteRepositoryProvider::new(pool.clone())),
            Arc::new(FixtureCloseAuthority(fixture)),
        );
        assert!(
            coordinator
                .close_order(
                    "order-ready",
                    9,
                    "fixture-actor",
                    &normal_context("tenant-a", "negative-close")
                )
                .is_err()
        );
    }
    let coordinator = RentalCloseCoordinator::new(
        Arc::new(SqliteRepositoryProvider::new(pool)),
        Arc::new(FixtureCloseAuthority(all)),
    );
    let closed = coordinator
        .close_order(
            "order-ready",
            9,
            "fixture-actor",
            &normal_context("tenant-a", "positive-close"),
        )
        .unwrap();
    assert_eq!(closed.lifecycle.commercial_status, "closed");
    assert_eq!(closed.lifecycle.version, 10);
}

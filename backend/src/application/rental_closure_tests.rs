use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
    NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision,
    SimulationId, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};

fn pool() -> Pool<SqliteConnectionManager> {
    let pool = Pool::builder()
        .max_size(1)
        .build(SqliteConnectionManager::memory())
        .unwrap();
    pool.get().unwrap().execute_batch("PRAGMA foreign_keys=ON;
      CREATE TABLE rental_reservations(id TEXT NOT NULL,tenant_id TEXT NOT NULL,order_id TEXT,status TEXT,created_at TEXT,PRIMARY KEY(tenant_id,id));
      CREATE TABLE allocations(id TEXT NOT NULL,tenant_id TEXT NOT NULL,reservation_id TEXT NOT NULL,device_serial_no TEXT NOT NULL,status TEXT NOT NULL,PRIMARY KEY(tenant_id,id));").unwrap();
    pool.get()
        .unwrap()
        .execute_batch(include_str!(
            "../db/migrations/057_durable_rental_workflow.sql"
        ))
        .unwrap();
    pool.get()
        .unwrap()
        .execute_batch(include_str!(
            "../db/migrations/058_return_inspection_settlement.sql"
        ))
        .unwrap();
    pool
}

fn context(tenant: &str, mode: ExecutionMode, namespace: Namespace) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    let actor = match mode {
        ExecutionMode::Normal => ActorIdentity::with_authority(
            format!("actor-{tenant}"),
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(format!("member-{tenant}")).unwrap(),
                tenant_id: tenant_id.clone(),
                role: TenantRole::Admin,
            },
        )
        .unwrap(),
        _ => ActorIdentity::with_authority(
            "platform",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-member").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
    };
    ExecutionContext::new(
        actor,
        TenantScope::tenant(tenant_id.clone()),
        DataScope::new(tenant_id, namespace, Revision::new("r1p7-test").unwrap()).unwrap(),
        mode,
        RequestId::new(format!("request-{tenant}")).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}
fn normal(t: &str) -> ExecutionContext {
    context(t, ExecutionMode::Normal, Namespace::Production)
}

fn seed(
    pool: &Pool<SqliteConnectionManager>,
    tenant: &str,
    order: &str,
    allocations: &[(&str, &str)],
) {
    let c = pool.get().unwrap();
    c.execute("INSERT INTO rental_reservations(id,tenant_id,order_id,status,created_at) VALUES (?1,?2,?3,'confirmed','2026-08-19T00:00:00Z')",rusqlite::params![format!("reservation-{order}"),tenant,order]).unwrap();
    for (id, device) in allocations {
        c.execute("INSERT INTO allocations(id,tenant_id,reservation_id,device_serial_no,status) VALUES (?1,?2,?3,?4,'allocated')",rusqlite::params![id,tenant,format!("reservation-{order}"),device]).unwrap();
    }
}

#[test]
fn return_is_partial_persistent_idempotent_and_tenant_scoped() {
    let pool = pool();
    seed(
        &pool,
        "tenant-a",
        "order-1",
        &[("allocation-1", "device-1"), ("allocation-2", "device-2")],
    );
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let partial = scoped
        .rental_closure()
        .receive_allocation("order-1", "allocation-1", "scan-1")
        .unwrap();
    assert_eq!(partial.status, "receiving");
    assert_eq!(partial.item_count, 1);
    let duplicate = scoped
        .rental_closure()
        .receive_allocation("order-1", "allocation-1", "scan-1")
        .unwrap();
    assert_eq!(duplicate.item_count, 1);
    let complete = scoped
        .rental_closure()
        .receive_allocation("order-1", "allocation-2", "scan-2")
        .unwrap();
    assert_eq!(complete.status, "received");
    assert_eq!(complete.required_count, 2);
    drop(scoped);
    let restarted = SqliteRepositoryProvider::new(pool.clone())
        .bind(&normal("tenant-a"))
        .unwrap();
    assert!(
        restarted
            .rental_closure()
            .facts("order-1")
            .unwrap()
            .return_received
    );
    assert!(
        SqliteRepositoryProvider::new(pool)
            .bind(&normal("tenant-b"))
            .unwrap()
            .rental_closure()
            .receive_allocation("order-1", "allocation-1", "foreign")
            .is_err()
    );
}

#[test]
fn inspection_is_per_allocation_fail_closed_and_failure_opens_review() {
    let pool = pool();
    seed(
        &pool,
        "tenant-a",
        "order-2",
        &[("allocation-1", "device-1"), ("allocation-2", "device-2")],
    );
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .rental_closure()
        .receive_allocation("order-2", "allocation-1", "scan-1")
        .unwrap();
    scoped
        .rental_closure()
        .receive_allocation("order-2", "allocation-2", "scan-2")
        .unwrap();
    let ids: Vec<String> = {
        let c = pool.get().unwrap();
        let mut s = c
            .prepare("SELECT id FROM rental_inspections ORDER BY allocation_id")
            .unwrap();
        s.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(
        scoped
            .rental_closure()
            .inspection(&ids[0])
            .unwrap()
            .unwrap()
            .status,
        "pending"
    );
    assert!(
        scoped
            .rental_closure()
            .transition_inspection(&ids[0], "passed", 1)
            .is_err()
    );
    scoped
        .rental_closure()
        .transition_inspection(&ids[0], "in_progress", 1)
        .unwrap();
    scoped
        .rental_closure()
        .transition_inspection(&ids[0], "failed", 2)
        .unwrap();
    scoped
        .rental_closure()
        .transition_inspection(&ids[1], "in_progress", 1)
        .unwrap();
    scoped
        .rental_closure()
        .transition_inspection(&ids[1], "passed", 2)
        .unwrap();
    let facts = scoped.rental_closure().facts("order-2").unwrap();
    assert!(facts.inspection_complete);
    assert_eq!(facts.open_damage_reviews, 1);
    scoped
        .rental_closure()
        .resolve_damage_review(&ids[0], "reviewed without assigning liability or fee")
        .unwrap();
    assert_eq!(
        scoped
            .rental_closure()
            .facts("order-2")
            .unwrap()
            .open_damage_reviews,
        0
    );
}

#[test]
fn settlement_uses_minor_units_is_deterministic_and_missing_authority_blocks() {
    let pool = pool();
    seed(
        &pool,
        "tenant-a",
        "order-3",
        &[("allocation-1", "device-1")],
    );
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    let blocked = scoped
        .rental_closure()
        .calculate_settlement("order-3", "cny", 12345, false)
        .unwrap();
    assert_eq!(blocked.amount_minor, 12345);
    assert_eq!(blocked.status, "blocked");
    assert_eq!(
        blocked.blocker_code.as_deref(),
        Some("FINANCIAL_AUTHORITY_NOT_AVAILABLE")
    );
    let fixture = scoped
        .rental_closure()
        .calculate_settlement("order-3", "CNY", 12345, true)
        .unwrap();
    let repeat = scoped
        .rental_closure()
        .calculate_settlement("order-3", "CNY", 12345, true)
        .unwrap();
    assert_eq!(fixture.facts_hash, repeat.facts_hash);
    scoped
        .rental_closure()
        .mark_fixture_settlement_terminal("order-3")
        .unwrap();
    assert!(
        scoped
            .rental_closure()
            .facts("order-3")
            .unwrap()
            .settlement_terminal
    );
}

#[test]
fn durable_intents_are_atomic_and_idempotent() {
    let pool = pool();
    seed(
        &pool,
        "tenant-a",
        "order-4",
        &[("allocation-1", "device-1")],
    );
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider.bind(&normal("tenant-a")).unwrap();
    scoped
        .workflows()
        .start_from_message("start-4", "order-4", chrono::Utc::now())
        .unwrap();
    scoped
        .rental_closure()
        .receive_allocation("order-4", "allocation-1", "scan-1")
        .unwrap();
    let inspection_id: String = pool
        .get()
        .unwrap()
        .query_row("SELECT id FROM rental_inspections", [], |r| r.get(0))
        .unwrap();
    scoped
        .rental_closure()
        .transition_inspection(&inspection_id, "in_progress", 1)
        .unwrap();
    scoped
        .rental_closure()
        .transition_inspection(&inspection_id, "passed", 2)
        .unwrap();
    assert_eq!(
        scoped
            .workflows()
            .consume_domain_events(chrono::Utc::now(), 100)
            .unwrap(),
        3
    );
    assert_eq!(
        scoped
            .workflows()
            .consume_domain_events(chrono::Utc::now(), 100)
            .unwrap(),
        0
    );
}

#[test]
fn preview_and_simulation_writes_fail_closed() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool);
    let preview = context(
        "tenant-a",
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview").unwrap()),
        Namespace::Production,
    );
    assert!(
        provider
            .bind(&preview)
            .unwrap()
            .rental_closure()
            .receive_allocation("order", "allocation", "scan")
            .is_err()
    );
    let simulation = SimulationId::new("sim-a").unwrap();
    let sim = context(
        "tenant-a",
        ExecutionMode::Simulation(simulation.clone()),
        Namespace::Simulation(simulation),
    );
    assert!(provider.bind(&sim).is_err());
}

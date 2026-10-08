use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
    NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision,
    SimulationId, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use crate::domain::{AllocationId, ReservationId};
use crate::repositories::{DeviceAllocationRequest, RepositoryProvider, SqliteRepositoryProvider};

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
               modelId TEXT, rentalStatus TEXT NOT NULL
             );
             CREATE TABLE orders (
               id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL,
               startDate TEXT NOT NULL, endDate TEXT NOT NULL,
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
             INSERT INTO inventory_reservations VALUES (1, 'tenant-a');
             INSERT INTO booking_availability VALUES (2, 'tenant-a', 0);
             INSERT INTO order_devices VALUES ('legacy-link', 'tenant-a');",
        )
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../db/migrations/055_reservation_allocation_v2.sql"
        ))
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../db/migrations/057_durable_rental_workflow.sql"
        ))
        .unwrap();
    connection
        .execute_batch(
            "INSERT INTO device_models (id, tenant_id, name) VALUES
               ('model-cap', 'tenant-a', 'Capacity'),
               ('model-exp', 'tenant-a', 'Expiry'),
               ('model-alloc', 'tenant-a', 'Allocation');
             INSERT INTO devices (serialNo, tenant_id, modelId, rentalStatus) VALUES
               ('CAP-1', 'tenant-a', 'model-cap', 'idle'),
               ('EXP-1', 'tenant-a', 'model-exp', 'idle'),
               ('ALLOC-1', 'tenant-a', 'model-alloc', 'idle'),
               ('ALLOC-2', 'tenant-a', 'model-alloc', 'idle');
             INSERT INTO orders (id, tenant_id, startDate, endDate) VALUES
               ('order-cap-a', 'tenant-a', '2026-09-01', '2026-09-03'),
               ('order-cap-adj', 'tenant-a', '2026-09-03', '2026-09-04'),
               ('order-cap-overlap', 'tenant-a', '2026-09-02', '2026-09-04'),
               ('order-exp-a', 'tenant-a', '2026-10-01', '2026-10-03'),
               ('order-exp-b', 'tenant-a', '2026-10-02', '2026-10-04'),
               ('order-alloc-a', 'tenant-a', '2026-11-01', '2026-11-03'),
               ('order-alloc-b', 'tenant-a', '2026-11-01', '2026-11-03');
             INSERT INTO order_lines (id, tenant_id, order_id, line_kind, reference_id, quantity) VALUES
               ('line-cap-a', 'tenant-a', 'order-cap-a', 'model', 'model-cap', 1),
               ('line-cap-adj', 'tenant-a', 'order-cap-adj', 'model', 'model-cap', 1),
               ('line-cap-overlap', 'tenant-a', 'order-cap-overlap', 'model', 'model-cap', 1),
               ('line-exp-a', 'tenant-a', 'order-exp-a', 'model', 'model-exp', 1),
               ('line-exp-b', 'tenant-a', 'order-exp-b', 'model', 'model-exp', 1),
               ('line-alloc-a', 'tenant-a', 'order-alloc-a', 'model', 'model-alloc', 1),
               ('line-alloc-b', 'tenant-a', 'order-alloc-b', 'model', 'model-alloc', 1);",
        )
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
        DataScope::production(tenant_id, Revision::new("reservation-v2-test").unwrap()).unwrap(),
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
        DataScope::production(tenant_id, Revision::new("reservation-preview").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(
            PreviewSessionId::new("reservation-preview-session").unwrap(),
        ),
        RequestId::new("reservation-preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn simulation_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    let simulation_id = SimulationId::new("reservation-simulation").unwrap();
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
            Revision::new("reservation-simulation-base").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Simulation(simulation_id),
        RequestId::new("reservation-simulation-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[test]
fn half_open_model_capacity_allows_adjacency_and_rejects_overlap() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool);
    let scoped = provider
        .bind(&normal_context("tenant-a", "capacity"))
        .unwrap();
    let repository = scoped.reservations();

    repository
        .create_from_order(ReservationId::new(), "order-cap-a", 30)
        .unwrap();
    repository
        .create_from_order(ReservationId::new(), "order-cap-adj", 30)
        .expect("[09-01,09-03) and [09-03,09-04) must not overlap");
    let error = repository
        .create_from_order(ReservationId::new(), "order-cap-overlap", 30)
        .unwrap_err();
    assert_eq!(error.code(), "REPOSITORY_CONTRACT_VIOLATION");
    assert!(error.to_string().contains("insufficient model capacity"));
}

#[test]
fn expired_hold_releases_capacity_for_overlapping_demand() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider
        .bind(&normal_context("tenant-a", "expiry"))
        .unwrap();
    let repository = scoped.reservations();
    let first = repository
        .create_from_order(ReservationId::new(), "order-exp-a", 30)
        .unwrap();
    assert!(
        repository
            .create_from_order(ReservationId::new(), "order-exp-b", 30)
            .is_err()
    );

    pool.get()
        .unwrap()
        .execute(
            "UPDATE rental_reservations SET expires_at = '2000-01-01T00:00:00Z'
             WHERE id = ?1",
            rusqlite::params![first.id.as_str()],
        )
        .unwrap();
    assert_eq!(repository.expire_due().unwrap(), 1);
    repository
        .create_from_order(ReservationId::new(), "order-exp-b", 30)
        .expect("expired hold must stop consuming capacity");
}

#[test]
fn confirmed_reservations_cannot_allocate_the_same_device_for_overlapping_windows() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool);
    let scoped = provider
        .bind(&normal_context("tenant-a", "allocation"))
        .unwrap();
    let repository = scoped.reservations();
    let first = repository
        .create_from_order(ReservationId::new(), "order-alloc-a", 30)
        .unwrap();
    let second = repository
        .create_from_order(ReservationId::new(), "order-alloc-b", 30)
        .unwrap();
    repository.confirm(&first.id, None).unwrap();
    repository.confirm(&second.id, None).unwrap();

    repository
        .allocate_device(&first.id, AllocationId::new(), "ALLOC-1")
        .unwrap();
    let conflict = repository
        .allocate_device(&second.id, AllocationId::new(), "ALLOC-1")
        .unwrap_err();
    assert_eq!(conflict.code(), "REPOSITORY_CONTRACT_VIOLATION");
    assert!(
        conflict
            .to_string()
            .contains("overlapping half-open interval")
    );
    repository
        .allocate_device(&second.id, AllocationId::new(), "ALLOC-2")
        .unwrap();
}

#[test]
fn device_allocation_batch_allows_multiple_devices_for_one_reservation() {
    let pool = pool();
    pool.get()
        .unwrap()
        .execute(
            "UPDATE order_lines SET quantity = 2
             WHERE tenant_id = 'tenant-a' AND order_id = 'order-alloc-a'",
            [],
        )
        .unwrap();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider
        .bind(&normal_context("tenant-a", "allocation-batch-positive"))
        .unwrap();
    let repository = scoped.reservations();
    let reservation = repository
        .create_from_order(ReservationId::new(), "order-alloc-a", 30)
        .unwrap();
    assert_eq!(reservation.requirements.len(), 1);
    assert_eq!(reservation.requirements[0].quantity, 2);
    repository.confirm(&reservation.id, None).unwrap();

    let allocations = repository
        .allocate_devices_batch(&[
            DeviceAllocationRequest {
                order_id: "order-alloc-a".into(),
                device_serial_no: "ALLOC-1".into(),
            },
            DeviceAllocationRequest {
                order_id: "order-alloc-a".into(),
                device_serial_no: "ALLOC-2".into(),
            },
        ])
        .unwrap();
    assert_eq!(allocations.len(), 2);
    assert!(
        allocations
            .iter()
            .all(|allocation| allocation.reservation_id == reservation.id)
    );

    let (count, distinct_reservations, database_reservation_id): (i64, i64, String) = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT COUNT(*), COUNT(DISTINCT reservation_id), MIN(reservation_id)
             FROM allocations
             WHERE tenant_id = 'tenant-a' AND order_id = 'order-alloc-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(distinct_reservations, 1);
    assert_eq!(database_reservation_id, reservation.id.as_str());
}

#[test]
fn device_allocation_batch_rolls_back_every_row_when_a_later_row_fails() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool.clone());
    let scoped = provider
        .bind(&normal_context("tenant-a", "allocation-batch-rollback"))
        .unwrap();
    let repository = scoped.reservations();
    let first = repository
        .create_from_order(ReservationId::new(), "order-alloc-a", 30)
        .unwrap();
    let second = repository
        .create_from_order(ReservationId::new(), "order-alloc-b", 30)
        .unwrap();
    repository.confirm(&first.id, None).unwrap();
    repository.confirm(&second.id, None).unwrap();

    let error = repository
        .allocate_devices_batch(&[
            DeviceAllocationRequest {
                order_id: "order-alloc-a".into(),
                device_serial_no: "ALLOC-1".into(),
            },
            DeviceAllocationRequest {
                order_id: "order-alloc-b".into(),
                device_serial_no: "MISSING-DEVICE".into(),
            },
        ])
        .unwrap_err();
    assert_eq!(error.code(), "REPOSITORY_CONTRACT_VIOLATION");
    assert!(error.to_string().contains("device not found"));

    let allocations: i64 = pool
        .get()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM allocations WHERE tenant_id = 'tenant-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        allocations, 0,
        "a rejected batch must leave no canonical allocations"
    );
}

#[test]
fn migration_exceptions_are_explicit_and_tenant_scoped() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool);
    let tenant_a = provider
        .bind(&normal_context("tenant-a", "migration-a"))
        .unwrap();
    let tenant_b = provider
        .bind(&normal_context("tenant-b", "migration-b"))
        .unwrap();
    let exceptions = tenant_a.reservations().list_migration_exceptions().unwrap();
    assert_eq!(exceptions.len(), 3);
    assert_eq!(
        exceptions
            .iter()
            .map(|item| item.source_table.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "booking_availability",
            "inventory_reservations",
            "order_devices",
        ])
    );
    assert!(
        tenant_b
            .reservations()
            .list_migration_exceptions()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn preview_can_read_but_cannot_reserve_and_simulation_fails_closed() {
    let pool = pool();
    let provider = SqliteRepositoryProvider::new(pool);
    let normal = provider
        .bind(&normal_context("tenant-a", "normal"))
        .unwrap();
    let reservation = normal
        .reservations()
        .create_from_order(ReservationId::new(), "order-cap-a", 30)
        .unwrap();

    let preview = provider.bind(&preview_context("tenant-a")).unwrap();
    assert_eq!(
        preview
            .reservations()
            .get(&reservation.id)
            .unwrap()
            .unwrap()
            .id,
        reservation.id
    );
    assert_eq!(
        preview
            .reservations()
            .create_from_order(ReservationId::new(), "order-cap-adj", 30)
            .unwrap_err()
            .code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );

    assert_eq!(
        provider
            .bind(&simulation_context("tenant-a"))
            .err()
            .unwrap()
            .code(),
        "REPOSITORY_SIMULATION_UNSUPPORTED"
    );
}

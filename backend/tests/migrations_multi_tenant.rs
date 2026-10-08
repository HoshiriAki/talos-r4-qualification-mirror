#![cfg(feature = "sqlite")]
#![allow(dead_code)]

mod utils {
    pub mod time {
        include!("../src/utils/time.rs");
    }
}

#[path = "../src/db/migrations.rs"]
mod migrations;

use rusqlite::Connection;
use std::fs;

#[test]
fn applies_all_migrations_to_a_clean_database_only_once() {
    let db_path =
        std::env::temp_dir().join(format!("talos-migrations-{}.sqlite", uuid::Uuid::new_v4()));
    let conn = Connection::open(&db_path).expect("open clean temporary SQLite database");

    let first_run = migrations::run_migrations(&conn).expect("all registered migrations apply");
    assert!(
        first_run
            .iter()
            .any(|id| id == "043_add_performance_indexes")
    );

    let second_run = migrations::run_migrations(&conn).expect("migrations are repeatable");
    assert!(
        second_run.is_empty(),
        "second run must have no pending migrations"
    );

    drop(conn);
    fs::remove_file(db_path).expect("remove temporary SQLite database");
}

#[test]
fn tenant_guards_reject_null_tenant_updates_and_role_downgrades() {
    let db_path = std::env::temp_dir().join(format!(
        "talos-tenant-guards-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    let conn = Connection::open(&db_path).expect("open clean temporary SQLite database");
    migrations::run_migrations(&conn).expect("apply migrations");

    conn.execute_batch(
        "
        INSERT INTO orders (id, orderNo, startDate, endDate, deliveryDate, pickupMethods, createdAt, tenant_id)
        VALUES ('order-1', 'ORDER-1', '2026-07-15', '2026-07-16', '2026-07-15', 'pickup', '2026-07-15T00:00:00+08:00', 'default');
        INSERT INTO devices (id, serialNo, rentalStatus, createdAt, tenant_id)
        VALUES ('device-1', 'DEVICE-1', 'available', '2026-07-15T00:00:00+08:00', 'default');
        INSERT INTO warehouses (id, name, type, createdAt, updatedAt, tenant_id)
        VALUES ('warehouse-1', 'Warehouse 1', 'main', '2026-07-15T00:00:00+08:00', '2026-07-15T00:00:00+08:00', 'default');
        INSERT INTO identities
            (id, username, password_hash, display_name, status, created_at, updated_at)
        VALUES
            ('identity-staff', 'staff-1', 'hash', 'Staff 1', 'active',
             '2026-07-15T00:00:00+08:00', '2026-07-15T00:00:00+08:00'),
            ('identity-platform', 'platform-1', 'hash', 'Platform 1', 'active',
             '2026-07-15T00:00:00+08:00', '2026-07-15T00:00:00+08:00');
        INSERT INTO tenant_memberships
            (id, identity_id, tenant_id, role, status, created_at, updated_at)
        VALUES
            ('membership-staff', 'identity-staff', 'default', 'staff', 'active',
             '2026-07-15T00:00:00+08:00', '2026-07-15T00:00:00+08:00');
        INSERT INTO platform_memberships
            (id, identity_id, status, created_at, updated_at)
        VALUES
            ('membership-platform', 'identity-platform', 'active',
             '2026-07-15T00:00:00+08:00', '2026-07-15T00:00:00+08:00');
        INSERT INTO platform_role_grants
            (id, platform_membership_id, role, granted_by_identity_id, granted_at)
        VALUES
            ('grant-owner', 'membership-platform', 'platform_owner',
             'identity-platform', '2026-07-15T00:00:00+08:00');
        ",
    )
    .expect("seed tenant-scoped records");

    for statement in [
        "UPDATE orders SET tenant_id = NULL WHERE id = 'order-1'",
        "UPDATE devices SET tenant_id = NULL WHERE id = 'device-1'",
        "UPDATE warehouses SET tenant_id = NULL WHERE id = 'warehouse-1'",
        "UPDATE tenant_memberships SET tenant_id = NULL WHERE id = 'membership-staff'",
        "UPDATE tenant_memberships SET role = 'platform_owner' WHERE id = 'membership-staff'",
        "UPDATE platform_role_grants SET role = 'staff' WHERE id = 'grant-owner'",
    ] {
        assert!(
            conn.execute(statement, []).is_err(),
            "{statement} must be rejected"
        );
    }

    drop(conn);
    fs::remove_file(db_path).expect("remove temporary SQLite database");
}

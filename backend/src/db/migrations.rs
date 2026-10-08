#[cfg(feature = "sqlite")]
use rusqlite::Connection;

#[cfg(feature = "sqlite")]
use crate::utils::time::shanghai_now_iso;

#[cfg(feature = "sqlite")]
struct Migration {
    id: &'static str,
    description: &'static str,
    sql: &'static str,
    conditional: bool,
}

#[cfg(feature = "sqlite")]
const MIGRATIONS: &[Migration] = &[
    Migration {
        id: "001_create_core_tables",
        description: "Create base business tables and audit indexes",
        sql: include_str!("migrations/001_create_core_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "002_add_devices_notes_column",
        description: "Ensure devices.notes column exists",
        sql: include_str!("migrations/002_add_devices_notes_column.sql"),
        conditional: true,
    },
    Migration {
        id: "003_add_devices_fallback_return_node_column",
        description: "Ensure devices.fallbackReturnNode column exists",
        sql: include_str!("migrations/003_add_devices_fallback_return_node_column.sql"),
        conditional: true,
    },
    Migration {
        id: "004_create_pricing_tables",
        description: "Create pricing config tables for shared pricing source",
        sql: include_str!("migrations/004_create_pricing_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "005_create_model_warehouse_tables",
        description: "Create device_models, model_base_prices, warehouses, warehouse_region_rules, order_price_details tables; extend devices and orders with new columns",
        sql: include_str!("migrations/005_create_model_warehouse_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "006_add_pricing_receive_shipping_fees_column",
        description: "Add configurable receive-area shipping fees to pricing config",
        sql: include_str!("migrations/006_add_receive_shipping_fees.sql"),
        conditional: true,
    },
    Migration {
        id: "007_identity_last_login",
        description: "Identity last-login field is part of the clean baseline",
        sql: include_str!("migrations/007_identity_last_login.sql"),
        conditional: true,
    },
    Migration {
        id: "008_add_orders_accessories",
        description: "Add accessories JSON column to orders",
        sql: include_str!("migrations/008_add_orders_accessories.sql"),
        conditional: true,
    },
    Migration {
        id: "009_add_orders_status",
        description: "Add status column to orders",
        sql: include_str!("migrations/009_add_orders_status.sql"),
        conditional: true,
    },
    Migration {
        id: "010_add_orders_tracking_no",
        description: "Add trackingNo column to orders for courier tracking numbers",
        sql: include_str!("migrations/010_add_orders_tracking_no.sql"),
        conditional: true,
    },
    Migration {
        id: "011_add_orders_device_models",
        description: "Add deviceModels JSON column to orders for storing selected model counts",
        sql: include_str!("migrations/011_add_orders_device_models.sql"),
        conditional: true,
    },
    Migration {
        id: "012_create_user_settings",
        description: "Create user_settings table for per-account UI preferences",
        sql: include_str!("migrations/012_create_user_settings.sql"),
        conditional: false,
    },
    Migration {
        id: "013_enhance_warehouses",
        description: "Add address, contactName, contactPhone, notes, capacity to warehouses",
        sql: include_str!("migrations/013_enhance_warehouses.sql"),
        conditional: true,
    },
    Migration {
        id: "014_identity_profile_fields",
        description: "Identity profile fields are part of the clean baseline",
        sql: include_str!("migrations/014_identity_profile_fields.sql"),
        conditional: true,
    },
    Migration {
        id: "016_add_indexes_and_constraints",
        description: "Add performance indexes on order_devices, orders, devices; ensure schema_migrations table",
        sql: include_str!("migrations/016_add_indexes_and_constraints.sql"),
        conditional: false,
    },
    Migration {
        id: "017_add_lost_device_statuses",
        description: "Add warning_status column to devices; extend rentalStatus with lost/scrapped states",
        sql: include_str!("migrations/017_add_lost_device_statuses.sql"),
        conditional: true,
    },
    Migration {
        id: "018_create_api_keys",
        description: "Create api_keys table for API key authentication",
        sql: include_str!("migrations/018_create_api_keys.sql"),
        conditional: false,
    },
    Migration {
        id: "021_order_status_enum",
        description: "Expand order status to 9-state lifecycle + status_history audit table",
        sql: include_str!("migrations/021_order_status_enum.sql"),
        conditional: false,
    },
    Migration {
        id: "022_create_deposit_tables",
        description: "Create deposits/refunds/deposit_ledger tables for deposit & refund workflow",
        sql: include_str!("migrations/022_create_deposit_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "023_create_damage_repair_tables",
        description: "Create damage_reports + repair_orders tables for device damage & repair tracking",
        sql: include_str!("migrations/023_create_damage_repair_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "024_create_notification_tables",
        description: "Create notification_templates + notification_log tables & seed defaults",
        sql: include_str!("migrations/024_create_notification_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "025_create_finance_tax_tables",
        description: "Create invoices + settlements + revenue_records + tax_config + accounting_entries tables",
        sql: include_str!("migrations/025_create_finance_tax_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "026_create_asset_lifecycle_tables",
        description: "Create asset_purchases + depreciation_log tables for device lifecycle management",
        sql: include_str!("migrations/026_create_asset_lifecycle_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "027_create_compliance_tables",
        description: "Create privacy_consents and data_deletion_requests tables",
        sql: include_str!("migrations/027_create_compliance_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "028_credit",
        description: "Create blacklist + violations + credit_scores tables with indexes",
        sql: include_str!("migrations/028_credit.sql"),
        conditional: false,
    },
    Migration {
        id: "029_overdue",
        description: "Create overdue_fee_config + overdue_records + overdue_notification_log tables",
        sql: include_str!("migrations/029_overdue.sql"),
        conditional: false,
    },
    Migration {
        id: "030_contract",
        description: "Create contract_templates + contracts + e_signatures tables for rental contract system",
        sql: include_str!("migrations/030_contract.sql"),
        conditional: false,
    },
    Migration {
        id: "031_optical_sop",
        description: "Create inspection_checklists table for 5-step optical SOP inspections",
        sql: include_str!("migrations/031_optical_sop.sql"),
        conditional: false,
    },
    Migration {
        id: "032_reservation",
        description: "Create inventory_reservations + reservation_rules tables for multi-warehouse inventory reservation",
        sql: include_str!("migrations/032_reservation.sql"),
        conditional: false,
    },
    Migration {
        id: "035_create_work_tasks",
        description: "Create work_tasks table for HUD task projection and PC task queue",
        sql: include_str!("migrations/035_create_work_tasks.sql"),
        conditional: false,
    },
    Migration {
        id: "036_fix_work_task_shanghai_defaults",
        description: "Rebuild work_tasks with Asia/Shanghai timestamp defaults",
        sql: include_str!("migrations/036_fix_work_task_shanghai_defaults.sql"),
        conditional: false,
    },
    Migration {
        id: "034_create_barcode_tables",
        description: "Create barcode_labels + scan_events tables for barcode/RFID scanning",
        sql: include_str!("migrations/034_barcode.sql"),
        conditional: false,
    },
    Migration {
        id: "033_booking",
        description: "Create booking_availability table for online device booking system",
        sql: include_str!("migrations/033_booking.sql"),
        conditional: false,
    },
    Migration {
        id: "037_create_tenants_table",
        description: "Create tenants table for multi-tenancy support",
        sql: include_str!("migrations/037_create_tenants_table.sql"),
        conditional: false,
    },
    Migration {
        id: "038_add_tenant_id_to_core_tables",
        description: "Add tenant_id column to core business tables",
        sql: include_str!("migrations/038_add_tenant_id_to_core_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "039_add_tenant_indexes",
        description: "Add composite indexes on (tenant_id, id) for query performance",
        sql: include_str!("migrations/039_add_tenant_indexes.sql"),
        conditional: false,
    },
    Migration {
        id: "040_add_tenant_to_remaining_tables",
        description: "Add tenant_id column to remaining business tables (40+ tables)",
        sql: include_str!("migrations/040_add_tenant_to_remaining_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "041_enforce_tenant_id_not_null",
        description: "Enforce tenant_id NOT NULL constraints on all tables",
        sql: include_str!("migrations/041_enforce_tenant_id_not_null.sql"),
        conditional: false,
    },
    Migration {
        id: "042_add_tenant_foreign_keys",
        description: "Add foreign key constraints for tenant_id via triggers",
        sql: include_str!("migrations/042_add_tenant_foreign_keys.sql"),
        conditional: false,
    },
    Migration {
        id: "043_add_performance_indexes",
        description: "Add performance indexes for multi-tenant queries",
        sql: include_str!("migrations/043_add_performance_indexes.sql"),
        conditional: false,
    },
    Migration {
        id: "044_add_tax_config_tenant_scope",
        description: "Add and index tenant scope for tax configuration",
        sql: include_str!("migrations/044_add_tax_config_tenant_scope.sql"),
        conditional: false,
    },
    Migration {
        id: "045_complete_admin_tenant_scope",
        description: "Complete tenant scope for credit, overdue notification, and order price detail tables",
        sql: include_str!("migrations/045_complete_admin_tenant_scope.sql"),
        conditional: false,
    },
    Migration {
        id: "046_scope_pricing_tables",
        description: "Scope pricing configuration, model prices, and dynamic prices by tenant",
        sql: include_str!("migrations/046_scope_pricing_tables.sql"),
        conditional: false,
    },
    Migration {
        id: "047_scope_reservation_rules",
        description: "Scope reservation policies and records by tenant",
        sql: include_str!("migrations/047_scope_reservation_rules.sql"),
        conditional: false,
    },
    Migration {
        id: "048_tenant_governance",
        description: "Create append-only change intents and governance audit indexes",
        sql: include_str!("migrations/048_tenant_governance.sql"),
        conditional: false,
    },
    Migration {
        id: "049_tenant_preview",
        description: "Create actor-bound read-only tenant preview sessions",
        sql: include_str!("migrations/049_tenant_preview.sql"),
        conditional: false,
    },
    Migration {
        id: "050_tenant_simulation",
        description: "Create isolated tenant simulation sessions, overlays, and evidence",
        sql: include_str!("migrations/050_tenant_simulation.sql"),
        conditional: false,
    },
    Migration {
        id: "051_tenant_simulation_pricing",
        description: "Add canonical pricing revision source for simulation coverage",
        sql: include_str!("migrations/051_tenant_simulation_pricing.sql"),
        conditional: false,
    },
    Migration {
        id: "052_identity_authority_foundation",
        description: "Replace legacy authentication storage with identity and authority tables",
        sql: include_str!("migrations/052_identity_authority_foundation.sql"),
        conditional: false,
    },
    Migration {
        id: "053_customer_domain",
        description: "Create tenant-scoped Customer domain, typed customer associations, and migration exceptions",
        sql: include_str!("migrations/053_customer_domain.sql"),
        conditional: false,
    },
    Migration {
        id: "054_quote_pricing_orderline",
        description: "Create Quote, immutable pricing snapshots, accessory catalog, and OrderLine associations",
        sql: include_str!("migrations/054_quote_pricing_orderline.sql"),
        conditional: false,
    },
    Migration {
        id: "055_reservation_allocation_v2",
        description: "Create canonical Reservation and Allocation V2 authority",
        sql: include_str!("migrations/055_reservation_allocation_v2.sql"),
        conditional: false,
    },
    Migration {
        id: "056_order_lifecycle_v2",
        description: "Create canonical multi-dimensional Order Lifecycle V2 authority",
        sql: include_str!("migrations/056_order_lifecycle_v2.sql"),
        conditional: false,
    },
    Migration {
        id: "057_durable_rental_workflow",
        description: "Create durable rental workflow, blocker, manual task, outbox, and inbox authority",
        sql: include_str!("migrations/057_durable_rental_workflow.sql"),
        conditional: false,
    },
    Migration {
        id: "058_return_inspection_settlement",
        description: "Create tenant-scoped Return, Inspection, damage review, and settlement skeleton",
        sql: include_str!("migrations/058_return_inspection_settlement.sql"),
        conditional: false,
    },
    Migration {
        id: "059_integration_fabric",
        description: "Create tenant-scoped Provider, external operation, webhook, deposit, and refund authority",
        sql: include_str!("migrations/059_integration_fabric.sql"),
        conditional: false,
    },
    Migration {
        id: "060_integration_runtime",
        description: "Add durable Integration Fabric runtime and reconciliation evidence",
        sql: include_str!("migrations/060_integration_runtime.sql"),
        conditional: false,
    },
    Migration {
        id: "061_integration_scheduler_cursor",
        description: "Add persisted fair-pagination cursor for Integration worker scheduling",
        sql: include_str!("migrations/061_integration_scheduler_cursor.sql"),
        conditional: false,
    },
    Migration {
        id: "062_integration_startup_recovery_snapshot",
        description: "Add bounded startup recovery snapshot for Integration worker",
        sql: include_str!("migrations/062_integration_startup_recovery_snapshot.sql"),
        conditional: false,
    },
    Migration {
        id: "063_r3_damage_settlement_clock",
        description: "Create R3 damage, repair, settlement, dispute, and business-time authority",
        sql: include_str!("migrations/063_r3_damage_settlement_clock.sql"),
        conditional: false,
    },
    Migration {
        id: "064_r3_settlement_integration_authority",
        description: "Make R2 Integration Fabric authoritative for R3 settlement external effects",
        sql: include_str!("migrations/064_r3_settlement_integration_authority.sql"),
        conditional: false,
    },
    Migration {
        id: "065_r3_terminal_settlement_seal",
        description: "Seal terminal R3 settlement facts until an approved amend or reopen authority exists",
        sql: include_str!("migrations/065_r3_terminal_settlement_seal.sql"),
        conditional: false,
    },
    Migration {
        id: "066_r3_machine_api",
        description: "Project machine identity and tenant-bound API governance",
        sql: include_str!("migrations/066_r3_machine_api.sql"),
        conditional: false,
    },
];

#[cfg(feature = "sqlite")]
fn ensure_migrations_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            id TEXT PRIMARY KEY,
            description TEXT NOT NULL,
            appliedAt TEXT NOT NULL
        )",
    )
}

#[cfg(feature = "sqlite")]
fn get_applied_ids(conn: &Connection) -> rusqlite::Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM schema_migrations")?;
    let ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(ids.into_iter().collect())
}

#[cfg(feature = "sqlite")]
fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info({})", table);
    conn.prepare(&sql)
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map([], |row| row.get::<_, String>(1))
                .ok()
                .map(|rows| rows.filter_map(|r| r.ok()).any(|name| name == column))
        })
        .unwrap_or(false)
}

#[cfg(feature = "sqlite")]
fn apply_conditional_columns(conn: &Connection) -> rusqlite::Result<()> {
    if !column_exists(conn, "devices", "modelId") {
        conn.execute("ALTER TABLE devices ADD COLUMN modelId TEXT DEFAULT ''", [])?;
    }
    if !column_exists(conn, "devices", "currentWarehouseId") {
        conn.execute(
            "ALTER TABLE devices ADD COLUMN currentWarehouseId TEXT DEFAULT ''",
            [],
        )?;
    }
    if !column_exists(conn, "devices", "expectedWarehouseId") {
        conn.execute(
            "ALTER TABLE devices ADD COLUMN expectedWarehouseId TEXT DEFAULT ''",
            [],
        )?;
    }
    if !column_exists(conn, "devices", "expectedAvailableDate") {
        conn.execute(
            "ALTER TABLE devices ADD COLUMN expectedAvailableDate TEXT DEFAULT ''",
            [],
        )?;
    }
    if !column_exists(conn, "orders", "totalPrice") {
        conn.execute(
            "ALTER TABLE orders ADD COLUMN totalPrice REAL DEFAULT 0",
            [],
        )?;
    }
    if !column_exists(conn, "orders", "province") {
        conn.execute("ALTER TABLE orders ADD COLUMN province TEXT DEFAULT ''", [])?;
    }
    if !column_exists(conn, "orders", "sendWarehouseId") {
        conn.execute(
            "ALTER TABLE orders ADD COLUMN sendWarehouseId TEXT DEFAULT ''",
            [],
        )?;
    }
    if !column_exists(conn, "orders", "returnWarehouseId") {
        conn.execute(
            "ALTER TABLE orders ADD COLUMN returnWarehouseId TEXT DEFAULT ''",
            [],
        )?;
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
pub fn run_migrations(conn: &Connection) -> anyhow::Result<Vec<String>> {
    ensure_migrations_table(conn)?;
    let applied_ids = get_applied_ids(conn)?;
    let mut executed = Vec::new();

    for migration in MIGRATIONS {
        if applied_ids.contains(migration.id) {
            continue;
        }

        if migration.conditional {
            let sql = migration
                .sql
                .lines()
                .skip_while(|line| line.trim().is_empty() || line.trim().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            let sql = sql.as_str();
            if let Some(rest) = sql.strip_prefix("ALTER TABLE ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if let [table, "ADD", "COLUMN", column, ..] = parts.as_slice()
                    && !column_exists(conn, table, column)
                {
                    conn.execute_batch(sql)?;
                }
            }
        } else {
            // Forward repair for databases where the old conditional parser
            // recorded multiline migration 006 without adding its column.
            if migration.id == "066_r3_machine_api"
                && !column_exists(conn, "pricing_configs", "receiveShippingFeesJson")
            {
                conn.execute_batch(include_str!("migrations/006_add_receive_shipping_fees.sql"))?;
            }
            conn.execute_batch(migration.sql)?;
            if migration.id == "005_create_model_warehouse_tables" {
                apply_conditional_columns(conn)?;
            }
        }

        conn.execute(
            "INSERT INTO schema_migrations (id, description, appliedAt) VALUES (?1, ?2, ?3)",
            rusqlite::params![migration.id, migration.description, shanghai_now_iso()],
        )?;
        executed.push(migration.id.to_string());
    }
    Ok(executed)
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;

    #[test]
    fn r3_forward_repair_restores_skipped_multiline_pricing_column() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        assert!(column_exists(
            &conn,
            "pricing_configs",
            "receiveShippingFeesJson"
        ));
        conn.execute_batch(
            "ALTER TABLE pricing_configs DROP COLUMN receiveShippingFeesJson;
            DELETE FROM schema_migrations WHERE id='066_r3_machine_api';",
        )
        .unwrap();
        run_migrations(&conn).unwrap();
        assert!(column_exists(
            &conn,
            "pricing_configs",
            "receiveShippingFeesJson"
        ));
        assert!(run_migrations(&conn).unwrap().is_empty());
    }

    #[test]
    fn clean_database_reaches_r3_damage_settlement_and_clock_authority() {
        let conn = Connection::open_in_memory().expect("open sqlite");
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        let executed = run_migrations(&conn).expect("run complete migration chain");
        assert_eq!(
            executed.last().map(String::as_str),
            Some("066_r3_machine_api")
        );
        let customer_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table'
                 AND name IN ('customers', 'customer_contacts', 'customer_external_identities',
                              'customer_history', 'customer_migration_exceptions')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(customer_tables, 5);
        let reservation_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table'
                 AND name IN ('rental_reservations', 'reservation_requirements', 'allocations',
                              'reservation_migration_exceptions')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reservation_tables, 4);
        let lifecycle_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table'
                 AND name IN ('order_lifecycle', 'order_lifecycle_history',
                              'lifecycle_migration_exceptions')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(lifecycle_tables, 3);
        let r3_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table'
                 AND name IN ('tenant_business_time_zones', 'rental_damage_findings',
                              'rental_liability_decisions', 'rental_repair_cases',
                              'rental_settlement_cases', 'rental_settlement_effect_admissions',
                              'rental_disputes')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            r3_tables, 7,
            "active R3 authority excludes renamed 063 legacy outcome tables"
        );
    }

    #[test]
    fn tenant_governance_evidence_is_append_only_and_not_cascaded() {
        let conn = Connection::open_in_memory().expect("open sqlite");
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE tenants (id TEXT PRIMARY KEY);
             CREATE TABLE audit_logs (
               id TEXT PRIMARY KEY, createdAt TEXT NOT NULL, tenant_id TEXT NOT NULL
             );
             INSERT INTO tenants VALUES ('tenant-a');",
        )
        .expect("create governance prerequisites");

        let migration = MIGRATIONS
            .iter()
            .find(|migration| migration.id == "048_tenant_governance")
            .expect("migration registered");
        conn.execute_batch(migration.sql)
            .expect("apply governance migration");

        conn.execute(
            "INSERT INTO change_intents
             (id, target_tenant_id, actor_id, reason, intended_outcome, impact,
              cost_minor, currency, source, correlation_id, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, '', NULL, NULL,
                     'platform_governance', ?6, 'recorded', ?7)",
            rusqlite::params![
                "intent-a",
                "tenant-a",
                "super-1",
                "capacity increase",
                "support a new production line",
                "request-a",
                "2026-07-19T12:00:00+08:00"
            ],
        )
        .expect("append intent");

        assert!(
            conn.execute(
                "UPDATE change_intents SET reason = 'rewritten' WHERE id = 'intent-a'",
                [],
            )
            .is_err(),
            "governance evidence must reject updates"
        );
        assert!(
            conn.execute("DELETE FROM change_intents WHERE id = 'intent-a'", [])
                .is_err(),
            "governance evidence must reject deletes"
        );

        conn.execute("DELETE FROM tenants WHERE id = 'tenant-a'", [])
            .expect("tenant lifecycle does not own evidence");
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM change_intents", [], |row| row.get(0))
            .expect("count evidence");
        assert_eq!(remaining, 1);

        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%governance%' OR name LIKE 'idx_change_intents_%'")
            .expect("prepare indexes")
            .query_map([], |row| row.get(0))
            .expect("query indexes")
            .collect::<Result<_, _>>()
            .expect("collect indexes");
        assert!(
            indexes
                .iter()
                .any(|name| name == "idx_audit_logs_governance_cursor")
        );
        assert!(
            indexes
                .iter()
                .any(|name| name == "idx_change_intents_tenant_created")
        );
    }

    #[test]
    fn reservation_rules_and_records_require_tenant_scope() {
        let conn = Connection::open_in_memory().expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE reservation_rules (
               id INTEGER PRIMARY KEY AUTOINCREMENT, rule_name TEXT NOT NULL,
               max_days_ahead INTEGER NOT NULL DEFAULT 90,
               max_concurrent_per_customer INTEGER NOT NULL DEFAULT 2,
               auto_release_minutes INTEGER NOT NULL DEFAULT 30,
               is_active INTEGER NOT NULL DEFAULT 1,
               created_at TEXT NOT NULL DEFAULT 'now', updated_at TEXT NOT NULL DEFAULT 'now'
             );
             INSERT INTO reservation_rules (id, rule_name) VALUES (1, 'default');
             CREATE TABLE inventory_reservations (
               id INTEGER PRIMARY KEY, device_serial_no TEXT NOT NULL,
               customer_name TEXT NOT NULL, customer_phone TEXT NOT NULL,
               start_date TEXT NOT NULL, end_date TEXT NOT NULL,
               status TEXT NOT NULL DEFAULT 'reserved', reserved_by INTEGER NOT NULL,
               created_at TEXT NOT NULL DEFAULT 'now', updated_at TEXT NOT NULL DEFAULT 'now',
               tenant_id TEXT
             );",
        )
        .expect("legacy reservation schema");
        let migration = MIGRATIONS
            .iter()
            .find(|migration| migration.id == "047_scope_reservation_rules")
            .expect("migration registered");
        conn.execute_batch(migration.sql)
            .expect("apply reservation scope migration");

        conn.execute(
            "INSERT INTO reservation_rules (rule_name, tenant_id) VALUES ('tenant-b', 'tenant-b')",
            [],
        )
        .expect("other tenant rule");
        assert!(conn.execute(
            "INSERT INTO reservation_rules (rule_name, tenant_id) VALUES ('duplicate', 'tenant-b')",
            [],
        ).is_err());
        assert!(conn.execute(
            "INSERT INTO inventory_reservations
             (id, device_serial_no, customer_name, customer_phone, start_date, end_date, reserved_by, tenant_id)
             VALUES (1, 'D-1', 'A', '13800000000', '2026-07-16', '2026-07-17', 1, '')",
            [],
        ).is_err());
    }

    #[test]
    fn pricing_scope_allows_tenant_specific_configs_and_dates() {
        let conn = Connection::open_in_memory().expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE pricing_configs (
               id INTEGER PRIMARY KEY, baseWeekdayPrice REAL NOT NULL,
               baseWeekendPrice REAL NOT NULL, holidayRulesJson TEXT NOT NULL DEFAULT '[]',
               receiveShippingFeesJson TEXT NOT NULL DEFAULT '{}', updatedBy TEXT DEFAULT '',
               createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL
             );
             CREATE TABLE device_models (id TEXT PRIMARY KEY);
             CREATE TABLE model_base_prices (
               modelId TEXT PRIMARY KEY, weekdayPrice REAL NOT NULL, weekendPrice REAL NOT NULL,
               updatedBy TEXT DEFAULT '', createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL
             );
             CREATE TABLE dynamic_daily_prices (
               id TEXT PRIMARY KEY, dateKey TEXT NOT NULL UNIQUE, price REAL NOT NULL,
               updatedBy TEXT DEFAULT '', createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL,
               tenant_id TEXT
             );
             INSERT INTO pricing_configs VALUES (1, 8.5, 14, '[]', '{}', '', 'now', 'now');
             INSERT INTO device_models VALUES ('model-a');
             INSERT INTO model_base_prices VALUES ('model-a', 10, 20, '', 'now', 'now');
             INSERT INTO dynamic_daily_prices VALUES ('price-a', '2026-07-16', 12, '', 'now', 'now', 'tenant-a');",
        )
        .expect("create legacy pricing schema");

        let migration = MIGRATIONS
            .iter()
            .find(|migration| migration.id == "046_scope_pricing_tables")
            .expect("migration registered");
        conn.execute_batch(migration.sql).expect("apply migration");

        conn.execute(
            "INSERT INTO pricing_configs
             (baseWeekdayPrice, baseWeekendPrice, holidayRulesJson, receiveShippingFeesJson,
              updatedBy, createdAt, updatedAt, tenant_id)
             VALUES (9, 15, '[]', '{}', '', 'now', 'now', 'tenant-b')",
            [],
        )
        .expect("second tenant pricing config");
        conn.execute(
            "INSERT INTO dynamic_daily_prices
             (id, dateKey, price, updatedBy, createdAt, updatedAt, tenant_id)
             VALUES ('price-b', '2026-07-16', 18, '', 'now', 'now', 'tenant-b')",
            [],
        )
        .expect("same date is valid in another tenant");
        assert!(
            conn.execute(
                "INSERT INTO dynamic_daily_prices
                 (id, dateKey, price, updatedBy, createdAt, updatedAt, tenant_id)
                 VALUES ('price-c', '2026-07-16', 20, '', 'now', 'now', 'tenant-a')",
                [],
            )
            .is_err()
        );
    }

    #[test]
    fn admin_support_scope_is_backfilled_and_enforced() {
        let conn = Connection::open_in_memory().expect("open sqlite");
        conn.execute_batch(
            "CREATE TABLE orders (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL);
             CREATE TABLE order_price_details (
               id TEXT PRIMARY KEY,
               orderId TEXT NOT NULL,
               dateKey TEXT NOT NULL,
               UNIQUE(orderId, dateKey)
             );
             CREATE TABLE overdue_records (
               id INTEGER PRIMARY KEY,
               tenant_id TEXT NOT NULL
             );
             CREATE TABLE overdue_notification_log (
               id INTEGER PRIMARY KEY,
               overdue_id INTEGER NOT NULL,
               escalation_level INTEGER NOT NULL
             );
             CREATE TABLE credit_scores (
               id INTEGER PRIMARY KEY,
               customer_phone TEXT NOT NULL,
               tenant_id TEXT NOT NULL
             );
             CREATE UNIQUE INDEX idx_credit_scores_phone ON credit_scores(customer_phone);
             INSERT INTO orders VALUES ('order-a', 'tenant-a');
             INSERT INTO order_price_details VALUES ('detail-a', 'order-a', '2026-07-16');
             INSERT INTO overdue_records VALUES (1, 'tenant-a');
             INSERT INTO overdue_notification_log VALUES (1, 1, 1);
             INSERT INTO credit_scores VALUES (1, '13800000000', 'tenant-a');",
        )
        .expect("create legacy schema");

        let migration = MIGRATIONS
            .iter()
            .find(|migration| migration.id == "045_complete_admin_tenant_scope")
            .expect("migration registered");
        conn.execute_batch(migration.sql).expect("apply migration");

        let detail_tenant: String = conn
            .query_row(
                "SELECT tenant_id FROM order_price_details WHERE id = 'detail-a'",
                [],
                |row| row.get(0),
            )
            .expect("price detail tenant");
        let notification_tenant: String = conn
            .query_row(
                "SELECT tenant_id FROM overdue_notification_log WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("notification tenant");
        assert_eq!(detail_tenant, "tenant-a");
        assert_eq!(notification_tenant, "tenant-a");

        conn.execute(
            "INSERT INTO credit_scores VALUES (2, '13800000000', 'tenant-b')",
            [],
        )
        .expect("same phone is valid in another tenant");
        assert!(
            conn.execute(
                "INSERT INTO credit_scores VALUES (3, '13800000000', 'tenant-a')",
                [],
            )
            .is_err()
        );

        assert!(
            conn.execute(
                "INSERT INTO order_price_details (id, orderId, dateKey, tenant_id)
                 VALUES ('detail-b', 'order-a', '2026-07-17', 'tenant-b')",
                [],
            )
            .is_err()
        );
        assert!(
            conn.execute(
                "INSERT INTO overdue_notification_log
                   (id, overdue_id, escalation_level, tenant_id)
                 VALUES (2, 1, 2, 'tenant-b')",
                [],
            )
            .is_err()
        );
    }
}

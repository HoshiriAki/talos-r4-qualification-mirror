/// PostgreSQL migration runner — activated with `--features postgres`.
///
/// Each migration mirrors the SQLite equivalent (same ID, same semantics) but uses
/// PostgreSQL-specific SQL dialect (BOOLEAN, JSONB, DOUBLE PRECISION, DO $$ blocks).
///
/// Migrations are embedded at compile time via `include_str!()` and applied in
/// numeric order through an sqlx Postgres connection pool.
#[cfg(feature = "postgres")]
use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::utils::time::shanghai_now_iso;

#[cfg(feature = "postgres")]
struct PgMigration {
    id: &'static str,
    description: &'static str,
    sql: &'static str,
    /// If true, the SQL uses DO $$ blocks to check column existence before ALTER TABLE.
    /// Non-conditional migrations use CREATE TABLE IF NOT EXISTS / CREATE INDEX IF NOT EXISTS.
    conditional: bool,
}

#[cfg(feature = "postgres")]
const PG_MIGRATIONS: &[PgMigration] = &[
    PgMigration {
        id: "001_create_core_tables",
        description: "Create base business tables and audit indexes (PG)",
        sql: include_str!("migrations/postgres/001_create_core_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "002_add_devices_notes_column",
        description: "Ensure devices.notes column exists (PG)",
        sql: include_str!("migrations/postgres/002_add_devices_notes_column.sql"),
        conditional: true,
    },
    PgMigration {
        id: "003_add_devices_fallback_return_node_column",
        description: "Ensure devices.fallbackReturnNode column exists (PG)",
        sql: include_str!("migrations/postgres/003_add_devices_fallback_return_node_column.sql"),
        conditional: true,
    },
    PgMigration {
        id: "004_create_pricing_tables",
        description: "Create pricing config tables (PG)",
        sql: include_str!("migrations/postgres/004_create_pricing_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "005_create_model_warehouse_tables",
        description: "Create device_models, base prices, warehouses, region rules, order price details (PG)",
        sql: include_str!("migrations/postgres/005_create_model_warehouse_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "006_add_pricing_receive_shipping_fees_column",
        description: "Add receive-area shipping fees column (PG)",
        sql: include_str!("migrations/postgres/006_add_pricing_receive_shipping_fees_column.sql"),
        conditional: true,
    },
    PgMigration {
        id: "007_identity_last_login",
        description: "Identity last-login field is part of the clean baseline (PG)",
        sql: include_str!("migrations/postgres/007_identity_last_login.sql"),
        conditional: true,
    },
    PgMigration {
        id: "008_add_orders_accessories",
        description: "Add accessories JSONB column to orders (PG)",
        sql: include_str!("migrations/postgres/008_add_orders_accessories.sql"),
        conditional: true,
    },
    PgMigration {
        id: "009_add_orders_status",
        description: "Add status column to orders (PG)",
        sql: include_str!("migrations/postgres/009_add_orders_status.sql"),
        conditional: true,
    },
    PgMigration {
        id: "010_add_orders_tracking_no",
        description: "Add trackingNo column to orders (PG)",
        sql: include_str!("migrations/postgres/010_add_orders_tracking_no.sql"),
        conditional: true,
    },
    PgMigration {
        id: "011_add_orders_device_models",
        description: "Add deviceModels JSONB column to orders (PG)",
        sql: include_str!("migrations/postgres/011_add_orders_device_models.sql"),
        conditional: true,
    },
    PgMigration {
        id: "012_create_user_settings",
        description: "Create user_settings table (PG)",
        sql: include_str!("migrations/postgres/012_create_user_settings.sql"),
        conditional: false,
    },
    PgMigration {
        id: "013_enhance_warehouses",
        description: "Add address, contact, notes, capacity to warehouses (PG)",
        sql: include_str!("migrations/postgres/013_enhance_warehouses.sql"),
        conditional: true,
    },
    PgMigration {
        id: "014_identity_profile_fields",
        description: "Identity profile fields are part of the clean baseline (PG)",
        sql: include_str!("migrations/postgres/014_identity_profile_fields.sql"),
        conditional: true,
    },
    PgMigration {
        id: "016_add_indexes_and_constraints",
        description: "Add performance indexes (PG)",
        sql: include_str!("migrations/postgres/016_add_indexes_and_constraints.sql"),
        conditional: false,
    },
    PgMigration {
        id: "017_add_lost_device_statuses",
        description: "Extend device statuses with lost/scrapped states (PG)",
        sql: include_str!("migrations/postgres/017_add_lost_device_statuses.sql"),
        conditional: true,
    },
    PgMigration {
        id: "018_create_api_keys",
        description: "Create api_keys table for API key auth (PG)",
        sql: include_str!("migrations/postgres/018_create_api_keys.sql"),
        conditional: false,
    },
    PgMigration {
        id: "021_order_status_enum",
        description: "Normalize order status and record history (PG)",
        sql: include_str!("migrations/postgres/021_order_status_enum.sql"),
        conditional: false,
    },
    PgMigration {
        id: "022_create_deposit_tables",
        description: "Create deposit and refund tables (PG)",
        sql: include_str!("migrations/postgres/022_create_deposit_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "023_create_damage_repair_tables",
        description: "Create damage and repair tables (PG)",
        sql: include_str!("migrations/postgres/023_create_damage_repair_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "024_create_notification_tables",
        description: "Create notification tables (PG)",
        sql: include_str!("migrations/postgres/024_create_notification_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "025_create_finance_tax_tables",
        description: "Create finance and tax tables (PG)",
        sql: include_str!("migrations/postgres/025_create_finance_tax_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "026_create_asset_lifecycle_tables",
        description: "Create asset lifecycle tables (PG)",
        sql: include_str!("migrations/postgres/026_create_asset_lifecycle_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "027_create_compliance_tables",
        description: "Create compliance tables (PG)",
        sql: include_str!("migrations/postgres/027_create_compliance_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "028_credit",
        description: "Create credit tables (PG)",
        sql: include_str!("migrations/postgres/028_credit.sql"),
        conditional: false,
    },
    PgMigration {
        id: "029_overdue",
        description: "Create overdue tables (PG)",
        sql: include_str!("migrations/postgres/029_overdue.sql"),
        conditional: false,
    },
    PgMigration {
        id: "030_contract",
        description: "Create contract tables (PG)",
        sql: include_str!("migrations/postgres/030_contract.sql"),
        conditional: false,
    },
    PgMigration {
        id: "031_optical_sop",
        description: "Create optical SOP tables (PG)",
        sql: include_str!("migrations/postgres/031_optical_sop.sql"),
        conditional: false,
    },
    PgMigration {
        id: "032_reservation",
        description: "Create reservation tables (PG)",
        sql: include_str!("migrations/postgres/032_reservation.sql"),
        conditional: false,
    },
    PgMigration {
        id: "033_booking",
        description: "Create booking tables (PG)",
        sql: include_str!("migrations/postgres/033_booking.sql"),
        conditional: false,
    },
    PgMigration {
        id: "034_create_barcode_tables",
        description: "Create barcode tables (PG)",
        sql: include_str!("migrations/postgres/034_barcode.sql"),
        conditional: false,
    },
    PgMigration {
        id: "035_create_work_tasks",
        description: "Create work_tasks table for HUD task projection and PC task queue (PG)",
        sql: include_str!("migrations/postgres/035_create_work_tasks.sql"),
        conditional: false,
    },
    PgMigration {
        id: "036_fix_work_task_shanghai_defaults",
        description: "Record the PostgreSQL work-task timestamp contract",
        sql: include_str!("migrations/postgres/036_work_task_timestamp_contract.sql"),
        conditional: false,
    },
    PgMigration {
        id: "037_create_tenants_table",
        description: "Create tenant registry with Shanghai timestamps (PG)",
        sql: include_str!("migrations/postgres/037_create_tenants_table.sql"),
        conditional: false,
    },
    PgMigration {
        id: "038_add_tenant_id_to_core_tables",
        description: "Add tenant scope to core tables idempotently (PG)",
        sql: include_str!("migrations/postgres/038_add_tenant_id_to_core_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "039_add_tenant_indexes",
        description: "Add tenant indexes (PG)",
        sql: include_str!("migrations/postgres/039_add_tenant_indexes.sql"),
        conditional: false,
    },
    PgMigration {
        id: "040_add_tenant_to_remaining_tables",
        description: "Scope remaining business tables by tenant (PG)",
        sql: include_str!("migrations/postgres/040_add_tenant_to_remaining_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "041_enforce_tenant_id_not_null",
        description: "Enforce tenant scope on core writes (PG)",
        sql: include_str!("migrations/postgres/041_enforce_tenant_id_not_null.sql"),
        conditional: false,
    },
    PgMigration {
        id: "042_add_tenant_foreign_keys",
        description: "Add tenant foreign keys (PG)",
        sql: include_str!("migrations/postgres/042_add_tenant_foreign_keys.sql"),
        conditional: false,
    },
    PgMigration {
        id: "043_add_performance_indexes",
        description: "Add tenant query performance indexes (PG)",
        sql: include_str!("migrations/postgres/043_add_performance_indexes.sql"),
        conditional: false,
    },
    PgMigration {
        id: "044_add_tax_config_tenant_scope",
        description: "Add and index tenant scope for tax configuration when available (PG)",
        sql: include_str!("migrations/postgres/044_add_tax_config_tenant_scope.sql"),
        conditional: false,
    },
    PgMigration {
        id: "045_complete_admin_tenant_scope",
        description: "Complete tenant scope for optional admin support tables (PG)",
        sql: include_str!("migrations/postgres/045_complete_admin_tenant_scope.sql"),
        conditional: false,
    },
    PgMigration {
        id: "046_scope_pricing_tables",
        description: "Scope pricing configuration, model prices, and dynamic prices by tenant (PG)",
        sql: include_str!("migrations/postgres/046_scope_pricing_tables.sql"),
        conditional: false,
    },
    PgMigration {
        id: "047_scope_reservation_rules",
        description: "Scope optional reservation policies and records by tenant (PG)",
        sql: include_str!("migrations/postgres/047_scope_reservation_rules.sql"),
        conditional: false,
    },
    PgMigration {
        id: "048_tenant_governance",
        description: "Create append-only change intents and governance audit indexes (PG)",
        sql: include_str!("migrations/postgres/048_tenant_governance.sql"),
        conditional: false,
    },
    PgMigration {
        id: "049_tenant_preview",
        description: "Create actor-bound read-only tenant preview sessions (PG)",
        sql: include_str!("migrations/postgres/049_tenant_preview.sql"),
        conditional: false,
    },
    PgMigration {
        id: "050_tenant_simulation",
        description: "Create isolated tenant simulation sessions, overlays, and evidence (PG)",
        sql: include_str!("migrations/postgres/050_tenant_simulation.sql"),
        conditional: false,
    },
    PgMigration {
        id: "051_tenant_simulation_pricing",
        description: "Add pricing simulation revision schema parity (PG)",
        sql: include_str!("migrations/postgres/051_tenant_simulation_pricing.sql"),
        conditional: false,
    },
    PgMigration {
        id: "052_identity_authority_foundation",
        description: "Replace legacy authentication storage with identity and authority tables (PG)",
        sql: include_str!("migrations/postgres/052_identity_authority_foundation.sql"),
        conditional: false,
    },
    PgMigration {
        id: "053_customer_domain",
        description: "Create tenant-scoped Customer domain and customer associations (PG)",
        sql: include_str!("migrations/postgres/053_customer_domain.sql"),
        conditional: false,
    },
    PgMigration {
        id: "054_quote_pricing_orderline",
        description: "Create Quote, immutable pricing snapshots, accessory catalog, and OrderLine associations (PG)",
        sql: include_str!("migrations/postgres/054_quote_pricing_orderline.sql"),
        conditional: false,
    },
    PgMigration {
        id: "055_reservation_allocation_v2",
        description: "Create canonical Reservation and Allocation V2 authority (PG)",
        sql: include_str!("migrations/postgres/055_reservation_allocation_v2.sql"),
        conditional: false,
    },
    PgMigration {
        id: "056_order_lifecycle_v2",
        description: "Create canonical multi-dimensional Order Lifecycle V2 authority (PG)",
        sql: include_str!("migrations/postgres/056_order_lifecycle_v2.sql"),
        conditional: false,
    },
    PgMigration {
        id: "057_durable_rental_workflow",
        description: "Create durable rental workflow, blocker, manual task, outbox, and inbox authority (PG)",
        sql: include_str!("migrations/postgres/057_durable_rental_workflow.sql"),
        conditional: false,
    },
    PgMigration {
        id: "058_return_inspection_settlement",
        description: "Create tenant-scoped Return, Inspection, damage review, and settlement skeleton (PG)",
        sql: include_str!("migrations/postgres/058_return_inspection_settlement.sql"),
        conditional: false,
    },
    PgMigration {
        id: "059_integration_fabric",
        description: "Create tenant-scoped Provider, external operation, webhook, deposit, and refund authority (PG)",
        sql: include_str!("migrations/postgres/059_integration_fabric.sql"),
        conditional: false,
    },
    PgMigration {
        id: "060_integration_runtime",
        description: "Add durable Integration Fabric runtime and reconciliation evidence (PG)",
        sql: include_str!("migrations/postgres/060_integration_runtime.sql"),
        conditional: false,
    },
    PgMigration {
        id: "061_integration_scheduler_cursor",
        description: "Add persisted fair-pagination cursor for Integration worker scheduling (PG)",
        sql: include_str!("migrations/postgres/061_integration_scheduler_cursor.sql"),
        conditional: false,
    },
    PgMigration {
        id: "062_integration_startup_recovery_snapshot",
        description: "Add bounded startup recovery snapshot for Integration worker (PG)",
        sql: include_str!("migrations/postgres/062_integration_startup_recovery_snapshot.sql"),
        conditional: false,
    },
    PgMigration {
        id: "063_r3_damage_settlement_clock",
        description: "Create R3 damage, repair, settlement, dispute, and business-time authority (PG)",
        sql: include_str!("migrations/postgres/063_r3_damage_settlement_clock.sql"),
        conditional: false,
    },
    PgMigration {
        id: "064_r3_settlement_integration_authority",
        description: "Make R2 Integration Fabric authoritative for R3 settlement external effects (PG)",
        sql: include_str!("migrations/postgres/064_r3_settlement_integration_authority.sql"),
        conditional: false,
    },
    PgMigration {
        id: "065_r3_terminal_settlement_seal",
        description: "Seal terminal R3 settlement facts until an approved amend or reopen authority exists (PG)",
        sql: include_str!("migrations/postgres/065_r3_terminal_settlement_seal.sql"),
        conditional: false,
    },
    PgMigration {
        id: "066_r3_machine_api",
        description: "Project machine identity and tenant-bound API governance",
        sql: include_str!("migrations/postgres/066_r3_machine_api.sql"),
        conditional: false,
    },
];

#[cfg(feature = "postgres")]
pub async fn run_pg_migrations(pool: &PgPool) -> anyhow::Result<Vec<String>> {
    use sqlx::Row;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            id TEXT PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    let rows = sqlx::query("SELECT id FROM schema_migrations ORDER BY id")
        .fetch_all(pool)
        .await?;
    let applied_ids: std::collections::HashSet<String> =
        rows.iter().map(|r| r.get::<String, _>("id")).collect();
    let mut executed = Vec::new();
    for migration in PG_MIGRATIONS {
        if applied_ids.contains(migration.id) {
            continue;
        }
        // PostgreSQL migrations are SQL scripts, not a single prepared
        // statement. `raw_sql` deliberately uses the simple-query path so
        // function/trigger definitions and other multi-statement migrations
        // apply atomically as their script form requires.
        sqlx::raw_sql(migration.sql).execute(pool).await?;
        sqlx::query(
            "INSERT INTO schema_migrations (id, description, applied_at) VALUES ($1, $2, $3)",
        )
        .bind(migration.id)
        .bind(migration.description)
        .bind(shanghai_now_iso())
        .execute(pool)
        .await?;
        executed.push(migration.id.to_string());
    }
    Ok(executed)
}

#[cfg(feature = "postgres")]
pub async fn create_pg_pool() -> anyhow::Result<PgPool> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://localhost:5432/talos".to_string());
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&database_url)
        .await?;
    Ok(pool)
}

#[cfg(all(test, feature = "postgres"))]
mod tests {
    use super::PG_MIGRATIONS;
    use sqlx::{Connection, PgConnection};
    use std::time::Duration;
    use tokio::sync::oneshot;

    #[derive(Clone, Copy)]
    enum OwnerMutation {
        Insert,
        PromoteExistingMembership,
    }

    struct LiveMachineAuthorityFixture {
        database_url: String,
        schema: String,
    }

    impl LiveMachineAuthorityFixture {
        async fn create(database_url: String) -> anyhow::Result<Self> {
            let schema = format!("r3_machine_authority_{}", uuid::Uuid::new_v4().simple());
            let mut connection = PgConnection::connect(&database_url).await?;
            sqlx::query(&format!("CREATE SCHEMA {schema}"))
                .execute(&mut connection)
                .await?;
            sqlx::query(&format!("SET search_path TO {schema}"))
                .execute(&mut connection)
                .await?;

            for statement in [
                "CREATE TABLE identities (id TEXT PRIMARY KEY, password_hash TEXT NOT NULL)",
                "CREATE TABLE tenant_memberships (\
                    id TEXT PRIMARY KEY,\
                    identity_id TEXT NOT NULL REFERENCES identities(id),\
                    tenant_id TEXT NOT NULL,\
                    role TEXT NOT NULL,\
                    status TEXT NOT NULL,\
                    created_at TEXT NOT NULL,\
                    updated_at TEXT NOT NULL,\
                    UNIQUE (identity_id, tenant_id),\
                    UNIQUE (id, identity_id, tenant_id)\
                 )",
                "CREATE TABLE auth_sessions (id TEXT PRIMARY KEY, identity_id TEXT NOT NULL)",
                "CREATE TABLE platform_memberships (id TEXT PRIMARY KEY, identity_id TEXT NOT NULL)",
            ] {
                sqlx::query(statement).execute(&mut connection).await?;
            }
            let machine_migration = PG_MIGRATIONS
                .iter()
                .find(|migration| migration.id == "066_r3_machine_api")
                .expect("066 PostgreSQL migration must be registered")
                .sql;
            // The migration remains repeatable on an unmerged branch.
            sqlx::raw_sql(machine_migration)
                .execute(&mut connection)
                .await?;
            sqlx::raw_sql(machine_migration)
                .execute(&mut connection)
                .await?;

            Ok(Self {
                database_url,
                schema,
            })
        }

        async fn connection(&self) -> anyhow::Result<PgConnection> {
            let mut connection = PgConnection::connect(&self.database_url).await?;
            sqlx::query(&format!("SET search_path TO {}", self.schema))
                .execute(&mut connection)
                .await?;
            Ok(connection)
        }

        async fn cleanup(self) -> anyhow::Result<()> {
            let mut connection = PgConnection::connect(&self.database_url).await?;
            sqlx::query(&format!("DROP SCHEMA {} CASCADE", self.schema))
                .execute(&mut connection)
                .await?;
            Ok(())
        }

        async fn seed_identity(
            &self,
            identity_id: &str,
            owner_mutation: OwnerMutation,
        ) -> anyhow::Result<()> {
            let mut connection = self.connection().await?;
            sqlx::query("INSERT INTO identities (id,password_hash) VALUES ($1,'!non-interactive')")
                .bind(identity_id)
                .execute(&mut connection)
                .await?;
            if matches!(owner_mutation, OwnerMutation::PromoteExistingMembership) {
                sqlx::query(
                    "INSERT INTO tenant_memberships (id,identity_id,tenant_id,role,status,created_at,updated_at) \
                     VALUES ($1,$2,'tenant-a','staff','active','now','now')",
                )
                .bind(format!("{identity_id}-membership"))
                .bind(identity_id)
                .execute(&mut connection)
                .await?;
            }
            Ok(())
        }

        async fn assert_machine_conflicts_with_owner_mutation(
            &self,
            identity_id: &str,
            owner_mutation: OwnerMutation,
        ) -> anyhow::Result<()> {
            self.seed_identity(identity_id, owner_mutation).await?;
            let mut marker = self.connection().await?;
            let mut owner = self.connection().await?;
            let mut observer = self.connection().await?;
            sqlx::query("BEGIN").execute(&mut marker).await?;
            sqlx::query("BEGIN").execute(&mut owner).await?;
            sqlx::query("SET LOCAL lock_timeout = '5s'")
                .execute(&mut owner)
                .await?;

            let (marker_inserted_tx, marker_inserted_rx) = oneshot::channel();
            let (marker_commit_tx, marker_commit_rx) = oneshot::channel();
            let marker_identity = identity_id.to_owned();
            let marker_task = tokio::spawn(async move {
                sqlx::query(
                    "INSERT INTO machine_identities (identity_id,created_at) VALUES ($1,'now')",
                )
                .bind(marker_identity)
                .execute(&mut marker)
                .await?;
                marker_inserted_tx
                    .send(())
                    .expect("test controller must await machine marker insert");
                marker_commit_rx
                    .await
                    .expect("test controller must release machine marker transaction");
                sqlx::query("COMMIT").execute(&mut marker).await?;
                Ok::<(), sqlx::Error>(())
            });
            marker_inserted_rx
                .await
                .expect("machine marker mutation must acquire the shared lock");

            let owner_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut owner)
                .await?;
            let (owner_started_tx, owner_started_rx) = oneshot::channel();
            let owner_identity = identity_id.to_owned();
            let owner_membership_id = format!("{identity_id}-membership");
            let owner_task = tokio::spawn(async move {
                owner_started_tx
                    .send(owner_pid)
                    .expect("test controller must observe owner mutation start");
                let result = match owner_mutation {
                    OwnerMutation::Insert => sqlx::query(
                        "INSERT INTO tenant_memberships (id,identity_id,tenant_id,role,status,created_at,updated_at) \
                         VALUES ($1,$2,'tenant-a','owner','active','now','now')",
                    )
                    .bind(format!("{owner_identity}-owner"))
                    .bind(owner_identity)
                    .execute(&mut owner)
                    .await,
                    OwnerMutation::PromoteExistingMembership => sqlx::query(
                        "UPDATE tenant_memberships SET role='owner' WHERE id=$1",
                    )
                    .bind(owner_membership_id)
                    .execute(&mut owner)
                    .await,
                };
                if result.is_ok() {
                    sqlx::query("COMMIT").execute(&mut owner).await?;
                } else {
                    sqlx::query("ROLLBACK").execute(&mut owner).await?;
                }
                result.map(|_| ())
            });
            let owner_pid = owner_started_rx
                .await
                .expect("owner mutation worker must start");
            // Channels hold the marker transaction open. The timeout below only
            // bounds observation of PostgreSQL's own advisory-lock wait.
            if !wait_for_advisory_waiter(&mut observer, owner_pid).await? {
                anyhow::bail!("owner mutation must wait on the marker transaction's identity lock");
            }
            marker_commit_tx
                .send(())
                .expect("machine marker worker must still hold its transaction");
            marker_task.await??;
            let owner_error = owner_task.await?.err().ok_or_else(|| {
                anyhow::anyhow!("owner mutation must reject after the marker commits")
            })?;
            if !owner_error.to_string().contains("MACHINE_AUTHORITY_GUARD") {
                anyhow::bail!("owner mutation must reject with MACHINE_AUTHORITY_GUARD");
            }

            let invariant_violated: bool = sqlx::query_scalar(
                r#"
                    SELECT EXISTS (
                        SELECT 1 FROM machine_identities mi
                        WHERE mi.identity_id=$1
                          AND EXISTS (
                              SELECT 1 FROM tenant_memberships tm
                              WHERE tm.identity_id=mi.identity_id AND tm.role='owner'
                          )
                    )
                "#,
            )
            .bind(identity_id)
            .fetch_one(&mut observer)
            .await?;
            if invariant_violated {
                anyhow::bail!("machine and owner must never coexist");
            }
            Ok(())
        }
    }

    async fn wait_for_advisory_waiter(
        observer: &mut PgConnection,
        owner_pid: i32,
    ) -> anyhow::Result<bool> {
        for _ in 0..100 {
            let waiting: bool = sqlx::query_scalar(
                r#"
                    SELECT EXISTS (
                        SELECT 1 FROM pg_locks
                        WHERE pid=$1 AND locktype='advisory' AND NOT granted
                    )
                "#,
            )
            .bind(owner_pid)
            .fetch_one(&mut *observer)
            .await?;
            if waiting {
                return Ok(true);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Ok(false)
    }

    #[test]
    fn governance_prerequisites_precede_evidence_migration() {
        let ids: Vec<&str> = PG_MIGRATIONS.iter().map(|migration| migration.id).collect();
        assert_eq!(ids.len(), 63, "PostgreSQL must mirror the SQLite registry");
        let unique: std::collections::HashSet<_> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "migration ids must be unique");
        let numbers: Vec<u16> = ids
            .iter()
            .map(|id| id[..3].parse().expect("numeric migration prefix"))
            .collect();
        assert!(
            numbers.windows(2).all(|pair| pair[0] < pair[1]),
            "PostgreSQL migrations must remain numerically ordered"
        );
        let tenant = ids
            .iter()
            .position(|id| *id == "037_create_tenants_table")
            .unwrap();
        let scope = ids
            .iter()
            .position(|id| *id == "038_add_tenant_id_to_core_tables")
            .unwrap();
        let governance = ids
            .iter()
            .position(|id| *id == "048_tenant_governance")
            .unwrap();
        let customer = ids
            .iter()
            .position(|id| *id == "053_customer_domain")
            .unwrap();
        let quote = ids
            .iter()
            .position(|id| *id == "054_quote_pricing_orderline")
            .unwrap();
        let reservation = ids
            .iter()
            .position(|id| *id == "055_reservation_allocation_v2")
            .unwrap();
        let lifecycle = ids
            .iter()
            .position(|id| *id == "056_order_lifecycle_v2")
            .unwrap();
        assert!(
            tenant < scope
                && scope < governance
                && governance < customer
                && customer < quote
                && quote < reservation
                && reservation < lifecycle
        );

        let tenant_sql = PG_MIGRATIONS[tenant].sql;
        assert!(!tenant_sql.contains("datetime('now')"));
        assert!(tenant_sql.contains("Asia/Shanghai"));
        assert!(PG_MIGRATIONS[governance].sql.contains("change_intents"));
        assert!(!PG_MIGRATIONS[governance].sql.contains("ON DELETE CASCADE"));
        assert!(
            PG_MIGRATIONS[customer]
                .sql
                .contains("customer_migration_exceptions")
        );
        assert!(
            PG_MIGRATIONS[lifecycle]
                .sql
                .contains("order_lifecycle_history")
        );
    }

    #[tokio::test]
    #[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable local PostgreSQL database"]
    async fn live_machine_owner_write_skew_is_serialized_for_insert_and_update()
    -> anyhow::Result<()> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")
            .expect("set TALOS_TEST_POSTGRES_URL to a disposable PostgreSQL database");
        let fixture = LiveMachineAuthorityFixture::create(database_url).await?;
        let result = async {
            fixture
                .assert_machine_conflicts_with_owner_mutation(
                    &format!("machine-insert-{}", uuid::Uuid::new_v4().simple()),
                    OwnerMutation::Insert,
                )
                .await?;
            fixture
                .assert_machine_conflicts_with_owner_mutation(
                    &format!("machine-promote-{}", uuid::Uuid::new_v4().simple()),
                    OwnerMutation::PromoteExistingMembership,
                )
                .await
        }
        .await;
        let cleanup = fixture.cleanup().await;
        result?;
        cleanup
    }
}

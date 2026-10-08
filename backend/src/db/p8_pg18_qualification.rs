#![cfg(all(test, feature = "postgres"))]

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

struct FreshPg18Fixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl FreshPg18Fixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_fresh_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        drop(admin);

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |connection, _| {
                let schema = schema_for_pool.clone();
                Box::pin(async move {
                    connection
                        .execute(format!("SET search_path TO {schema}").as_str())
                        .await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await?;

        Ok(Self {
            database_url,
            schema,
            pool,
        })
    }

    async fn cleanup(self) -> anyhow::Result<()> {
        self.pool.close().await;
        let mut admin = PgConnection::connect(&self.database_url).await?;
        admin
            .execute(format!("DROP SCHEMA {} CASCADE", self.schema).as_str())
            .await?;
        Ok(())
    }
}

async fn table_exists(pool: &PgPool, name: &str) -> anyhow::Result<bool> {
    let qualified = format!("{}.{name}", current_schema(pool).await?);
    Ok(
        sqlx::query_scalar::<_, Option<String>>("SELECT to_regclass($1)::text")
            .bind(qualified)
            .fetch_one(pool)
            .await?
            .is_some(),
    )
}

async fn current_schema(pool: &PgPool) -> anyhow::Result<String> {
    Ok(sqlx::query_scalar::<_, String>("SELECT current_schema()")
        .fetch_one(pool)
        .await?)
}

async fn run_migration_chain_with_diagnostics(
    pool: &PgPool,
    phase: &str,
) -> anyhow::Result<Vec<String>> {
    match crate::db::run_all_pg_migrations(pool).await {
        Ok(executed) => Ok(executed),
        Err(error) => {
            let last_applied = sqlx::query_scalar::<_, String>(
                "SELECT id FROM schema_migrations ORDER BY id DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
            let schema = current_schema(pool).await.ok();
            anyhow::bail!(
                "PostgreSQL 18 migration {phase} failed in schema {schema:?} after last applied migration {last_applied:?}: {error:#}"
            );
        }
    }
}

#[tokio::test]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_fresh_complete_migration_chain_is_idempotent() -> anyhow::Result<()> {
    let fixture = FreshPg18Fixture::create().await?;

    let first = run_migration_chain_with_diagnostics(&fixture.pool, "fresh-apply").await?;
    assert!(
        !first.is_empty(),
        "fresh PostgreSQL schema must apply migrations"
    );
    for required in [
        "001_create_core_tables",
        "052_identity_authority_foundation",
        "057_durable_rental_workflow",
        "066_r3_machine_api",
        "067_r4_interconnect_fabric",
        "068_r4_plugin_host_security",
        "069_r4_platform_security_hardening",
        "070_r4_session_revocation_invariant",
        "071_r4_r3_terminal_blocker_trigger_fix",
        "072_r4_tenant_scoped_catalog_names",
        "073_r4_tenant_scoped_device_serials",
        "074_r4_tenant_scoped_asset_purchases",
        "075_r4_depreciation_tenant_invariant",
        "076_r4_tenant_scoped_invoice_numbers",
        "077_r4_tenant_scoped_tax_config",
        "078_r4_credit_score_tenant_invariant",
        "079_r4_overdue_tenant_invariant",
        "080_r4_contract_tenant_invariant",
        "081_r4_optical_sop_tenant_invariant",
        "082_r4_reservation_rule_tenant_invariant",
        "083_r4_reservation_rule_sequence_invariant",
    ] {
        assert!(
            first.iter().any(|id| id == required),
            "fresh migration chain did not execute {required}: {first:?}"
        );
    }

    sqlx::query(
        "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)
         VALUES
         ('tenant-catalog-a','Catalog A','tenant-catalog-a','active','test','now','now'),
         ('tenant-catalog-b','Catalog B','tenant-catalog-b','active','test','now','now'),
         ('tenant-sequence-proof','Sequence Proof','tenant-sequence-proof','active','test','now','now')",
    )
    .execute(&fixture.pool)
    .await?;

    let reservation_rule_tenant_column: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema=current_schema()
              AND table_name='reservation_rules'
              AND column_name='tenant_id'
              AND is_nullable='NO'
        )",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert!(
        reservation_rule_tenant_column,
        "reservation_rules tenant_id must exist and be NOT NULL in the active schema"
    );

    let sequence_rule_id: i64 = sqlx::query_scalar(
        "INSERT INTO reservation_rules
         (rule_name,max_days_ahead,max_concurrent_per_customer,
          auto_release_minutes,is_active,created_at,updated_at,tenant_id)
         VALUES ('sequence-proof',90,2,30,1,'now','now','tenant-sequence-proof')
         RETURNING id",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert!(
        sequence_rule_id > 1,
        "reservation_rules BIGSERIAL must advance beyond the historical explicit seed id"
    );

    sqlx::query(
        "INSERT INTO reservation_rules
         (id,rule_name,max_days_ahead,max_concurrent_per_customer,
          auto_release_minutes,is_active,created_at,updated_at,tenant_id)
         VALUES
         (10001,'tenant-a-rule',90,2,30,1,'now','now','tenant-catalog-a'),
         (10002,'tenant-b-rule',90,2,30,1,'now','now','tenant-catalog-b')",
    )
    .execute(&fixture.pool)
    .await?;
    assert!(
        sqlx::query(
            "INSERT INTO reservation_rules
             (id,rule_name,max_days_ahead,max_concurrent_per_customer,
              auto_release_minutes,is_active,created_at,updated_at,tenant_id)
             VALUES
             (10003,'tenant-a-duplicate',90,2,30,1,'now','now','tenant-catalog-a')",
        )
        .execute(&fixture.pool)
        .await
        .is_err(),
        "reservation rules must remain unique inside one tenant"
    );
    for (tenant, suffix) in [("tenant-catalog-a", "a"), ("tenant-catalog-b", "b")] {
        sqlx::query(
            "INSERT INTO device_models
             (id,name,category,prefix,enabled,createdat,updatedat,tenant_id)
             VALUES ($1,'Shared Model Name','camera',$2,true,'now','now',$3)",
        )
        .bind(format!("catalog-model-{suffix}"))
        .bind(suffix)
        .bind(tenant)
        .execute(&fixture.pool)
        .await?;
        sqlx::query(
            "INSERT INTO warehouses
             (id,name,type,enabled,createdat,updatedat,tenant_id)
             VALUES ($1,'Shared Warehouse Name','owned',true,'now','now',$2)",
        )
        .bind(format!("catalog-warehouse-{suffix}"))
        .bind(tenant)
        .execute(&fixture.pool)
        .await?;
    }
    assert!(
        sqlx::query(
            "INSERT INTO device_models
             (id,name,category,prefix,enabled,createdat,updatedat,tenant_id)
             VALUES ('catalog-model-a-dup','Shared Model Name','camera','dup',true,'now','now','tenant-catalog-a')",
        )
        .execute(&fixture.pool)
        .await
        .is_err(),
        "model names must remain unique inside one tenant"
    );
    assert!(
        sqlx::query(
            "INSERT INTO warehouses
             (id,name,type,enabled,createdat,updatedat,tenant_id)
             VALUES ('catalog-warehouse-a-dup','Shared Warehouse Name','owned',true,'now','now','tenant-catalog-a')",
        )
        .execute(&fixture.pool)
        .await
        .is_err(),
        "warehouse names must remain unique inside one tenant"
    );

    sqlx::query(
        "INSERT INTO devices
         (id,serialno,rentalstatus,createdat,tenant_id)
         VALUES
         ('catalog-device-a','SHARED-SERIAL','已入库','now','tenant-catalog-a'),
         ('catalog-device-b','SHARED-SERIAL','已入库','now','tenant-catalog-b'),
         ('catalog-device-a-only','ONLY-A','已入库','now','tenant-catalog-a')",
    )
    .execute(&fixture.pool)
    .await?;
    assert!(
        sqlx::query(
            "INSERT INTO devices
             (id,serialno,rentalstatus,createdat,tenant_id)
             VALUES ('catalog-device-a-dup','SHARED-SERIAL','已入库','now','tenant-catalog-a')",
        )
        .execute(&fixture.pool)
        .await
        .is_err(),
        "device serials must remain unique inside one tenant"
    );

    sqlx::query(
        "INSERT INTO orders
         (id,orderno,startdate,enddate,deliverydate,pickupmethods,createdat,tenant_id)
         VALUES
         ('catalog-order-a','CATALOG-A','2026-09-01','2026-09-02','2026-09-01','[]','now','tenant-catalog-a'),
         ('catalog-order-b','CATALOG-B','2026-09-01','2026-09-02','2026-09-01','[]','now','tenant-catalog-b')",
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "INSERT INTO order_devices (id,orderid,serialno,createdat,tenant_id)
         VALUES
         ('catalog-link-a','catalog-order-a','SHARED-SERIAL','now','tenant-catalog-a'),
         ('catalog-link-b','catalog-order-b','SHARED-SERIAL','now','tenant-catalog-b')",
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "INSERT INTO contracts
         (order_id,customer_name,customer_phone,tenant_id)
         VALUES ('catalog-order-a','Catalog A Customer','10086','tenant-catalog-a')",
    )
    .execute(&fixture.pool)
    .await?;
    assert!(
        sqlx::query(
            "INSERT INTO contracts
             (order_id,customer_name,customer_phone,tenant_id)
             VALUES ('catalog-order-a','Wrong Tenant','10010','tenant-catalog-b')",
        )
        .execute(&fixture.pool)
        .await
        .is_err(),
        "contract order FK must include tenant identity"
    );
    assert!(
        sqlx::query(
            "INSERT INTO order_devices (id,orderid,serialno,createdat,tenant_id)
             VALUES ('catalog-link-b-invalid','catalog-order-b','ONLY-A','now','tenant-catalog-b')",
        )
        .execute(&fixture.pool)
        .await
        .is_err(),
        "legacy order-device serial FK must include tenant identity"
    );

    sqlx::query(
        "INSERT INTO asset_purchases
         (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
          replacement_value,notes,created_at,tenant_id)
         VALUES
         ('purchase-a','SHARED-SERIAL',100,'2026-09-01','','',100,'','now','tenant-catalog-a'),
         ('purchase-b','SHARED-SERIAL',120,'2026-09-01','','',120,'','now','tenant-catalog-b')",
    )
    .execute(&fixture.pool)
    .await?;
    assert!(
        sqlx::query(
            "INSERT INTO asset_purchases
             (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
              replacement_value,notes,created_at,tenant_id)
             VALUES ('purchase-a-dup','SHARED-SERIAL',130,'2026-09-01','','',130,'','now','tenant-catalog-a')",
        )
        .execute(&fixture.pool)
        .await
        .is_err(),
        "asset purchase serials must remain unique inside one tenant"
    );

    let second = run_migration_chain_with_diagnostics(&fixture.pool, "repeat-apply").await?;
    assert!(
        second.is_empty(),
        "repeated production migration startup must be idempotent: {second:?}"
    );

    let recorded: Vec<String> = sqlx::query_scalar("SELECT id FROM schema_migrations ORDER BY id")
        .fetch_all(&fixture.pool)
        .await?;
    for required in [
        "067_r4_interconnect_fabric",
        "068_r4_plugin_host_security",
        "069_r4_platform_security_hardening",
        "070_r4_session_revocation_invariant",
        "071_r4_r3_terminal_blocker_trigger_fix",
        "072_r4_tenant_scoped_catalog_names",
        "073_r4_tenant_scoped_device_serials",
        "074_r4_tenant_scoped_asset_purchases",
        "076_r4_tenant_scoped_invoice_numbers",
        "077_r4_tenant_scoped_tax_config",
        "078_r4_credit_score_tenant_invariant",
        "079_r4_overdue_tenant_invariant",
        "080_r4_contract_tenant_invariant",
        "081_r4_optical_sop_tenant_invariant",
        "082_r4_reservation_rule_tenant_invariant",
        "083_r4_reservation_rule_sequence_invariant",
    ] {
        assert!(
            recorded.iter().any(|id| id == required),
            "schema_migrations must record {required}"
        );
    }

    for table in [
        "orders",
        "identities",
        "auth_sessions",
        "auth_rate_limit_state",
        "interconnect_state_heads",
        "plugin_packages",
    ] {
        assert!(
            table_exists(&fixture.pool, table).await?,
            "fresh PostgreSQL production schema missing table {table}"
        );
    }

    let trigger_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM pg_trigger WHERE tgname='trg_auth_password_change_revoke_sessions' AND NOT tgisinternal",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(
        trigger_count, 1,
        "P7 password/session revocation trigger must exist exactly once after fresh migration"
    );

    fixture.cleanup().await?;
    Ok(())
}

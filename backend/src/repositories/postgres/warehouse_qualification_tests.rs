use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{
    NewWarehouse, PostgresRepositoryProvider, RepositoryProvider, UpsertWarehouseRegionRule,
    WarehouseDeviceMoveOutcome, WarehouseMutationError, WarehousePatch,
};

struct LiveWarehouseAuthorityFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveWarehouseAuthorityFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!(
            "r4_p8_warehouse_authority_{}",
            uuid::Uuid::new_v4().simple()
        );
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
        crate::db::run_all_pg_migrations(&pool).await?;

        sqlx::query(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)
             VALUES
             ('tenant-warehouse-a','Warehouse Tenant A','tenant-warehouse-a','active','test','2026-09-25T00:00:00Z','2026-09-25T00:00:00Z'),
             ('tenant-warehouse-b','Warehouse Tenant B','tenant-warehouse-b','active','test','2026-09-25T00:00:00Z','2026-09-25T00:00:00Z')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO device_models
             (id,name,category,prefix,enabled,createdat,updatedat,tenant_id)
             VALUES
             ('warehouse-model-a','Warehouse Model A','camera','A',true,'now','now','tenant-warehouse-a'),
             ('warehouse-model-b','Warehouse Model B','camera','B',true,'now','now','tenant-warehouse-b')",
        )
        .execute(&pool)
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

fn context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("warehouse-authority-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(
            tenant_id,
            Revision::new("warehouse-authority-revision").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn new_warehouse(id: &str) -> NewWarehouse {
    NewWarehouse {
        id: id.into(),
        name: "Shared Warehouse Authority Name".into(),
        wh_type: "owned".into(),
        enabled: true,
        address: "Address".into(),
        contact_name: "Contact".into(),
        contact_phone: "10086".into(),
        notes: "warehouse authority".into(),
        capacity: 10,
        now: "2026-09-25T12:00:00+08:00".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_warehouse_authority_preserves_scope_regions_devices_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveWarehouseAuthorityFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-warehouse-a", "warehouse-request-a");
    let ctx_b = context("tenant-warehouse-b", "warehouse-request-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let created_a = scoped_a
            .warehouses()
            .create(&new_warehouse("warehouse-a"))?;
        let created_b = scoped_b
            .warehouses()
            .create(&new_warehouse("warehouse-b"))?;
        assert_eq!(created_a.name, created_b.name);
        assert!(scoped_b.warehouses().get("warehouse-a")?.is_none());
        assert_eq!(scoped_a.warehouses().list()?.len(), 1);
        assert_eq!(scoped_b.warehouses().list()?.len(), 1);

        let duplicate = scoped_a
            .warehouses()
            .create(&new_warehouse("warehouse-a-duplicate"));
        assert!(matches!(
            duplicate,
            Err(WarehouseMutationError::DuplicateName)
        ));

        let updated = scoped_a.warehouses().update(
            "warehouse-a",
            &WarehousePatch {
                notes: Some("updated".into()),
                capacity: Some(20),
                now: "2026-09-25T12:01:00+08:00".into(),
                ..WarehousePatch::default()
            },
        )?;
        assert_eq!(updated.notes, "updated");
        assert_eq!(updated.capacity, 20);

        let rules = scoped_a.warehouses().upsert_region_rule(
            "warehouse-a",
            &UpsertWarehouseRegionRule {
                id: "warehouse-rule-a".into(),
                province: "Shanghai".into(),
                shipping_days: 1,
                return_days: 2,
                is_primary: true,
                now: "2026-09-25T12:02:00+08:00".into(),
            },
        )?;
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].province, "Shanghai");

        let routing_rules = scoped_a
            .warehouses()
            .routing_rules_for_province("Shanghai")?;
        assert_eq!(routing_rules.len(), 1);
        assert_eq!(routing_rules[0].warehouse_id, "warehouse-a");
        assert_eq!(routing_rules[0].shipping_days, 1);
        assert!(
            scoped_b
                .warehouses()
                .routing_rules_for_province("Shanghai")?
                .is_empty()
        );

        assert!(matches!(
            scoped_b.warehouses().region_rules("warehouse-a"),
            Err(WarehouseMutationError::NotFound)
        ));

        sqlx::query(
            "INSERT INTO devices
             (id,serialno,rentalstatus,notes,modelid,currentwarehouseid,expectedwarehouseid,expectedavailabledate,createdat,tenant_id)
             VALUES
             ('warehouse-device-a','WAREHOUSE001','已入库','needle-note','warehouse-model-a','warehouse-a','','','2026-09-25T12:03:00+08:00','tenant-warehouse-a')",
        )
        .execute(&fixture.pool)
        .await?;

        let stats = scoped_a.warehouses().stats()?;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].total_devices, 1);
        assert_eq!(stats[0].available_devices, 1);

        let low_stock = scoped_a.warehouses().advanced_low_stock(None, 2)?;
        assert_eq!(low_stock.len(), 1);
        assert_eq!(low_stock[0].warehouse_id, "warehouse-a");
        assert_eq!(low_stock[0].available_count, 1);
        let low_stock_b = scoped_b.warehouses().advanced_low_stock(None, 2)?;
        assert_eq!(low_stock_b.len(), 1);
        assert_eq!(low_stock_b[0].warehouse_id, "warehouse-b");
        assert_eq!(low_stock_b[0].available_count, 0);

        let capacity = scoped_a.warehouses().advanced_capacity_stats()?;
        assert_eq!(capacity.len(), 1);
        assert_eq!(capacity[0].warehouse_id, "warehouse-a");
        assert_eq!(capacity[0].capacity, 20);
        assert_eq!(capacity[0].total_devices, 1);
        assert_eq!(capacity[0].available_devices, 1);

        let devices = scoped_a
            .warehouses()
            .devices("warehouse-a", Some("已入库"))?;
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].model_name, "Warehouse Model A");

        let page = scoped_a
            .warehouses()
            .devices_paged("warehouse-a", 1, 20, Some("needle"))?;
        assert_eq!(page.total, 1);
        assert_eq!(page.data.len(), 1);
        assert_eq!(page.data[0].serial_no, "WAREHOUSE001");

        let mut target = new_warehouse("warehouse-a-target");
        target.name = "Target Warehouse".into();
        target.capacity = 30;
        scoped_a.warehouses().create(&target)?;

        assert_eq!(
            scoped_b.warehouses().move_device_between_warehouses(
                "WAREHOUSE001",
                "warehouse-a",
                "warehouse-a-target",
            )?,
            WarehouseDeviceMoveOutcome::DeviceNotInSource
        );
        assert_eq!(
            scoped_a.warehouses().move_device_between_warehouses(
                "WAREHOUSE001",
                "warehouse-a",
                "missing-target",
            )?,
            WarehouseDeviceMoveOutcome::TargetWarehouseNotFound
        );
        assert_eq!(
            scoped_a.warehouses().move_device_between_warehouses(
                "WAREHOUSE001",
                "warehouse-a",
                "warehouse-a-target",
            )?,
            WarehouseDeviceMoveOutcome::Moved
        );
        assert_eq!(
            scoped_a
                .warehouses()
                .devices("warehouse-a-target", Some("已入库"))?
                .len(),
            1
        );
        assert_eq!(
            scoped_a.warehouses().move_device_between_warehouses(
                "WAREHOUSE001",
                "warehouse-a-target",
                "warehouse-a",
            )?,
            WarehouseDeviceMoveOutcome::Moved
        );

        assert!(matches!(
            scoped_a.warehouses().delete("warehouse-a"),
            Err(WarehouseMutationError::Referenced(1))
        ));
        scoped_a
            .warehouses()
            .delete_region_rule("warehouse-a", "Shanghai")?;
        assert!(
            scoped_a
                .warehouses()
                .region_rules("warehouse-a")?
                .is_empty()
        );
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_a_after = recomposed.bind(&context(
        "tenant-warehouse-a",
        "warehouse-request-recomposed",
    ))?;
    assert_eq!(
        scoped_a_after
            .warehouses()
            .get("warehouse-a")?
            .expect("updated warehouse survives provider recomposition")
            .notes,
        "updated"
    );

    sqlx::query("DELETE FROM devices WHERE tenant_id=$1 AND serialno=$2")
        .bind("tenant-warehouse-a")
        .bind("WAREHOUSE001")
        .execute(&fixture.pool)
        .await?;
    scoped_a_after.warehouses().delete("warehouse-a")?;
    assert!(scoped_a_after.warehouses().get("warehouse-a")?.is_none());

    fixture.cleanup().await
}

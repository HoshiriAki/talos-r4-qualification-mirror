use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{PostgresRepositoryProvider, RepairMutationError, RepositoryProvider};

struct LiveRepairFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveRepairFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_repair_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-repair-a','Repair A','tenant-repair-a','active','test','now','now'),
             ('tenant-repair-b','Repair B','tenant-repair-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno,createdat,status,tenant_id)
             VALUES
             ('repair-order-a','REP-A','2026-09-01','2026-09-02','2026-09-01','pickup','','','','now','draft','tenant-repair-a'),
             ('repair-order-b','REP-B','2026-09-01','2026-09-02','2026-09-01','pickup','','','','now','draft','tenant-repair-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices (id,serialno,rentalstatus,createdat,tenant_id)
             VALUES
             ('repair-device-a','REP-A-1','repairing','now','tenant-repair-a'),
             ('repair-device-b','REP-B-1','repairing','now','tenant-repair-b')",
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
        ActorIdentity::authenticated("repair-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("repair-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_repair_authority_preserves_scope_lifecycle_device_return_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveRepairFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-repair-a", "repair-a");
    let ctx_b = context("tenant-repair-b", "repair-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let damage = scoped_a.damages().report(
            "repair-order-a",
            "REP-A-1",
            false,
            true,
            true,
            "screen damage",
            "repair-actor",
            "2026-09-29T20:58:00+08:00",
        )?;
        scoped_a.damages().assess(
            &damage.damage_id,
            500.0,
            "customer",
            "repair required",
            "repair-actor",
            "2026-09-29T20:59:00+08:00",
        )?;
        scoped_a.damages().adjudicate(
            &damage.damage_id,
            "repair-actor",
            "2026-09-29T21:00:00+08:00",
        )?;

        let pending_damage = scoped_a.damages().report(
            "repair-order-a",
            "REP-A-1",
            false,
            true,
            true,
            "second finding",
            "repair-actor",
            "2026-09-29T21:01:00+08:00",
        )?;

        let created = scoped_a.repairs().create(
            &damage.damage_id,
            "replace screen",
            "vendor-a",
            "repair-actor",
            "2026-09-29T21:02:00+08:00",
        )?;
        assert_eq!(created.device_serial_no, "REP-A-1");
        assert_eq!(scoped_a.repairs().list(None, None, 1, 20)?.total, 1);
        assert_eq!(scoped_b.repairs().list(None, None, 1, 20)?.total, 0);

        let cross_tenant = scoped_b
            .repairs()
            .start(&created.repair_id, "2026-09-29T21:03:00+08:00");
        assert!(matches!(cross_tenant, Err(RepairMutationError::NotFound)));

        let duplicate = scoped_a.repairs().create(
            &damage.damage_id,
            "",
            "",
            "repair-actor",
            "2026-09-29T21:04:00+08:00",
        );
        assert!(matches!(
            duplicate,
            Err(RepairMutationError::AlreadyExists(_))
        ));

        let not_adjudicated = scoped_a.repairs().create(
            &pending_damage.damage_id,
            "",
            "",
            "repair-actor",
            "2026-09-29T21:05:00+08:00",
        );
        assert!(matches!(
            not_adjudicated,
            Err(RepairMutationError::DamageNotAdjudicated(status)) if status == "reported"
        ));

        let premature = scoped_a.repairs().complete(
            &created.repair_id,
            500.0,
            "repair-actor",
            "2026-09-29T21:06:00+08:00",
        );
        assert!(matches!(
            premature,
            Err(RepairMutationError::NotInProgress(status)) if status == "pending"
        ));

        scoped_a
            .repairs()
            .start(&created.repair_id, "2026-09-29T21:07:00+08:00")?;
        scoped_a.repairs().complete(
            &created.repair_id,
            500.0,
            "repair-actor",
            "2026-09-29T21:08:00+08:00",
        )?;
        let returned = scoped_a.repairs().return_to_stock(
            &created.repair_id,
            "repair-actor",
            "2026-09-29T21:09:00+08:00",
        )?;
        assert_eq!(returned.status, "returned");
        assert_eq!(returned.device_serial_no.as_deref(), Some("REP-A-1"));

        let device = scoped_a.devices().get("REP-A-1")?.expect("device");
        assert_eq!(device.status, "available");

        let stats = scoped_a.repairs().device_stats("REP-A-1")?;
        assert_eq!(stats.total_repairs, 1);
        assert_eq!(stats.total_repair_cost, 500.0);
        assert_eq!(stats.recent_repairs.len(), 1);
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-repair-a", "repair-recomposed"))?;
    let persisted = scoped_after
        .repairs()
        .list(Some("returned"), Some("REP-A-1"), 1, 20)?;
    assert_eq!(persisted.total, 1);
    assert_eq!(persisted.repairs[0].repair_cost, 500.0);

    fixture.cleanup().await
}

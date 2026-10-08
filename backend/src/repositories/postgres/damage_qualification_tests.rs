use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{DamageMutationError, PostgresRepositoryProvider, RepositoryProvider};

struct LiveDamageFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveDamageFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_damage_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-damage-a','Damage A','tenant-damage-a','active','test','now','now'),
             ('tenant-damage-b','Damage B','tenant-damage-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno,createdat,status,tenant_id)
             VALUES
             ('damage-order-a','DMG-A','2026-09-01','2026-09-02','2026-09-01','pickup','','','','now','draft','tenant-damage-a'),
             ('damage-order-b','DMG-B','2026-09-01','2026-09-02','2026-09-01','pickup','','','','now','draft','tenant-damage-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices (id,serialno,rentalstatus,createdat,tenant_id)
             VALUES
             ('damage-device-a','DMG-A-1','已入库','now','tenant-damage-a'),
             ('damage-device-b','DMG-B-1','已入库','now','tenant-damage-b')",
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
        ActorIdentity::authenticated("damage-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("damage-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_damage_authority_preserves_scope_state_machine_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveDamageFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-damage-a", "damage-a");
    let ctx_b = context("tenant-damage-b", "damage-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let reported = scoped_a.damages().report(
            "damage-order-a",
            "DMG-A-1",
            false,
            true,
            true,
            "screen damage",
            "damage-actor",
            "2026-09-29T19:30:00+08:00",
        )?;

        assert_eq!(scoped_a.damages().list(None, 1, 20)?.total, 1);
        assert_eq!(scoped_b.damages().list(None, 1, 20)?.total, 0);
        assert_eq!(scoped_a.damages().get_by_device("DMG-A-1")?.len(), 1);

        let cross_tenant = scoped_b.damages().assess(
            &reported.damage_id,
            1200.0,
            "customer",
            "",
            "damage-actor",
            "2026-09-29T19:31:00+08:00",
        );
        assert!(matches!(cross_tenant, Err(DamageMutationError::NotFound)));

        let premature = scoped_a.damages().adjudicate(
            &reported.damage_id,
            "damage-actor",
            "2026-09-29T19:32:00+08:00",
        );
        assert!(matches!(
            premature,
            Err(DamageMutationError::NotAssessed(status)) if status == "reported"
        ));

        let assessed = scoped_a.damages().assess(
            &reported.damage_id,
            1200.0,
            "customer",
            "confirmed",
            "damage-actor",
            "2026-09-29T19:33:00+08:00",
        )?;
        assert_eq!(assessed.status, "assessed");

        let adjudicated = scoped_a.damages().adjudicate(
            &reported.damage_id,
            "damage-actor",
            "2026-09-29T19:34:00+08:00",
        )?;
        assert_eq!(adjudicated.status, "adjudicated");
        assert_eq!(
            scoped_a.damages().list(Some("adjudicated"), 1, 20)?.total,
            1
        );

        let cross_resource = scoped_a.damages().report(
            "damage-order-a",
            "DMG-B-1",
            false,
            false,
            false,
            "foreign device",
            "damage-actor",
            "2026-09-29T19:35:00+08:00",
        );
        assert!(matches!(
            cross_resource,
            Err(DamageMutationError::ResourceNotFound)
        ));
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-damage-a", "damage-recomposed"))?;
    let persisted = scoped_after.damages().get_by_order("damage-order-a")?;
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].status, "adjudicated");
    assert_eq!(persisted[0].estimated_damage_amount, 1200.0);
    assert_eq!(persisted[0].liability, "customer");
    assert!(
        persisted[0]
            .damage_description
            .contains("定损备注: confirmed")
    );

    fixture.cleanup().await
}

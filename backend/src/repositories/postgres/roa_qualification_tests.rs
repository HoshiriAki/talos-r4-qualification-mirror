use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{PostgresRepositoryProvider, RepositoryProvider};

struct LiveRoaFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveRoaFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_roa_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-roa-a','ROA A','tenant-roa-a','active','test','now','now'),
             ('tenant-roa-b','ROA B','tenant-roa-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices (id,serialno,rentalstatus,createdat,tenant_id)
             VALUES
             ('roa-device-a','ROA-SHARED','已入库','now','tenant-roa-a'),
             ('roa-device-b','ROA-SHARED','已入库','now','tenant-roa-b'),
             ('roa-device-b-only','ROA-B-ONLY','已入库','now','tenant-roa-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO asset_purchases
             (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
              replacement_value,notes,created_at,tenant_id)
             VALUES
             ('roa-purchase-a','ROA-SHARED',100.0,'2026-01-01','Vendor A','ROA-A',
              100.0,'','2026-01-01T00:00:00Z','tenant-roa-a'),
             ('roa-purchase-b','ROA-SHARED',200.0,'2026-01-01','Vendor B','ROA-B',
              200.0,'','2026-01-01T00:00:00Z','tenant-roa-b'),
             ('roa-purchase-b-only','ROA-B-ONLY',300.0,'2026-01-01','Vendor B','ROA-B2',
              300.0,'','2026-01-01T00:00:00Z','tenant-roa-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,status,createdat,tenant_id)
             VALUES
             ('roa-order-a','ROA-ORDER-A','2026-01-01','2026-01-03','2026-01-01','[]','','',
              'completed','2026-01-01T00:00:00Z','tenant-roa-a'),
             ('roa-order-b','ROA-ORDER-B','2026-02-01','2026-02-03','2026-02-01','[]','','',
              'completed','2026-02-01T00:00:00Z','tenant-roa-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO order_devices (id,orderid,serialno,createdat,tenant_id)
             VALUES
             ('roa-link-a','roa-order-a','ROA-SHARED','2026-01-01T00:00:00Z','tenant-roa-a'),
             ('roa-link-b','roa-order-b','ROA-SHARED','2026-02-01T00:00:00Z','tenant-roa-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO order_price_details
             (id,orderid,datekey,finaldailyprice,pricesource,occupytype,occupyfactor,
              amount,createdat)
             VALUES
             ('roa-price-a','roa-order-a','2026-01-01',20.0,'base_weekday','normal',1.0,
              20.0,'2026-01-01T00:00:00Z'),
             ('roa-price-b','roa-order-b','2026-02-01',900.0,'base_weekday','normal',1.0,
              900.0,'2026-02-01T00:00:00Z')",
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
        ActorIdentity::authenticated("roa-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("roa-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_roa_authority_preserves_scope_revenue_schema_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveRoaFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-roa-a", "roa-a");
    let ctx_b = context("tenant-roa-b", "roa-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let basis_a = scoped_a
            .roa()
            .get("ROA-SHARED")?
            .expect("tenant A ROA basis");
        let basis_b = scoped_b
            .roa()
            .get("ROA-SHARED")?
            .expect("tenant B ROA basis");

        assert_eq!(basis_a.purchase_price, 100.0);
        assert_eq!(basis_a.total_revenue, 20.0);
        assert_eq!(basis_a.first_order_date.as_deref(), Some("2026-01-01"));
        assert_eq!(basis_b.purchase_price, 200.0);
        assert_eq!(basis_b.total_revenue, 900.0);
        assert_eq!(basis_b.first_order_date.as_deref(), Some("2026-02-01"));

        assert!(scoped_a.roa().get("ROA-B-ONLY")?.is_none());
        assert_eq!(scoped_a.roa().list()?.len(), 1);
        assert_eq!(scoped_b.roa().list()?.len(), 2);
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_a_after = recomposed.bind(&context("tenant-roa-a", "roa-recomposed"))?;
    let persisted = scoped_a_after
        .roa()
        .get("ROA-SHARED")?
        .expect("ROA basis survives provider recomposition");
    assert_eq!(persisted.total_revenue, 20.0);

    fixture.cleanup().await
}

use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{
    PostgresRepositoryProvider, RepositoryProvider, SettlementMutationError,
};

struct LiveSettlementFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveSettlementFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_settlement_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        drop(admin);

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(6)
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
             ('tenant-settlement-a','Settlement A','tenant-settlement-a','active','test','now','now'),
             ('tenant-settlement-b','Settlement B','tenant-settlement-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno,createdat,status,tenant_id)
             VALUES
             ('order-a','SET-A','2026-09-29','2026-09-30','2026-09-29','pickup','','','','now','active','tenant-settlement-a'),
             ('order-b','SET-B','2026-09-29','2026-09-30','2026-09-29','pickup','','','','now','active','tenant-settlement-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO revenue_records
             (id,order_id,amount,recognition_date,source,created_at,tenant_id)
             VALUES
             ('set-rev-a','order-a',100,'2026-09-29','order_complete','now','tenant-settlement-a'),
             ('set-rev-b','order-b',900,'2026-09-29','order_complete','now','tenant-settlement-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO deposits
             (id,order_id,amount,status,paid_at,created_at,updated_at,tenant_id)
             VALUES
             ('dep-a','order-a',50,'paid','2026-09-29T09:00:00','now','now','tenant-settlement-a'),
             ('dep-b','order-b',500,'paid','2026-09-29T09:00:00','now','now','tenant-settlement-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO deposit_ledger
             (id,deposit_id,order_id,entry_type,amount,balance_after,description,operator,created_at,tenant_id)
             VALUES
             ('set-dep-a1','dep-a','order-a','collect',50,50,'','','2026-09-29T10:00:00','tenant-settlement-a'),
             ('set-dep-a2','dep-a','order-a','release',20,30,'','','2026-09-29T11:00:00','tenant-settlement-a'),
             ('set-dep-a3','dep-a','order-a','refund',10,20,'','','2026-09-29T12:00:00','tenant-settlement-a'),
             ('set-dep-b1','dep-b','order-b','collect',500,500,'','','2026-09-29T10:00:00','tenant-settlement-b')",
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
        ActorIdentity::authenticated("settlement-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("settlement-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_settlement_authority_preserves_scope_reaggregation_confirmation_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveSettlementFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-settlement-a", "settlement-a");
    let ctx_b = context("tenant-settlement-b", "settlement-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let first = scoped_a.settlements().generate(
            "daily",
            "2026-09-29",
            "2026-09-29T00:00:00",
            "2026-09-29T23:59:59",
            "2026-09-29T22:00:00+08:00",
        )?;
        assert_eq!(first.total_revenue, Some(100.0));
        assert_eq!(first.total_deposits, Some(50.0));
        assert_eq!(first.total_refunds, Some(30.0));

        let other = scoped_b.settlements().generate(
            "daily",
            "2026-09-29",
            "2026-09-29T00:00:00",
            "2026-09-29T23:59:59",
            "2026-09-29T22:00:00+08:00",
        )?;
        assert_eq!(other.total_revenue, Some(900.0));
        assert_eq!(other.total_deposits, Some(500.0));
        assert_ne!(other.settlement_id, first.settlement_id);

        sqlx::query(
            "INSERT INTO revenue_records
             (id,order_id,amount,recognition_date,source,created_at,tenant_id)
             VALUES ('set-rev-a2','order-a2',40,'2026-09-29','other','now','tenant-settlement-a')",
        )
        .execute(&fixture.pool)
        .await?;

        let regenerated = scoped_a.settlements().generate(
            "daily",
            "2026-09-29",
            "2026-09-29T00:00:00",
            "2026-09-29T23:59:59",
            "2026-09-29T22:05:00+08:00",
        )?;
        assert_eq!(regenerated.settlement_id, first.settlement_id);
        assert_eq!(regenerated.total_revenue, Some(140.0));

        let cross_tenant = scoped_b
            .settlements()
            .confirm(&first.settlement_id, "2026-09-29T22:06:00+08:00");
        assert!(matches!(
            cross_tenant,
            Err(SettlementMutationError::NotFound)
        ));

        scoped_a
            .settlements()
            .confirm(&first.settlement_id, "2026-09-29T22:07:00+08:00")?;
        let repeated = scoped_a
            .settlements()
            .confirm(&first.settlement_id, "2026-09-29T22:08:00+08:00");
        assert!(matches!(
            repeated,
            Err(SettlementMutationError::AlreadyConfirmed)
        ));

        sqlx::query(
            "INSERT INTO revenue_records
             (id,order_id,amount,recognition_date,source,created_at,tenant_id)
             VALUES ('set-rev-a3','order-a3',60,'2026-09-29','other','now','tenant-settlement-a')",
        )
        .execute(&fixture.pool)
        .await?;

        let frozen = scoped_a.settlements().generate(
            "daily",
            "2026-09-29",
            "2026-09-29T00:00:00",
            "2026-09-29T23:59:59",
            "2026-09-29T22:09:00+08:00",
        )?;
        assert_eq!(frozen.settlement_id, first.settlement_id);
        assert!(frozen.existing);
        assert!(frozen.confirmed);
        assert_eq!(frozen.total_revenue, None);
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-settlement-a", "settlement-recomposed"))?;
    let persisted = scoped_after
        .settlements()
        .get_by_period("daily", "2026-09-29")?
        .expect("settlement");
    assert!(persisted.confirmed);
    assert_eq!(persisted.total_revenue, 140.0);
    assert_eq!(persisted.total_deposits, 50.0);
    assert_eq!(persisted.total_refunds, 30.0);
    assert_eq!(scoped_after.settlements().list(None, None, 1, 20)?.total, 1);

    let details = scoped_after
        .settlements()
        .revenue_details("2026-09-29", "2026-09-29")?;
    assert_eq!(details.len(), 3);
    assert!(details.iter().all(|row| row.amount != 900.0));

    fixture.cleanup().await
}

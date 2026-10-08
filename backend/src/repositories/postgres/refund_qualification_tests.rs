use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{PostgresRepositoryProvider, RefundMutationError, RepositoryProvider};

struct LiveRefundFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveRefundFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_refund_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-refund-a','Refund A','tenant-refund-a','active','test','now','now'),
             ('tenant-refund-b','Refund B','tenant-refund-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno,createdat,status,tenant_id)
             VALUES
             ('refund-order-a','REF-A','2026-09-01','2026-09-02','2026-09-01','pickup','','','','now','draft','tenant-refund-a'),
             ('refund-order-b','REF-B','2026-09-01','2026-09-02','2026-09-01','pickup','','','','now','draft','tenant-refund-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO deposits
             (id,order_id,amount,status,paid_at,created_at,updated_at,tenant_id)
             VALUES
             ('refund-deposit-a','refund-order-a',4000,'paid','now','now','now','tenant-refund-a'),
             ('refund-deposit-b','refund-order-b',2000,'paid','now','now','now','tenant-refund-b')",
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
        ActorIdentity::authenticated("refund-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("refund-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_refund_authority_preserves_scope_transitions_ledger_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveRefundFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-refund-a", "refund-a");
    let ctx_b = context("tenant-refund-b", "refund-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let requested = scoped_a.refunds().request(
            "refund-order-a",
            1000.0,
            "customer request",
            "refund-actor",
            "2026-09-29T18:20:00+08:00",
        )?;
        assert_eq!(requested.deposit_id, "refund-deposit-a");
        assert_eq!(requested.amount, 1000.0);

        let tenant_a = scoped_a.refunds().list(None, None, 1, 20)?;
        let tenant_b = scoped_b.refunds().list(None, None, 1, 20)?;
        assert_eq!(tenant_a.total, 1);
        assert_eq!(tenant_b.total, 0);

        let cross_tenant = scoped_b.refunds().approve(
            &requested.refund_id,
            "refund-actor",
            "2026-09-29T18:21:00+08:00",
        );
        assert!(matches!(cross_tenant, Err(RefundMutationError::NotFound)));

        let premature = scoped_a.refunds().execute_refund(
            &requested.refund_id,
            "refund-actor",
            "2026-09-29T18:22:00+08:00",
        );
        assert!(matches!(
            premature,
            Err(RefundMutationError::NotApproved(status)) if status == "pending"
        ));

        let approved = scoped_a.refunds().approve(
            &requested.refund_id,
            "refund-actor",
            "2026-09-29T18:23:00+08:00",
        )?;
        assert_eq!(approved.status, "approved");

        let executed = scoped_a.refunds().execute_refund(
            &requested.refund_id,
            "refund-actor",
            "2026-09-29T18:24:00+08:00",
        )?;
        assert_eq!(executed.amount, 1000.0);

        let executed_list =
            scoped_a
                .refunds()
                .list(Some("refund-order-a"), Some("executed"), 1, 20)?;
        assert_eq!(executed_list.total, 1);
        assert_eq!(executed_list.refunds[0].id, requested.refund_id);

        let deposit = scoped_a.deposits().get("refund-order-a")?;
        assert_eq!(
            deposit
                .ledger
                .iter()
                .map(|entry| entry.entry_type.as_str())
                .collect::<Vec<_>>(),
            vec!["refund"]
        );

        let rejected_request = scoped_a.refunds().request(
            "refund-order-a",
            250.0,
            "second request",
            "refund-actor",
            "2026-09-29T18:25:00+08:00",
        )?;
        let rejected = scoped_a.refunds().reject(
            &rejected_request.refund_id,
            "duplicate",
            "refund-actor",
            "2026-09-29T18:26:00+08:00",
        )?;
        assert_eq!(rejected.status, "rejected");
        assert_eq!(
            scoped_a
                .refunds()
                .list(None, Some("rejected"), 1, 20)?
                .total,
            1
        );
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-refund-a", "refund-recomposed"))?;
    let persisted = scoped_after.refunds().list(None, None, 1, 20)?;
    assert_eq!(persisted.total, 2);
    assert_eq!(
        scoped_after
            .deposits()
            .get("refund-order-a")?
            .ledger
            .iter()
            .filter(|entry| entry.entry_type == "refund")
            .count(),
        1
    );

    fixture.cleanup().await
}

use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{DepositMutationError, PostgresRepositoryProvider, RepositoryProvider};

struct LiveDepositFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveDepositFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_deposit_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-deposit-a','Deposit A','tenant-deposit-a','active','test','now','now'),
             ('tenant-deposit-b','Deposit B','tenant-deposit-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno,createdat,status,tenant_id)
             VALUES
             ('deposit-order-a','DEP-A','2026-09-01','2026-09-02','2026-09-01','pickup','','','','now','draft','tenant-deposit-a'),
             ('deposit-order-b','DEP-B','2026-09-01','2026-09-02','2026-09-01','pickup','','','','now','draft','tenant-deposit-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices (id,serialno,rentalstatus,createdat,tenant_id)
             VALUES
             ('deposit-device-a1','DEP-A-1','已入库','now','tenant-deposit-a'),
             ('deposit-device-a2','DEP-A-2','已入库','now','tenant-deposit-a'),
             ('deposit-device-b1','DEP-B-1','已入库','now','tenant-deposit-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO order_devices (id,orderid,serialno,createdat,tenant_id)
             VALUES
             ('deposit-link-a1','deposit-order-a','DEP-A-1','now','tenant-deposit-a'),
             ('deposit-link-a2','deposit-order-a','DEP-A-2','now','tenant-deposit-a'),
             ('deposit-link-b1','deposit-order-b','DEP-B-1','now','tenant-deposit-b')",
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
        ActorIdentity::authenticated("deposit-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("deposit-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_deposit_authority_preserves_scope_ledger_and_recomposition() -> anyhow::Result<()>
{
    let fixture = LiveDepositFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-deposit-a", "deposit-a");
    let ctx_b = context("tenant-deposit-b", "deposit-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let calculated = scoped_a.deposits().calculate(
            "deposit-order-a",
            2000.0,
            "2026-09-29T15:00:00+08:00",
        )?;
        assert!(!calculated.existing);
        assert_eq!(calculated.device_count, Some(2));
        assert_eq!(calculated.deposit.amount, 4000.0);

        let repeated = scoped_a.deposits().calculate(
            "deposit-order-a",
            2000.0,
            "2026-09-29T15:01:00+08:00",
        )?;
        assert!(repeated.existing);
        assert!(
            scoped_b
                .deposits()
                .get("deposit-order-a")?
                .deposit
                .is_none()
        );

        let missing =
            scoped_b
                .deposits()
                .calculate("deposit-order-a", 2000.0, "2026-09-29T15:02:00+08:00");
        assert!(matches!(missing, Err(DepositMutationError::OrderNotFound)));

        let collected = scoped_a.deposits().collect(
            "deposit-order-a",
            4500.0,
            "deposit-actor",
            "2026-09-29T15:03:00+08:00",
        )?;
        assert_eq!(collected.amount, 4000.0);

        let forfeited = scoped_a.deposits().forfeit(
            "deposit-order-a",
            500.0,
            "damage",
            "deposit-actor",
            "2026-09-29T15:04:00+08:00",
        )?;
        assert_eq!(forfeited.remaining_balance, 3500.0);
        assert_eq!(forfeited.status, "partially_forfeited");

        let released = scoped_a.deposits().release(
            "deposit-order-a",
            "close",
            "deposit-actor",
            "2026-09-29T15:05:00+08:00",
        )?;
        assert_eq!(released.released_amount, 3500.0);
        assert_eq!(released.forfeited_amount, 500.0);

        let detail = scoped_a.deposits().get("deposit-order-a")?;
        assert_eq!(detail.deposit.as_ref().unwrap().status, "released");
        assert_eq!(
            detail
                .ledger
                .iter()
                .map(|entry| entry.entry_type.as_str())
                .collect::<Vec<_>>(),
            vec!["collect", "forfeit", "release"]
        );
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-deposit-a", "deposit-recomposed"))?;
    let persisted = scoped_after.deposits().get("deposit-order-a")?;
    assert_eq!(
        persisted
            .deposit
            .expect("deposit record survives provider recomposition")
            .status,
        "released"
    );
    assert_eq!(persisted.ledger.len(), 3);

    fixture.cleanup().await
}

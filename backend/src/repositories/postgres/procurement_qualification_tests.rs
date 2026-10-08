use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{
    NewProcurementRecord, PostgresRepositoryProvider, ProcurementMutationError, RepositoryProvider,
};

struct LiveProcurementFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveProcurementFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_procurement_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-procurement-a','Procurement A','tenant-procurement-a','active','test','now','now'),
             ('tenant-procurement-b','Procurement B','tenant-procurement-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices (id,serialno,rentalstatus,createdat,tenant_id)
             VALUES
             ('proc-device-a','PROC-SHARED','已入库','now','tenant-procurement-a'),
             ('proc-device-b','PROC-SHARED','已入库','now','tenant-procurement-b'),
             ('proc-device-a-only','PROC-A-ONLY','已入库','now','tenant-procurement-a')",
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
        ActorIdentity::authenticated("procurement-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("procurement-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn purchase(id: &str, serial: &str, price: f64) -> NewProcurementRecord {
    NewProcurementRecord {
        id: id.into(),
        device_serial_no: serial.into(),
        purchase_price: price,
        purchase_date: "2026-09-28".into(),
        vendor: "Vendor".into(),
        invoice_no: format!("INV-{id}"),
        replacement_value: price,
        notes: "qualified".into(),
        created_at: "2026-09-28T12:00:00+08:00".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_procurement_authority_preserves_scope_identity_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveProcurementFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-procurement-a", "procurement-a");
    let ctx_b = context("tenant-procurement-b", "procurement-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let created_a =
            scoped_a
                .procurements()
                .create(&purchase("purchase-a", "PROC-SHARED", 100.0))?;
        let created_b =
            scoped_b
                .procurements()
                .create(&purchase("purchase-b", "PROC-SHARED", 200.0))?;
        assert_eq!(created_a.device_serial_no, created_b.device_serial_no);
        assert_eq!(created_a.purchase_price, 100.0);
        assert_eq!(created_b.purchase_price, 200.0);

        let duplicate =
            scoped_a
                .procurements()
                .create(&purchase("purchase-a-duplicate", "PROC-SHARED", 300.0));
        assert!(matches!(
            duplicate,
            Err(ProcurementMutationError::Duplicate)
        ));

        scoped_a
            .procurements()
            .create(&purchase("purchase-a-only", "PROC-A-ONLY", 125.0))?;
        assert!(scoped_b.procurements().get("PROC-A-ONLY")?.is_none());

        let updated = scoped_a
            .procurements()
            .set_replacement_value("PROC-SHARED", 155.0)?;
        assert_eq!(updated.replacement_value, 155.0);
        assert_eq!(
            scoped_b
                .procurements()
                .get("PROC-SHARED")?
                .expect("tenant B record")
                .replacement_value,
            200.0
        );
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_a_after =
        recomposed.bind(&context("tenant-procurement-a", "procurement-recomposed"))?;
    let persisted = scoped_a_after
        .procurements()
        .get("PROC-SHARED")?
        .expect("procurement record survives provider recomposition");
    assert_eq!(persisted.replacement_value, 155.0);

    fixture.cleanup().await
}

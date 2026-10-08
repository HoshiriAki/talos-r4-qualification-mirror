use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{
    DepreciationMutationError, PostgresRepositoryProvider, RepositoryProvider,
};

struct LiveDepreciationFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveDepreciationFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_depreciation_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-depreciation-a','Depreciation A','tenant-depreciation-a','active','test','now','now'),
             ('tenant-depreciation-b','Depreciation B','tenant-depreciation-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO asset_purchases
             (id,device_serial_no,purchase_price,purchase_date,vendor,invoice_no,
              replacement_value,notes,created_at,tenant_id)
             VALUES
             ('dep-purchase-a','DEP-SHARED',360,'2026-09-01','Vendor','INV-A',360,'','now','tenant-depreciation-a'),
             ('dep-purchase-b','DEP-SHARED',720,'2026-09-01','Vendor','INV-B',720,'','now','tenant-depreciation-b'),
             ('dep-purchase-a2','DEP-A-ONLY',180,'2026-09-01','Vendor','INV-A2',180,'','now','tenant-depreciation-a')",
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
        ActorIdentity::authenticated("depreciation-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("depreciation-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_depreciation_authority_preserves_scope_monthly_idempotence_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveDepreciationFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-depreciation-a", "depreciation-a");
    let ctx_b = context("tenant-depreciation-b", "depreciation-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let initial_a = scoped_a
            .depreciations()
            .snapshot("DEP-SHARED")?
            .expect("tenant A purchase");
        let initial_b = scoped_b
            .depreciations()
            .snapshot("DEP-SHARED")?
            .expect("tenant B purchase");
        assert_eq!(initial_a.purchase_price, 360.0);
        assert_eq!(initial_b.purchase_price, 720.0);
        assert!(scoped_b.depreciations().snapshot("DEP-A-ONLY")?.is_none());

        let run_a =
            scoped_a
                .depreciations()
                .run_monthly("2026-09", 36, "2026-09-28T12:00:00+08:00")?;
        let run_b =
            scoped_b
                .depreciations()
                .run_monthly("2026-09", 36, "2026-09-28T12:00:00+08:00")?;
        assert_eq!(run_a.total_devices, 2);
        assert_eq!(run_a.devices_processed, 2);
        assert_eq!(run_b.total_devices, 1);
        assert_eq!(run_b.devices_processed, 1);

        let duplicate =
            scoped_a
                .depreciations()
                .run_monthly("2026-09", 36, "2026-09-28T12:01:00+08:00");
        assert!(matches!(
            duplicate,
            Err(DepreciationMutationError::AlreadyRun(2))
        ));

        assert_eq!(scoped_a.depreciations().logs("DEP-SHARED")?.len(), 1);
        assert_eq!(scoped_b.depreciations().logs("DEP-SHARED")?.len(), 1);
        assert!(scoped_b.depreciations().logs("DEP-A-ONLY")?.is_empty());
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_a_after =
        recomposed.bind(&context("tenant-depreciation-a", "depreciation-recomposed"))?;
    let persisted = scoped_a_after
        .depreciations()
        .snapshot("DEP-SHARED")?
        .expect("depreciation survives provider recomposition");
    assert_eq!(persisted.depreciation_months, 1);
    assert_eq!(persisted.total_depreciation, 10.0);

    fixture.cleanup().await
}

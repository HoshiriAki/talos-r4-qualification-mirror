use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{PostgresRepositoryProvider, RepositoryProvider};

struct LiveDeviceCandidateFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveDeviceCandidateFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_device_candidate_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-device-a','Device A','tenant-device-a','active','test','2026-09-25T00:00:00Z','2026-09-25T00:00:00Z'),
             ('tenant-device-b','Device B','tenant-device-b','active','test','2026-09-25T00:00:00Z','2026-09-25T00:00:00Z')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices
             (id,serialNo,rentalStatus,createdAt,tenant_id)
             VALUES
             ('device-a2','A-SERIAL-002','available','2026-09-25T00:00:00Z','tenant-device-a'),
             ('device-a1','A-SERIAL-001','available','2026-09-25T00:00:00Z','tenant-device-a'),
             ('device-b1','B-SERIAL-001','available','2026-09-25T00:00:00Z','tenant-device-b')",
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
        ActorIdentity::authenticated("device-import-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("device-import-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_device_candidate_cutover_preserves_tenant_scope_order_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveDeviceCandidateFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());

    let tenant_a = provider.bind(&context("tenant-device-a", "device-request-a"))?;
    let tenant_b = provider.bind(&context("tenant-device-b", "device-request-b"))?;

    assert_eq!(
        tenant_a.device_candidates().list_for_import()?,
        vec!["A-SERIAL-001".to_string(), "A-SERIAL-002".to_string()]
    );
    assert_eq!(
        tenant_b.device_candidates().list_for_import()?,
        vec!["B-SERIAL-001".to_string()]
    );

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let tenant_a_after =
        recomposed.bind(&context("tenant-device-a", "device-request-a-recomposed"))?;
    assert_eq!(
        tenant_a_after.device_candidates().list_for_import()?,
        vec!["A-SERIAL-001".to_string(), "A-SERIAL-002".to_string()]
    );

    fixture.cleanup().await
}

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use crate::repositories::TenantResolutionRepository;

struct LiveTenantResolutionFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveTenantResolutionFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_tenant_resolution_{}", uuid::Uuid::new_v4().simple());
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
            "INSERT INTO tenants
             (id,name,slug,status,plan,settings,created_at,updated_at)
             VALUES
             ('tenant-a','Tenant A','tenant-a','active','pro','{\"features\":{\"reports\":true}}','2026-09-20T00:00:00Z','2026-09-20T01:00:00Z'),
             ('tenant-b','Tenant B','tenant-b','suspended','free',NULL,'2026-09-19T00:00:00Z','2026-09-19T01:00:00Z')",
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_tenant_resolution_preserves_slug_status_projection_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveTenantResolutionFixture::create().await?;
    let repository = TenantResolutionRepository::postgres(fixture.pool.clone());

    let active = repository
        .resolve_by_slug("tenant-a")?
        .expect("active tenant must resolve");
    assert_eq!(active.id, "tenant-a");
    assert_eq!(active.name, "Tenant A");
    assert_eq!(active.slug, "tenant-a");
    assert_eq!(active.status, "active");
    assert_eq!(active.plan, "pro");
    assert_eq!(
        active.settings.as_deref(),
        Some(r#"{"features":{"reports":true}}"#)
    );
    assert_eq!(active.created_at, "2026-09-20T00:00:00Z");
    assert_eq!(active.updated_at, "2026-09-20T01:00:00Z");

    let suspended = repository
        .resolve_by_slug("tenant-b")?
        .expect("suspended tenant record remains visible to the policy layer");
    assert_eq!(suspended.id, "tenant-b");
    assert_eq!(suspended.status, "suspended");
    assert!(repository.resolve_by_slug("missing")?.is_none());

    let recomposed = TenantResolutionRepository::postgres(fixture.pool.clone());
    let after_recomposition = recomposed
        .resolve_by_slug("tenant-a")?
        .expect("tenant resolution must survive repository recomposition");
    assert_eq!(after_recomposition.id, "tenant-a");
    assert_eq!(after_recomposition.status, "active");

    fixture.cleanup().await
}

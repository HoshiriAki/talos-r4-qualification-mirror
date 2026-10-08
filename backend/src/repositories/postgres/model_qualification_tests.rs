use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{
    ModelMutationError, ModelPatch, ModelPricingPatch, NewModel, PostgresRepositoryProvider,
    RepositoryProvider,
};

struct LiveModelAuthorityFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveModelAuthorityFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_model_authority_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-model-a','Model Tenant A','tenant-model-a','active','test','2026-09-25T00:00:00Z','2026-09-25T00:00:00Z'),
             ('tenant-model-b','Model Tenant B','tenant-model-b','active','test','2026-09-25T00:00:00Z','2026-09-25T00:00:00Z')",
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
        ActorIdentity::authenticated("model-authority-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(
            tenant_id,
            Revision::new("model-authority-revision").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_model_authority_preserves_scope_pricing_references_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveModelAuthorityFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-model-a", "model-request-a");
    let ctx_b = context("tenant-model-b", "model-request-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let created_a = scoped_a.models().create(&NewModel {
            id: "model-a".into(),
            name: "Model A".into(),
            category: "camera".into(),
            prefix: "A".into(),
            enabled: true,
            weekday_price: Some(100.0),
            weekend_price: Some(120.0),
            updated_by: Some("admin-a".into()),
            now: "2026-09-25T12:00:00+08:00".into(),
        })?;
        assert_eq!(created_a.weekday_price, Some(100.0));
        assert_eq!(created_a.weekend_price, Some(120.0));
        assert!(scoped_b.models().get("model-a")?.is_none());
        assert!(scoped_b.models().list()?.is_empty());

        scoped_b.models().create(&NewModel {
            id: "model-b".into(),
            name: "Model B".into(),
            category: "camera".into(),
            prefix: "B".into(),
            enabled: true,
            weekday_price: None,
            weekend_price: None,
            updated_by: None,
            now: "2026-09-25T12:00:00+08:00".into(),
        })?;
        assert_eq!(scoped_a.models().list()?.len(), 1);
        assert_eq!(scoped_b.models().list()?.len(), 1);

        let duplicate = scoped_a.models().create(&NewModel {
            id: "model-a-duplicate".into(),
            name: "Model A".into(),
            category: "camera".into(),
            prefix: "AX".into(),
            enabled: true,
            weekday_price: None,
            weekend_price: None,
            updated_by: None,
            now: "2026-09-25T12:01:00+08:00".into(),
        });
        assert!(matches!(duplicate, Err(ModelMutationError::DuplicateName)));

        let foreign_update = scoped_b.models().update(
            "model-a",
            &ModelPatch {
                name: Some("Foreign Rewrite".into()),
                now: "2026-09-25T12:02:00+08:00".into(),
                ..ModelPatch::default()
            },
        );
        assert!(matches!(foreign_update, Err(ModelMutationError::NotFound)));

        let updated = scoped_a.models().update(
            "model-a",
            &ModelPatch {
                name: Some("Model A Updated".into()),
                pricing: ModelPricingPatch::Replace {
                    weekday_price: 110.0,
                    weekend_price: 130.0,
                    updated_by: Some("admin-a".into()),
                },
                now: "2026-09-25T12:03:00+08:00".into(),
                ..ModelPatch::default()
            },
        )?;
        assert_eq!(updated.name, "Model A Updated");
        assert_eq!(updated.weekday_price, Some(110.0));
        assert_eq!(updated.weekend_price, Some(130.0));

        sqlx::query(
            "INSERT INTO devices
             (id,serialNo,rentalStatus,notes,modelId,createdAt,tenant_id)
             VALUES
             ('device-model-a','MODELA001','available','','model-a','2026-09-25T12:04:00+08:00','tenant-model-a')",
        )
        .execute(&fixture.pool)
        .await?;
        let referenced = scoped_a.models().delete("model-a");
        assert!(matches!(referenced, Err(ModelMutationError::Referenced(1))));
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_a_after = recomposed.bind(&context("tenant-model-a", "model-request-recomposed"))?;
    let persisted = scoped_a_after
        .models()
        .get("model-a")?
        .expect("updated model survives provider recomposition");
    assert_eq!(persisted.name, "Model A Updated");
    assert_eq!(persisted.weekday_price, Some(110.0));

    sqlx::query("DELETE FROM devices WHERE tenant_id = $1 AND modelid = $2")
        .bind("tenant-model-a")
        .bind("model-a")
        .execute(&fixture.pool)
        .await?;
    scoped_a_after.models().delete("model-a")?;
    assert!(scoped_a_after.models().get("model-a")?.is_none());

    fixture.cleanup().await
}

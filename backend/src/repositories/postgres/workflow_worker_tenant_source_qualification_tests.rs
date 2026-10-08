use std::sync::Arc;

use chrono::{TimeZone, Utc};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use crate::repositories::{
    PostgresRepositoryProvider, RepositoryProvider, WorkflowWorkerTenantSource,
};

struct LiveWorkerTenantSourceFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveWorkerTenantSourceFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_worker_tenants_{}", uuid::Uuid::new_v4().simple());
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

fn tenant_context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            format!("worker-fixture-{tenant}"),
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new(format!("member-{tenant}")).unwrap(),
                tenant_id: tenant_id.clone(),
                role: TenantRole::Admin,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(
            tenant_id,
            Revision::new("worker-tenant-source-pg18").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_workflow_worker_tenant_source_discovers_deduplicates_limits_and_recomposes()
-> anyhow::Result<()> {
    let fixture = LiveWorkerTenantSourceFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let now = Utc.with_ymd_and_hms(2026, 9, 19, 12, 0, 0).unwrap();

    let tenant_a = provider.bind(&tenant_context("tenant-a", "worker-source-a"))?;
    tenant_a
        .workflows()
        .start_from_message("message-a", "order-a", now)?;

    for (id, tenant, state) in [
        ("outbox-a", "tenant-a", "pending"),
        ("outbox-b", "tenant-b", "pending"),
        ("outbox-c", "tenant-c", "pending"),
        ("outbox-d", "tenant-d", "delivered"),
    ] {
        sqlx::query(
            "INSERT INTO domain_outbox
             (id,tenant_id,source_kind,source_id,message_type,idempotency_key,payload_json,
              payload_version,state,available_at,created_at)
             VALUES ($1,$2,'order',$1,'OrderConfirmed',$1,'{}',1,$3,$4,$4)",
        )
        .bind(id)
        .bind(tenant)
        .bind(state)
        .bind(now.to_rfc3339())
        .execute(&fixture.pool)
        .await?;
    }

    let source = WorkflowWorkerTenantSource::postgres(fixture.pool.clone());
    assert_eq!(
        source.discover_due_tenants(2).map_err(anyhow::Error::msg)?,
        vec!["tenant-a".to_string(), "tenant-b".to_string()]
    );
    assert_eq!(
        source
            .discover_due_tenants(100)
            .map_err(anyhow::Error::msg)?,
        vec![
            "tenant-a".to_string(),
            "tenant-b".to_string(),
            "tenant-c".to_string(),
        ]
    );

    assert!(source.discover_due_tenants(0).is_err());
    assert!(source.discover_due_tenants(1_001).is_err());

    let recomposed = WorkflowWorkerTenantSource::postgres(fixture.pool.clone());
    assert_eq!(
        recomposed
            .discover_due_tenants(100)
            .map_err(anyhow::Error::msg)?,
        vec![
            "tenant-a".to_string(),
            "tenant-b".to_string(),
            "tenant-c".to_string(),
        ]
    );

    sqlx::query("UPDATE workflow_instances SET status='completed' WHERE tenant_id='tenant-a'")
        .execute(&fixture.pool)
        .await?;
    sqlx::query("UPDATE domain_outbox SET state='delivered' WHERE tenant_id='tenant-a'")
        .execute(&fixture.pool)
        .await?;
    assert_eq!(
        recomposed
            .discover_due_tenants(100)
            .map_err(anyhow::Error::msg)?,
        vec!["tenant-b".to_string(), "tenant-c".to_string()]
    );

    fixture.cleanup().await
}

use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor, Row};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision, TenantId,
    TenantScope,
};

use crate::repositories::{PostgresRepositoryProvider, RepositoryProvider};

struct LiveLifecycleFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveLifecycleFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_lifecycle_{}", uuid::Uuid::new_v4().simple());
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

        for (id, name, slug) in [
            ("tenant-a", "Tenant A", "tenant-a"),
            ("tenant-b", "Tenant B", "tenant-b"),
        ] {
            sqlx::query(
                "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
                 VALUES ($1,$2,$3,'active','test','2026-09-16T00:00:00Z','2026-09-16T00:00:00Z')",
            )
            .bind(id)
            .bind(name)
            .bind(slug)
            .execute(&pool)
            .await?;
        }

        for (id, order_no, tenant_id) in [
            ("lifecycle-order-a", "LC-A-001", "tenant-a"),
            ("lifecycle-order-b", "LC-B-001", "tenant-b"),
        ] {
            sqlx::query(
                "INSERT INTO orders \
                 (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,deviceserialno,createdat,tenant_id,totalprice,province,sendwarehouseid,returnwarehouseid,accessories,status,trackingno,devicemodels) \
                 VALUES ($1,$2,'2026-09-16','2026-09-20','2026-09-16','[]','','','', \
                         '2026-09-16T00:00:00Z',$3,0,'','','','[]'::jsonb,'draft','','{}'::jsonb)",
            )
            .bind(id)
            .bind(order_no)
            .bind(tenant_id)
            .execute(&pool)
            .await?;
        }

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

fn normal_context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("tenant-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("lifecycle-write").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn preview_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "platform-actor",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("lifecycle-preview").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("lifecycle-preview").unwrap()),
        RequestId::new("lifecycle-preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_lifecycle_write_preserves_version_history_outbox_and_scope() -> anyhow::Result<()>
{
    let fixture = LiveLifecycleFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let tenant_a = provider.bind(&normal_context("tenant-a", "lifecycle-write-a"))?;
    let tenant_b = provider.bind(&normal_context("tenant-b", "lifecycle-write-b"))?;

    let submitted = tenant_a.lifecycles().apply_action(
        "lifecycle-order-a",
        "submit_order",
        1,
        "actor-a",
        "submit fixture",
    )?;
    assert_eq!(submitted.lifecycle.commercial_status, "submitted");
    assert_eq!(submitted.lifecycle.version, 2);

    let stale = tenant_a
        .lifecycles()
        .apply_action(
            "lifecycle-order-a",
            "confirm_order",
            1,
            "actor-a",
            "stale fixture",
        )
        .unwrap_err();
    assert_eq!(stale.code(), "REPOSITORY_CONTRACT_VIOLATION");

    let confirmed = tenant_a.lifecycles().apply_action(
        "lifecycle-order-a",
        "confirm_order",
        2,
        "actor-a",
        "confirm fixture",
    )?;
    assert_eq!(confirmed.lifecycle.commercial_status, "confirmed");
    assert_eq!(confirmed.lifecycle.version, 3);

    let history = tenant_a.lifecycles().history("lifecycle-order-a")?;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].from_status, "draft");
    assert_eq!(history[0].to_status, "submitted");
    assert_eq!(history[1].from_status, "submitted");
    assert_eq!(history[1].to_status, "confirmed");
    assert_eq!(history[1].actor_user_id, "actor-a");
    assert_eq!(history[1].reason, "confirm fixture");
    assert!(tenant_b.lifecycles().get("lifecycle-order-a")?.is_none());
    assert!(
        tenant_b
            .lifecycles()
            .history("lifecycle-order-a")?
            .is_empty()
    );

    let legacy_status: String = sqlx::query_scalar(
        "SELECT status FROM orders WHERE tenant_id='tenant-a' AND id='lifecycle-order-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(legacy_status, "confirmed");

    let outbox = sqlx::query(
        "SELECT message_type,idempotency_key,payload_json,state \
         FROM domain_outbox \
         WHERE tenant_id='tenant-a' AND source_id='lifecycle-order-a'",
    )
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(outbox.len(), 1);
    assert_eq!(
        outbox[0].try_get::<String, _>("message_type")?,
        "OrderConfirmed"
    );
    assert_eq!(
        outbox[0].try_get::<String, _>("idempotency_key")?,
        "order:lifecycle-order-a:confirmed:v3"
    );
    assert_eq!(outbox[0].try_get::<String, _>("state")?, "pending");
    let payload: serde_json::Value =
        serde_json::from_str(&outbox[0].try_get::<String, _>("payload_json")?)?;
    assert_eq!(payload["orderId"], "lifecycle-order-a");
    assert_eq!(payload["lifecycleVersion"], 3);

    let preview = provider.bind(&preview_context("tenant-a"))?;
    let preview_error = preview
        .lifecycles()
        .apply_action(
            "lifecycle-order-a",
            "require_contract",
            3,
            "platform-actor",
            "forbidden preview write",
        )
        .unwrap_err();
    assert_eq!(preview_error.code(), "REPOSITORY_PREVIEW_WRITE_DENIED");

    let version_after_preview: i64 = sqlx::query_scalar(
        "SELECT version FROM order_lifecycle \
         WHERE tenant_id='tenant-a' AND order_id='lifecycle-order-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(version_after_preview, 3);

    fixture.cleanup().await?;
    Ok(())
}

use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision, SystemModule,
    TenantId, TenantScope,
};

use crate::application::TenantPreviewCompatibilityModule;
use crate::repositories::TenantPreviewRepository;

struct LivePreviewFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LivePreviewFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_tenant_preview_{}", uuid::Uuid::new_v4().simple());
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
            "INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES
             ('preview-actor-a','preview-a','hash','Preview A','active','now','now'),
             ('preview-actor-b','preview-b','hash','Preview B','active','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)
             VALUES
             ('preview-tenant-a','Tenant A','preview-a','active','pro','now','now'),
             ('preview-tenant-b','Tenant B','preview-b','active','free','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,
              deviceserialno,createdat,status,tenant_id)
             VALUES
             ('preview-order-a','PVA','2026-10-01','2026-10-02','2026-10-01',
              'pickup','','','','now','draft','preview-tenant-a'),
             ('preview-order-b','PVB','2026-10-01','2026-10-02','2026-10-01',
              'pickup','','','','now','completed','preview-tenant-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices (id,serialno,rentalstatus,createdat,tenant_id)
             VALUES
             ('preview-device-a','PVA-1','已入库','now','preview-tenant-a'),
             ('preview-device-b','PVB-1','租赁中','now','preview-tenant-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO simulation_sessions
             (id,actor_id,target_tenant_id,namespace_id,base_revision,generation,
              idempotency_key_hash,create_request_digest,scenario_name,change_intent,status,
              created_at,expires_at)
             VALUES
             ('preview-simulation-a','preview-actor-a','preview-tenant-a','namespace-preview-a',
              1,1,'idem-preview-a','digest-preview-a','Preview scenario','Read-only validation',
              'active',NOW(),NOW()+INTERVAL '30 minutes')",
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

fn platform(actor: &str, request: &str) -> ExecutionContext {
    ExecutionContext::new(
        ActorIdentity::with_authority(
            actor,
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new(format!("membership-{actor}")).unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::platform(),
        DataScope::platform(Revision::new("preview-revision").unwrap()),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn preview(actor: &str, tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            actor,
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new(format!("membership-{actor}")).unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("preview-tenant-revision").unwrap())
            .unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-live").unwrap()),
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_tenant_preview_preserves_actor_binding_workspace_modes_tenant_reads_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LivePreviewFixture::create().await?;
    let module = TenantPreviewCompatibilityModule::new(TenantPreviewRepository::postgres(
        fixture.pool.clone(),
    ));

    let created = module
        .execute(
            "workspace.create_preview",
            serde_json::json!({
                "tenantId":"preview-tenant-a",
                "ttlMinutes":15,
                "mode":"preview"
            }),
            &platform("preview-actor-a", "preview-create"),
        )
        .map_err(anyhow::Error::msg)?;
    let id = created["id"].as_str().expect("preview id").to_owned();
    assert_eq!(created["tenantId"], "preview-tenant-a");
    assert_eq!(created["capabilities"], serde_json::json!(["preview.read"]));

    let resolved = module
        .execute(
            "session.resolve",
            serde_json::json!({"id":id}),
            &platform("preview-actor-a", "preview-resolve"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(resolved["tenantId"], "preview-tenant-a");
    assert_eq!(resolved["mode"], "preview");

    let wrong_actor = module.execute(
        "session.resolve",
        serde_json::json!({"id":id}),
        &platform("preview-actor-b", "preview-wrong-actor"),
    );
    assert!(wrong_actor.unwrap_err().contains("NOT_FOUND"));

    let simulation = module
        .execute(
            "workspace.create_simulation",
            serde_json::json!({
                "tenantId":"preview-tenant-a",
                "mode":"simulation",
                "simulationId":"preview-simulation-a"
            }),
            &platform("preview-actor-a", "preview-simulation"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(simulation["mode"], "simulation");
    assert_eq!(simulation["simulationId"], "preview-simulation-a");
    assert_eq!(
        simulation["capabilities"],
        serde_json::json!(["simulation.read", "simulation.execute"])
    );

    let cross_actor_simulation = module.execute(
        "workspace.create_simulation",
        serde_json::json!({
            "tenantId":"preview-tenant-a",
            "mode":"simulation",
            "simulationId":"preview-simulation-a"
        }),
        &platform("preview-actor-b", "preview-simulation-cross"),
    );
    assert!(
        cross_actor_simulation
            .unwrap_err()
            .contains("SIMULATION_NOT_FOUND")
    );

    let summary_a = module
        .execute(
            "dashboard.summary",
            serde_json::json!({}),
            &preview("preview-actor-a", "preview-tenant-a", "preview-summary-a"),
        )
        .map_err(anyhow::Error::msg)?;
    let summary_b = module
        .execute(
            "dashboard.summary",
            serde_json::json!({}),
            &preview("preview-actor-a", "preview-tenant-b", "preview-summary-b"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(
        summary_a,
        serde_json::json!({
            "orders":1,
            "activeOrders":1,
            "devices":1,
            "availableDevices":1
        })
    );
    assert_eq!(
        summary_b,
        serde_json::json!({
            "orders":1,
            "activeOrders":0,
            "devices":1,
            "availableDevices":0
        })
    );

    let ended = module
        .execute(
            "session.end",
            serde_json::json!({"id":id}),
            &platform("preview-actor-a", "preview-end"),
        )
        .map_err(anyhow::Error::msg)?;
    let ended_again = module
        .execute(
            "session.end",
            serde_json::json!({"id":id}),
            &platform("preview-actor-a", "preview-end-again"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(ended["status"], "ended");
    assert_eq!(ended["endedAt"], ended_again["endedAt"]);

    let paired: (String, Option<String>) = sqlx::query_as(
        "SELECT status,ended_at
         FROM tenant_workspace_sessions WHERE id=$1",
    )
    .bind(&id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(paired.0, "ended");
    assert_eq!(paired.1.as_deref(), ended["endedAt"].as_str());

    let inactive_resolve = module.execute(
        "session.resolve",
        serde_json::json!({"id":id}),
        &platform("preview-actor-a", "preview-ended-resolve"),
    );
    assert!(
        inactive_resolve
            .unwrap_err()
            .contains("PREVIEW_SESSION_INACTIVE")
    );

    let recomposed = TenantPreviewCompatibilityModule::new(TenantPreviewRepository::postgres(
        fixture.pool.clone(),
    ));
    let persisted = recomposed
        .execute(
            "session.get",
            serde_json::json!({"id":id}),
            &platform("preview-actor-a", "preview-recomposed"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(persisted["status"], "ended");
    assert_eq!(persisted["tenantId"], "preview-tenant-a");

    fixture.cleanup().await
}

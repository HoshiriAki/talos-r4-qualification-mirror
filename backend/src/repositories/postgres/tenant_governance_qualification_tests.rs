use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    PlatformMembershipId, PlatformRole, RequestId, Revision, SystemModule, TenantId, TenantScope,
};

use crate::application::TenantGovernanceCompatibilityModule;
use crate::repositories::TenantGovernanceRepository;

struct LiveGovernanceFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveGovernanceFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_tenant_governance_{}", uuid::Uuid::new_v4().simple());
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
             ('gov-tenant-a','A','gov-a','active','pro',
              '2026-09-29T10:00:00+08:00','2026-09-29T10:00:00+08:00'),
             ('gov-tenant-b','B','gov-b','suspended','free',
              '2026-09-28T10:00:00+08:00','2026-09-28T10:00:00+08:00')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO audit_events
             (id,authority_kind,tenant_id,roles_snapshot,capabilities_snapshot,action,
              resource_type,resource_id,correlation_id,detail_json,occurred_at)
             VALUES
             ('gov-audit-a','system','gov-tenant-a','[]'::jsonb,'[]'::jsonb,'login',
              'user','u-a','corr-a',
              $1::jsonb,'2026-09-29T11:00:00+08:00'),
             ('gov-audit-b','system','gov-tenant-b','[]'::jsonb,'[]'::jsonb,'login',
              'user','u-b','corr-b',
              $2::jsonb,'2026-09-29T12:00:00+08:00')",
        )
        .bind(
            serde_json::json!({
                "actor": {"id": "u-a"},
                "command": "login",
                "result": "Succeeded",
                "payload_policy": "Full",
                "payload": {"passwordHash": "leak", "ok": "visible"},
                "note": {"bankCard": "also-leak"},
                "correlation_id": "corr-a",
                "source": "qualification"
            })
            .to_string(),
        )
        .bind(
            serde_json::json!({
                "actor": {"id": "u-b"},
                "command": "login",
                "result": "Succeeded",
                "payload_policy": "ReferenceOnly",
                "payload": {"token": "leak"},
                "correlation_id": "corr-b"
            })
            .to_string(),
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

fn platform_context(request: &str) -> ExecutionContext {
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "governance-actor",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::platform(),
        DataScope::platform(Revision::new("governance-revision").unwrap()),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn tenant_context() -> ExecutionContext {
    let tenant_id = TenantId::new("gov-tenant-a").unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("governance-actor", "admin").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("governance-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new("governance-tenant-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_tenant_governance_preserves_platform_scope_redaction_health_intent_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveGovernanceFixture::create().await?;
    let module = TenantGovernanceCompatibilityModule::new(TenantGovernanceRepository::postgres(
        fixture.pool.clone(),
    ));
    let platform = platform_context("governance-platform-request");

    assert!(
        module
            .execute("tenant.list", serde_json::json!({}), &tenant_context())
            .is_err()
    );

    let first = module
        .execute("tenant.list", serde_json::json!({"limit": 1}), &platform)
        .map_err(anyhow::Error::msg)?;
    assert_eq!(first["tenants"].as_array().unwrap().len(), 1);
    assert!(first["tenants"][0].get("settings").is_none());
    let cursor = first["nextCursor"].as_str().unwrap();
    let second = module
        .execute(
            "tenant.list",
            serde_json::json!({"limit": 1, "cursor": cursor}),
            &platform,
        )
        .map_err(anyhow::Error::msg)?;
    assert_ne!(first["tenants"][0]["id"], second["tenants"][0]["id"]);

    let audit = module
        .execute(
            "audit.query",
            serde_json::json!({"tenantId": "gov-tenant-a", "result": "succeeded"}),
            &platform,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(audit["events"].as_array().unwrap().len(), 1);
    assert_eq!(audit["events"][0]["actorId"], "u-a");
    let audit_text = audit.to_string();
    assert!(!audit_text.contains("passwordHash"));
    assert!(!audit_text.contains("bankCard"));
    assert!(!audit_text.contains("leak"));

    let now = crate::utils::time::shanghai_now_iso();
    sqlx::query(
        "INSERT INTO audit_events
         (id,authority_kind,tenant_id,roles_snapshot,capabilities_snapshot,action,
          resource_type,resource_id,correlation_id,detail_json,occurred_at)
         VALUES
         ('gov-health','system','gov-tenant-a','[]'::jsonb,'[]'::jsonb,'health_probe',
          'system','probe','corr-health',$1::jsonb,$2)",
    )
    .bind(serde_json::json!({"result":"Succeeded"}).to_string())
    .bind(&now)
    .execute(&fixture.pool)
    .await?;

    let health = module
        .execute(
            "tenant.health",
            serde_json::json!({"id": "gov-tenant-a"}),
            &platform,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(health["status"], "healthy");
    assert_eq!(health["lifecycleStatus"], "active");
    assert_eq!(health["auditEvents24h"], 1);
    assert!(health["checkedAt"].as_str().unwrap().ends_with("+08:00"));

    let intent = module
        .execute(
            "change_intent.record",
            serde_json::json!({
                "targetTenantId": "gov-tenant-b",
                "reason": " capacity ",
                "intendedOutcome": " more devices ",
                "costMinor": 50000,
                "currency": "cny"
            }),
            &platform,
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(intent["actorId"], "governance-actor");
    assert_eq!(intent["targetTenantId"], "gov-tenant-b");
    assert_eq!(intent["status"], "recorded");

    let stored: (String, String, String, String, String) = sqlx::query_as(
        "SELECT actor_id,target_tenant_id,reason,currency,status
         FROM change_intents LIMIT 1",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(
        stored,
        (
            "governance-actor".into(),
            "gov-tenant-b".into(),
            "capacity".into(),
            "CNY".into(),
            "recorded".into(),
        )
    );
    assert!(
        sqlx::query("UPDATE change_intents SET reason='rewritten'")
            .execute(&fixture.pool)
            .await
            .is_err()
    );

    let recomposed = TenantGovernanceCompatibilityModule::new(
        TenantGovernanceRepository::postgres(fixture.pool.clone()),
    );
    let tenant = recomposed
        .execute(
            "tenant.get",
            serde_json::json!({"id": "gov-tenant-a"}),
            &platform_context("governance-recomposed"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(tenant["id"], "gov-tenant-a");
    let persisted_audit = recomposed
        .execute(
            "audit.get",
            serde_json::json!({"id": "gov-audit-a"}),
            &platform_context("governance-audit-recomposed"),
        )
        .map_err(anyhow::Error::msg)?;
    assert_eq!(persisted_audit["actorId"], "u-a");

    fixture.cleanup().await
}

use std::sync::Arc;

use serde_json::json;
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::audit::{AuditEvent, AuditResult};
use system_core::{
    ActorIdentity, AuditPayloadPolicy, AuthorityContext, DataScope, ExecutionContext,
    ExecutionMode, NoopHttpClient, RequestId, Revision, TenantId, TenantMembershipId, TenantRole,
    TenantScope,
};

use super::audit_sink::AuditSink;
use super::audit_sink_postgres::PostgresAuditSink;

struct LiveAuditFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveAuditFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_registry_audit_{}", uuid::Uuid::new_v4().simple());
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
             (id,name,slug,status,plan,created_at,updated_at)
             VALUES ('tenant-a','Tenant A','tenant-a','active','test','now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES ('identity-a','audit-user','hash','Audit User','active','now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO tenant_memberships
             (id,identity_id,tenant_id,role,status,created_at,updated_at)
             VALUES ('membership-a','identity-a','tenant-a','owner','active','now','now')",
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

fn tenant_context(request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new("tenant-a").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "identity-a",
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new("membership-a").unwrap(),
                tenant_id: tenant_id.clone(),
                role: TenantRole::Owner,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(
            tenant_id,
            Revision::new("registry-audit-production").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn event(id: &str, command: &str, request: &str) -> AuditEvent {
    AuditEvent::from_execution(
        id,
        &tenant_context(request),
        command,
        AuditResult::Succeeded,
        "2026-09-19T08:00:00Z",
        AuditPayloadPolicy::ReferenceOnly,
        json!({"module":"device","command":command}),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_registry_audit_is_append_only_tenant_scoped_and_restart_durable()
-> anyhow::Result<()> {
    let fixture = LiveAuditFixture::create().await?;
    let sink = PostgresAuditSink::new(fixture.pool.clone()).map_err(anyhow::Error::msg)?;

    sink.append(event(
        "audit-pg-1",
        "device.list",
        "registry-audit-request-1",
    ))
    .map_err(anyhow::Error::msg)?;

    let stored: (String, String, String, String, String) = sqlx::query_as(
        "SELECT authority_kind, tenant_id, tenant_membership_id,
                roles_snapshot::text, action
         FROM audit_events WHERE id='audit-pg-1'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(stored.0, "tenant");
    assert_eq!(stored.1, "tenant-a");
    assert_eq!(stored.2, "membership-a");
    assert!(stored.3.contains("owner"));
    assert_eq!(stored.4, "device.list");

    assert!(
        sink.append(event(
            "audit-pg-1",
            "device.get",
            "registry-audit-request-duplicate",
        ))
        .is_err()
    );
    let original_action: String =
        sqlx::query_scalar("SELECT action FROM audit_events WHERE id='audit-pg-1'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(original_action, "device.list");

    let recomposed = PostgresAuditSink::new(fixture.pool.clone()).map_err(anyhow::Error::msg)?;
    recomposed
        .append(event(
            "audit-pg-2",
            "device.get",
            "registry-audit-request-2",
        ))
        .map_err(anyhow::Error::msg)?;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE tenant_id='tenant-a'")
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(count, 2);

    fixture.cleanup().await
}

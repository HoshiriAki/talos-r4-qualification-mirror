use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use crate::repositories::{
    AuditAuthorityWrite, AuditCompatibilityRepository, AuditCompatibilityWrite,
};
use system_admin::audit::ListAuditLogsInput;

struct LiveAuditFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveAuditFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_audit_{}", uuid::Uuid::new_v4().simple());
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
             VALUES ('tenant-audit','Tenant Audit','tenant-audit','active','free',NULL,'now','now')",
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

fn write(
    id: &str,
    action: &str,
    entity: &str,
    detail: &str,
    created_at: &str,
) -> AuditCompatibilityWrite {
    AuditCompatibilityWrite {
        id: id.into(),
        tenant_id: "tenant-audit".into(),
        actor_identity_id: "actor-a".into(),
        actor_username: "actor-a".into(),
        action_type: action.into(),
        entity_type: entity.into(),
        entity_id: format!("{entity}-1"),
        entity_label: format!("{entity} label"),
        detail_json: detail.into(),
        ip: String::new(),
        user_agent: String::new(),
        created_at: created_at.into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_audit_compatibility_preserves_legacy_view_and_canonical_store()
-> anyhow::Result<()> {
    let fixture = LiveAuditFixture::create().await?;
    let repository = AuditCompatibilityRepository::postgres(fixture.pool.clone());

    repository.append_audit_log(&write(
        "audit-compat-1",
        "device.update",
        "device",
        r#"{"status":"ok","marker":"alpha"}"#,
        "2026-09-24T08:00:00+08:00",
    ))?;
    repository.append_audit_log(&write(
        "audit-compat-2",
        "order.update",
        "order",
        r#"{"status":"ok","marker":"beta"}"#,
        "2026-09-24T09:00:00+08:00",
    ))?;

    let canonical: (String, String, String) = sqlx::query_as(
        "SELECT authority_kind, action, resource_type
         FROM audit_events WHERE id = 'audit-compat-1'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(canonical.0, "system");
    assert_eq!(canonical.1, "device.update");
    assert_eq!(canonical.2, "device");

    let input = ListAuditLogsInput {
        actor_identity_id: String::new(),
        actor_username: String::new(),
        action_type: String::new(),
        entity_type: "device".into(),
        keyword: "alpha".into(),
        date: "2026-09-24".into(),
        start_date: String::new(),
        end_date: String::new(),
        limit: 100,
        offset: 0,
        sort_by: Some("actionType".into()),
        sort_order: Some("asc".into()),
    };
    let result = repository.list_audit_logs("tenant-audit", &input)?;
    assert_eq!(result.total, 1);
    assert_eq!(result.logs.len(), 1);
    assert_eq!(result.logs[0].id, "audit-compat-1");
    assert_eq!(result.logs[0].action_type, "device.update");
    assert!(result.logs[0].detail_json.contains("alpha"));

    let foreign = repository.list_audit_logs(
        "missing-tenant",
        &ListAuditLogsInput {
            limit: 100,
            ..input.clone()
        },
    )?;
    assert_eq!(foreign.total, 0);
    assert!(foreign.logs.is_empty());

    sqlx::query(
        "INSERT INTO identities
         (id,username,password_hash,display_name,status,created_at,updated_at)
         VALUES
         ('actor-tenant-audit','actor-tenant-audit','hash','Audit Actor','active','now','now')",
    )
    .execute(&fixture.pool)
    .await?;
    sqlx::query(
        "INSERT INTO tenant_memberships
         (id,identity_id,tenant_id,role,status,created_at,updated_at)
         VALUES
         ('membership-tenant-audit','actor-tenant-audit','tenant-audit','admin','active','now','now')",
    )
    .execute(&fixture.pool)
    .await?;

    repository.append_authority_event(&AuditAuthorityWrite {
        id: "audit-authority-1".into(),
        actor_identity_id: "actor-tenant-audit".into(),
        authority_kind: "tenant".into(),
        tenant_id: Some("tenant-audit".into()),
        tenant_membership_id: Some("membership-tenant-audit".into()),
        platform_membership_id: None,
        roles_snapshot: r#"["admin"]"#.into(),
        capabilities_snapshot: "[]".into(),
        action: "order.attach_device".into(),
        resource_type: "order_device".into(),
        resource_id: Some("allocation-a".into()),
        correlation_id: "audit-authority-1".into(),
        detail_json: r#"{"label":"123 -> ABC","detail":{"orderId":"order-a"}}"#.into(),
        occurred_at: "2026-09-24T10:00:00+08:00".into(),
    })?;

    let authority: (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
    ) = sqlx::query_as(
        "SELECT authority_kind, actor_identity_id, tenant_id, tenant_membership_id,
                action, resource_type, detail_json::text
         FROM audit_events WHERE id = 'audit-authority-1'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(authority.0, "tenant");
    assert_eq!(authority.1.as_deref(), Some("actor-tenant-audit"));
    assert_eq!(authority.2.as_deref(), Some("tenant-audit"));
    assert_eq!(authority.3.as_deref(), Some("membership-tenant-audit"));
    assert_eq!(authority.4, "order.attach_device");
    assert_eq!(authority.5, "order_device");
    assert!(authority.6.contains("order-a"));

    fixture.cleanup().await
}

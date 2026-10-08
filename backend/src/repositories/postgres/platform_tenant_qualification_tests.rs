use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor, Row};

use crate::repositories::{
    ClosePlatformTenant, CreatePlatformTenant, PlatformTenantAuditActor,
    PlatformTenantMutationError, PlatformTenantRepository, UpdatePlatformTenant,
    UpdatePlatformTenantStatus,
};

struct LivePlatformTenantFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LivePlatformTenantFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_platform_tenant_{}", uuid::Uuid::new_v4().simple());
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
             ('platform-owner','platform-owner','hash','Platform Owner','active','now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO platform_memberships
             (id,identity_id,status,created_at,updated_at)
             VALUES ('pm-owner','platform-owner','active','now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO platform_role_grants
             (id,platform_membership_id,role,granted_by_identity_id,granted_at)
             VALUES ('grant-owner','pm-owner','platform_owner','platform-owner','now')",
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

fn actor() -> PlatformTenantAuditActor {
    PlatformTenantAuditActor {
        identity_id: "platform-owner".into(),
        membership_id: "pm-owner".into(),
        roles_snapshot: r#"["platform_owner"]"#.into(),
        capabilities_snapshot:
            r#"["tenant_create","tenant_update","tenant_suspend","tenant_delete"]"#.into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_platform_tenant_cutover_preserves_owner_audit_lifecycle_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LivePlatformTenantFixture::create().await?;
    let repository = PlatformTenantRepository::postgres(fixture.pool.clone());

    let created = repository.create(CreatePlatformTenant {
        name: "Tenant Alpha".into(),
        slug: "tenant-alpha".into(),
        settings: Some(r#"{"theme":"dark"}"#.into()),
        owner_identity_id: "platform-owner".into(),
        actor: actor(),
        now: "2026-09-24T20:00:00+08:00".into(),
    })?;
    assert!(created.id.starts_with("tenant_"));
    assert_eq!(created.name, "Tenant Alpha");
    assert_eq!(created.slug, "tenant-alpha");
    assert_eq!(created.status, "active");
    assert_eq!(created.plan, "free");
    assert_eq!(created.settings.as_deref(), Some(r#"{"theme":"dark"}"#));

    let owner: (String, String) = sqlx::query_as(
        "SELECT identity_id, role FROM tenant_memberships
         WHERE tenant_id=$1 AND status='active'",
    )
    .bind(&created.id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(owner.0, "platform-owner");
    assert_eq!(owner.1, "owner");

    let create_audit = sqlx::query(
        "SELECT authority_kind,tenant_id,platform_membership_id,action,detail_json::text
         FROM audit_events WHERE action='tenant.created' AND resource_id=$1",
    )
    .bind(&created.id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(create_audit.get::<String, _>(0), "platform");
    assert_eq!(create_audit.get::<String, _>(1), created.id);
    assert_eq!(create_audit.get::<String, _>(2), "pm-owner");
    assert_eq!(create_audit.get::<String, _>(3), "tenant.created");
    assert!(
        create_audit
            .get::<String, _>(4)
            .contains(r#""ownerIdentityId": "platform-owner""#)
    );

    let duplicate = repository
        .create(CreatePlatformTenant {
            name: "Duplicate".into(),
            slug: "tenant-alpha".into(),
            settings: None,
            owner_identity_id: "platform-owner".into(),
            actor: actor(),
            now: "2026-09-24T20:01:00+08:00".into(),
        })
        .unwrap_err();
    assert!(matches!(duplicate, PlatformTenantMutationError::SlugExists));

    let missing_owner = repository
        .create(CreatePlatformTenant {
            name: "Missing Owner".into(),
            slug: "missing-owner".into(),
            settings: None,
            owner_identity_id: "missing".into(),
            actor: actor(),
            now: "2026-09-24T20:02:00+08:00".into(),
        })
        .unwrap_err();
    assert!(matches!(
        missing_owner,
        PlatformTenantMutationError::OwnerIdentityNotFound
    ));
    let missing_created: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE slug='missing-owner')")
            .fetch_one(&fixture.pool)
            .await?;
    assert!(!missing_created);

    let updated = repository.update(UpdatePlatformTenant {
        tenant_id: created.id.clone(),
        name: Some("Tenant Alpha Prime".into()),
        slug: Some("tenant-alpha-prime".into()),
        settings: Some(r#"{"theme":"light"}"#.into()),
        actor: actor(),
        now: "2026-09-24T20:03:00+08:00".into(),
    })?;
    assert_eq!(updated.name, "Tenant Alpha Prime");
    assert_eq!(updated.slug, "tenant-alpha-prime");
    assert_eq!(updated.settings.as_deref(), Some(r#"{"theme":"light"}"#));

    let suspended = repository.update_status(UpdatePlatformTenantStatus {
        tenant_id: created.id.clone(),
        status: "suspended".into(),
        actor: actor(),
        now: "2026-09-24T20:04:00+08:00".into(),
    })?;
    assert_eq!(suspended.status, "suspended");

    repository.close(ClosePlatformTenant {
        tenant_id: created.id.clone(),
        actor: actor(),
        now: "2026-09-24T20:05:00+08:00".into(),
    })?;
    let second_close = repository
        .close(ClosePlatformTenant {
            tenant_id: created.id.clone(),
            actor: actor(),
            now: "2026-09-24T20:06:00+08:00".into(),
        })
        .unwrap_err();
    assert!(matches!(
        second_close,
        PlatformTenantMutationError::TenantNotFoundOrClosed
    ));

    let recomposed = PlatformTenantRepository::postgres(fixture.pool.clone());
    let after = recomposed
        .get(&created.id)?
        .expect("closed tenant remains observable");
    assert_eq!(after.status, "deleted");
    assert_eq!(after.slug, "tenant-alpha-prime");

    let actions = sqlx::query_scalar::<_, String>(
        "SELECT action FROM audit_events
         WHERE tenant_id=$1 AND resource_type='tenant'
         ORDER BY occurred_at",
    )
    .bind(&created.id)
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(
        actions,
        vec![
            "tenant.created",
            "tenant.updated",
            "tenant.status_changed",
            "tenant.closed",
        ]
    );

    fixture.cleanup().await
}

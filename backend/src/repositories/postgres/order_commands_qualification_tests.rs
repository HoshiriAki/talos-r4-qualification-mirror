use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision, TenantId,
    TenantScope,
};

use crate::repositories::{
    DraftOrderPatch, ImportedOrderDraft, PostgresRepositoryProvider, RepositoryProvider,
};

struct LiveOrderCommandsFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveOrderCommandsFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_order_commands_{}", uuid::Uuid::new_v4().simple());
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
                 VALUES ($1,$2,$3,'active','test','2026-09-17T00:00:00Z','2026-09-17T00:00:00Z')",
            )
            .bind(id)
            .bind(name)
            .bind(slug)
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
        ActorIdentity::authenticated(format!("actor-{tenant}"), "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("order-commands-write").unwrap()).unwrap(),
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
            "platform-order-preview",
            AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-order-preview-member").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("order-commands-preview").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("order-commands-preview").unwrap()),
        RequestId::new("order-commands-preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn draft(order_no: &str) -> ImportedOrderDraft {
    ImportedOrderDraft {
        reconciliation_order_no: order_no.into(),
        start_date: "2026-10-01".into(),
        end_date: "2026-10-03".into(),
        delivery_date: "2026-10-01".into(),
        pickup_methods: vec!["delivery".into()],
        address: "initial address".into(),
        notes: "initial notes".into(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_order_commands_preserve_draft_collision_version_scope_and_preview()
-> anyhow::Result<()> {
    let fixture = LiveOrderCommandsFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let tenant_a = provider.bind(&normal_context("tenant-a", "order-command-a"))?;
    let tenant_b = provider.bind(&normal_context("tenant-b", "order-command-b"))?;

    let created = tenant_a
        .order_commands()
        .create_imported_draft(draft("IMPORT-10001"))?;
    assert_eq!(created.status, "draft");

    let row: (String, String, String, i64, String) = sqlx::query_as(
        "SELECT o.tenant_id, o.deviceserialno, l.commercial_status, l.version, o.notes \
         FROM orders o \
         JOIN order_lifecycle l ON l.order_id=o.id AND l.tenant_id=o.tenant_id \
         WHERE o.id=$1",
    )
    .bind(&created.id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(row.0, "tenant-a");
    assert!(row.1.is_empty());
    assert_eq!(row.2, "draft");
    assert_eq!(row.3, 1);
    assert_eq!(row.4, "initial notes");

    let same_tenant_collision = tenant_a
        .order_commands()
        .create_imported_draft(draft("IMPORT-10001"))
        .unwrap_err();
    assert_eq!(
        same_tenant_collision.code(),
        "REPOSITORY_CONTRACT_VIOLATION"
    );
    let cross_tenant_collision = tenant_b
        .order_commands()
        .create_imported_draft(draft("IMPORT-10001"))
        .unwrap_err();
    assert_eq!(
        cross_tenant_collision.code(),
        "REPOSITORY_CONTRACT_VIOLATION"
    );

    tenant_a.order_commands().update_draft(
        &created.id,
        1,
        DraftOrderPatch {
            start_date: Some("2026-10-02".into()),
            end_date: Some("2026-10-05".into()),
            delivery_date: None,
            pickup_methods: Some(vec!["pickup".into()]),
            address: Some("updated address".into()),
            province: Some("Shanghai".into()),
            notes: Some("updated notes".into()),
        },
    )?;

    let updated: (String, String, String, String, String, String) = sqlx::query_as(
        "SELECT startdate,enddate,pickupmethods,address,province,notes \
         FROM orders WHERE tenant_id='tenant-a' AND id=$1",
    )
    .bind(&created.id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(updated.0, "2026-10-02");
    assert_eq!(updated.1, "2026-10-05");
    assert_eq!(updated.2, "[\"pickup\"]");
    assert_eq!(updated.3, "updated address");
    assert_eq!(updated.4, "Shanghai");
    assert_eq!(updated.5, "updated notes");

    let stale = tenant_a
        .order_commands()
        .update_draft(
            &created.id,
            2,
            DraftOrderPatch {
                notes: Some("stale notes".into()),
                ..DraftOrderPatch::default()
            },
        )
        .unwrap_err();
    assert_eq!(stale.code(), "REPOSITORY_CONTRACT_VIOLATION");

    let cross_tenant = tenant_b
        .order_commands()
        .update_draft(
            &created.id,
            1,
            DraftOrderPatch {
                notes: Some("forbidden tenant write".into()),
                ..DraftOrderPatch::default()
            },
        )
        .unwrap_err();
    assert_eq!(cross_tenant.code(), "REPOSITORY_CONTRACT_VIOLATION");

    let preview = provider.bind(&preview_context("tenant-a"))?;
    let preview_error = preview
        .order_commands()
        .update_draft(
            &created.id,
            1,
            DraftOrderPatch {
                notes: Some("forbidden preview write".into()),
                ..DraftOrderPatch::default()
            },
        )
        .unwrap_err();
    assert_eq!(preview_error.code(), "REPOSITORY_PREVIEW_WRITE_DENIED");

    tenant_a.lifecycles().apply_action(
        &created.id,
        "submit_order",
        1,
        "actor-tenant-a",
        "make order non-draft",
    )?;
    let non_draft = tenant_a
        .order_commands()
        .update_draft(
            &created.id,
            2,
            DraftOrderPatch {
                notes: Some("forbidden after submit".into()),
                ..DraftOrderPatch::default()
            },
        )
        .unwrap_err();
    assert_eq!(non_draft.code(), "REPOSITORY_CONTRACT_VIOLATION");

    let final_notes: String =
        sqlx::query_scalar("SELECT notes FROM orders WHERE tenant_id='tenant-a' AND id=$1")
            .bind(&created.id)
            .fetch_one(&fixture.pool)
            .await?;
    assert_eq!(final_notes, "updated notes");

    fixture.cleanup().await?;
    Ok(())
}

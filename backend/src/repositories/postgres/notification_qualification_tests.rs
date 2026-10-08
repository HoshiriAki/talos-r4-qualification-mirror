use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{
    NotificationMutationError, PostgresRepositoryProvider, RepositoryProvider,
};

struct LiveNotificationFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveNotificationFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_notify_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-notify-a','Notify A','tenant-notify-a','active','test','now','now'),
             ('tenant-notify-b','Notify B','tenant-notify-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO customers
             (id,tenant_id,legal_name,display_name,status,risk_status,version,created_at,updated_at)
             VALUES
             ('notify-customer-a','tenant-notify-a','Customer A','Customer A','active','clear',1,'now','now'),
             ('notify-customer-b','tenant-notify-b','Customer B','Customer B','active','clear',1,'now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,
              deviceserialno,createdat,status,customer_id,tenant_id)
             VALUES
             ('notify-order-a','NOTIFY-A','2026-10-01','2026-10-10','2026-10-01',
              'pickup','','','DEVICE-A','now','draft','notify-customer-a','tenant-notify-a'),
             ('notify-order-b','NOTIFY-B','2026-10-01','2026-10-11','2026-10-01',
              'pickup','','','DEVICE-B','now','draft','notify-customer-b','tenant-notify-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO notification_templates
             (id,event_type,channel,subject_template,body_template,is_enabled,created_at,updated_at,tenant_id)
             VALUES
             ('notify-tpl-a','shipped','in_app','Tenant A','Order {orderNo}',1,'now','now','tenant-notify-a'),
             ('notify-tpl-b','shipped','in_app','Tenant B','Private {orderNo}',1,'now','now','tenant-notify-b')",
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
        ActorIdentity::authenticated("notify-test-user", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("notify-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_notification_authority_preserves_scope_templates_messages_order_hydration_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveNotificationFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-notify-a", "notify-a");
    let ctx_b = context("tenant-notify-b", "notify-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let template = scoped_a
            .notifications()
            .template("shipped", "in_app")?
            .expect("tenant A template");
        assert_eq!(template.id, "notify-tpl-a");
        assert_eq!(
            scoped_a.notifications().enabled_channels("shipped")?,
            vec!["in_app"]
        );

        let logged = scoped_a.notifications().insert_log(
            "notify-tpl-a",
            "shipped",
            "in_app",
            "notify-test-user",
            "Tenant A",
            "Order NOTIFY-A",
            "2026-10-01T18:10:00+08:00",
        )?;

        assert_eq!(
            scoped_a
                .notifications()
                .list_in_app("notify-test-user", 1, 20)?
                .total,
            1
        );
        assert_eq!(
            scoped_b
                .notifications()
                .list_in_app("notify-test-user", 1, 20)?
                .total,
            0
        );

        assert_eq!(
            scoped_b.notifications().mark_read(
                Some(&logged.message_id),
                "notify-test-user",
                "2026-10-01T18:11:00+08:00",
            )?,
            0
        );
        assert_eq!(
            scoped_a.notifications().mark_read(
                Some(&logged.message_id),
                "notify-test-user",
                "2026-10-01T18:12:00+08:00",
            )?,
            1
        );

        let order = scoped_a
            .notifications()
            .order_context("notify-order-a")?
            .expect("tenant A order");
        assert_eq!(order.order_no, "NOTIFY-A");
        assert_eq!(order.customer_name, "Customer A");
        assert_eq!(order.due_date, "2026-10-10");
        assert!(
            scoped_a
                .notifications()
                .order_context("notify-order-b")?
                .is_none()
        );

        let cross_tenant_update = scoped_b.notifications().upsert_template(
            Some("notify-tpl-a"),
            "shipped",
            "in_app",
            "mutated",
            "mutated",
            true,
            "2026-10-01T18:13:00+08:00",
        );
        assert!(matches!(
            cross_tenant_update,
            Err(NotificationMutationError::TemplateNotFound)
        ));

        let created = scoped_a.notifications().upsert_template(
            None,
            "return_reminder",
            "sms",
            "",
            "Return {orderNo}",
            true,
            "2026-10-01T18:14:00+08:00",
        )?;
        assert!(created.created);
        assert_eq!(scoped_a.notifications().list_templates()?.len(), 2);
        assert_eq!(scoped_b.notifications().list_templates()?.len(), 1);
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-notify-a", "notify-recomposed"))?;
    let persisted = scoped_after
        .notifications()
        .list_in_app("notify-test-user", 1, 20)?;
    assert_eq!(persisted.total, 1);
    assert_eq!(persisted.unread_count, 0);
    assert_eq!(persisted.messages[0].body, "Order NOTIFY-A");

    fixture.cleanup().await
}

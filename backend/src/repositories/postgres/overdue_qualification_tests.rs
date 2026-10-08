use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor, Row};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::OverdueFeeConfigPatch;
use crate::repositories::{PostgresRepositoryProvider, RepositoryProvider};

struct LiveOverdueFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveOverdueFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_overdue_{}", uuid::Uuid::new_v4().simple());
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

        sqlx::query(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)
             VALUES
             ('tenant-overdue-a','Overdue A','tenant-overdue-a','active','test','now','now'),
             ('tenant-overdue-b','Overdue B','tenant-overdue-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES
             ('identity-overdue-admin','overdue-admin','hash','Overdue Admin','active','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO customers
             (id,tenant_id,legal_name,display_name,status,risk_status,version,created_at,updated_at)
             VALUES
             ('customer-overdue-a','tenant-overdue-a','Customer A','Customer A','active','clear',1,'now','now'),
             ('customer-overdue-b','tenant-overdue-b','Customer B','Customer B','active','clear',1,'now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO customer_contacts
             (id,tenant_id,customer_id,kind,raw_value,normalized_value,is_primary,
              classification,purpose,created_at,updated_at)
             VALUES
             ('contact-overdue-a','tenant-overdue-a','customer-overdue-a','phone',
              '13800000000','13800000000',TRUE,'pii','rental_contact','now','now'),
             ('contact-overdue-b','tenant-overdue-b','customer-overdue-b','phone',
              '13800000000','13800000000',TRUE,'pii','rental_contact','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,
              deviceserialno,createdat,status,tenant_id,customer_id)
             VALUES
             ('overdue-order-a','OVD-A','2026-09-01','2026-09-20','2026-09-01','[]','','',
              '','now','in_use','tenant-overdue-a','customer-overdue-a'),
             ('overdue-order-b','OVD-B','2026-09-01','2026-09-28','2026-09-01','[]','','',
              '','now','shipped','tenant-overdue-b','customer-overdue-b')",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "UPDATE order_lifecycle
                SET commercial_status='confirmed',fulfilment_status='in_use'
              WHERE tenant_id='tenant-overdue-a' AND order_id='overdue-order-a'",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "UPDATE order_lifecycle
                SET commercial_status='confirmed',fulfilment_status='return_pending'
              WHERE tenant_id='tenant-overdue-b' AND order_id='overdue-order-b'",
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
        ActorIdentity::authenticated("overdue-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("overdue-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_overdue_authority_preserves_scope_identity_escalation_delegation_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveOverdueFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-overdue-a", "overdue-a");
    let ctx_b = context("tenant-overdue-b", "overdue-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        scoped_a.overdues().config_upsert(
            OverdueFeeConfigPatch {
                daily_rate: Some(50.0),
                max_days: Some(30),
                cap_multiplier: Some(3.0),
                grace_period_hours: Some(0),
            },
            "2026-09-30T21:00:00+08:00",
        )?;
        scoped_b.overdues().config_upsert(
            OverdueFeeConfigPatch {
                daily_rate: Some(10.0),
                max_days: Some(30),
                cap_multiplier: Some(3.0),
                grace_period_hours: Some(4),
            },
            "2026-09-30T21:00:00+08:00",
        )?;

        let detected_a = scoped_a
            .overdues()
            .detect("2026-09-30", "2026-09-30T21:01:00+08:00")?;
        let detected_b = scoped_b
            .overdues()
            .detect("2026-09-30", "2026-09-30T21:01:00+08:00")?;
        assert_eq!(detected_a["detected"], 1);
        assert_eq!(detected_b["detected"], 1);
        assert_eq!(detected_a["records"][0]["orderId"], "overdue-order-a");
        assert_eq!(detected_b["records"][0]["orderId"], "overdue-order-b");
        assert_eq!(detected_a["records"][0]["totalFee"], 500.0);
        assert_eq!(detected_b["records"][0]["totalFee"], 20.0);

        let list_a = scoped_a.overdues().list(None, None, None, 1, 20)?;
        let list_b = scoped_b.overdues().list(None, None, None, 1, 20)?;
        assert_eq!(list_a["total"], 1);
        assert_eq!(list_b["total"], 1);
        assert_eq!(list_a["items"][0]["status"], "escalated_d7");
        assert_eq!(list_b["items"][0]["status"], "active");

        let overdue_id = list_a["items"][0]["id"].as_i64().unwrap();
        assert_eq!(scoped_a.overdues().escalation_history(overdue_id)?.len(), 1);

        scoped_a
            .overdues()
            .detect("2026-09-30", "2026-09-30T21:02:00+08:00")?;
        assert_eq!(
            scoped_a.overdues().list(None, None, None, 1, 20)?["total"],
            1
        );
        assert_eq!(scoped_a.overdues().escalation_history(overdue_id)?.len(), 1);

        let delegated = scoped_a.overdues().apply_delegated(overdue_id)?;
        assert_eq!(delegated["requestedAmount"], 500.0);
        assert_eq!(delegated["appliedAmount"], 0.0);
        assert_eq!(delegated["financialEffectApplied"], false);
        assert_eq!(delegated["settlementAuthority"], "r3_settlement");

        let price_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint
             FROM order_price_details AS price
             JOIN orders AS scoped_order ON scoped_order.id=price.orderid
             WHERE scoped_order.tenant_id='tenant-overdue-a'
               AND scoped_order.id='overdue-order-a'",
        )
        .fetch_one(&fixture.pool)
        .await?;
        assert_eq!(price_rows, 0);

        let duplicate_config = sqlx::query(
            "INSERT INTO overdue_fee_config
             (tenant_id,daily_rate,max_days,cap_multiplier,grace_period_hours,created_at,updated_at)
             VALUES ('tenant-overdue-a',1,1,1,1,'now','now')",
        )
        .execute(&fixture.pool)
        .await;
        assert!(duplicate_config.is_err());

        let duplicate_notification = sqlx::query(
            "INSERT INTO overdue_notification_log
             (tenant_id,overdue_id,escalation_level,channel,sent_at)
             VALUES ('tenant-overdue-a',$1,3,'sms','now')",
        )
        .bind(overdue_id)
        .execute(&fixture.pool)
        .await;
        assert!(duplicate_notification.is_err());

        let check = scoped_a.overdues().check_before_order("13800000000")?;
        assert_eq!(check["allowed"], false);

        scoped_a.overdues().waive(
            overdue_id,
            "manual exception",
            "identity-overdue-admin",
            "2026-09-30T21:03:00+08:00",
        )?;
        assert_eq!(
            scoped_a.overdues().get(overdue_id)?.unwrap()["waivedBy"],
            "identity-overdue-admin"
        );
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-overdue-a", "overdue-recomposed"))?;
    let persisted =
        scoped_after
            .overdues()
            .list(Some("waived"), None, Some("overdue-order-a"), 1, 20)?;
    assert_eq!(persisted["total"], 1);
    assert_eq!(persisted["items"][0]["orderId"], "overdue-order-a");

    let migration_079: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM schema_migrations
          WHERE id='079_r4_overdue_tenant_invariant'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(migration_079, 1);

    fixture.cleanup().await
}

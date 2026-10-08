use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{
    LegacyReservationRulePatch, PostgresRepositoryProvider, RepositoryProvider,
};

struct LiveLegacyReservationFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveLegacyReservationFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_legacy_reservation_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-reservation-a','Reservation A','tenant-reservation-a','active','test','now','now'),
             ('tenant-reservation-b','Reservation B','tenant-reservation-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO identities
             (id,username,password_hash,display_name,status,created_at,updated_at)
             VALUES
             ('identity-reservation-a','reservation-a','hash','Reservation A','active','now','now'),
             ('identity-reservation-b','reservation-b','hash','Reservation B','active','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO inventory_reservations
             (device_serial_no,warehouse_id,order_id,customer_name,customer_phone,
              start_date,end_date,status,notes,reserved_by,created_at,updated_at,tenant_id)
             VALUES
             ('LEG-A-1',NULL,NULL,'Alice','13800000000',
              '2026-10-10','2026-10-12','reserved','legacy',
              'identity-reservation-a','2026-10-02T10:00:00+08:00','2026-10-02T10:00:00+08:00','tenant-reservation-a'),
             ('LEG-B-1',NULL,NULL,'Bob','13900000000',
              '2026-10-10','2026-10-12','confirmed',NULL,
              'identity-reservation-b','2026-10-02T10:00:00+08:00','2026-10-02T10:00:00+08:00','tenant-reservation-b')",
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
        ActorIdentity::authenticated("reservation-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("reservation-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_legacy_reservation_compatibility_preserves_scope_conflicts_rules_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveLegacyReservationFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-reservation-a", "reservation-a");
    let ctx_b = context("tenant-reservation-b", "reservation-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let a = scoped_a
            .legacy_reservations()
            .list(None, None, None, None, 1, 20)?;
        let b = scoped_b
            .legacy_reservations()
            .list(None, None, None, None, 1, 20)?;
        assert_eq!(a.total, 1);
        assert_eq!(b.total, 1);
        assert_eq!(a.items[0].device_serial_no, "LEG-A-1");
        assert_eq!(a.items[0].reserved_by, "identity-reservation-a");
        assert!(scoped_a.legacy_reservations().get(b.items[0].id)?.is_none());

        let overlap =
            scoped_a
                .legacy_reservations()
                .conflicts("LEG-A-1", "2026-10-11", "2026-10-13")?;
        assert_eq!(overlap.len(), 1);
        let adjacent =
            scoped_a
                .legacy_reservations()
                .conflicts("LEG-A-1", "2026-10-12", "2026-10-13")?;
        assert!(adjacent.is_empty());

        assert!(scoped_a.legacy_reservations().get_rule()?.is_none());
        assert!(scoped_b.legacy_reservations().get_rule()?.is_none());

        let updated_a = scoped_a.legacy_reservations().update_rule(
            &LegacyReservationRulePatch {
                max_concurrent_per_customer: Some(7),
                auto_release_minutes: Some(45),
                ..Default::default()
            },
            "2026-10-02T10:11:00+08:00",
        )?;
        assert_eq!(updated_a.max_concurrent_per_customer, 7);
        assert_eq!(updated_a.auto_release_minutes, 45);
        assert!(scoped_b.legacy_reservations().get_rule()?.is_none());

        let updated_b = scoped_b.legacy_reservations().update_rule(
            &LegacyReservationRulePatch {
                max_concurrent_per_customer: Some(3),
                ..Default::default()
            },
            "2026-10-02T10:12:00+08:00",
        )?;
        assert_eq!(updated_b.max_concurrent_per_customer, 3);
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after =
        recomposed.bind(&context("tenant-reservation-a", "reservation-recomposed"))?;
    let persisted =
        scoped_after
            .legacy_reservations()
            .list(None, Some("LEG-A-1"), None, None, 1, 20)?;
    assert_eq!(persisted.total, 1);
    assert_eq!(persisted.items[0].reserved_by, "identity-reservation-a");
    let rule = scoped_after
        .legacy_reservations()
        .get_rule()?
        .expect("tenant A rule persists");
    assert_eq!(rule.max_concurrent_per_customer, 7);
    assert_eq!(rule.auto_release_minutes, 45);

    fixture.cleanup().await
}

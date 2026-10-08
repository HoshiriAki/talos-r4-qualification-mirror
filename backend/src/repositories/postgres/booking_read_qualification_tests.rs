use std::sync::Arc;

use chrono::NaiveDate;
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{NewModel, PostgresRepositoryProvider, RepositoryProvider};

struct LiveBookingFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveBookingFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_booking_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-booking-a','Booking A','tenant-booking-a','active','test','now','now'),
             ('tenant-booking-b','Booking B','tenant-booking-b','active','test','now','now')",
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
        ActorIdentity::authenticated("booking-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("booking-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_booking_reads_preserve_scope_availability_pricing_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveBookingFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-booking-a", "booking-a");
    let ctx_b = context("tenant-booking-b", "booking-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        scoped_a.models().create(&NewModel {
            id: "booking-model-a".into(),
            name: "Camera A".into(),
            category: "camera".into(),
            prefix: "BA".into(),
            enabled: true,
            weekday_price: Some(10.0),
            weekend_price: Some(20.0),
            updated_by: Some("booking-actor".into()),
            now: "2026-10-02T15:00:00+08:00".into(),
        })?;
        scoped_b.models().create(&NewModel {
            id: "booking-model-b".into(),
            name: "Camera B".into(),
            category: "camera".into(),
            prefix: "BB".into(),
            enabled: true,
            weekday_price: Some(30.0),
            weekend_price: Some(40.0),
            updated_by: Some("booking-actor".into()),
            now: "2026-10-02T15:00:00+08:00".into(),
        })?;

        sqlx::query(
            "INSERT INTO devices (id,serialno,modelid,rentalstatus,createdat,tenant_id)
             VALUES
             ('booking-device-a','BOOK-A-1','booking-model-a','available','now','tenant-booking-a'),
             ('booking-device-b','BOOK-B-1','booking-model-b','available','now','tenant-booking-b')",
        )
        .execute(&fixture.pool)
        .await?;

        sqlx::query(
            "INSERT INTO booking_availability
             (device_serial_no,date,is_available,tenant_id)
             VALUES
             ('BOOK-A-1','2026-10-10',0,'tenant-booking-a'),
             ('BOOK-B-1','2026-10-10',0,'tenant-booking-b')",
        )
        .execute(&fixture.pool)
        .await?;

        let start = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();

        let a = scoped_a.booking_reads().availability(None, start, end)?;
        assert_eq!(a.devices.len(), 1);
        assert_eq!(a.devices[0].serial_no, "BOOK-A-1");
        assert_eq!(a.dates.len(), 2);
        assert!(!a.dates[0].available);
        assert!(a.dates[1].available);

        let b = scoped_b.booking_reads().availability(None, start, end)?;
        assert_eq!(b.devices.len(), 1);
        assert_eq!(b.devices[0].serial_no, "BOOK-B-1");

        let price = scoped_a
            .booking_reads()
            .price("BOOK-A-1")?
            .expect("tenant A price");
        assert_eq!(price.model_name, "Camera A");
        assert_eq!(price.weekday_price, 10.0);
        assert_eq!(price.weekend_price, 20.0);

        let search = scoped_a
            .booking_reads()
            .search(Some("Camera"), Some("booking-model-a"))?;
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].serial_no, "BOOK-A-1");
        assert_eq!(search[0].daily_rate, 15.0);

        assert!(scoped_a.booking_reads().price("BOOK-B-1")?.is_none());
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-booking-a", "booking-recomposed"))?;
    let price_after = scoped_after
        .booking_reads()
        .price("BOOK-A-1")?
        .expect("recomposed booking price");
    assert_eq!(price_after.weekday_price, 10.0);
    assert_eq!(price_after.weekend_price, 20.0);

    fixture.cleanup().await
}

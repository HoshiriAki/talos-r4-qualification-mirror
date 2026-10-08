use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use crate::repositories::ReportCompatibilityRepository;

struct LiveReportFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveReportFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_report_{}", uuid::Uuid::new_v4().simple());
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
            "INSERT INTO tenants (id, name, slug, status, plan, created_at, updated_at)
             VALUES
             ('tenant-report-a','Report A','tenant-report-a','active','free','now','now'),
             ('tenant-report-b','Report B','tenant-report-b','active','free','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id, orderno, startdate, enddate, deliverydate, pickupmethods,
              address, notes, deviceserialno, totalprice, province, status, createdat, tenant_id)
             VALUES
             ('report-a-1','R-A-1','2026-09-01','2026-09-02','2026-09-01','[]',
              'A','','',100,'Alpha','paid','2026-09-01T08:00:00+08:00','tenant-report-a'),
             ('report-a-2','R-A-2','2026-09-15','2026-09-16','2026-09-15','[]',
              'B','','',300,'Beta','paid','2026-09-15T08:00:00+08:00','tenant-report-a'),
             ('report-b-1','R-B-1','2026-09-10','2026-09-11','2026-09-10','[]',
              'C','','',999,'Gamma','paid','2026-09-10T08:00:00+08:00','tenant-report-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices
             (id, serialno, rentalstatus, createdat, tenant_id)
             VALUES
             ('report-device-a','REPORT-A','rented','2026-09-01T08:00:00+08:00','tenant-report-a'),
             ('report-device-b','REPORT-B','rented','2026-09-15T08:00:00+08:00','tenant-report-a'),
             ('report-device-x','REPORT-X','rented','2026-09-10T08:00:00+08:00','tenant-report-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO order_devices (id, orderid, serialno, createdat, tenant_id)
             VALUES
             ('report-link-a','report-a-1','REPORT-A','2026-09-01T08:00:00+08:00','tenant-report-a'),
             ('report-link-b','report-a-2','REPORT-B','2026-09-15T08:00:00+08:00','tenant-report-a'),
             ('report-link-x','report-b-1','REPORT-X','2026-09-10T08:00:00+08:00','tenant-report-b')",
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_report_compatibility_preserves_tenant_scope_and_aggregates() -> anyhow::Result<()>
{
    let fixture = LiveReportFixture::create().await?;
    let repository = ReportCompatibilityRepository::postgres(fixture.pool.clone());

    let full = repository.revenue_data("tenant-report-a", "", "")?;
    assert_eq!(full.summary.total_orders, 2);
    assert!((full.summary.total_revenue - 400.0).abs() < f64::EPSILON);
    assert!((full.summary.avg_order_value - 200.0).abs() < f64::EPSILON);
    assert_eq!(full.summary.devices_rented, 2);
    assert_eq!(full.by_province.len(), 2);
    assert_eq!(full.by_month.len(), 1);
    assert_eq!(full.by_month[0].month, "2026-09");
    assert_eq!(full.by_month[0].order_count, 2);

    let filtered = repository.revenue_data("tenant-report-a", "2026-09-10", "2026-09-30")?;
    assert_eq!(filtered.summary.total_orders, 1);
    assert!((filtered.summary.total_revenue - 300.0).abs() < f64::EPSILON);
    assert_eq!(filtered.summary.devices_rented, 1);

    let foreign = repository.revenue_data("tenant-report-b", "", "")?;
    assert_eq!(foreign.summary.total_orders, 1);
    assert!((foreign.summary.total_revenue - 999.0).abs() < f64::EPSILON);

    fixture.cleanup().await
}

use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{PostgresRepositoryProvider, RepositoryProvider};

struct LiveDashboardFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveDashboardFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_dashboard_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-dashboard-a','Dashboard A','tenant-dashboard-a','active','test','now','now'),
             ('tenant-dashboard-b','Dashboard B','tenant-dashboard-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO device_models
             (id,name,category,prefix,enabled,createdat,updatedat,tenant_id)
             VALUES
             ('dashboard-model-a','Dashboard Model A','camera','DA',true,'now','now','tenant-dashboard-a'),
             ('dashboard-model-b','Dashboard Model B','camera','DB',true,'now','now','tenant-dashboard-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO warehouses
             (id,name,type,enabled,createdat,updatedat,tenant_id)
             VALUES
             ('dashboard-warehouse-a','Dashboard Warehouse A','owned',true,'now','now','tenant-dashboard-a'),
             ('dashboard-warehouse-b','Dashboard Warehouse B','owned',true,'now','now','tenant-dashboard-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices
             (id,serialno,rentalstatus,notes,modelid,currentwarehouseid,createdat,tenant_id)
             VALUES
             ('dashboard-device-a1','DASH-SHARED','租赁中','A rented','dashboard-model-a','dashboard-warehouse-a','2026-09-20T08:00:00+08:00','tenant-dashboard-a'),
             ('dashboard-device-a2','DASH-A-OVERDUE','已入库','A stock','dashboard-model-a','dashboard-warehouse-a','2026-09-01T08:00:00+08:00','tenant-dashboard-a'),
             ('dashboard-device-b1','DASH-SHARED','租赁中','B rented','dashboard-model-b','dashboard-warehouse-b','2026-09-20T08:00:00+08:00','tenant-dashboard-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,
              totalprice,province,sendwarehouseid,status,createdat,tenant_id)
             VALUES
             ('dashboard-order-a1','DASH-A-1','2026-09-25','2026-09-27','2026-09-25','[]','','',
              100,'Alpha','dashboard-warehouse-a','active','2026-09-25T08:00:00+08:00','tenant-dashboard-a'),
             ('dashboard-order-a2','DASH-A-2','2026-09-01','2026-09-20','2026-09-01','[]','','',
              50,'Alpha','dashboard-warehouse-a','active','2026-09-20T08:00:00+08:00','tenant-dashboard-a'),
             ('dashboard-order-b1','DASH-B-1','2026-09-25','2026-09-27','2026-09-25','[]','','',
              999,'Beta','dashboard-warehouse-b','active','2026-09-25T08:00:00+08:00','tenant-dashboard-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO order_devices (id,orderid,serialno,createdat,tenant_id)
             VALUES
             ('dashboard-link-a1','dashboard-order-a1','DASH-SHARED','2026-09-25T08:00:00+08:00','tenant-dashboard-a'),
             ('dashboard-link-a2','dashboard-order-a2','DASH-A-OVERDUE','2026-09-01T08:00:00+08:00','tenant-dashboard-a'),
             ('dashboard-link-b1','dashboard-order-b1','DASH-SHARED','2026-09-25T08:00:00+08:00','tenant-dashboard-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO audit_events
             (id,authority_kind,tenant_id,action,resource_type,resource_id,correlation_id,occurred_at)
             VALUES
             ('dashboard-audit-a','system','tenant-dashboard-a','order_delete','order','deleted-a','dashboard-audit-a','2026-09-26T08:00:00+08:00'),
             ('dashboard-audit-b','system','tenant-dashboard-b','order_delete','order','deleted-b','dashboard-audit-b','2026-09-26T08:00:00+08:00')",
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
        ActorIdentity::authenticated("dashboard-authority-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(
            tenant_id,
            Revision::new("dashboard-authority-revision").unwrap(),
        )
        .unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_dashboard_reads_are_tenant_scoped_across_all_aggregate_families()
-> anyhow::Result<()> {
    let fixture = LiveDashboardFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_a = provider.bind(&context("tenant-dashboard-a", "dashboard-request-a"))?;
    let scoped_b = provider.bind(&context("tenant-dashboard-b", "dashboard-request-b"))?;

    let overview_a = scoped_a.dashboards().overview("2026-09-27", "2026-09-23")?;
    assert_eq!(overview_a.active_orders, 1);
    assert_eq!(overview_a.devices_out, 1);
    assert_eq!(overview_a.total_devices, 2);
    assert_eq!(overview_a.returns_due_today, 1);
    assert_eq!(overview_a.overdue_returns, 1);
    assert_eq!(overview_a.recent_orders.len(), 2);
    assert!(
        overview_a
            .recent_orders
            .iter()
            .all(|order| order.order_no.starts_with("DASH-A-"))
    );

    let overview_b = scoped_b.dashboards().overview("2026-09-27", "2026-09-23")?;
    assert_eq!(overview_b.active_orders, 1);
    assert_eq!(overview_b.total_devices, 1);
    assert!(
        overview_b
            .recent_orders
            .iter()
            .all(|order| order.order_no.starts_with("DASH-B-"))
    );

    let windows_a = scoped_a
        .dashboards()
        .order_windows("2026-09-27", "2026-09-29")?;
    assert_eq!(windows_a.len(), 1);

    let order_daily_a = scoped_a.dashboards().order_daily("2026-09-01")?;
    assert_eq!(
        order_daily_a.iter().map(|point| point.count).sum::<i64>(),
        2
    );
    let revenue_a = scoped_a.dashboards().revenue_daily("2026-09-01")?;
    assert!(
        (revenue_a
            .iter()
            .map(|point| point.total_revenue)
            .sum::<f64>()
            - 150.0)
            .abs()
            < f64::EPSILON
    );

    let revenue_b = scoped_b.dashboards().revenue_daily("2026-09-01")?;
    assert!(
        (revenue_b
            .iter()
            .map(|point| point.total_revenue)
            .sum::<f64>()
            - 999.0)
            .abs()
            < f64::EPSILON
    );

    let cancels_a = scoped_a.dashboards().cancel_daily("2026-09-01")?;
    assert_eq!(cancels_a.iter().map(|point| point.count).sum::<i64>(), 1);
    let cancels_b = scoped_b.dashboards().cancel_daily("2026-09-01")?;
    assert_eq!(cancels_b.iter().map(|point| point.count).sum::<i64>(), 1);

    let status_a = scoped_a.dashboards().device_status_distribution()?;
    assert_eq!(status_a.iter().map(|bucket| bucket.count).sum::<i64>(), 2);
    let status_b = scoped_b.dashboards().device_status_distribution()?;
    assert_eq!(status_b.iter().map(|bucket| bucket.count).sum::<i64>(), 1);

    let province_a = scoped_a.dashboards().province_pie("2026-09-01")?;
    assert_eq!(province_a.len(), 1);
    assert_eq!(province_a[0].name, "Alpha");
    assert_eq!(province_a[0].value, 2);
    let province_b = scoped_b.dashboards().province_pie("2026-09-01")?;
    assert_eq!(province_b[0].name, "Beta");

    let province_trend_a = scoped_a.dashboards().province_trend("2026-09-01")?;
    assert!(
        province_trend_a
            .iter()
            .all(|point| point.province == "Alpha")
    );

    let ranking_a = scoped_a.dashboards().model_ranking("2026-09-01", 10)?;
    assert_eq!(ranking_a.len(), 1);
    assert_eq!(ranking_a[0].model_name, "Dashboard Model A");
    assert!((ranking_a[0].revenue - 150.0).abs() < f64::EPSILON);
    let ranking_b = scoped_b.dashboards().model_ranking("2026-09-01", 10)?;
    assert_eq!(ranking_b[0].model_name, "Dashboard Model B");
    assert!((ranking_b[0].revenue - 999.0).abs() < f64::EPSILON);

    let warehouse_a = scoped_a.dashboards().warehouse_stats()?;
    assert_eq!(warehouse_a.len(), 1);
    assert_eq!(warehouse_a[0].warehouse_id, "dashboard-warehouse-a");
    assert_eq!(warehouse_a[0].outgoing, 2);
    assert_eq!(warehouse_a[0].current_stock, 1);
    let warehouse_b = scoped_b.dashboards().warehouse_stats()?;
    assert_eq!(warehouse_b[0].warehouse_id, "dashboard-warehouse-b");
    assert_eq!(warehouse_b[0].outgoing, 1);

    fixture.cleanup().await
}

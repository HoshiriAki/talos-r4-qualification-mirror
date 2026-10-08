use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor, Row};

use crate::repositories::MaintenanceCompatibilityRepository;

struct LiveMaintenanceFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveMaintenanceFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_maintenance_{}", uuid::Uuid::new_v4().simple());
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

async fn insert_order(
    pool: &PgPool,
    id: &str,
    order_no: &str,
    tenant_id: &str,
    end_date: &str,
    delivery_date: &str,
    status: &str,
    serial_no: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO orders
         (id,orderNo,startDate,endDate,deliveryDate,pickupMethods,address,notes,
          deviceSerialNo,createdAt,status,tenant_id)
         VALUES ($1,$2,'2026-09-01',$3,$4,'[]','','',$5,'2026-09-01T00:00:00Z',$6,$7)",
    )
    .bind(id)
    .bind(order_no)
    .bind(end_date)
    .bind(delivery_date)
    .bind(serial_no)
    .bind(status)
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_maintenance_compatibility_preserves_status_sync_overdue_tenant_and_idempotency()
-> anyhow::Result<()> {
    let fixture = LiveMaintenanceFixture::create().await?;
    sqlx::query(
        "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at)
         VALUES
           ('tenant-a','Tenant A','tenant-a','active','test','2026-09-01T00:00:00Z','2026-09-01T00:00:00Z'),
           ('tenant-b','Tenant B','tenant-b','active','test','2026-09-01T00:00:00Z','2026-09-01T00:00:00Z')",
    )
    .execute(&fixture.pool)
    .await?;

    insert_order(
        &fixture.pool,
        "order-future",
        "1001",
        "tenant-a",
        "2026-09-30",
        "2026-09-25",
        "active",
        "",
    )
    .await?;
    insert_order(
        &fixture.pool,
        "order-due",
        "1002",
        "tenant-a",
        "2026-09-30",
        "2026-09-19",
        "reserved",
        "",
    )
    .await?;
    insert_order(
        &fixture.pool,
        "order-overdue-a",
        "1003",
        "tenant-a",
        "2026-09-10",
        "2026-09-01",
        "active",
        "SERIAL-A",
    )
    .await?;
    insert_order(
        &fixture.pool,
        "order-overdue-b",
        "1004",
        "tenant-b",
        "2026-09-14",
        "2026-09-01",
        "in_use",
        "SERIAL-B",
    )
    .await?;

    let repository = MaintenanceCompatibilityRepository::postgres(fixture.pool.clone());
    let synced = repository.sync_order_statuses("2026-09-20")?;
    assert_eq!(synced.reserved, 1);
    assert_eq!(synced.activated, 1);

    let future_status: String =
        sqlx::query_scalar("SELECT status FROM orders WHERE id='order-future'")
            .fetch_one(&fixture.pool)
            .await?;
    let due_status: String = sqlx::query_scalar("SELECT status FROM orders WHERE id='order-due'")
        .fetch_one(&fixture.pool)
        .await?;
    assert_eq!(future_status, "reserved");
    assert_eq!(due_status, "active");

    assert_eq!(
        repository.seed_overdue_tasks("2026-09-20", "2026-09-20T18:00:00+08:00",)?,
        2
    );
    assert_eq!(
        repository.seed_overdue_tasks("2026-09-20", "2026-09-20T18:01:00+08:00",)?,
        0
    );

    let rows = sqlx::query(
        "SELECT id,tenant_id,risk,source_id,capabilities_json,work_route_json
         FROM work_tasks
         WHERE kind='overdue-device'
         ORDER BY id",
    )
    .fetch_all(&fixture.pool)
    .await?;
    assert_eq!(rows.len(), 2);

    let row_a = rows
        .iter()
        .find(|row| row.get::<String, _>(0) == "overdue-device-order-overdue-a-SERIAL-A")
        .expect("tenant-a overdue task");
    assert_eq!(row_a.get::<String, _>(1), "tenant-a");
    assert_eq!(row_a.get::<String, _>(2), "high");
    assert_eq!(row_a.get::<String, _>(3), "SERIAL-A");
    assert_eq!(
        row_a.get::<String, _>(4),
        r#"["defer","open_work","send_to_pc"]"#
    );
    assert!(
        row_a
            .get::<String, _>(5)
            .contains(r#""orderId":"order-overdue-a""#)
    );

    let row_b = rows
        .iter()
        .find(|row| row.get::<String, _>(0) == "overdue-device-order-overdue-b-SERIAL-B")
        .expect("tenant-b overdue task");
    assert_eq!(row_b.get::<String, _>(1), "tenant-b");
    assert_eq!(row_b.get::<String, _>(2), "medium");
    assert_eq!(
        row_b.get::<String, _>(4),
        r#"["acknowledge","defer","open_work","send_to_pc"]"#
    );

    let recomposed = MaintenanceCompatibilityRepository::postgres(fixture.pool.clone());
    assert_eq!(
        recomposed.seed_overdue_tasks("2026-09-20", "2026-09-20T18:02:00+08:00",)?,
        0
    );

    fixture.cleanup().await
}

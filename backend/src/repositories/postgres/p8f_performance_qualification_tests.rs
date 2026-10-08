use std::sync::Arc;
use std::time::{Duration, Instant};

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor, Row};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{
    ImportedOrderDraft, OrderListRequest, PostgresRepositoryProvider, RepositoryProvider,
};

const PERF_ORDER_COUNT: i64 = 500;
const READ_SAMPLES: usize = 30;
const WRITE_SAMPLES: usize = 15;
const ORDER_LIST_P95_LIMIT: Duration = Duration::from_millis(500);
const IMPORTED_DRAFT_P95_LIMIT: Duration = Duration::from_millis(1_000);

struct LivePerformanceFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LivePerformanceFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_perf_{}", uuid::Uuid::new_v4().simple());
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
             VALUES ('tenant-perf','Performance Tenant','tenant-perf','active','test',
                     '2026-10-05T00:00:00Z','2026-10-05T00:00:00Z')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,
              deviceserialno,createdat,tenant_id,totalprice,province,sendwarehouseid,
              returnwarehouseid,accessories,status,trackingno,devicemodels)
             SELECT
               'perf-order-' || g::text,
               'PERF-' || lpad(g::text, 6, '0'),
               '2026-10-06',
               '2026-10-08',
               '2026-10-06',
               '[]',
               'Performance Street ' || g::text,
               '',
               '',
               '2026-10-05T00:00:00Z',
               'tenant-perf',
               (g % 100)::double precision,
               'Tokyo',
               '',
               '',
               '[]'::jsonb,
               'draft',
               '',
               '{}'::jsonb
             FROM generate_series(1, $1) AS g",
        )
        .bind(PERF_ORDER_COUNT)
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

fn context(request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new("tenant-perf").unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("performance-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("p8f-performance").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn imported_draft(index: usize) -> ImportedOrderDraft {
    ImportedOrderDraft {
        reconciliation_order_no: format!("P8F-WRITE-{index:05}"),
        start_date: "2026-10-10".into(),
        end_date: "2026-10-12".into(),
        delivery_date: "2026-10-10".into(),
        pickup_methods: vec!["delivery".into()],
        address: "P8-F performance baseline".into(),
        notes: "bounded PG18 regression sample".into(),
    }
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    assert!(!samples.is_empty());
    let mut ordered = samples.to_vec();
    ordered.sort_unstable();
    let rank = ((ordered.len() * percentile).div_ceil(100)).saturating_sub(1);
    ordered[rank.min(ordered.len() - 1)]
}

fn millis(value: Duration) -> f64 {
    value.as_secs_f64() * 1_000.0
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_p8f_performance_baseline_bounds_order_read_and_transactional_write()
-> anyhow::Result<()> {
    let fixture = LivePerformanceFixture::create().await?;

    let identity = sqlx::query(
        "SELECT current_database() AS database_name,
                current_setting('server_version_num') AS server_version_num,
                pg_is_in_recovery() AS in_recovery",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let database_name: String = identity.try_get("database_name")?;
    let server_version_num: String = identity.try_get("server_version_num")?;
    let in_recovery: bool = identity.try_get("in_recovery")?;

    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped = provider.bind(&context("p8f-performance-baseline"))?;

    for _ in 0..3 {
        let page = scoped.orders().list(&OrderListRequest::default())?;
        assert_eq!(page.total, PERF_ORDER_COUNT as u32);
        assert_eq!(page.orders.len(), 30);
    }

    let mut read_samples = Vec::with_capacity(READ_SAMPLES);
    for _ in 0..READ_SAMPLES {
        let started = Instant::now();
        let page = scoped.orders().list(&OrderListRequest::default())?;
        read_samples.push(started.elapsed());
        assert_eq!(page.total, PERF_ORDER_COUNT as u32);
        assert_eq!(page.orders.len(), 30);
    }

    let warmup = scoped
        .order_commands()
        .create_imported_draft(imported_draft(0))?;
    assert_eq!(warmup.status, "draft");

    let mut write_samples = Vec::with_capacity(WRITE_SAMPLES);
    for index in 1..=WRITE_SAMPLES {
        let started = Instant::now();
        let created = scoped
            .order_commands()
            .create_imported_draft(imported_draft(index))?;
        write_samples.push(started.elapsed());
        assert_eq!(created.status, "draft");
    }

    let read_p50 = percentile(&read_samples, 50);
    let read_p95 = percentile(&read_samples, 95);
    let write_p50 = percentile(&write_samples, 50);
    let write_p95 = percentile(&write_samples, 95);

    println!(
        "P8F_PERF_BASELINE database={} server_version_num={} in_recovery={} seed_orders={}",
        database_name, server_version_num, in_recovery, PERF_ORDER_COUNT
    );
    println!(
        "P8F_PERF_BASELINE order_list samples={} p50_ms={:.3} p95_ms={:.3} guard_ms={}",
        READ_SAMPLES,
        millis(read_p50),
        millis(read_p95),
        ORDER_LIST_P95_LIMIT.as_millis()
    );
    println!(
        "P8F_PERF_BASELINE imported_draft samples={} p50_ms={:.3} p95_ms={:.3} guard_ms={}",
        WRITE_SAMPLES,
        millis(write_p50),
        millis(write_p95),
        IMPORTED_DRAFT_P95_LIMIT.as_millis()
    );

    anyhow::ensure!(
        read_p95 <= ORDER_LIST_P95_LIMIT,
        "P8-F order-list p95 regression: {:.3}ms > {}ms guard",
        millis(read_p95),
        ORDER_LIST_P95_LIMIT.as_millis()
    );
    anyhow::ensure!(
        write_p95 <= IMPORTED_DRAFT_P95_LIMIT,
        "P8-F imported-draft p95 regression: {:.3}ms > {}ms guard",
        millis(write_p95),
        IMPORTED_DRAFT_P95_LIMIT.as_millis()
    );

    fixture.cleanup().await
}

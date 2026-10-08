use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{BarcodeMutationError, PostgresRepositoryProvider, RepositoryProvider};

struct LiveBarcodeFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveBarcodeFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_barcode_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-barcode-a','Barcode A','tenant-barcode-a','active','test','now','now'),
             ('tenant-barcode-b','Barcode B','tenant-barcode-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        for identity in ["barcode-actor-a", "barcode-actor-b"] {
            sqlx::query(
                "INSERT INTO identities
                 (id,username,password_hash,display_name,status,created_at,updated_at)
                 VALUES ($1,$1,'!non-interactive',$1,'active','now','now')",
            )
            .bind(identity)
            .execute(&pool)
            .await?;
        }

        sqlx::query(
            "INSERT INTO tenant_memberships
             (id,identity_id,tenant_id,role,status,created_at,updated_at)
             VALUES
             ('barcode-membership-a','barcode-actor-a','tenant-barcode-a','admin','active','now','now'),
             ('barcode-membership-b','barcode-actor-b','tenant-barcode-b','admin','active','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO warehouses
             (id,name,type,enabled,createdat,updatedat,tenant_id)
             VALUES
             ('barcode-wh-a','Barcode Warehouse A','physical',TRUE,'now','now','tenant-barcode-a'),
             ('barcode-wh-b','Barcode Warehouse B','physical',TRUE,'now','now','tenant-barcode-b')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO devices
             (id,serialno,rentalstatus,createdat,modelid,currentwarehouseid,warning_status,tenant_id)
             VALUES
             ('barcode-device-a1','BAR-A-001','available','now','','barcode-wh-a','正常','tenant-barcode-a'),
             ('barcode-device-a2','BAR-A-002','available','now','','barcode-wh-a','正常','tenant-barcode-a'),
             ('barcode-device-b1','BAR-B-001','available','now','','barcode-wh-b','正常','tenant-barcode-b')",
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

fn context(tenant: &str, actor: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated(actor, "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("barcode-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_barcode_authority_preserves_scope_idempotence_scan_references_stats_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveBarcodeFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-barcode-a", "barcode-actor-a", "barcode-a");
    let ctx_b = context("tenant-barcode-b", "barcode-actor-b", "barcode-b");

    let barcode_text = {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let (generated, already_exists) = scoped_a
            .barcodes()
            .generate("BAR-A-001", "2026-10-01 22:00:00")?;
        assert!(!already_exists);
        assert!(generated.barcode_text.starts_with("PKT-BAR-A-001-"));

        let (same, already_exists) = scoped_a
            .barcodes()
            .generate("BAR-A-001", "2026-10-01 22:00:01")?;
        assert!(already_exists);
        assert_eq!(same.id, generated.id);
        assert_eq!(same.barcode_text, generated.barcode_text);

        assert!(
            scoped_b
                .barcodes()
                .lookup(&generated.barcode_text)?
                .is_none()
        );
        let lookup = scoped_a
            .barcodes()
            .lookup(&generated.barcode_text)?
            .expect("tenant A barcode");
        assert_eq!(lookup.device_serial_no, "BAR-A-001");
        assert_eq!(
            lookup.device_info.current_warehouse_id.as_deref(),
            Some("barcode-wh-a")
        );
        assert_eq!(lookup.device_info.status.as_deref(), Some("正常"));

        assert_eq!(
            scoped_a.barcodes().batch_generate("2026-10-01 22:00:02")?,
            1
        );

        let event = scoped_a.barcodes().record_scan(
            "BAR-A-001",
            Some(&generated.barcode_text),
            "inventory",
            "barcode-actor-a",
            Some("barcode-wh-a"),
            Some("counted"),
            "2026-10-01 22:00:03",
        )?;
        assert_eq!(event.scanned_by, "barcode-actor-a");
        assert_eq!(event.warehouse_id.as_deref(), Some("barcode-wh-a"));

        let cross_tenant_device = scoped_b.barcodes().record_scan(
            "BAR-A-001",
            None,
            "inventory",
            "barcode-actor-b",
            Some("barcode-wh-b"),
            None,
            "2026-10-01 22:00:04",
        );
        assert!(matches!(
            cross_tenant_device,
            Err(BarcodeMutationError::ScanReferencesNotFound)
        ));

        let cross_tenant_warehouse = scoped_a.barcodes().record_scan(
            "BAR-A-001",
            None,
            "inventory",
            "barcode-actor-a",
            Some("barcode-wh-b"),
            None,
            "2026-10-01 22:00:05",
        );
        assert!(matches!(
            cross_tenant_warehouse,
            Err(BarcodeMutationError::ScanReferencesNotFound)
        ));

        let cross_tenant_identity = scoped_a.barcodes().record_scan(
            "BAR-A-001",
            None,
            "inventory",
            "barcode-actor-b",
            Some("barcode-wh-a"),
            None,
            "2026-10-01 22:00:06",
        );
        assert!(matches!(
            cross_tenant_identity,
            Err(BarcodeMutationError::ScanReferencesNotFound)
        ));

        let history = scoped_a.barcodes().history(
            None,
            None,
            Some("2026-10-01"),
            Some("2026-10-01"),
            1,
            20,
        )?;
        assert_eq!(history.total, 1);
        assert_eq!(history.events[0].scanned_by, "barcode-actor-a");
        assert_eq!(
            scoped_b
                .barcodes()
                .history(None, None, None, None, 1, 20)?
                .total,
            0
        );

        let stats = scoped_a
            .barcodes()
            .stats("2026-10-01", "2026-09-24 00:00:00")?;
        assert_eq!(stats.today_scans, 1);
        assert_eq!(stats.this_week_scans, 1);
        assert_eq!(stats.by_type.inventory, 1);

        generated.barcode_text
    };

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context(
        "tenant-barcode-a",
        "barcode-actor-a",
        "barcode-recomposed",
    ))?;
    let persisted = scoped_after
        .barcodes()
        .lookup(&barcode_text)?
        .expect("barcode after recomposition");
    assert_eq!(persisted.device_serial_no, "BAR-A-001");
    assert_eq!(
        scoped_after
            .barcodes()
            .history(Some("BAR-A-001"), Some("inventory"), None, None, 1, 20)?
            .total,
        1
    );

    fixture.cleanup().await
}

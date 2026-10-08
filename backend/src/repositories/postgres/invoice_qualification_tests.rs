use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{InvoiceMutationError, PostgresRepositoryProvider, RepositoryProvider};

struct LiveInvoiceFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveInvoiceFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_invoice_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-invoice-a','Invoice A','tenant-invoice-a','active','test','now','now'),
             ('tenant-invoice-b','Invoice B','tenant-invoice-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO tax_config
             (id,tax_type,rate,effective_from,is_active,created_at,tenant_id)
             VALUES ('vat-invoice-a','vat',0.06,'2026-01-01',1,'now','tenant-invoice-a')",
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
        ActorIdentity::authenticated("invoice-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("invoice-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_invoice_authority_preserves_tenant_numbering_atomic_accounting_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveInvoiceFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-invoice-a", "invoice-a");
    let ctx_b = context("tenant-invoice-b", "invoice-b");
    let now = "2026-09-29T21:45:00.000+08:00";

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let a1 = scoped_a
            .invoices()
            .issue("order-a", 100.0, "普通发票", None, now)?;
        let b1 = scoped_b
            .invoices()
            .issue("order-b", 200.0, "普通发票", None, now)?;

        assert_eq!(a1.invoice_no, "INV-20260929-0001");
        assert_eq!(b1.invoice_no, "INV-20260929-0001");
        assert_eq!(a1.tax_rate, 0.06);
        assert_eq!(a1.tax_amount, 6.0);
        assert_eq!(b1.tax_rate, 0.13);
        assert_eq!(b1.tax_amount, 26.0);
        assert!(scoped_b.invoices().get_by_id(&a1.invoice_id)?.is_none());

        let a2 = scoped_a
            .invoices()
            .issue("order-a-2", 50.0, "专用发票", Some(0.10), now)?;
        assert_eq!(a2.invoice_no, "INV-20260929-0002");

        scoped_a
            .invoices()
            .void(&a2.invoice_id, "2026-09-29T21:46:00.000+08:00")?;
        let repeated_void = scoped_a
            .invoices()
            .void(&a2.invoice_id, "2026-09-29T21:47:00.000+08:00");
        assert!(matches!(
            repeated_void,
            Err(InvoiceMutationError::NotIssued(status)) if status == "voided"
        ));

        let red = scoped_a.invoices().red_flush(
            &a1.invoice_id,
            "customer correction",
            "2026-09-29T21:48:00.000+08:00",
        )?;
        assert_eq!(red.red_invoice_no, "INV-20260929-0003");
        assert_eq!(red.red_amount, -100.0);

        let original = scoped_a
            .invoices()
            .get_by_id(&a1.invoice_id)?
            .expect("original invoice");
        let red_invoice = scoped_a
            .invoices()
            .get_by_id(&red.red_invoice_id)?
            .expect("red invoice");
        assert_eq!(original.status, "voided");
        assert_eq!(red_invoice.status, "issued");
        assert_eq!(red_invoice.amount, -100.0);

        assert_eq!(
            scoped_a
                .invoices()
                .list(None, Some("2026-09-29"), Some("2026-09-29"), 1, 20)?
                .total,
            3
        );
        assert_eq!(scoped_b.invoices().list(None, None, None, 1, 20)?.total, 1);
    }

    let migration_applied: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM schema_migrations
             WHERE id='076_r4_tenant_scoped_invoice_numbers'
         )",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert!(migration_applied);

    let tax_scope_migration_applied: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM schema_migrations
             WHERE id='077_r4_tenant_scoped_tax_config'
         )",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert!(tax_scope_migration_applied);

    let revenue_sum: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount),0)::double precision
         FROM revenue_records WHERE tenant_id='tenant-invoice-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    let accounting_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint
         FROM accounting_entries WHERE tenant_id='tenant-invoice-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(revenue_sum, 50.0);
    assert_eq!(accounting_count, 7);

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-invoice-a", "invoice-recomposed"))?;
    let next = scoped_after.invoices().issue(
        "order-a-3",
        25.0,
        "普通发票",
        Some(0.05),
        "2026-09-29T21:49:00.000+08:00",
    )?;
    assert_eq!(next.invoice_no, "INV-20260929-0004");

    fixture.cleanup().await
}

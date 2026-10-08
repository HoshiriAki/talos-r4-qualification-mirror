use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor, Row};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{PostgresRepositoryProvider, RepositoryProvider};

struct LiveTaxFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveTaxFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_tax_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-tax-a','Tax A','tenant-tax-a','active','test','now','now'),
             ('tenant-tax-b','Tax B','tenant-tax-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO invoices
             (id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,
              tenant_id,issued_at,created_at)
             VALUES
             ('tax-invoice-a','tax-order-a','TAX-A','普通发票',106,0.06,6,'issued',
              'tenant-tax-a','2026-09-30T09:00:00+08:00','now'),
             ('tax-invoice-b','tax-order-b','TAX-B','专用发票',113,0.13,13,'issued',
              'tenant-tax-b','2026-09-30T09:00:00+08:00','now')",
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
        ActorIdentity::authenticated("tax-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("tax-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_tax_authority_preserves_scope_active_rate_export_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveTaxFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-tax-a", "tax-a");
    let ctx_b = context("tenant-tax-b", "tax-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        scoped_a
            .taxes()
            .upsert_config("vat", 0.06, "2026-01-01", "2026-09-30T09:10:00+08:00")?;
        scoped_a
            .taxes()
            .upsert_config("vat", 0.09, "2026-07-01", "2026-09-30T09:11:00+08:00")?;
        scoped_b
            .taxes()
            .upsert_config("vat", 0.13, "2026-01-01", "2026-09-30T09:12:00+08:00")?;

        let configs_a = scoped_a.taxes().get_configs("vat")?;
        assert_eq!(configs_a.len(), 2);
        assert_eq!(configs_a.iter().filter(|row| row.is_active).count(), 1);
        assert_eq!(scoped_a.taxes().active_rate("vat")?, Some(0.09));
        assert_eq!(scoped_b.taxes().active_rate("vat")?, Some(0.13));

        let export_a = scoped_a.taxes().export_snapshot("2026-09")?;
        assert_eq!(export_a.invoices.len(), 1);
        assert_eq!(export_a.invoices[0].invoice_no, "TAX-A");
        assert_eq!(export_a.configs.len(), 1);
        assert_eq!(export_a.configs[0].rate, 0.09);
        assert!(
            export_a
                .invoices
                .iter()
                .all(|row| row.invoice_no != "TAX-B")
        );
    }

    let active_count = sqlx::query(
        "SELECT COUNT(*)::bigint AS active_count
         FROM tax_config
         WHERE tenant_id='tenant-tax-a' AND tax_type='vat' AND is_active=1",
    )
    .fetch_one(&fixture.pool)
    .await?
    .try_get::<i64, _>("active_count")?;
    assert_eq!(active_count, 1);

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-tax-a", "tax-recomposed"))?;
    assert_eq!(scoped_after.taxes().active_rate("vat")?, Some(0.09));
    let persisted = scoped_after.taxes().export_snapshot("2026-09")?;
    assert_eq!(persisted.invoices.len(), 1);
    assert_eq!(persisted.invoices[0].invoice_no, "TAX-A");
    assert_eq!(persisted.configs.len(), 1);
    assert_eq!(persisted.configs[0].rate, 0.09);

    fixture.cleanup().await
}

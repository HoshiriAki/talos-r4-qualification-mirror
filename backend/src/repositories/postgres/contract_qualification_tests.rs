use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId, Revision,
    TenantId, TenantScope,
};

use crate::repositories::{ContractMutationError, PostgresRepositoryProvider, RepositoryProvider};

struct LiveContractFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveContractFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_contract_{}", uuid::Uuid::new_v4().simple());
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
             ('tenant-contract-a','Contract A','tenant-contract-a','active','test','now','now'),
             ('tenant-contract-b','Contract B','tenant-contract-b','active','test','now','now')",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "INSERT INTO orders
             (id,orderno,startdate,enddate,deliverydate,pickupmethods,address,notes,
              deviceserialno,createdat,status,tenant_id)
             VALUES
             ('contract-order-alpha','CONTRACT-A','2026-10-01','2026-10-10','2026-10-01',
              'pickup','','','','now','draft','tenant-contract-a'),
             ('contract-order-beta','CONTRACT-B','2026-10-01','2026-10-11','2026-10-01',
              'pickup','','','','now','draft','tenant-contract-b')",
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
        ActorIdentity::authenticated("contract-test-user", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("contract-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_contract_authority_preserves_scope_order_identity_signature_lifecycle_and_recomposition()
-> anyhow::Result<()> {
    let fixture = LiveContractFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let ctx_a = context("tenant-contract-a", "contract-a");
    let ctx_b = context("tenant-contract-b", "contract-b");

    {
        let scoped_a = provider.bind(&ctx_a)?;
        let scoped_b = provider.bind(&ctx_b)?;

        let template_a = scoped_a.contracts().template_create(
            "Tenant A Contract",
            &serde_json::json!({"body":"Customer {customer} <unsafe>"}),
            "2026-10-01T21:30:00+08:00",
        )?;
        let template_b = scoped_b.contracts().template_create(
            "Tenant B Contract",
            &serde_json::json!({"body":"Tenant B"}),
            "2026-10-01T21:30:01+08:00",
        )?;
        assert_ne!(template_a.id, template_b.id);
        assert_eq!(scoped_a.contracts().template_list(None, 1, 20)?.total, 1);
        assert_eq!(scoped_b.contracts().template_list(None, 1, 20)?.total, 1);

        let generated = scoped_a.contracts().generate(
            "contract-order-alpha",
            None,
            "Alice",
            "10086",
            20_000.0,
            Some(&serde_json::json!({"customer":"<Alice>"})),
            "2026-10-01T21:31:00+08:00",
        )?;
        assert_eq!(generated.status, "generated");
        assert!(generated.auto_triggered);
        assert!(scoped_b.contracts().get(generated.id)?.is_none());

        let cross_tenant_sign = scoped_b.contracts().sign(
            generated.id,
            "Bob",
            "10010",
            "sig-b",
            "2026-10-01T21:32:00+08:00",
        );
        assert!(matches!(
            cross_tenant_sign,
            Err(ContractMutationError::ContractNotFound)
        ));

        let invalid_order = scoped_a.contracts().generate(
            "contract-order-beta",
            None,
            "Alice",
            "10086",
            100.0,
            None,
            "2026-10-01T21:32:10+08:00",
        );
        assert!(matches!(
            invalid_order,
            Err(ContractMutationError::OrderNotFound)
        ));

        let signed = scoped_a.contracts().sign(
            generated.id,
            "Alice",
            "10086",
            "sig-a",
            "2026-10-01T21:33:00+08:00",
        )?;
        assert_eq!(signed.id, generated.id);

        let verified = scoped_a
            .contracts()
            .verify(generated.id, "2026-10-01T21:34:00+08:00")?;
        assert_eq!(verified.id, generated.id);

        let persisted = scoped_a.contracts().get(generated.id)?.expect("contract");
        assert_eq!(persisted.order_id, "contract-order-alpha");
        assert_eq!(persisted.status, "verified");
        assert_eq!(persisted.signatures.len(), 1);
        assert!(persisted.content_json.to_string().contains("&lt;Alice&gt;"));

        assert_eq!(
            scoped_a
                .contracts()
                .list(Some("contract-order-alpha"), None, Some("verified"), 1, 20,)?
                .total,
            1
        );
        assert_eq!(
            scoped_b
                .contracts()
                .list(Some("contract-order-alpha"), None, Some("verified"), 1, 20,)?
                .total,
            0
        );
    }

    let recomposed = PostgresRepositoryProvider::new(fixture.pool.clone());
    let scoped_after = recomposed.bind(&context("tenant-contract-a", "contract-recomposed"))?;
    let persisted = scoped_after.contracts().list(
        Some("contract-order-alpha"),
        None,
        Some("verified"),
        1,
        20,
    )?;
    assert_eq!(persisted.total, 1);
    assert_eq!(persisted.items[0].order_id, "contract-order-alpha");

    fixture.cleanup().await
}

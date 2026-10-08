use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision, TenantId,
    TenantScope,
};

use crate::domain::{
    ContactKind, CustomerId, CustomerRiskStatus, CustomerStatus, Money, QuoteId, QuoteLineId,
    QuoteLineKind, QuoteStatus,
};
use crate::repositories::{
    NewCustomerContact, NewCustomerRecord, NewQuote, NewQuoteLine, PostgresRepositoryProvider,
    RepositoryProvider,
};

struct QuoteFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl QuoteFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_quote_{}", uuid::Uuid::new_v4().simple());
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

        for (id, name, slug) in [
            ("tenant-a", "Tenant A", "tenant-a"),
            ("tenant-b", "Tenant B", "tenant-b"),
        ] {
            sqlx::query(
                "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
                 VALUES ($1,$2,$3,'active','test','2026-09-15T00:00:00+08:00','2026-09-15T00:00:00+08:00')",
            )
            .bind(id)
            .bind(name)
            .bind(slug)
            .execute(&pool)
            .await?;
        }

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

fn normal_context(tenant: &str, request: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    ExecutionContext::new(
        ActorIdentity::authenticated("tenant-actor", "staff").unwrap(),
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("pg-revision").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new(request).unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn preview_context(tenant: &str) -> ExecutionContext {
    let tenant_id = TenantId::new(tenant).unwrap();
    let actor = ActorIdentity::with_authority(
        "platform-actor",
        AuthorityContext::Platform {
            membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
            roles: vec![PlatformRole::Owner],
        },
    )
    .unwrap();
    ExecutionContext::new(
        actor,
        TenantScope::tenant(tenant_id.clone()),
        DataScope::production(tenant_id, Revision::new("preview-revision").unwrap()).unwrap(),
        ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-session").unwrap()),
        RequestId::new("preview-request").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_quote_cutover_preserves_tenant_snapshot_and_conversion_authority()
-> anyhow::Result<()> {
    let fixture = QuoteFixture::create().await?;
    let provider = PostgresRepositoryProvider::new(fixture.pool.clone());
    let tenant_a = provider.bind(&normal_context("tenant-a", "quote-a"))?;
    let tenant_b = provider.bind(&normal_context("tenant-b", "quote-b"))?;

    let customer_id = CustomerId::new();
    tenant_a.customers().create(&NewCustomerRecord {
        id: customer_id.clone(),
        legal_name: "Quote Customer".into(),
        display_name: "Quote Customer".into(),
        status: CustomerStatus::Active,
        risk_status: CustomerRiskStatus::Clear,
        contacts: vec![NewCustomerContact {
            kind: ContactKind::Email,
            raw_value: "quote@example.com".into(),
            normalized_value: "quote@example.com".into(),
            is_primary: true,
        }],
        actor_identity_id: None,
    })?;

    let accessory_price = Money::new(500, "CNY").unwrap();
    let accessory = tenant_a.quotes().upsert_accessory(
        "accessory-a",
        "SKU-A",
        "Accessory A",
        &accessory_price,
        true,
    )?;
    assert_eq!(accessory.unit_price, accessory_price);
    assert!(tenant_b.quotes().get_accessory("accessory-a")?.is_none());

    let unit_price = Money::new(2500, "CNY").unwrap();
    let subtotal = unit_price.checked_mul(2).unwrap();
    let quote_id = QuoteId::new();
    let created = tenant_a.quotes().create(&NewQuote {
        id: quote_id.clone(),
        customer_id: customer_id.clone(),
        start_date: "2026-09-16".into(),
        end_date: "2026-09-20".into(),
        region: "Shanghai".into(),
        currency: "CNY".into(),
        expires_at: "2099-01-01T00:00:00Z".into(),
        lines: vec![NewQuoteLine {
            id: QuoteLineId::new(),
            kind: QuoteLineKind::Model,
            reference_id: "model-a".into(),
            description: "model:model-a".into(),
            quantity: 2,
            unit_price,
            subtotal: subtotal.clone(),
            price_snapshot_json: serde_json::json!({"authority":"fixture"}).to_string(),
        }],
    })?;
    assert_eq!(created.status, QuoteStatus::Draft);
    assert_eq!(created.total, subtotal);
    assert_eq!(created.lines.len(), 1);
    assert!(tenant_b.quotes().get(&quote_id)?.is_none());

    let confirmed = tenant_a.quotes().confirm(&quote_id)?;
    assert_eq!(confirmed.status, QuoteStatus::Confirmed);
    let converted = tenant_a
        .quotes()
        .create_order_from_quote(&quote_id, "quote conversion fixture")?;
    assert_eq!(converted.quote_id, quote_id);
    assert_eq!(converted.customer_id, customer_id);
    assert_eq!(converted.total.minor, 5000);

    let converted_quote = tenant_a
        .quotes()
        .get(&quote_id)?
        .expect("converted quote remains queryable");
    assert_eq!(converted_quote.status, QuoteStatus::Converted);
    assert_eq!(
        converted_quote.converted_order_id.as_deref(),
        Some(converted.order_id.as_str())
    );
    assert!(tenant_a.orders().get_by_id(&converted.order_id)?.is_some());
    assert!(tenant_b.orders().get_by_id(&converted.order_id)?.is_none());
    let line_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM order_lines WHERE tenant_id = 'tenant-a' AND order_id = $1",
    )
    .bind(&converted.order_id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(line_count, 1);

    let preview = provider.bind(&preview_context("tenant-a"))?;
    assert!(preview.quotes().get(&quote_id)?.is_some());
    assert_eq!(
        preview
            .quotes()
            .upsert_accessory(
                "accessory-preview",
                "SKU-PREVIEW",
                "Forbidden",
                &Money::new(100, "CNY").unwrap(),
                true,
            )
            .unwrap_err()
            .code(),
        "REPOSITORY_PREVIEW_WRITE_DENIED"
    );

    fixture.cleanup().await?;
    Ok(())
}

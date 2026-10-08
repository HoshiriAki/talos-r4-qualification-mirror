#![cfg(feature = "postgres")]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use super::catalog_repository::IntegrationCatalogRepository;
use super::deposit::DepositReconciliationOutcome;
use super::deposit_refund_persistence_contract::DepositRefundPersistence;
use super::deposit_refund_persistence_postgres::PostgresDepositRefundPersistence;
use super::operation::{ExternalOperation, OperationState};
use super::operation_runtime_contract::OperationRuntimePersistence;
use super::operation_runtime_postgres::PostgresOperationRuntimePersistence;
use super::types::{
    CapabilityId, IntegrationError, Money, ProviderBinding, ProviderBindingId, ProviderHealth,
    ProviderId, ProviderInstance, ProviderInstanceId, ProviderLifecycle, ProviderManifest,
    ProviderReadiness,
};
use crate::observability::RuntimeMetrics;

struct LiveFinanceFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveFinanceFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_deposit_refund_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        drop(admin);

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(8)
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

fn manifest() -> ProviderManifest {
    let capability = CapabilityId::new("fixture.refund").unwrap();
    ProviderManifest {
        provider_id: ProviderId::new("fixture-payments").unwrap(),
        version: "1.0.0".into(),
        capabilities: BTreeSet::from([capability.clone()]),
        config_schema: vec![],
        secret_schema: vec![],
        api_versions: BTreeMap::new(),
        webhook_types: BTreeSet::new(),
        simulation_capabilities: BTreeSet::from([capability]),
        readiness: ProviderReadiness::Fixture,
        compatibility: BTreeMap::new(),
    }
}

fn instance() -> ProviderInstance {
    ProviderInstance {
        id: ProviderInstanceId::new("instance-a").unwrap(),
        tenant_id: "tenant-a".into(),
        provider_id: ProviderId::new("fixture-payments").unwrap(),
        manifest_version: "1.0.0".into(),
        config_revision: "rev-1".into(),
        config: BTreeMap::new(),
        secret_refs: BTreeMap::new(),
        lifecycle: ProviderLifecycle::Active,
        health: ProviderHealth::Ready,
        readiness: ProviderReadiness::Fixture,
    }
}

fn binding() -> ProviderBinding {
    ProviderBinding {
        id: ProviderBindingId::new("binding-a").unwrap(),
        tenant_id: "tenant-a".into(),
        provider_instance_id: ProviderInstanceId::new("instance-a").unwrap(),
        capability: CapabilityId::new("fixture.refund").unwrap(),
        config_revision: "rev-1".into(),
        enabled: true,
    }
}

fn refund_operation(refund_id: &str) -> ExternalOperation {
    ExternalOperation {
        id: super::types::ExternalOperationId::new("refund-operation-a").unwrap(),
        tenant_id: "tenant-a".into(),
        provider_instance_id: "instance-a".into(),
        binding_id: "binding-a".into(),
        binding_revision: "rev-1".into(),
        capability: "fixture.refund".into(),
        operation_type: "refund".into(),
        idempotency_key: format!("refund:{refund_id}"),
        request_hash: "refund-request-hash-a".into(),
        state: OperationState::Ready,
        attempts: 0,
    }
}

async fn deposit_state(pool: &PgPool, deposit_id: &str) -> anyhow::Result<String> {
    Ok(sqlx::query_scalar(
        "SELECT state FROM integration_deposits
             WHERE tenant_id='tenant-a' AND id=$1",
    )
    .bind(deposit_id)
    .fetch_one(pool)
    .await?)
}

async fn refund_state(pool: &PgPool, refund_id: &str) -> anyhow::Result<String> {
    Ok(sqlx::query_scalar(
        "SELECT state FROM refund_intents
             WHERE tenant_id='tenant-a' AND id=$1",
    )
    .bind(refund_id)
    .fetch_one(pool)
    .await?)
}

async fn refundable_minor(pool: &PgPool, deposit_id: &str) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(SUM(
                CASE entry_type
                    WHEN 'received' THEN amount_minor
                    WHEN 'manual_adjustment' THEN amount_minor
                    WHEN 'deducted' THEN -amount_minor
                    WHEN 'refund_completed' THEN -amount_minor
                    ELSE 0
                END
             ),0)::BIGINT
             FROM integration_deposit_ledger
             WHERE tenant_id='tenant-a' AND deposit_id=$1",
    )
    .bind(deposit_id)
    .fetch_one(pool)
    .await?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_deposit_refund_preserves_ledger_idempotency_reconciliation_and_metrics()
-> anyhow::Result<()> {
    let fixture = LiveFinanceFixture::create().await?;
    let catalog = IntegrationCatalogRepository::postgres(fixture.pool.clone());
    catalog.save_manifest(&manifest())?;
    catalog.save_instance(&instance())?;
    catalog.save_binding(&binding(), "operator-a")?;

    let metrics = Arc::new(RuntimeMetrics::default());
    let finance =
        PostgresDepositRefundPersistence::new_with_metrics(fixture.pool.clone(), metrics.clone());
    let operations = PostgresOperationRuntimePersistence::new(fixture.pool.clone());

    let deposit =
        finance.create_deposit("tenant-a", "order", "order-a", Money::new(10_000, "USD")?)?;
    assert_eq!(
        deposit_state(&fixture.pool, deposit.as_str()).await?,
        "expected"
    );

    finance.record_deposit_received(
        "tenant-a",
        &deposit,
        Money::new(10_000, "USD")?,
        "receipt-a",
    )?;
    assert_eq!(
        deposit_state(&fixture.pool, deposit.as_str()).await?,
        "recorded"
    );
    assert_eq!(
        refundable_minor(&fixture.pool, deposit.as_str()).await?,
        10_000
    );

    assert_eq!(
        finance
            .deduct_deposit(
                "tenant-a",
                &deposit,
                Money::new(100, "EUR")?,
                "currency-mismatch",
            )
            .unwrap_err(),
        IntegrationError::CurrencyMismatch
    );

    finance.hold_deposit("tenant-a", &deposit, Money::new(1_000, "USD")?, "hold-a")?;
    finance.deduct_deposit(
        "tenant-a",
        &deposit,
        Money::new(2_500, "USD")?,
        "deduction-a",
    )?;
    assert_eq!(
        deposit_state(&fixture.pool, deposit.as_str()).await?,
        "partially_deducted"
    );
    assert_eq!(
        refundable_minor(&fixture.pool, deposit.as_str()).await?,
        7_500
    );

    let refund = finance.request_refund(
        "tenant-a",
        &deposit,
        Money::new(4_000, "USD")?,
        "refund-key-a",
        "customer refund",
    )?;
    let duplicate = finance.request_refund(
        "tenant-a",
        &deposit,
        Money::new(4_000, "USD")?,
        "refund-key-a",
        "customer refund",
    )?;
    assert_eq!(refund, duplicate);
    assert_eq!(
        finance
            .request_refund(
                "tenant-a",
                &deposit,
                Money::new(4_100, "USD")?,
                "refund-key-a",
                "customer refund",
            )
            .unwrap_err(),
        IntegrationError::InvalidOperationTransition
    );
    assert_eq!(
        finance
            .request_refund(
                "tenant-a",
                &deposit,
                Money::new(4_000, "USD")?,
                "refund-key-b",
                "second refund",
            )
            .unwrap_err(),
        IntegrationError::RefundAmountExceedsAvailable
    );
    assert!(
        finance
            .request_refund(
                "tenant-b",
                &deposit,
                Money::new(100, "USD")?,
                "cross-tenant",
                "must fail closed",
            )
            .is_err()
    );

    let operation = refund_operation(refund.as_str());
    operations.persist_operation(&operation)?;
    finance.link_refund_operation("tenant-a", &refund, &operation.id)?;
    assert_eq!(
        refund_state(&fixture.pool, refund.as_str()).await?,
        "approved"
    );

    let linked_operation: Option<String> = sqlx::query_scalar(
        "SELECT external_operation_id
         FROM refund_intents
         WHERE tenant_id='tenant-a' AND id=$1",
    )
    .bind(refund.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(linked_operation.as_deref(), Some(operation.id.as_str()));

    finance.reconcile_refund(
        "tenant-a",
        &refund,
        DepositReconciliationOutcome::Matched,
        "provider-match-a",
        Some("operator-a"),
    )?;
    assert_eq!(
        refund_state(&fixture.pool, refund.as_str()).await?,
        "completed"
    );
    assert_eq!(
        refundable_minor(&fixture.pool, deposit.as_str()).await?,
        3_500
    );

    let refund_ledger_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM integration_deposit_ledger
         WHERE tenant_id='tenant-a'
           AND deposit_id=$1
           AND entry_type='refund_completed'
           AND external_operation_id=$2",
    )
    .bind(deposit.as_str())
    .bind(operation.id.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(refund_ledger_count, 1);

    let matched_reconciliation_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM integration_deposit_reconciliations
         WHERE tenant_id='tenant-a'
           AND deposit_id=$1
           AND refund_id=$2
           AND outcome='matched'
           AND evidence_ref='provider-match-a'
           AND resolved_by='operator-a'",
    )
    .bind(deposit.as_str())
    .bind(refund.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(matched_reconciliation_count, 1);
    assert_eq!(
        finance
            .reconcile_refund(
                "tenant-a",
                &refund,
                DepositReconciliationOutcome::Matched,
                "duplicate-match",
                Some("operator-a"),
            )
            .unwrap_err(),
        IntegrationError::InvalidOperationTransition
    );

    let manual_deposit = finance.create_deposit(
        "tenant-a",
        "settlement",
        "settlement-manual",
        Money::new(2_000, "USD")?,
    )?;
    finance.record_deposit_received(
        "tenant-a",
        &manual_deposit,
        Money::new(2_000, "USD")?,
        "receipt-manual",
    )?;
    let manual_refund = finance.request_refund(
        "tenant-a",
        &manual_deposit,
        Money::new(1_000, "USD")?,
        "manual-refund",
        "manual review",
    )?;
    finance.reconcile_refund(
        "tenant-a",
        &manual_refund,
        DepositReconciliationOutcome::ManualRequired,
        "manual-evidence-a",
        Some("operator-a"),
    )?;
    assert_eq!(
        refund_state(&fixture.pool, manual_refund.as_str()).await?,
        "manual_resolution_required"
    );
    assert_eq!(
        deposit_state(&fixture.pool, manual_deposit.as_str()).await?,
        "manual_resolution_required"
    );

    let release_deposit = finance.create_deposit(
        "tenant-a",
        "order",
        "order-release",
        Money::new(5_000, "USD")?,
    )?;
    finance.record_deposit_received(
        "tenant-a",
        &release_deposit,
        Money::new(5_000, "USD")?,
        "receipt-release",
    )?;
    finance.hold_deposit(
        "tenant-a",
        &release_deposit,
        Money::new(500, "USD")?,
        "hold-release",
    )?;
    finance.release_deposit(
        "tenant-a",
        &release_deposit,
        Money::new(5_000, "USD")?,
        "release-a",
    )?;
    assert_eq!(
        deposit_state(&fixture.pool, release_deposit.as_str()).await?,
        "released"
    );

    let ledger_id: String = sqlx::query_scalar(
        "SELECT id
         FROM integration_deposit_ledger
         WHERE tenant_id='tenant-a' AND deposit_id=$1
         ORDER BY created_at,id
         LIMIT 1",
    )
    .bind(deposit.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert!(
        sqlx::query(
            "UPDATE integration_deposit_ledger
             SET audit_ref='tampered'
             WHERE id=$1",
        )
        .bind(&ledger_id)
        .execute(&fixture.pool)
        .await
        .is_err()
    );

    let output = metrics.render();
    for (event, count) in [
        ("deposit_recorded", 3),
        ("deposit_received_ledger", 3),
        ("deposit_deduction_admitted", 1),
        ("refund_intent_admitted", 2),
        ("refund_reconciliation", 2),
    ] {
        let expected =
            format!("talos_financial_foundation_events_total{{event=\"{event}\"}} {count}");
        assert!(
            output.contains(&expected),
            "missing expected financial metric {expected}; rendered metrics:\n{output}"
        );
    }
    assert!(
        !output.contains(
            "talos_financial_foundation_events_total{event=\"refund_operation_planned\"}"
        ),
        "idempotent refund linkage must not emit refund_operation_planned metrics; rendered metrics:\n{output}"
    );

    fixture.cleanup().await
}

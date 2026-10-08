use std::collections::{BTreeMap, BTreeSet};

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use super::catalog_repository::IntegrationCatalogRepository;
use super::operation::{ExternalOperation, OperationState};
use super::operation_runtime_contract::{
    DispatchResult, OperationRuntimePersistence, OperationRuntimePolicy,
};
use super::operation_runtime_postgres::PostgresOperationRuntimePersistence;
use super::transport::{TransportErrorClass, TransportFailure};
use super::types::{
    CapabilityId, ExternalOperationId, IntegrationError, ProviderBinding, ProviderBindingId,
    ProviderHealth, ProviderId, ProviderInstance, ProviderInstanceId, ProviderLifecycle,
    ProviderManifest, ProviderReadiness,
};

struct LiveOperationRuntimeFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveOperationRuntimeFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_operation_runtime_{}", uuid::Uuid::new_v4().simple());
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
    let capability = CapabilityId::new("fixture.payment.charge").unwrap();
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
        capability: CapabilityId::new("fixture.payment.charge").unwrap(),
        config_revision: "rev-1".into(),
        enabled: true,
    }
}

fn operation(id: &str, idempotency_key: &str, request_hash: &str) -> ExternalOperation {
    ExternalOperation {
        id: ExternalOperationId::new(id).unwrap(),
        tenant_id: "tenant-a".into(),
        provider_instance_id: "instance-a".into(),
        binding_id: "binding-a".into(),
        binding_revision: "rev-1".into(),
        capability: "fixture.payment.charge".into(),
        operation_type: "charge".into(),
        idempotency_key: idempotency_key.into(),
        request_hash: request_hash.into(),
        state: OperationState::Ready,
        attempts: 0,
    }
}

async fn stored_state(pool: &PgPool, id: &str) -> anyhow::Result<String> {
    Ok(sqlx::query_scalar(
        "SELECT state FROM external_operations WHERE tenant_id='tenant-a' AND id=$1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_operation_runtime_preserves_claim_outcome_recovery_and_reconciliation()
-> anyhow::Result<()> {
    let fixture = LiveOperationRuntimeFixture::create().await?;
    let catalog = IntegrationCatalogRepository::postgres(fixture.pool.clone());
    catalog.save_manifest(&manifest())?;
    catalog.save_instance(&instance())?;
    catalog.save_binding(&binding(), "actor-a")?;

    let persistence = PostgresOperationRuntimePersistence::new(fixture.pool.clone());
    let policy = OperationRuntimePolicy::default();

    let primary = operation("operation-a", "idem-a", "hash-a");
    assert_eq!(
        persistence.persist_operation(&primary)?,
        ExternalOperationId::new("operation-a")?
    );
    assert_eq!(
        persistence.persist_operation(&primary)?,
        ExternalOperationId::new("operation-a")?
    );

    let mut conflicting = operation("operation-conflict", "idem-a", "hash-conflict");
    conflicting.binding_id = primary.binding_id.clone();
    assert_eq!(
        persistence.persist_operation(&conflicting).unwrap_err(),
        IntegrationError::InvalidOperationTransition
    );

    assert!(
        persistence
            .claim_next_operation("tenant-b", &policy)?
            .is_none()
    );

    let claimed = persistence
        .claim_next_operation("tenant-a", &policy)?
        .expect("tenant-a operation must be claimable");
    assert_eq!(claimed.operation_id, primary.id);
    assert_eq!(claimed.attempt_number, 1);
    assert_eq!(
        stored_state(&fixture.pool, "operation-a").await?,
        "dispatching"
    );

    persistence.record_runtime_outcome(
        &claimed,
        Err(TransportFailure::after_dispatch(
            TransportErrorClass::ResponseTimeout,
        )),
        &policy,
    )?;
    assert_eq!(
        stored_state(&fixture.pool, "operation-a").await?,
        "unknown_outcome"
    );

    persistence.begin_reconciliation("tenant-a", &primary.id, "evidence://effect-check")?;
    assert_eq!(
        stored_state(&fixture.pool, "operation-a").await?,
        "reconciling"
    );
    persistence.resolve_reconciliation("tenant-a", &primary.id, false, "operator-a")?;
    assert_eq!(stored_state(&fixture.pool, "operation-a").await?, "ready");

    let retry = persistence
        .claim_next_operation("tenant-a", &policy)?
        .expect("reconciled no-effect operation must be retryable");
    assert_eq!(retry.operation_id, primary.id);
    assert_eq!(retry.attempt_number, 2);
    persistence.record_runtime_outcome(
        &retry,
        Ok(DispatchResult::Succeeded {
            provider_result_ref: Some("provider://result-a".into()),
        }),
        &policy,
    )?;
    assert_eq!(
        stored_state(&fixture.pool, "operation-a").await?,
        "succeeded"
    );

    let recovery = operation("operation-recovery", "idem-recovery", "hash-recovery");
    persistence.persist_operation(&recovery)?;
    let recovery_claim = persistence
        .claim_next_operation("tenant-a", &policy)?
        .expect("recovery operation must be claimable");
    assert_eq!(recovery_claim.operation_id, recovery.id);
    assert_eq!(
        persistence.recover_inflight_operations("tenant-a")?,
        1,
        "only the currently dispatching operation may be recovered"
    );
    assert_eq!(
        stored_state(&fixture.pool, "operation-recovery").await?,
        "unknown_outcome"
    );
    let recovery_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM external_operation_runtime_events
         WHERE tenant_id='tenant-a' AND operation_id='operation-recovery'
           AND event_type='recovered_after_restart'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(recovery_events, 1);

    let mut stale = operation("operation-stale", "idem-stale", "hash-stale");
    stale.binding_revision = "rev-stale".into();
    persistence.persist_operation(&stale)?;
    assert_eq!(
        persistence
            .claim_next_operation("tenant-a", &policy)
            .unwrap_err(),
        IntegrationError::BindingRevisionStale
    );
    assert_eq!(
        stored_state(&fixture.pool, "operation-stale").await?,
        "manual_resolution_required"
    );

    let reconciliation_outcome: String = sqlx::query_scalar(
        "SELECT outcome FROM external_operation_reconciliations
         WHERE tenant_id='tenant-a' AND operation_id='operation-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(reconciliation_outcome, "effect_absent");

    let metrics = persistence.operation_runtime_metrics("tenant-a")?;
    assert_eq!(metrics.dispatching, 0);
    assert_eq!(metrics.retryable_failure, 0);
    assert_eq!(metrics.unknown_outcome, 1);

    fixture.cleanup().await
}

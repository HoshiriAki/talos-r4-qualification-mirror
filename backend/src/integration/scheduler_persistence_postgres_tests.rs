#![cfg(feature = "postgres")]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};

use super::catalog_repository::IntegrationCatalogRepository;
use super::operation::{ExternalOperation, OperationState};
use super::operation_runtime_contract::{OperationRuntimePersistence, OperationRuntimePolicy};
use super::operation_runtime_postgres::PostgresOperationRuntimePersistence;
use super::scheduler_persistence_contract::IntegrationSchedulerPersistence;
use super::scheduler_persistence_postgres::PostgresIntegrationSchedulerPersistence;
use super::types::{
    CapabilityId, ExternalOperationId, ProviderBinding, ProviderBindingId, ProviderHealth,
    ProviderId, ProviderInstance, ProviderInstanceId, ProviderLifecycle, ProviderManifest,
    ProviderReadiness,
};
use super::webhook_persistence_contract::{WebhookAdminPersistence, WebhookRuntimePersistence};
use super::webhook_persistence_postgres::PostgresWebhookPersistence;
use super::worker::FixtureIntegrationWorker;
use crate::observability::NoopMetrics;

struct LiveSchedulerFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveSchedulerFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_p8_scheduler_recovery_{}", uuid::Uuid::new_v4().simple());
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
    let capability = CapabilityId::new("fixture.integration.work").unwrap();
    ProviderManifest {
        provider_id: ProviderId::new("fixture-scheduler").unwrap(),
        version: "1.0.0".into(),
        capabilities: BTreeSet::from([capability.clone()]),
        config_schema: vec![],
        secret_schema: vec![],
        api_versions: BTreeMap::new(),
        webhook_types: BTreeSet::from(["fixture.scheduler.updated".into()]),
        simulation_capabilities: BTreeSet::from([capability]),
        readiness: ProviderReadiness::Fixture,
        compatibility: BTreeMap::new(),
    }
}

fn instance(tenant_id: &str) -> ProviderInstance {
    ProviderInstance {
        id: ProviderInstanceId::new("instance-a").unwrap(),
        tenant_id: tenant_id.into(),
        provider_id: ProviderId::new("fixture-scheduler").unwrap(),
        manifest_version: "1.0.0".into(),
        config_revision: "rev-1".into(),
        config: BTreeMap::new(),
        secret_refs: BTreeMap::new(),
        lifecycle: ProviderLifecycle::Active,
        health: ProviderHealth::Ready,
        readiness: ProviderReadiness::Fixture,
    }
}

fn binding(tenant_id: &str) -> ProviderBinding {
    ProviderBinding {
        id: ProviderBindingId::new("binding-a").unwrap(),
        tenant_id: tenant_id.into(),
        provider_instance_id: ProviderInstanceId::new("instance-a").unwrap(),
        capability: CapabilityId::new("fixture.integration.work").unwrap(),
        config_revision: "rev-1".into(),
        enabled: true,
    }
}

fn operation(tenant_id: &str, id: &str, key: &str) -> ExternalOperation {
    ExternalOperation {
        id: ExternalOperationId::new(id).unwrap(),
        tenant_id: tenant_id.into(),
        provider_instance_id: "instance-a".into(),
        binding_id: "binding-a".into(),
        binding_revision: "rev-1".into(),
        capability: "fixture.integration.work".into(),
        operation_type: "fixture_dispatch".into(),
        idempotency_key: key.into(),
        request_hash: format!("hash-{key}"),
        state: OperationState::Ready,
        attempts: 0,
    }
}

async fn operation_state(pool: &PgPool, id: &str) -> anyhow::Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM external_operations WHERE id=$1")
            .bind(id)
            .fetch_one(pool)
            .await?,
    )
}

async fn webhook_status(pool: &PgPool, event_id: &str) -> anyhow::Result<String> {
    Ok(
        sqlx::query_scalar("SELECT status FROM webhook_inbox WHERE provider_event_id=$1")
            .bind(event_id)
            .fetch_one(pool)
            .await?,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires TALOS_TEST_POSTGRES_URL for a disposable PostgreSQL 18 database"]
async fn live_pg18_scheduler_recovery_preserves_snapshot_boundary_atomicity_and_cursor_fairness()
-> anyhow::Result<()> {
    let fixture = LiveSchedulerFixture::create().await?;
    let catalog = IntegrationCatalogRepository::postgres(fixture.pool.clone());
    catalog.save_manifest(&manifest())?;
    for tenant_id in ["tenant-a", "tenant-b", "tenant-c"] {
        catalog.save_instance(&instance(tenant_id))?;
        catalog.save_binding(&binding(tenant_id), "operator-a")?;
    }

    let operations = PostgresOperationRuntimePersistence::new(fixture.pool.clone());
    let webhooks = PostgresWebhookPersistence::new(fixture.pool.clone());
    let scheduler = PostgresIntegrationSchedulerPersistence::new(fixture.pool.clone());
    let policy = OperationRuntimePolicy::default();

    let before_operation = operation("tenant-a", "operation-before", "before");
    operations.persist_operation(&before_operation)?;
    let before_claim = operations
        .claim_next_operation("tenant-a", &policy)?
        .expect("pre-start operation must enter dispatching");
    assert_eq!(before_claim.operation_id, before_operation.id);

    let endpoint_token = b"scheduler-webhook-token";
    let endpoint_id = webhooks.register_webhook_endpoint(
        "tenant-a",
        &ProviderBindingId::new("binding-a")?,
        endpoint_token,
    )?;
    let endpoint = webhooks.resolve_webhook_endpoint(endpoint_token)?;
    let (before_inbox, duplicate) = webhooks.record_verified_webhook(
        &endpoint,
        "webhook-before",
        "{}",
        br#"{"phase":"before"}"#,
    )?;
    assert!(!duplicate);
    let before_webhook = webhooks
        .claim_next_webhook("tenant-a")?
        .expect("pre-start webhook must enter processing");
    assert_eq!(before_webhook.inbox_id, before_inbox);

    let recovery_id = scheduler.begin_startup_recovery_snapshot()?;

    let after_operation = operation("tenant-a", "operation-after", "after");
    operations.persist_operation(&after_operation)?;
    let after_claim = operations
        .claim_next_operation("tenant-a", &policy)?
        .expect("post-snapshot operation must enter dispatching");
    assert_eq!(after_claim.operation_id, after_operation.id);

    let (after_inbox, duplicate) = webhooks.record_verified_webhook(
        &endpoint,
        "webhook-after",
        "{}",
        br#"{"phase":"after"}"#,
    )?;
    assert!(!duplicate);
    let after_webhook = webhooks
        .claim_next_webhook("tenant-a")?
        .expect("post-snapshot webhook must enter processing");
    assert_eq!(after_webhook.inbox_id, after_inbox);

    let page = scheduler.recover_next_startup_snapshot_page(&recovery_id)?;
    assert_eq!(page.tenants, 1);
    assert_eq!(page.recovered_operations, 1);
    assert_eq!(page.recovered_webhooks, 1);

    assert_eq!(
        operation_state(&fixture.pool, "operation-before").await?,
        "unknown_outcome"
    );
    assert_eq!(
        operation_state(&fixture.pool, "operation-after").await?,
        "dispatching"
    );
    assert_eq!(
        webhook_status(&fixture.pool, "webhook-before").await?,
        "verified"
    );
    assert_eq!(
        webhook_status(&fixture.pool, "webhook-after").await?,
        "processing"
    );

    let operation_attempt_state: String = sqlx::query_scalar(
        "SELECT state FROM external_operation_attempts
         WHERE tenant_id='tenant-a' AND operation_id='operation-before'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(operation_attempt_state, "unknown_outcome");

    let webhook_attempt_state: String = sqlx::query_scalar(
        "SELECT state FROM webhook_processing_attempts
         WHERE tenant_id='tenant-a' AND inbox_id=$1",
    )
    .bind(before_inbox.as_str())
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(webhook_attempt_state, "retryable_failure");

    let recovery_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM external_operation_runtime_events
         WHERE tenant_id='tenant-a'
           AND operation_id='operation-before'
           AND event_type='recovered_after_restart'
           AND classification='worker_restarted_after_dispatch'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(recovery_events, 1);

    let remaining_snapshot_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM integration_startup_recovery_snapshot
         WHERE scheduler_id='fixture_integration_worker' AND recovery_id=$1",
    )
    .bind(&recovery_id)
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(remaining_snapshot_rows, 0);
    assert_eq!(
        scheduler.recover_next_startup_snapshot_page(&recovery_id)?,
        Default::default()
    );

    for (tenant_id, id) in [("tenant-b", "operation-b"), ("tenant-c", "operation-c")] {
        operations.persist_operation(&operation(tenant_id, id, id))?;
    }

    sqlx::query(
        "INSERT INTO integration_scheduler_cursor
            (scheduler_id,tenant_id,updated_at)
         VALUES ('fixture_integration_worker','tenant-a',$1)
         ON CONFLICT (scheduler_id) DO UPDATE SET
            tenant_id=EXCLUDED.tenant_id,
            updated_at=EXCLUDED.updated_at",
    )
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(&fixture.pool)
    .await?;

    assert_eq!(
        scheduler.next_tenant_work_page()?,
        vec!["tenant-b".to_owned(), "tenant-c".to_owned()]
    );
    assert_eq!(
        scheduler.next_tenant_work_page()?,
        vec![
            "tenant-a".to_owned(),
            "tenant-b".to_owned(),
            "tenant-c".to_owned(),
        ]
    );

    let cursor: Option<String> = sqlx::query_scalar(
        "SELECT tenant_id FROM integration_scheduler_cursor
         WHERE scheduler_id='fixture_integration_worker'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(cursor.as_deref(), Some("tenant-c"));

    let _worker =
        FixtureIntegrationWorker::from_postgres(fixture.pool.clone(), Arc::new(NoopMetrics))?;
    assert_eq!(endpoint_id, endpoint.endpoint_id);

    fixture.cleanup().await
}

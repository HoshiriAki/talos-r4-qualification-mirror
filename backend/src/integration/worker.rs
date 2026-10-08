//! Scheduled fixture-only Integration Fabric worker.
//!
//! This worker consumes only records that the durable Store has already
//! admitted. It owns no business repository and has no real Provider adapter:
//! ExternalOperation dispatch is a deterministic fixture result, while inbound
//! Webhook mapping ends at a provider-neutral application-port acknowledgement.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use super::runtime::{
    ClaimedOperation, DispatchResult, OperationDispatcher, OperationRuntime,
    OperationRuntimePolicy, RunOnceResult,
};
use super::scheduler_persistence_contract::IntegrationSchedulerPersistence;
#[cfg(feature = "postgres")]
use super::scheduler_persistence_postgres::PostgresIntegrationSchedulerPersistence;
use super::store::IntegrationStore;
use super::transport::TransportFailure;
use super::types::IntegrationError;
use super::webhook::{
    CanonicalWebhookEvent, ClaimedWebhook, WebhookApplicationPort, WebhookEventMapper,
    WebhookHandlingError, WebhookRuntime,
};
use crate::observability::{MetricsSink, NoopMetrics, RecoveryOutcome, RecoverySource};

const MAX_WORK_PER_TENANT_TICK: usize = 25;
// Webhook retry scheduling does not yet persist a next-at timestamp. Process at
// most one inbound item per tenant/tick so a retryable item cannot spin in a
// hot loop; WebhookRuntime separately enforces a finite attempt budget.
const MAX_WEBHOOKS_PER_TENANT_TICK: usize = 1;
const INTEGRATION_WORKER_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IntegrationWorkerReport {
    pub tenants: u64,
    pub recovered_tenants: u64,
    pub recovered_operations: u64,
    pub recovered_webhooks: u64,
    pub dispatched_operations: u64,
    pub processed_webhooks: u64,
    pub deferred_operations: u64,
}

/// A Stage-2 fixture dispatcher. It makes no network request and never sees a
/// resolved secret; records can reach this boundary only after the Store has
/// verified a fixture-ready binding.
struct FixtureOperationDispatcher;

#[async_trait]
impl OperationDispatcher for FixtureOperationDispatcher {
    async fn dispatch(
        &self,
        operation: &ClaimedOperation,
    ) -> Result<DispatchResult, TransportFailure> {
        Ok(DispatchResult::Succeeded {
            provider_result_ref: Some(format!(
                "fixture-operation:{}:{}",
                operation.operation_id.as_str(),
                operation.attempt_number
            )),
        })
    }
}

struct FixtureWebhookMapper;

impl WebhookEventMapper for FixtureWebhookMapper {
    fn map(&self, webhook: &ClaimedWebhook) -> Result<CanonicalWebhookEvent, WebhookHandlingError> {
        if !webhook
            .endpoint
            .provider_id
            .as_str()
            .starts_with("fixture-")
        {
            return Err(WebhookHandlingError::Permanent(
                "fixture_provider_required".into(),
            ));
        }
        let payload = serde_json::from_slice::<serde_json::Value>(&webhook.raw_payload)
            .map_err(|_| WebhookHandlingError::Permanent("malformed_fixture_payload".into()))?;
        let event_type = payload
            .get("eventType")
            .or_else(|| payload.get("event_type"))
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| WebhookHandlingError::Permanent("fixture_event_type_missing".into()))?;
        Ok(CanonicalWebhookEvent {
            tenant_id: webhook.tenant_id.clone(),
            provider_id: webhook.endpoint.provider_id.clone(),
            binding_id: webhook.endpoint.binding_id.clone(),
            provider_event_id: webhook.provider_event_id.clone(),
            event_type: event_type.to_owned(),
            payload_hash: hex::encode(Sha256::digest(&webhook.raw_payload)),
        })
    }
}

/// The provider-neutral receipt port deliberately confirms only durable
/// mapping. A concrete business connector must add its own Registry-governed
/// application port; this generic fixture path cannot mutate business tables.
struct FixtureWebhookReceiptPort;

#[async_trait]
impl WebhookApplicationPort for FixtureWebhookReceiptPort {
    async fn dispatch(&self, _event: CanonicalWebhookEvent) -> Result<(), WebhookHandlingError> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct FixtureIntegrationWorker {
    scheduler: Arc<dyn IntegrationSchedulerPersistence>,
    operations: OperationRuntime,
    webhooks: WebhookRuntime,
    metrics: Arc<dyn MetricsSink>,
}

impl FixtureIntegrationWorker {
    pub fn new(store: IntegrationStore) -> Result<Self, IntegrationError> {
        Self::new_with_metrics(store, Arc::new(NoopMetrics))
    }

    pub(crate) fn new_with_metrics(
        store: IntegrationStore,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, IntegrationError> {
        let operations = OperationRuntime::new_with_metrics(
            store.clone(),
            OperationRuntimePolicy::default(),
            metrics.clone(),
        )?;
        let webhooks = WebhookRuntime::new_with_metrics(store.clone(), metrics.clone());
        Ok(Self::from_components(
            Arc::new(store),
            operations,
            webhooks,
            metrics,
        ))
    }

    pub(crate) fn from_components(
        scheduler: Arc<dyn IntegrationSchedulerPersistence>,
        operations: OperationRuntime,
        webhooks: WebhookRuntime,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self {
            scheduler,
            operations,
            webhooks,
            metrics,
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn from_postgres(
        pool: sqlx::PgPool,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, IntegrationError> {
        let operations =
            OperationRuntime::from_postgres(pool.clone(), OperationRuntimePolicy::default())?;
        let webhooks = WebhookRuntime::from_postgres(pool.clone());
        Ok(Self::from_components(
            Arc::new(PostgresIntegrationSchedulerPersistence::new(pool)),
            operations,
            webhooks,
            metrics,
        ))
    }

    /// Captures and drains all startup-era in-flight work before this worker
    /// begins normal scheduling. The snapshot gives recovery an explicit
    /// ownership boundary: periodic ticks never classify current in-flight
    /// work as crash residue.
    pub async fn run_startup(&self) -> Result<IntegrationWorkerReport, IntegrationError> {
        self.metrics
            .recovery(RecoverySource::StartupSnapshot, RecoveryOutcome::Started);
        let recovery_id = match self.scheduler.begin_startup_recovery_snapshot() {
            Ok(recovery_id) => recovery_id,
            Err(error) => {
                self.metrics
                    .recovery(RecoverySource::StartupSnapshot, RecoveryOutcome::Failed);
                return Err(error);
            }
        };
        let mut report = IntegrationWorkerReport::default();
        loop {
            let page = match self
                .scheduler
                .recover_next_startup_snapshot_page(&recovery_id)
            {
                Ok(page) => page,
                Err(error) => {
                    self.metrics
                        .recovery(RecoverySource::StartupSnapshot, RecoveryOutcome::Failed);
                    return Err(error);
                }
            };
            if page.tenants == 0 {
                self.metrics.recovery(
                    RecoverySource::StartupSnapshot,
                    if report.recovered_operations + report.recovered_webhooks == 0 {
                        RecoveryOutcome::Empty
                    } else {
                        RecoveryOutcome::Recovered
                    },
                );
                break;
            }
            self.metrics
                .recovery(RecoverySource::TenantPage, RecoveryOutcome::Recovered);
            for _ in 0..page.recovered_operations {
                self.metrics
                    .recovery(RecoverySource::UnknownOutcome, RecoveryOutcome::Recovered);
            }
            for _ in 0..page.recovered_webhooks {
                self.metrics
                    .recovery(RecoverySource::Webhook, RecoveryOutcome::Recovered);
            }
            report.recovered_tenants += page.tenants;
            report.recovered_operations += page.recovered_operations;
            report.recovered_webhooks += page.recovered_webhooks;
        }
        Ok(report)
    }

    pub async fn run_periodic_tick(&self) -> Result<IntegrationWorkerReport, IntegrationError> {
        self.run_scheduled_page().await
    }

    async fn run_scheduled_page(&self) -> Result<IntegrationWorkerReport, IntegrationError> {
        self.metrics
            .recovery(RecoverySource::ScheduledPage, RecoveryOutcome::Started);
        let tenants = match self.scheduler.next_tenant_work_page() {
            Ok(tenants) => tenants,
            Err(error) => {
                self.metrics
                    .recovery(RecoverySource::ScheduledPage, RecoveryOutcome::Failed);
                return Err(error);
            }
        };
        let mut report = IntegrationWorkerReport {
            tenants: tenants.len() as u64,
            ..Default::default()
        };
        if tenants.is_empty() {
            self.metrics
                .recovery(RecoverySource::ScheduledPage, RecoveryOutcome::Empty);
            return Ok(report);
        }
        for tenant_id in tenants {
            for _ in 0..MAX_WORK_PER_TENANT_TICK {
                match self
                    .operations
                    .run_once(&tenant_id, &FixtureOperationDispatcher)
                    .await
                {
                    Ok(RunOnceResult::Dispatched { .. }) => report.dispatched_operations += 1,
                    Ok(RunOnceResult::Idle) => break,
                    Ok(RunOnceResult::Deferred(_))
                    | Err(
                        IntegrationError::BindingRevisionStale
                        | IntegrationError::BindingUnavailable
                        | IntegrationError::InstanceNotReady
                        | IntegrationError::ModeBlocked
                        | IntegrationError::CircuitOpen
                        | IntegrationError::ConcurrencyLimited
                        | IntegrationError::RateLimited
                        | IntegrationError::OperationNotDue,
                    ) => {
                        report.deferred_operations += 1;
                        break;
                    }
                    Err(error) => {
                        self.metrics
                            .recovery(RecoverySource::ScheduledPage, RecoveryOutcome::Failed);
                        return Err(error);
                    }
                }
            }
            for _ in 0..MAX_WEBHOOKS_PER_TENANT_TICK {
                let webhook = match self
                    .webhooks
                    .run_once(
                        &tenant_id,
                        &FixtureWebhookMapper,
                        &FixtureWebhookReceiptPort,
                    )
                    .await
                {
                    Ok(webhook) => webhook,
                    Err(error) => {
                        self.metrics
                            .recovery(RecoverySource::ScheduledPage, RecoveryOutcome::Failed);
                        return Err(error);
                    }
                };
                match webhook {
                    Some(_) => report.processed_webhooks += 1,
                    None => break,
                }
            }
        }
        self.metrics
            .recovery(RecoverySource::ScheduledPage, RecoveryOutcome::Recovered);
        Ok(report)
    }

    pub fn spawn_periodic(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            if self.run_startup().await.is_err() {
                tracing::error!(
                    error_class = "startup_recovery_failed",
                    "integration fixture worker startup failed"
                );
            }
            let mut interval = tokio::time::interval(INTEGRATION_WORKER_INTERVAL);
            loop {
                interval.tick().await;
                if self.run_periodic_tick().await.is_err() {
                    tracing::error!(
                        error_class = "scheduled_page_failed",
                        "integration fixture worker periodic tick failed"
                    );
                }
            }
        })
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use async_trait::async_trait;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use super::FixtureIntegrationWorker;
    use crate::db::migrations::run_migrations;
    use crate::integration::operation::ExternalOperation;
    use crate::integration::store::IntegrationStore;
    use crate::integration::types::{
        CapabilityId, ExternalOperationId, IntegrationError, ProviderBinding, ProviderBindingId,
        ProviderHealth, ProviderId, ProviderInstance, ProviderInstanceId, ProviderLifecycle,
        ProviderManifest, ProviderReadiness,
    };
    use crate::integration::webhook::{WebhookReceiptInput, WebhookRuntime, WebhookVerifier};
    use crate::observability::RuntimeMetrics;

    fn store_with_pool() -> (
        IntegrationStore,
        ProviderBinding,
        CapabilityId,
        Pool<SqliteConnectionManager>,
    ) {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        run_migrations(&pool.get().unwrap()).unwrap();
        let store = IntegrationStore::new(pool.clone());
        let provider = ProviderId::new("fixture-worker").unwrap();
        let capability = CapabilityId::new("fixture.worker").unwrap();
        store
            .save_manifest(&ProviderManifest {
                provider_id: provider.clone(),
                version: "1".into(),
                capabilities: BTreeSet::from([capability.clone()]),
                config_schema: vec![],
                secret_schema: vec![],
                api_versions: BTreeMap::new(),
                webhook_types: BTreeSet::from(["fixture.worker.updated".into()]),
                simulation_capabilities: BTreeSet::from([capability.clone()]),
                readiness: ProviderReadiness::Fixture,
                compatibility: BTreeMap::new(),
            })
            .unwrap();
        let instance = ProviderInstance {
            id: ProviderInstanceId::new("worker-instance").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_id: provider.clone(),
            manifest_version: "1".into(),
            config_revision: "revision-a".into(),
            config: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            lifecycle: ProviderLifecycle::Active,
            health: ProviderHealth::Ready,
            readiness: ProviderReadiness::Fixture,
        };
        store.save_instance(&instance).unwrap();
        let binding = ProviderBinding {
            id: ProviderBindingId::new("worker-binding").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_instance_id: instance.id,
            capability: capability.clone(),
            config_revision: "revision-a".into(),
            enabled: true,
        };
        store.save_binding(&binding, "operator-a").unwrap();
        store
            .register_webhook_endpoint("tenant-a", &binding.id, b"worker-token")
            .unwrap();
        (store, binding, capability, pool)
    }

    fn store() -> (IntegrationStore, ProviderBinding, CapabilityId) {
        let (store, binding, capability, _) = store_with_pool();
        (store, binding, capability)
    }

    fn context_for(tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("worker-test", "admin").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("worker-revision").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new("worker-request").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn context() -> ExecutionContext {
        context_for("tenant-a")
    }

    fn create_tenant_binding(
        store: &IntegrationStore,
        capability: &CapabilityId,
        tenant_id: &str,
        suffix: &str,
    ) -> ProviderBinding {
        let instance = ProviderInstance {
            id: ProviderInstanceId::new(format!("worker-instance-{suffix}")).unwrap(),
            tenant_id: tenant_id.into(),
            provider_id: ProviderId::new("fixture-worker").unwrap(),
            manifest_version: "1".into(),
            config_revision: "revision-a".into(),
            config: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            lifecycle: ProviderLifecycle::Active,
            health: ProviderHealth::Ready,
            readiness: ProviderReadiness::Fixture,
        };
        store.save_instance(&instance).unwrap();
        let binding = ProviderBinding {
            id: ProviderBindingId::new(format!("worker-binding-{suffix}")).unwrap(),
            tenant_id: tenant_id.into(),
            provider_instance_id: instance.id,
            capability: capability.clone(),
            config_revision: "revision-a".into(),
            enabled: true,
        };
        store
            .save_binding(&binding, "worker-scheduler-test")
            .unwrap();
        binding
    }

    fn admit_ready_operation(
        store: &IntegrationStore,
        capability: &CapabilityId,
        tenant_id: &str,
        operation_id: &str,
    ) -> ExternalOperation {
        let resolved = store
            .catalog()
            .unwrap()
            .resolve(&context_for(tenant_id), capability)
            .unwrap();
        let mut operation = ExternalOperation::planned(
            ExternalOperationId::new(operation_id).unwrap(),
            &resolved,
            "fixture.scheduler",
            format!("scheduler-idempotency-{operation_id}"),
            format!("scheduler-request-{operation_id}"),
        )
        .unwrap();
        operation.ready().unwrap();
        store.persist_operation(&operation).unwrap();
        operation
    }

    struct AcceptingVerifier;

    #[async_trait]
    impl WebhookVerifier for AcceptingVerifier {
        async fn verify(
            &self,
            _endpoint: &crate::integration::webhook::WebhookEndpointContext,
            _headers_json: &str,
            _raw_payload: &[u8],
        ) -> Result<(), IntegrationError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn startup_recovers_claimed_webhooks_and_processes_fixture_work() {
        let (store, _binding, capability) = store();
        let resolved = store
            .catalog()
            .unwrap()
            .resolve(&context(), &capability)
            .unwrap();
        let mut operation = ExternalOperation::planned(
            ExternalOperationId::new("worker-operation").unwrap(),
            &resolved,
            "fixture.refund",
            "worker-idempotency",
            "worker-request-hash",
        )
        .unwrap();
        operation.ready().unwrap();
        store.persist_operation(&operation).unwrap();

        let webhook = WebhookRuntime::new(store.clone());
        let receipt = webhook
            .receive(
                WebhookReceiptInput {
                    endpoint_token: b"worker-token".to_vec(),
                    provider_event_id: "worker-event".into(),
                    verification_headers_json: "{}".into(),
                    stored_headers_json: "{}".into(),
                    raw_payload: br#"{"eventType":"fixture.worker.updated"}"#.to_vec(),
                },
                &AcceptingVerifier,
            )
            .await
            .unwrap();
        assert!(store.claim_next_webhook("tenant-a").unwrap().is_some());

        let worker = FixtureIntegrationWorker::new(store.clone()).unwrap();
        let startup = worker.run_startup().await.unwrap();
        assert_eq!(startup.recovered_tenants, 1);
        assert_eq!(startup.recovered_webhooks, 1);
        assert_eq!(startup.dispatched_operations, 0);
        assert_eq!(startup.processed_webhooks, 0);

        let periodic = worker.run_periodic_tick().await.unwrap();
        assert_eq!(periodic.dispatched_operations, 1);
        assert_eq!(periodic.processed_webhooks, 1);
        assert!(store.claim_next_webhook("tenant-a").unwrap().is_none());
        assert!(store.stored_webhook_headers(&receipt.inbox_id).is_ok());
        assert_eq!(
            store.operation_runtime_metrics("tenant-a").unwrap().ready,
            0
        );
    }

    #[tokio::test]
    async fn empty_startup_and_scheduled_runs_each_emit_one_terminal_recovery_outcome() {
        let (store, _, _) = store();
        let metrics = Arc::new(RuntimeMetrics::default());
        let worker = FixtureIntegrationWorker::new_with_metrics(store, metrics.clone()).unwrap();

        worker.run_startup().await.unwrap();
        worker.run_periodic_tick().await.unwrap();

        let output = metrics.render();
        assert!(output.contains("source=\"startup_snapshot\",outcome=\"started\"} 1"));
        assert!(output.contains("source=\"startup_snapshot\",outcome=\"empty\"} 1"));
        assert!(output.contains("source=\"scheduled_page\",outcome=\"started\"} 1"));
        assert!(output.contains("source=\"scheduled_page\",outcome=\"empty\"} 1"));
        for source in ["startup_snapshot", "scheduled_page"] {
            assert!(!output.contains(&format!("source=\"{source}\",outcome=\"recovered\"")));
            assert!(!output.contains(&format!("source=\"{source}\",outcome=\"failed\"")));
        }
    }

    #[tokio::test]
    async fn startup_and_scheduled_persistence_failures_emit_terminal_failed_metrics() {
        let (store, _, _, pool) = store_with_pool();
        let metrics = Arc::new(RuntimeMetrics::default());
        let worker = FixtureIntegrationWorker::new_with_metrics(store, metrics.clone()).unwrap();
        pool.get()
            .unwrap()
            .execute_batch("DROP TABLE external_operations")
            .unwrap();

        assert_eq!(
            worker.run_startup().await.unwrap_err(),
            IntegrationError::Persistence
        );
        let output = metrics.render();
        assert!(output.contains("source=\"startup_snapshot\",outcome=\"started\"} 1"));
        assert!(output.contains("source=\"startup_snapshot\",outcome=\"failed\"} 1"));
        assert!(!output.contains("source=\"startup_snapshot\",outcome=\"empty\""));
        assert!(!output.contains("source=\"startup_snapshot\",outcome=\"recovered\""));

        let (store, _, _, pool) = store_with_pool();
        let metrics = Arc::new(RuntimeMetrics::default());
        let worker = FixtureIntegrationWorker::new_with_metrics(store, metrics.clone()).unwrap();
        pool.get()
            .unwrap()
            .execute_batch("DROP TABLE integration_scheduler_cursor")
            .unwrap();

        assert_eq!(
            worker.run_periodic_tick().await.unwrap_err(),
            IntegrationError::Persistence
        );
        let output = metrics.render();
        assert!(output.contains("source=\"scheduled_page\",outcome=\"started\"} 1"));
        assert!(output.contains("source=\"scheduled_page\",outcome=\"failed\"} 1"));
        assert!(!output.contains("source=\"scheduled_page\",outcome=\"empty\""));
        assert!(!output.contains("source=\"scheduled_page\",outcome=\"recovered\""));
    }

    #[tokio::test]
    async fn startup_snapshot_drains_over_one_hundred_tenants_before_periodic_scheduling() {
        let (store, _binding, capability) = store();
        let mut tail_binding = None;
        for index in 0..=100 {
            let tenant_id = format!("tenant-{index:03}");
            let binding =
                create_tenant_binding(&store, &capability, &tenant_id, &format!("{index:03}"));
            let operation = admit_ready_operation(
                &store,
                &capability,
                &tenant_id,
                &format!("scheduler-operation-{index:03}"),
            );
            store.start_attempt(&tenant_id, &operation.id).unwrap();
            if tenant_id == "tenant-100" {
                tail_binding = Some(binding);
            }
        }

        let tail_binding = tail_binding.unwrap();
        store
            .register_webhook_endpoint("tenant-100", &tail_binding.id, b"worker-tail-token")
            .unwrap();
        let tail_webhook = WebhookRuntime::new(store.clone());
        tail_webhook
            .receive(
                WebhookReceiptInput {
                    endpoint_token: b"worker-tail-token".to_vec(),
                    provider_event_id: "worker-tail-event".into(),
                    verification_headers_json: "{}".into(),
                    stored_headers_json: "{}".into(),
                    raw_payload: br#"{"eventType":"fixture.worker.updated"}"#.to_vec(),
                },
                &AcceptingVerifier,
            )
            .await
            .unwrap();
        assert!(store.claim_next_webhook("tenant-100").unwrap().is_some());

        let metrics = Arc::new(RuntimeMetrics::default());
        let worker =
            FixtureIntegrationWorker::new_with_metrics(store.clone(), metrics.clone()).unwrap();
        let startup = worker.run_startup().await.unwrap();
        assert_eq!(startup.recovered_tenants, 101);
        assert_eq!(startup.recovered_operations, 101);
        assert_eq!(startup.recovered_webhooks, 1);
        assert_eq!(startup.dispatched_operations, 0);
        assert_eq!(startup.processed_webhooks, 0);
        let tail_metrics = store.operation_runtime_metrics("tenant-100").unwrap();
        assert_eq!(tail_metrics.dispatching, 0);
        assert_eq!(tail_metrics.unknown_outcome, 1);
        assert_eq!(tail_metrics.ready, 0);
        assert_eq!(tail_metrics.retryable_failure, 0);

        // Once startup recovery is complete, the ordinary persisted scheduler
        // resumes and may process the recovered tail webhook without treating
        // current periodic work as crash residue.
        admit_ready_operation(
            &store,
            &capability,
            "tenant-000",
            "scheduler-operation-wrap",
        );
        create_tenant_binding(&store, &capability, "tenant-current", "current");
        let current_process_operation = admit_ready_operation(
            &store,
            &capability,
            "tenant-current",
            "scheduler-operation-current-process",
        );
        store
            .start_attempt("tenant-current", &current_process_operation.id)
            .unwrap();
        let periodic = worker.run_periodic_tick().await.unwrap();
        assert_eq!(periodic.tenants, 3);
        assert_eq!(periodic.dispatched_operations, 1);
        assert_eq!(periodic.processed_webhooks, 1);
        assert_eq!(
            store.operation_runtime_metrics("tenant-000").unwrap().ready,
            0
        );
        let current_metrics = store.operation_runtime_metrics("tenant-current").unwrap();
        assert_eq!(current_metrics.dispatching, 1);
        assert_eq!(current_metrics.unknown_outcome, 0);
        assert_eq!(current_metrics.ready, 0);
        assert_eq!(current_metrics.retryable_failure, 0);
        assert!(store.claim_next_webhook("tenant-100").unwrap().is_none());
        let output = metrics.render();
        assert!(output.contains("source=\"startup_snapshot\""));
        assert!(output.contains("source=\"tenant_page\""));
        assert!(output.contains("source=\"unknown_outcome\",outcome=\"recovered\"} 101"));
        assert!(output.contains("source=\"webhook\",outcome=\"recovered\"} 1"));
        assert!(output.contains("source=\"scheduled_page\""));
        assert!(!output.contains("tenant-100"));
    }

    #[tokio::test]
    async fn deferred_tenant_does_not_block_another_tenant_in_the_same_page() {
        let (store, binding, capability) = store();
        let deferred = admit_ready_operation(
            &store,
            &capability,
            "tenant-a",
            "scheduler-operation-deferred",
        );
        create_tenant_binding(&store, &capability, "tenant-b", "other");
        let other =
            admit_ready_operation(&store, &capability, "tenant-b", "scheduler-operation-other");
        store
            .open_circuit_for_test("tenant-a", &binding.id, "9998-01-01T00:00:00Z")
            .unwrap();

        let report = FixtureIntegrationWorker::new(store.clone())
            .unwrap()
            .run_periodic_tick()
            .await
            .unwrap();
        assert_eq!(report.tenants, 2);
        assert_eq!(report.dispatched_operations, 1);
        assert_eq!(
            store.operation_runtime_metrics("tenant-a").unwrap().ready,
            1
        );
        assert_eq!(
            store.operation_runtime_metrics("tenant-b").unwrap().ready,
            0
        );
        assert_eq!(
            deferred.state,
            crate::integration::operation::OperationState::Ready
        );
        assert_eq!(
            other.state,
            crate::integration::operation::OperationState::Ready
        );
    }
}

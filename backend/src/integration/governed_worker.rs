//! R4-P5 production-facing Integration worker.
//!
//! This keeps the Stage-2 scheduling/recovery semantics but replaces the old
//! fixture webhook receipt sink with a normalized R4-P3 Event Lane handoff.
//! External operations remain fixture-only in R4 Core; real providers are
//! separate Official Plugin release trains.

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
    CanonicalWebhookEvent, ClaimedWebhook, WebhookEventMapper, WebhookHandlingError, WebhookRuntime,
};
use super::webhook_event::{InterconnectWebhookEventPort, WebhookEventLane};
use super::worker::IntegrationWorkerReport;
use crate::observability::{MetricsSink, NoopMetrics, RecoveryOutcome, RecoverySource};

const MAX_WORK_PER_TENANT_TICK: usize = 25;
const MAX_WEBHOOKS_PER_TENANT_TICK: usize = 1;
const INTEGRATION_WORKER_INTERVAL: Duration = Duration::from_secs(15);

/// R4 Core still executes only deterministic fixture effects. Networked
/// provider dispatch is admitted later through Official Plugin + P5 egress.
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
        // P5 deliberately does not invent a second subject grammar. The Event
        // publisher validates this value through the P3 Subject constructor.
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

#[derive(Clone)]
pub struct GovernedIntegrationWorker {
    scheduler: Arc<dyn IntegrationSchedulerPersistence>,
    operations: OperationRuntime,
    webhooks: WebhookRuntime,
    webhook_events: InterconnectWebhookEventPort,
    metrics: Arc<dyn MetricsSink>,
}

impl GovernedIntegrationWorker {
    pub fn new(
        store: IntegrationStore,
        event_lane: WebhookEventLane,
    ) -> Result<Self, IntegrationError> {
        Self::new_with_metrics(store, event_lane, Arc::new(NoopMetrics))
    }

    pub(crate) fn new_with_metrics(
        store: IntegrationStore,
        event_lane: WebhookEventLane,
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
            event_lane,
            metrics,
        ))
    }

    pub(crate) fn from_components(
        scheduler: Arc<dyn IntegrationSchedulerPersistence>,
        operations: OperationRuntime,
        webhooks: WebhookRuntime,
        event_lane: WebhookEventLane,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self {
            scheduler,
            operations,
            webhooks,
            webhook_events: InterconnectWebhookEventPort::new(event_lane),
            metrics,
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn from_postgres_with_metrics(
        pool: sqlx::PgPool,
        event_lane: WebhookEventLane,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, IntegrationError> {
        let operations = OperationRuntime::from_postgres_with_metrics(
            pool.clone(),
            OperationRuntimePolicy::default(),
            metrics.clone(),
        )?;
        let webhooks = WebhookRuntime::from_postgres_with_metrics(pool.clone(), metrics.clone());
        Ok(Self::from_components(
            Arc::new(PostgresIntegrationSchedulerPersistence::new(pool)),
            operations,
            webhooks,
            event_lane,
            metrics,
        ))
    }

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
                    .run_once(&tenant_id, &FixtureWebhookMapper, &self.webhook_events)
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
                    "governed integration worker startup failed"
                );
            }
            let mut interval = tokio::time::interval(INTEGRATION_WORKER_INTERVAL);
            loop {
                interval.tick().await;
                if self.run_periodic_tick().await.is_err() {
                    tracing::error!(
                        error_class = "scheduled_page_failed",
                        "governed integration worker periodic tick failed"
                    );
                }
            }
        })
    }
}

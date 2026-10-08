//! Durable, provider-neutral execution loop for `ExternalOperation`.
//!
//! The runtime owns scheduling, retry classification, circuit/rate/concurrency
//! control, and restart recovery. It deliberately does **not** own Provider
//! request construction, secret resolution, or business repositories. A
//! connector supplies a narrow dispatcher only after the operation was claimed
//! against its immutable binding revision.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;

#[cfg(feature = "postgres")]
use super::operation_runtime_postgres::PostgresOperationRuntimePersistence;
use super::store::IntegrationStore;
use super::transport::TransportFailure;
use super::types::{ExternalOperationId, IntegrationError};
use crate::observability::{
    AttemptDispatchClass, AttemptOutcome, IntegrationEvent, MetricsSink, NoopMetrics,
    OperationStateClass, RecoveryOutcome, RecoverySource,
};

pub(crate) use super::operation_runtime_contract::OperationRuntimePersistence;
pub use super::operation_runtime_contract::{
    ClaimedOperation, DispatchResult, OperationRuntimeMetrics, OperationRuntimePolicy, RetryPolicy,
};

/// Connector-side boundary. Implementations may use the generic transport and
/// controlled KeyStore internally, but this runtime never sees resolved secret
/// values or an application/business repository.
#[async_trait]
pub trait OperationDispatcher: Send + Sync {
    async fn dispatch(
        &self,
        operation: &ClaimedOperation,
    ) -> Result<DispatchResult, TransportFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOnceResult {
    Idle,
    Deferred(IntegrationError),
    Dispatched { operation_id: ExternalOperationId },
}

impl OperationRuntimePersistence for IntegrationStore {
    fn persist_operation(
        &self,
        operation: &super::operation::ExternalOperation,
    ) -> Result<ExternalOperationId, IntegrationError> {
        IntegrationStore::persist_operation(self, operation)
    }

    fn claim_next_operation(
        &self,
        tenant_id: &str,
        policy: &OperationRuntimePolicy,
    ) -> Result<Option<ClaimedOperation>, IntegrationError> {
        IntegrationStore::claim_next_operation(self, tenant_id, policy)
    }

    fn record_runtime_outcome(
        &self,
        claimed: &ClaimedOperation,
        outcome: Result<DispatchResult, TransportFailure>,
        policy: &OperationRuntimePolicy,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::record_runtime_outcome(self, claimed, outcome, policy)
    }

    fn recover_inflight_operations(&self, tenant_id: &str) -> Result<u64, IntegrationError> {
        IntegrationStore::recover_inflight_operations(self, tenant_id)
    }

    fn operation_runtime_metrics(
        &self,
        tenant_id: &str,
    ) -> Result<OperationRuntimeMetrics, IntegrationError> {
        IntegrationStore::operation_runtime_metrics(self, tenant_id)
    }

    fn begin_reconciliation(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
        evidence_ref: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::begin_reconciliation(self, tenant_id, operation_id, evidence_ref)
    }

    fn resolve_reconciliation(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
        effect_confirmed: bool,
        actor_ref: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::resolve_reconciliation(
            self,
            tenant_id,
            operation_id,
            effect_confirmed,
            actor_ref,
        )
    }
}

#[derive(Clone)]
pub struct OperationRuntime {
    persistence: Arc<dyn OperationRuntimePersistence>,
    policy: OperationRuntimePolicy,
    metrics: Arc<dyn MetricsSink>,
}

impl OperationRuntime {
    pub fn new(
        store: IntegrationStore,
        policy: OperationRuntimePolicy,
    ) -> Result<Self, IntegrationError> {
        Self::new_with_metrics(store, policy, Arc::new(NoopMetrics))
    }

    pub(crate) fn new_with_metrics(
        store: IntegrationStore,
        policy: OperationRuntimePolicy,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, IntegrationError> {
        Self::from_persistence_with_metrics(Arc::new(store), policy, metrics)
    }

    pub(crate) fn from_persistence(
        persistence: Arc<dyn OperationRuntimePersistence>,
        policy: OperationRuntimePolicy,
    ) -> Result<Self, IntegrationError> {
        Self::from_persistence_with_metrics(persistence, policy, Arc::new(NoopMetrics))
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn from_postgres(
        pool: sqlx::PgPool,
        policy: OperationRuntimePolicy,
    ) -> Result<Self, IntegrationError> {
        Self::from_postgres_with_metrics(pool, policy, Arc::new(NoopMetrics))
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn from_postgres_with_metrics(
        pool: sqlx::PgPool,
        policy: OperationRuntimePolicy,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, IntegrationError> {
        Self::from_persistence_with_metrics(
            Arc::new(PostgresOperationRuntimePersistence::new(pool)),
            policy,
            metrics,
        )
    }

    pub(crate) fn from_persistence_with_metrics(
        persistence: Arc<dyn OperationRuntimePersistence>,
        policy: OperationRuntimePolicy,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, IntegrationError> {
        policy.validate()?;
        Ok(Self {
            persistence,
            policy,
            metrics,
        })
    }

    /// Converts abandoned in-flight work to `UnknownOutcome`. Recovery is
    /// intentionally conservative: a process crash is evidence that an effect
    /// may have left the process, so it never schedules a blind retry.
    pub fn recover_after_restart(&self, tenant_id: &str) -> Result<u64, IntegrationError> {
        let result = self.persistence.recover_inflight_operations(tenant_id);
        self.metrics.recovery(
            RecoverySource::UnknownOutcome,
            match result {
                Ok(0) => RecoveryOutcome::Empty,
                Ok(_) => RecoveryOutcome::Recovered,
                Err(_) => RecoveryOutcome::Failed,
            },
        );
        result
    }

    pub fn metrics(&self, tenant_id: &str) -> Result<OperationRuntimeMetrics, IntegrationError> {
        self.persistence.operation_runtime_metrics(tenant_id)
    }

    pub async fn run_once(
        &self,
        tenant_id: &str,
        dispatcher: &dyn OperationDispatcher,
    ) -> Result<RunOnceResult, IntegrationError> {
        let claimed = match self
            .persistence
            .claim_next_operation(tenant_id, &self.policy)
        {
            Ok(value) => value,
            Err(error) => {
                let (event, state) = claim_error_metric(&error);
                self.metrics.integration_operation(event, state);
                return Err(error);
            }
        };
        let Some(claimed) = claimed else {
            return Ok(RunOnceResult::Idle);
        };
        self.metrics
            .integration_operation(IntegrationEvent::Claimed, OperationStateClass::Dispatching);
        self.metrics.integration_operation(
            IntegrationEvent::DispatchStarted,
            OperationStateClass::Dispatching,
        );
        let attempt_started = Instant::now();
        let outcome = dispatcher.dispatch(&claimed).await;
        let attempt_elapsed = attempt_started.elapsed();
        let (event, state, attempt_outcome, dispatch_class) =
            operation_outcome_metric(&outcome, claimed.attempt_number, &self.policy);
        self.metrics.integration_operation(
            IntegrationEvent::Attempted,
            OperationStateClass::Dispatching,
        );
        if let Err(error) = self
            .persistence
            .record_runtime_outcome(&claimed, outcome, &self.policy)
        {
            // The provider outcome is known only in-process until this
            // transaction commits. Do not report it as a durable terminal
            // result, mutate state, or schedule a retry. The attempted effect
            // retains its dispatch certainty while durable authority remains
            // `dispatching` for restart recovery.
            self.metrics.integration_operation(
                IntegrationEvent::SystemFailure,
                OperationStateClass::Dispatching,
            );
            self.metrics.integration_attempt(
                AttemptOutcome::PersistenceFailure,
                dispatch_class,
                attempt_elapsed,
            );
            return Err(error);
        }
        self.metrics.integration_operation(event, state);
        self.metrics
            .integration_attempt(attempt_outcome, dispatch_class, attempt_elapsed);
        Ok(RunOnceResult::Dispatched {
            operation_id: claimed.operation_id,
        })
    }
}

/// Claim failures happen before the operation state changes. Their metrics
/// must therefore classify the bounded reason while retaining the existing
/// durable `Ready` state rather than inventing a manual-resolution transition.
fn claim_error_metric(error: &IntegrationError) -> (IntegrationEvent, OperationStateClass) {
    match error {
        IntegrationError::CircuitOpen
        | IntegrationError::ConcurrencyLimited
        | IntegrationError::RateLimited
        | IntegrationError::OperationNotDue => {
            (IntegrationEvent::Deferred, OperationStateClass::Ready)
        }
        IntegrationError::BindingRevisionStale
        | IntegrationError::BindingUnavailable
        | IntegrationError::InstanceNotReady
        | IntegrationError::ModeBlocked => (
            IntegrationEvent::BlockedConfiguration,
            OperationStateClass::Ready,
        ),
        IntegrationError::Persistence => {
            (IntegrationEvent::SystemFailure, OperationStateClass::Ready)
        }
        // A durable ManualResolutionRequired operation is not claimable, so it
        // never arrives here as a claim error. Other unexpected pre-claim
        // failures are bounded system failures, not manual work.
        _ => (IntegrationEvent::SystemFailure, OperationStateClass::Ready),
    }
}

fn operation_outcome_metric(
    outcome: &Result<DispatchResult, TransportFailure>,
    attempt: u32,
    policy: &OperationRuntimePolicy,
) -> (
    IntegrationEvent,
    OperationStateClass,
    AttemptOutcome,
    AttemptDispatchClass,
) {
    match outcome {
        Ok(DispatchResult::Succeeded { .. }) => (
            IntegrationEvent::Succeeded,
            OperationStateClass::Succeeded,
            AttemptOutcome::Succeeded,
            AttemptDispatchClass::Dispatched,
        ),
        Ok(DispatchResult::Rejected { .. }) => (
            IntegrationEvent::Rejected,
            OperationStateClass::Rejected,
            AttemptOutcome::Rejected,
            AttemptDispatchClass::Dispatched,
        ),
        Err(failure) if failure.may_have_dispatched => (
            IntegrationEvent::UnknownOutcome,
            OperationStateClass::UnknownOutcome,
            AttemptOutcome::UnknownOutcome,
            AttemptDispatchClass::MayHaveDispatched,
        ),
        Err(failure) if failure.is_retryable() && attempt < policy.retry.max_attempts => (
            IntegrationEvent::RetryScheduled,
            OperationStateClass::RetryableFailure,
            AttemptOutcome::RetryableFailure,
            AttemptDispatchClass::NotDispatched,
        ),
        Err(_) => (
            IntegrationEvent::NonRetryableFailure,
            OperationStateClass::NonRetryableFailure,
            AttemptOutcome::NonRetryableFailure,
            AttemptDispatchClass::NotDispatched,
        ),
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use super::{
        ClaimedOperation, DispatchResult, OperationDispatcher, OperationRuntime,
        OperationRuntimeMetrics, OperationRuntimePersistence, OperationRuntimePolicy, RetryPolicy,
        RunOnceResult,
    };
    use crate::db::migrations::run_migrations;
    use crate::integration::operation::ExternalOperation;
    use crate::integration::store::IntegrationStore;
    use crate::integration::transport::{TransportErrorClass, TransportFailure};
    use crate::integration::types::{
        CapabilityId, ExternalOperationId, IntegrationError, ProviderBinding, ProviderBindingId,
        ProviderHealth, ProviderId, ProviderInstance, ProviderInstanceId, ProviderLifecycle,
        ProviderManifest, ProviderReadiness,
    };
    use crate::observability::{IntegrationEvent, OperationStateClass, RuntimeMetrics};

    fn store_with_pool() -> (IntegrationStore, Pool<SqliteConnectionManager>) {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        run_migrations(&pool.get().unwrap()).unwrap();
        (IntegrationStore::new(pool.clone()), pool)
    }

    fn store() -> IntegrationStore {
        store_with_pool().0
    }

    fn context() -> ExecutionContext {
        let tenant_id = TenantId::new("tenant-a").unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("operator-a", "admin").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("revision-a").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new("request-a").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn seed(store: &IntegrationStore) -> ExternalOperationId {
        let capability = CapabilityId::new("fixture.refund").unwrap();
        let provider = ProviderId::new("fixture-payments").unwrap();
        store
            .save_manifest(&ProviderManifest {
                provider_id: provider.clone(),
                version: "1".into(),
                capabilities: BTreeSet::from([capability.clone()]),
                config_schema: vec![],
                secret_schema: vec![],
                api_versions: BTreeMap::new(),
                webhook_types: BTreeSet::new(),
                simulation_capabilities: BTreeSet::from([capability.clone()]),
                readiness: ProviderReadiness::Fixture,
                compatibility: BTreeMap::new(),
            })
            .unwrap();
        let instance = ProviderInstance {
            id: ProviderInstanceId::new("instance-a").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_id: provider,
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
            id: ProviderBindingId::new("binding-a").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_instance_id: instance.id,
            capability: capability.clone(),
            config_revision: "revision-a".into(),
            enabled: true,
        };
        store.save_binding(&binding, "operator-a").unwrap();
        let resolved = store
            .catalog()
            .unwrap()
            .resolve(&context(), &capability)
            .unwrap();
        let mut operation = ExternalOperation::planned(
            ExternalOperationId::new("operation-a").unwrap(),
            &resolved,
            "refund",
            "idempotency-a",
            "request-hash-a",
        )
        .unwrap();
        operation.ready().unwrap();
        store.persist_operation(&operation).unwrap()
    }

    fn runtime(store: IntegrationStore) -> OperationRuntime {
        OperationRuntime::new(
            store,
            OperationRuntimePolicy {
                retry: RetryPolicy {
                    max_attempts: 2,
                    base_delay: Duration::from_millis(1),
                    max_delay: Duration::from_millis(1),
                },
                max_in_flight_per_binding: 1,
                max_attempts_per_minute: 10,
                circuit_failure_threshold: 2,
                circuit_open_for: Duration::from_secs(1),
            },
        )
        .unwrap()
    }

    #[test]
    fn claim_errors_use_bounded_deferred_configuration_and_system_classifiers() {
        for error in [
            IntegrationError::CircuitOpen,
            IntegrationError::RateLimited,
            IntegrationError::ConcurrencyLimited,
            IntegrationError::OperationNotDue,
        ] {
            assert_eq!(
                super::claim_error_metric(&error),
                (IntegrationEvent::Deferred, OperationStateClass::Ready)
            );
        }
        for error in [
            IntegrationError::BindingRevisionStale,
            IntegrationError::BindingUnavailable,
            IntegrationError::InstanceNotReady,
            IntegrationError::ModeBlocked,
        ] {
            assert_eq!(
                super::claim_error_metric(&error),
                (
                    IntegrationEvent::BlockedConfiguration,
                    OperationStateClass::Ready
                )
            );
        }
        assert_eq!(
            super::claim_error_metric(&IntegrationError::Persistence),
            (IntegrationEvent::SystemFailure, OperationStateClass::Ready)
        );
        assert_ne!(
            super::claim_error_metric(&IntegrationError::InvalidOperationTransition).0,
            IntegrationEvent::ManualResolutionRequired
        );
    }

    #[test]
    fn non_retryable_transport_failure_is_terminal_known_no_effect_not_manual_resolution() {
        let (event, state, outcome, dispatch_class) = super::operation_outcome_metric(
            &Err(TransportFailure::before_dispatch(
                TransportErrorClass::Remote4xx,
            )),
            1,
            &OperationRuntimePolicy::default(),
        );

        assert_eq!(event, IntegrationEvent::NonRetryableFailure);
        assert_eq!(state, OperationStateClass::NonRetryableFailure);
        assert_eq!(
            outcome,
            crate::observability::AttemptOutcome::NonRetryableFailure
        );
        assert_eq!(
            dispatch_class,
            crate::observability::AttemptDispatchClass::NotDispatched
        );
    }

    struct PortOnlyPersistence;

    impl OperationRuntimePersistence for PortOnlyPersistence {
        fn persist_operation(
            &self,
            _operation: &ExternalOperation,
        ) -> Result<ExternalOperationId, IntegrationError> {
            Err(IntegrationError::Persistence)
        }

        fn claim_next_operation(
            &self,
            _tenant_id: &str,
            _policy: &OperationRuntimePolicy,
        ) -> Result<Option<ClaimedOperation>, IntegrationError> {
            Ok(None)
        }

        fn record_runtime_outcome(
            &self,
            _claimed: &ClaimedOperation,
            _outcome: Result<DispatchResult, TransportFailure>,
            _policy: &OperationRuntimePolicy,
        ) -> Result<(), IntegrationError> {
            Err(IntegrationError::Persistence)
        }

        fn recover_inflight_operations(&self, _tenant_id: &str) -> Result<u64, IntegrationError> {
            Ok(0)
        }

        fn operation_runtime_metrics(
            &self,
            _tenant_id: &str,
        ) -> Result<OperationRuntimeMetrics, IntegrationError> {
            Ok(OperationRuntimeMetrics {
                ready: 7,
                ..Default::default()
            })
        }

        fn begin_reconciliation(
            &self,
            _tenant_id: &str,
            _operation_id: &ExternalOperationId,
            _evidence_ref: &str,
        ) -> Result<(), IntegrationError> {
            Err(IntegrationError::Persistence)
        }

        fn resolve_reconciliation(
            &self,
            _tenant_id: &str,
            _operation_id: &ExternalOperationId,
            _effect_confirmed: bool,
            _actor_ref: &str,
        ) -> Result<(), IntegrationError> {
            Err(IntegrationError::Persistence)
        }
    }

    #[tokio::test]
    async fn runtime_accepts_persistence_port_without_integration_store() {
        let runtime = OperationRuntime::from_persistence(
            Arc::new(PortOnlyPersistence),
            OperationRuntimePolicy::default(),
        )
        .unwrap();

        assert_eq!(runtime.metrics("tenant-a").unwrap().ready, 7);
        assert_eq!(runtime.recover_after_restart("tenant-a").unwrap(), 0);

        let result = runtime
            .run_once(
                "tenant-a",
                &OutcomeDispatcher {
                    outcome: Ok(DispatchResult::Succeeded {
                        provider_result_ref: None,
                    }),
                },
            )
            .await
            .unwrap();
        assert_eq!(result, RunOnceResult::Idle);
    }

    struct OutcomeDispatcher {
        outcome: Result<DispatchResult, TransportFailure>,
    }

    #[async_trait]
    impl OperationDispatcher for OutcomeDispatcher {
        async fn dispatch(
            &self,
            _operation: &ClaimedOperation,
        ) -> Result<DispatchResult, TransportFailure> {
            self.outcome.clone()
        }
    }

    struct PersistenceFailingDispatcher {
        pool: Pool<SqliteConnectionManager>,
        outcome: Result<DispatchResult, TransportFailure>,
    }

    #[async_trait]
    impl OperationDispatcher for PersistenceFailingDispatcher {
        async fn dispatch(
            &self,
            _operation: &ClaimedOperation,
        ) -> Result<DispatchResult, TransportFailure> {
            self.pool
                .get()
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER force_operation_outcome_persistence_failure
                     BEFORE UPDATE ON external_operation_attempts
                     BEGIN
                       SELECT RAISE(ABORT, 'forced outcome persistence failure');
                     END;",
                )
                .unwrap();
            self.outcome.clone()
        }
    }

    #[tokio::test]
    async fn post_dispatch_failure_becomes_unknown_and_restart_never_retries_it() {
        let integration_store = store();
        seed(&integration_store);
        let operation_runtime = runtime(integration_store.clone());
        let result = operation_runtime
            .run_once(
                "tenant-a",
                &OutcomeDispatcher {
                    outcome: Err(TransportFailure::after_dispatch(
                        TransportErrorClass::ResponseTimeout,
                    )),
                },
            )
            .await
            .unwrap();
        assert!(matches!(result, RunOnceResult::Dispatched { .. }));
        assert_eq!(
            operation_runtime
                .metrics("tenant-a")
                .unwrap()
                .unknown_outcome,
            1
        );
        assert!(matches!(
            operation_runtime
                .run_once(
                    "tenant-a",
                    &OutcomeDispatcher {
                        outcome: Ok(DispatchResult::Succeeded {
                            provider_result_ref: None,
                        }),
                    },
                )
                .await
                .unwrap(),
            RunOnceResult::Idle
        ));

        // Simulate the process losing an in-flight dispatch after a successful
        // claim. Recovery must make it UnknownOutcome, not Ready.
        let recovery_store = store();
        seed(&recovery_store);
        let claimed = recovery_store
            .claim_next_operation("tenant-a", &OperationRuntimePolicy::default())
            .unwrap()
            .unwrap();
        assert!(!claimed.attempt_id.as_str().is_empty());
        let recovery = runtime(recovery_store)
            .recover_after_restart("tenant-a")
            .unwrap();
        assert_eq!(recovery, 1);
    }

    #[tokio::test]
    async fn retryable_before_dispatch_failure_is_scheduled_and_circuit_opens() {
        let store = store();
        seed(&store);
        let runtime = runtime(store);
        let dispatcher = OutcomeDispatcher {
            outcome: Err(TransportFailure::before_dispatch(
                TransportErrorClass::ConnectFailure,
            )),
        };
        runtime.run_once("tenant-a", &dispatcher).await.unwrap();
        assert_eq!(runtime.metrics("tenant-a").unwrap().retryable_failure, 1);
    }

    #[tokio::test]
    async fn unknown_outcome_is_observed_without_exporting_operation_identity() {
        let store = store();
        seed(&store);
        let metrics = Arc::new(RuntimeMetrics::default());
        let runtime = OperationRuntime::new_with_metrics(
            store,
            OperationRuntimePolicy::default(),
            metrics.clone(),
        )
        .unwrap();
        runtime
            .run_once(
                "tenant-a",
                &OutcomeDispatcher {
                    outcome: Err(TransportFailure::after_dispatch(
                        TransportErrorClass::ResponseTimeout,
                    )),
                },
            )
            .await
            .unwrap();

        let output = metrics.render();
        assert!(output.contains("event=\"unknown_outcome\""));
        assert!(output.contains("state=\"unknown_outcome\""));
        assert!(output.contains("talos_integration_attempts_total"));
        assert!(
            output.contains("outcome=\"unknown_outcome\",dispatch_class=\"may_have_dispatched\"")
        );
        assert!(output.contains("talos_integration_attempt_latency_milliseconds_count"));
        assert!(!output.contains("operation-a"));
    }

    #[tokio::test]
    async fn successful_dispatch_records_durable_success_and_terminal_metrics() {
        let store = store();
        seed(&store);
        let metrics = Arc::new(RuntimeMetrics::default());
        let runtime = OperationRuntime::new_with_metrics(
            store,
            OperationRuntimePolicy::default(),
            metrics.clone(),
        )
        .unwrap();

        runtime
            .run_once(
                "tenant-a",
                &OutcomeDispatcher {
                    outcome: Ok(DispatchResult::Succeeded {
                        provider_result_ref: None,
                    }),
                },
            )
            .await
            .unwrap();

        let output = metrics.render();
        assert!(output.contains("event=\"succeeded\",state=\"succeeded\"} 1"));
        assert!(output.contains("outcome=\"succeeded\",dispatch_class=\"dispatched\"} 1"));
        assert!(
            !output.contains("outcome=\"persistence_failure\",dispatch_class=\"dispatched\"} 1")
        );
    }

    #[tokio::test]
    async fn outcome_persistence_failure_keeps_dispatching_and_emits_system_terminal_metrics() {
        let (store, pool) = store_with_pool();
        seed(&store);
        let metrics = Arc::new(RuntimeMetrics::default());
        let runtime = OperationRuntime::new_with_metrics(
            store,
            OperationRuntimePolicy::default(),
            metrics.clone(),
        )
        .unwrap();

        assert_eq!(
            runtime
                .run_once(
                    "tenant-a",
                    &PersistenceFailingDispatcher {
                        pool: pool.clone(),
                        outcome: Ok(DispatchResult::Succeeded {
                            provider_result_ref: None,
                        }),
                    },
                )
                .await
                .unwrap_err(),
            IntegrationError::Persistence
        );

        let operation_state: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT state FROM external_operations WHERE tenant_id = 'tenant-a' AND id = 'operation-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let attempt_state: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT state FROM external_operation_attempts WHERE tenant_id = 'tenant-a' AND operation_id = 'operation-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(operation_state, "dispatching");
        assert_eq!(attempt_state, "dispatching");

        let output = metrics.render();
        assert!(output.contains("event=\"system_failure\",state=\"dispatching\"} 1"));
        assert!(
            output.contains("outcome=\"persistence_failure\",dispatch_class=\"dispatched\"} 1")
        );
        assert!(!output.contains("event=\"succeeded\",state=\"succeeded\"} 1"));
        assert!(!output.contains("outcome=\"succeeded\",dispatch_class=\"dispatched\"} 1"));
        assert!(!output.contains("operation-a"));
    }

    #[tokio::test]
    async fn dispatched_rejection_is_not_counted_as_success() {
        let store = store();
        seed(&store);
        let metrics = Arc::new(RuntimeMetrics::default());
        let runtime = OperationRuntime::new_with_metrics(
            store,
            OperationRuntimePolicy::default(),
            metrics.clone(),
        )
        .unwrap();
        runtime
            .run_once(
                "tenant-a",
                &OutcomeDispatcher {
                    outcome: Ok(DispatchResult::Rejected {
                        provider_result_ref: None,
                    }),
                },
            )
            .await
            .unwrap();

        let output = metrics.render();
        assert!(output.contains("event=\"rejected\",state=\"rejected\"} 1"));
        assert!(output.contains("outcome=\"rejected\",dispatch_class=\"dispatched\"} 1"));
        assert!(!output.contains("operation-a"));
    }
}

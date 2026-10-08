//! Provider-neutral durable webhook intake and worker boundary.
//!
//! HTTP adapters hand the raw request only to this runtime. The runtime first
//! resolves a deterministic endpoint, fails closed if verification cannot be
//! proven, then persists a deduplicated inbox record. The asynchronous worker
//! maps that record to a canonical event and invokes an application port; it
//! never performs a business-table write itself.

use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

use super::store::IntegrationStore;
use super::types::{
    IntegrationError, ProviderBindingId, ProviderId, WebhookEndpointId, WebhookInboxId,
};
use super::webhook_persistence_contract::WebhookRuntimePersistence;
#[cfg(feature = "postgres")]
use super::webhook_persistence_postgres::PostgresWebhookPersistence;
use crate::observability::{MetricsSink, NoopMetrics, WebhookEvent, WebhookOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookEndpointContext {
    pub endpoint_id: WebhookEndpointId,
    pub tenant_id: String,
    pub binding_id: ProviderBindingId,
    pub provider_id: ProviderId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookReceiptInput {
    pub endpoint_token: Vec<u8>,
    pub provider_event_id: String,
    /// Ephemeral verification material. It may contain a signature and is
    /// intentionally never persisted or returned from the runtime.
    pub verification_headers_json: String,
    /// Redacted receipt metadata that may be persisted with the inbox entry.
    pub stored_headers_json: String,
    pub raw_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookReceipt {
    pub inbox_id: WebhookInboxId,
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedWebhook {
    pub tenant_id: String,
    pub inbox_id: WebhookInboxId,
    pub endpoint: WebhookEndpointContext,
    pub provider_event_id: String,
    pub headers_json: String,
    pub raw_payload: Vec<u8>,
    pub attempt_number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalWebhookEvent {
    pub tenant_id: String,
    pub provider_id: ProviderId,
    pub binding_id: ProviderBindingId,
    pub provider_event_id: String,
    pub event_type: String,
    pub payload_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookHandlingError {
    Retryable(String),
    Permanent(String),
}

impl WebhookHandlingError {
    pub fn classification(&self) -> &str {
        match self {
            Self::Retryable(classification) | Self::Permanent(classification) => classification,
        }
    }
}

/// Provider-specific verification stays behind this narrow hook. An unknown
/// endpoint, missing verifier, or verifier error must all reject the callback.
#[async_trait]
pub trait WebhookVerifier: Send + Sync {
    async fn verify(
        &self,
        endpoint: &WebhookEndpointContext,
        headers_json: &str,
        raw_payload: &[u8],
    ) -> Result<(), IntegrationError>;
}

/// Converts provider payload into a provider-neutral application event. This
/// has no repository or transport capability.
pub trait WebhookEventMapper: Send + Sync {
    fn map(&self, webhook: &ClaimedWebhook) -> Result<CanonicalWebhookEvent, WebhookHandlingError>;
}

/// The worker's only business-side capability. A production implementation
/// must dispatch through Registry/application workflows with its own trusted
/// context; webhook callbacks never receive a repository handle.
#[async_trait]
pub trait WebhookApplicationPort: Send + Sync {
    async fn dispatch(&self, event: CanonicalWebhookEvent) -> Result<(), WebhookHandlingError>;
}

#[derive(Clone)]
pub struct WebhookRuntime {
    persistence: Arc<dyn WebhookRuntimePersistence>,
    metrics: Arc<dyn MetricsSink>,
}

const FIXTURE_WEBHOOK_SIGNATURE: &str = "fixture-signed";
const MAX_WEBHOOK_ATTEMPTS: u32 = 5;

/// The only mounted Stage-2 provider callback adapter. It recognizes a
/// deterministic fixture signature and rejects every other provider; real
/// connector verifiers remain deferred with SP-21/SP-22.
struct FixtureWebhookVerifier;

#[async_trait]
impl WebhookVerifier for FixtureWebhookVerifier {
    async fn verify(
        &self,
        endpoint: &WebhookEndpointContext,
        headers_json: &str,
        _raw_payload: &[u8],
    ) -> Result<(), IntegrationError> {
        if !endpoint.provider_id.as_str().starts_with("fixture-") {
            return Err(IntegrationError::WebhookUnverifiable);
        }
        let headers = serde_json::from_str::<BTreeMap<String, String>>(headers_json)
            .map_err(|_| IntegrationError::WebhookUnverifiable)?;
        (headers
            .get("x-integration-fixture-signature")
            .is_some_and(|value| value == FIXTURE_WEBHOOK_SIGNATURE))
        .then_some(())
        .ok_or(IntegrationError::WebhookUnverifiable)
    }
}

/// Route-facing fixture ingress boundary. It intentionally owns the runtime
/// instead of exposing an IntegrationStore to HTTP adapters.
#[derive(Clone)]
pub struct FixtureWebhookIngress {
    runtime: WebhookRuntime,
}

impl FixtureWebhookIngress {
    pub fn new(store: IntegrationStore) -> Self {
        Self::new_with_metrics(store, Arc::new(NoopMetrics))
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn from_postgres(pool: sqlx::PgPool) -> Self {
        Self::from_postgres_with_metrics(pool, Arc::new(NoopMetrics))
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn from_postgres_with_metrics(
        pool: sqlx::PgPool,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self {
            runtime: WebhookRuntime::from_postgres_with_metrics(pool, metrics),
        }
    }

    pub(crate) fn new_with_metrics(store: IntegrationStore, metrics: Arc<dyn MetricsSink>) -> Self {
        Self {
            runtime: WebhookRuntime::new_with_metrics(store, metrics),
        }
    }

    pub async fn receive(
        &self,
        input: WebhookReceiptInput,
    ) -> Result<WebhookReceipt, IntegrationError> {
        self.runtime.receive(input, &FixtureWebhookVerifier).await
    }
}

impl WebhookRuntime {
    pub fn new(store: IntegrationStore) -> Self {
        Self::new_with_metrics(store, Arc::new(NoopMetrics))
    }

    pub(crate) fn new_with_metrics(store: IntegrationStore, metrics: Arc<dyn MetricsSink>) -> Self {
        Self::from_persistence_with_metrics(Arc::new(store), metrics)
    }

    pub(crate) fn from_persistence(persistence: Arc<dyn WebhookRuntimePersistence>) -> Self {
        Self::from_persistence_with_metrics(persistence, Arc::new(NoopMetrics))
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn from_postgres(pool: sqlx::PgPool) -> Self {
        Self::from_postgres_with_metrics(pool, Arc::new(NoopMetrics))
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn from_postgres_with_metrics(
        pool: sqlx::PgPool,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self::from_persistence_with_metrics(
            Arc::new(PostgresWebhookPersistence::new(pool)),
            metrics,
        )
    }

    pub(crate) fn from_persistence_with_metrics(
        persistence: Arc<dyn WebhookRuntimePersistence>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self {
            persistence,
            metrics,
        }
    }

    pub async fn receive(
        &self,
        input: WebhookReceiptInput,
        verifier: &dyn WebhookVerifier,
    ) -> Result<WebhookReceipt, IntegrationError> {
        self.metrics
            .webhook(WebhookEvent::Received, WebhookOutcome::Pending);
        if input.provider_event_id.trim().is_empty() || input.raw_payload.is_empty() {
            self.metrics
                .webhook(WebhookEvent::Rejected, WebhookOutcome::Rejected);
            return Err(IntegrationError::WebhookUnverifiable);
        }
        let endpoint = match self
            .persistence
            .resolve_webhook_endpoint(&input.endpoint_token)
        {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.metrics
                    .webhook(WebhookEvent::Rejected, webhook_failure_outcome(&error));
                return Err(error);
            }
        };
        if verifier
            .verify(
                &endpoint,
                &input.verification_headers_json,
                &input.raw_payload,
            )
            .await
            .is_err()
        {
            let rejected = self.persistence.record_rejected_webhook(
                &endpoint,
                &input.provider_event_id,
                &input.stored_headers_json,
                &input.raw_payload,
                "verification_failed",
            );
            return match rejected {
                Ok(()) => {
                    self.metrics
                        .webhook(WebhookEvent::Rejected, WebhookOutcome::Rejected);
                    Err(IntegrationError::WebhookUnverifiable)
                }
                Err(error) => {
                    self.metrics
                        .webhook(WebhookEvent::Rejected, WebhookOutcome::SystemFailure);
                    Err(error)
                }
            };
        }
        let (inbox_id, duplicate) = match self.persistence.record_verified_webhook(
            &endpoint,
            &input.provider_event_id,
            &input.stored_headers_json,
            &input.raw_payload,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.metrics
                    .webhook(WebhookEvent::Rejected, WebhookOutcome::SystemFailure);
                return Err(error);
            }
        };
        self.metrics.webhook(
            if duplicate {
                WebhookEvent::Duplicate
            } else {
                WebhookEvent::Verified
            },
            WebhookOutcome::Accepted,
        );
        Ok(WebhookReceipt {
            inbox_id,
            duplicate,
        })
    }

    fn retry_or_dead_letter(
        &self,
        webhook: &ClaimedWebhook,
        classification: &str,
    ) -> Result<(), IntegrationError> {
        if webhook.attempt_number >= MAX_WEBHOOK_ATTEMPTS {
            self.persist_processing_terminal(
                self.persistence.dead_letter_webhook(
                    webhook,
                    &format!("retry_budget_exhausted:{classification}"),
                ),
                WebhookEvent::DeadLettered,
                WebhookOutcome::PermanentFailure,
            )
        } else {
            self.persist_processing_terminal(
                self.persistence.retry_webhook(webhook, classification),
                WebhookEvent::RetryScheduled,
                WebhookOutcome::Retryable,
            )
        }
    }

    /// A claimed webhook has entered a durable processing lifecycle. Once the
    /// start metric is emitted, every branch must emit exactly one bounded
    /// terminal observation. A persistence failure never invents successful
    /// durable state; it is surfaced as a system terminal and returned.
    fn persist_processing_terminal(
        &self,
        persistence: Result<(), IntegrationError>,
        event: WebhookEvent,
        outcome: WebhookOutcome,
    ) -> Result<(), IntegrationError> {
        match persistence {
            Ok(()) => {
                self.metrics.webhook(event, outcome);
                Ok(())
            }
            Err(error) => {
                self.metrics
                    .webhook(WebhookEvent::SystemFailure, WebhookOutcome::SystemFailure);
                Err(error)
            }
        }
    }

    pub async fn run_once(
        &self,
        tenant_id: &str,
        mapper: &dyn WebhookEventMapper,
        application: &dyn WebhookApplicationPort,
    ) -> Result<Option<WebhookInboxId>, IntegrationError> {
        let Some(webhook) = self.persistence.claim_next_webhook(tenant_id)? else {
            return Ok(None);
        };
        self.metrics
            .webhook(WebhookEvent::ProcessingStarted, WebhookOutcome::Accepted);
        let event = match mapper.map(&webhook) {
            Ok(event) => event,
            Err(WebhookHandlingError::Retryable(classification)) => {
                self.retry_or_dead_letter(&webhook, &classification)?;
                return Ok(Some(webhook.inbox_id));
            }
            Err(WebhookHandlingError::Permanent(classification)) => {
                self.persist_processing_terminal(
                    self.persistence
                        .dead_letter_webhook(&webhook, &classification),
                    WebhookEvent::DeadLettered,
                    WebhookOutcome::PermanentFailure,
                )?;
                return Ok(Some(webhook.inbox_id));
            }
        };
        if event.tenant_id != webhook.tenant_id
            || event.provider_id != webhook.endpoint.provider_id
            || event.binding_id != webhook.endpoint.binding_id
            || event.provider_event_id != webhook.provider_event_id
        {
            self.persist_processing_terminal(
                self.persistence
                    .dead_letter_webhook(&webhook, "canonical_identity_mismatch"),
                WebhookEvent::Conflict,
                WebhookOutcome::PermanentFailure,
            )?;
            return Ok(Some(webhook.inbox_id));
        }
        let event_type = event.event_type.clone();
        match application.dispatch(event).await {
            Ok(()) => {
                self.persist_processing_terminal(
                    self.persistence.complete_webhook(&webhook, &event_type),
                    WebhookEvent::Processed,
                    WebhookOutcome::Accepted,
                )?;
            }
            Err(WebhookHandlingError::Retryable(classification)) => {
                self.retry_or_dead_letter(&webhook, &classification)?
            }
            Err(WebhookHandlingError::Permanent(classification)) => self
                .persist_processing_terminal(
                    self.persistence
                        .dead_letter_webhook(&webhook, &classification),
                    WebhookEvent::DeadLettered,
                    WebhookOutcome::PermanentFailure,
                )?,
        }
        Ok(Some(webhook.inbox_id))
    }

    /// Replay returns the inbox entry to the same worker queue. It does not
    /// invoke an application port directly and therefore cannot bypass normal
    /// Registry/workflow governance.
    pub fn replay(
        &self,
        tenant_id: &str,
        inbox_id: &WebhookInboxId,
        actor_ref: &str,
        reason: &str,
    ) -> Result<(), IntegrationError> {
        let result = self
            .persistence
            .replay_webhook(tenant_id, inbox_id, actor_ref, reason);
        if result.is_ok() {
            self.metrics
                .webhook(WebhookEvent::Replayed, WebhookOutcome::Accepted);
        }
        result
    }
}

fn webhook_failure_outcome(error: &IntegrationError) -> WebhookOutcome {
    match error {
        IntegrationError::Persistence => WebhookOutcome::SystemFailure,
        _ => WebhookOutcome::Rejected,
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use async_trait::async_trait;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;

    use super::{
        CanonicalWebhookEvent, ClaimedWebhook, FixtureWebhookIngress, WebhookApplicationPort,
        WebhookEventMapper, WebhookHandlingError, WebhookReceiptInput, WebhookRuntime,
        WebhookVerifier,
    };
    use crate::db::migrations::run_migrations;
    use crate::integration::store::IntegrationStore;
    use crate::integration::types::{
        CapabilityId, IntegrationError, ProviderBinding, ProviderBindingId, ProviderHealth,
        ProviderId, ProviderInstance, ProviderInstanceId, ProviderLifecycle, ProviderManifest,
        ProviderReadiness, WebhookEndpointId, WebhookInboxId,
    };
    use crate::integration::webhook_persistence_contract::{
        WebhookReplayPersistence, WebhookRuntimePersistence,
    };
    use crate::observability::RuntimeMetrics;

    fn store_with_pool() -> (
        IntegrationStore,
        ProviderBinding,
        Pool<SqliteConnectionManager>,
    ) {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        run_migrations(&pool.get().unwrap()).unwrap();
        let store = IntegrationStore::new(pool.clone());
        let provider = ProviderId::new("fixture-webhooks").unwrap();
        let capability = CapabilityId::new("fixture.webhook").unwrap();
        store
            .save_manifest(&ProviderManifest {
                provider_id: provider.clone(),
                version: "1".into(),
                capabilities: BTreeSet::from([capability.clone()]),
                config_schema: vec![],
                secret_schema: vec![],
                api_versions: BTreeMap::new(),
                webhook_types: BTreeSet::from(["fixture.updated".into()]),
                simulation_capabilities: BTreeSet::from([capability.clone()]),
                readiness: ProviderReadiness::Fixture,
                compatibility: BTreeMap::new(),
            })
            .unwrap();
        let instance = ProviderInstance {
            id: ProviderInstanceId::new("instance-a").unwrap(),
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
            id: ProviderBindingId::new("binding-a").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_instance_id: instance.id,
            capability,
            config_revision: "revision-a".into(),
            enabled: true,
        };
        store.save_binding(&binding, "operator-a").unwrap();
        store
            .register_webhook_endpoint("tenant-a", &binding.id, b"endpoint-token")
            .unwrap();
        (store, binding, pool)
    }

    fn store() -> (IntegrationStore, ProviderBinding) {
        let (store, binding, _) = store_with_pool();
        (store, binding)
    }

    struct AcceptingVerifier;

    #[async_trait]
    impl WebhookVerifier for AcceptingVerifier {
        async fn verify(
            &self,
            _endpoint: &super::WebhookEndpointContext,
            _headers_json: &str,
            _raw_payload: &[u8],
        ) -> Result<(), IntegrationError> {
            Ok(())
        }
    }

    struct PortOnlyWebhookPersistence;

    impl WebhookReplayPersistence for PortOnlyWebhookPersistence {
        fn replay_webhook(
            &self,
            _tenant_id: &str,
            _inbox_id: &WebhookInboxId,
            _actor_ref: &str,
            _reason: &str,
        ) -> Result<(), IntegrationError> {
            Ok(())
        }
    }

    impl WebhookRuntimePersistence for PortOnlyWebhookPersistence {
        fn resolve_webhook_endpoint(
            &self,
            _token: &[u8],
        ) -> Result<super::WebhookEndpointContext, IntegrationError> {
            Ok(super::WebhookEndpointContext {
                endpoint_id: WebhookEndpointId::new("endpoint-port-only").unwrap(),
                tenant_id: "tenant-port-only".into(),
                binding_id: ProviderBindingId::new("binding-port-only").unwrap(),
                provider_id: ProviderId::new("fixture-port-only").unwrap(),
            })
        }

        fn record_verified_webhook(
            &self,
            _endpoint: &super::WebhookEndpointContext,
            _provider_event_id: &str,
            _headers_json: &str,
            _raw_payload: &[u8],
        ) -> Result<(WebhookInboxId, bool), IntegrationError> {
            Ok((WebhookInboxId::new("inbox-port-only").unwrap(), false))
        }

        fn record_rejected_webhook(
            &self,
            _endpoint: &super::WebhookEndpointContext,
            _provider_event_id: &str,
            _headers_json: &str,
            _raw_payload: &[u8],
            _classification: &str,
        ) -> Result<(), IntegrationError> {
            Ok(())
        }

        fn claim_next_webhook(
            &self,
            _tenant_id: &str,
        ) -> Result<Option<ClaimedWebhook>, IntegrationError> {
            Ok(None)
        }

        fn complete_webhook(
            &self,
            _webhook: &ClaimedWebhook,
            _canonical_event_type: &str,
        ) -> Result<(), IntegrationError> {
            Ok(())
        }

        fn retry_webhook(
            &self,
            _webhook: &ClaimedWebhook,
            _classification: &str,
        ) -> Result<(), IntegrationError> {
            Ok(())
        }

        fn dead_letter_webhook(
            &self,
            _webhook: &ClaimedWebhook,
            _reason: &str,
        ) -> Result<(), IntegrationError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn runtime_accepts_webhook_persistence_port_without_integration_store() {
        let runtime = WebhookRuntime::from_persistence(Arc::new(PortOnlyWebhookPersistence));
        let receipt = runtime
            .receive(
                WebhookReceiptInput {
                    endpoint_token: b"port-only-token".to_vec(),
                    provider_event_id: "provider-port-only".into(),
                    verification_headers_json: "{}".into(),
                    stored_headers_json: "{}".into(),
                    raw_payload: br#"{"fixture":true}"#.to_vec(),
                },
                &AcceptingVerifier,
            )
            .await
            .unwrap();

        assert_eq!(
            receipt.inbox_id,
            WebhookInboxId::new("inbox-port-only").unwrap()
        );
        assert!(!receipt.duplicate);
    }

    struct RejectingVerifier;

    #[async_trait]
    impl WebhookVerifier for RejectingVerifier {
        async fn verify(
            &self,
            _endpoint: &super::WebhookEndpointContext,
            _headers_json: &str,
            _raw_payload: &[u8],
        ) -> Result<(), IntegrationError> {
            Err(IntegrationError::WebhookUnverifiable)
        }
    }

    struct Mapper;

    impl WebhookEventMapper for Mapper {
        fn map(
            &self,
            webhook: &ClaimedWebhook,
        ) -> Result<CanonicalWebhookEvent, WebhookHandlingError> {
            Ok(CanonicalWebhookEvent {
                tenant_id: webhook.tenant_id.clone(),
                provider_id: webhook.endpoint.provider_id.clone(),
                binding_id: webhook.endpoint.binding_id.clone(),
                provider_event_id: webhook.provider_event_id.clone(),
                event_type: "fixture.updated".into(),
                payload_hash: "hash-a".into(),
            })
        }
    }

    fn force_processing_terminal_persistence_failure(pool: &Pool<SqliteConnectionManager>) {
        pool.get()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER force_webhook_processing_terminal_failure
                 BEFORE UPDATE ON webhook_inbox
                 BEGIN
                   SELECT RAISE(ABORT, 'forced terminal persistence failure');
                 END;",
            )
            .unwrap();
    }

    struct RetryableMapperWithPersistenceFailure {
        pool: Pool<SqliteConnectionManager>,
    }

    impl WebhookEventMapper for RetryableMapperWithPersistenceFailure {
        fn map(
            &self,
            _webhook: &ClaimedWebhook,
        ) -> Result<CanonicalWebhookEvent, WebhookHandlingError> {
            force_processing_terminal_persistence_failure(&self.pool);
            Err(WebhookHandlingError::Retryable("mapping_retryable".into()))
        }
    }

    struct PermanentMapper;

    impl WebhookEventMapper for PermanentMapper {
        fn map(
            &self,
            _webhook: &ClaimedWebhook,
        ) -> Result<CanonicalWebhookEvent, WebhookHandlingError> {
            Err(WebhookHandlingError::Permanent("mapping_permanent".into()))
        }
    }

    struct MismatchMapperWithPersistenceFailure {
        pool: Pool<SqliteConnectionManager>,
    }

    impl WebhookEventMapper for MismatchMapperWithPersistenceFailure {
        fn map(
            &self,
            webhook: &ClaimedWebhook,
        ) -> Result<CanonicalWebhookEvent, WebhookHandlingError> {
            force_processing_terminal_persistence_failure(&self.pool);
            Ok(CanonicalWebhookEvent {
                tenant_id: "other-tenant".into(),
                provider_id: webhook.endpoint.provider_id.clone(),
                binding_id: webhook.endpoint.binding_id.clone(),
                provider_event_id: webhook.provider_event_id.clone(),
                event_type: "fixture.updated".into(),
                payload_hash: "hash-a".into(),
            })
        }
    }

    struct AcceptingApplication;

    #[async_trait]
    impl WebhookApplicationPort for AcceptingApplication {
        async fn dispatch(
            &self,
            _event: CanonicalWebhookEvent,
        ) -> Result<(), WebhookHandlingError> {
            Ok(())
        }
    }

    struct AcceptingApplicationWithPersistenceFailure {
        pool: Pool<SqliteConnectionManager>,
    }

    #[async_trait]
    impl WebhookApplicationPort for AcceptingApplicationWithPersistenceFailure {
        async fn dispatch(
            &self,
            _event: CanonicalWebhookEvent,
        ) -> Result<(), WebhookHandlingError> {
            force_processing_terminal_persistence_failure(&self.pool);
            Ok(())
        }
    }

    struct PermanentFailure;

    #[async_trait]
    impl WebhookApplicationPort for PermanentFailure {
        async fn dispatch(
            &self,
            _event: CanonicalWebhookEvent,
        ) -> Result<(), WebhookHandlingError> {
            Err(WebhookHandlingError::Permanent("business_rejected".into()))
        }
    }

    struct RetryableFailure;

    #[async_trait]
    impl WebhookApplicationPort for RetryableFailure {
        async fn dispatch(
            &self,
            _event: CanonicalWebhookEvent,
        ) -> Result<(), WebhookHandlingError> {
            Err(WebhookHandlingError::Retryable("temporary_failure".into()))
        }
    }

    fn input(event_id: &str) -> WebhookReceiptInput {
        WebhookReceiptInput {
            endpoint_token: b"endpoint-token".to_vec(),
            provider_event_id: event_id.into(),
            verification_headers_json: "{}".into(),
            stored_headers_json: "{}".into(),
            raw_payload: br#"{\"event\":\"fixture.updated\"}"#.to_vec(),
        }
    }

    fn assert_processing_system_failure(output: &str) {
        assert!(output.contains("event=\"processing_started\",outcome=\"accepted\"} 1"));
        assert!(output.contains("event=\"system_failure\",outcome=\"system_failure\"} 1"));
        assert!(!output.contains("event=\"processed\",outcome=\"accepted\"} 1"));
        assert!(!output.contains("event=\"retry_scheduled\",outcome=\"retryable\"} 1"));
        assert!(!output.contains("event=\"dead_lettered\",outcome=\"permanent_failure\"} 1"));
        assert!(!output.contains("event=\"conflict\",outcome=\"permanent_failure\"} 1"));
    }

    #[tokio::test]
    async fn fixture_ingress_accepts_only_fixture_signature() {
        let (store, _) = store();
        let ingress = FixtureWebhookIngress::new(store);
        let mut signed = input("fixture-ingress-a");
        signed.verification_headers_json =
            r#"{"x-integration-fixture-signature":"fixture-signed"}"#.into();

        assert!(!ingress.receive(signed).await.unwrap().duplicate);

        let rejected = ingress.receive(input("fixture-ingress-b")).await;
        assert_eq!(rejected.unwrap_err(), IntegrationError::WebhookUnverifiable);
    }

    #[tokio::test]
    async fn invalid_receipt_is_pending_then_rejected_without_acceptance() {
        let (store, _) = store();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store, metrics.clone());
        let mut invalid = input("provider-event-invalid-receipt");
        invalid.raw_payload.clear();

        assert_eq!(
            runtime
                .receive(invalid, &AcceptingVerifier)
                .await
                .unwrap_err(),
            IntegrationError::WebhookUnverifiable
        );

        let output = metrics.render();
        assert!(output.contains("event=\"received\",outcome=\"pending\"} 1"));
        assert!(output.contains("event=\"rejected\",outcome=\"rejected\"} 1"));
        assert!(!output.contains("outcome=\"accepted\""));
        assert!(!output.contains("outcome=\"system_failure\""));
    }

    #[tokio::test]
    async fn receipt_lifecycle_is_pending_then_terminal_and_unknown_token_is_rejected() {
        let (store, _) = store();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store, metrics.clone());
        let mut unknown = input("provider-event-unknown-token");
        unknown.endpoint_token = b"unknown-endpoint-token".to_vec();

        assert_eq!(
            runtime
                .receive(unknown, &AcceptingVerifier)
                .await
                .unwrap_err(),
            IntegrationError::BindingUnavailable
        );

        let output = metrics.render();
        assert!(output.contains("event=\"received\",outcome=\"pending\""));
        assert!(output.contains("event=\"rejected\",outcome=\"rejected\""));
        assert!(!output.contains("event=\"received\",outcome=\"accepted\""));
        assert!(!output.contains("outcome=\"system_failure\""));
        assert!(!output.contains("unknown-endpoint-token"));
        assert!(!output.contains("provider-event-unknown-token"));
    }

    #[tokio::test]
    async fn verification_failure_has_one_rejected_terminal_metric() {
        let (store, _) = store();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store, metrics.clone());

        assert_eq!(
            runtime
                .receive(
                    input("provider-event-verification-failure"),
                    &RejectingVerifier
                )
                .await
                .unwrap_err(),
            IntegrationError::WebhookUnverifiable
        );

        let output = metrics.render();
        assert!(output.contains("event=\"received\",outcome=\"pending\""));
        assert!(output.contains("event=\"rejected\",outcome=\"rejected\""));
        assert!(!output.contains("outcome=\"accepted\""));
        assert!(!output.contains("outcome=\"system_failure\""));
    }

    #[tokio::test]
    async fn receipt_persistence_failure_has_bounded_system_failure_terminal_metric() {
        let (store, _, pool) = store_with_pool();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store, metrics.clone());
        pool.get()
            .unwrap()
            .execute_batch("DROP TABLE webhook_inbox")
            .unwrap();

        assert_eq!(
            runtime
                .receive(
                    input("provider-event-persistence-failure"),
                    &AcceptingVerifier
                )
                .await
                .unwrap_err(),
            IntegrationError::Persistence
        );

        let output = metrics.render();
        assert!(output.contains("event=\"received\",outcome=\"pending\""));
        assert!(output.contains("event=\"rejected\",outcome=\"system_failure\""));
        assert!(!output.contains("outcome=\"accepted\""));
        assert!(!output.contains("outcome=\"rejected\""));
        assert!(!output.contains("provider-event-persistence-failure"));
    }

    #[tokio::test]
    async fn duplicate_callback_is_idempotent_and_worker_uses_application_port() {
        let (store, _) = store();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store.clone(), metrics.clone());
        let mut signed_input = input("event-a");
        signed_input.verification_headers_json =
            r#"{"x-integration-fixture-signature":"fixture-signed"}"#.into();
        signed_input.stored_headers_json =
            r#"{"content-type":"application/json","x-integration-event-id":"event-a"}"#.into();
        let first = runtime
            .receive(signed_input, &AcceptingVerifier)
            .await
            .unwrap();
        let duplicate = runtime
            .receive(input("event-a"), &AcceptingVerifier)
            .await
            .unwrap();
        assert!(!first.duplicate);
        assert!(duplicate.duplicate);
        assert_eq!(first.inbox_id, duplicate.inbox_id);
        let stored_headers = store.stored_webhook_headers(&first.inbox_id).unwrap();
        assert!(stored_headers.contains("x-integration-event-id"));
        assert!(!stored_headers.contains("fixture-signed"));
        assert!(!stored_headers.contains("signature"));
        assert_eq!(
            runtime
                .run_once("tenant-a", &Mapper, &AcceptingApplication)
                .await
                .unwrap(),
            Some(first.inbox_id)
        );
        assert!(
            runtime
                .run_once("tenant-a", &Mapper, &AcceptingApplication)
                .await
                .unwrap()
                .is_none()
        );
        let output = metrics.render();
        assert!(output.contains("event=\"received\",outcome=\"pending\"} 2"));
        assert!(output.contains("event=\"verified\",outcome=\"accepted\"} 1"));
        assert!(output.contains("event=\"duplicate\",outcome=\"accepted\"} 1"));
        assert!(!output.contains("event=\"rejected\""));
        assert!(!output.contains("outcome=\"system_failure\""));
        assert!(output.contains("event=\"processed\""));
        assert!(!output.contains("event-a"));
    }

    #[tokio::test]
    async fn mapper_retryable_persistence_failure_emits_one_system_terminal() {
        let (store, _, pool) = store_with_pool();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store.clone(), metrics.clone());
        let receipt = runtime
            .receive(
                input("mapper-retryable-persistence-failure"),
                &AcceptingVerifier,
            )
            .await
            .unwrap();

        assert_eq!(
            runtime
                .run_once(
                    "tenant-a",
                    &RetryableMapperWithPersistenceFailure { pool: pool.clone() },
                    &AcceptingApplication,
                )
                .await
                .unwrap_err(),
            IntegrationError::Persistence
        );
        assert_processing_system_failure(&metrics.render());
        let status: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT status FROM webhook_inbox WHERE id = ?1",
                [receipt.inbox_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "processing");
    }

    #[tokio::test]
    async fn mapper_permanent_dead_letters_with_one_permanent_terminal() {
        let (store, _) = store();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store, metrics.clone());
        runtime
            .receive(input("mapper-permanent"), &AcceptingVerifier)
            .await
            .unwrap();

        runtime
            .run_once("tenant-a", &PermanentMapper, &AcceptingApplication)
            .await
            .unwrap();
        let output = metrics.render();
        assert!(output.contains("event=\"processing_started\",outcome=\"accepted\"} 1"));
        assert!(output.contains("event=\"dead_lettered\",outcome=\"permanent_failure\"} 1"));
        assert!(!output.contains("event=\"system_failure\",outcome=\"system_failure\"} 1"));
    }

    #[tokio::test]
    async fn accepted_application_persistence_failure_emits_one_system_terminal() {
        let (store, _, pool) = store_with_pool();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store, metrics.clone());
        runtime
            .receive(
                input("application-success-persistence-failure"),
                &AcceptingVerifier,
            )
            .await
            .unwrap();

        assert_eq!(
            runtime
                .run_once(
                    "tenant-a",
                    &Mapper,
                    &AcceptingApplicationWithPersistenceFailure { pool },
                )
                .await
                .unwrap_err(),
            IntegrationError::Persistence
        );
        assert_processing_system_failure(&metrics.render());
    }

    #[tokio::test]
    async fn permanent_application_dead_letters_with_one_permanent_terminal() {
        let (store, _) = store();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store, metrics.clone());
        runtime
            .receive(input("application-permanent"), &AcceptingVerifier)
            .await
            .unwrap();

        runtime
            .run_once("tenant-a", &Mapper, &PermanentFailure)
            .await
            .unwrap();
        let output = metrics.render();
        assert!(output.contains("event=\"processing_started\",outcome=\"accepted\"} 1"));
        assert!(output.contains("event=\"dead_lettered\",outcome=\"permanent_failure\"} 1"));
        assert!(!output.contains("event=\"system_failure\",outcome=\"system_failure\"} 1"));
    }

    #[tokio::test]
    async fn identity_mismatch_persistence_failure_emits_one_system_terminal() {
        let (store, _, pool) = store_with_pool();
        let metrics = std::sync::Arc::new(RuntimeMetrics::default());
        let runtime = WebhookRuntime::new_with_metrics(store, metrics.clone());
        runtime
            .receive(
                input("identity-mismatch-persistence-failure"),
                &AcceptingVerifier,
            )
            .await
            .unwrap();

        assert_eq!(
            runtime
                .run_once(
                    "tenant-a",
                    &MismatchMapperWithPersistenceFailure { pool },
                    &AcceptingApplication,
                )
                .await
                .unwrap_err(),
            IntegrationError::Persistence
        );
        let output = metrics.render();
        assert_processing_system_failure(&output);
        assert!(!output.contains("identity-mismatch-persistence-failure"));
        assert!(!output.contains("other-tenant"));
    }

    #[tokio::test]
    async fn retryable_webhook_is_bounded_and_dead_letters_at_budget() {
        let (store, _) = store();
        let runtime = WebhookRuntime::new(store.clone());
        let receipt = runtime
            .receive(input("event-retry-budget"), &AcceptingVerifier)
            .await
            .unwrap();

        for _ in 0..5 {
            assert_eq!(
                runtime
                    .run_once("tenant-a", &Mapper, &RetryableFailure)
                    .await
                    .unwrap(),
                Some(receipt.inbox_id.clone())
            );
        }
        assert!(
            runtime
                .run_once("tenant-a", &Mapper, &RetryableFailure)
                .await
                .unwrap()
                .is_none()
        );
        let letters = store.list_webhook_dead_letters("tenant-a").unwrap();
        assert_eq!(letters.len(), 1);
        assert!(letters[0].reason.starts_with("retry_budget_exhausted:"));
    }

    #[tokio::test]
    async fn unverifiable_callback_fails_closed_and_dead_letter_replay_reenters_queue() {
        let (store, _) = store();
        let runtime = WebhookRuntime::new(store);
        assert_eq!(
            runtime
                .receive(input("rejected-a"), &RejectingVerifier)
                .await
                .unwrap_err(),
            IntegrationError::WebhookUnverifiable
        );
        let receipt = runtime
            .receive(input("event-b"), &AcceptingVerifier)
            .await
            .unwrap();
        runtime
            .run_once("tenant-a", &Mapper, &PermanentFailure)
            .await
            .unwrap();
        runtime
            .replay("tenant-a", &receipt.inbox_id, "operator-a", "fixed mapping")
            .unwrap();
        assert_eq!(
            runtime
                .run_once("tenant-a", &Mapper, &AcceptingApplication)
                .await
                .unwrap(),
            Some(receipt.inbox_id)
        );
    }
}

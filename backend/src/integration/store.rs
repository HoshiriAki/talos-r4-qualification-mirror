use std::collections::BTreeSet;
use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use super::catalog::ProviderCatalog;
use super::catalog_authority::IntegrationCatalogAuthority;
use super::catalog_repository::IntegrationCatalogRepository;
use super::deposit::{
    DepositLedgerEntryKind, DepositReconciliationOutcome, DepositState, RefundState,
    require_positive_minor_units,
};
use super::keystore::{InMemoryKeyStore, KeyStore, SecretEnvironment, SecretRegistrationScope};
use super::operation::{ExternalOperation, OperationState};
use super::runtime::{
    ClaimedOperation, DispatchResult, OperationRuntimeMetrics, OperationRuntimePolicy,
};
use super::scheduler_persistence_contract::{
    INTEGRATION_TENANT_PAGE_SIZE, INTEGRATION_WORKER_SCHEDULER_ID, StartupRecoveryPage,
};
use super::transport::{TransportErrorClass, TransportFailure};
use super::types::{
    CapabilityId, DepositId, ExternalAttemptId, ExternalOperationId, IntegrationError, Money,
    ProviderBinding, ProviderBindingId, ProviderHealth, ProviderId, ProviderInstance,
    ProviderInstanceId, ProviderLifecycle, ProviderManifest, ProviderReadiness, RefundId,
    WebhookEndpointId, WebhookInboxId,
};
use super::webhook::{ClaimedWebhook, WebhookEndpointContext};
pub use super::webhook_persistence_contract::{WebhookDeadLetterSummary, WebhookEndpointSummary};
use crate::observability::{
    FinancialEvent, IntegrationEvent, MetricsSink, NoopMetrics, OperationStateClass,
};

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn persistence<T>(_error: T) -> IntegrationError {
    IntegrationError::Persistence
}

fn tenant_work_page_after(
    tx: &Transaction<'_>,
    after_tenant_id: Option<&str>,
) -> Result<Vec<String>, IntegrationError> {
    // The UNION is deliberately scoped to durable worker records. In
    // particular, this is not a route or connector supplied tenant list.
    let mut statement = match after_tenant_id {
        Some(_) => tx.prepare(
            "SELECT tenant_id FROM (
                SELECT tenant_id FROM external_operations
                 WHERE state IN ('ready', 'retryable_failure', 'dispatching')
                UNION
                SELECT tenant_id FROM webhook_inbox
                 WHERE status IN ('verified', 'processing')
            ) WHERE tenant_id > ?1 ORDER BY tenant_id LIMIT ?2",
        ),
        None => tx.prepare(
            "SELECT tenant_id FROM (
                SELECT tenant_id FROM external_operations
                 WHERE state IN ('ready', 'retryable_failure', 'dispatching')
                UNION
                SELECT tenant_id FROM webhook_inbox
                 WHERE status IN ('verified', 'processing')
            ) ORDER BY tenant_id LIMIT ?1",
        ),
    }
    .map_err(persistence)?;
    match after_tenant_id {
        Some(tenant_id) => statement
            .query_map(params![tenant_id, INTEGRATION_TENANT_PAGE_SIZE], |row| {
                row.get::<_, String>(0)
            })
            .map_err(persistence)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(persistence),
        None => statement
            .query_map(params![INTEGRATION_TENANT_PAGE_SIZE], |row| {
                row.get::<_, String>(0)
            })
            .map_err(persistence)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(persistence),
    }
}

fn startup_recovery_snapshot_tenant_page(
    tx: &Transaction<'_>,
    recovery_id: &str,
) -> Result<Vec<String>, IntegrationError> {
    let mut statement = tx
        .prepare(
            "SELECT DISTINCT tenant_id
             FROM integration_startup_recovery_snapshot
             WHERE scheduler_id = ?1 AND recovery_id = ?2
             ORDER BY tenant_id
             LIMIT ?3",
        )
        .map_err(persistence)?;
    statement
        .query_map(
            params![
                INTEGRATION_WORKER_SCHEDULER_ID,
                recovery_id,
                INTEGRATION_TENANT_PAGE_SIZE
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(persistence)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(persistence)
}

fn validate_instance_against_manifest(
    instance: &ProviderInstance,
    manifest: &ProviderManifest,
) -> Result<(), IntegrationError> {
    let declared_config = manifest
        .config_schema
        .iter()
        .map(|field| field.name.as_str())
        .collect::<BTreeSet<_>>();
    let declared_secrets = manifest
        .secret_schema
        .iter()
        .map(|requirement| requirement.name.as_str())
        .collect::<BTreeSet<_>>();

    if instance
        .config
        .keys()
        .any(|field| !declared_config.contains(field.as_str()))
    {
        return Err(IntegrationError::InvalidManifest(
            "instance configuration contains a field not declared by the manifest".into(),
        ));
    }
    if instance.config.iter().any(|(name, value)| {
        manifest
            .config_schema
            .iter()
            .find(|field| field.name == *name)
            .is_none_or(|field| !field.value_type.accepts(value))
    }) {
        return Err(IntegrationError::InvalidManifest(
            "instance configuration does not match the manifest's non-secret value type".into(),
        ));
    }
    if manifest
        .config_schema
        .iter()
        .any(|field| field.required && !instance.config.contains_key(&field.name))
    {
        return Err(IntegrationError::InvalidManifest(
            "required instance configuration is missing".into(),
        ));
    }
    if instance
        .secret_refs
        .keys()
        .any(|field| !declared_secrets.contains(field.as_str()))
    {
        return Err(IntegrationError::InvalidManifest(
            "instance secret reference is not declared by the manifest".into(),
        ));
    }
    if manifest.secret_schema.iter().any(|requirement| {
        requirement.required && !instance.secret_refs.contains_key(&requirement.name)
    }) {
        return Err(IntegrationError::RequiredSecretMissing);
    }
    Ok(())
}

fn operation_state(value: &str) -> Result<OperationState, IntegrationError> {
    OperationState::from_persisted(value).ok_or(IntegrationError::Persistence)
}

fn state_value(value: &OperationState) -> &'static str {
    value.as_persisted()
}

fn operation_state_metric(value: &OperationState) -> OperationStateClass {
    match value {
        OperationState::Planned => OperationStateClass::Planned,
        OperationState::Ready => OperationStateClass::Ready,
        OperationState::Dispatching => OperationStateClass::Dispatching,
        OperationState::Succeeded => OperationStateClass::Succeeded,
        OperationState::Rejected => OperationStateClass::Rejected,
        OperationState::RetryableFailure => OperationStateClass::RetryableFailure,
        OperationState::NonRetryableFailure => OperationStateClass::NonRetryableFailure,
        OperationState::UnknownOutcome => OperationStateClass::UnknownOutcome,
        OperationState::Reconciling => OperationStateClass::Reconciling,
        OperationState::Resolved => OperationStateClass::Resolved,
        OperationState::ManualResolutionRequired => OperationStateClass::ManualResolutionRequired,
    }
}

fn transport_class_value(value: &TransportErrorClass) -> &'static str {
    value.as_persisted()
}

fn retry_timestamp(delay: std::time::Duration) -> Result<String, IntegrationError> {
    let delay = chrono::Duration::from_std(delay).map_err(persistence)?;
    Ok((chrono::Utc::now() + delay).to_rfc3339())
}

fn insert_runtime_event(
    tx: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    operation_id: &ExternalOperationId,
    attempt_id: Option<&ExternalAttemptId>,
    event_type: &str,
    classification: Option<&str>,
) -> Result<(), IntegrationError> {
    tx.execute(
        "INSERT INTO external_operation_runtime_events (id, tenant_id, operation_id, attempt_id, event_type, classification, occurred_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            uuid::Uuid::new_v4().to_string(),
            tenant_id,
            operation_id.as_str(),
            attempt_id.map(ExternalAttemptId::as_str),
            event_type,
            classification,
            now(),
        ],
    )
    .map_err(persistence)?;
    Ok(())
}

#[derive(Clone)]
pub struct IntegrationStore {
    pool: Pool<SqliteConnectionManager>,
    catalog_repository: IntegrationCatalogRepository,
    key_store: Arc<dyn KeyStore>,
    metrics: Arc<dyn MetricsSink>,
}

impl IntegrationStore {
    pub fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        // The application has no ambient secret registry. Production startup
        // must inject a configured KeyStore; the empty default fails closed.
        Self::with_key_store(pool, Arc::new(InMemoryKeyStore::default()))
    }

    pub fn with_key_store(
        pool: Pool<SqliteConnectionManager>,
        key_store: Arc<dyn KeyStore>,
    ) -> Self {
        Self::with_key_store_and_metrics(pool, key_store, Arc::new(NoopMetrics))
    }

    pub(crate) fn with_key_store_and_metrics(
        pool: Pool<SqliteConnectionManager>,
        key_store: Arc<dyn KeyStore>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Self {
        Self {
            catalog_repository: IntegrationCatalogRepository::new(pool.clone()),
            pool,
            key_store,
            metrics,
        }
    }

    pub(crate) fn catalog_authority(&self) -> IntegrationCatalogAuthority {
        IntegrationCatalogAuthority::new(self.catalog_repository.clone(), self.key_store.clone())
    }

    pub fn save_manifest(&self, manifest: &ProviderManifest) -> Result<(), IntegrationError> {
        manifest.validate()?;
        self.catalog_repository.save_manifest(manifest)
    }

    pub fn save_instance(&self, instance: &ProviderInstance) -> Result<(), IntegrationError> {
        instance.validate()?;
        let catalog = self.catalog_repository.catalog()?;
        let manifest = catalog
            .manifest(&instance.provider_id, &instance.manifest_version)
            .ok_or(IntegrationError::BindingUnavailable)?;
        validate_instance_against_manifest(instance, &manifest)?;

        let secret_environment = match instance.readiness {
            ProviderReadiness::Fixture => SecretEnvironment::Fixture,
            ProviderReadiness::Stub
            | ProviderReadiness::Sandbox
            | ProviderReadiness::Production => SecretEnvironment::Deployment,
        };
        let secret_registration =
            SecretRegistrationScope::new(instance.tenant_id.clone(), instance.id.clone())?;
        if instance.secret_refs.values().any(|secret_ref| {
            !self
                .key_store
                .accepts_reference(secret_ref, &secret_registration, secret_environment)
        }) {
            return Err(IntegrationError::SecretUnavailable);
        }

        self.catalog_repository.save_instance(instance)
    }

    pub fn save_binding(
        &self,
        binding: &ProviderBinding,
        actor_ref: &str,
    ) -> Result<(), IntegrationError> {
        binding.validate()?;
        self.catalog_repository.save_binding(binding, actor_ref)
    }

    pub fn catalog(&self) -> Result<ProviderCatalog, IntegrationError> {
        self.catalog_repository.catalog()
    }

    pub fn list_manifests(&self) -> Result<Vec<ProviderManifest>, IntegrationError> {
        Ok(self.catalog()?.manifests())
    }

    /// This intentionally returns internal instance values only to the
    /// Integration Module, which must redact `secret_refs` before responding.
    pub fn list_instances(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<ProviderInstance>, IntegrationError> {
        Ok(self.catalog()?.instances_for_tenant(tenant_id))
    }

    pub fn list_bindings(&self, tenant_id: &str) -> Result<Vec<ProviderBinding>, IntegrationError> {
        Ok(self.catalog()?.bindings_for_tenant(tenant_id))
    }

    pub fn persist_operation(
        &self,
        operation: &ExternalOperation,
    ) -> Result<ExternalOperationId, IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let existing: Option<(String, String)> = tx.query_row(
            "SELECT id, request_hash FROM external_operations WHERE tenant_id = ?1 AND binding_id = ?2 AND idempotency_key = ?3",
            params![operation.tenant_id, operation.binding_id, operation.idempotency_key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        ).optional().map_err(persistence)?;
        if let Some((id, request_hash)) = existing {
            if request_hash != operation.request_hash {
                return Err(IntegrationError::InvalidOperationTransition);
            }
            tx.commit().map_err(persistence)?;
            return ExternalOperationId::new(id);
        }
        let timestamp = now();
        tx.execute(
            "INSERT INTO external_operations (id, tenant_id, provider_instance_id, binding_id, binding_revision, capability_id, operation_type, idempotency_key, request_hash, state, attempt_count, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            params![operation.id.as_str(), operation.tenant_id, operation.provider_instance_id, operation.binding_id, operation.binding_revision, operation.capability, operation.operation_type, operation.idempotency_key, operation.request_hash, state_value(&operation.state), operation.attempts, timestamp],
        ).map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        self.metrics.integration_operation(
            IntegrationEvent::Admitted,
            operation_state_metric(&operation.state),
        );
        Ok(operation.id.clone())
    }

    /// Atomically claims a due operation only after validating its immutable
    /// binding revision and runtime guards. The runtime is fixture-only in this
    /// Stage 2 slice: no sandbox or production connector can be dispatched.
    pub fn claim_next_operation(
        &self,
        tenant_id: &str,
        policy: &OperationRuntimePolicy,
    ) -> Result<Option<ClaimedOperation>, IntegrationError> {
        policy.validate()?;
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let timestamp = now();
        let row: Option<(
            String,
            String,
            String,
            String,
            String,
            String,
            i64,
            String,
            bool,
            String,
            String,
            String,
            String,
            String,
        )> = tx
            .query_row(
                "SELECT o.id, o.binding_id, o.binding_revision, o.capability_id, o.operation_type, o.request_hash, o.attempt_count,
                        b.config_revision, b.enabled, i.config_revision, i.lifecycle, i.health, i.readiness, m.readiness
                 FROM external_operations o
                 JOIN provider_bindings b ON b.tenant_id = o.tenant_id AND b.id = o.binding_id
                 JOIN provider_instances i ON i.tenant_id = b.tenant_id AND i.id = b.provider_instance_id
                 JOIN provider_manifests m ON m.provider_id = i.provider_id AND m.version = i.manifest_version
                 WHERE o.tenant_id = ?1
                   AND o.state IN ('ready', 'retryable_failure')
                   AND (o.next_retry_at IS NULL OR o.next_retry_at <= ?2)
                 ORDER BY o.created_at, o.id
                 LIMIT 1",
                params![tenant_id, timestamp],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                        row.get(13)?,
                    ))
                },
            )
            .optional()
            .map_err(persistence)?;
        let Some((
            operation_id,
            binding_id,
            binding_revision,
            capability,
            operation_type,
            request_hash,
            attempts,
            current_binding_revision,
            enabled,
            current_instance_revision,
            lifecycle,
            health,
            readiness,
            manifest_readiness,
        )) = row
        else {
            tx.commit().map_err(persistence)?;
            return Ok(None);
        };
        let operation_id = ExternalOperationId::new(operation_id)?;
        let binding_id = ProviderBindingId::new(binding_id)?;
        if binding_revision != current_binding_revision
            || binding_revision != current_instance_revision
        {
            tx.execute(
                "UPDATE external_operations SET state = 'manual_resolution_required', classification = 'binding_revision_stale', updated_at = ?1 WHERE tenant_id = ?2 AND id = ?3",
                params![timestamp, tenant_id, operation_id.as_str()],
            )
            .map_err(persistence)?;
            insert_runtime_event(
                &tx,
                tenant_id,
                &operation_id,
                None,
                "binding_revision_mismatch",
                Some("binding_revision_stale"),
            )?;
            tx.commit().map_err(persistence)?;
            return Err(IntegrationError::BindingRevisionStale);
        }
        if !enabled || lifecycle != "active" || health != "ready" {
            tx.commit().map_err(persistence)?;
            return Err(IntegrationError::InstanceNotReady);
        }
        // Explicitly stop before any non-fixture Provider effect. SP-21/SP-22
        // remain deferred and no production adapter is attached in Stage 2.
        if readiness != "fixture" || manifest_readiness != "fixture" {
            tx.commit().map_err(persistence)?;
            return Err(IntegrationError::ModeBlocked);
        }
        let circuit: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT state, opened_until FROM integration_circuit_state WHERE tenant_id = ?1 AND binding_id = ?2",
                params![tenant_id, binding_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(persistence)?;
        if let Some((state, Some(opened_until))) = circuit {
            if state == "open" && opened_until > timestamp {
                tx.commit().map_err(persistence)?;
                return Err(IntegrationError::CircuitOpen);
            }
            if state == "open" {
                tx.execute(
                    "UPDATE integration_circuit_state SET state = 'half_open', opened_until = NULL, updated_at = ?1 WHERE tenant_id = ?2 AND binding_id = ?3",
                    params![timestamp, tenant_id, binding_id.as_str()],
                )
                .map_err(persistence)?;
            }
        }
        let in_flight: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM external_operations WHERE tenant_id = ?1 AND binding_id = ?2 AND state = 'dispatching'",
                params![tenant_id, binding_id.as_str()],
                |row| row.get(0),
            )
            .map_err(persistence)?;
        if in_flight >= i64::from(policy.max_in_flight_per_binding) {
            insert_runtime_event(
                &tx,
                tenant_id,
                &operation_id,
                None,
                "concurrency_limited",
                None,
            )?;
            tx.commit().map_err(persistence)?;
            return Err(IntegrationError::ConcurrencyLimited);
        }
        let window_start = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
        let recent_attempts: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM external_operation_attempts a
                 JOIN external_operations o ON o.tenant_id = a.tenant_id AND o.id = a.operation_id
                 WHERE a.tenant_id = ?1 AND o.binding_id = ?2 AND a.started_at >= ?3",
                params![tenant_id, binding_id.as_str(), window_start],
                |row| row.get(0),
            )
            .map_err(persistence)?;
        if recent_attempts >= i64::from(policy.max_attempts_per_minute) {
            insert_runtime_event(&tx, tenant_id, &operation_id, None, "rate_limited", None)?;
            tx.commit().map_err(persistence)?;
            return Err(IntegrationError::RateLimited);
        }
        let attempt_number = u32::try_from(attempts)
            .map_err(persistence)?
            .checked_add(1)
            .ok_or(IntegrationError::InvalidOperationTransition)?;
        let attempt_id = ExternalAttemptId::new(uuid::Uuid::new_v4().to_string())?;
        tx.execute(
            "UPDATE external_operations SET state = 'dispatching', attempt_count = ?1, next_retry_at = NULL, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4",
            params![attempt_number, timestamp, tenant_id, operation_id.as_str()],
        )
        .map_err(persistence)?;
        tx.execute(
            "INSERT INTO external_operation_attempts (id, tenant_id, operation_id, attempt_number, state, request_hash, started_at) VALUES (?1, ?2, ?3, ?4, 'dispatching', ?5, ?6)",
            params![attempt_id.as_str(), tenant_id, operation_id.as_str(), attempt_number, request_hash, timestamp],
        )
        .map_err(persistence)?;
        insert_runtime_event(
            &tx,
            tenant_id,
            &operation_id,
            Some(&attempt_id),
            "claimed",
            None,
        )?;
        tx.commit().map_err(persistence)?;
        Ok(Some(ClaimedOperation {
            tenant_id: tenant_id.to_owned(),
            operation_id,
            attempt_id,
            binding_id,
            binding_revision,
            capability,
            operation_type,
            request_hash,
            attempt_number,
        }))
    }

    /// Records an attempted effect. A potentially dispatched failure can only
    /// reach `UnknownOutcome`; the only path to `RetryableFailure` is a
    /// confirmed before-dispatch transport failure.
    pub fn record_runtime_outcome(
        &self,
        claimed: &ClaimedOperation,
        outcome: Result<DispatchResult, TransportFailure>,
        policy: &OperationRuntimePolicy,
    ) -> Result<(), IntegrationError> {
        policy.validate()?;
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let current: String = tx
            .query_row(
                "SELECT state FROM external_operations WHERE tenant_id = ?1 AND id = ?2",
                params![claimed.tenant_id, claimed.operation_id.as_str()],
                |row| row.get(0),
            )
            .map_err(persistence)?;
        if operation_state(&current)? != OperationState::Dispatching {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let (state, classification, result_ref, next_retry_at, failure, event_type) = match outcome
        {
            Ok(DispatchResult::Succeeded {
                provider_result_ref,
            }) => (
                OperationState::Succeeded,
                None,
                provider_result_ref,
                None,
                false,
                None,
            ),
            Ok(DispatchResult::Rejected {
                provider_result_ref,
            }) => (
                OperationState::Rejected,
                None,
                provider_result_ref,
                None,
                false,
                None,
            ),
            Err(failure) if failure.may_have_dispatched => (
                OperationState::UnknownOutcome,
                Some(transport_class_value(&failure.class)),
                None,
                None,
                true,
                Some("unknown_outcome"),
            ),
            Err(failure)
                if failure.is_retryable() && claimed.attempt_number < policy.retry.max_attempts =>
            {
                let retry_at =
                    retry_timestamp(policy.retry.delay_for_attempt(claimed.attempt_number))?;
                (
                    OperationState::RetryableFailure,
                    Some(transport_class_value(&failure.class)),
                    None,
                    Some(retry_at),
                    true,
                    Some("retry_scheduled"),
                )
            }
            Err(failure) => (
                OperationState::NonRetryableFailure,
                Some(transport_class_value(&failure.class)),
                None,
                None,
                true,
                None,
            ),
        };
        let timestamp = now();
        tx.execute(
            "UPDATE external_operation_attempts SET state = ?1, classification = ?2, provider_result_ref = ?3, completed_at = ?4 WHERE id = ?5 AND tenant_id = ?6 AND operation_id = ?7 AND state = 'dispatching'",
            params![state_value(&state), classification, result_ref, timestamp, claimed.attempt_id.as_str(), claimed.tenant_id, claimed.operation_id.as_str()],
        )
        .map_err(persistence)?;
        tx.execute(
            "UPDATE external_operations SET state = ?1, classification = ?2, result_ref = ?3, next_retry_at = ?4, updated_at = ?5 WHERE tenant_id = ?6 AND id = ?7",
            params![state_value(&state), classification, result_ref, next_retry_at, timestamp, claimed.tenant_id, claimed.operation_id.as_str()],
        )
        .map_err(persistence)?;
        if let Some(event_type) = event_type {
            insert_runtime_event(
                &tx,
                &claimed.tenant_id,
                &claimed.operation_id,
                Some(&claimed.attempt_id),
                event_type,
                classification,
            )?;
        }
        if failure {
            let existing: Option<i64> = tx
                .query_row(
                    "SELECT failure_count FROM integration_circuit_state WHERE tenant_id = ?1 AND binding_id = ?2",
                    params![claimed.tenant_id, claimed.binding_id.as_str()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(persistence)?;
            let failures = existing.unwrap_or(0) + 1;
            if failures >= i64::from(policy.circuit_failure_threshold) {
                let open_until = retry_timestamp(policy.circuit_open_for)?;
                tx.execute(
                    "INSERT INTO integration_circuit_state (tenant_id, binding_id, state, failure_count, opened_until, updated_at) VALUES (?1, ?2, 'open', ?3, ?4, ?5)
                     ON CONFLICT(tenant_id, binding_id) DO UPDATE SET state = 'open', failure_count = excluded.failure_count, opened_until = excluded.opened_until, updated_at = excluded.updated_at",
                    params![claimed.tenant_id, claimed.binding_id.as_str(), failures, open_until, timestamp],
                )
                .map_err(persistence)?;
                insert_runtime_event(
                    &tx,
                    &claimed.tenant_id,
                    &claimed.operation_id,
                    Some(&claimed.attempt_id),
                    "circuit_opened",
                    classification,
                )?;
            } else {
                tx.execute(
                    "INSERT INTO integration_circuit_state (tenant_id, binding_id, state, failure_count, opened_until, updated_at) VALUES (?1, ?2, 'closed', ?3, NULL, ?4)
                     ON CONFLICT(tenant_id, binding_id) DO UPDATE SET state = 'closed', failure_count = excluded.failure_count, opened_until = NULL, updated_at = excluded.updated_at",
                    params![claimed.tenant_id, claimed.binding_id.as_str(), failures, timestamp],
                )
                .map_err(persistence)?;
            }
        } else {
            tx.execute(
                "INSERT INTO integration_circuit_state (tenant_id, binding_id, state, failure_count, opened_until, updated_at) VALUES (?1, ?2, 'closed', 0, NULL, ?3)
                 ON CONFLICT(tenant_id, binding_id) DO UPDATE SET state = 'closed', failure_count = 0, opened_until = NULL, updated_at = excluded.updated_at",
                params![claimed.tenant_id, claimed.binding_id.as_str(), timestamp],
            )
            .map_err(persistence)?;
            insert_runtime_event(
                &tx,
                &claimed.tenant_id,
                &claimed.operation_id,
                Some(&claimed.attempt_id),
                "circuit_closed",
                None,
            )?;
        }
        tx.commit().map_err(persistence)
    }

    /// Captures the in-flight work that existed before this process starts
    /// ordinary scheduling. The snapshot is worker-owned operational metadata:
    /// it has no route or connector-supplied tenant authority. A fresh startup
    /// replaces an unfinished snapshot so it can safely capture any work left
    /// in-flight by an interrupted earlier startup.
    pub(crate) fn begin_startup_recovery_snapshot(&self) -> Result<String, IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let recovery_id = uuid::Uuid::new_v4().to_string();
        let timestamp = now();
        tx.execute(
            "DELETE FROM integration_startup_recovery_snapshot WHERE scheduler_id = ?1",
            params![INTEGRATION_WORKER_SCHEDULER_ID],
        )
        .map_err(persistence)?;
        tx.execute(
            "INSERT INTO integration_startup_recovery_snapshot
                (scheduler_id, recovery_id, tenant_id, work_kind, work_id, captured_at)
             SELECT ?1, ?2, tenant_id, 'external_operation', id, ?3
             FROM external_operations
             WHERE state = 'dispatching'",
            params![INTEGRATION_WORKER_SCHEDULER_ID, recovery_id, timestamp],
        )
        .map_err(persistence)?;
        tx.execute(
            "INSERT INTO integration_startup_recovery_snapshot
                (scheduler_id, recovery_id, tenant_id, work_kind, work_id, captured_at)
             SELECT ?1, ?2, tenant_id, 'webhook', id, ?3
             FROM webhook_inbox
             WHERE status = 'processing'",
            params![INTEGRATION_WORKER_SCHEDULER_ID, recovery_id, timestamp],
        )
        .map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        Ok(recovery_id)
    }

    /// Drains one bounded tenant page from a startup snapshot. Deleting the
    /// page inside the same transaction makes the next invocation advance
    /// deterministically without rescanning an already recovered tenant. Only
    /// exact identifiers captured at startup are eligible for recovery, so a
    /// periodic worker never reclassifies new in-flight work as crash residue.
    pub(crate) fn recover_next_startup_snapshot_page(
        &self,
        recovery_id: &str,
    ) -> Result<StartupRecoveryPage, IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let tenant_ids = startup_recovery_snapshot_tenant_page(&tx, recovery_id)?;
        if tenant_ids.is_empty() {
            tx.commit().map_err(persistence)?;
            return Ok(StartupRecoveryPage::default());
        }

        let timestamp = now();
        let mut page = StartupRecoveryPage {
            tenants: tenant_ids.len() as u64,
            ..Default::default()
        };
        for tenant_id in &tenant_ids {
            let operation_ids = {
                let mut statement = tx
                    .prepare(
                        "SELECT work_id FROM integration_startup_recovery_snapshot
                         WHERE scheduler_id = ?1 AND recovery_id = ?2
                           AND tenant_id = ?3 AND work_kind = 'external_operation'",
                    )
                    .map_err(persistence)?;
                statement
                    .query_map(
                        params![INTEGRATION_WORKER_SCHEDULER_ID, recovery_id, tenant_id],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(persistence)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(persistence)?
            };
            for id in operation_ids {
                let operation_id = ExternalOperationId::new(id)?;
                let changed = tx
                    .execute(
                        "UPDATE external_operations
                         SET state = 'unknown_outcome',
                             classification = 'worker_restarted_after_dispatch',
                             next_retry_at = NULL,
                             updated_at = ?1
                         WHERE tenant_id = ?2 AND id = ?3 AND state = 'dispatching'",
                        params![timestamp, tenant_id, operation_id.as_str()],
                    )
                    .map_err(persistence)?;
                if changed == 1 {
                    tx.execute(
                        "UPDATE external_operation_attempts
                         SET state = 'unknown_outcome',
                             classification = 'worker_restarted_after_dispatch',
                             completed_at = ?1
                         WHERE tenant_id = ?2 AND operation_id = ?3
                           AND state = 'dispatching'",
                        params![timestamp, tenant_id, operation_id.as_str()],
                    )
                    .map_err(persistence)?;
                    insert_runtime_event(
                        &tx,
                        tenant_id,
                        &operation_id,
                        None,
                        "recovered_after_restart",
                        Some("worker_restarted_after_dispatch"),
                    )?;
                    page.recovered_operations += 1;
                }
            }

            let webhook_ids = {
                let mut statement = tx
                    .prepare(
                        "SELECT work_id FROM integration_startup_recovery_snapshot
                         WHERE scheduler_id = ?1 AND recovery_id = ?2
                           AND tenant_id = ?3 AND work_kind = 'webhook'",
                    )
                    .map_err(persistence)?;
                statement
                    .query_map(
                        params![INTEGRATION_WORKER_SCHEDULER_ID, recovery_id, tenant_id],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(persistence)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(persistence)?
            };
            for inbox_id in webhook_ids {
                let changed = tx
                    .execute(
                        "UPDATE webhook_inbox
                         SET status = 'verified',
                             error_classification = 'worker_restarted_during_processing'
                         WHERE tenant_id = ?1 AND id = ?2 AND status = 'processing'",
                        params![tenant_id, inbox_id],
                    )
                    .map_err(persistence)?;
                if changed == 1 {
                    tx.execute(
                        "UPDATE webhook_processing_attempts
                         SET state = 'retryable_failure',
                             classification = 'worker_restarted_during_processing',
                             completed_at = ?1
                         WHERE tenant_id = ?2 AND inbox_id = ?3 AND state = 'processing'",
                        params![timestamp, tenant_id, inbox_id],
                    )
                    .map_err(persistence)?;
                    page.recovered_webhooks += 1;
                }
            }

            tx.execute(
                "DELETE FROM integration_startup_recovery_snapshot
                 WHERE scheduler_id = ?1 AND recovery_id = ?2 AND tenant_id = ?3",
                params![INTEGRATION_WORKER_SCHEDULER_ID, recovery_id, tenant_id],
            )
            .map_err(persistence)?;
        }
        tx.commit().map_err(persistence)?;
        Ok(page)
    }

    pub fn recover_inflight_operations(&self, tenant_id: &str) -> Result<u64, IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let rows = {
            let mut statement = tx
                .prepare(
                    "SELECT id FROM external_operations WHERE tenant_id = ?1 AND state = 'dispatching'",
                )
                .map_err(persistence)?;
            statement
                .query_map(params![tenant_id], |row| row.get::<_, String>(0))
                .map_err(persistence)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(persistence)?
        };
        let timestamp = now();
        for id in &rows {
            let operation_id = ExternalOperationId::new(id.clone())?;
            tx.execute(
                "UPDATE external_operations SET state = 'unknown_outcome', classification = 'worker_restarted_after_dispatch', next_retry_at = NULL, updated_at = ?1 WHERE tenant_id = ?2 AND id = ?3",
                params![timestamp, tenant_id, operation_id.as_str()],
            )
            .map_err(persistence)?;
            tx.execute(
                "UPDATE external_operation_attempts SET state = 'unknown_outcome', classification = 'worker_restarted_after_dispatch', completed_at = ?1 WHERE tenant_id = ?2 AND operation_id = ?3 AND state = 'dispatching'",
                params![timestamp, tenant_id, operation_id.as_str()],
            )
            .map_err(persistence)?;
            insert_runtime_event(
                &tx,
                tenant_id,
                &operation_id,
                None,
                "recovered_after_restart",
                Some("worker_restarted_after_dispatch"),
            )?;
        }
        tx.commit().map_err(persistence)?;
        Ok(rows.len() as u64)
    }

    /// Returns one bounded, keyset-paginated page of internal worker work.
    ///
    /// The cursor is persisted with the durable Integration records so a
    /// restart continues after the last serviced tenant. Once no greater key
    /// remains, the next page wraps to the beginning. Therefore every tenant
    /// in a finite admitted-work set is selected after a bounded number of
    /// ticks; unavailable work from one tenant cannot keep later tenants out
    /// of discovery. This function never accepts tenant authority from a
    /// route or connector.
    pub fn next_tenant_work_page(&self) -> Result<Vec<String>, IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let cursor = tx
            .query_row(
                "SELECT tenant_id FROM integration_scheduler_cursor WHERE scheduler_id = ?1",
                params![INTEGRATION_WORKER_SCHEDULER_ID],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(persistence)?
            .flatten();
        let mut tenants = tenant_work_page_after(&tx, cursor.as_deref())?;
        if tenants.is_empty() && cursor.is_some() {
            tenants = tenant_work_page_after(&tx, None)?;
        }
        let next_cursor = tenants.last().cloned();
        tx.execute(
            "INSERT INTO integration_scheduler_cursor (scheduler_id, tenant_id, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(scheduler_id) DO UPDATE SET
                 tenant_id = excluded.tenant_id,
                 updated_at = excluded.updated_at",
            params![INTEGRATION_WORKER_SCHEDULER_ID, next_cursor, now()],
        )
        .map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        Ok(tenants)
    }

    /// A worker crash after claiming an inbound event did not invoke a
    /// business write directly. The safe recovery is to place the durable
    /// inbox item back in the normal verified queue with an attempt record.
    pub fn recover_inflight_webhooks(&self, tenant_id: &str) -> Result<u64, IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let rows = {
            let mut statement = tx
                .prepare(
                    "SELECT id FROM webhook_inbox WHERE tenant_id = ?1 AND status = 'processing'",
                )
                .map_err(persistence)?;
            statement
                .query_map([tenant_id], |row| row.get::<_, String>(0))
                .map_err(persistence)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(persistence)?
        };
        let timestamp = now();
        for inbox_id in &rows {
            tx.execute(
                "UPDATE webhook_inbox
                 SET status = 'verified', error_classification = 'worker_restarted_during_processing'
                 WHERE tenant_id = ?1 AND id = ?2 AND status = 'processing'",
                params![tenant_id, inbox_id],
            )
            .map_err(persistence)?;
            tx.execute(
                "UPDATE webhook_processing_attempts
                 SET state = 'retryable_failure', classification = 'worker_restarted_during_processing', completed_at = ?1
                 WHERE tenant_id = ?2 AND inbox_id = ?3 AND state = 'processing'",
                params![timestamp, tenant_id, inbox_id],
            )
            .map_err(persistence)?;
        }
        tx.commit().map_err(persistence)?;
        Ok(rows.len() as u64)
    }

    pub fn operation_runtime_metrics(
        &self,
        tenant_id: &str,
    ) -> Result<OperationRuntimeMetrics, IntegrationError> {
        let conn = self.pool.get().map_err(persistence)?;
        let count = |state: &str| -> Result<u64, IntegrationError> {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM external_operations WHERE tenant_id = ?1 AND state = ?2",
                    params![tenant_id, state],
                    |row| row.get(0),
                )
                .map_err(persistence)?;
            u64::try_from(count).map_err(persistence)
        };
        let circuit_open: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM integration_circuit_state WHERE tenant_id = ?1 AND state = 'open'",
                params![tenant_id],
                |row| row.get(0),
            )
            .map_err(persistence)?;
        Ok(OperationRuntimeMetrics {
            ready: count("ready")?,
            dispatching: count("dispatching")?,
            retryable_failure: count("retryable_failure")?,
            unknown_outcome: count("unknown_outcome")?,
            circuit_open: u64::try_from(circuit_open).map_err(persistence)?,
        })
    }

    #[cfg(test)]
    pub fn start_attempt(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
    ) -> Result<ExternalAttemptId, IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let (state, count, request_hash): (String, i64, String) = tx.query_row(
            "SELECT state, attempt_count, request_hash FROM external_operations WHERE tenant_id = ?1 AND id = ?2", params![tenant_id, operation_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        ).map_err(persistence)?;
        if !matches!(
            operation_state(&state)?,
            OperationState::Ready | OperationState::RetryableFailure
        ) {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let attempt_number = count + 1;
        let attempt_id = ExternalAttemptId::new(uuid::Uuid::new_v4().to_string())?;
        let timestamp = now();
        tx.execute("UPDATE external_operations SET state = 'dispatching', attempt_count = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4", params![attempt_number, timestamp, tenant_id, operation_id.as_str()]).map_err(persistence)?;
        tx.execute("INSERT INTO external_operation_attempts (id, tenant_id, operation_id, attempt_number, state, request_hash, started_at) VALUES (?1, ?2, ?3, ?4, 'dispatching', ?5, ?6)", params![attempt_id.as_str(), tenant_id, operation_id.as_str(), attempt_number, request_hash, timestamp]).map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        Ok(attempt_id)
    }

    #[cfg(test)]
    pub fn open_circuit_for_test(
        &self,
        tenant_id: &str,
        binding_id: &ProviderBindingId,
        opened_until: &str,
    ) -> Result<(), IntegrationError> {
        let conn = self.pool.get().map_err(persistence)?;
        conn.execute(
            "INSERT INTO integration_circuit_state (tenant_id, binding_id, state, failure_count, opened_until, updated_at)
             VALUES (?1, ?2, 'open', 3, ?3, ?4)",
            params![tenant_id, binding_id.as_str(), opened_until, now()],
        )
        .map_err(persistence)?;
        Ok(())
    }

    #[cfg(test)]
    pub fn complete_attempt(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
        attempt_id: &ExternalAttemptId,
        state: OperationState,
        classification: Option<&str>,
        result_ref: Option<&str>,
    ) -> Result<(), IntegrationError> {
        if !matches!(
            state,
            OperationState::Succeeded
                | OperationState::Rejected
                | OperationState::RetryableFailure
                | OperationState::NonRetryableFailure
                | OperationState::UnknownOutcome
                | OperationState::ManualResolutionRequired
        ) {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let current: String = tx
            .query_row(
                "SELECT state FROM external_operations WHERE tenant_id = ?1 AND id = ?2",
                params![tenant_id, operation_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .map_err(persistence)?;
        if operation_state(&current)? != OperationState::Dispatching {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let timestamp = now();
        tx.execute("UPDATE external_operation_attempts SET state = ?1, classification = ?2, provider_result_ref = ?3, completed_at = ?4 WHERE id = ?5 AND tenant_id = ?6 AND operation_id = ?7", params![state_value(&state), classification, result_ref, timestamp, attempt_id.as_str(), tenant_id, operation_id.as_str()]).map_err(persistence)?;
        tx.execute("UPDATE external_operations SET state = ?1, classification = ?2, result_ref = ?3, updated_at = ?4 WHERE tenant_id = ?5 AND id = ?6", params![state_value(&state), classification, result_ref, timestamp, tenant_id, operation_id.as_str()]).map_err(persistence)?;
        tx.commit().map_err(persistence)
    }

    pub fn begin_reconciliation(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
        evidence_ref: &str,
    ) -> Result<(), IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let updated = tx.execute("UPDATE external_operations SET state = 'reconciling', updated_at = ?1 WHERE tenant_id = ?2 AND id = ?3 AND state = 'unknown_outcome'", params![now(), tenant_id, operation_id.as_str()]).map_err(persistence)?;
        if updated != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.execute("INSERT INTO external_operation_reconciliations (id, tenant_id, operation_id, outcome, evidence_ref, created_at) VALUES (?1, ?2, ?3, 'pending', ?4, ?5)", params![uuid::Uuid::new_v4().to_string(), tenant_id, operation_id.as_str(), evidence_ref, now()]).map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        self.metrics.integration_operation(
            IntegrationEvent::ReconciliationStarted,
            OperationStateClass::Reconciling,
        );
        Ok(())
    }

    pub fn resolve_reconciliation(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
        effect_confirmed: bool,
        actor_ref: &str,
    ) -> Result<(), IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let next = if effect_confirmed {
            "resolved"
        } else {
            "ready"
        };
        let updated = tx.execute("UPDATE external_operations SET state = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4 AND state = 'reconciling'", params![next, now(), tenant_id, operation_id.as_str()]).map_err(persistence)?;
        if updated != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.execute("UPDATE external_operation_reconciliations SET outcome = ?1, resolved_by = ?2, resolved_at = ?3 WHERE tenant_id = ?4 AND operation_id = ?5 AND outcome = 'pending'", params![if effect_confirmed { "effect_confirmed" } else { "effect_absent" }, actor_ref, now(), tenant_id, operation_id.as_str()]).map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        self.metrics.integration_operation(
            IntegrationEvent::ReconciliationResolved,
            if effect_confirmed {
                OperationStateClass::Resolved
            } else {
                OperationStateClass::Ready
            },
        );
        Ok(())
    }

    pub fn register_webhook_endpoint(
        &self,
        tenant_id: &str,
        binding_id: &ProviderBindingId,
        token: &[u8],
    ) -> Result<WebhookEndpointId, IntegrationError> {
        if token.is_empty() {
            return Err(IntegrationError::WebhookUnverifiable);
        }
        let endpoint_id = WebhookEndpointId::new(uuid::Uuid::new_v4().to_string())?;
        let token_hash = hex::encode(Sha256::digest(token));
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let binding_context: Option<(String, bool, String, String, String, String)> = tx
            .query_row(
                "SELECT i.provider_id, b.enabled, i.lifecycle, i.health, i.readiness, m.readiness
                 FROM provider_bindings b
                 JOIN provider_instances i ON i.tenant_id = b.tenant_id AND i.id = b.provider_instance_id
                 JOIN provider_manifests m ON m.provider_id = i.provider_id AND m.version = i.manifest_version
                 WHERE b.tenant_id = ?1 AND b.id = ?2",
                params![tenant_id, binding_id.as_str()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()
            .map_err(persistence)?;
        let Some((
            provider_id,
            binding_enabled,
            lifecycle,
            health,
            instance_readiness,
            manifest_readiness,
        )) = binding_context
        else {
            return Err(IntegrationError::BindingUnavailable);
        };
        if !binding_enabled
            || lifecycle != "active"
            || health != "ready"
            || instance_readiness != "fixture"
            || manifest_readiness != "fixture"
        {
            return Err(IntegrationError::BindingUnavailable);
        }
        tx.execute("INSERT INTO webhook_endpoints (id, tenant_id, binding_id, provider_id, token_hash, enabled, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)", params![endpoint_id.as_str(), tenant_id, binding_id.as_str(), provider_id, token_hash, now()]).map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        Ok(endpoint_id)
    }

    pub fn list_webhook_endpoints(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookEndpointSummary>, IntegrationError> {
        let conn = self.pool.get().map_err(persistence)?;
        let mut statement = conn
            .prepare(
                "SELECT id, binding_id, provider_id, enabled, created_at
                 FROM webhook_endpoints WHERE tenant_id = ?1 ORDER BY created_at DESC, id DESC",
            )
            .map_err(persistence)?;
        statement
            .query_map([tenant_id], |row| {
                Ok(WebhookEndpointSummary {
                    endpoint_id: WebhookEndpointId::new(row.get::<_, String>(0)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    binding_id: ProviderBindingId::new(row.get::<_, String>(1)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    provider_id: ProviderId::new(row.get::<_, String>(2)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    enabled: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(persistence)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(persistence)
    }

    pub fn set_webhook_endpoint_enabled(
        &self,
        tenant_id: &str,
        endpoint_id: &WebhookEndpointId,
        enabled: bool,
    ) -> Result<(), IntegrationError> {
        let conn = self.pool.get().map_err(persistence)?;
        let changed = conn
            .execute(
                "UPDATE webhook_endpoints SET enabled = ?1 WHERE tenant_id = ?2 AND id = ?3",
                params![enabled, tenant_id, endpoint_id.as_str()],
            )
            .map_err(persistence)?;
        if changed != 1 {
            return Err(IntegrationError::BindingUnavailable);
        }
        Ok(())
    }

    pub fn resolve_webhook_endpoint(
        &self,
        token: &[u8],
    ) -> Result<WebhookEndpointContext, IntegrationError> {
        if token.is_empty() {
            return Err(IntegrationError::WebhookUnverifiable);
        }
        let token_hash = hex::encode(Sha256::digest(token));
        let conn = self.pool.get().map_err(persistence)?;
        let endpoint: Option<(
            String,
            String,
            String,
            String,
            bool,
            bool,
            String,
            String,
            String,
            String,
        )> = conn
            .query_row(
                "SELECT e.id, e.tenant_id, e.binding_id, e.provider_id, e.enabled, b.enabled, i.lifecycle, i.health, i.readiness, m.readiness
                 FROM webhook_endpoints e
                 JOIN provider_bindings b ON b.tenant_id = e.tenant_id AND b.id = e.binding_id
                 JOIN provider_instances i ON i.tenant_id = b.tenant_id AND i.id = b.provider_instance_id
                 JOIN provider_manifests m ON m.provider_id = i.provider_id AND m.version = i.manifest_version
                 WHERE e.token_hash = ?1",
                params![token_hash],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ))
                },
            )
            .optional()
            .map_err(persistence)?;
        let Some((
            endpoint_id,
            tenant_id,
            binding_id,
            provider_id,
            endpoint_enabled,
            binding_enabled,
            lifecycle,
            health,
            instance_readiness,
            manifest_readiness,
        )) = endpoint
        else {
            return Err(IntegrationError::BindingUnavailable);
        };
        if !endpoint_enabled
            || !binding_enabled
            || lifecycle != "active"
            || health != "ready"
            || instance_readiness != "fixture"
            || manifest_readiness != "fixture"
        {
            return Err(IntegrationError::BindingUnavailable);
        }
        Ok(WebhookEndpointContext {
            endpoint_id: WebhookEndpointId::new(endpoint_id)?,
            tenant_id,
            binding_id: ProviderBindingId::new(binding_id)?,
            provider_id: ProviderId::new(provider_id)?,
        })
    }

    pub fn record_verified_webhook(
        &self,
        endpoint: &WebhookEndpointContext,
        provider_event_id: &str,
        headers_json: &str,
        raw_payload: &[u8],
    ) -> Result<(WebhookInboxId, bool), IntegrationError> {
        if provider_event_id.trim().is_empty() || raw_payload.is_empty() {
            return Err(IntegrationError::WebhookUnverifiable);
        }
        let conn = self.pool.get().map_err(persistence)?;
        let inbox_id = WebhookInboxId::new(uuid::Uuid::new_v4().to_string())?;
        let inserted = conn.execute("INSERT OR IGNORE INTO webhook_inbox (id, tenant_id, endpoint_id, provider_event_id, payload_hash, headers_json, raw_payload, received_at, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'verified')", params![inbox_id.as_str(), endpoint.tenant_id, endpoint.endpoint_id.as_str(), provider_event_id, hex::encode(Sha256::digest(raw_payload)), headers_json, raw_payload, now()]).map_err(persistence)?;
        if inserted == 1 {
            return Ok((inbox_id, false));
        }
        let existing: String = conn.query_row("SELECT id FROM webhook_inbox WHERE tenant_id = ?1 AND endpoint_id = ?2 AND provider_event_id = ?3", params![endpoint.tenant_id, endpoint.endpoint_id.as_str(), provider_event_id], |row| row.get(0)).map_err(persistence)?;
        Ok((WebhookInboxId::new(existing)?, true))
    }

    pub fn record_rejected_webhook(
        &self,
        endpoint: &WebhookEndpointContext,
        provider_event_id: &str,
        headers_json: &str,
        raw_payload: &[u8],
        classification: &str,
    ) -> Result<(), IntegrationError> {
        if provider_event_id.trim().is_empty()
            || raw_payload.is_empty()
            || classification.trim().is_empty()
        {
            return Err(IntegrationError::WebhookUnverifiable);
        }
        let conn = self.pool.get().map_err(persistence)?;
        conn.execute(
            "INSERT OR IGNORE INTO webhook_inbox (id, tenant_id, endpoint_id, provider_event_id, payload_hash, headers_json, raw_payload, received_at, status, error_classification) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'rejected', ?9)",
            params![uuid::Uuid::new_v4().to_string(), endpoint.tenant_id, endpoint.endpoint_id.as_str(), provider_event_id, hex::encode(Sha256::digest(raw_payload)), headers_json, raw_payload, now(), classification],
        )
        .map_err(persistence)?;
        Ok(())
    }

    pub fn claim_next_webhook(
        &self,
        tenant_id: &str,
    ) -> Result<Option<ClaimedWebhook>, IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let row: Option<(String, String, String, String, String, String, Vec<u8>)> = tx
            .query_row(
                "SELECT i.id, i.endpoint_id, e.binding_id, e.provider_id, i.provider_event_id, i.headers_json, i.raw_payload
                 FROM webhook_inbox i
                 JOIN webhook_endpoints e ON e.tenant_id = i.tenant_id AND e.id = i.endpoint_id
                 JOIN provider_bindings b ON b.tenant_id = e.tenant_id AND b.id = e.binding_id
                 JOIN provider_instances p ON p.tenant_id = b.tenant_id AND p.id = b.provider_instance_id
                 JOIN provider_manifests m ON m.provider_id = p.provider_id AND m.version = p.manifest_version
                 WHERE i.tenant_id = ?1 AND i.status = 'verified'
                   AND e.enabled = 1 AND b.enabled = 1 AND p.lifecycle = 'active' AND p.health = 'ready' AND p.readiness = 'fixture' AND m.readiness = 'fixture'
                 ORDER BY i.received_at, i.id LIMIT 1",
                params![tenant_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
            )
            .optional()
            .map_err(persistence)?;
        let Some((
            inbox_id,
            endpoint_id,
            binding_id,
            provider_id,
            provider_event_id,
            headers_json,
            raw_payload,
        )) = row
        else {
            tx.commit().map_err(persistence)?;
            return Ok(None);
        };
        let inbox_id = WebhookInboxId::new(inbox_id)?;
        let endpoint_id = WebhookEndpointId::new(endpoint_id)?;
        let binding_id = ProviderBindingId::new(binding_id)?;
        let provider_id = ProviderId::new(provider_id)?;
        let attempts: i64 = tx.query_row(
            "SELECT COUNT(*) FROM webhook_processing_attempts WHERE tenant_id = ?1 AND inbox_id = ?2",
            params![tenant_id, inbox_id.as_str()],
            |row| row.get(0),
        ).map_err(persistence)?;
        let attempt_number = u32::try_from(attempts)
            .map_err(persistence)?
            .checked_add(1)
            .ok_or(IntegrationError::InvalidOperationTransition)?;
        let timestamp = now();
        let changed = tx.execute(
            "UPDATE webhook_inbox SET status = 'processing', error_classification = NULL WHERE tenant_id = ?1 AND id = ?2 AND status = 'verified'",
            params![tenant_id, inbox_id.as_str()],
        ).map_err(persistence)?;
        if changed != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.execute(
            "INSERT INTO webhook_processing_attempts (id, tenant_id, inbox_id, attempt_number, state, started_at) VALUES (?1, ?2, ?3, ?4, 'processing', ?5)",
            params![uuid::Uuid::new_v4().to_string(), tenant_id, inbox_id.as_str(), attempt_number, timestamp],
        ).map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        Ok(Some(ClaimedWebhook {
            tenant_id: tenant_id.to_owned(),
            inbox_id,
            endpoint: WebhookEndpointContext {
                endpoint_id,
                tenant_id: tenant_id.to_owned(),
                binding_id,
                provider_id,
            },
            provider_event_id,
            headers_json,
            raw_payload,
            attempt_number,
        }))
    }

    pub fn complete_webhook(
        &self,
        webhook: &ClaimedWebhook,
        canonical_event_type: &str,
    ) -> Result<(), IntegrationError> {
        if canonical_event_type.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        self.finish_webhook_attempt(
            webhook,
            "processed",
            Some(canonical_event_type),
            None,
            "processed",
        )
    }

    pub fn retry_webhook(
        &self,
        webhook: &ClaimedWebhook,
        classification: &str,
    ) -> Result<(), IntegrationError> {
        self.finish_webhook_attempt(
            webhook,
            "verified",
            None,
            Some(classification),
            "retryable_failure",
        )
    }

    pub fn dead_letter_webhook(
        &self,
        webhook: &ClaimedWebhook,
        reason: &str,
    ) -> Result<(), IntegrationError> {
        if reason.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let timestamp = now();
        let changed = tx.execute("UPDATE webhook_inbox SET status = 'dead_letter', error_classification = ?1 WHERE id = ?2 AND tenant_id = ?3 AND status = 'processing'", params![reason, webhook.inbox_id.as_str(), webhook.tenant_id]).map_err(persistence)?;
        if changed != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.execute("UPDATE webhook_processing_attempts SET state = 'dead_letter', classification = ?1, completed_at = ?2 WHERE tenant_id = ?3 AND inbox_id = ?4 AND attempt_number = ?5 AND state = 'processing'", params![reason, timestamp, webhook.tenant_id, webhook.inbox_id.as_str(), webhook.attempt_number]).map_err(persistence)?;
        tx.execute("INSERT INTO webhook_dead_letters (id, tenant_id, inbox_id, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![uuid::Uuid::new_v4().to_string(), webhook.tenant_id, webhook.inbox_id.as_str(), reason, timestamp]).map_err(persistence)?;
        tx.commit().map_err(persistence)
    }

    fn finish_webhook_attempt(
        &self,
        webhook: &ClaimedWebhook,
        inbox_state: &str,
        canonical_event_type: Option<&str>,
        classification: Option<&str>,
        attempt_state: &str,
    ) -> Result<(), IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let timestamp = now();
        let changed = tx.execute(
            "UPDATE webhook_inbox SET status = ?1, canonical_event_type = COALESCE(?2, canonical_event_type), error_classification = ?3 WHERE id = ?4 AND tenant_id = ?5 AND status = 'processing'",
            params![inbox_state, canonical_event_type, classification, webhook.inbox_id.as_str(), webhook.tenant_id],
        ).map_err(persistence)?;
        if changed != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.execute(
            "UPDATE webhook_processing_attempts SET state = ?1, canonical_event_type = ?2, classification = ?3, completed_at = ?4 WHERE tenant_id = ?5 AND inbox_id = ?6 AND attempt_number = ?7 AND state = 'processing'",
            params![attempt_state, canonical_event_type, classification, timestamp, webhook.tenant_id, webhook.inbox_id.as_str(), webhook.attempt_number],
        ).map_err(persistence)?;
        tx.commit().map_err(persistence)
    }

    #[cfg(test)]
    pub fn accept_verified_webhook(
        &self,
        token: &[u8],
        provider_event_id: &str,
        headers_json: &str,
        raw_payload: &[u8],
    ) -> Result<(WebhookInboxId, bool), IntegrationError> {
        let endpoint = self.resolve_webhook_endpoint(token)?;
        self.record_verified_webhook(&endpoint, provider_event_id, headers_json, raw_payload)
    }

    #[cfg(test)]
    pub fn stored_webhook_headers(
        &self,
        inbox_id: &WebhookInboxId,
    ) -> Result<String, IntegrationError> {
        let conn = self.pool.get().map_err(persistence)?;
        conn.query_row(
            "SELECT headers_json FROM webhook_inbox WHERE id = ?1",
            [inbox_id.as_str()],
            |row| row.get(0),
        )
        .map_err(persistence)
    }

    #[cfg(test)]
    pub fn move_webhook_to_dead_letter(
        &self,
        tenant_id: &str,
        inbox_id: &WebhookInboxId,
        reason: &str,
    ) -> Result<(), IntegrationError> {
        let conn = self.pool.get().map_err(persistence)?;
        let changed = conn.execute("UPDATE webhook_inbox SET status = 'dead_letter', error_classification = ?1 WHERE id = ?2 AND tenant_id = ?3 AND status IN ('verified', 'processing')", params![reason, inbox_id.as_str(), tenant_id]).map_err(persistence)?;
        if changed != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        conn.execute("INSERT INTO webhook_dead_letters (id, tenant_id, inbox_id, reason, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![uuid::Uuid::new_v4().to_string(), tenant_id, inbox_id.as_str(), reason, now()]).map_err(persistence)?;
        Ok(())
    }

    pub fn replay_webhook(
        &self,
        tenant_id: &str,
        inbox_id: &WebhookInboxId,
        actor_ref: &str,
        reason: &str,
    ) -> Result<(), IntegrationError> {
        if actor_ref.trim().is_empty() || reason.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let changed = tx.execute("UPDATE webhook_inbox SET status = 'verified', error_classification = NULL WHERE id = ?1 AND tenant_id = ?2 AND status = 'dead_letter'", params![inbox_id.as_str(), tenant_id]).map_err(persistence)?;
        if changed != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let timestamp = now();
        tx.execute("UPDATE webhook_dead_letters SET replay_count = replay_count + 1, replayed_at = ?1 WHERE tenant_id = ?2 AND inbox_id = ?3", params![timestamp, tenant_id, inbox_id.as_str()]).map_err(persistence)?;
        tx.execute("INSERT INTO webhook_replay_audit (id, tenant_id, inbox_id, actor_ref, reason, occurred_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![uuid::Uuid::new_v4().to_string(), tenant_id, inbox_id.as_str(), actor_ref, reason, timestamp]).map_err(persistence)?;
        tx.commit().map_err(persistence)
    }

    pub fn list_webhook_dead_letters(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<WebhookDeadLetterSummary>, IntegrationError> {
        let conn = self.pool.get().map_err(persistence)?;
        let mut statement = conn
            .prepare(
                "SELECT d.inbox_id, i.endpoint_id, i.provider_event_id, d.reason,
                        d.replay_count, d.created_at, d.replayed_at
                 FROM webhook_dead_letters d
                 JOIN webhook_inbox i ON i.id = d.inbox_id AND i.tenant_id = d.tenant_id
                 WHERE d.tenant_id = ?1
                 ORDER BY d.created_at DESC, d.id DESC",
            )
            .map_err(persistence)?;
        statement
            .query_map([tenant_id], |row| {
                Ok(WebhookDeadLetterSummary {
                    inbox_id: WebhookInboxId::new(row.get::<_, String>(0)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    endpoint_id: WebhookEndpointId::new(row.get::<_, String>(1)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    provider_event_id: row.get(2)?,
                    reason: row.get(3)?,
                    replay_count: row.get(4)?,
                    created_at: row.get(5)?,
                    replayed_at: row.get(6)?,
                })
            })
            .map_err(persistence)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(persistence)
    }

    pub fn create_deposit(
        &self,
        tenant_id: &str,
        authority_kind: &str,
        authority_id: &str,
        expected: Money,
    ) -> Result<DepositId, IntegrationError> {
        if !matches!(authority_kind, "order" | "settlement")
            || authority_id.trim().is_empty()
            || expected.minor < 0
        {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let id = DepositId::new(uuid::Uuid::new_v4().to_string())?;
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let timestamp = now();
        tx.execute("INSERT INTO integration_deposits (id, tenant_id, authority_kind, authority_id, expected_amount_minor, currency, state, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'expected', ?7, ?7)", params![id.as_str(), tenant_id, authority_kind, authority_id, expected.minor, expected.currency, timestamp]).map_err(persistence)?;
        Self::append_deposit_ledger_tx(
            &tx,
            tenant_id,
            &id,
            DepositLedgerEntryKind::Expected,
            &expected,
            None,
            "deposit_created",
        )?;
        tx.commit().map_err(persistence)?;
        self.metrics.financial(FinancialEvent::DepositRecorded);
        Ok(id)
    }

    pub fn record_deposit_received(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        require_positive_minor_units(money.minor)?;
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        Self::assert_deposit_currency(&tx, tenant_id, deposit_id, &money)?;
        Self::append_deposit_ledger_tx(
            &tx,
            tenant_id,
            deposit_id,
            DepositLedgerEntryKind::Received,
            &money,
            None,
            audit_ref,
        )?;
        tx.execute(
            "UPDATE integration_deposits SET state = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4 AND state IN ('expected', 'recorded')",
            params![DepositState::Recorded.as_str(), now(), tenant_id, deposit_id.as_str()],
        )
        .map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        self.metrics
            .financial(FinancialEvent::DepositReceivedLedger);
        Ok(())
    }

    pub fn hold_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        require_positive_minor_units(money.minor)?;
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        Self::assert_deposit_currency(&tx, tenant_id, deposit_id, &money)?;
        Self::append_deposit_ledger_tx(
            &tx,
            tenant_id,
            deposit_id,
            DepositLedgerEntryKind::Held,
            &money,
            None,
            audit_ref,
        )?;
        let changed = tx.execute(
            "UPDATE integration_deposits SET state = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4 AND state IN ('recorded', 'held')",
            params![DepositState::Held.as_str(), now(), tenant_id, deposit_id.as_str()],
        )
        .map_err(persistence)?;
        if changed != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.commit().map_err(persistence)
    }

    pub fn deduct_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        require_positive_minor_units(money.minor)?;
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        Self::assert_deposit_currency(&tx, tenant_id, deposit_id, &money)?;
        if Self::available_refundable_minor(&tx, tenant_id, deposit_id)? < money.minor {
            return Err(IntegrationError::RefundAmountExceedsAvailable);
        }
        Self::append_deposit_ledger_tx(
            &tx,
            tenant_id,
            deposit_id,
            DepositLedgerEntryKind::Deducted,
            &money,
            None,
            audit_ref,
        )?;
        let changed = tx.execute(
            "UPDATE integration_deposits SET state = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4 AND state IN ('recorded', 'held', 'partially_deducted')",
            params![DepositState::PartiallyDeducted.as_str(), now(), tenant_id, deposit_id.as_str()],
        )
        .map_err(persistence)?;
        if changed != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.commit().map_err(persistence)?;
        self.metrics
            .financial(FinancialEvent::DepositDeductionAdmitted);
        Ok(())
    }

    pub fn release_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        require_positive_minor_units(money.minor)?;
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        Self::assert_deposit_currency(&tx, tenant_id, deposit_id, &money)?;
        Self::append_deposit_ledger_tx(
            &tx,
            tenant_id,
            deposit_id,
            DepositLedgerEntryKind::Released,
            &money,
            None,
            audit_ref,
        )?;
        let changed = tx.execute(
            "UPDATE integration_deposits SET state = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4 AND state IN ('recorded', 'held', 'partially_deducted')",
            params![DepositState::Released.as_str(), now(), tenant_id, deposit_id.as_str()],
        )
        .map_err(persistence)?;
        if changed != 1 {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.commit().map_err(persistence)
    }

    #[cfg(test)]
    pub fn append_deposit_ledger(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        entry_type: &str,
        money: Money,
        external_operation_id: Option<&ExternalOperationId>,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        let kind = match entry_type {
            "expected" => DepositLedgerEntryKind::Expected,
            "received" => DepositLedgerEntryKind::Received,
            "held" => DepositLedgerEntryKind::Held,
            "deducted" => DepositLedgerEntryKind::Deducted,
            "released" => DepositLedgerEntryKind::Released,
            "refund_completed" => DepositLedgerEntryKind::RefundCompleted,
            "manual_adjustment" => DepositLedgerEntryKind::ManualAdjustment,
            _ => return Err(IntegrationError::InvalidOperationTransition),
        };
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        Self::assert_deposit_currency(&tx, tenant_id, deposit_id, &money)?;
        Self::append_deposit_ledger_tx(
            &tx,
            tenant_id,
            deposit_id,
            kind,
            &money,
            external_operation_id,
            audit_ref,
        )?;
        tx.commit().map_err(persistence)
    }

    pub fn request_refund(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        idempotency_key: &str,
        reason: &str,
    ) -> Result<RefundId, IntegrationError> {
        if idempotency_key.trim().is_empty() || reason.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        require_positive_minor_units(money.minor)?;
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        Self::assert_deposit_currency(&tx, tenant_id, deposit_id, &money)?;
        let existing: Option<(String, i64, String, String)> = tx.query_row("SELECT id, amount_minor, currency, reason FROM refund_intents WHERE tenant_id = ?1 AND deposit_id = ?2 AND idempotency_key = ?3", params![tenant_id, deposit_id.as_str(), idempotency_key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).optional().map_err(persistence)?;
        if let Some((existing, amount_minor, currency, existing_reason)) = existing {
            if amount_minor != money.minor
                || currency != money.currency
                || existing_reason != reason
            {
                return Err(IntegrationError::InvalidOperationTransition);
            }
            tx.commit().map_err(persistence)?;
            return RefundId::new(existing);
        }
        let reserved: i64 = tx.query_row(
            "SELECT COALESCE(SUM(amount_minor), 0) FROM refund_intents WHERE tenant_id = ?1 AND deposit_id = ?2 AND state IN ('requested', 'approved', 'dispatching', 'unknown_outcome')",
            params![tenant_id, deposit_id.as_str()],
            |row| row.get(0),
        ).map_err(persistence)?;
        let available = Self::available_refundable_minor(&tx, tenant_id, deposit_id)?;
        if money.minor > available.saturating_sub(reserved) {
            return Err(IntegrationError::RefundAmountExceedsAvailable);
        }
        let id = RefundId::new(uuid::Uuid::new_v4().to_string())?;
        let timestamp = now();
        tx.execute("INSERT INTO refund_intents (id, tenant_id, deposit_id, amount_minor, currency, idempotency_key, state, reason, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'requested', ?7, ?8, ?8)", params![id.as_str(), tenant_id, deposit_id.as_str(), money.minor, money.currency, idempotency_key, reason, timestamp]).map_err(persistence)?;
        tx.commit().map_err(persistence)?;
        self.metrics.financial(FinancialEvent::RefundIntentAdmitted);
        Ok(id)
    }

    pub fn link_refund_operation(
        &self,
        tenant_id: &str,
        refund_id: &RefundId,
        operation_id: &ExternalOperationId,
    ) -> Result<(), IntegrationError> {
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let exists: Option<String> = tx
            .query_row(
                "SELECT id FROM external_operations WHERE tenant_id = ?1 AND id = ?2 AND operation_type = 'refund' AND state IN ('ready', 'retryable_failure', 'dispatching', 'unknown_outcome', 'reconciling')",
                params![tenant_id, operation_id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(persistence)?;
        if exists.is_none() {
            return Err(IntegrationError::RefundOperationUnavailable);
        }
        let changed = tx.execute(
            "UPDATE refund_intents SET state = ?1, external_operation_id = ?2, updated_at = ?3 WHERE tenant_id = ?4 AND id = ?5 AND state = 'requested'",
            params![RefundState::Approved.as_str(), operation_id.as_str(), now(), tenant_id, refund_id.as_str()],
        ).map_err(persistence)?;
        if changed != 1 {
            let existing: Option<(String, Option<String>)> = tx
                .query_row(
                    "SELECT state, external_operation_id FROM refund_intents WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, refund_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(persistence)?;
            if existing.as_ref().is_some_and(|(state, existing_id)| {
                state == RefundState::Approved.as_str()
                    && existing_id.as_deref() == Some(operation_id.as_str())
            }) {
                return tx.commit().map_err(persistence);
            }
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.commit().map_err(persistence)?;
        self.metrics
            .financial(FinancialEvent::RefundOperationPlanned);
        Ok(())
    }

    pub fn reconcile_refund(
        &self,
        tenant_id: &str,
        refund_id: &RefundId,
        outcome: DepositReconciliationOutcome,
        evidence_ref: &str,
        actor_ref: Option<&str>,
    ) -> Result<(), IntegrationError> {
        if evidence_ref.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let mut conn = self.pool.get().map_err(persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(persistence)?;
        let refund: (String, i64, String, Option<String>, String) = tx.query_row(
            "SELECT deposit_id, amount_minor, currency, external_operation_id, state FROM refund_intents WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, refund_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        ).map_err(persistence)?;
        let deposit_id = DepositId::new(refund.0)?;
        let amount = Money::new(refund.1, refund.2)?;
        let next_state = match outcome {
            DepositReconciliationOutcome::Pending => RefundState::Dispatching,
            DepositReconciliationOutcome::Matched => RefundState::Completed,
            DepositReconciliationOutcome::Unknown => RefundState::UnknownOutcome,
            DepositReconciliationOutcome::ManualRequired => RefundState::ManualResolutionRequired,
        };
        if matches!(
            refund.4.as_str(),
            "completed" | "failed" | "manual_resolution_required"
        ) {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        if matches!(outcome, DepositReconciliationOutcome::Matched)
            && Self::available_refundable_minor(&tx, tenant_id, &deposit_id)? < amount.minor
        {
            return Err(IntegrationError::RefundAmountExceedsAvailable);
        }
        let timestamp = now();
        tx.execute(
            "UPDATE refund_intents SET state = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4",
            params![next_state.as_str(), timestamp, tenant_id, refund_id.as_str()],
        ).map_err(persistence)?;
        tx.execute(
            "INSERT INTO integration_deposit_reconciliations (id, tenant_id, deposit_id, refund_id, external_operation_id, outcome, evidence_ref, resolved_by, created_at, resolved_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![uuid::Uuid::new_v4().to_string(), tenant_id, deposit_id.as_str(), refund_id.as_str(), refund.3, outcome.as_str(), evidence_ref, actor_ref, timestamp, if matches!(outcome, DepositReconciliationOutcome::Pending | DepositReconciliationOutcome::Unknown) { None } else { Some(timestamp.clone()) }],
        ).map_err(persistence)?;
        if matches!(outcome, DepositReconciliationOutcome::Matched) {
            Self::append_deposit_ledger_tx(
                &tx,
                tenant_id,
                &deposit_id,
                DepositLedgerEntryKind::RefundCompleted,
                &amount,
                refund
                    .3
                    .as_deref()
                    .map(ExternalOperationId::new)
                    .transpose()?
                    .as_ref(),
                "refund_reconciliation_matched",
            )?;
        }
        if matches!(outcome, DepositReconciliationOutcome::ManualRequired) {
            tx.execute(
                "UPDATE integration_deposits SET state = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4",
                params![DepositState::ManualResolutionRequired.as_str(), timestamp, tenant_id, deposit_id.as_str()],
            )
            .map_err(persistence)?;
        }
        tx.commit().map_err(persistence)?;
        self.metrics.financial(FinancialEvent::RefundReconciliation);
        Ok(())
    }

    fn assert_deposit_currency(
        tx: &rusqlite::Transaction<'_>,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: &Money,
    ) -> Result<(), IntegrationError> {
        let currency: String = tx
            .query_row(
                "SELECT currency FROM integration_deposits WHERE tenant_id = ?1 AND id = ?2",
                params![tenant_id, deposit_id.as_str()],
                |row| row.get(0),
            )
            .map_err(persistence)?;
        if currency != money.currency {
            return Err(IntegrationError::CurrencyMismatch);
        }
        Ok(())
    }

    fn append_deposit_ledger_tx(
        tx: &rusqlite::Transaction<'_>,
        tenant_id: &str,
        deposit_id: &DepositId,
        kind: DepositLedgerEntryKind,
        money: &Money,
        external_operation_id: Option<&ExternalOperationId>,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        if audit_ref.trim().is_empty() {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        tx.execute("INSERT INTO integration_deposit_ledger (id, tenant_id, deposit_id, entry_type, amount_minor, currency, external_operation_id, audit_ref, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", params![uuid::Uuid::new_v4().to_string(), tenant_id, deposit_id.as_str(), kind.as_str(), money.minor, money.currency, external_operation_id.map(ExternalOperationId::as_str), audit_ref, now()]).map_err(persistence)?;
        Ok(())
    }

    fn available_refundable_minor(
        tx: &rusqlite::Transaction<'_>,
        tenant_id: &str,
        deposit_id: &DepositId,
    ) -> Result<i64, IntegrationError> {
        tx.query_row(
            "SELECT COALESCE(SUM(CASE entry_type WHEN 'received' THEN amount_minor WHEN 'manual_adjustment' THEN amount_minor WHEN 'deducted' THEN -amount_minor WHEN 'refund_completed' THEN -amount_minor ELSE 0 END), 0) FROM integration_deposit_ledger WHERE tenant_id = ?1 AND deposit_id = ?2",
            params![tenant_id, deposit_id.as_str()],
            |row| row.get(0),
        ).map_err(persistence)
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use secrecy::Secret;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use super::IntegrationStore;
    use crate::db::migrations::run_migrations;
    use crate::integration::deposit::DepositReconciliationOutcome;
    use crate::integration::keystore::{
        InMemoryKeyStore, SecretEnvironment, SecretRegistrationScope,
    };
    use crate::integration::operation::{ExternalOperation, OperationState};
    use crate::integration::types::{
        CapabilityId, ConfigField, ConfigValueType, HttpsOrigin, IntegrationError, Money,
        NonSecretConfigValue, ProviderBinding, ProviderBindingId, ProviderHealth, ProviderId,
        ProviderInstance, ProviderInstanceId, ProviderLifecycle, ProviderManifest,
        ProviderReadiness, SecretPurpose, SecretRef, SecretRequirement, SecretValueType,
    };
    use crate::observability::RuntimeMetrics;

    fn store() -> IntegrationStore {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        run_migrations(&conn).unwrap();
        drop(conn);
        IntegrationStore::new(pool)
    }

    fn store_with_metrics() -> (IntegrationStore, Arc<RuntimeMetrics>) {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        run_migrations(&conn).unwrap();
        drop(conn);
        let metrics = Arc::new(RuntimeMetrics::default());
        let store = IntegrationStore::with_key_store_and_metrics(
            pool,
            Arc::new(InMemoryKeyStore::default()),
            metrics.clone(),
        );
        (store, metrics)
    }

    fn fixture(store: &IntegrationStore) -> (ProviderBinding, CapabilityId) {
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
                webhook_types: BTreeSet::from(["refund.updated".into()]),
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
            config_revision: "rev-1".into(),
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
            config_revision: "rev-1".into(),
            enabled: true,
        };
        store.save_binding(&binding, "actor-a").unwrap();
        (binding, capability)
    }

    fn context_for(tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("actor-a", "admin").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("revision-a").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new("request-a").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn context() -> ExecutionContext {
        context_for("tenant-a")
    }

    #[test]
    fn persisted_catalog_enforces_tenant_binding_and_no_plaintext_secret_column() {
        let store = store();
        let (_, capability) = fixture(&store);
        assert!(
            store
                .catalog()
                .unwrap()
                .resolve(&context(), &capability)
                .is_ok()
        );
        let conn = store.pool.get().unwrap();
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(provider_instances)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(columns.contains(&"secret_refs_json".to_string()));
        assert!(!columns.iter().any(|column| column.contains("secret_value")));
    }

    #[test]
    fn instance_and_binding_validation_fail_closed_before_persistence() {
        let store = store();
        let (_, capability) = fixture(&store);
        let instance = store.list_instances("tenant-a").unwrap().pop().unwrap();
        assert!(store.list_instances("tenant-b").unwrap().is_empty());

        let stale_revision = ProviderBinding {
            id: ProviderBindingId::new("binding-stale").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_instance_id: instance.id.clone(),
            capability: capability.clone(),
            config_revision: "rev-2".into(),
            enabled: true,
        };
        assert_eq!(
            store.save_binding(&stale_revision, "actor-a").unwrap_err(),
            IntegrationError::BindingRevisionStale
        );

        let cross_tenant = ProviderBinding {
            id: ProviderBindingId::new("binding-tenant-b").unwrap(),
            tenant_id: "tenant-b".into(),
            provider_instance_id: instance.id.clone(),
            capability: capability.clone(),
            config_revision: instance.config_revision.clone(),
            enabled: true,
        };
        assert_eq!(
            store.save_binding(&cross_tenant, "actor-b").unwrap_err(),
            IntegrationError::BindingUnavailable
        );

        let missing_capability = ProviderBinding {
            id: ProviderBindingId::new("binding-unknown-capability").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_instance_id: instance.id.clone(),
            capability: CapabilityId::new("fixture.undeclared").unwrap(),
            config_revision: instance.config_revision.clone(),
            enabled: true,
        };
        assert_eq!(
            store
                .save_binding(&missing_capability, "actor-a")
                .unwrap_err(),
            IntegrationError::CapabilityNotDeclared
        );

        let undeclared_config = ProviderInstance {
            id: ProviderInstanceId::new("instance-unsafe-config").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_id: instance.provider_id,
            manifest_version: instance.manifest_version,
            config_revision: "rev-unsafe".into(),
            config: BTreeMap::from([(
                "api_key".into(),
                NonSecretConfigValue::HttpsOrigin(
                    HttpsOrigin::new("https://fixture.invalid").unwrap(),
                ),
            )]),
            secret_refs: BTreeMap::new(),
            lifecycle: ProviderLifecycle::Draft,
            health: ProviderHealth::Unknown,
            readiness: ProviderReadiness::Stub,
        };
        assert!(matches!(
            store.save_instance(&undeclared_config),
            Err(IntegrationError::InvalidManifest(_))
        ));

        let unsafe_manifest = ProviderManifest {
            provider_id: ProviderId::new("fixture-unsafe").unwrap(),
            version: "1".into(),
            capabilities: BTreeSet::from([CapabilityId::new("fixture.unsafe").unwrap()]),
            config_schema: vec![ConfigField {
                name: "api_key".into(),
                required: true,
                value_type: ConfigValueType::HttpsOrigin,
            }],
            secret_schema: vec![SecretRequirement {
                name: "api_key".into(),
                required: true,
                value_type: SecretValueType::OpaqueCredential,
                purpose: SecretPurpose::ProviderAuthentication,
            }],
            api_versions: BTreeMap::new(),
            webhook_types: BTreeSet::new(),
            simulation_capabilities: BTreeSet::new(),
            readiness: ProviderReadiness::Fixture,
            compatibility: BTreeMap::new(),
        };
        assert!(matches!(
            store.save_manifest(&unsafe_manifest),
            Err(IntegrationError::InvalidManifest(_))
        ));
        let unsafe_count: i64 = store
            .pool
            .get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM provider_manifests WHERE provider_id = 'fixture-unsafe'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unsafe_count, 0);
    }

    #[test]
    fn secret_reference_registration_rejects_same_instance_id_in_other_tenant() {
        let unconfigured = store();
        let pool = unconfigured.pool.clone();
        let key_store = Arc::new(InMemoryKeyStore::default());
        let secret_ref = SecretRef::new("keystore://fixture/payment-a").unwrap();
        key_store.insert(
            secret_ref.clone(),
            SecretEnvironment::Fixture,
            BTreeSet::from([SecretRegistrationScope::new(
                "tenant-a",
                ProviderInstanceId::new("shared-instance").unwrap(),
            )
            .unwrap()]),
            Secret::new(b"fixture-only-secret".to_vec()),
        );
        let store = IntegrationStore::with_key_store(pool, key_store);
        let provider = ProviderId::new("fixture-payment-acl").unwrap();
        store
            .save_manifest(&ProviderManifest {
                provider_id: provider.clone(),
                version: "1".into(),
                capabilities: BTreeSet::from([CapabilityId::new("fixture.acl").unwrap()]),
                config_schema: vec![],
                secret_schema: vec![SecretRequirement {
                    name: "api_key".into(),
                    required: true,
                    value_type: SecretValueType::OpaqueCredential,
                    purpose: SecretPurpose::ProviderAuthentication,
                }],
                api_versions: BTreeMap::new(),
                webhook_types: BTreeSet::new(),
                simulation_capabilities: BTreeSet::new(),
                readiness: ProviderReadiness::Fixture,
                compatibility: BTreeMap::new(),
            })
            .unwrap();
        let instance = |tenant_id: &str| ProviderInstance {
            id: ProviderInstanceId::new("shared-instance").unwrap(),
            tenant_id: tenant_id.into(),
            provider_id: provider.clone(),
            manifest_version: "1".into(),
            config_revision: "revision-a".into(),
            config: BTreeMap::new(),
            secret_refs: BTreeMap::from([("api_key".into(), secret_ref.clone())]),
            lifecycle: ProviderLifecycle::Draft,
            health: ProviderHealth::Unknown,
            readiness: ProviderReadiness::Fixture,
        };
        store.save_instance(&instance("tenant-a")).unwrap();
        assert_eq!(
            store.save_instance(&instance("tenant-b")).unwrap_err(),
            IntegrationError::SecretUnavailable
        );
        assert!(store.list_instances("tenant-b").unwrap().is_empty());
    }

    #[test]
    fn catalog_keeps_duplicate_local_ids_tenant_scoped() {
        let store = store();
        let (binding_a, capability) = fixture(&store);
        let instance_a = store.list_instances("tenant-a").unwrap().pop().unwrap();
        let instance_b = ProviderInstance {
            id: instance_a.id.clone(),
            tenant_id: "tenant-b".into(),
            provider_id: instance_a.provider_id.clone(),
            manifest_version: instance_a.manifest_version.clone(),
            config_revision: instance_a.config_revision.clone(),
            config: instance_a.config.clone(),
            secret_refs: instance_a.secret_refs.clone(),
            lifecycle: ProviderLifecycle::Active,
            health: ProviderHealth::Ready,
            readiness: ProviderReadiness::Fixture,
        };
        store.save_instance(&instance_b).unwrap();
        let binding_b = ProviderBinding {
            id: binding_a.id.clone(),
            tenant_id: "tenant-b".into(),
            provider_instance_id: instance_b.id.clone(),
            capability: capability.clone(),
            config_revision: instance_b.config_revision.clone(),
            enabled: true,
        };
        store.save_binding(&binding_b, "actor-b").unwrap();

        let catalog = store.catalog().unwrap();
        assert_eq!(
            catalog
                .resolve(&context_for("tenant-a"), &capability)
                .unwrap()
                .instance
                .tenant_id,
            "tenant-a"
        );
        assert_eq!(
            catalog
                .resolve(&context_for("tenant-b"), &capability)
                .unwrap()
                .instance
                .tenant_id,
            "tenant-b"
        );
    }

    #[test]
    fn durable_operation_blocks_blind_retry_and_webhook_replay_is_idempotent() {
        let store = store();
        let (binding, capability) = fixture(&store);
        let resolved = store
            .catalog()
            .unwrap()
            .resolve(&context(), &capability)
            .unwrap();
        let mut operation = ExternalOperation::planned(
            crate::integration::types::ExternalOperationId::new("operation-a").unwrap(),
            &resolved,
            "refund",
            "idem-a",
            "request-a",
        )
        .unwrap();
        operation.ready().unwrap();
        let operation_id = store.persist_operation(&operation).unwrap();
        let attempt = store.start_attempt("tenant-a", &operation_id).unwrap();
        store
            .complete_attempt(
                "tenant-a",
                &operation_id,
                &attempt,
                OperationState::UnknownOutcome,
                Some("response_timeout"),
                None,
            )
            .unwrap();
        assert!(store.start_attempt("tenant-a", &operation_id).is_err());
        store
            .begin_reconciliation("tenant-a", &operation_id, "evidence-a")
            .unwrap();
        store
            .resolve_reconciliation("tenant-a", &operation_id, false, "operator-a")
            .unwrap();
        assert!(store.start_attempt("tenant-a", &operation_id).is_ok());

        let endpoint = store
            .register_webhook_endpoint("tenant-a", &binding.id, b"endpoint-token")
            .unwrap();
        let (first, duplicate) = store
            .accept_verified_webhook(b"endpoint-token", "event-a", "{}", b"payload")
            .unwrap();
        assert!(!duplicate);
        let (again, duplicate) = store
            .accept_verified_webhook(b"endpoint-token", "event-a", "{}", b"payload")
            .unwrap();
        assert!(duplicate);
        assert_eq!(first.as_str(), again.as_str());
        store
            .move_webhook_to_dead_letter("tenant-a", &first, "malformed")
            .unwrap();
        store
            .replay_webhook(
                "tenant-a",
                &first,
                "operator-a",
                "corrected payload mapping",
            )
            .unwrap();
        assert!(
            store
                .replay_webhook("tenant-a", &first, "operator-a", "duplicate replay")
                .is_err()
        );

        let mut non_fixture_manifest = store
            .catalog()
            .unwrap()
            .manifest(&ProviderId::new("fixture-payments").unwrap(), "1")
            .unwrap();
        non_fixture_manifest.readiness = ProviderReadiness::Sandbox;
        assert!(matches!(
            store.save_manifest(&non_fixture_manifest),
            Err(IntegrationError::InvalidManifest(_))
        ));
        assert!(store.resolve_webhook_endpoint(b"endpoint-token").is_ok());
        assert!(
            store
                .register_webhook_endpoint("tenant-a", &binding.id, b"endpoint-b")
                .is_ok()
        );
        assert!(!endpoint.as_str().is_empty());
    }

    #[test]
    fn queued_work_rechecks_manifest_fixture_readiness_before_consumption() {
        let store = store();
        let (binding, capability) = fixture(&store);
        let resolved = store
            .catalog()
            .unwrap()
            .resolve(&context(), &capability)
            .unwrap();
        let mut operation = ExternalOperation::planned(
            crate::integration::types::ExternalOperationId::new("manifest-readiness-operation")
                .unwrap(),
            &resolved,
            "refund",
            "manifest-readiness-idempotency",
            "manifest-readiness-request",
        )
        .unwrap();
        operation.ready().unwrap();
        store.persist_operation(&operation).unwrap();
        store
            .register_webhook_endpoint("tenant-a", &binding.id, b"manifest-readiness-token")
            .unwrap();
        let (inbox_id, duplicate) = store
            .accept_verified_webhook(
                b"manifest-readiness-token",
                "manifest-readiness-event",
                "{}",
                b"payload",
            )
            .unwrap();
        assert!(!duplicate);

        // This deliberately bypasses the public immutable-manifest API to
        // prove the worker-side guard also protects queued records from an
        // inconsistent database state.
        store
            .pool
            .get()
            .unwrap()
            .execute(
                "UPDATE provider_manifests SET readiness = 'sandbox' WHERE provider_id = 'fixture-payments' AND version = '1'",
                [],
            )
            .unwrap();

        assert!(
            store
                .claim_next_operation(
                    "tenant-a",
                    &crate::integration::runtime::OperationRuntimePolicy::default(),
                )
                .unwrap()
                .is_none()
        );
        let operation_state: String = store
            .pool
            .get()
            .unwrap()
            .query_row(
                "SELECT state FROM external_operations WHERE tenant_id = ?1 AND id = ?2",
                ["tenant-a", operation.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(operation_state, "manual_resolution_required");
        assert!(store.claim_next_webhook("tenant-a").unwrap().is_none());
        let status: String = store
            .pool
            .get()
            .unwrap()
            .query_row(
                "SELECT status FROM webhook_inbox WHERE id = ?1",
                [inbox_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "verified");
    }

    #[test]
    fn deposit_ledger_is_immutable_and_refund_idempotency_is_scoped() {
        let store = store();
        let deposit = store
            .create_deposit(
                "tenant-a",
                "order",
                "order-a",
                Money::new(1200, "CNY").unwrap(),
            )
            .unwrap();
        store
            .record_deposit_received(
                "tenant-a",
                &deposit,
                Money::new(1200, "CNY").unwrap(),
                "receipt-a",
            )
            .unwrap();
        let first = store
            .request_refund(
                "tenant-a",
                &deposit,
                Money::new(200, "CNY").unwrap(),
                "refund-idem-a",
                "fixture only",
            )
            .unwrap();
        let again = store
            .request_refund(
                "tenant-a",
                &deposit,
                Money::new(200, "CNY").unwrap(),
                "refund-idem-a",
                "fixture only",
            )
            .unwrap();
        assert_eq!(first.as_str(), again.as_str());
        assert_eq!(
            store
                .request_refund(
                    "tenant-a",
                    &deposit,
                    Money::new(1100, "CNY").unwrap(),
                    "refund-idem-b",
                    "exceeds available balance",
                )
                .unwrap_err(),
            crate::integration::types::IntegrationError::RefundAmountExceedsAvailable
        );
        assert_eq!(
            store
                .request_refund(
                    "tenant-a",
                    &deposit,
                    Money::new(100, "USD").unwrap(),
                    "refund-idem-c",
                    "wrong currency",
                )
                .unwrap_err(),
            crate::integration::types::IntegrationError::CurrencyMismatch
        );
        let conn = store.pool.get().unwrap();
        assert!(
            conn.execute(
                "UPDATE integration_deposit_ledger SET audit_ref = 'mutated'",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn durable_financial_admissions_emit_bounded_metrics_only_after_commit() {
        let (store, metrics) = store_with_metrics();
        let (_, capability) = fixture(&store);
        let deposit = store
            .create_deposit(
                "tenant-a",
                "order",
                "order-metrics-a",
                Money::new(1000, "CNY").unwrap(),
            )
            .unwrap();
        store
            .record_deposit_received(
                "tenant-a",
                &deposit,
                Money::new(1000, "CNY").unwrap(),
                "receipt-metrics-a",
            )
            .unwrap();
        store
            .deduct_deposit(
                "tenant-a",
                &deposit,
                Money::new(200, "CNY").unwrap(),
                "deduction-metrics-a",
            )
            .unwrap();
        let resolved = store
            .catalog()
            .unwrap()
            .resolve(&context(), &capability)
            .unwrap();
        let mut operation = ExternalOperation::planned(
            crate::integration::types::ExternalOperationId::new("refund-operation-metrics-a")
                .unwrap(),
            &resolved,
            "refund",
            "refund-effect-metrics-a",
            "refund-request-metrics-a",
        )
        .unwrap();
        operation.ready().unwrap();
        let operation_id = store.persist_operation(&operation).unwrap();
        let refund = store
            .request_refund(
                "tenant-a",
                &deposit,
                Money::new(300, "CNY").unwrap(),
                "refund-intent-metrics-a",
                "fixture-only refund",
            )
            .unwrap();
        store
            .link_refund_operation("tenant-a", &refund, &operation_id)
            .unwrap();
        store
            .reconcile_refund(
                "tenant-a",
                &refund,
                DepositReconciliationOutcome::Matched,
                "reconciliation-metrics-a",
                Some("operator-a"),
            )
            .unwrap();

        let output = metrics.render();
        for event in [
            "deposit_recorded",
            "deposit_received_ledger",
            "deposit_deduction_admitted",
            "refund_intent_admitted",
            "refund_operation_planned",
            "refund_reconciliation",
        ] {
            assert!(output.contains(&format!("event=\"{event}\"")));
        }
        assert!(output.contains("event=\"admitted\",state=\"ready\""));
        for prohibited in [
            "tenant-a",
            "order-metrics-a",
            "receipt-metrics-a",
            "refund-operation-metrics-a",
            "refund-intent-metrics-a",
        ] {
            assert!(!output.contains(prohibited));
        }
    }

    #[test]
    fn matched_refund_is_linked_to_external_operation_and_appends_immutable_ledger_evidence() {
        let store = store();
        let (_, capability) = fixture(&store);
        let deposit = store
            .create_deposit(
                "tenant-a",
                "order",
                "order-refund-a",
                Money::new(800, "CNY").unwrap(),
            )
            .unwrap();
        store
            .record_deposit_received(
                "tenant-a",
                &deposit,
                Money::new(800, "CNY").unwrap(),
                "receipt-refund-a",
            )
            .unwrap();
        let resolved = store
            .catalog()
            .unwrap()
            .resolve(&context(), &capability)
            .unwrap();
        let mut operation = ExternalOperation::planned(
            crate::integration::types::ExternalOperationId::new("refund-operation-a").unwrap(),
            &resolved,
            "refund",
            "refund-effect-idempotency-a",
            "refund-request-hash-a",
        )
        .unwrap();
        operation.ready().unwrap();
        let operation_id = store.persist_operation(&operation).unwrap();
        let refund = store
            .request_refund(
                "tenant-a",
                &deposit,
                Money::new(300, "CNY").unwrap(),
                "refund-intent-idempotency-a",
                "fixture-only refund",
            )
            .unwrap();
        store
            .link_refund_operation("tenant-a", &refund, &operation_id)
            .unwrap();
        store
            .reconcile_refund(
                "tenant-a",
                &refund,
                DepositReconciliationOutcome::Matched,
                "fixture-reconciliation-a",
                Some("operator-a"),
            )
            .unwrap();
        assert_eq!(
            store
                .request_refund(
                    "tenant-a",
                    &deposit,
                    Money::new(600, "CNY").unwrap(),
                    "refund-intent-idempotency-b",
                    "remaining balance exceeded",
                )
                .unwrap_err(),
            crate::integration::types::IntegrationError::RefundAmountExceedsAvailable
        );
    }
}

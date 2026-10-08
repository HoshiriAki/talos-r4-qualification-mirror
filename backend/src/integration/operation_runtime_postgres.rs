use std::future::Future;
use std::sync::Arc;

use sqlx::postgres::PgPool;
use sqlx::{Executor, Row};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::operation::{ExternalOperation, OperationState};
use super::operation_runtime_contract::{
    ClaimedOperation, DispatchResult, OperationRuntimeMetrics, OperationRuntimePersistence,
    OperationRuntimePolicy,
};
use super::transport::TransportFailure;
use super::types::{ExternalAttemptId, ExternalOperationId, IntegrationError, ProviderBindingId};
use crate::observability::{IntegrationEvent, MetricsSink, NoopMetrics, OperationStateClass};

#[derive(Clone)]
pub(crate) struct PostgresOperationRuntimePersistence {
    pool: PgPool,
    metrics: Arc<dyn MetricsSink>,
}

impl PostgresOperationRuntimePersistence {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self::new_with_metrics(pool, Arc::new(NoopMetrics))
    }

    pub(crate) fn new_with_metrics(pool: PgPool, metrics: Arc<dyn MetricsSink>) -> Self {
        Self { pool, metrics }
    }
}

impl OperationRuntimePersistence for PostgresOperationRuntimePersistence {
    fn persist_operation(
        &self,
        operation: &ExternalOperation,
    ) -> Result<ExternalOperationId, IntegrationError> {
        let pool = self.pool.clone();
        let state_class = operation_state_metric(&operation.state);
        let operation = operation.clone();
        let (operation_id, admitted) = run_pg_operation(async move {
            let mut transaction = pool.begin().await.map_err(persistence)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;

            let timestamp = now();
            let inserted = sqlx::query(
                "INSERT INTO external_operations
                 (id,tenant_id,provider_instance_id,binding_id,binding_revision,capability_id,
                  operation_type,idempotency_key,request_hash,state,attempt_count,created_at,updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$12)
                 ON CONFLICT (tenant_id,binding_id,idempotency_key) DO NOTHING",
            )
            .bind(operation.id.as_str())
            .bind(&operation.tenant_id)
            .bind(&operation.provider_instance_id)
            .bind(&operation.binding_id)
            .bind(&operation.binding_revision)
            .bind(&operation.capability)
            .bind(&operation.operation_type)
            .bind(&operation.idempotency_key)
            .bind(&operation.request_hash)
            .bind(operation.state.as_persisted())
            .bind(i64::from(operation.attempts))
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();

            if inserted == 1 {
                transaction.commit().await.map_err(persistence)?;
                return Ok((operation.id, true));
            }

            let existing = sqlx::query(
                "SELECT id,request_hash
                 FROM external_operations
                 WHERE tenant_id=$1 AND binding_id=$2 AND idempotency_key=$3
                 FOR UPDATE",
            )
            .bind(&operation.tenant_id)
            .bind(&operation.binding_id)
            .bind(&operation.idempotency_key)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?;

            let Some(existing) = existing else {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::Persistence);
            };
            let id: String = existing.try_get(0).map_err(persistence)?;
            let request_hash: String = existing.try_get(1).map_err(persistence)?;
            if request_hash != operation.request_hash {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }
            transaction.commit().await.map_err(persistence)?;
            Ok((ExternalOperationId::new(id)?, false))
        })?;

        if admitted {
            self.metrics
                .integration_operation(IntegrationEvent::Admitted, state_class);
        }
        Ok(operation_id)
    }

    fn claim_next_operation(
        &self,
        tenant_id: &str,
        policy: &OperationRuntimePolicy,
    ) -> Result<Option<ClaimedOperation>, IntegrationError> {
        policy.validate()?;
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let policy = policy.clone();

        run_pg_operation(async move {
            let mut transaction = pool.begin().await.map_err(persistence)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;

            let timestamp = now();
            let row = sqlx::query(
                "SELECT o.id,o.binding_id,o.binding_revision,o.capability_id,o.operation_type,
                        o.request_hash,o.attempt_count,b.config_revision,b.enabled,
                        i.config_revision,i.lifecycle,i.health,i.readiness,m.readiness
                 FROM external_operations o
                 JOIN provider_bindings b
                   ON b.tenant_id=o.tenant_id AND b.id=o.binding_id
                 JOIN provider_instances i
                   ON i.tenant_id=b.tenant_id AND i.id=b.provider_instance_id
                 JOIN provider_manifests m
                   ON m.provider_id=i.provider_id AND m.version=i.manifest_version
                 WHERE o.tenant_id=$1
                   AND o.state IN ('ready','retryable_failure')
                   AND (o.next_retry_at IS NULL OR o.next_retry_at <= $2)
                 ORDER BY o.created_at,o.id
                 LIMIT 1
                 FOR UPDATE OF o,b,i SKIP LOCKED",
            )
            .bind(&tenant_id)
            .bind(&timestamp)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?;

            let Some(row) = row else {
                transaction.commit().await.map_err(persistence)?;
                return Ok(None);
            };

            let operation_id =
                ExternalOperationId::new(row.try_get::<String, _>(0).map_err(persistence)?)?;
            let binding_id =
                ProviderBindingId::new(row.try_get::<String, _>(1).map_err(persistence)?)?;
            let binding_revision: String = row.try_get(2).map_err(persistence)?;
            let capability: String = row.try_get(3).map_err(persistence)?;
            let operation_type: String = row.try_get(4).map_err(persistence)?;
            let request_hash: String = row.try_get(5).map_err(persistence)?;
            let attempts: i64 = row.try_get(6).map_err(persistence)?;
            let current_binding_revision: String = row.try_get(7).map_err(persistence)?;
            let enabled: bool = row.try_get(8).map_err(persistence)?;
            let current_instance_revision: String = row.try_get(9).map_err(persistence)?;
            let lifecycle: String = row.try_get(10).map_err(persistence)?;
            let health: String = row.try_get(11).map_err(persistence)?;
            let readiness: String = row.try_get(12).map_err(persistence)?;
            let manifest_readiness: String = row.try_get(13).map_err(persistence)?;

            if binding_revision != current_binding_revision
                || binding_revision != current_instance_revision
            {
                sqlx::query(
                    "UPDATE external_operations
                     SET state='manual_resolution_required',
                         classification='binding_revision_stale',
                         updated_at=$1
                     WHERE tenant_id=$2 AND id=$3",
                )
                .bind(&timestamp)
                .bind(&tenant_id)
                .bind(operation_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;
                insert_runtime_event(
                    &mut transaction,
                    &tenant_id,
                    &operation_id,
                    None,
                    "binding_revision_mismatch",
                    Some("binding_revision_stale"),
                )
                .await?;
                transaction.commit().await.map_err(persistence)?;
                return Err(IntegrationError::BindingRevisionStale);
            }

            if !enabled || lifecycle != "active" || health != "ready" {
                transaction.commit().await.map_err(persistence)?;
                return Err(IntegrationError::InstanceNotReady);
            }
            if readiness != "fixture" || manifest_readiness != "fixture" {
                transaction.commit().await.map_err(persistence)?;
                return Err(IntegrationError::ModeBlocked);
            }

            let circuit = sqlx::query(
                "SELECT state,opened_until
                 FROM integration_circuit_state
                 WHERE tenant_id=$1 AND binding_id=$2
                 FOR UPDATE",
            )
            .bind(&tenant_id)
            .bind(binding_id.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence)?;

            if let Some(circuit) = circuit {
                let state: String = circuit.try_get(0).map_err(persistence)?;
                let opened_until: Option<String> = circuit.try_get(1).map_err(persistence)?;
                if let Some(opened_until) = opened_until {
                    if state == "open" && opened_until > timestamp {
                        transaction.commit().await.map_err(persistence)?;
                        return Err(IntegrationError::CircuitOpen);
                    }
                    if state == "open" {
                        sqlx::query(
                            "UPDATE integration_circuit_state
                             SET state='half_open',opened_until=NULL,updated_at=$1
                             WHERE tenant_id=$2 AND binding_id=$3",
                        )
                        .bind(&timestamp)
                        .bind(&tenant_id)
                        .bind(binding_id.as_str())
                        .execute(&mut *transaction)
                        .await
                        .map_err(persistence)?;
                    }
                }
            }

            let in_flight: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)
                 FROM external_operations
                 WHERE tenant_id=$1 AND binding_id=$2 AND state='dispatching'",
            )
            .bind(&tenant_id)
            .bind(binding_id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence)?;
            if in_flight >= i64::from(policy.max_in_flight_per_binding) {
                insert_runtime_event(
                    &mut transaction,
                    &tenant_id,
                    &operation_id,
                    None,
                    "concurrency_limited",
                    None,
                )
                .await?;
                transaction.commit().await.map_err(persistence)?;
                return Err(IntegrationError::ConcurrencyLimited);
            }

            let window_start = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
            let recent_attempts: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)
                 FROM external_operation_attempts a
                 JOIN external_operations o
                   ON o.tenant_id=a.tenant_id AND o.id=a.operation_id
                 WHERE a.tenant_id=$1 AND o.binding_id=$2 AND a.started_at >= $3",
            )
            .bind(&tenant_id)
            .bind(binding_id.as_str())
            .bind(&window_start)
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence)?;
            if recent_attempts >= i64::from(policy.max_attempts_per_minute) {
                insert_runtime_event(
                    &mut transaction,
                    &tenant_id,
                    &operation_id,
                    None,
                    "rate_limited",
                    None,
                )
                .await?;
                transaction.commit().await.map_err(persistence)?;
                return Err(IntegrationError::RateLimited);
            }

            let attempt_number = u32::try_from(attempts)
                .map_err(persistence)?
                .checked_add(1)
                .ok_or(IntegrationError::InvalidOperationTransition)?;
            let attempt_id = ExternalAttemptId::new(uuid::Uuid::new_v4().to_string())?;

            sqlx::query(
                "UPDATE external_operations
                 SET state='dispatching',attempt_count=$1,next_retry_at=NULL,updated_at=$2
                 WHERE tenant_id=$3 AND id=$4",
            )
            .bind(i64::from(attempt_number))
            .bind(&timestamp)
            .bind(&tenant_id)
            .bind(operation_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;
            sqlx::query(
                "INSERT INTO external_operation_attempts
                 (id,tenant_id,operation_id,attempt_number,state,request_hash,started_at)
                 VALUES ($1,$2,$3,$4,'dispatching',$5,$6)",
            )
            .bind(attempt_id.as_str())
            .bind(&tenant_id)
            .bind(operation_id.as_str())
            .bind(i64::from(attempt_number))
            .bind(&request_hash)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;
            insert_runtime_event(
                &mut transaction,
                &tenant_id,
                &operation_id,
                Some(&attempt_id),
                "claimed",
                None,
            )
            .await?;
            transaction.commit().await.map_err(persistence)?;

            Ok(Some(ClaimedOperation {
                tenant_id,
                operation_id,
                attempt_id,
                binding_id,
                binding_revision,
                capability,
                operation_type,
                request_hash,
                attempt_number,
            }))
        })
    }

    fn record_runtime_outcome(
        &self,
        claimed: &ClaimedOperation,
        outcome: Result<DispatchResult, TransportFailure>,
        policy: &OperationRuntimePolicy,
    ) -> Result<(), IntegrationError> {
        policy.validate()?;
        let pool = self.pool.clone();
        let claimed = claimed.clone();
        let policy = policy.clone();

        run_pg_operation(async move {
            let mut transaction = pool.begin().await.map_err(persistence)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;

            let current: String = sqlx::query_scalar(
                "SELECT state
                 FROM external_operations
                 WHERE tenant_id=$1 AND id=$2
                 FOR UPDATE",
            )
            .bind(&claimed.tenant_id)
            .bind(claimed.operation_id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence)?;
            if OperationState::from_persisted(&current) != Some(OperationState::Dispatching) {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            let (state, classification, result_ref, next_retry_at, failed, event_type) =
                match outcome {
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
                        Some(failure.class.as_persisted().to_owned()),
                        None,
                        None,
                        true,
                        Some("unknown_outcome"),
                    ),
                    Err(failure)
                        if failure.is_retryable()
                            && claimed.attempt_number < policy.retry.max_attempts =>
                    {
                        (
                            OperationState::RetryableFailure,
                            Some(failure.class.as_persisted().to_owned()),
                            None,
                            Some(retry_timestamp(
                                policy.retry.delay_for_attempt(claimed.attempt_number),
                            )?),
                            true,
                            Some("retry_scheduled"),
                        )
                    }
                    Err(failure) => (
                        OperationState::NonRetryableFailure,
                        Some(failure.class.as_persisted().to_owned()),
                        None,
                        None,
                        true,
                        None,
                    ),
                };

            let timestamp = now();
            sqlx::query(
                "UPDATE external_operation_attempts
                 SET state=$1,classification=$2,provider_result_ref=$3,completed_at=$4
                 WHERE id=$5 AND tenant_id=$6 AND operation_id=$7 AND state='dispatching'",
            )
            .bind(state.as_persisted())
            .bind(classification.as_deref())
            .bind(result_ref.as_deref())
            .bind(&timestamp)
            .bind(claimed.attempt_id.as_str())
            .bind(&claimed.tenant_id)
            .bind(claimed.operation_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;
            sqlx::query(
                "UPDATE external_operations
                 SET state=$1,classification=$2,result_ref=$3,next_retry_at=$4,updated_at=$5
                 WHERE tenant_id=$6 AND id=$7",
            )
            .bind(state.as_persisted())
            .bind(classification.as_deref())
            .bind(result_ref.as_deref())
            .bind(next_retry_at.as_deref())
            .bind(&timestamp)
            .bind(&claimed.tenant_id)
            .bind(claimed.operation_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            if let Some(event_type) = event_type {
                insert_runtime_event(
                    &mut transaction,
                    &claimed.tenant_id,
                    &claimed.operation_id,
                    Some(&claimed.attempt_id),
                    event_type,
                    classification.as_deref(),
                )
                .await?;
            }

            if failed {
                let existing: Option<i64> = sqlx::query_scalar(
                    "SELECT failure_count
                     FROM integration_circuit_state
                     WHERE tenant_id=$1 AND binding_id=$2
                     FOR UPDATE",
                )
                .bind(&claimed.tenant_id)
                .bind(claimed.binding_id.as_str())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(persistence)?;
                let failures = existing.unwrap_or(0) + 1;

                if failures >= i64::from(policy.circuit_failure_threshold) {
                    let open_until = retry_timestamp(policy.circuit_open_for)?;
                    sqlx::query(
                        "INSERT INTO integration_circuit_state
                         (tenant_id,binding_id,state,failure_count,opened_until,updated_at)
                         VALUES ($1,$2,'open',$3,$4,$5)
                         ON CONFLICT (tenant_id,binding_id) DO UPDATE SET
                           state='open',
                           failure_count=EXCLUDED.failure_count,
                           opened_until=EXCLUDED.opened_until,
                           updated_at=EXCLUDED.updated_at",
                    )
                    .bind(&claimed.tenant_id)
                    .bind(claimed.binding_id.as_str())
                    .bind(failures)
                    .bind(&open_until)
                    .bind(&timestamp)
                    .execute(&mut *transaction)
                    .await
                    .map_err(persistence)?;
                    insert_runtime_event(
                        &mut transaction,
                        &claimed.tenant_id,
                        &claimed.operation_id,
                        Some(&claimed.attempt_id),
                        "circuit_opened",
                        classification.as_deref(),
                    )
                    .await?;
                } else {
                    sqlx::query(
                        "INSERT INTO integration_circuit_state
                         (tenant_id,binding_id,state,failure_count,opened_until,updated_at)
                         VALUES ($1,$2,'closed',$3,NULL,$4)
                         ON CONFLICT (tenant_id,binding_id) DO UPDATE SET
                           state='closed',
                           failure_count=EXCLUDED.failure_count,
                           opened_until=NULL,
                           updated_at=EXCLUDED.updated_at",
                    )
                    .bind(&claimed.tenant_id)
                    .bind(claimed.binding_id.as_str())
                    .bind(failures)
                    .bind(&timestamp)
                    .execute(&mut *transaction)
                    .await
                    .map_err(persistence)?;
                }
            } else {
                sqlx::query(
                    "INSERT INTO integration_circuit_state
                     (tenant_id,binding_id,state,failure_count,opened_until,updated_at)
                     VALUES ($1,$2,'closed',0,NULL,$3)
                     ON CONFLICT (tenant_id,binding_id) DO UPDATE SET
                       state='closed',
                       failure_count=0,
                       opened_until=NULL,
                       updated_at=EXCLUDED.updated_at",
                )
                .bind(&claimed.tenant_id)
                .bind(claimed.binding_id.as_str())
                .bind(&timestamp)
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;
                insert_runtime_event(
                    &mut transaction,
                    &claimed.tenant_id,
                    &claimed.operation_id,
                    Some(&claimed.attempt_id),
                    "circuit_closed",
                    None,
                )
                .await?;
            }

            transaction.commit().await.map_err(persistence)
        })
    }

    fn recover_inflight_operations(&self, tenant_id: &str) -> Result<u64, IntegrationError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        run_pg_operation(async move {
            let mut transaction = pool.begin().await.map_err(persistence)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;

            let rows = sqlx::query_scalar::<_, String>(
                "SELECT id
                 FROM external_operations
                 WHERE tenant_id=$1 AND state='dispatching'
                 FOR UPDATE",
            )
            .bind(&tenant_id)
            .fetch_all(&mut *transaction)
            .await
            .map_err(persistence)?;
            let timestamp = now();

            for id in &rows {
                let operation_id = ExternalOperationId::new(id.clone())?;
                sqlx::query(
                    "UPDATE external_operations
                     SET state='unknown_outcome',
                         classification='worker_restarted_after_dispatch',
                         next_retry_at=NULL,
                         updated_at=$1
                     WHERE tenant_id=$2 AND id=$3 AND state='dispatching'",
                )
                .bind(&timestamp)
                .bind(&tenant_id)
                .bind(operation_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;
                sqlx::query(
                    "UPDATE external_operation_attempts
                     SET state='unknown_outcome',
                         classification='worker_restarted_after_dispatch',
                         completed_at=$1
                     WHERE tenant_id=$2 AND operation_id=$3 AND state='dispatching'",
                )
                .bind(&timestamp)
                .bind(&tenant_id)
                .bind(operation_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;
                insert_runtime_event(
                    &mut transaction,
                    &tenant_id,
                    &operation_id,
                    None,
                    "recovered_after_restart",
                    Some("worker_restarted_after_dispatch"),
                )
                .await?;
            }

            transaction.commit().await.map_err(persistence)?;
            u64::try_from(rows.len()).map_err(persistence)
        })
    }

    fn operation_runtime_metrics(
        &self,
        tenant_id: &str,
    ) -> Result<OperationRuntimeMetrics, IntegrationError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        run_pg_operation(async move {
            let mut transaction = pool.begin().await.map_err(persistence)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE READ ONLY")
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;

            async fn count_state(
                transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
                tenant_id: &str,
                state: &str,
            ) -> Result<u64, IntegrationError> {
                let count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM external_operations WHERE tenant_id=$1 AND state=$2",
                )
                .bind(tenant_id)
                .bind(state)
                .fetch_one(&mut **transaction)
                .await
                .map_err(persistence)?;
                u64::try_from(count).map_err(persistence)
            }

            let ready = count_state(&mut transaction, &tenant_id, "ready").await?;
            let dispatching = count_state(&mut transaction, &tenant_id, "dispatching").await?;
            let retryable_failure =
                count_state(&mut transaction, &tenant_id, "retryable_failure").await?;
            let unknown_outcome =
                count_state(&mut transaction, &tenant_id, "unknown_outcome").await?;
            let circuit_open: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)
                 FROM integration_circuit_state
                 WHERE tenant_id=$1 AND state='open'",
            )
            .bind(&tenant_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence)?;
            transaction.commit().await.map_err(persistence)?;

            Ok(OperationRuntimeMetrics {
                ready,
                dispatching,
                retryable_failure,
                unknown_outcome,
                circuit_open: u64::try_from(circuit_open).map_err(persistence)?,
            })
        })
    }

    fn begin_reconciliation(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
        evidence_ref: &str,
    ) -> Result<(), IntegrationError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let operation_id = operation_id.clone();
        let evidence_ref = evidence_ref.to_owned();

        run_pg_operation(async move {
            let mut transaction = pool.begin().await.map_err(persistence)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;

            let timestamp = now();
            let updated = sqlx::query(
                "UPDATE external_operations
                 SET state='reconciling',updated_at=$1
                 WHERE tenant_id=$2 AND id=$3 AND state='unknown_outcome'",
            )
            .bind(&timestamp)
            .bind(&tenant_id)
            .bind(operation_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if updated != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            sqlx::query(
                "INSERT INTO external_operation_reconciliations
                 (id,tenant_id,operation_id,outcome,evidence_ref,created_at)
                 VALUES ($1,$2,$3,'pending',$4,$5)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&tenant_id)
            .bind(operation_id.as_str())
            .bind(&evidence_ref)
            .bind(&timestamp)
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)
        })
    }

    fn resolve_reconciliation(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
        effect_confirmed: bool,
        actor_ref: &str,
    ) -> Result<(), IntegrationError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let operation_id = operation_id.clone();
        let actor_ref = actor_ref.to_owned();

        run_pg_operation(async move {
            let mut transaction = pool.begin().await.map_err(persistence)?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                .execute(&mut *transaction)
                .await
                .map_err(persistence)?;

            let next = if effect_confirmed {
                "resolved"
            } else {
                "ready"
            };
            let timestamp = now();
            let updated = sqlx::query(
                "UPDATE external_operations
                 SET state=$1,updated_at=$2
                 WHERE tenant_id=$3 AND id=$4 AND state='reconciling'",
            )
            .bind(next)
            .bind(&timestamp)
            .bind(&tenant_id)
            .bind(operation_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?
            .rows_affected();
            if updated != 1 {
                let _ = transaction.rollback().await;
                return Err(IntegrationError::InvalidOperationTransition);
            }

            sqlx::query(
                "UPDATE external_operation_reconciliations
                 SET outcome=$1,resolved_by=$2,resolved_at=$3
                 WHERE tenant_id=$4 AND operation_id=$5 AND outcome='pending'",
            )
            .bind(if effect_confirmed {
                "effect_confirmed"
            } else {
                "effect_absent"
            })
            .bind(&actor_ref)
            .bind(&timestamp)
            .bind(&tenant_id)
            .bind(operation_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence)?;

            transaction.commit().await.map_err(persistence)
        })
    }
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

async fn insert_runtime_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &str,
    operation_id: &ExternalOperationId,
    attempt_id: Option<&ExternalAttemptId>,
    event_type: &str,
    classification: Option<&str>,
) -> Result<(), IntegrationError> {
    sqlx::query(
        "INSERT INTO external_operation_runtime_events
         (id,tenant_id,operation_id,attempt_id,event_type,classification,occurred_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(tenant_id)
    .bind(operation_id.as_str())
    .bind(attempt_id.map(ExternalAttemptId::as_str))
    .bind(event_type)
    .bind(classification)
    .bind(now())
    .execute(&mut **transaction)
    .await
    .map_err(persistence)?;
    Ok(())
}

fn retry_timestamp(delay: std::time::Duration) -> Result<String, IntegrationError> {
    let delay = chrono::Duration::from_std(delay).map_err(persistence)?;
    Ok((chrono::Utc::now() + delay).to_rfc3339())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn persistence<T>(_error: T) -> IntegrationError {
    IntegrationError::Persistence
}

fn run_pg_operation<T, F>(future: F) -> Result<T, IntegrationError>
where
    T: Send,
    F: Future<Output = Result<T, IntegrationError>> + Send,
{
    let handle = Handle::try_current().map_err(persistence)?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(IntegrationError::Persistence);
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

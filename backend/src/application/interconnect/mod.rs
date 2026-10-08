//! R4 TALOS Interconnect Fabric runtime semantics.
//!
//! Registry remains the sole business Command authority. This layer supplies
//! transport-neutral request admission plus an in-process reference driver for
//! Event / Work / State semantics. PostgreSQL durable semantics live in the
//! feature-gated `postgres` driver and share the same envelope contract.

#[cfg(feature = "postgres")]
pub mod postgres;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use system_core::transport::interconnect::{
    CallRequirements, ConsumerId, DriverDescriptor, EventCursor, InterconnectError,
    InterconnectErrorCode, LeaseOwner, MessageEnvelope, MessageId, MessageKind, PayloadRef,
    RequestMessage, RequestResult, ResumeDirective, RetentionMetadata, StateRevision, Subject,
    TransportCapability, WatchCursor, WorkClaim,
};
use system_core::{ErrorPayload, ExecutionContext};

use crate::registry::ModuleRegistry;

const IN_PROCESS_EVENT_LIMIT: usize = 4096;
const IN_PROCESS_WORK_LIMIT: usize = 16_384;
const IN_PROCESS_STATE_DELTA_LIMIT: usize = 1024;

#[derive(Clone, Default)]
pub struct CancellationSignal(Arc<AtomicBool>);

impl CancellationSignal {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone)]
pub struct InterconnectFabric {
    registry: Arc<ModuleRegistry>,
    in_process: InProcessDriver,
}

impl InterconnectFabric {
    pub fn new(registry: Arc<ModuleRegistry>) -> Self {
        Self {
            registry,
            in_process: InProcessDriver::default(),
        }
    }

    pub fn in_process(&self) -> &InProcessDriver {
        &self.in_process
    }

    /// Query/Command request-reply lane. Cancellation is admission-only: once
    /// Registry dispatch begins, caller cancellation cannot rewrite a real
    /// command outcome into `cancelled`.
    pub fn dispatch_request(
        &self,
        request: &RequestMessage,
        ctx: &ExecutionContext,
        now_ms: u64,
        cancellation: &CancellationSignal,
    ) -> RequestResult {
        if !matches!(
            request.envelope.kind,
            MessageKind::Query | MessageKind::Command
        ) {
            return RequestResult::Err(InterconnectError::new(
                InterconnectErrorCode::InvalidPayload,
                "request lane accepts only query or command messages",
            ));
        }
        self.dispatch_registry(request, ctx, now_ms, cancellation)
    }

    /// Effect Lane admission is deliberately narrow. It routes effect planning
    /// through the existing Integration module's `plan_operation` authority,
    /// preserving EffectIntent -> ExternalOperation semantics from R2/R3.
    pub fn dispatch_effect(
        &self,
        request: &RequestMessage,
        ctx: &ExecutionContext,
        now_ms: u64,
        cancellation: &CancellationSignal,
    ) -> RequestResult {
        if request.envelope.kind != MessageKind::Effect
            || request.target.module != "integration"
            || request.target.command != "plan_operation"
        {
            return RequestResult::Err(InterconnectError::new(
                InterconnectErrorCode::PolicyDenied,
                "effect lane may only admit integration.plan_operation",
            ));
        }
        self.dispatch_registry(request, ctx, now_ms, cancellation)
    }

    fn dispatch_registry(
        &self,
        request: &RequestMessage,
        ctx: &ExecutionContext,
        now_ms: u64,
        cancellation: &CancellationSignal,
    ) -> RequestResult {
        if let Err(error) = request.envelope.validate() {
            return RequestResult::Err(error);
        }
        if request.envelope.correlation_id.as_str() != ctx.correlation_id().as_str() {
            return RequestResult::Err(InterconnectError::new(
                InterconnectErrorCode::PolicyDenied,
                "message correlation must be bound to the trusted ExecutionContext request id",
            ));
        }
        if request
            .envelope
            .deadline_ms
            .is_some_and(|deadline| now_ms > deadline)
        {
            return RequestResult::Err(InterconnectError::new(
                InterconnectErrorCode::DeadlineExceeded,
                "request deadline elapsed before Registry admission",
            ));
        }
        if cancellation.is_cancelled() {
            return RequestResult::Err(InterconnectError::new(
                InterconnectErrorCode::Cancelled,
                "request cancelled before Registry admission",
            ));
        }

        let expected_subject = match Subject::new(format!(
            "{}.{}",
            request.target.module, request.target.command
        )) {
            Ok(subject) => subject,
            Err(error) => return RequestResult::Err(error),
        };
        if request.envelope.subject != expected_subject {
            return RequestResult::Err(InterconnectError::new(
                InterconnectErrorCode::ContractIncompatible,
                "request subject and Registry target disagree",
            ));
        }

        let payload = match &request.envelope.payload {
            PayloadRef::Inline(value) => value.clone(),
            PayloadRef::Content(_) => {
                return RequestResult::Err(InterconnectError::new(
                    InterconnectErrorCode::CapabilityUnavailable,
                    "request lane ContentRef resolution requires the governed content gateway",
                ));
            }
        };

        match self.registry.execute(
            &request.target.module,
            &request.target.command,
            payload,
            ctx,
        ) {
            Ok(value) => RequestResult::Ok(value),
            Err(error) => RequestResult::Err(map_registry_error(&error)),
        }
    }
}

fn map_registry_error(error: &str) -> InterconnectError {
    if let Ok(payload) = serde_json::from_str::<ErrorPayload>(error) {
        let code = match payload.category.as_str() {
            "auth" => InterconnectErrorCode::PolicyDenied,
            "val" => InterconnectErrorCode::InvalidPayload,
            _ => InterconnectErrorCode::DriverFailure,
        };
        return InterconnectError::new(code, format!("{}: {}", payload.code, payload.message));
    }
    InterconnectError::new(
        InterconnectErrorCode::DriverFailure,
        "Registry returned an unstructured failure",
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRecord {
    pub sequence: u64,
    pub tenant_id: String,
    pub envelope: MessageEnvelope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Ready,
    Leased,
    Succeeded,
    DeadLetter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRecord {
    pub tenant_id: String,
    pub envelope: MessageEnvelope,
    pub status: WorkStatus,
    pub claim_generation: u64,
    pub lease_owner: Option<LeaseOwner>,
    pub lease_deadline_ms: Option<u64>,
    pub attempt: u32,
    pub retry_budget: u32,
    pub created_at_ms: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkMetrics {
    pub ready: usize,
    pub leased: usize,
    pub dead_letter: usize,
    pub oldest_ready_age_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDeltaRecord {
    pub cursor: WatchCursor,
    pub tenant_id: String,
    pub subject: Subject,
    pub revision: StateRevision,
    pub envelope: MessageEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateSnapshotRecord {
    pub tenant_id: String,
    pub subject: Subject,
    pub revision: StateRevision,
    pub envelope: MessageEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateWatchBatch {
    pub directive: ResumeDirective,
    pub snapshot: Option<StateSnapshotRecord>,
    pub deltas: Vec<StateDeltaRecord>,
}

#[derive(Default)]
struct InProcessState {
    next_event_sequence: u64,
    events: Vec<EventRecord>,
    event_ids: HashMap<String, (String, u64, MessageEnvelope)>,
    event_dedup: HashMap<(String, Subject, String), (u64, MessageEnvelope)>,
    checkpoints: HashMap<(String, ConsumerId, Subject), u64>,
    work: BTreeMap<String, WorkRecord>,
    work_dedup: HashMap<(String, Subject, String), String>,
    snapshots: HashMap<(String, Subject), StateSnapshotRecord>,
    state_heads: HashMap<(String, Subject), StateRevision>,
    next_state_cursor: u64,
    deltas: Vec<StateDeltaRecord>,
}

#[derive(Clone, Default)]
pub struct InProcessDriver {
    state: Arc<Mutex<InProcessState>>,
}

impl InProcessDriver {
    pub fn descriptor(&self) -> DriverDescriptor {
        DriverDescriptor {
            name: "in_process".into(),
            version: system_core::transport::interconnect::ContractVersion::new("1.0.0")
                .expect("static driver version"),
            capabilities: [
                TransportCapability::Replayable,
                TransportCapability::RequestReply,
                TransportCapability::Fanout,
                TransportCapability::CompetingConsumers,
                TransportCapability::OrderedByKey,
                TransportCapability::ConsumerCheckpoint,
                TransportCapability::DeadLetter,
                TransportCapability::FlowControl,
                TransportCapability::ContentReference,
            ]
            .into_iter()
            .collect(),
        }
    }

    pub fn require(&self, requirements: &CallRequirements) -> Result<(), InterconnectError> {
        self.descriptor().satisfies(requirements)
    }

    pub fn append_event(
        &self,
        ctx: &ExecutionContext,
        envelope: MessageEnvelope,
    ) -> Result<u64, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        validate_lane(&envelope, MessageKind::Event, ctx)?;
        let mut state = self.lock()?;

        if let Some((existing_tenant, sequence, existing)) =
            state.event_ids.get(envelope.id.as_str())
        {
            if existing_tenant == &tenant && existing == &envelope {
                return Ok(*sequence);
            }
            return Err(identity_conflict("event message id"));
        }
        if let Some(key) = envelope.idempotency_key.as_ref() {
            let dedup = (
                tenant.clone(),
                envelope.subject.clone(),
                key.as_str().to_owned(),
            );
            if let Some((sequence, existing)) = state.event_dedup.get(&dedup) {
                if existing == &envelope {
                    return Ok(*sequence);
                }
                return Err(identity_conflict("event idempotency key"));
            }
        }

        state.next_event_sequence = state.next_event_sequence.saturating_add(1);
        let sequence = state.next_event_sequence;
        let record = EventRecord {
            sequence,
            tenant_id: tenant.clone(),
            envelope: envelope.clone(),
        };
        state.events.push(record);
        state.event_ids.insert(
            envelope.id.as_str().to_owned(),
            (tenant.clone(), sequence, envelope.clone()),
        );
        if let Some(key) = envelope.idempotency_key.as_ref() {
            state.event_dedup.insert(
                (tenant, envelope.subject.clone(), key.as_str().to_owned()),
                (sequence, envelope),
            );
        }
        while state.events.len() > IN_PROCESS_EVENT_LIMIT {
            let removed = state.events.remove(0);
            state.event_ids.remove(removed.envelope.id.as_str());
            if let Some(key) = removed.envelope.idempotency_key.as_ref() {
                state.event_dedup.remove(&(
                    removed.tenant_id,
                    removed.envelope.subject,
                    key.as_str().to_owned(),
                ));
            }
        }
        Ok(sequence)
    }

    pub fn event_retention(
        &self,
        ctx: &ExecutionContext,
        subject: &Subject,
    ) -> Result<RetentionMetadata, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let state = self.lock()?;
        let mut sequences = state
            .events
            .iter()
            .filter(|record| record.tenant_id == tenant && record.envelope.subject == *subject)
            .map(|record| record.sequence);
        let Some(first) = sequences.next() else {
            return Ok(RetentionMetadata {
                earliest_sequence: 0,
                latest_sequence: 0,
                retention_seconds: None,
            });
        };
        let latest = sequences.last().unwrap_or(first);
        Ok(RetentionMetadata {
            earliest_sequence: first,
            latest_sequence: latest,
            retention_seconds: None,
        })
    }

    pub fn replay_events(
        &self,
        ctx: &ExecutionContext,
        consumer: ConsumerId,
        subject: Subject,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Vec<EventRecord>, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        if limit == 0 || limit > 1000 {
            return Err(resource_error("event replay limit must be 1..=1000"));
        }
        let state = self.lock()?;
        let earliest = state
            .events
            .iter()
            .filter(|record| record.tenant_id == tenant && record.envelope.subject == subject)
            .map(|record| record.sequence)
            .min();
        if earliest.is_some_and(|sequence| after_sequence.saturating_add(1) < sequence) {
            return Err(InterconnectError::new(
                InterconnectErrorCode::CursorExpired,
                "event cursor predates the retained replay window",
            ));
        }
        let records: Vec<_> = state
            .events
            .iter()
            .filter(|record| {
                record.tenant_id == tenant
                    && record.envelope.subject == subject
                    && record.sequence > after_sequence
            })
            .take(limit)
            .cloned()
            .collect();
        drop(state);
        if let Some(last) = records.last() {
            self.checkpoint_event(ctx, consumer, subject, last.sequence)?;
        }
        Ok(records)
    }

    pub fn checkpoint_event(
        &self,
        ctx: &ExecutionContext,
        consumer: ConsumerId,
        subject: Subject,
        sequence: u64,
    ) -> Result<EventCursor, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let mut state = self.lock()?;
        let slot = state
            .checkpoints
            .entry((tenant, consumer.clone(), subject.clone()))
            .or_default();
        *slot = (*slot).max(sequence);
        Ok(EventCursor {
            consumer,
            subject,
            sequence: *slot,
        })
    }

    pub fn enqueue_work(
        &self,
        ctx: &ExecutionContext,
        envelope: MessageEnvelope,
        retry_budget: u32,
    ) -> Result<MessageId, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        validate_lane(&envelope, MessageKind::Work, ctx)?;
        if retry_budget > 100 {
            return Err(resource_error("work retry budget exceeds 100"));
        }
        let mut state = self.lock()?;

        if let Some(existing) = state.work.get(envelope.id.as_str()) {
            if existing.tenant_id == tenant
                && existing.envelope == envelope
                && existing.retry_budget == retry_budget
            {
                return Ok(envelope.id.clone());
            }
            return Err(identity_conflict("work message id"));
        }
        if let Some(key) = envelope.idempotency_key.as_ref() {
            let dedup = (
                tenant.clone(),
                envelope.subject.clone(),
                key.as_str().to_owned(),
            );
            if let Some(existing_id) = state.work_dedup.get(&dedup) {
                let existing = state.work.get(existing_id).ok_or_else(|| {
                    InterconnectError::new(
                        InterconnectErrorCode::DriverFailure,
                        "work dedup index points to a missing record",
                    )
                })?;
                if existing.envelope == envelope && existing.retry_budget == retry_budget {
                    return MessageId::new(existing_id.clone());
                }
                return Err(identity_conflict("work idempotency key"));
            }
        }
        if state.work.len() >= IN_PROCESS_WORK_LIMIT {
            return Err(resource_error("in-process work budget exhausted"));
        }

        let work_id = envelope.id.clone();
        state.work.insert(
            work_id.as_str().to_owned(),
            WorkRecord {
                tenant_id: tenant.clone(),
                envelope: envelope.clone(),
                status: WorkStatus::Ready,
                claim_generation: 0,
                lease_owner: None,
                lease_deadline_ms: None,
                attempt: 0,
                retry_budget,
                created_at_ms: envelope.created_at_ms,
                last_error: None,
            },
        );
        if let Some(key) = envelope.idempotency_key.as_ref() {
            state.work_dedup.insert(
                (tenant, envelope.subject.clone(), key.as_str().to_owned()),
                work_id.as_str().to_owned(),
            );
        }
        Ok(work_id)
    }

    pub fn claim_work(
        &self,
        ctx: &ExecutionContext,
        subject: &Subject,
        lease_owner: LeaseOwner,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<Option<WorkClaim>, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        if lease_ms == 0 || lease_ms > 3_600_000 {
            return Err(resource_error("work lease must be 1..=3600000 ms"));
        }
        let mut state = self.lock()?;
        let candidate = state.work.values_mut().find(|record| {
            record.tenant_id == tenant
                && record.envelope.subject == *subject
                && (record.status == WorkStatus::Ready
                    || (record.status == WorkStatus::Leased
                        && record
                            .lease_deadline_ms
                            .is_some_and(|deadline| deadline <= now_ms)))
        });
        let Some(record) = candidate else {
            return Ok(None);
        };
        let max_attempts = record.retry_budget.saturating_add(1);
        if record.attempt >= max_attempts {
            record.status = WorkStatus::DeadLetter;
            record.lease_owner = None;
            record.lease_deadline_ms = None;
            return Ok(None);
        }
        record.claim_generation = record.claim_generation.saturating_add(1);
        record.attempt = record.attempt.saturating_add(1);
        record.status = WorkStatus::Leased;
        record.lease_owner = Some(lease_owner.clone());
        record.lease_deadline_ms = Some(now_ms.saturating_add(lease_ms));
        Ok(Some(WorkClaim {
            work_id: record.envelope.id.clone(),
            claim_generation: record.claim_generation,
            lease_owner,
            lease_deadline_ms: record.lease_deadline_ms.unwrap_or(now_ms),
            attempt: record.attempt,
            retry_budget: record.retry_budget,
        }))
    }

    pub fn ack_work(
        &self,
        ctx: &ExecutionContext,
        claim: &WorkClaim,
    ) -> Result<(), InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let mut state = self.lock()?;
        let record = state.work.get_mut(claim.work_id.as_str()).ok_or_else(|| {
            InterconnectError::new(InterconnectErrorCode::InvalidIdentifier, "unknown work id")
        })?;
        validate_claim(record, &tenant, claim)?;
        record.status = WorkStatus::Succeeded;
        record.lease_owner = None;
        record.lease_deadline_ms = None;
        Ok(())
    }

    pub fn nack_work(
        &self,
        ctx: &ExecutionContext,
        claim: &WorkClaim,
        retryable: bool,
        error: impl Into<String>,
    ) -> Result<(), InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let mut state = self.lock()?;
        let record = state.work.get_mut(claim.work_id.as_str()).ok_or_else(|| {
            InterconnectError::new(InterconnectErrorCode::InvalidIdentifier, "unknown work id")
        })?;
        validate_claim(record, &tenant, claim)?;
        record.last_error = Some(error.into());
        record.lease_owner = None;
        record.lease_deadline_ms = None;
        record.status = if retryable && record.attempt <= record.retry_budget {
            WorkStatus::Ready
        } else {
            WorkStatus::DeadLetter
        };
        Ok(())
    }

    pub fn work_metrics(
        &self,
        ctx: &ExecutionContext,
        subject: &Subject,
        now_ms: u64,
    ) -> Result<WorkMetrics, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let state = self.lock()?;
        let matching: Vec<_> = state
            .work
            .values()
            .filter(|record| record.tenant_id == tenant && record.envelope.subject == *subject)
            .collect();
        let oldest = matching
            .iter()
            .filter(|record| record.status == WorkStatus::Ready)
            .map(|record| record.created_at_ms)
            .min();
        Ok(WorkMetrics {
            ready: matching
                .iter()
                .filter(|record| record.status == WorkStatus::Ready)
                .count(),
            leased: matching
                .iter()
                .filter(|record| record.status == WorkStatus::Leased)
                .count(),
            dead_letter: matching
                .iter()
                .filter(|record| record.status == WorkStatus::DeadLetter)
                .count(),
            oldest_ready_age_ms: oldest.map_or(0, |created| now_ms.saturating_sub(created)),
        })
    }

    pub fn put_state_snapshot(
        &self,
        ctx: &ExecutionContext,
        revision: StateRevision,
        envelope: MessageEnvelope,
    ) -> Result<(), InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        validate_lane(&envelope, MessageKind::StateSnapshot, ctx)?;
        let mut state = self.lock()?;
        let key = (tenant.clone(), envelope.subject.clone());
        if state
            .state_heads
            .get(&key)
            .is_some_and(|current| current.0 >= revision.0)
        {
            return Err(state_revision_error());
        }
        state.state_heads.insert(key.clone(), revision);
        state.snapshots.insert(
            key,
            StateSnapshotRecord {
                tenant_id: tenant,
                subject: envelope.subject.clone(),
                revision,
                envelope,
            },
        );
        Ok(())
    }

    pub fn append_state_delta(
        &self,
        ctx: &ExecutionContext,
        revision: StateRevision,
        envelope: MessageEnvelope,
    ) -> Result<WatchCursor, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        validate_lane(&envelope, MessageKind::StateDelta, ctx)?;
        let mut state = self.lock()?;
        let key = (tenant.clone(), envelope.subject.clone());
        if state
            .state_heads
            .get(&key)
            .is_some_and(|current| current.0 >= revision.0)
        {
            return Err(state_revision_error());
        }
        state.state_heads.insert(key, revision);
        state.next_state_cursor = state.next_state_cursor.saturating_add(1);
        let cursor = WatchCursor(state.next_state_cursor);
        state.deltas.push(StateDeltaRecord {
            cursor,
            tenant_id: tenant,
            subject: envelope.subject.clone(),
            revision,
            envelope,
        });
        if state.deltas.len() > IN_PROCESS_STATE_DELTA_LIMIT {
            let overflow = state.deltas.len() - IN_PROCESS_STATE_DELTA_LIMIT;
            state.deltas.drain(..overflow);
        }
        Ok(cursor)
    }

    pub fn watch_state(
        &self,
        ctx: &ExecutionContext,
        subject: Subject,
        after: WatchCursor,
        limit: usize,
    ) -> Result<StateWatchBatch, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        if limit == 0 || limit > 1000 {
            return Err(resource_error("state watch limit must be 1..=1000"));
        }
        let state = self.lock()?;
        let relevant: Vec<_> = state
            .deltas
            .iter()
            .filter(|delta| delta.tenant_id == tenant && delta.subject == subject)
            .cloned()
            .collect();
        let earliest = relevant
            .first()
            .map(|delta| delta.cursor.0)
            .unwrap_or(after.0);
        let latest_revision = state
            .state_heads
            .get(&(tenant.clone(), subject.clone()))
            .copied()
            .unwrap_or(StateRevision(0));
        let snapshot = state
            .snapshots
            .get(&(tenant.clone(), subject.clone()))
            .cloned();
        if after.0.saturating_add(1) < earliest {
            return Ok(StateWatchBatch {
                directive: ResumeDirective::ResyncRequired {
                    latest_revision,
                    earliest_available_cursor: WatchCursor(earliest),
                },
                snapshot,
                deltas: Vec::new(),
            });
        }
        let deltas: Vec<_> = relevant
            .into_iter()
            .filter(|delta| delta.cursor.0 > after.0)
            .take(limit)
            .collect();
        let cursor = deltas.last().map(|delta| delta.cursor).unwrap_or(after);
        Ok(StateWatchBatch {
            directive: ResumeDirective::Resume { cursor },
            snapshot,
            deltas,
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, InProcessState>, InterconnectError> {
        self.state.lock().map_err(|_| {
            InterconnectError::new(
                InterconnectErrorCode::DriverFailure,
                "in-process interconnect state lock poisoned",
            )
        })
    }
}

pub(super) fn trusted_tenant(ctx: &ExecutionContext) -> Result<String, InterconnectError> {
    ctx.data_scope()
        .tenant_id_opt()
        .map(|tenant| tenant.as_str().to_owned())
        .ok_or_else(|| {
            InterconnectError::new(
                InterconnectErrorCode::PolicyDenied,
                "durable tenant lanes require a trusted tenant DataScope",
            )
        })
}

pub(super) fn validate_lane(
    envelope: &MessageEnvelope,
    expected: MessageKind,
    ctx: &ExecutionContext,
) -> Result<(), InterconnectError> {
    envelope.validate()?;
    if envelope.kind != expected {
        return Err(InterconnectError::new(
            InterconnectErrorCode::ContractIncompatible,
            "message kind does not match the selected semantic lane",
        ));
    }
    if envelope.correlation_id.as_str() != ctx.correlation_id().as_str() {
        return Err(InterconnectError::new(
            InterconnectErrorCode::PolicyDenied,
            "lane correlation must be bound to trusted ExecutionContext",
        ));
    }
    if envelope.subject.is_reserved() && ctx.actor().id().is_some() {
        return Err(InterconnectError::new(
            InterconnectErrorCode::PolicyDenied,
            "reserved subjects are Runtime-owned and cannot be selected by an authenticated actor",
        ));
    }
    Ok(())
}

fn validate_claim(
    record: &WorkRecord,
    tenant: &str,
    claim: &WorkClaim,
) -> Result<(), InterconnectError> {
    if record.tenant_id != tenant
        || record.status != WorkStatus::Leased
        || record.claim_generation != claim.claim_generation
        || record.lease_owner.as_ref() != Some(&claim.lease_owner)
    {
        return Err(InterconnectError::new(
            InterconnectErrorCode::StaleClaimGeneration,
            "stale or foreign work claim cannot mutate the current generation",
        ));
    }
    Ok(())
}

fn identity_conflict(kind: &str) -> InterconnectError {
    InterconnectError::new(
        InterconnectErrorCode::ContractIncompatible,
        format!("{kind} was reused for a materially different or foreign-tenant message"),
    )
}

fn state_revision_error() -> InterconnectError {
    InterconnectError::new(
        InterconnectErrorCode::ContractIncompatible,
        "state revision must increase monotonically across snapshots and deltas",
    )
}

fn resource_error(message: impl Into<String>) -> InterconnectError {
    InterconnectError::new(InterconnectErrorCode::ResourceBudgetExceeded, message)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use serde_json::{Value, json};
    use system_core::transport::interconnect::{
        ContractBinding, ContractRef, ContractVersion, CorrelationId, Extensions, IdempotencyKey,
        MessageEnvelope, MessageKind, OrderingKey, PayloadRef, RequestTarget, SchemaRef,
    };
    use system_core::{
        AccessRequirement, ActorIdentity, AuthorityContext, CommandMetadata, CommandSchema,
        DataScope, EffectClass, ExecutionContext, ExecutionMode, ModuleMetadata, ModuleSchema,
        NoopHttpClient, RequestId, Revision, SimulationSupport, SystemModule, TenantId,
        TenantMembershipId, TenantRole, TenantScope,
    };

    use super::*;

    struct ProbeModule;

    impl SystemModule for ProbeModule {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                name: "probe".into(),
                version: "1.0.0".into(),
                description: "interconnect request probe".into(),
                author: "test".into(),
                wasm_compatible: false,
                storage: None,
            }
        }

        fn init(&mut self, _config: Value) -> Result<(), String> {
            Ok(())
        }

        fn commands(&self) -> Vec<CommandMetadata> {
            vec![CommandMetadata::new(
                "read",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            )]
        }

        fn execute(
            &self,
            command: &str,
            payload: Value,
            _ctx: &ExecutionContext,
        ) -> Result<Value, String> {
            assert_eq!(command, "read");
            Ok(payload)
        }

        fn schema(&self) -> ModuleSchema {
            ModuleSchema {
                name: "probe".into(),
                description: "probe".into(),
                commands: vec![CommandSchema {
                    name: "read".into(),
                    description: "read".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                }],
            }
        }
    }

    fn ctx_for(tenant_text: &str, correlation: &str) -> ExecutionContext {
        let tenant = TenantId::new(tenant_text).unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                format!("staff-{tenant_text}"),
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new(format!("membership-{tenant_text}"))
                        .unwrap(),
                    tenant_id: tenant.clone(),
                    role: TenantRole::Staff,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(correlation).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn ctx(correlation: &str) -> ExecutionContext {
        ctx_for("tenant-a", correlation)
    }

    fn envelope(kind: MessageKind, subject: &str, id: &str, correlation: &str) -> MessageEnvelope {
        MessageEnvelope {
            id: MessageId::new(id).unwrap(),
            kind,
            subject: Subject::new(subject).unwrap(),
            contract: ContractBinding {
                contract: ContractRef::new(subject).unwrap(),
                version: ContractVersion::new("1.0.0").unwrap(),
                schema: SchemaRef::new(format!("{subject}.v1")).unwrap(),
            },
            correlation_id: CorrelationId::new(correlation).unwrap(),
            causation_id: None,
            created_at_ms: 10,
            deadline_ms: Some(1000),
            ordering_key: Some(OrderingKey::new("aggregate-a").unwrap()),
            idempotency_key: Some(IdempotencyKey::new(format!("idem-{id}")).unwrap()),
            payload: PayloadRef::Inline(json!({"ok": true})),
            extensions: Extensions::empty(),
        }
    }

    #[test]
    fn request_lane_dispatches_only_through_registry_and_binds_correlation() {
        let registry = Arc::new(
            ModuleRegistry::new(HashMap::from([(
                "probe".into(),
                Arc::new(ProbeModule) as Arc<dyn SystemModule>,
            )]))
            .unwrap(),
        );
        let fabric = InterconnectFabric::new(registry);
        let trusted = ctx("corr-a");
        let request = RequestMessage {
            envelope: envelope(MessageKind::Query, "probe.read", "msg-1", "corr-a"),
            target: RequestTarget::new("probe", "read").unwrap(),
        };
        assert!(matches!(
            fabric.dispatch_request(&request, &trusted, 20, &CancellationSignal::default()),
            RequestResult::Ok(_)
        ));

        let foreign = ctx("corr-b");
        assert!(matches!(
            fabric.dispatch_request(&request, &foreign, 20, &CancellationSignal::default()),
            RequestResult::Err(InterconnectError {
                code: InterconnectErrorCode::PolicyDenied,
                ..
            })
        ));
    }

    #[test]
    fn cancellation_is_admission_only() {
        let registry = Arc::new(
            ModuleRegistry::new(HashMap::from([(
                "probe".into(),
                Arc::new(ProbeModule) as Arc<dyn SystemModule>,
            )]))
            .unwrap(),
        );
        let fabric = InterconnectFabric::new(registry);
        let trusted = ctx("corr-a");
        let request = RequestMessage {
            envelope: envelope(MessageKind::Query, "probe.read", "msg-1", "corr-a"),
            target: RequestTarget::new("probe", "read").unwrap(),
        };
        let cancellation = CancellationSignal::default();
        cancellation.cancel();
        assert!(matches!(
            fabric.dispatch_request(&request, &trusted, 20, &cancellation),
            RequestResult::Err(InterconnectError {
                code: InterconnectErrorCode::Cancelled,
                ..
            })
        ));
    }

    #[test]
    fn event_dedup_requires_material_identity_and_tenant_scope() {
        let driver = InProcessDriver::default();
        let trusted = ctx("corr-a");
        let first = envelope(MessageKind::Event, "order.closed", "event-1", "corr-a");
        let sequence = driver.append_event(&trusted, first.clone()).unwrap();
        assert_eq!(
            driver.append_event(&trusted, first.clone()).unwrap(),
            sequence
        );

        let mut changed = first.clone();
        changed.payload = PayloadRef::Inline(json!({"ok": false}));
        assert_eq!(
            driver.append_event(&trusted, changed).unwrap_err().code,
            InterconnectErrorCode::ContractIncompatible
        );

        let foreign = ctx_for("tenant-b", "corr-a");
        assert_eq!(
            driver.append_event(&foreign, first).unwrap_err().code,
            InterconnectErrorCode::ContractIncompatible
        );
    }

    #[test]
    fn event_replay_and_retention_are_stable() {
        let driver = InProcessDriver::default();
        let trusted = ctx("corr-a");
        let first = envelope(MessageKind::Event, "order.closed", "event-1", "corr-a");
        let sequence = driver.append_event(&trusted, first).unwrap();
        let retention = driver
            .event_retention(&trusted, &Subject::new("order.closed").unwrap())
            .unwrap();
        assert_eq!(
            (retention.earliest_sequence, retention.latest_sequence),
            (sequence, sequence)
        );
        let replay = driver
            .replay_events(
                &trusted,
                ConsumerId::new("projection-a").unwrap(),
                Subject::new("order.closed").unwrap(),
                0,
                10,
            )
            .unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].sequence, sequence);
    }

    #[test]
    fn work_idempotency_rejects_material_or_foreign_tenant_reuse() {
        let driver = InProcessDriver::default();
        let trusted = ctx("corr-a");
        let work = envelope(MessageKind::Work, "workflow.step", "work-1", "corr-a");
        driver.enqueue_work(&trusted, work.clone(), 2).unwrap();
        assert!(driver.enqueue_work(&trusted, work.clone(), 2).is_ok());
        assert_eq!(
            driver
                .enqueue_work(&trusted, work.clone(), 3)
                .unwrap_err()
                .code,
            InterconnectErrorCode::ContractIncompatible
        );
        let foreign = ctx_for("tenant-b", "corr-a");
        assert_eq!(
            driver.enqueue_work(&foreign, work, 2).unwrap_err().code,
            InterconnectErrorCode::ContractIncompatible
        );
    }

    #[test]
    fn work_reclaim_fences_stale_worker_generation() {
        let driver = InProcessDriver::default();
        let trusted = ctx("corr-a");
        driver
            .enqueue_work(
                &trusted,
                envelope(MessageKind::Work, "workflow.step", "work-1", "corr-a"),
                2,
            )
            .unwrap();
        let subject = Subject::new("workflow.step").unwrap();
        let claim_a = driver
            .claim_work(
                &trusted,
                &subject,
                LeaseOwner::new("worker-a").unwrap(),
                100,
                10,
            )
            .unwrap()
            .unwrap();
        let claim_b = driver
            .claim_work(
                &trusted,
                &subject,
                LeaseOwner::new("worker-b").unwrap(),
                111,
                10,
            )
            .unwrap()
            .unwrap();
        assert!(claim_b.claim_generation > claim_a.claim_generation);
        assert_eq!(
            driver.ack_work(&trusted, &claim_a).unwrap_err().code,
            InterconnectErrorCode::StaleClaimGeneration
        );
        driver.ack_work(&trusted, &claim_b).unwrap();
    }

    #[test]
    fn work_ack_has_no_external_operation_outcome_authority() {
        use crate::integration::operation::OperationState;

        let driver = InProcessDriver::default();
        let trusted = ctx("corr-a");
        driver
            .enqueue_work(
                &trusted,
                envelope(
                    MessageKind::Work,
                    "integration.reconcile",
                    "work-1",
                    "corr-a",
                ),
                0,
            )
            .unwrap();
        let claim = driver
            .claim_work(
                &trusted,
                &Subject::new("integration.reconcile").unwrap(),
                LeaseOwner::new("worker-a").unwrap(),
                100,
                10,
            )
            .unwrap()
            .unwrap();
        let external_operation_state = OperationState::UnknownOutcome;
        driver.ack_work(&trusted, &claim).unwrap();
        assert_eq!(external_operation_state, OperationState::UnknownOutcome);
    }

    #[test]
    fn state_revision_is_monotonic_across_snapshot_and_delta() {
        let driver = InProcessDriver::default();
        let trusted = ctx("corr-a");
        driver
            .put_state_snapshot(
                &trusted,
                StateRevision(1),
                envelope(
                    MessageKind::StateSnapshot,
                    "order.state",
                    "snap-1",
                    "corr-a",
                ),
            )
            .unwrap();
        let cursor = driver
            .append_state_delta(
                &trusted,
                StateRevision(5),
                envelope(MessageKind::StateDelta, "order.state", "delta-5", "corr-a"),
            )
            .unwrap();
        assert_eq!(
            driver
                .append_state_delta(
                    &trusted,
                    StateRevision(3),
                    envelope(MessageKind::StateDelta, "order.state", "delta-3", "corr-a"),
                )
                .unwrap_err()
                .code,
            InterconnectErrorCode::ContractIncompatible
        );
        let batch = driver
            .watch_state(
                &trusted,
                Subject::new("order.state").unwrap(),
                WatchCursor(0),
                10,
            )
            .unwrap();
        assert_eq!(batch.deltas.len(), 1);
        assert!(matches!(
            batch.directive,
            ResumeDirective::Resume { cursor: seen } if seen == cursor
        ));
    }
}

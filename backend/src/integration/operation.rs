use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::transport::{TransportErrorClass, TransportFailure};
use super::types::{
    ExternalAttemptId, ExternalOperationId, IntegrationError, ResolvedProviderBinding,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Planned,
    Ready,
    Dispatching,
    Succeeded,
    Rejected,
    RetryableFailure,
    NonRetryableFailure,
    UnknownOutcome,
    Reconciling,
    Resolved,
    ManualResolutionRequired,
}

/// The only classification R3 may consume from R2 when it decides whether an
/// admitted financial effect is confirmed, can release its reservation, or
/// remains a closure blocker.  R3 never writes these states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinancialEffectState {
    Confirmed,
    KnownNoEffect,
    Unresolved,
}

impl OperationState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::Rejected
                | Self::NonRetryableFailure
                | Self::Resolved
                | Self::ManualResolutionRequired
        )
    }

    pub fn financial_effect_state(&self) -> FinancialEffectState {
        match self {
            Self::Succeeded | Self::Resolved => FinancialEffectState::Confirmed,
            // A rejected request and a failure proven to have happened before
            // an external effect are both known-no-effect.  They release an
            // R3 authorization reservation but do not count toward settlement.
            Self::Rejected | Self::NonRetryableFailure => FinancialEffectState::KnownNoEffect,
            Self::Planned
            | Self::Ready
            | Self::Dispatching
            | Self::RetryableFailure
            | Self::UnknownOutcome
            | Self::Reconciling
            | Self::ManualResolutionRequired => FinancialEffectState::Unresolved,
        }
    }

    pub fn from_persisted(value: &str) -> Option<Self> {
        Some(match value {
            "planned" => Self::Planned,
            "ready" => Self::Ready,
            "dispatching" => Self::Dispatching,
            "succeeded" => Self::Succeeded,
            "rejected" => Self::Rejected,
            "retryable_failure" => Self::RetryableFailure,
            "non_retryable_failure" => Self::NonRetryableFailure,
            "unknown_outcome" => Self::UnknownOutcome,
            "reconciling" => Self::Reconciling,
            "resolved" => Self::Resolved,
            "manual_resolution_required" => Self::ManualResolutionRequired,
            _ => return None,
        })
    }

    pub fn as_persisted(&self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Ready => "ready",
            Self::Dispatching => "dispatching",
            Self::Succeeded => "succeeded",
            Self::Rejected => "rejected",
            Self::RetryableFailure => "retryable_failure",
            Self::NonRetryableFailure => "non_retryable_failure",
            Self::UnknownOutcome => "unknown_outcome",
            Self::Reconciling => "reconciling",
            Self::Resolved => "resolved",
            Self::ManualResolutionRequired => "manual_resolution_required",
        }
    }
}

/// Immutable provider-neutral material from which the durable external-effect
/// identity is derived. The final provider request body is intentionally not
/// modeled by Stage 2 fixture runtime, but the persisted compatibility column
/// `request_hash` is no longer a caller-chosen authority value: it is a
/// canonical hash bound to the resolved tenant/provider/binding revision,
/// operation identity, and the upstream request fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectIntent {
    pub tenant_id: String,
    pub provider_instance_id: String,
    pub binding_id: String,
    pub binding_revision: String,
    pub capability: String,
    pub operation_type: String,
    pub idempotency_key: String,
    pub request_fingerprint: String,
}

impl EffectIntent {
    fn from_resolved(
        binding: &ResolvedProviderBinding,
        operation_type: String,
        idempotency_key: String,
        request_fingerprint: String,
    ) -> Self {
        Self {
            tenant_id: binding.instance.tenant_id.clone(),
            provider_instance_id: binding.instance.id.as_str().to_owned(),
            binding_id: binding.binding.id.as_str().to_owned(),
            binding_revision: binding.binding.config_revision.clone(),
            capability: binding.binding.capability.as_str().to_owned(),
            operation_type,
            idempotency_key,
            request_fingerprint,
        }
    }

    pub fn canonical_hash(&self) -> Result<String, IntegrationError> {
        let encoded = serde_json::to_vec(self).map_err(|_| IntegrationError::Persistence)?;
        Ok(hex::encode(Sha256::digest(encoded)))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalOperation {
    pub id: ExternalOperationId,
    pub tenant_id: String,
    pub provider_instance_id: String,
    pub binding_id: String,
    pub binding_revision: String,
    pub capability: String,
    pub operation_type: String,
    pub idempotency_key: String,
    /// Compatibility field name. The value is the server-derived canonical
    /// EffectIntent hash, not the raw caller-provided request fingerprint.
    pub request_hash: String,
    pub state: OperationState,
    pub attempts: u32,
}

impl ExternalOperation {
    pub fn planned(
        id: ExternalOperationId,
        binding: &ResolvedProviderBinding,
        operation_type: impl Into<String>,
        idempotency_key: impl Into<String>,
        request_hash: impl Into<String>,
    ) -> Result<Self, IntegrationError> {
        let operation_type = operation_type.into();
        let idempotency_key = idempotency_key.into();
        let request_fingerprint = request_hash.into();
        if operation_type.trim().is_empty()
            || idempotency_key.trim().is_empty()
            || request_fingerprint.trim().is_empty()
        {
            return Err(IntegrationError::BlankValue {
                kind: "external operation field",
            });
        }
        let intent = EffectIntent::from_resolved(
            binding,
            operation_type,
            idempotency_key,
            request_fingerprint,
        );
        let request_hash = intent.canonical_hash()?;
        Ok(Self {
            id,
            tenant_id: intent.tenant_id,
            provider_instance_id: intent.provider_instance_id,
            binding_id: intent.binding_id,
            binding_revision: intent.binding_revision,
            capability: intent.capability,
            operation_type: intent.operation_type,
            idempotency_key: intent.idempotency_key,
            request_hash,
            state: OperationState::Planned,
            attempts: 0,
        })
    }

    pub fn ready(&mut self) -> Result<(), IntegrationError> {
        transition(&mut self.state, OperationState::Ready)
    }

    pub fn begin_attempt(&mut self) -> Result<ExternalOperationAttempt, IntegrationError> {
        if !matches!(
            self.state,
            OperationState::Ready | OperationState::RetryableFailure
        ) {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        self.state = OperationState::Dispatching;
        self.attempts += 1;
        Ok(ExternalOperationAttempt {
            id: ExternalAttemptId::new(format!("{}:{}", self.id.as_str(), self.attempts))?,
            attempt_number: self.attempts,
            state: OperationState::Dispatching,
            classification: None,
        })
    }

    pub fn record_attempt(
        &mut self,
        attempt: &mut ExternalOperationAttempt,
        outcome: AttemptOutcome,
    ) -> Result<(), IntegrationError> {
        if !matches!(self.state, OperationState::Dispatching) {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        let next = match &outcome {
            AttemptOutcome::Succeeded => OperationState::Succeeded,
            AttemptOutcome::Rejected => OperationState::Rejected,
            AttemptOutcome::Failure(failure) if failure.may_have_dispatched => {
                OperationState::UnknownOutcome
            }
            AttemptOutcome::Failure(failure) if failure.is_retryable() => {
                OperationState::RetryableFailure
            }
            AttemptOutcome::Failure(_) => OperationState::NonRetryableFailure,
        };
        attempt.classification = match &outcome {
            AttemptOutcome::Failure(failure) => Some(failure.class),
            _ => None,
        };
        attempt.state = next.clone();
        self.state = next;
        Ok(())
    }

    /// Unknown outcomes require a provider query/reconciliation decision before
    /// a new dispatch attempt can be authorized. This intentionally makes a
    /// blind retry impossible through the state machine.
    pub fn begin_reconciliation(&mut self) -> Result<(), IntegrationError> {
        transition(&mut self.state, OperationState::Reconciling)
    }

    pub fn resolve_reconciliation(
        &mut self,
        provider_effect_confirmed: bool,
    ) -> Result<(), IntegrationError> {
        if self.state != OperationState::Reconciling {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        self.state = if provider_effect_confirmed {
            OperationState::Resolved
        } else {
            OperationState::Ready
        };
        Ok(())
    }

    pub fn require_manual_resolution(&mut self) -> Result<(), IntegrationError> {
        transition(&mut self.state, OperationState::ManualResolutionRequired)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalOperationAttempt {
    pub id: ExternalAttemptId,
    pub attempt_number: u32,
    pub state: OperationState,
    pub classification: Option<TransportErrorClass>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptOutcome {
    Succeeded,
    Rejected,
    Failure(TransportFailure),
}

fn transition(current: &mut OperationState, next: OperationState) -> Result<(), IntegrationError> {
    let allowed = matches!(
        (&*current, &next),
        (OperationState::Planned, OperationState::Ready)
            | (OperationState::UnknownOutcome, OperationState::Reconciling)
            | (
                OperationState::Reconciling,
                OperationState::ManualResolutionRequired
            )
            | (_, OperationState::ManualResolutionRequired)
    );
    if !allowed {
        return Err(IntegrationError::InvalidOperationTransition);
    }
    *current = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{AttemptOutcome, ExternalOperation, FinancialEffectState, OperationState};
    use crate::integration::transport::{TransportErrorClass, TransportFailure};
    use crate::integration::types::{
        CapabilityId, ExternalOperationId, ProviderBinding, ProviderBindingId, ProviderHealth,
        ProviderId, ProviderInstance, ProviderInstanceId, ProviderLifecycle, ProviderManifest,
        ProviderReadiness, ResolvedProviderBinding,
    };

    fn binding() -> ResolvedProviderBinding {
        let capability = CapabilityId::new("payment.refund").unwrap();
        let provider = ProviderId::new("fixture-payment").unwrap();
        let instance = ProviderInstance {
            id: ProviderInstanceId::new("instance-a").unwrap(),
            tenant_id: "tenant-a".into(),
            provider_id: provider.clone(),
            manifest_version: "1".into(),
            config_revision: "rev-1".into(),
            config: BTreeMap::new(),
            secret_refs: BTreeMap::new(),
            lifecycle: ProviderLifecycle::Active,
            health: ProviderHealth::Ready,
            readiness: ProviderReadiness::Fixture,
        };
        ResolvedProviderBinding {
            manifest: ProviderManifest {
                provider_id: provider,
                version: "1".into(),
                capabilities: BTreeSet::from([capability.clone()]),
                config_schema: vec![],
                secret_schema: vec![],
                api_versions: BTreeMap::new(),
                webhook_types: BTreeSet::new(),
                simulation_capabilities: BTreeSet::from([capability.clone()]),
                readiness: ProviderReadiness::Fixture,
                compatibility: BTreeMap::new(),
            },
            binding: ProviderBinding {
                id: ProviderBindingId::new("binding-a").unwrap(),
                tenant_id: "tenant-a".into(),
                provider_instance_id: instance.id.clone(),
                capability,
                config_revision: "rev-1".into(),
                enabled: true,
            },
            instance,
        }
    }

    #[test]
    fn canonical_effect_intent_hash_binds_resolved_revision_and_request_fingerprint() {
        let first_binding = binding();
        let first = ExternalOperation::planned(
            ExternalOperationId::new("intent-a").unwrap(),
            &first_binding,
            "refund",
            "idem-intent",
            "upstream-request-a",
        )
        .unwrap();
        assert_eq!(first.request_hash.len(), 64);
        assert_ne!(first.request_hash, "upstream-request-a");

        let same = ExternalOperation::planned(
            ExternalOperationId::new("intent-b").unwrap(),
            &first_binding,
            "refund",
            "idem-intent",
            "upstream-request-a",
        )
        .unwrap();
        assert_eq!(first.request_hash, same.request_hash);

        let mut next_binding = first_binding;
        next_binding.binding.config_revision = "rev-2".into();
        next_binding.instance.config_revision = "rev-2".into();
        let next = ExternalOperation::planned(
            ExternalOperationId::new("intent-c").unwrap(),
            &next_binding,
            "refund",
            "idem-intent",
            "upstream-request-a",
        )
        .unwrap();
        assert_ne!(first.request_hash, next.request_hash);
    }

    #[test]
    fn post_dispatch_timeout_requires_reconciliation_before_retry() {
        let binding = binding();
        let mut operation = ExternalOperation::planned(
            ExternalOperationId::new("operation-a").unwrap(),
            &binding,
            "refund",
            "idem-a",
            "request-a",
        )
        .unwrap();
        operation.ready().unwrap();
        let mut attempt = operation.begin_attempt().unwrap();
        operation
            .record_attempt(
                &mut attempt,
                AttemptOutcome::Failure(TransportFailure::after_dispatch(
                    TransportErrorClass::ResponseTimeout,
                )),
            )
            .unwrap();
        assert_eq!(operation.state, OperationState::UnknownOutcome);
        assert!(operation.begin_attempt().is_err());
        operation.begin_reconciliation().unwrap();
        operation.resolve_reconciliation(false).unwrap();
        assert_eq!(operation.state, OperationState::Ready);
        assert_eq!(operation.begin_attempt().unwrap().attempt_number, 2);
    }

    #[test]
    fn before_dispatch_429_is_retryable_and_4xx_is_not() {
        let binding = binding();
        let mut retryable = ExternalOperation::planned(
            ExternalOperationId::new("operation-b").unwrap(),
            &binding,
            "refund",
            "idem-b",
            "request-b",
        )
        .unwrap();
        retryable.ready().unwrap();
        let mut attempt = retryable.begin_attempt().unwrap();
        retryable
            .record_attempt(
                &mut attempt,
                AttemptOutcome::Failure(TransportFailure::before_dispatch(
                    TransportErrorClass::Remote429,
                )),
            )
            .unwrap();
        assert_eq!(retryable.state, OperationState::RetryableFailure);

        let mut rejected = retryable;
        let mut attempt = rejected.begin_attempt().unwrap();
        rejected
            .record_attempt(
                &mut attempt,
                AttemptOutcome::Failure(TransportFailure::before_dispatch(
                    TransportErrorClass::Remote4xx,
                )),
            )
            .unwrap();
        assert_eq!(rejected.state, OperationState::NonRetryableFailure);
    }

    #[test]
    fn financial_effect_classification_releases_only_known_no_effect_states() {
        assert_eq!(
            OperationState::Succeeded.financial_effect_state(),
            FinancialEffectState::Confirmed
        );
        assert_eq!(
            OperationState::Resolved.financial_effect_state(),
            FinancialEffectState::Confirmed
        );
        for state in [
            OperationState::Rejected,
            OperationState::NonRetryableFailure,
        ] {
            assert_eq!(
                state.financial_effect_state(),
                FinancialEffectState::KnownNoEffect
            );
        }
        for state in [
            OperationState::RetryableFailure,
            OperationState::UnknownOutcome,
            OperationState::Reconciling,
            OperationState::ManualResolutionRequired,
        ] {
            assert_eq!(
                state.financial_effect_state(),
                FinancialEffectState::Unresolved
            );
        }
    }
}

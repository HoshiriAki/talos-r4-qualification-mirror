use std::time::Duration;

use super::operation::ExternalOperation;
use super::transport::TransportFailure;
use super::types::{ExternalAttemptId, ExternalOperationId, IntegrationError, ProviderBindingId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl RetryPolicy {
    pub fn validate(&self) -> Result<(), IntegrationError> {
        if self.max_attempts == 0
            || self.base_delay.is_zero()
            || self.max_delay.is_zero()
            || self.base_delay > self.max_delay
        {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        Ok(())
    }

    pub fn delay_for_attempt(&self, attempt_number: u32) -> Duration {
        let exponent = attempt_number.saturating_sub(1).min(20);
        let multiplier = 1_u32 << exponent;
        self.base_delay
            .checked_mul(multiplier)
            .unwrap_or(self.max_delay)
            .min(self.max_delay)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRuntimePolicy {
    pub retry: RetryPolicy,
    pub max_in_flight_per_binding: u32,
    pub max_attempts_per_minute: u32,
    pub circuit_failure_threshold: u32,
    pub circuit_open_for: Duration,
}

impl Default for OperationRuntimePolicy {
    fn default() -> Self {
        Self {
            retry: RetryPolicy {
                max_attempts: 4,
                base_delay: Duration::from_secs(5),
                max_delay: Duration::from_secs(5 * 60),
            },
            max_in_flight_per_binding: 2,
            max_attempts_per_minute: 20,
            circuit_failure_threshold: 3,
            circuit_open_for: Duration::from_secs(60),
        }
    }
}

impl OperationRuntimePolicy {
    pub fn validate(&self) -> Result<(), IntegrationError> {
        self.retry.validate()?;
        if self.max_in_flight_per_binding == 0
            || self.max_attempts_per_minute == 0
            || self.circuit_failure_threshold == 0
            || self.circuit_open_for.is_zero()
        {
            return Err(IntegrationError::InvalidOperationTransition);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedOperation {
    pub tenant_id: String,
    pub operation_id: ExternalOperationId,
    pub attempt_id: ExternalAttemptId,
    pub binding_id: ProviderBindingId,
    pub binding_revision: String,
    pub capability: String,
    pub operation_type: String,
    pub request_hash: String,
    pub attempt_number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchResult {
    Succeeded { provider_result_ref: Option<String> },
    Rejected { provider_result_ref: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperationRuntimeMetrics {
    pub ready: u64,
    pub dispatching: u64,
    pub retryable_failure: u64,
    pub unknown_outcome: u64,
    pub circuit_open: u64,
}

pub trait OperationRuntimePersistence: Send + Sync {
    fn persist_operation(
        &self,
        operation: &ExternalOperation,
    ) -> Result<ExternalOperationId, IntegrationError>;

    fn claim_next_operation(
        &self,
        tenant_id: &str,
        policy: &OperationRuntimePolicy,
    ) -> Result<Option<ClaimedOperation>, IntegrationError>;

    fn record_runtime_outcome(
        &self,
        claimed: &ClaimedOperation,
        outcome: Result<DispatchResult, TransportFailure>,
        policy: &OperationRuntimePolicy,
    ) -> Result<(), IntegrationError>;

    fn recover_inflight_operations(&self, tenant_id: &str) -> Result<u64, IntegrationError>;

    fn operation_runtime_metrics(
        &self,
        tenant_id: &str,
    ) -> Result<OperationRuntimeMetrics, IntegrationError>;

    fn begin_reconciliation(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
        evidence_ref: &str,
    ) -> Result<(), IntegrationError>;

    fn resolve_reconciliation(
        &self,
        tenant_id: &str,
        operation_id: &ExternalOperationId,
        effect_confirmed: bool,
        actor_ref: &str,
    ) -> Result<(), IntegrationError>;
}

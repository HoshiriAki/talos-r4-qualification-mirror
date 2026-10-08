use super::deposit::DepositReconciliationOutcome;
use super::types::{DepositId, ExternalOperationId, IntegrationError, Money, RefundId};

pub(crate) trait DepositRefundPersistence: Send + Sync {
    fn create_deposit(
        &self,
        tenant_id: &str,
        authority_kind: &str,
        authority_id: &str,
        expected: Money,
    ) -> Result<DepositId, IntegrationError>;

    fn record_deposit_received(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError>;

    fn hold_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError>;

    fn deduct_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError>;

    fn release_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError>;

    fn request_refund(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        idempotency_key: &str,
        reason: &str,
    ) -> Result<RefundId, IntegrationError>;

    fn link_refund_operation(
        &self,
        tenant_id: &str,
        refund_id: &RefundId,
        operation_id: &ExternalOperationId,
    ) -> Result<(), IntegrationError>;

    fn reconcile_refund(
        &self,
        tenant_id: &str,
        refund_id: &RefundId,
        outcome: DepositReconciliationOutcome,
        evidence_ref: &str,
        actor_ref: Option<&str>,
    ) -> Result<(), IntegrationError>;
}

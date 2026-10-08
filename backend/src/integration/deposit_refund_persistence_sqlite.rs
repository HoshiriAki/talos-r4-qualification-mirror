use super::deposit::DepositReconciliationOutcome;
use super::deposit_refund_persistence_contract::DepositRefundPersistence;
use super::store::IntegrationStore;
use super::types::{DepositId, ExternalOperationId, IntegrationError, Money, RefundId};

impl DepositRefundPersistence for IntegrationStore {
    fn create_deposit(
        &self,
        tenant_id: &str,
        authority_kind: &str,
        authority_id: &str,
        expected: Money,
    ) -> Result<DepositId, IntegrationError> {
        IntegrationStore::create_deposit(self, tenant_id, authority_kind, authority_id, expected)
    }

    fn record_deposit_received(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::record_deposit_received(self, tenant_id, deposit_id, money, audit_ref)
    }

    fn hold_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::hold_deposit(self, tenant_id, deposit_id, money, audit_ref)
    }

    fn deduct_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::deduct_deposit(self, tenant_id, deposit_id, money, audit_ref)
    }

    fn release_deposit(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        audit_ref: &str,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::release_deposit(self, tenant_id, deposit_id, money, audit_ref)
    }

    fn request_refund(
        &self,
        tenant_id: &str,
        deposit_id: &DepositId,
        money: Money,
        idempotency_key: &str,
        reason: &str,
    ) -> Result<RefundId, IntegrationError> {
        IntegrationStore::request_refund(
            self,
            tenant_id,
            deposit_id,
            money,
            idempotency_key,
            reason,
        )
    }

    fn link_refund_operation(
        &self,
        tenant_id: &str,
        refund_id: &RefundId,
        operation_id: &ExternalOperationId,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::link_refund_operation(self, tenant_id, refund_id, operation_id)
    }

    fn reconcile_refund(
        &self,
        tenant_id: &str,
        refund_id: &RefundId,
        outcome: DepositReconciliationOutcome,
        evidence_ref: &str,
        actor_ref: Option<&str>,
    ) -> Result<(), IntegrationError> {
        IntegrationStore::reconcile_refund(
            self,
            tenant_id,
            refund_id,
            outcome,
            evidence_ref,
            actor_ref,
        )
    }
}

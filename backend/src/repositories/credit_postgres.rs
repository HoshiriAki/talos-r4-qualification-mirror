#![cfg(feature = "postgres")]

use serde_json::Value;

use crate::repositories::RepositoryError;
use crate::repositories::credit::{
    BlacklistAddOutcome, BlacklistRemoveOutcome, CreditMutationError, CreditRecalculateOutcome,
    ViolationRecordOutcome, ViolationTransitionOutcome,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresCreditRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresCreditRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::repositories) fn blacklist_add(
        &self,
        customer_name: &str,
        customer_phone: &str,
        id_number: Option<&str>,
        reason: &str,
        severity: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<BlacklistAddOutcome, CreditMutationError> {
        crate::repositories::credit_postgres_mutation::blacklist_add(
            self.session,
            customer_name,
            customer_phone,
            id_number,
            reason,
            severity,
            actor_identity_id,
            now,
        )
    }

    pub(in crate::repositories) fn blacklist_remove(
        &self,
        id: i64,
        removal_reason: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<BlacklistRemoveOutcome, CreditMutationError> {
        crate::repositories::credit_postgres_mutation::blacklist_remove(
            self.session,
            id,
            removal_reason,
            actor_identity_id,
            now,
        )
    }

    pub(in crate::repositories) fn blacklist_check(
        &self,
        customer_phone: &str,
    ) -> Result<Value, RepositoryError> {
        crate::repositories::credit_postgres_read::blacklist_check(self.session, customer_phone)
    }

    pub(in crate::repositories) fn blacklist_list(
        &self,
        is_active: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<Value, RepositoryError> {
        crate::repositories::credit_postgres_read::blacklist_list(
            self.session,
            is_active,
            page,
            page_size,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::repositories) fn violation_record(
        &self,
        customer_name: &str,
        customer_phone: &str,
        order_id: Option<i64>,
        violation_type: &str,
        severity: &str,
        description: &str,
        evidence: Option<&str>,
        financial_penalty: f64,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<ViolationRecordOutcome, CreditMutationError> {
        crate::repositories::credit_postgres_mutation::violation_record(
            self.session,
            customer_name,
            customer_phone,
            order_id,
            violation_type,
            severity,
            description,
            evidence,
            financial_penalty,
            actor_identity_id,
            now,
        )
    }

    pub(in crate::repositories) fn violation_appeal(
        &self,
        id: i64,
        appeal_reason: &str,
        now: &str,
    ) -> Result<ViolationTransitionOutcome, CreditMutationError> {
        crate::repositories::credit_postgres_mutation::violation_appeal(
            self.session,
            id,
            appeal_reason,
            now,
        )
    }

    pub(in crate::repositories) fn violation_review(
        &self,
        id: i64,
        status: &str,
        review_notes: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<ViolationTransitionOutcome, CreditMutationError> {
        crate::repositories::credit_postgres_mutation::violation_review(
            self.session,
            id,
            status,
            review_notes,
            actor_identity_id,
            now,
        )
    }

    pub(in crate::repositories) fn violation_list(
        &self,
        customer_phone: Option<&str>,
        status: Option<&str>,
        violation_type: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Value, RepositoryError> {
        crate::repositories::credit_postgres_read::violation_list(
            self.session,
            customer_phone,
            status,
            violation_type,
            page,
            page_size,
        )
    }

    pub(in crate::repositories) fn violation_get(
        &self,
        id: i64,
    ) -> Result<Option<Value>, RepositoryError> {
        crate::repositories::credit_postgres_read::violation_get(self.session, id)
    }

    pub(in crate::repositories) fn credit_get(
        &self,
        customer_phone: &str,
        now: &str,
    ) -> Result<Value, CreditMutationError> {
        crate::repositories::credit_postgres_mutation::credit_get(self.session, customer_phone, now)
    }

    pub(in crate::repositories) fn credit_history(
        &self,
        customer_phone: &str,
    ) -> Result<Value, RepositoryError> {
        crate::repositories::credit_postgres_read::credit_history(self.session, customer_phone)
    }

    pub(in crate::repositories) fn credit_recalculate(
        &self,
        customer_phone: &str,
        now: &str,
    ) -> Result<CreditRecalculateOutcome, CreditMutationError> {
        crate::repositories::credit_postgres_mutation::credit_recalculate(
            self.session,
            customer_phone,
            now,
        )
    }

    pub(in crate::repositories) fn check_before_order(
        &self,
        customer_phone: &str,
    ) -> Result<Value, RepositoryError> {
        crate::repositories::credit_postgres_read::check_before_order(self.session, customer_phone)
    }
}

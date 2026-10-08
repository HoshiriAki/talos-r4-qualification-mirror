use serde_json::Value;

use crate::repositories::credit::{
    BlacklistAddOutcome, BlacklistRemoveOutcome, CreditMutationError, CreditRecalculateOutcome,
    SqliteCreditRepository, ViolationRecordOutcome, ViolationTransitionOutcome,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::credit_postgres::PostgresCreditRepository;

pub struct ScopedCreditRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedCreditRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn blacklist_add(
        &self,
        customer_name: &str,
        customer_phone: &str,
        id_number: Option<&str>,
        reason: &str,
        severity: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<BlacklistAddOutcome, CreditMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session()).blacklist_add(
                customer_name,
                customer_phone,
                id_number,
                reason,
                severity,
                actor_identity_id,
                now,
            );
        }
        SqliteCreditRepository::new(self.scoped).blacklist_add(
            customer_name,
            customer_phone,
            id_number,
            reason,
            severity,
            actor_identity_id,
            now,
        )
    }

    pub fn blacklist_remove(
        &self,
        id: i64,
        removal_reason: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<BlacklistRemoveOutcome, CreditMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session()).blacklist_remove(
                id,
                removal_reason,
                actor_identity_id,
                now,
            );
        }
        SqliteCreditRepository::new(self.scoped).blacklist_remove(
            id,
            removal_reason,
            actor_identity_id,
            now,
        )
    }

    pub fn blacklist_check(&self, customer_phone: &str) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session())
                .blacklist_check(customer_phone);
        }
        SqliteCreditRepository::new(self.scoped).blacklist_check(customer_phone)
    }

    pub fn blacklist_list(
        &self,
        is_active: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session())
                .blacklist_list(is_active, page, page_size);
        }
        SqliteCreditRepository::new(self.scoped).blacklist_list(is_active, page, page_size)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn violation_record(
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
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session()).violation_record(
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
            );
        }
        SqliteCreditRepository::new(self.scoped).violation_record(
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

    pub fn violation_appeal(
        &self,
        id: i64,
        appeal_reason: &str,
        now: &str,
    ) -> Result<ViolationTransitionOutcome, CreditMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session()).violation_appeal(
                id,
                appeal_reason,
                now,
            );
        }
        SqliteCreditRepository::new(self.scoped).violation_appeal(id, appeal_reason, now)
    }

    pub fn violation_review(
        &self,
        id: i64,
        status: &str,
        review_notes: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<ViolationTransitionOutcome, CreditMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session()).violation_review(
                id,
                status,
                review_notes,
                actor_identity_id,
                now,
            );
        }
        SqliteCreditRepository::new(self.scoped).violation_review(
            id,
            status,
            review_notes,
            actor_identity_id,
            now,
        )
    }

    pub fn violation_list(
        &self,
        customer_phone: Option<&str>,
        status: Option<&str>,
        violation_type: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session()).violation_list(
                customer_phone,
                status,
                violation_type,
                page,
                page_size,
            );
        }
        SqliteCreditRepository::new(self.scoped).violation_list(
            customer_phone,
            status,
            violation_type,
            page,
            page_size,
        )
    }

    pub fn violation_get(&self, id: i64) -> Result<Option<Value>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session()).violation_get(id);
        }
        SqliteCreditRepository::new(self.scoped).violation_get(id)
    }

    pub fn credit_get(
        &self,
        customer_phone: &str,
        now: &str,
    ) -> Result<Value, CreditMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session())
                .credit_get(customer_phone, now);
        }
        SqliteCreditRepository::new(self.scoped).credit_get(customer_phone, now)
    }

    pub fn credit_history(&self, customer_phone: &str) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session())
                .credit_history(customer_phone);
        }
        SqliteCreditRepository::new(self.scoped).credit_history(customer_phone)
    }

    pub fn credit_recalculate(
        &self,
        customer_phone: &str,
        now: &str,
    ) -> Result<CreditRecalculateOutcome, CreditMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session())
                .credit_recalculate(customer_phone, now);
        }
        SqliteCreditRepository::new(self.scoped).credit_recalculate(customer_phone, now)
    }

    pub fn check_before_order(&self, customer_phone: &str) -> Result<Value, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresCreditRepository::new(self.scoped.session())
                .check_before_order(customer_phone);
        }
        SqliteCreditRepository::new(self.scoped).check_before_order(customer_phone)
    }
}

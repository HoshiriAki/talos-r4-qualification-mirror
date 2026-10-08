use crate::repositories::refund::{
    RefundExecuteOutcome, RefundListProjection, RefundMutationError, RefundRequestOutcome,
    RefundStatusOutcome, SqliteRefundRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::refund_postgres::PostgresRefundRepository;

pub struct ScopedRefundRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedRefundRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn request(
        &self,
        order_id: &str,
        amount: f64,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundRequestOutcome, RefundMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRefundRepository::new(self.scoped.session())
                .request(order_id, amount, reason, operator, now);
        }
        SqliteRefundRepository::new(self.scoped).request(order_id, amount, reason, operator, now)
    }

    pub fn approve(
        &self,
        refund_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundStatusOutcome, RefundMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRefundRepository::new(self.scoped.session())
                .approve(refund_id, operator, now);
        }
        SqliteRefundRepository::new(self.scoped).approve(refund_id, operator, now)
    }

    pub fn reject(
        &self,
        refund_id: &str,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundStatusOutcome, RefundMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRefundRepository::new(self.scoped.session())
                .reject(refund_id, reason, operator, now);
        }
        SqliteRefundRepository::new(self.scoped).reject(refund_id, reason, operator, now)
    }

    pub fn execute_refund(
        &self,
        refund_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundExecuteOutcome, RefundMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRefundRepository::new(self.scoped.session())
                .execute(refund_id, operator, now);
        }
        SqliteRefundRepository::new(self.scoped).execute(refund_id, operator, now)
    }

    pub fn list(
        &self,
        order_id: Option<&str>,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<RefundListProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresRefundRepository::new(self.scoped.session())
                .list(order_id, status, page, page_size);
        }
        SqliteRefundRepository::new(self.scoped).list(order_id, status, page, page_size)
    }
}

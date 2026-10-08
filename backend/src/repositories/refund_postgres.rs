#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::refund::{
    RefundExecuteOutcome, RefundListProjection, RefundMutationError, RefundRequestOutcome,
    RefundStatusOutcome,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresRefundRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresRefundRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn request(
        &self,
        order_id: &str,
        amount: f64,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundRequestOutcome, RefundMutationError> {
        crate::repositories::refund_postgres_request::request(
            self.session,
            order_id,
            amount,
            reason,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn approve(
        &self,
        refund_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundStatusOutcome, RefundMutationError> {
        crate::repositories::refund_postgres_transition::approve(
            self.session,
            refund_id,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn reject(
        &self,
        refund_id: &str,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundStatusOutcome, RefundMutationError> {
        crate::repositories::refund_postgres_transition::reject(
            self.session,
            refund_id,
            reason,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn execute(
        &self,
        refund_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundExecuteOutcome, RefundMutationError> {
        crate::repositories::refund_postgres_execute::execute(
            self.session,
            refund_id,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn list(
        &self,
        order_id: Option<&str>,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<RefundListProjection, RepositoryError> {
        crate::repositories::refund_postgres_read::list(
            self.session,
            order_id,
            status,
            page,
            page_size,
        )
    }
}

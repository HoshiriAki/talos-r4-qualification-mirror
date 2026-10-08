#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::deposit::{
    DepositCalculateOutcome, DepositCollectOutcome, DepositDetailProjection, DepositForfeitOutcome,
    DepositMutationError, DepositReleaseOutcome,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresDepositRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresDepositRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn calculate(
        &self,
        order_id: &str,
        default_per_device: f64,
        now: &str,
    ) -> Result<DepositCalculateOutcome, DepositMutationError> {
        crate::repositories::deposit_postgres_calculate::calculate(
            self.session,
            order_id,
            default_per_device,
            now,
        )
    }

    pub(in crate::repositories) fn collect(
        &self,
        order_id: &str,
        amount: f64,
        operator: &str,
        now: &str,
    ) -> Result<DepositCollectOutcome, DepositMutationError> {
        crate::repositories::deposit_postgres_collect::collect(
            self.session,
            order_id,
            amount,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn release(
        &self,
        order_id: &str,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<DepositReleaseOutcome, DepositMutationError> {
        crate::repositories::deposit_postgres_release::release(
            self.session,
            order_id,
            reason,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn forfeit(
        &self,
        order_id: &str,
        amount: f64,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<DepositForfeitOutcome, DepositMutationError> {
        crate::repositories::deposit_postgres_forfeit::forfeit(
            self.session,
            order_id,
            amount,
            reason,
            operator,
            now,
        )
    }

    pub(in crate::repositories) fn get(
        &self,
        order_id: &str,
    ) -> Result<DepositDetailProjection, RepositoryError> {
        crate::repositories::deposit_postgres_read::get(self.session, order_id)
    }
}

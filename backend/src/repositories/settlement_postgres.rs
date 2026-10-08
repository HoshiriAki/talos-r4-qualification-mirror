#![cfg(feature = "postgres")]

use crate::repositories::RepositoryError;
use crate::repositories::session::RepositorySession;
use crate::repositories::settlement::{
    SettlementConfirmOutcome, SettlementGenerateOutcome, SettlementListProjection,
    SettlementMutationError, SettlementProjection, SettlementRevenueDetail,
};

pub(in crate::repositories) struct PostgresSettlementRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresSettlementRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn generate(
        &self,
        period_type: &str,
        period_key: &str,
        date_start: &str,
        date_end: &str,
        now: &str,
    ) -> Result<SettlementGenerateOutcome, SettlementMutationError> {
        crate::repositories::settlement_postgres_mutation::generate(
            self.session,
            period_type,
            period_key,
            date_start,
            date_end,
            now,
        )
    }

    pub(in crate::repositories) fn confirm(
        &self,
        settlement_id: &str,
        now: &str,
    ) -> Result<SettlementConfirmOutcome, SettlementMutationError> {
        crate::repositories::settlement_postgres_mutation::confirm(self.session, settlement_id, now)
    }

    pub(in crate::repositories) fn get_by_period(
        &self,
        period_type: &str,
        period_key: &str,
    ) -> Result<Option<SettlementProjection>, RepositoryError> {
        crate::repositories::settlement_postgres_read::get_by_period(
            self.session,
            period_type,
            period_key,
        )
    }

    pub(in crate::repositories) fn get_by_id(
        &self,
        settlement_id: &str,
    ) -> Result<Option<SettlementProjection>, RepositoryError> {
        crate::repositories::settlement_postgres_read::get_by_id(self.session, settlement_id)
    }

    pub(in crate::repositories) fn list(
        &self,
        period_type: Option<&str>,
        confirmed: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<SettlementListProjection, RepositoryError> {
        crate::repositories::settlement_postgres_read::list(
            self.session,
            period_type,
            confirmed,
            page,
            page_size,
        )
    }

    pub(in crate::repositories) fn revenue_details(
        &self,
        date_start: &str,
        date_end: &str,
    ) -> Result<Vec<SettlementRevenueDetail>, RepositoryError> {
        crate::repositories::settlement_postgres_read::revenue_details(
            self.session,
            date_start,
            date_end,
        )
    }
}

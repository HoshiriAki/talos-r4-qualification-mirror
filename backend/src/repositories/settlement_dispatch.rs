use crate::repositories::settlement::{
    SettlementConfirmOutcome, SettlementGenerateOutcome, SettlementListProjection,
    SettlementMutationError, SettlementProjection, SettlementRevenueDetail,
    SqliteSettlementRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::settlement_postgres::PostgresSettlementRepository;

pub struct ScopedSettlementRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedSettlementRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn generate(
        &self,
        period_type: &str,
        period_key: &str,
        date_start: &str,
        date_end: &str,
        now: &str,
    ) -> Result<SettlementGenerateOutcome, SettlementMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresSettlementRepository::new(self.scoped.session()).generate(
                period_type,
                period_key,
                date_start,
                date_end,
                now,
            );
        }
        SqliteSettlementRepository::new(self.scoped).generate(
            period_type,
            period_key,
            date_start,
            date_end,
            now,
        )
    }

    pub fn confirm(
        &self,
        settlement_id: &str,
        now: &str,
    ) -> Result<SettlementConfirmOutcome, SettlementMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresSettlementRepository::new(self.scoped.session())
                .confirm(settlement_id, now);
        }
        SqliteSettlementRepository::new(self.scoped).confirm(settlement_id, now)
    }

    pub fn get_by_period(
        &self,
        period_type: &str,
        period_key: &str,
    ) -> Result<Option<SettlementProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresSettlementRepository::new(self.scoped.session())
                .get_by_period(period_type, period_key);
        }
        SqliteSettlementRepository::new(self.scoped).get_by_period(period_type, period_key)
    }

    pub fn get_by_id(
        &self,
        settlement_id: &str,
    ) -> Result<Option<SettlementProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresSettlementRepository::new(self.scoped.session())
                .get_by_id(settlement_id);
        }
        SqliteSettlementRepository::new(self.scoped).get_by_id(settlement_id)
    }

    pub fn list(
        &self,
        period_type: Option<&str>,
        confirmed: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<SettlementListProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresSettlementRepository::new(self.scoped.session()).list(
                period_type,
                confirmed,
                page,
                page_size,
            );
        }
        SqliteSettlementRepository::new(self.scoped).list(period_type, confirmed, page, page_size)
    }

    pub fn revenue_details(
        &self,
        date_start: &str,
        date_end: &str,
    ) -> Result<Vec<SettlementRevenueDetail>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresSettlementRepository::new(self.scoped.session())
                .revenue_details(date_start, date_end);
        }
        SqliteSettlementRepository::new(self.scoped).revenue_details(date_start, date_end)
    }
}

use crate::repositories::depreciation::{
    DepreciationLogProjection, DepreciationMutationError, DepreciationSnapshot,
    MonthlyDepreciationRun, SqliteDepreciationRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::depreciation_postgres::PostgresDepreciationRepository;

pub struct ScopedDepreciationRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedDepreciationRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn snapshot(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<DepreciationSnapshot>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDepreciationRepository::new(self.scoped.session())
                .snapshot(device_serial_no);
        }
        SqliteDepreciationRepository::new(self.scoped).snapshot(device_serial_no)
    }

    pub fn logs(
        &self,
        device_serial_no: &str,
    ) -> Result<Vec<DepreciationLogProjection>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDepreciationRepository::new(self.scoped.session())
                .logs(device_serial_no);
        }
        SqliteDepreciationRepository::new(self.scoped).logs(device_serial_no)
    }

    pub fn run_monthly(
        &self,
        period: &str,
        useful_life_months: i32,
        now: &str,
    ) -> Result<MonthlyDepreciationRun, DepreciationMutationError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDepreciationRepository::new(self.scoped.session()).run_monthly(
                period,
                useful_life_months,
                now,
            );
        }
        SqliteDepreciationRepository::new(self.scoped).run_monthly(period, useful_life_months, now)
    }
}

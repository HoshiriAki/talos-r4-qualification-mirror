use crate::repositories::dashboard_read::{
    DashboardCountPoint, DashboardModelRankingPoint, DashboardOrderBucket, DashboardOrderWindow,
    DashboardOverviewProjection, DashboardProvincePiePoint, DashboardProvinceTrendPoint,
    DashboardRevenuePoint, DashboardWarehousePoint, SqliteDashboardReadRepository,
};
use crate::repositories::{RepositoryError, ScopedRepositories};

#[cfg(feature = "postgres")]
use crate::repositories::dashboard_read_postgres::PostgresDashboardReadRepository;

pub struct ScopedDashboardReadRepository<'a> {
    scoped: &'a ScopedRepositories,
}

impl<'a> ScopedDashboardReadRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self { scoped }
    }

    pub fn overview(
        &self,
        today: &str,
        overdue_cutoff: &str,
    ) -> Result<DashboardOverviewProjection, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session())
                .overview(today, overdue_cutoff);
        }
        SqliteDashboardReadRepository::new(self.scoped).overview(today, overdue_cutoff)
    }

    pub fn order_windows(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<DashboardOrderWindow>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session())
                .order_windows(start_date, end_date);
        }
        SqliteDashboardReadRepository::new(self.scoped).order_windows(start_date, end_date)
    }

    pub fn order_daily(&self, cutoff: &str) -> Result<Vec<DashboardCountPoint>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session()).order_daily(cutoff);
        }
        SqliteDashboardReadRepository::new(self.scoped).order_daily(cutoff)
    }

    pub fn revenue_daily(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardRevenuePoint>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session())
                .revenue_daily(cutoff);
        }
        SqliteDashboardReadRepository::new(self.scoped).revenue_daily(cutoff)
    }

    pub fn cancel_daily(&self, cutoff: &str) -> Result<Vec<DashboardCountPoint>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session())
                .cancel_daily(cutoff);
        }
        SqliteDashboardReadRepository::new(self.scoped).cancel_daily(cutoff)
    }

    pub fn device_status_distribution(&self) -> Result<Vec<DashboardOrderBucket>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session())
                .device_status_distribution();
        }
        SqliteDashboardReadRepository::new(self.scoped).device_status_distribution()
    }

    pub fn province_pie(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardProvincePiePoint>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session())
                .province_pie(cutoff);
        }
        SqliteDashboardReadRepository::new(self.scoped).province_pie(cutoff)
    }

    pub fn province_trend(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardProvinceTrendPoint>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session())
                .province_trend(cutoff);
        }
        SqliteDashboardReadRepository::new(self.scoped).province_trend(cutoff)
    }

    pub fn model_ranking(
        &self,
        cutoff: &str,
        limit: i64,
    ) -> Result<Vec<DashboardModelRankingPoint>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session())
                .model_ranking(cutoff, limit);
        }
        SqliteDashboardReadRepository::new(self.scoped).model_ranking(cutoff, limit)
    }

    pub fn warehouse_stats(&self) -> Result<Vec<DashboardWarehousePoint>, RepositoryError> {
        #[cfg(feature = "postgres")]
        if self.scoped.session().is_postgres() {
            return PostgresDashboardReadRepository::new(self.scoped.session()).warehouse_stats();
        }
        SqliteDashboardReadRepository::new(self.scoped).warehouse_stats()
    }
}

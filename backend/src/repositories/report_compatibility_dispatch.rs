use official_order::report::RevenueData;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use super::RepositoryError;
use super::report_compatibility::SqliteReportCompatibilityRepository;
#[cfg(feature = "postgres")]
use super::report_compatibility_postgres::PostgresReportCompatibilityRepository;

#[derive(Clone)]
enum ReportCompatibilityBackend {
    Sqlite(SqliteReportCompatibilityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresReportCompatibilityRepository),
}

#[derive(Clone)]
pub(crate) struct ReportCompatibilityRepository {
    backend: ReportCompatibilityBackend,
}

impl ReportCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: ReportCompatibilityBackend::Sqlite(SqliteReportCompatibilityRepository::new(
                pool,
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: ReportCompatibilityBackend::Postgres(
                PostgresReportCompatibilityRepository::new(pool),
            ),
        }
    }

    pub(crate) fn revenue_data(
        &self,
        tenant_id: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<RevenueData, RepositoryError> {
        match &self.backend {
            ReportCompatibilityBackend::Sqlite(repository) => {
                repository.revenue_data(tenant_id, start_date, end_date)
            }
            #[cfg(feature = "postgres")]
            ReportCompatibilityBackend::Postgres(repository) => {
                repository.revenue_data(tenant_id, start_date, end_date)
            }
        }
    }
}

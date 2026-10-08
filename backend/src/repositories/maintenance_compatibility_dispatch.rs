use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use crate::error::AppError;

use super::maintenance_compatibility::{
    MaintenanceStatusSync, SqliteMaintenanceCompatibilityRepository,
};
#[cfg(feature = "postgres")]
use super::maintenance_compatibility_postgres::PostgresMaintenanceCompatibilityRepository;

#[derive(Clone)]
enum MaintenanceCompatibilityBackend {
    Sqlite(SqliteMaintenanceCompatibilityRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresMaintenanceCompatibilityRepository),
}

#[derive(Clone)]
pub(crate) struct MaintenanceCompatibilityRepository {
    backend: MaintenanceCompatibilityBackend,
}

impl MaintenanceCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: MaintenanceCompatibilityBackend::Sqlite(
                SqliteMaintenanceCompatibilityRepository::new(pool),
            ),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: MaintenanceCompatibilityBackend::Postgres(
                PostgresMaintenanceCompatibilityRepository::new(pool),
            ),
        }
    }

    pub(crate) fn sync_order_statuses(
        &self,
        today: &str,
    ) -> Result<MaintenanceStatusSync, AppError> {
        match &self.backend {
            MaintenanceCompatibilityBackend::Sqlite(repository) => {
                repository.sync_order_statuses(today)
            }
            #[cfg(feature = "postgres")]
            MaintenanceCompatibilityBackend::Postgres(repository) => {
                repository.sync_order_statuses(today)
            }
        }
    }

    pub(crate) fn seed_overdue_tasks(&self, today: &str, now: &str) -> Result<usize, AppError> {
        match &self.backend {
            MaintenanceCompatibilityBackend::Sqlite(repository) => {
                repository.seed_overdue_tasks(today, now)
            }
            #[cfg(feature = "postgres")]
            MaintenanceCompatibilityBackend::Postgres(repository) => {
                repository.seed_overdue_tasks(today, now)
            }
        }
    }
}

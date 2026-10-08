use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use serde_json::Value;

use super::RepositoryError;
use super::user_settings_profile::SqliteUserSettingsProfileRepository;
#[cfg(feature = "postgres")]
use super::user_settings_profile_postgres::PostgresUserSettingsProfileRepository;

#[derive(Clone)]
enum UserSettingsProfileBackend {
    Sqlite(SqliteUserSettingsProfileRepository),
    #[cfg(feature = "postgres")]
    Postgres(PostgresUserSettingsProfileRepository),
}

#[derive(Clone)]
pub(crate) struct UserSettingsProfileRepository {
    backend: UserSettingsProfileBackend,
}

impl UserSettingsProfileRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: UserSettingsProfileBackend::Sqlite(SqliteUserSettingsProfileRepository::new(
                pool,
            )),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: UserSettingsProfileBackend::Postgres(
                PostgresUserSettingsProfileRepository::new(pool),
            ),
        }
    }

    pub(crate) fn get(&self, user_id: &str) -> Result<Value, RepositoryError> {
        match &self.backend {
            UserSettingsProfileBackend::Sqlite(repository) => repository.get(user_id),
            #[cfg(feature = "postgres")]
            UserSettingsProfileBackend::Postgres(repository) => repository.get(user_id),
        }
    }

    pub(crate) fn save(
        &self,
        user_id: &str,
        settings: &Value,
        now: &str,
    ) -> Result<(), RepositoryError> {
        match &self.backend {
            UserSettingsProfileBackend::Sqlite(repository) => {
                repository.save(user_id, settings, now)
            }
            #[cfg(feature = "postgres")]
            UserSettingsProfileBackend::Postgres(repository) => {
                repository.save(user_id, settings, now)
            }
        }
    }
}

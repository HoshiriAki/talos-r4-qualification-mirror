use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OptionalExtension;
use serde_json::Value;

use super::RepositoryError;

#[derive(Clone)]
pub(crate) struct SqliteUserSettingsProfileRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteUserSettingsProfileRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn get(&self, user_id: &str) -> Result<Value, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let raw = conn
            .query_row(
                "SELECT settingsJson FROM user_settings WHERE userId = ?1",
                [user_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_storage)?;
        Ok(raw
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({})))
    }

    pub(crate) fn save(
        &self,
        user_id: &str,
        settings: &Value,
        now: &str,
    ) -> Result<(), RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let json = serde_json::to_string(settings)
            .map_err(|error| RepositoryError::ContractViolation(error.to_string()))?;
        conn.execute(
            "INSERT INTO user_settings (userId, settingsJson, updatedAt)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(userId) DO UPDATE SET
               settingsJson = excluded.settingsJson,
               updatedAt = excluded.updatedAt",
            rusqlite::params![user_id, json, now],
        )
        .map_err(sqlite_storage)?;
        Ok(())
    }
}

fn sqlite_storage(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

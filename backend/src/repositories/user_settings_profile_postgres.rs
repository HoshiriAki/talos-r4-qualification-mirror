#![cfg(feature = "postgres")]

use std::future::Future;

use serde_json::Value;
use sqlx::{PgPool, Row};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::RepositoryError;

#[derive(Clone)]
pub(crate) struct PostgresUserSettingsProfileRepository {
    pool: PgPool,
}

impl PostgresUserSettingsProfileRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn get(&self, user_id: &str) -> Result<Value, RepositoryError> {
        let pool = self.pool.clone();
        let user_id = user_id.to_owned();
        run_pg_profile(async move {
            let row = sqlx::query(
                "SELECT settingsJson::text AS settings_json
                 FROM user_settings
                 WHERE userId = $1",
            )
            .bind(user_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_storage)?;
            let Some(row) = row else {
                return Ok(serde_json::json!({}));
            };
            let raw: String = row.try_get("settings_json").map_err(pg_storage)?;
            Ok(serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({})))
        })
    }

    pub(crate) fn save(
        &self,
        user_id: &str,
        settings: &Value,
        now: &str,
    ) -> Result<(), RepositoryError> {
        let pool = self.pool.clone();
        let user_id = user_id.to_owned();
        let settings = serde_json::to_string(settings)
            .map_err(|error| RepositoryError::ContractViolation(error.to_string()))?;
        let now = now.to_owned();
        run_pg_profile(async move {
            sqlx::query(
                "INSERT INTO user_settings (userId, settingsJson, updatedAt)
                 VALUES ($1, $2::jsonb, $3)
                 ON CONFLICT (userId) DO UPDATE SET
                   settingsJson = excluded.settingsJson,
                   updatedAt = excluded.updatedAt",
            )
            .bind(user_id)
            .bind(settings)
            .bind(now)
            .execute(&pool)
            .await
            .map_err(pg_storage)?;
            Ok(())
        })
    }
}

fn pg_storage(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn run_pg_profile<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("user settings runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "user settings requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{TransactionBehavior, params};
use uuid::Uuid;

use super::RepositoryError;

#[derive(Debug, Clone)]
pub(crate) struct BootstrapPlatformOwnerCommand {
    pub username: String,
    pub password_hash: String,
    pub now: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapPlatformOwnerOutcome {
    Created,
    AlreadyInitialized,
}

#[derive(Clone)]
pub(crate) struct SqliteBootstrapAuthorityRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteBootstrapAuthorityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn has_identities(&self) -> Result<bool, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let count: i64 = conn
            .query_row("SELECT COUNT(1) FROM identities", [], |row| row.get(0))
            .map_err(sqlite_storage)?;
        Ok(count > 0)
    }

    pub(crate) fn ensure_initial_platform_owner(
        &self,
        command: BootstrapPlatformOwnerCommand,
    ) -> Result<BootstrapPlatformOwnerOutcome, RepositoryError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;

        let count: i64 = tx
            .query_row("SELECT COUNT(1) FROM identities", [], |row| row.get(0))
            .map_err(sqlite_storage)?;
        if count > 0 {
            tx.commit().map_err(sqlite_storage)?;
            return Ok(BootstrapPlatformOwnerOutcome::AlreadyInitialized);
        }

        let identity_id = Uuid::new_v4().to_string();
        let membership_id = Uuid::new_v4().to_string();
        let grant_id = Uuid::new_v4().to_string();

        tx.execute(
            "INSERT INTO identities
             (id, username, password_hash, display_name, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?2, 'active', ?4, ?4)",
            params![
                identity_id.as_str(),
                command.username.as_str(),
                command.password_hash.as_str(),
                command.now.as_str(),
            ],
        )
        .map_err(sqlite_storage)?;
        tx.execute(
            "INSERT INTO platform_memberships
             (id, identity_id, status, created_at, updated_at)
             VALUES (?1, ?2, 'active', ?3, ?3)",
            params![
                membership_id.as_str(),
                identity_id.as_str(),
                command.now.as_str(),
            ],
        )
        .map_err(sqlite_storage)?;
        tx.execute(
            "INSERT INTO platform_role_grants
             (id, platform_membership_id, role, granted_by_identity_id, granted_at)
             VALUES (?1, ?2, 'platform_owner', ?3, ?4)",
            params![
                grant_id.as_str(),
                membership_id.as_str(),
                identity_id.as_str(),
                command.now.as_str(),
            ],
        )
        .map_err(sqlite_storage)?;
        tx.commit().map_err(sqlite_storage)?;

        Ok(BootstrapPlatformOwnerOutcome::Created)
    }
}

fn sqlite_storage(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

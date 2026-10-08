use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use system_admin::deletion::{DeletionAdminInput, DeletionListInput, DeletionRequestInput};
use uuid::Uuid;

use super::RepositoryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeletionRequestOutcome {
    Created { id: String, requested_at: String },
    Existing { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeletionRecord {
    pub id: String,
    pub user_id: String,
    pub request_type: String,
    pub status: String,
    pub reason: String,
    pub requested_at: String,
    pub completed_at: Option<String>,
    pub admin_notes: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeletionCompleteResult {
    pub completed_at: String,
    pub identity_id: String,
    pub identity_anonymized: bool,
}

#[derive(Debug)]
pub(crate) enum DeletionCompatibilityError {
    Storage(RepositoryError),
    LastOwner,
    MembershipNotFound,
}

impl From<RepositoryError> for DeletionCompatibilityError {
    fn from(error: RepositoryError) -> Self {
        Self::Storage(error)
    }
}

impl std::fmt::Display for DeletionCompatibilityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "{error}"),
            Self::LastOwner => formatter.write_str("cannot delete the last active tenant owner"),
            Self::MembershipNotFound => formatter.write_str("tenant membership not found"),
        }
    }
}

impl std::error::Error for DeletionCompatibilityError {}

#[derive(Clone)]
pub(crate) struct SqliteDeletionCompatibilityRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteDeletionCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn request(
        &self,
        tenant_id: &str,
        input: &DeletionRequestInput,
        now: &str,
    ) -> Result<DeletionRequestOutcome, DeletionCompatibilityError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        let existing = tx
            .query_row(
                "SELECT id FROM data_deletion_requests
                 WHERE tenant_id = ?1 AND user_id = ?2
                   AND status IN ('pending', 'processing')
                 LIMIT 1",
                params![tenant_id, input.user_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_storage)?;
        if let Some(id) = existing {
            tx.commit().map_err(sqlite_storage)?;
            return Ok(DeletionRequestOutcome::Existing { id });
        }

        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO data_deletion_requests
             (id, user_id, request_type, status, reason, requested_at, created_at, tenant_id)
             VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?5, ?6)",
            params![
                id.as_str(),
                input.user_id.as_str(),
                input.request_type.as_str(),
                input.reason.as_str(),
                now,
                tenant_id,
            ],
        )
        .map_err(sqlite_storage)?;
        tx.commit().map_err(sqlite_storage)?;
        Ok(DeletionRequestOutcome::Created {
            id,
            requested_at: now.to_owned(),
        })
    }

    pub(crate) fn list(
        &self,
        tenant_id: &str,
        input: &DeletionListInput,
    ) -> Result<Vec<DeletionRecord>, DeletionCompatibilityError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let sql = if input.status.is_some() {
            "SELECT id, user_id, request_type, status, reason, requested_at,
                    completed_at, admin_notes, created_at
             FROM data_deletion_requests
             WHERE tenant_id = ?1 AND status = ?2
             ORDER BY created_at DESC"
        } else {
            "SELECT id, user_id, request_type, status, reason, requested_at,
                    completed_at, admin_notes, created_at
             FROM data_deletion_requests
             WHERE tenant_id = ?1
             ORDER BY created_at DESC"
        };
        let mut stmt = conn.prepare(sql).map_err(sqlite_storage)?;
        let rows = match input.status.as_deref() {
            Some(status) => stmt.query_map(params![tenant_id, status], map_sqlite_record),
            None => stmt.query_map(params![tenant_id], map_sqlite_record),
        }
        .map_err(sqlite_storage)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_storage)
            .map_err(DeletionCompatibilityError::from)
    }

    pub(crate) fn process(
        &self,
        tenant_id: &str,
        input: &DeletionAdminInput,
    ) -> Result<(), DeletionCompatibilityError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        conn.execute(
            "UPDATE data_deletion_requests
             SET status = 'processing', admin_notes = ?1
             WHERE tenant_id = ?2 AND id = ?3",
            params![
                input.admin_notes.as_str(),
                tenant_id,
                input.request_id.as_str()
            ],
        )
        .map_err(sqlite_storage)?;
        Ok(())
    }

    pub(crate) fn reject(
        &self,
        tenant_id: &str,
        input: &DeletionAdminInput,
        now: &str,
    ) -> Result<(), DeletionCompatibilityError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        conn.execute(
            "UPDATE data_deletion_requests
             SET status = 'rejected', completed_at = ?1, admin_notes = ?2
             WHERE tenant_id = ?3 AND id = ?4",
            params![
                now,
                input.admin_notes.as_str(),
                tenant_id,
                input.request_id.as_str()
            ],
        )
        .map_err(sqlite_storage)?;
        Ok(())
    }

    pub(crate) fn complete(
        &self,
        tenant_id: &str,
        input: &DeletionAdminInput,
        now: &str,
    ) -> Result<DeletionCompleteResult, DeletionCompatibilityError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_storage)?;
        let identity_id: String = tx
            .query_row(
                "SELECT user_id FROM data_deletion_requests
                 WHERE tenant_id = ?1 AND id = ?2",
                params![tenant_id, input.request_id.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_storage)?;
        let target_role: String = tx
            .query_row(
                "SELECT role FROM tenant_memberships
                 WHERE tenant_id = ?1 AND identity_id = ?2 AND status != 'revoked'",
                params![tenant_id, identity_id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_storage)?
            .ok_or(DeletionCompatibilityError::MembershipNotFound)?;
        if target_role == "owner" {
            let owner_count: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM tenant_memberships
                     WHERE tenant_id = ?1 AND role = 'owner' AND status = 'active'",
                    [tenant_id],
                    |row| row.get(0),
                )
                .map_err(sqlite_storage)?;
            if owner_count <= 1 {
                return Err(DeletionCompatibilityError::LastOwner);
            }
        }

        let revoked = tx
            .execute(
                "UPDATE tenant_memberships
                 SET status = 'revoked', updated_at = ?1
                 WHERE tenant_id = ?2 AND identity_id = ?3 AND status != 'revoked'",
                params![now, tenant_id, identity_id.as_str()],
            )
            .map_err(sqlite_storage)?;
        if revoked == 0 {
            return Err(DeletionCompatibilityError::MembershipNotFound);
        }

        let anon_id = format!("ANONYMIZED-{}", Uuid::new_v4());
        let anon_display = format!("已删除用户-{}", &Uuid::new_v4().to_string()[..8]);
        let deleted_email = format!("{anon_id}@deleted.local");
        let identity_anonymized = tx
            .execute(
                "UPDATE identities
                 SET username = ?1, display_name = ?2, email = ?3, phone = NULL,
                     password_hash = 'deleted', totp_secret_ciphertext = NULL, totp_enabled = 0,
                     status = 'disabled', updated_at = ?4
                 WHERE id = ?5
                   AND NOT EXISTS (
                     SELECT 1 FROM tenant_memberships tm
                     WHERE tm.identity_id = identities.id AND tm.status != 'revoked'
                   )
                   AND NOT EXISTS (
                     SELECT 1 FROM platform_memberships pm
                     WHERE pm.identity_id = identities.id AND pm.status != 'revoked'
                   )",
                params![
                    anon_id.as_str(),
                    anon_display.as_str(),
                    deleted_email.as_str(),
                    now,
                    identity_id.as_str(),
                ],
            )
            .map_err(sqlite_storage)?
            == 1;
        tx.execute(
            "UPDATE data_deletion_requests
             SET status = 'completed', completed_at = ?1, admin_notes = ?2
             WHERE tenant_id = ?3 AND id = ?4",
            params![
                now,
                input.admin_notes.as_str(),
                tenant_id,
                input.request_id.as_str()
            ],
        )
        .map_err(sqlite_storage)?;
        tx.commit().map_err(sqlite_storage)?;

        Ok(DeletionCompleteResult {
            completed_at: now.to_owned(),
            identity_id,
            identity_anonymized,
        })
    }
}

fn map_sqlite_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeletionRecord> {
    Ok(DeletionRecord {
        id: row.get(0)?,
        user_id: row.get(1)?,
        request_type: row.get(2)?,
        status: row.get(3)?,
        reason: row.get(4)?,
        requested_at: row.get(5)?,
        completed_at: row.get(6)?,
        admin_notes: row.get(7)?,
        created_at: row.get(8)?,
    })
}

fn sqlite_storage(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

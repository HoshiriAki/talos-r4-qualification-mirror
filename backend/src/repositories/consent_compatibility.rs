use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use system_admin::consent::{ConsentCheckInput, ConsentRecordInput, ConsentRevokeInput};
use uuid::Uuid;

use super::RepositoryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsentRecordResult {
    pub id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsentRevokeResult {
    pub affected: usize,
    pub revoked_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConsentAuditRecord {
    pub id: String,
    pub user_id: String,
    pub consent_type: String,
    pub version: String,
    pub consented: bool,
    pub ip_address: String,
    pub user_agent: String,
    pub revoked_at: Option<String>,
    pub created_at: String,
}

#[derive(Clone)]
pub(crate) struct SqliteConsentCompatibilityRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteConsentCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn record(
        &self,
        tenant_id: &str,
        input: &ConsentRecordInput,
        now: &str,
    ) -> Result<ConsentRecordResult, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO privacy_consents
             (id, user_id, consent_type, version, consented, ip_address, user_agent, created_at, tenant_id)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8)",
            params![
                id.as_str(),
                input.user_id.as_str(),
                input.consent_type.as_str(),
                input.version.as_str(),
                input.ip_address.as_str(),
                input.user_agent.as_str(),
                now,
                tenant_id,
            ],
        )
        .map_err(sqlite_storage)?;
        Ok(ConsentRecordResult {
            id,
            created_at: now.to_owned(),
        })
    }

    pub(crate) fn check(
        &self,
        tenant_id: &str,
        input: &ConsentCheckInput,
    ) -> Result<bool, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM privacy_consents
                WHERE tenant_id = ?1 AND user_id = ?2 AND consent_type = ?3
                  AND version = ?4 AND consented = 1 AND revoked_at IS NULL
                ORDER BY created_at DESC LIMIT 1
            )",
            params![
                tenant_id,
                input.user_id.as_str(),
                input.consent_type.as_str(),
                input.version.as_str(),
            ],
            |row| row.get(0),
        )
        .map_err(sqlite_storage)
    }

    pub(crate) fn revoke(
        &self,
        tenant_id: &str,
        input: &ConsentRevokeInput,
        now: &str,
    ) -> Result<ConsentRevokeResult, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let affected = conn
            .execute(
                "UPDATE privacy_consents
                 SET consented = 0, revoked_at = ?1
                 WHERE tenant_id = ?2 AND user_id = ?3 AND consent_type = ?4
                   AND revoked_at IS NULL",
                params![
                    now,
                    tenant_id,
                    input.user_id.as_str(),
                    input.consent_type.as_str(),
                ],
            )
            .map_err(sqlite_storage)?;
        Ok(ConsentRevokeResult {
            affected,
            revoked_at: now.to_owned(),
        })
    }

    pub(crate) fn audit(
        &self,
        tenant_id: &str,
        user_id: &str,
    ) -> Result<Vec<ConsentAuditRecord>, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, consent_type, version, consented, ip_address,
                        user_agent, revoked_at, created_at
                 FROM privacy_consents
                 WHERE tenant_id = ?1 AND user_id = ?2
                 ORDER BY created_at DESC",
            )
            .map_err(sqlite_storage)?;
        stmt.query_map(params![tenant_id, user_id], |row| {
            Ok(ConsentAuditRecord {
                id: row.get(0)?,
                user_id: row.get(1)?,
                consent_type: row.get(2)?,
                version: row.get(3)?,
                consented: row.get::<_, i32>(4)? == 1,
                ip_address: row.get(5)?,
                user_agent: row.get(6)?,
                revoked_at: row.get(7)?,
                created_at: row.get(8)?,
            })
        })
        .map_err(sqlite_storage)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_storage)
    }
}

fn sqlite_storage(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::RepositoryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewSessionCreate {
    pub id: String,
    pub actor_id: String,
    pub target_tenant_id: String,
    pub mode: String,
    pub simulation_id: Option<String>,
    pub capabilities: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewSessionProjection {
    pub id: String,
    #[serde(skip_serializing)]
    pub actor_id: String,
    #[serde(rename = "tenantId")]
    pub target_tenant_id: String,
    pub tenant_name: String,
    pub tenant_slug: String,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub ended_at: Option<String>,
    pub mode: String,
    pub simulation_id: Option<String>,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewDashboardProjection {
    pub orders: i64,
    pub active_orders: i64,
    pub devices: i64,
    pub available_devices: i64,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PreviewMutationError {
    #[error("active tenant not found")]
    TenantNotFound,
    #[error("active actor-owned simulation not found")]
    SimulationNotFound,
    #[error("preview session not found")]
    NotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

#[derive(Clone)]
pub(crate) struct SqliteTenantPreviewRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteTenantPreviewRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn create_session(
        &self,
        command: PreviewSessionCreate,
    ) -> Result<PreviewSessionProjection, PreviewMutationError> {
        let mut conn = self.connection().map_err(PreviewMutationError::Storage)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sqlite_mutation)?;

        let tenant = tx
            .query_row(
                "SELECT name,slug FROM tenants WHERE id=?1 AND status='active'",
                [&command.target_tenant_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(sqlite_mutation)?
            .ok_or(PreviewMutationError::TenantNotFound)?;

        if command.mode == "simulation" {
            let simulation_id = command
                .simulation_id
                .as_deref()
                .ok_or(PreviewMutationError::SimulationNotFound)?;
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(
                       SELECT 1 FROM simulation_sessions
                       WHERE id=?1 AND actor_id=?2 AND target_tenant_id=?3
                         AND status='active'
                     )",
                    params![simulation_id, command.actor_id, command.target_tenant_id],
                    |row| row.get(0),
                )
                .map_err(sqlite_mutation)?;
            if !exists {
                return Err(PreviewMutationError::SimulationNotFound);
            }
        }

        let capabilities_json = serde_json::to_string(&command.capabilities).map_err(|error| {
            PreviewMutationError::Storage(RepositoryError::ContractViolation(format!(
                "preview capabilities serialization failed: {error}"
            )))
        })?;

        tx.execute(
            "INSERT INTO tenant_preview_sessions
             (id,actor_id,target_tenant_id,status,created_at,expires_at,ended_at)
             VALUES (?1,?2,?3,'active',?4,?5,NULL)",
            params![
                command.id,
                command.actor_id,
                command.target_tenant_id,
                command.created_at,
                command.expires_at
            ],
        )
        .map_err(sqlite_mutation)?;

        tx.execute(
            "INSERT INTO tenant_workspace_sessions
             (id,actor_identity_id,tenant_id,mode,preview_session_id,simulation_id,
              capabilities_json,status,created_at,expires_at,ended_at)
             VALUES (?1,?2,?3,?4,?1,?5,?6,'active',?7,?8,NULL)",
            params![
                command.id,
                command.actor_id,
                command.target_tenant_id,
                command.mode,
                command.simulation_id,
                capabilities_json,
                command.created_at,
                command.expires_at
            ],
        )
        .map_err(sqlite_mutation)?;

        tx.commit().map_err(sqlite_mutation)?;

        Ok(PreviewSessionProjection {
            id: command.id,
            actor_id: command.actor_id,
            target_tenant_id: command.target_tenant_id,
            tenant_name: tenant.0,
            tenant_slug: tenant.1,
            status: "active".into(),
            created_at: command.created_at,
            expires_at: command.expires_at,
            ended_at: None,
            mode: command.mode,
            simulation_id: command.simulation_id,
            capabilities: command.capabilities,
        })
    }

    pub(crate) fn load_owned(
        &self,
        id: &str,
        actor_id: &str,
    ) -> Result<Option<PreviewSessionProjection>, RepositoryError> {
        let conn = self.connection()?;
        conn.query_row(
            "SELECT s.id,s.actor_id,s.target_tenant_id,t.name,t.slug,
                    s.status,s.created_at,s.expires_at,s.ended_at,
                    w.mode,w.simulation_id,w.capabilities_json
             FROM tenant_preview_sessions s
             JOIN tenants t ON t.id=s.target_tenant_id
             JOIN tenant_workspace_sessions w ON w.preview_session_id=s.id
             WHERE s.id=?1 AND s.actor_id=?2",
            params![id, actor_id],
            map_session,
        )
        .optional()
        .map_err(sqlite_storage)
    }

    pub(crate) fn end_session(
        &self,
        id: &str,
        actor_id: &str,
        ended_at: &str,
    ) -> Result<PreviewSessionProjection, PreviewMutationError> {
        let mut conn = self.connection().map_err(PreviewMutationError::Storage)?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(sqlite_mutation)?;

        let owned: bool = tx
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM tenant_preview_sessions WHERE id=?1 AND actor_id=?2
                 )",
                params![id, actor_id],
                |row| row.get(0),
            )
            .map_err(sqlite_mutation)?;
        if !owned {
            return Err(PreviewMutationError::NotFound);
        }

        tx.execute(
            "UPDATE tenant_preview_sessions
             SET status='ended',ended_at=COALESCE(ended_at,?3)
             WHERE id=?1 AND actor_id=?2 AND status='active'",
            params![id, actor_id, ended_at],
        )
        .map_err(sqlite_mutation)?;
        tx.execute(
            "UPDATE tenant_workspace_sessions
             SET status='ended',ended_at=COALESCE(ended_at,?3)
             WHERE id=?1 AND actor_identity_id=?2 AND status='active'",
            params![id, actor_id, ended_at],
        )
        .map_err(sqlite_mutation)?;
        tx.commit().map_err(sqlite_mutation)?;
        drop(conn);

        self.load_owned(id, actor_id)
            .map_err(PreviewMutationError::Storage)?
            .ok_or(PreviewMutationError::NotFound)
    }

    pub(crate) fn tenant_is_active(&self, tenant_id: &str) -> Result<bool, RepositoryError> {
        let conn = self.connection()?;
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tenants WHERE id=?1 AND status='active')",
            [tenant_id],
            |row| row.get(0),
        )
        .map_err(sqlite_storage)
    }

    pub(crate) fn dashboard_summary(
        &self,
        tenant_id: &str,
    ) -> Result<PreviewDashboardProjection, RepositoryError> {
        let conn = self.connection()?;
        let orders: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM orders WHERE tenant_id=?1",
                [tenant_id],
                |row| row.get(0),
            )
            .map_err(sqlite_storage)?;
        let active_orders: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM orders
                 WHERE tenant_id=?1 AND status NOT IN ('completed','closed','cancelled')",
                [tenant_id],
                |row| row.get(0),
            )
            .map_err(sqlite_storage)?;
        let devices: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM devices WHERE tenant_id=?1",
                [tenant_id],
                |row| row.get(0),
            )
            .map_err(sqlite_storage)?;
        let available_devices: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM devices
                 WHERE tenant_id=?1 AND rentalStatus IN ('available','idle','已入库')",
                [tenant_id],
                |row| row.get(0),
            )
            .map_err(sqlite_storage)?;

        Ok(PreviewDashboardProjection {
            orders,
            active_orders,
            devices,
            available_devices,
        })
    }

    fn connection(
        &self,
    ) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, RepositoryError> {
        self.pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))
    }
}

fn map_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<PreviewSessionProjection> {
    let capabilities_json: String = row.get(11)?;
    Ok(PreviewSessionProjection {
        id: row.get(0)?,
        actor_id: row.get(1)?,
        target_tenant_id: row.get(2)?,
        tenant_name: row.get(3)?,
        tenant_slug: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        expires_at: row.get(7)?,
        ended_at: row.get(8)?,
        mode: row.get(9)?,
        simulation_id: row.get(10)?,
        capabilities: serde_json::from_str(&capabilities_json).unwrap_or_default(),
    })
}

fn sqlite_storage(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn sqlite_mutation(error: rusqlite::Error) -> PreviewMutationError {
    PreviewMutationError::Storage(sqlite_storage(error))
}

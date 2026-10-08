use rusqlite::{OptionalExtension, params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::repositories::{RepositoryError, RepositorySession, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkTaskListRequest {
    pub statuses: Vec<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTaskProjection {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub risk: String,
    pub source_type: String,
    pub source_id: String,
    pub title: String,
    pub summary: String,
    pub reason: String,
    pub due_at: Option<String>,
    pub capabilities: Vec<String>,
    pub work_route: Value,
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkTaskState {
    pub status: String,
    pub risk: String,
    pub capabilities: Vec<String>,
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkTaskMutation {
    pub id: String,
    pub version: i64,
}

pub(in crate::repositories) struct SqliteWorkTaskRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteWorkTaskRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn list(
        &self,
        request: &WorkTaskListRequest,
    ) -> Result<Vec<WorkTaskProjection>, RepositoryError> {
        if request.statuses.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "work task status filter must not be empty".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let statuses = request.statuses.clone();
        let limit = i64::from(request.limit.clamp(1, 200));
        self.session.read(move |connection| {
            let placeholders = std::iter::repeat_n("?", statuses.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT id, kind, status, risk, source_type, source_id, title, summary, reason, \
                 due_at, capabilities_json, work_route_json, version \
                 FROM work_tasks \
                 WHERE tenant_id = ? AND status IN ({placeholders}) \
                 ORDER BY CASE risk WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END, \
                 created_at DESC LIMIT ?"
            );
            let mut values = Vec::with_capacity(statuses.len() + 2);
            values.push(SqlValue::Text(tenant_id));
            values.extend(statuses.into_iter().map(SqlValue::Text));
            values.push(SqlValue::Integer(limit));

            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(params_from_iter(values.iter()), map_sqlite_task)?
                .collect()
        })
    }

    pub(in crate::repositories) fn get_state(
        &self,
        id: &str,
    ) -> Result<Option<WorkTaskState>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT status, risk, capabilities_json, version \
                     FROM work_tasks WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, id],
                    |row| {
                        let capabilities_raw: String = row.get(2)?;
                        Ok(WorkTaskState {
                            status: row.get(0)?,
                            risk: row.get(1)?,
                            capabilities: serde_json::from_str(&capabilities_raw)
                                .unwrap_or_default(),
                            version: row.get(3)?,
                        })
                    },
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn update_status(
        &self,
        id: &str,
        expected_version: i64,
        new_status: &str,
        now: &str,
    ) -> Result<Option<WorkTaskMutation>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        let new_status = new_status.to_owned();
        let now = now.to_owned();
        self.session.write(move |transaction| {
            let affected = transaction.execute(
                "UPDATE work_tasks \
                 SET status = ?1, version = version + 1, updated_at = ?2 \
                 WHERE tenant_id = ?3 AND id = ?4 AND version = ?5",
                params![new_status, now, tenant_id, id, expected_version],
            )?;
            if affected == 0 {
                return Ok(None);
            }
            transaction
                .query_row(
                    "SELECT id, version FROM work_tasks WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, id],
                    |row| {
                        Ok(WorkTaskMutation {
                            id: row.get(0)?,
                            version: row.get(1)?,
                        })
                    },
                )
                .map(Some)
        })
    }

    pub(in crate::repositories) fn send_to_pc(
        &self,
        id: &str,
        expected_version: i64,
        now: &str,
    ) -> Result<Option<WorkTaskMutation>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        let now = now.to_owned();
        self.session.write(move |transaction| {
            let affected = transaction.execute(
                "UPDATE work_tasks \
                 SET status = 'in_progress', assignee_id = 'pc_queue', \
                     version = version + 1, updated_at = ?1 \
                 WHERE tenant_id = ?2 AND id = ?3 AND version = ?4",
                params![now, tenant_id, id, expected_version],
            )?;
            if affected == 0 {
                return Ok(None);
            }
            transaction
                .query_row(
                    "SELECT id, version FROM work_tasks WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, id],
                    |row| {
                        Ok(WorkTaskMutation {
                            id: row.get(0)?,
                            version: row.get(1)?,
                        })
                    },
                )
                .map(Some)
        })
    }
}

fn map_sqlite_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkTaskProjection> {
    let capabilities_raw: String = row.get(10)?;
    let work_route_raw: String = row.get(11)?;
    Ok(WorkTaskProjection {
        id: row.get(0)?,
        kind: row.get(1)?,
        status: row.get(2)?,
        risk: row.get(3)?,
        source_type: row.get(4)?,
        source_id: row.get(5)?,
        title: row.get(6)?,
        summary: row.get(7)?,
        reason: row.get(8)?,
        due_at: row.get(9)?,
        capabilities: serde_json::from_str(&capabilities_raw).unwrap_or_default(),
        work_route: serde_json::from_str(&work_route_raw)
            .unwrap_or_else(|_| serde_json::json!({"name": "dashboard"})),
        version: row.get(12)?,
    })
}

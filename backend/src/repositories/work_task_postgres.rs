#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::session::RepositorySession;
use crate::repositories::work_task::{
    WorkTaskListRequest, WorkTaskMutation, WorkTaskProjection, WorkTaskState,
};

pub(in crate::repositories) struct PostgresWorkTaskRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresWorkTaskRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
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
        let request = request.clone();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let mut query = QueryBuilder::<Postgres>::new(
                    "SELECT id, kind, status, risk, source_type, source_id, title, summary, reason, \
                     due_at, capabilities_json, work_route_json, version::BIGINT AS version \
                     FROM work_tasks WHERE tenant_id = ",
                );
                query.push_bind(&tenant_id).push(" AND status IN (");
                {
                    let mut statuses = query.separated(", ");
                    for status in &request.statuses {
                        statuses.push_bind(status);
                    }
                }
                query
                    .push(") ORDER BY CASE risk WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END, created_at DESC LIMIT ")
                    .push_bind(i64::from(request.limit.clamp(1, 200)));
                let rows = query.build().fetch_all(&mut *connection).await?;
                rows.iter().map(map_pg_task).collect()
            })
        })
    }

    pub(in crate::repositories) fn get_state(
        &self,
        id: &str,
    ) -> Result<Option<WorkTaskState>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT status, risk, capabilities_json, version::BIGINT AS version \
                     FROM work_tasks WHERE tenant_id = $1 AND id = $2",
                )
                .bind(&tenant_id)
                .bind(&id)
                .fetch_optional(&mut *connection)
                .await?;
                row.map(|row| {
                    let capabilities_raw: String = row.try_get("capabilities_json")?;
                    Ok(WorkTaskState {
                        status: row.try_get("status")?,
                        risk: row.try_get("risk")?,
                        capabilities: serde_json::from_str(&capabilities_raw).unwrap_or_default(),
                        version: row.try_get("version")?,
                    })
                })
                .transpose()
            })
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
        self.session.pg_write(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "UPDATE work_tasks \
                     SET status = $1, version = version + 1, updated_at = CAST($2 AS TIMESTAMPTZ) \
                     WHERE tenant_id = $3 AND id = $4 AND version = $5 \
                     RETURNING id, version::BIGINT AS version",
                )
                .bind(new_status)
                .bind(now)
                .bind(tenant_id)
                .bind(id)
                .bind(expected_version)
                .fetch_optional(&mut *connection)
                .await?;
                row.map(|row| {
                    Ok(WorkTaskMutation {
                        id: row.try_get("id")?,
                        version: row.try_get("version")?,
                    })
                })
                .transpose()
            })
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
        self.session.pg_write(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "UPDATE work_tasks \
                     SET status = 'in_progress', assignee_id = 'pc_queue', \
                         version = version + 1, updated_at = CAST($1 AS TIMESTAMPTZ) \
                     WHERE tenant_id = $2 AND id = $3 AND version = $4 \
                     RETURNING id, version::BIGINT AS version",
                )
                .bind(now)
                .bind(tenant_id)
                .bind(id)
                .bind(expected_version)
                .fetch_optional(&mut *connection)
                .await?;
                row.map(|row| {
                    Ok(WorkTaskMutation {
                        id: row.try_get("id")?,
                        version: row.try_get("version")?,
                    })
                })
                .transpose()
            })
        })
    }
}

fn map_pg_task(row: &sqlx::postgres::PgRow) -> Result<WorkTaskProjection, sqlx::Error> {
    let capabilities_raw: String = row.try_get("capabilities_json")?;
    let work_route_raw: String = row.try_get("work_route_json")?;
    Ok(WorkTaskProjection {
        id: row.try_get("id")?,
        kind: row.try_get("kind")?,
        status: row.try_get("status")?,
        risk: row.try_get("risk")?,
        source_type: row.try_get("source_type")?,
        source_id: row.try_get("source_id")?,
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        reason: row.try_get("reason")?,
        due_at: row.try_get("due_at")?,
        capabilities: serde_json::from_str(&capabilities_raw).unwrap_or_default(),
        work_route: serde_json::from_str(&work_route_raw)
            .unwrap_or_else(|_| serde_json::json!({"name": "dashboard"})),
        version: row.try_get("version")?,
    })
}

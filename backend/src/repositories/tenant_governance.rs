use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, params, params_from_iter, types::Value as SqlValue};
use serde::Serialize;

use super::RepositoryError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GovernanceTenantProjection {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub status: String,
    pub plan: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GovernanceTenantListQuery {
    pub search: Option<String>,
    pub status: Option<String>,
    pub cursor_at: Option<String>,
    pub cursor_id: Option<String>,
    pub limit_plus_one: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GovernanceHealthProjection {
    pub tenant_id: String,
    pub lifecycle_status: String,
    pub last_activity_at: Option<String>,
    pub audit_events_24h: i64,
    pub failed_command_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GovernanceAuditQuery {
    pub tenant_id: Option<String>,
    pub actor: Option<String>,
    pub command: Option<String>,
    pub result: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub cursor_at: Option<String>,
    pub cursor_id: Option<String>,
    pub limit_plus_one: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GovernanceAuditRecord {
    pub id: String,
    pub tenant_id: String,
    pub actor_identity_id: Option<String>,
    pub action_type: String,
    pub created_at: String,
    pub detail_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GovernanceChangeIntent {
    pub id: String,
    pub target_tenant_id: String,
    pub actor_id: String,
    pub reason: String,
    pub intended_outcome: String,
    pub impact: String,
    pub cost_minor: Option<i64>,
    pub currency: Option<String>,
    pub correlation_id: String,
    pub created_at: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum GovernanceMutationError {
    #[error("tenant not found")]
    TenantNotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

#[derive(Clone)]
pub(crate) struct SqliteTenantGovernanceRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteTenantGovernanceRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn tenant_list(
        &self,
        query: GovernanceTenantListQuery,
    ) -> Result<Vec<GovernanceTenantProjection>, RepositoryError> {
        let conn = self.connection()?;
        let mut sql =
            "SELECT id,name,slug,status,plan,created_at,updated_at FROM tenants".to_owned();
        let mut values = Vec::<SqlValue>::new();
        let mut clauses = Vec::new();

        if let Some(search) = query.search {
            clauses.push("(name LIKE ? ESCAPE '\\' OR slug LIKE ? ESCAPE '\\')".to_owned());
            let pattern = format!("%{}%", escape_like(&search));
            values.extend([pattern.clone().into(), pattern.into()]);
        }
        if let Some(status) = query.status {
            clauses.push("status=?".to_owned());
            values.push(status.into());
        }
        if let (Some(at), Some(id)) = (query.cursor_at, query.cursor_id) {
            clauses.push(
                "(julianday(created_at) < julianday(?) OR                  (julianday(created_at) = julianday(?) AND id < ?))"
                    .to_owned(),
            );
            values.extend([at.clone().into(), at.into(), id.into()]);
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY julianday(created_at) DESC,id DESC LIMIT ?");
        values.push(query.limit_plus_one.into());

        let mut statement = conn.prepare(&sql).map_err(sqlite_storage)?;
        statement
            .query_map(params_from_iter(values), map_tenant)
            .map_err(sqlite_storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_storage)
    }

    pub(crate) fn tenant_get(
        &self,
        tenant_id: &str,
    ) -> Result<Option<GovernanceTenantProjection>, RepositoryError> {
        let conn = self.connection()?;
        conn.query_row(
            "SELECT id,name,slug,status,plan,created_at,updated_at
             FROM tenants WHERE id=?1",
            [tenant_id],
            map_tenant,
        )
        .optional()
        .map_err(sqlite_storage)
    }

    pub(crate) fn tenant_health(
        &self,
        tenant_id: &str,
        since: &str,
    ) -> Result<Option<GovernanceHealthProjection>, RepositoryError> {
        let conn = self.connection()?;
        conn.query_row(
            "SELECT t.id,t.status,MAX(a.createdAt),
                    COUNT(CASE WHEN a.id IS NOT NULL
                         AND (NOT json_valid(a.detailJson)
                              OR LOWER(COALESCE(json_extract(a.detailJson,'$.result'),'')) <> 'attempted')
                         THEN 1 END),
                    COALESCE(SUM(CASE WHEN a.id IS NOT NULL
                         AND json_valid(a.detailJson)
                         AND json_type(a.detailJson,'$.result')='object'
                         THEN 1 ELSE 0 END),0)
             FROM tenants t
             LEFT JOIN audit_logs a
               ON a.tenant_id=t.id AND julianday(a.createdAt) >= julianday(?2)
             WHERE t.id=?1
             GROUP BY t.id,t.status",
            params![tenant_id, since],
            |row| {
                Ok(GovernanceHealthProjection {
                    tenant_id: row.get(0)?,
                    lifecycle_status: row.get(1)?,
                    last_activity_at: row.get(2)?,
                    audit_events_24h: row.get(3)?,
                    failed_command_count: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(sqlite_storage)
    }

    pub(crate) fn audit_query(
        &self,
        query: GovernanceAuditQuery,
    ) -> Result<Vec<GovernanceAuditRecord>, RepositoryError> {
        let conn = self.connection()?;
        let mut clauses = Vec::new();
        let mut values = Vec::<SqlValue>::new();

        if let Some(value) = query.tenant_id {
            clauses.push("tenant_id=?".to_owned());
            values.push(value.into());
        }
        if let Some(value) = query.actor {
            clauses.push(
                "COALESCE(CASE WHEN json_valid(detailJson)
                     THEN json_extract(detailJson,'$.actor.id') END,actorIdentityId)=?"
                    .to_owned(),
            );
            values.push(value.into());
        }
        if let Some(value) = query.command {
            clauses.push(
                "COALESCE(CASE WHEN json_valid(detailJson)
                     THEN json_extract(detailJson,'$.command') END,actionType)=?"
                    .to_owned(),
            );
            values.push(value.into());
        }
        if let Some(value) = query.result {
            clauses.push(
                "CASE
                   WHEN json_valid(detailJson)
                    AND json_type(detailJson,'$.result')='object'
                   THEN CASE
                     WHEN UPPER(COALESCE(
                       json_extract(detailJson,'$.result.failed.error_code'),
                       json_extract(detailJson,'$.result.Failed.error_code'),'')) LIKE '%AUTH%'
                       OR UPPER(COALESCE(
                       json_extract(detailJson,'$.result.failed.error_code'),
                       json_extract(detailJson,'$.result.Failed.error_code'),'')) LIKE '%DENIED%'
                       OR UPPER(COALESCE(
                       json_extract(detailJson,'$.result.failed.error_code'),
                       json_extract(detailJson,'$.result.Failed.error_code'),'')) LIKE '%FORBIDDEN%'
                     THEN 'denied' ELSE 'failed' END
                   WHEN LOWER(COALESCE(json_extract(detailJson,'$.result'),''))='attempted'
                     THEN 'attempted'
                   WHEN LOWER(COALESCE(json_extract(detailJson,'$.result'),''))='succeeded'
                     THEN 'succeeded'
                   ELSE 'failed'
                 END=?"
                    .to_owned(),
            );
            values.push(value.into());
        }
        if let Some(value) = query.from {
            clauses.push("julianday(createdAt)>=julianday(?)".to_owned());
            values.push(value.into());
        }
        if let Some(value) = query.to {
            clauses.push("julianday(createdAt)<=julianday(?)".to_owned());
            values.push(value.into());
        }
        if let (Some(at), Some(id)) = (query.cursor_at, query.cursor_id) {
            clauses.push(
                "(julianday(createdAt) < julianday(?) OR                  (julianday(createdAt) = julianday(?) AND id < ?))"
                    .to_owned(),
            );
            values.extend([at.clone().into(), at.into(), id.into()]);
        }

        let mut sql =
            "SELECT id,tenant_id,actorIdentityId,actionType,createdAt,detailJson FROM audit_logs"
                .to_owned();
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY julianday(createdAt) DESC,id DESC LIMIT ?");
        values.push(query.limit_plus_one.into());

        let mut statement = conn.prepare(&sql).map_err(sqlite_storage)?;
        statement
            .query_map(params_from_iter(values), map_audit)
            .map_err(sqlite_storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_storage)
    }

    pub(crate) fn audit_get(
        &self,
        audit_id: &str,
    ) -> Result<Option<GovernanceAuditRecord>, RepositoryError> {
        let conn = self.connection()?;
        conn.query_row(
            "SELECT id,tenant_id,actorIdentityId,actionType,createdAt,detailJson
             FROM audit_logs WHERE id=?1",
            [audit_id],
            map_audit,
        )
        .optional()
        .map_err(sqlite_storage)
    }

    pub(crate) fn record_change_intent(
        &self,
        command: GovernanceChangeIntent,
    ) -> Result<(), GovernanceMutationError> {
        let mut conn = self.connection()?;
        let transaction = conn.unchecked_transaction().map_err(sqlite_mutation)?;
        let tenant_exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tenants WHERE id=?1)",
                [command.target_tenant_id.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_mutation)?;
        if !tenant_exists {
            return Err(GovernanceMutationError::TenantNotFound);
        }

        transaction
            .execute(
                "INSERT INTO change_intents
                 (id,target_tenant_id,actor_id,reason,intended_outcome,impact,cost_minor,
                  currency,source,correlation_id,status,created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'platform_governance',?9,'recorded',?10)",
                params![
                    command.id,
                    command.target_tenant_id,
                    command.actor_id,
                    command.reason,
                    command.intended_outcome,
                    command.impact,
                    command.cost_minor,
                    command.currency,
                    command.correlation_id,
                    command.created_at,
                ],
            )
            .map_err(sqlite_mutation)?;
        transaction.commit().map_err(sqlite_mutation)?;
        Ok(())
    }

    fn connection(
        &self,
    ) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, RepositoryError> {
        self.pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))
    }
}

fn map_tenant(row: &rusqlite::Row<'_>) -> rusqlite::Result<GovernanceTenantProjection> {
    Ok(GovernanceTenantProjection {
        id: row.get(0)?,
        name: row.get(1)?,
        slug: row.get(2)?,
        status: row.get(3)?,
        plan: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn map_audit(row: &rusqlite::Row<'_>) -> rusqlite::Result<GovernanceAuditRecord> {
    Ok(GovernanceAuditRecord {
        id: row.get(0)?,
        tenant_id: row.get(1)?,
        actor_identity_id: row.get(2)?,
        action_type: row.get(3)?,
        created_at: row.get(4)?,
        detail_json: row.get(5)?,
    })
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn sqlite_storage(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn sqlite_mutation(error: rusqlite::Error) -> GovernanceMutationError {
    GovernanceMutationError::Storage(sqlite_storage(error))
}

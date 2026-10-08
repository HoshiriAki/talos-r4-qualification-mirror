use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::types::ToSql;
use system_admin::audit::{AuditLogEntry, ListAuditLogsInput, ListAuditLogsOutput};

use super::RepositoryError;

#[derive(Debug, Clone)]
pub(crate) struct AuditAuthorityWrite {
    pub id: String,
    pub actor_identity_id: String,
    pub authority_kind: String,
    pub tenant_id: Option<String>,
    pub tenant_membership_id: Option<String>,
    pub platform_membership_id: Option<String>,
    pub roles_snapshot: String,
    pub capabilities_snapshot: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub correlation_id: String,
    pub detail_json: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AuditCompatibilityWrite {
    pub id: String,
    pub tenant_id: String,
    pub actor_identity_id: String,
    pub actor_username: String,
    pub action_type: String,
    pub entity_type: String,
    pub entity_id: String,
    pub entity_label: String,
    pub detail_json: String,
    pub ip: String,
    pub user_agent: String,
    pub created_at: String,
}

#[derive(Clone)]
pub(crate) struct SqliteAuditCompatibilityRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteAuditCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn append_authority_event(
        &self,
        entry: &AuditAuthorityWrite,
    ) -> Result<(), RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        conn.execute(
            "INSERT INTO audit_events
             (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
              platform_membership_id, roles_snapshot, capabilities_snapshot, action,
              resource_type, resource_id, correlation_id, detail_json, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                entry.id.as_str(),
                entry.actor_identity_id.as_str(),
                entry.authority_kind.as_str(),
                entry.tenant_id.as_deref(),
                entry.tenant_membership_id.as_deref(),
                entry.platform_membership_id.as_deref(),
                entry.roles_snapshot.as_str(),
                entry.capabilities_snapshot.as_str(),
                entry.action.as_str(),
                entry.resource_type.as_str(),
                entry.resource_id.as_deref(),
                entry.correlation_id.as_str(),
                entry.detail_json.as_str(),
                entry.occurred_at.as_str(),
            ],
        )
        .map_err(sqlite_storage)?;
        Ok(())
    }

    pub(crate) fn append_audit_log(
        &self,
        entry: &AuditCompatibilityWrite,
    ) -> Result<(), RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        conn.execute(
            "INSERT INTO audit_logs
             (id, actorIdentityId, actorUsername, actionType, entityType, entityId,
              entityLabel, detailJson, ip, userAgent, createdAt, tenant_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                entry.id.as_str(),
                entry.actor_identity_id.as_str(),
                entry.actor_username.as_str(),
                entry.action_type.as_str(),
                entry.entity_type.as_str(),
                entry.entity_id.as_str(),
                entry.entity_label.as_str(),
                entry.detail_json.as_str(),
                entry.ip.as_str(),
                entry.user_agent.as_str(),
                entry.created_at.as_str(),
                entry.tenant_id.as_str(),
            ],
        )
        .map_err(sqlite_storage)?;
        Ok(())
    }

    pub(crate) fn list_audit_logs(
        &self,
        tenant_id: &str,
        input: &ListAuditLogsInput,
    ) -> Result<ListAuditLogsOutput, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let (where_sql, values) = sqlite_filters(tenant_id, input);

        let count_sql = format!("SELECT COUNT(1) FROM audit_logs {where_sql}");
        let mut count_stmt = conn.prepare(&count_sql).map_err(sqlite_storage)?;
        let count_boxed: Vec<Box<dyn ToSql>> = values
            .iter()
            .map(|value| Box::new(value.clone()) as Box<dyn ToSql>)
            .collect();
        let count_refs: Vec<&dyn ToSql> = count_boxed.iter().map(|value| value.as_ref()).collect();
        let total: i64 = count_stmt
            .query_row(count_refs.as_slice(), |row| row.get(0))
            .map_err(sqlite_storage)?;

        let sql = format!(
            "SELECT id, actorIdentityId, actorUsername, actionType, entityType, entityId,
                    entityLabel, detailJson, ip, userAgent, createdAt
             FROM audit_logs
             {where_sql}
             ORDER BY {} {}, id ASC
             LIMIT ? OFFSET ?",
            sqlite_sort_column(input),
            sort_order(input),
        );
        let mut stmt = conn.prepare(&sql).map_err(sqlite_storage)?;
        let mut boxed: Vec<Box<dyn ToSql>> = values
            .into_iter()
            .map(|value| Box::new(value) as Box<dyn ToSql>)
            .collect();
        boxed.push(Box::new(input.limit));
        boxed.push(Box::new(input.offset));
        let refs: Vec<&dyn ToSql> = boxed.iter().map(|value| value.as_ref()).collect();
        let logs = stmt
            .query_map(refs.as_slice(), |row| {
                Ok(AuditLogEntry {
                    id: row.get(0)?,
                    actor_identity_id: row.get::<_, String>(1).unwrap_or_default(),
                    actor_username: row.get::<_, String>(2).unwrap_or_default(),
                    action_type: row.get(3)?,
                    entity_type: row.get(4)?,
                    entity_id: row.get::<_, String>(5).unwrap_or_default(),
                    entity_label: row.get::<_, String>(6).unwrap_or_default(),
                    detail_json: row.get::<_, String>(7).unwrap_or_default(),
                    ip: row.get::<_, String>(8).unwrap_or_default(),
                    user_agent: row.get::<_, String>(9).unwrap_or_default(),
                    created_at: row.get::<_, String>(10).unwrap_or_default(),
                })
            })
            .map_err(sqlite_storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_storage)?;

        Ok(ListAuditLogsOutput {
            logs,
            limit: input.limit,
            total,
        })
    }
}

fn sqlite_filters(tenant_id: &str, input: &ListAuditLogsInput) -> (String, Vec<String>) {
    let mut clauses = vec!["tenant_id = ?".to_string()];
    let mut values = vec![tenant_id.to_owned()];
    if !input.actor_identity_id.is_empty() {
        clauses.push("actorIdentityId = ?".into());
        values.push(input.actor_identity_id.clone());
    }
    if !input.actor_username.is_empty() {
        clauses.push("actorUsername LIKE ?".into());
        values.push(format!("%{}%", input.actor_username));
    }
    if !input.action_type.is_empty() {
        clauses.push("actionType = ?".into());
        values.push(input.action_type.clone());
    }
    if !input.entity_type.is_empty() {
        clauses.push("entityType = ?".into());
        values.push(input.entity_type.clone());
    }
    if !input.keyword.is_empty() {
        clauses.push("(entityLabel LIKE ? OR actorUsername LIKE ? OR detailJson LIKE ?)".into());
        let like = format!("%{}%", input.keyword);
        values.extend([like.clone(), like.clone(), like]);
    }
    if is_legacy_date(&input.date) {
        clauses.push("createdAt >= ?".into());
        values.push(format!("{}T00:00:00+08:00", input.date));
        clauses.push("createdAt <= ?".into());
        values.push(format!("{}T23:59:59.999+08:00", input.date));
    }
    if is_legacy_date(&input.start_date) {
        clauses.push("createdAt >= ?".into());
        values.push(input.start_date.clone());
    }
    if is_legacy_date(&input.end_date) {
        clauses.push("createdAt <= ?".into());
        values.push(format!("{}T23:59:59.999+08:00", input.end_date));
    }
    (format!("WHERE {}", clauses.join(" AND ")), values)
}

pub(crate) fn is_legacy_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

pub(crate) fn sort_order(input: &ListAuditLogsInput) -> &'static str {
    if input.sort_order.as_deref() == Some("asc") {
        "ASC"
    } else {
        "DESC"
    }
}

pub(crate) fn sqlite_sort_column(input: &ListAuditLogsInput) -> &'static str {
    match input.sort_by.as_deref() {
        Some("actorUsername") => "actorUsername",
        Some("actionType") => "actionType",
        Some("entityType") => "entityType",
        Some("entityLabel") => "entityLabel",
        _ => "createdAt",
    }
}

fn sqlite_storage(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

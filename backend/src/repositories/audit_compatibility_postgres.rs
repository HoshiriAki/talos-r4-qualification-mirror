use std::future::Future;

use sqlx::postgres::PgPool;
use sqlx::{Postgres, QueryBuilder, Row};
use system_admin::audit::{AuditLogEntry, ListAuditLogsInput, ListAuditLogsOutput};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::RepositoryError;
use super::audit_compatibility::{
    AuditAuthorityWrite, AuditCompatibilityWrite, is_legacy_date, sort_order,
};

#[derive(Clone)]
pub(crate) struct PostgresAuditCompatibilityRepository {
    pool: PgPool,
}

impl PostgresAuditCompatibilityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn append_authority_event(
        &self,
        entry: &AuditAuthorityWrite,
    ) -> Result<(), RepositoryError> {
        let pool = self.pool.clone();
        let entry = entry.clone();
        run_pg_audit(async move {
            sqlx::query(
                "INSERT INTO audit_events
                 (id, actor_identity_id, authority_kind, tenant_id, tenant_membership_id,
                  platform_membership_id, roles_snapshot, capabilities_snapshot, action,
                  resource_type, resource_id, correlation_id, detail_json, occurred_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7::jsonb,$8::jsonb,$9,$10,$11,$12,$13::jsonb,$14)",
            )
            .bind(entry.id)
            .bind(entry.actor_identity_id)
            .bind(entry.authority_kind)
            .bind(entry.tenant_id)
            .bind(entry.tenant_membership_id)
            .bind(entry.platform_membership_id)
            .bind(entry.roles_snapshot)
            .bind(entry.capabilities_snapshot)
            .bind(entry.action)
            .bind(entry.resource_type)
            .bind(entry.resource_id)
            .bind(entry.correlation_id)
            .bind(entry.detail_json)
            .bind(entry.occurred_at)
            .execute(&pool)
            .await
            .map_err(pg_storage)?;
            Ok(())
        })
    }

    pub(crate) fn append_audit_log(
        &self,
        entry: &AuditCompatibilityWrite,
    ) -> Result<(), RepositoryError> {
        let pool = self.pool.clone();
        let entry = entry.clone();
        run_pg_audit(async move {
            sqlx::query(
                r#"INSERT INTO audit_logs
                   (id, "actorIdentityId", "actorUsername", "actionType", "entityType",
                    "entityId", "entityLabel", "detailJson", ip, "userAgent", "createdAt",
                    tenant_id)
                   VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)"#,
            )
            .bind(entry.id)
            .bind(entry.actor_identity_id)
            .bind(entry.actor_username)
            .bind(entry.action_type)
            .bind(entry.entity_type)
            .bind(entry.entity_id)
            .bind(entry.entity_label)
            .bind(entry.detail_json)
            .bind(entry.ip)
            .bind(entry.user_agent)
            .bind(entry.created_at)
            .bind(entry.tenant_id)
            .execute(&pool)
            .await
            .map_err(pg_storage)?;
            Ok(())
        })
    }

    pub(crate) fn list_audit_logs(
        &self,
        tenant_id: &str,
        input: &ListAuditLogsInput,
    ) -> Result<ListAuditLogsOutput, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let input = input.clone();
        run_pg_audit(async move {
            let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(1) FROM audit_logs");
            push_filters(&mut count, &tenant_id, &input);
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&pool)
                .await
                .map_err(pg_storage)?;

            let mut query = QueryBuilder::<Postgres>::new(
                r#"SELECT id, "actorIdentityId", "actorUsername", "actionType", "entityType",
                          "entityId", "entityLabel", "detailJson", ip, "userAgent", "createdAt"
                   FROM audit_logs"#,
            );
            push_filters(&mut query, &tenant_id, &input);
            query
                .push(" ORDER BY ")
                .push(pg_sort_column(&input))
                .push(" ")
                .push(sort_order(&input))
                .push(", id ASC LIMIT ")
                .push_bind(input.limit)
                .push(" OFFSET ")
                .push_bind(input.offset);
            let rows = query.build().fetch_all(&pool).await.map_err(pg_storage)?;
            let logs = rows
                .into_iter()
                .map(|row| {
                    Ok(AuditLogEntry {
                        id: row.try_get(0).map_err(pg_storage)?,
                        actor_identity_id: row.try_get(1).unwrap_or_default(),
                        actor_username: row.try_get(2).unwrap_or_default(),
                        action_type: row.try_get(3).map_err(pg_storage)?,
                        entity_type: row.try_get(4).map_err(pg_storage)?,
                        entity_id: row.try_get(5).unwrap_or_default(),
                        entity_label: row.try_get(6).unwrap_or_default(),
                        detail_json: row.try_get(7).unwrap_or_default(),
                        ip: row.try_get(8).unwrap_or_default(),
                        user_agent: row.try_get(9).unwrap_or_default(),
                        created_at: row.try_get(10).unwrap_or_default(),
                    })
                })
                .collect::<Result<Vec<_>, RepositoryError>>()?;
            Ok(ListAuditLogsOutput {
                logs,
                limit: input.limit,
                total,
            })
        })
    }
}

fn push_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    tenant_id: &str,
    input: &ListAuditLogsInput,
) {
    builder
        .push(" WHERE tenant_id = ")
        .push_bind(tenant_id.to_owned());
    if !input.actor_identity_id.is_empty() {
        builder
            .push(r#" AND "actorIdentityId" = "#)
            .push_bind(input.actor_identity_id.clone());
    }
    if !input.actor_username.is_empty() {
        builder
            .push(r#" AND "actorUsername" ILIKE "#)
            .push_bind(format!("%{}%", input.actor_username));
    }
    if !input.action_type.is_empty() {
        builder
            .push(r#" AND "actionType" = "#)
            .push_bind(input.action_type.clone());
    }
    if !input.entity_type.is_empty() {
        builder
            .push(r#" AND "entityType" = "#)
            .push_bind(input.entity_type.clone());
    }
    if !input.keyword.is_empty() {
        let like = format!("%{}%", input.keyword);
        builder
            .push(r#" AND ("entityLabel" ILIKE "#)
            .push_bind(like.clone())
            .push(r#" OR "actorUsername" ILIKE "#)
            .push_bind(like.clone())
            .push(r#" OR "detailJson" ILIKE "#)
            .push_bind(like)
            .push(")");
    }
    if is_legacy_date(&input.date) {
        builder
            .push(r#" AND "createdAt" >= "#)
            .push_bind(format!("{}T00:00:00+08:00", input.date))
            .push(r#" AND "createdAt" <= "#)
            .push_bind(format!("{}T23:59:59.999+08:00", input.date));
    }
    if is_legacy_date(&input.start_date) {
        builder
            .push(r#" AND "createdAt" >= "#)
            .push_bind(input.start_date.clone());
    }
    if is_legacy_date(&input.end_date) {
        builder
            .push(r#" AND "createdAt" <= "#)
            .push_bind(format!("{}T23:59:59.999+08:00", input.end_date));
    }
}

fn pg_sort_column(input: &ListAuditLogsInput) -> &'static str {
    match input.sort_by.as_deref() {
        Some("actorUsername") => r#""actorUsername""#,
        Some("actionType") => r#""actionType""#,
        Some("entityType") => r#""entityType""#,
        Some("entityLabel") => r#""entityLabel""#,
        _ => r#""createdAt""#,
    }
}

fn pg_storage(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn run_pg_audit<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("audit compatibility runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "audit compatibility requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

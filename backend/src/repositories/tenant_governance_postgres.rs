#![cfg(feature = "postgres")]

use std::future::Future;

use sqlx::{Postgres, QueryBuilder, Row};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::RepositoryError;
use super::tenant_governance::{
    GovernanceAuditQuery, GovernanceAuditRecord, GovernanceChangeIntent,
    GovernanceHealthProjection, GovernanceMutationError, GovernanceTenantListQuery,
    GovernanceTenantProjection,
};

#[derive(Clone)]
pub(crate) struct PostgresTenantGovernanceRepository {
    pool: sqlx::PgPool,
}

impl PostgresTenantGovernanceRepository {
    pub(crate) fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn tenant_list(
        &self,
        query: GovernanceTenantListQuery,
    ) -> Result<Vec<GovernanceTenantProjection>, RepositoryError> {
        let pool = self.pool.clone();
        run_pg_governance(async move {
            let mut builder = QueryBuilder::<Postgres>::new(
                "SELECT id,name,slug,status,plan,created_at,updated_at FROM tenants",
            );
            let mut has_where = false;
            if let Some(search) = query.search {
                push_where(&mut builder, &mut has_where);
                let like = format!("%{}%", escape_like_pg(&search));
                builder
                    .push("(name ILIKE ")
                    .push_bind(like.clone())
                    .push(" ESCAPE '\\' OR slug ILIKE ")
                    .push_bind(like)
                    .push(" ESCAPE '\\')");
            }
            if let Some(status) = query.status {
                push_where(&mut builder, &mut has_where);
                builder.push("status=").push_bind(status);
            }
            if let (Some(at), Some(id)) = (query.cursor_at, query.cursor_id) {
                push_where(&mut builder, &mut has_where);
                builder
                    .push("(created_at::timestamptz < ")
                    .push_bind(at.clone())
                    .push("::timestamptz OR (created_at::timestamptz = ")
                    .push_bind(at)
                    .push("::timestamptz AND id < ")
                    .push_bind(id)
                    .push("))");
            }
            builder
                .push(" ORDER BY created_at::timestamptz DESC,id DESC LIMIT ")
                .push_bind(query.limit_plus_one);
            let rows = builder.build().fetch_all(&pool).await.map_err(pg_storage)?;
            rows.into_iter().map(map_tenant).collect()
        })
    }

    pub(crate) fn tenant_get(
        &self,
        tenant_id: &str,
    ) -> Result<Option<GovernanceTenantProjection>, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        run_pg_governance(async move {
            sqlx::query(
                "SELECT id,name,slug,status,plan,created_at,updated_at
                 FROM tenants WHERE id=$1",
            )
            .bind(tenant_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_storage)?
            .map(map_tenant)
            .transpose()
        })
    }

    pub(crate) fn tenant_health(
        &self,
        tenant_id: &str,
        since: &str,
    ) -> Result<Option<GovernanceHealthProjection>, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let since = since.to_owned();
        run_pg_governance(async move {
            sqlx::query(
                r#"SELECT t.id,t.status,MAX(a."createdAt"),
                          COUNT(*) FILTER (
                            WHERE a.id IS NOT NULL
                              AND LOWER(COALESCE((a."detailJson"::jsonb)->>'result','')) <> 'attempted'
                          )::bigint AS audit_events_24h,
                          COUNT(*) FILTER (
                            WHERE a.id IS NOT NULL
                              AND jsonb_typeof((a."detailJson"::jsonb)->'result')='object'
                          )::bigint AS failed_command_count
                   FROM tenants t
                   LEFT JOIN audit_logs a
                     ON a.tenant_id=t.id
                    AND a."createdAt"::timestamptz >= $2::timestamptz
                   WHERE t.id=$1
                   GROUP BY t.id,t.status"#,
            )
            .bind(tenant_id)
            .bind(since)
            .fetch_optional(&pool)
            .await
            .map_err(pg_storage)?
            .map(|row| {
                Ok(GovernanceHealthProjection {
                    tenant_id: row.try_get(0).map_err(pg_storage)?,
                    lifecycle_status: row.try_get(1).map_err(pg_storage)?,
                    last_activity_at: row.try_get(2).map_err(pg_storage)?,
                    audit_events_24h: row.try_get(3).map_err(pg_storage)?,
                    failed_command_count: row.try_get(4).map_err(pg_storage)?,
                })
            })
            .transpose()
        })
    }

    pub(crate) fn audit_query(
        &self,
        query: GovernanceAuditQuery,
    ) -> Result<Vec<GovernanceAuditRecord>, RepositoryError> {
        let pool = self.pool.clone();
        run_pg_governance(async move {
            let mut builder = QueryBuilder::<Postgres>::new(
                r#"SELECT id,tenant_id,"actorIdentityId","actionType","createdAt","detailJson"
                   FROM audit_logs"#,
            );
            let mut has_where = false;

            if let Some(value) = query.tenant_id {
                push_where(&mut builder, &mut has_where);
                builder.push("tenant_id=").push_bind(value);
            }
            if let Some(value) = query.actor {
                push_where(&mut builder, &mut has_where);
                builder
                    .push(r#"COALESCE(("detailJson"::jsonb #>> '{actor,id}'),"actorIdentityId")="#)
                    .push_bind(value);
            }
            if let Some(value) = query.command {
                push_where(&mut builder, &mut has_where);
                builder
                    .push(r#"COALESCE(("detailJson"::jsonb ->> 'command'),"actionType")="#)
                    .push_bind(value);
            }
            if let Some(value) = query.result {
                push_where(&mut builder, &mut has_where);
                builder
                    .push(
                        r#"CASE
                           WHEN jsonb_typeof(("detailJson"::jsonb)->'result')='object'
                           THEN CASE
                             WHEN UPPER(COALESCE(
                               ("detailJson"::jsonb #>> '{result,failed,error_code}'),
                               ("detailJson"::jsonb #>> '{result,Failed,error_code}'),'')) LIKE '%AUTH%'
                               OR UPPER(COALESCE(
                               ("detailJson"::jsonb #>> '{result,failed,error_code}'),
                               ("detailJson"::jsonb #>> '{result,Failed,error_code}'),'')) LIKE '%DENIED%'
                               OR UPPER(COALESCE(
                               ("detailJson"::jsonb #>> '{result,failed,error_code}'),
                               ("detailJson"::jsonb #>> '{result,Failed,error_code}'),'')) LIKE '%FORBIDDEN%'
                             THEN 'denied' ELSE 'failed' END
                           WHEN LOWER(COALESCE(("detailJson"::jsonb)->>'result',''))='attempted'
                             THEN 'attempted'
                           WHEN LOWER(COALESCE(("detailJson"::jsonb)->>'result',''))='succeeded'
                             THEN 'succeeded'
                           ELSE 'failed'
                         END="#,
                    )
                    .push_bind(value);
            }
            if let Some(value) = query.from {
                push_where(&mut builder, &mut has_where);
                builder
                    .push(r#""createdAt"::timestamptz >= "#)
                    .push_bind(value)
                    .push("::timestamptz");
            }
            if let Some(value) = query.to {
                push_where(&mut builder, &mut has_where);
                builder
                    .push(r#""createdAt"::timestamptz <= "#)
                    .push_bind(value)
                    .push("::timestamptz");
            }
            if let (Some(at), Some(id)) = (query.cursor_at, query.cursor_id) {
                push_where(&mut builder, &mut has_where);
                builder
                    .push(r#"("createdAt"::timestamptz < "#)
                    .push_bind(at.clone())
                    .push(r#"::timestamptz OR ("createdAt"::timestamptz = "#)
                    .push_bind(at)
                    .push("::timestamptz AND id < ")
                    .push_bind(id)
                    .push("))");
            }
            builder
                .push(r#" ORDER BY "createdAt"::timestamptz DESC,id DESC LIMIT "#)
                .push_bind(query.limit_plus_one);

            let rows = builder.build().fetch_all(&pool).await.map_err(pg_storage)?;
            rows.into_iter().map(map_audit).collect()
        })
    }

    pub(crate) fn audit_get(
        &self,
        audit_id: &str,
    ) -> Result<Option<GovernanceAuditRecord>, RepositoryError> {
        let pool = self.pool.clone();
        let audit_id = audit_id.to_owned();
        run_pg_governance(async move {
            sqlx::query(
                r#"SELECT id,tenant_id,"actorIdentityId","actionType","createdAt","detailJson"
                   FROM audit_logs WHERE id=$1"#,
            )
            .bind(audit_id)
            .fetch_optional(&pool)
            .await
            .map_err(pg_storage)?
            .map(map_audit)
            .transpose()
        })
    }

    pub(crate) fn record_change_intent(
        &self,
        command: GovernanceChangeIntent,
    ) -> Result<(), GovernanceMutationError> {
        let pool = self.pool.clone();
        run_pg_governance_mutation(async move {
            let mut transaction = pool.begin().await.map_err(pg_mutation)?;
            let tenant_exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1)")
                    .bind(&command.target_tenant_id)
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(pg_mutation)?;
            if !tenant_exists {
                return Err(GovernanceMutationError::TenantNotFound);
            }

            sqlx::query(
                "INSERT INTO change_intents
                 (id,target_tenant_id,actor_id,reason,intended_outcome,impact,cost_minor,
                  currency,source,correlation_id,status,created_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'platform_governance',$9,'recorded',$10)",
            )
            .bind(command.id)
            .bind(command.target_tenant_id)
            .bind(command.actor_id)
            .bind(command.reason)
            .bind(command.intended_outcome)
            .bind(command.impact)
            .bind(command.cost_minor)
            .bind(command.currency)
            .bind(command.correlation_id)
            .bind(command.created_at)
            .execute(&mut *transaction)
            .await
            .map_err(pg_mutation)?;
            transaction.commit().await.map_err(pg_mutation)?;
            Ok(())
        })
    }
}

fn push_where(builder: &mut QueryBuilder<'_, Postgres>, has_where: &mut bool) {
    if *has_where {
        builder.push(" AND ");
    } else {
        builder.push(" WHERE ");
        *has_where = true;
    }
}

fn map_tenant(row: sqlx::postgres::PgRow) -> Result<GovernanceTenantProjection, RepositoryError> {
    Ok(GovernanceTenantProjection {
        id: row.try_get(0).map_err(pg_storage)?,
        name: row.try_get(1).map_err(pg_storage)?,
        slug: row.try_get(2).map_err(pg_storage)?,
        status: row.try_get(3).map_err(pg_storage)?,
        plan: row.try_get(4).map_err(pg_storage)?,
        created_at: row.try_get(5).map_err(pg_storage)?,
        updated_at: row.try_get(6).map_err(pg_storage)?,
    })
}

fn map_audit(row: sqlx::postgres::PgRow) -> Result<GovernanceAuditRecord, RepositoryError> {
    Ok(GovernanceAuditRecord {
        id: row.try_get(0).map_err(pg_storage)?,
        tenant_id: row.try_get(1).map_err(pg_storage)?,
        actor_identity_id: row.try_get(2).map_err(pg_storage)?,
        action_type: row.try_get(3).map_err(pg_storage)?,
        created_at: row.try_get(4).map_err(pg_storage)?,
        detail_json: row.try_get(5).map_err(pg_storage)?,
    })
}

fn escape_like_pg(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn pg_storage(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn pg_mutation(error: sqlx::Error) -> GovernanceMutationError {
    GovernanceMutationError::Storage(pg_storage(error))
}

fn run_pg_governance<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("tenant governance runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "tenant governance requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn run_pg_governance_mutation<T, F>(future: F) -> Result<T, GovernanceMutationError>
where
    F: Future<Output = Result<T, GovernanceMutationError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        GovernanceMutationError::Storage(RepositoryError::AdapterUnavailable(
            "tenant governance runtime unavailable".into(),
        ))
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(GovernanceMutationError::Storage(
            RepositoryError::AdapterUnavailable(
                "tenant governance requires the multi-thread runtime".into(),
            ),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

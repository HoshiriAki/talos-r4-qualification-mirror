#![cfg(feature = "postgres")]

use serde_json::{Value, json};
use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::credit::score_penalty;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn blacklist_check(
    session: &RepositorySession,
    customer_phone: &str,
) -> Result<Value, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let customer_phone = customer_phone.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT id,customer_name,customer_phone,id_number,reason,severity,created_by,
                        (is_active <> 0) AS is_active,removed_at,removed_by,removal_reason,
                        created_at,updated_at
                 FROM blacklist
                 WHERE tenant_id=$1 AND customer_phone=$2 AND is_active=1
                 ORDER BY created_at DESC",
            )
            .bind(&tenant_id)
            .bind(&customer_phone)
            .fetch_all(&mut *connection)
            .await?;
            let records = rows
                .iter()
                .map(map_blacklist_record)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "blacklisted": !records.is_empty(),
                "records": records,
            }))
        })
    })
}

pub(in crate::repositories) fn blacklist_list(
    session: &RepositorySession,
    is_active: Option<bool>,
    page: i64,
    page_size: i64,
) -> Result<Value, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;
            let mut count =
                QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM blacklist WHERE ");
            count.push("tenant_id=").push_bind(&tenant_id);
            if let Some(active) = is_active {
                count
                    .push(" AND is_active=")
                    .push_bind(if active { 1_i32 } else { 0_i32 });
            }
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,customer_name,customer_phone,id_number,reason,severity,created_by,
                        (is_active <> 0) AS is_active,removed_at,removed_by,removal_reason,
                        created_at,updated_at
                 FROM blacklist WHERE ",
            );
            data.push("tenant_id=").push_bind(&tenant_id);
            if let Some(active) = is_active {
                data.push(" AND is_active=")
                    .push_bind(if active { 1_i32 } else { 0_i32 });
            }
            data.push(" ORDER BY created_at DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);
            let rows = data.build().fetch_all(&mut *connection).await?;
            let items = rows
                .iter()
                .map(map_blacklist_record)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "items": items,
                "total": total,
                "page": page,
                "pageSize": page_size,
            }))
        })
    })
}

pub(in crate::repositories) fn violation_list(
    session: &RepositorySession,
    customer_phone: Option<&str>,
    status: Option<&str>,
    violation_type: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Value, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let customer_phone = customer_phone.map(str::to_owned);
    let status = status.map(str::to_owned);
    let violation_type = violation_type.map(str::to_owned);
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;
            let mut count =
                QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM violations WHERE ");
            push_violation_filters(
                &mut count,
                &tenant_id,
                customer_phone.as_deref(),
                status.as_deref(),
                violation_type.as_deref(),
            );
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,customer_name,customer_phone,order_id,violation_type,severity,
                        description,evidence,
                        financial_penalty::double precision AS financial_penalty,
                        reported_by,status,appeal_reason,appeal_at,reviewed_by,reviewed_at,
                        review_notes,created_at,updated_at
                 FROM violations WHERE ",
            );
            push_violation_filters(
                &mut data,
                &tenant_id,
                customer_phone.as_deref(),
                status.as_deref(),
                violation_type.as_deref(),
            );
            data.push(" ORDER BY created_at DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);
            let rows = data.build().fetch_all(&mut *connection).await?;
            let items = rows
                .iter()
                .map(map_violation_record)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "items": items,
                "total": total,
                "page": page,
                "pageSize": page_size,
            }))
        })
    })
}

pub(in crate::repositories) fn violation_get(
    session: &RepositorySession,
    id: i64,
) -> Result<Option<Value>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "SELECT id,customer_name,customer_phone,order_id,violation_type,severity,
                        description,evidence,
                        financial_penalty::double precision AS financial_penalty,
                        reported_by,status,appeal_reason,appeal_at,reviewed_by,reviewed_at,
                        review_notes,created_at,updated_at
                 FROM violations
                 WHERE tenant_id=$1 AND id=$2 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(id)
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| map_violation_record(&row))
            .transpose()
        })
    })
}

pub(in crate::repositories) fn credit_history(
    session: &RepositorySession,
    customer_phone: &str,
) -> Result<Value, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let customer_phone = customer_phone.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT id,violation_type,severity,description,status,created_at
                 FROM violations
                 WHERE tenant_id=$1 AND customer_phone=$2
                 ORDER BY created_at DESC",
            )
            .bind(&tenant_id)
            .bind(&customer_phone)
            .fetch_all(&mut *connection)
            .await?;
            let history = rows
                .iter()
                .map(|row| -> Result<Value, sqlx::Error> {
                    let violation_type = row.try_get::<String, _>("violation_type")?;
                    let severity = row.try_get::<String, _>("severity")?;
                    Ok(json!({
                        "id": row.try_get::<i64, _>("id")?,
                        "eventType": "violation",
                        "violationType": violation_type,
                        "severity": severity,
                        "description": row.try_get::<String, _>("description")?,
                        "scoreImpact": -score_penalty(&violation_type, &severity),
                        "status": row.try_get::<String, _>("status")?,
                        "createdAt": row.try_get::<String, _>("created_at")?,
                    }))
                })
                .collect::<Result<Vec<_>, _>>()?;

            let current = sqlx::query(
                "SELECT score::bigint AS score,
                        on_time_returns::bigint AS on_time_returns,
                        late_returns::bigint AS late_returns,
                        damage_incidents::bigint AS damage_incidents
                 FROM credit_scores
                 WHERE tenant_id=$1 AND customer_phone=$2 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(&customer_phone)
            .fetch_optional(&mut *connection)
            .await?;

            let current_score = match current {
                Some(row) => json!({
                    "score": row.try_get::<i64, _>("score")?,
                    "onTimeReturns": row.try_get::<i64, _>("on_time_returns")?,
                    "lateReturns": row.try_get::<i64, _>("late_returns")?,
                    "damageIncidents": row.try_get::<i64, _>("damage_incidents")?,
                }),
                None => json!({
                    "score": 100,
                    "onTimeReturns": 0,
                    "lateReturns": 0,
                    "damageIncidents": 0,
                }),
            };

            Ok(json!({
                "currentScore": current_score,
                "history": history,
            }))
        })
    })
}

pub(in crate::repositories) fn check_before_order(
    session: &RepositorySession,
    customer_phone: &str,
) -> Result<Value, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let customer_phone = customer_phone.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let blacklist = sqlx::query(
                "SELECT severity,reason FROM blacklist
                 WHERE tenant_id=$1 AND customer_phone=$2 AND is_active=1",
            )
            .bind(&tenant_id)
            .bind(&customer_phone)
            .fetch_all(&mut *connection)
            .await?;
            let mut warnings = Vec::<String>::new();
            for row in blacklist {
                let severity = row.try_get::<String, _>("severity")?;
                let reason = row.try_get::<String, _>("reason")?;
                if severity == "critical" || severity == "high" {
                    return Ok(json!({
                        "allowed": false,
                        "blockReason": format!("黑名单({severity}): {reason}"),
                        "creditScore": Value::Null,
                        "creditWarning": Value::Null,
                        "warnings": [format!("该客户在黑名单中，严重程度: {severity}，原因: {reason}")],
                    }));
                }
                warnings.push(format!("该客户在黑名单中（{severity}）, 原因: {reason}"));
            }

            let score: Option<i64> = sqlx::query_scalar(
                "SELECT score::bigint FROM credit_scores
                 WHERE tenant_id=$1 AND customer_phone=$2 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(&customer_phone)
            .fetch_optional(&mut *connection)
            .await?;
            let (credit_score, credit_warning) = match score {
                Some(score) if score < 50 => {
                    return Ok(json!({
                        "allowed": false,
                        "blockReason": format!("信用评分过低({score})，拒绝下单"),
                        "creditScore": score,
                        "creditWarning": format!("信用评分极低({score}分)，建议人工审核"),
                        "warnings": [format!("信用评分 {score} 分，已低于最低标准 50 分")],
                    }));
                }
                Some(score) if score < 80 => {
                    (score, Some(format!("信用评分较低({score}分)，建议关注")))
                }
                Some(score) => (score, None),
                None => (100, None),
            };

            let pending = sqlx::query(
                "SELECT violation_type,severity FROM violations
                 WHERE tenant_id=$1 AND customer_phone=$2
                   AND severity IN ('major','critical')
                   AND status IN ('recorded','appealed','under_review')",
            )
            .bind(&tenant_id)
            .bind(&customer_phone)
            .fetch_all(&mut *connection)
            .await?;
            for row in pending {
                warnings.push(format!(
                    "未处理的严重违规: {} ({})",
                    row.try_get::<String, _>("violation_type")?,
                    row.try_get::<String, _>("severity")?,
                ));
            }

            Ok(json!({
                "allowed": true,
                "blockReason": Value::Null,
                "creditScore": credit_score,
                "creditWarning": credit_warning,
                "warnings": warnings,
            }))
        })
    })
}

fn push_violation_filters<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    customer_phone: Option<&'args str>,
    status: Option<&'args str>,
    violation_type: Option<&'args str>,
) {
    query.push("tenant_id=").push_bind(tenant_id);
    if let Some(customer_phone) = customer_phone {
        query.push(" AND customer_phone=").push_bind(customer_phone);
    }
    if let Some(status) = status {
        query.push(" AND status=").push_bind(status);
    }
    if let Some(violation_type) = violation_type {
        query.push(" AND violation_type=").push_bind(violation_type);
    }
}

fn actor_json(actor: String) -> Value {
    actor
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(actor))
}

fn order_json(order_id: Option<String>) -> Value {
    match order_id {
        Some(value) => value
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(value)),
        None => Value::Null,
    }
}

fn map_blacklist_record(row: &sqlx::postgres::PgRow) -> Result<Value, sqlx::Error> {
    Ok(json!({
        "id": row.try_get::<i64, _>("id")?,
        "customerName": row.try_get::<String, _>("customer_name")?,
        "customerPhone": row.try_get::<String, _>("customer_phone")?,
        "idNumber": row.try_get::<Option<String>, _>("id_number")?,
        "reason": row.try_get::<String, _>("reason")?,
        "severity": row.try_get::<String, _>("severity")?,
        "createdBy": actor_json(row.try_get::<String, _>("created_by")?),
        "isActive": row.try_get::<bool, _>("is_active")?,
        "removedAt": row.try_get::<Option<String>, _>("removed_at")?,
        "removedBy": row
            .try_get::<Option<String>, _>("removed_by")?
            .map(actor_json),
        "removalReason": row.try_get::<Option<String>, _>("removal_reason")?,
        "createdAt": row.try_get::<String, _>("created_at")?,
        "updatedAt": row.try_get::<String, _>("updated_at")?,
    }))
}

fn map_violation_record(row: &sqlx::postgres::PgRow) -> Result<Value, sqlx::Error> {
    Ok(json!({
        "id": row.try_get::<i64, _>("id")?,
        "customerName": row.try_get::<String, _>("customer_name")?,
        "customerPhone": row.try_get::<String, _>("customer_phone")?,
        "orderId": order_json(row.try_get::<Option<String>, _>("order_id")?),
        "violationType": row.try_get::<String, _>("violation_type")?,
        "severity": row.try_get::<String, _>("severity")?,
        "description": row.try_get::<String, _>("description")?,
        "evidence": row.try_get::<Option<String>, _>("evidence")?,
        "financialPenalty": row.try_get::<f64, _>("financial_penalty")?,
        "reportedBy": actor_json(row.try_get::<String, _>("reported_by")?),
        "status": row.try_get::<String, _>("status")?,
        "appealReason": row.try_get::<Option<String>, _>("appeal_reason")?,
        "appealAt": row.try_get::<Option<String>, _>("appeal_at")?,
        "reviewedBy": row
            .try_get::<Option<String>, _>("reviewed_by")?
            .map(actor_json),
        "reviewedAt": row.try_get::<Option<String>, _>("reviewed_at")?,
        "reviewNotes": row.try_get::<Option<String>, _>("review_notes")?,
        "createdAt": row.try_get::<String, _>("created_at")?,
        "updatedAt": row.try_get::<String, _>("updated_at")?,
    }))
}

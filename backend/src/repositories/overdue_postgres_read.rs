#![cfg(feature = "postgres")]

use serde_json::{Value, json};
use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::overdue::{OverdueFeeConfig, OverdueMutationError};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn calc(
    session: &RepositorySession,
    order_id: &str,
    today: &str,
) -> Result<Value, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let today = today.to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let config = load_config_pg(connection, &tenant).await?;
            let row = sqlx::query(
                "SELECT o.enddate,l.fulfilment_status
                   FROM orders o
                   JOIN order_lifecycle l
                     ON l.tenant_id=o.tenant_id AND l.order_id=o.id
                  WHERE o.tenant_id=$1 AND o.id=$2",
            )
            .bind(&tenant)
            .bind(&order_id)
            .fetch_one(&mut *connection)
            .await?;
            let expected_date = row.try_get::<String, _>("enddate")?;
            let fulfilment_status = row.try_get::<String, _>("fulfilment_status")?;
            let days =
                crate::repositories::overdue::days_between_public(&expected_date, &today).max(0);
            let raw_fee = days as f64 * config.daily_rate;
            let cap_amount = config.daily_rate * config.max_days as f64 * config.cap_multiplier;
            let actual_fee = raw_fee.min(cap_amount).max(0.0);
            Ok(json!({
                "orderId": order_id,
                "daysOverdue": days,
                "dailyRate": config.daily_rate,
                "totalFee": raw_fee,
                "capAmount": cap_amount,
                "actualFee": actual_fee,
                "orderStatus": fulfilment_status,
            }))
        })
    })
}

pub(in crate::repositories) fn apply_delegated(
    session: &RepositorySession,
    overdue_id: i64,
) -> Result<Value, OverdueMutationError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    let row = session
        .pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT order_id,
                            total_fee::double precision AS total_fee,
                            waived_amount::double precision AS waived_amount,
                            paid_amount::double precision AS paid_amount,
                            status
                       FROM overdue_records
                      WHERE tenant_id=$1 AND id=$2",
                )
                .bind(&tenant)
                .bind(overdue_id)
                .fetch_optional(&mut *connection)
                .await?;
                row.map(|row| {
                    Ok((
                        row.try_get::<String, _>("order_id")?,
                        row.try_get::<f64, _>("total_fee")?,
                        row.try_get::<f64, _>("waived_amount")?,
                        row.try_get::<f64, _>("paid_amount")?,
                        row.try_get::<String, _>("status")?,
                    ))
                })
                .transpose()
            })
        })
        .map_err(OverdueMutationError::Storage)?
        .ok_or(OverdueMutationError::NotFound)?;

    let (order_id, total, waived, paid, status) = row;
    if matches!(status.as_str(), "waived" | "paid" | "cancelled") {
        return Err(OverdueMutationError::StatusInvalid(status));
    }
    let requested = (total - waived - paid).max(0.0);
    Ok(json!({
        "overdueId": overdue_id,
        "orderId": order_id,
        "requestedAmount": requested,
        "appliedAmount": 0.0,
        "financialEffectApplied": false,
        "settlementAuthority": "r3_settlement",
        "status": status,
    }))
}

pub(in crate::repositories) fn list(
    session: &RepositorySession,
    status: Option<&str>,
    customer_phone: Option<&str>,
    order_id: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Value, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    let status = nonblank(status);
    let phone = nonblank(customer_phone);
    let order = nonblank(order_id);
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;
            let mut count = QueryBuilder::<Postgres>::new(
                "SELECT COUNT(*)::bigint FROM overdue_records WHERE ",
            );
            push_filters(
                &mut count,
                &tenant,
                status.as_deref(),
                phone.as_deref(),
                order.as_deref(),
            );
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut query = QueryBuilder::<Postgres>::new(
                "SELECT id,order_id,customer_id,customer_name,customer_phone,
                        expected_return_date,actual_return_date,
                        days_overdue::bigint AS days_overdue,
                        daily_rate::double precision AS daily_rate,
                        total_fee::double precision AS total_fee,
                        waived_amount::double precision AS waived_amount,
                        paid_amount::double precision AS paid_amount,
                        status,escalation_level::bigint AS escalation_level,
                        last_escalation_at,waived_by,waived_reason,created_at,updated_at
                   FROM overdue_records WHERE ",
            );
            push_filters(
                &mut query,
                &tenant,
                status.as_deref(),
                phone.as_deref(),
                order.as_deref(),
            );
            query
                .push(" ORDER BY created_at DESC,id DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);
            let rows = query.build().fetch_all(&mut *connection).await?;
            let items = rows.iter().map(row_json).collect::<Result<Vec<_>, _>>()?;

            Ok(json!({
                "items": items,
                "total": total,
                "page": page,
                "pageSize": page_size,
            }))
        })
    })
}

pub(in crate::repositories) fn get(
    session: &RepositorySession,
    overdue_id: i64,
) -> Result<Option<Value>, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "SELECT id,order_id,customer_id,customer_name,customer_phone,
                        expected_return_date,actual_return_date,
                        days_overdue::bigint AS days_overdue,
                        daily_rate::double precision AS daily_rate,
                        total_fee::double precision AS total_fee,
                        waived_amount::double precision AS waived_amount,
                        paid_amount::double precision AS paid_amount,
                        status,escalation_level::bigint AS escalation_level,
                        last_escalation_at,waived_by,waived_reason,created_at,updated_at
                   FROM overdue_records WHERE tenant_id=$1 AND id=$2",
            )
            .bind(&tenant)
            .bind(overdue_id)
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| row_json(&row))
            .transpose()
        })
    })
}

pub(in crate::repositories) fn config_get(
    session: &RepositorySession,
) -> Result<OverdueFeeConfig, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let row = sqlx::query(
                "SELECT daily_rate::double precision AS daily_rate,
                        max_days::bigint AS max_days,
                        cap_multiplier::double precision AS cap_multiplier,
                        grace_period_hours::bigint AS grace_period_hours
                   FROM overdue_fee_config WHERE tenant_id=$1 LIMIT 1",
            )
            .bind(&tenant)
            .fetch_optional(&mut *connection)
            .await?;
            match row {
                Some(row) => Ok(OverdueFeeConfig {
                    daily_rate: row.try_get("daily_rate")?,
                    max_days: row.try_get("max_days")?,
                    cap_multiplier: row.try_get("cap_multiplier")?,
                    grace_period_hours: row.try_get("grace_period_hours")?,
                }),
                None => Ok(OverdueFeeConfig::default()),
            }
        })
    })
}

pub(in crate::repositories) fn escalation_history(
    session: &RepositorySession,
    overdue_id: i64,
) -> Result<Vec<Value>, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT id,overdue_id,escalation_level::bigint AS escalation_level,
                        channel,sent_at
                   FROM overdue_notification_log
                  WHERE tenant_id=$1 AND overdue_id=$2
                  ORDER BY escalation_level,id",
            )
            .bind(&tenant)
            .bind(overdue_id)
            .fetch_all(&mut *connection)
            .await?;
            rows.iter()
                .map(|row| {
                    Ok(json!({
                        "id": row.try_get::<i64, _>("id")?,
                        "overdueId": row.try_get::<i64, _>("overdue_id")?,
                        "escalationLevel": row.try_get::<i64, _>("escalation_level")?,
                        "channel": row.try_get::<String, _>("channel")?,
                        "sentAt": row.try_get::<String, _>("sent_at")?,
                    }))
                })
                .collect()
        })
    })
}

pub(in crate::repositories) fn stats(
    session: &RepositorySession,
) -> Result<Value, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let totals = sqlx::query(
                "SELECT COUNT(*)::bigint AS total_active,
                        COALESCE(SUM(total_fee-waived_amount-paid_amount),0)::double precision
                            AS total_amount
                   FROM overdue_records
                  WHERE tenant_id=$1
                    AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')",
            )
            .bind(&tenant)
            .fetch_one(&mut *connection)
            .await?;
            let total_active = totals.try_get::<i64, _>("total_active")?;
            let total_amount = totals.try_get::<f64, _>("total_amount")?;

            let by_rows = sqlx::query(
                "SELECT escalation_level::bigint AS escalation_level,
                        COUNT(*)::bigint AS count,
                        COALESCE(SUM(total_fee-waived_amount-paid_amount),0)::double precision
                            AS total_amount
                   FROM overdue_records
                  WHERE tenant_id=$1
                    AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')
                  GROUP BY escalation_level ORDER BY escalation_level",
            )
            .bind(&tenant)
            .fetch_all(&mut *connection)
            .await?;
            let mut by_escalation = serde_json::Map::new();
            for row in by_rows {
                let level = row.try_get::<i64, _>("escalation_level")?;
                by_escalation.insert(
                    format!("level_{level}"),
                    json!({
                        "escalationLevel": level,
                        "count": row.try_get::<i64, _>("count")?,
                        "totalAmount": row.try_get::<f64, _>("total_amount")?,
                    }),
                );
            }

            let top_rows = sqlx::query(
                "SELECT id,order_id,customer_name,customer_phone,
                        days_overdue::bigint AS days_overdue,
                        total_fee::double precision AS total_fee,status
                   FROM overdue_records
                  WHERE tenant_id=$1
                    AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')
                  ORDER BY days_overdue DESC,id DESC LIMIT 5",
            )
            .bind(&tenant)
            .fetch_all(&mut *connection)
            .await?;
            let top_overdue = top_rows
                .iter()
                .map(|row| {
                    Ok(json!({
                        "id": row.try_get::<i64, _>("id")?,
                        "orderId": row.try_get::<String, _>("order_id")?,
                        "customerName": row.try_get::<String, _>("customer_name")?,
                        "customerPhone": row.try_get::<String, _>("customer_phone")?,
                        "daysOverdue": row.try_get::<i64, _>("days_overdue")?,
                        "totalFee": row.try_get::<f64, _>("total_fee")?,
                        "status": row.try_get::<String, _>("status")?,
                    }))
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;

            Ok(json!({
                "totalActive": total_active,
                "totalAmount": total_amount,
                "byEscalationLevel": by_escalation,
                "topOverdue": top_overdue,
            }))
        })
    })
}

pub(in crate::repositories) fn check_before_order(
    session: &RepositorySession,
    customer_phone: &str,
) -> Result<Value, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    let phone = customer_phone.to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT id,order_id,days_overdue::bigint AS days_overdue,
                        total_fee::double precision AS total_fee,
                        waived_amount::double precision AS waived_amount,
                        paid_amount::double precision AS paid_amount,status
                   FROM overdue_records
                  WHERE tenant_id=$1 AND customer_phone=$2
                    AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')
                  ORDER BY days_overdue DESC,id DESC",
            )
            .bind(&tenant)
            .bind(&phone)
            .fetch_all(&mut *connection)
            .await?;

            let mut active = Vec::new();
            let mut total_unpaid = 0.0;
            let mut has_severe = false;
            for row in rows {
                let status = row.try_get::<String, _>("status")?;
                let total = row.try_get::<f64, _>("total_fee")?;
                let waived = row.try_get::<f64, _>("waived_amount")?;
                let paid = row.try_get::<f64, _>("paid_amount")?;
                total_unpaid += (total - waived - paid).max(0.0);
                has_severe |= status == "escalated_d7";
                active.push(json!({
                    "id": row.try_get::<i64, _>("id")?,
                    "orderId": row.try_get::<String, _>("order_id")?,
                    "daysOverdue": row.try_get::<i64, _>("days_overdue")?,
                    "totalFee": total,
                    "waivedAmount": waived,
                    "paidAmount": paid,
                    "status": status,
                }));
            }

            Ok(json!({
                "hasOverdue": !active.is_empty(),
                "activeRecords": active,
                "totalUnpaid": total_unpaid,
                "blockReason": if has_severe {
                    Some("该客户存在严重逾期记录(已进入法律告知阶段), 请先处理逾期再下单")
                } else {
                    None
                },
                "allowed": !has_severe,
            }))
        })
    })
}

fn push_filters<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant: &'args str,
    status: Option<&'args str>,
    phone: Option<&'args str>,
    order_id: Option<&'args str>,
) {
    query.push("tenant_id=").push_bind(tenant);
    if let Some(status) = status {
        query.push(" AND status=").push_bind(status);
    }
    if let Some(phone) = phone {
        query.push(" AND customer_phone=").push_bind(phone);
    }
    if let Some(order_id) = order_id {
        query.push(" AND order_id=").push_bind(order_id);
    }
}

fn row_json(row: &sqlx::postgres::PgRow) -> Result<Value, sqlx::Error> {
    Ok(json!({
        "id": row.try_get::<i64, _>("id")?,
        "orderId": row.try_get::<String, _>("order_id")?,
        "customerId": row.try_get::<Option<String>, _>("customer_id")?,
        "customerName": row.try_get::<String, _>("customer_name")?,
        "customerPhone": row.try_get::<String, _>("customer_phone")?,
        "expectedReturnDate": row.try_get::<String, _>("expected_return_date")?,
        "actualReturnDate": row.try_get::<Option<String>, _>("actual_return_date")?,
        "daysOverdue": row.try_get::<i64, _>("days_overdue")?,
        "dailyRate": row.try_get::<f64, _>("daily_rate")?,
        "totalFee": row.try_get::<f64, _>("total_fee")?,
        "waivedAmount": row.try_get::<f64, _>("waived_amount")?,
        "paidAmount": row.try_get::<f64, _>("paid_amount")?,
        "status": row.try_get::<String, _>("status")?,
        "escalationLevel": row.try_get::<i64, _>("escalation_level")?,
        "lastEscalationAt": row.try_get::<Option<String>, _>("last_escalation_at")?,
        "waivedBy": row.try_get::<Option<String>, _>("waived_by")?,
        "waivedReason": row.try_get::<Option<String>, _>("waived_reason")?,
        "createdAt": row.try_get::<String, _>("created_at")?,
        "updatedAt": row.try_get::<String, _>("updated_at")?,
    }))
}

async fn load_config_pg(
    connection: &mut sqlx::PgConnection,
    tenant: &str,
) -> Result<OverdueFeeConfig, sqlx::Error> {
    let row = sqlx::query(
        "SELECT daily_rate::double precision AS daily_rate,
                max_days::bigint AS max_days,
                cap_multiplier::double precision AS cap_multiplier,
                grace_period_hours::bigint AS grace_period_hours
           FROM overdue_fee_config WHERE tenant_id=$1 LIMIT 1",
    )
    .bind(tenant)
    .fetch_optional(&mut *connection)
    .await?;
    match row {
        Some(row) => Ok(OverdueFeeConfig {
            daily_rate: row.try_get("daily_rate")?,
            max_days: row.try_get("max_days")?,
            cap_multiplier: row.try_get("cap_multiplier")?,
            grace_period_hours: row.try_get("grace_period_hours")?,
        }),
        None => Ok(OverdueFeeConfig::default()),
    }
}

fn nonblank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

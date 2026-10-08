#![cfg(feature = "postgres")]

use serde_json::{Value, json};
use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::overdue::{
    OverdueFeeConfig, OverdueFeeConfigPatch, OverdueMutationError, contract, map_mutation_error,
};
use crate::repositories::overdue_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn detect(
    session: &RepositorySession,
    today: &str,
    now: &str,
) -> Result<Value, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    let today = today.to_owned();
    let now = now.to_owned();

    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
                .bind(format!("overdue-detect:{tenant}"))
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

            let config = load_config_pg(connection, &tenant).await?;
            let candidates = sqlx::query(
                "SELECT o.id,o.customer_id,o.enddate,c.display_name,
                        (SELECT cc.raw_value
                           FROM customer_contacts cc
                          WHERE cc.tenant_id=o.tenant_id
                            AND cc.customer_id=o.customer_id
                            AND cc.kind='phone'
                          ORDER BY cc.is_primary DESC,cc.created_at,cc.id
                          LIMIT 1) AS customer_phone
                   FROM orders o
                   JOIN order_lifecycle l
                     ON l.tenant_id=o.tenant_id AND l.order_id=o.id
                   JOIN customers c
                     ON c.tenant_id=o.tenant_id AND c.id=o.customer_id
                  WHERE o.tenant_id=$1
                    AND o.customer_id IS NOT NULL
                    AND o.enddate < $2
                    AND l.commercial_status NOT IN ('closed','cancelled')
                    AND l.fulfilment_status IN ('shipped','in_use','return_pending')
                  ORDER BY o.enddate,o.id",
            )
            .bind(&tenant)
            .bind(&today)
            .fetch_all(&mut *connection)
            .await
            .map_err(pg_error)?;

            let mut records = Vec::new();
            let mut skipped_unresolved_contact = 0_i64;
            for row in candidates {
                let order_id = row.try_get::<String, _>("id").map_err(pg_error)?;
                let customer_id = row.try_get::<String, _>("customer_id").map_err(pg_error)?;
                let expected_date = row.try_get::<String, _>("enddate").map_err(pg_error)?;
                let customer_name = row.try_get::<String, _>("display_name").map_err(pg_error)?;
                let phone = row
                    .try_get::<Option<String>, _>("customer_phone")
                    .map_err(pg_error)?;
                let Some(customer_phone) = phone.filter(|value| !value.trim().is_empty()) else {
                    skipped_unresolved_contact += 1;
                    continue;
                };

                let days =
                    crate::repositories::overdue::days_between_public(&expected_date, &today)
                        .max(0);
                let (capped_days, total_fee) =
                    crate::repositories::overdue::calculate_fee_public(days, &config);

                let existing = sqlx::query(
                    "SELECT id,status,escalation_level::bigint AS escalation_level
                       FROM overdue_records
                      WHERE tenant_id=$1 AND order_id=$2
                        AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')
                      ORDER BY id DESC LIMIT 1
                      FOR UPDATE",
                )
                .bind(&tenant)
                .bind(&order_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;

                let (record_id, action, status, level) = if let Some(existing) = existing {
                    let record_id = existing.try_get::<i64, _>("id").map_err(pg_error)?;
                    let status = existing.try_get::<String, _>("status").map_err(pg_error)?;
                    let level = existing
                        .try_get::<i64, _>("escalation_level")
                        .map_err(pg_error)?;
                    sqlx::query(
                        "UPDATE overdue_records
                            SET customer_id=$1,customer_name=$2,customer_phone=$3,
                                expected_return_date=$4,days_overdue=$5,daily_rate=$6,
                                total_fee=$7,updated_at=$8
                          WHERE tenant_id=$9 AND id=$10",
                    )
                    .bind(&customer_id)
                    .bind(&customer_name)
                    .bind(&customer_phone)
                    .bind(&expected_date)
                    .bind(capped_days as i32)
                    .bind(config.daily_rate)
                    .bind(total_fee)
                    .bind(&now)
                    .bind(&tenant)
                    .bind(record_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    (record_id, "updated", status, level)
                } else {
                    let record_id: i64 = sqlx::query_scalar(
                        "INSERT INTO overdue_records
                         (tenant_id,order_id,customer_id,customer_name,customer_phone,
                          expected_return_date,days_overdue,daily_rate,total_fee,status,
                          created_at,updated_at)
                         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'active',$10,$10)
                         RETURNING id",
                    )
                    .bind(&tenant)
                    .bind(&order_id)
                    .bind(&customer_id)
                    .bind(&customer_name)
                    .bind(&customer_phone)
                    .bind(&expected_date)
                    .bind(capped_days as i32)
                    .bind(config.daily_rate)
                    .bind(total_fee)
                    .bind(&now)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    (record_id, "created", "active".into(), 0)
                };

                let escalation = if days >= 1 && config.grace_period_hours == 0 {
                    escalate_one_pg(connection, &tenant, record_id, &order_id, days, level, &now)
                        .await?
                } else {
                    None
                };

                records.push(json!({
                    "id": record_id,
                    "orderId": order_id,
                    "customerName": customer_name,
                    "customerPhone": customer_phone,
                    "expectedReturnDate": expected_date,
                    "daysOverdue": capped_days,
                    "totalFee": total_fee,
                    "action": action,
                    "previousStatus": status,
                    "escalation": escalation,
                }));
            }

            Ok(json!({
                "detected": records.len(),
                "records": records,
                "scannedAt": now,
                "skippedUnresolvedContact": skipped_unresolved_contact,
            }))
        })
    })
}

pub(in crate::repositories) fn waive(
    session: &RepositorySession,
    overdue_id: i64,
    reason: &str,
    actor_identity_id: &str,
    now: &str,
) -> Result<Value, OverdueMutationError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    let reason = reason.to_owned();
    let actor = actor_identity_id.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT status,total_fee::double precision AS total_fee
                       FROM overdue_records
                      WHERE tenant_id=$1 AND id=$2
                      FOR UPDATE",
                )
                .bind(&tenant)
                .bind(overdue_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("overdue-not-found".into()))?;
                let status = row.try_get::<String, _>("status").map_err(pg_error)?;
                let total_fee = row.try_get::<f64, _>("total_fee").map_err(pg_error)?;
                if status == "waived" {
                    return Err(contract("overdue-already-waived".into()));
                }
                if matches!(status.as_str(), "paid" | "cancelled") {
                    return Err(contract(format!("overdue-status-invalid:{status}")));
                }

                sqlx::query(
                    "UPDATE overdue_records
                        SET waived_amount=$1,status='waived',waived_by=$2,waived_reason=$3,
                            updated_at=$4
                      WHERE tenant_id=$5 AND id=$6",
                )
                .bind(total_fee)
                .bind(&actor)
                .bind(&reason)
                .bind(&now)
                .bind(&tenant)
                .bind(overdue_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(json!({
                    "overdueId": overdue_id,
                    "waivedAmount": total_fee,
                    "waivedBy": actor,
                    "waivedAt": now,
                }))
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn config_upsert(
    session: &RepositorySession,
    patch: OverdueFeeConfigPatch,
    now: &str,
) -> Result<OverdueFeeConfig, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    let now = now.to_owned();
    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            let current = load_config_pg(connection, &tenant).await?;
            let next = patch.apply_public(current);
            sqlx::query(
                "INSERT INTO overdue_fee_config
                 (tenant_id,daily_rate,max_days,cap_multiplier,grace_period_hours,created_at,updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$6)
                 ON CONFLICT(tenant_id) DO UPDATE SET
                   daily_rate=EXCLUDED.daily_rate,
                   max_days=EXCLUDED.max_days,
                   cap_multiplier=EXCLUDED.cap_multiplier,
                   grace_period_hours=EXCLUDED.grace_period_hours,
                   updated_at=EXCLUDED.updated_at",
            )
            .bind(&tenant)
            .bind(next.daily_rate)
            .bind(next.max_days as i32)
            .bind(next.cap_multiplier)
            .bind(next.grace_period_hours as i32)
            .bind(&now)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;
            Ok(next)
        })
    })
}

pub(in crate::repositories) fn escalate(
    session: &RepositorySession,
    now: &str,
) -> Result<Value, RepositoryError> {
    let tenant = session.binding().tenant_id().as_str().to_owned();
    let now = now.to_owned();
    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT id,order_id,days_overdue::bigint AS days_overdue,
                        status,escalation_level::bigint AS escalation_level
                   FROM overdue_records
                  WHERE tenant_id=$1
                    AND status IN ('active','escalated_d1','escalated_d3')
                  ORDER BY id
                  FOR UPDATE",
            )
            .bind(&tenant)
            .fetch_all(&mut *connection)
            .await
            .map_err(pg_error)?;

            let mut results = Vec::new();
            for row in rows {
                let id = row.try_get::<i64, _>("id").map_err(pg_error)?;
                let order_id = row.try_get::<String, _>("order_id").map_err(pg_error)?;
                let days = row.try_get::<i64, _>("days_overdue").map_err(pg_error)?;
                let level = row
                    .try_get::<i64, _>("escalation_level")
                    .map_err(pg_error)?;
                let result =
                    escalate_one_pg(connection, &tenant, id, &order_id, days, level, &now).await?;
                results.push(result.unwrap_or_else(|| {
                    json!({
                        "overdueId": id,
                        "orderId": order_id,
                        "daysOverdue": days,
                        "previousLevel": level,
                        "action": "none",
                    })
                }));
            }
            let escalated = results
                .iter()
                .filter(|value| value.get("action").and_then(Value::as_str) == Some("escalated"))
                .count();
            Ok(json!({
                "escalated": escalated,
                "checked": results.len(),
                "results": results,
            }))
        })
    })
}

async fn load_config_pg(
    connection: &mut sqlx::PgConnection,
    tenant: &str,
) -> Result<OverdueFeeConfig, RepositoryError> {
    let row = sqlx::query(
        "SELECT daily_rate::double precision AS daily_rate,
                max_days::bigint AS max_days,
                cap_multiplier::double precision AS cap_multiplier,
                grace_period_hours::bigint AS grace_period_hours
           FROM overdue_fee_config WHERE tenant_id=$1 LIMIT 1",
    )
    .bind(tenant)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    match row {
        Some(row) => Ok(OverdueFeeConfig {
            daily_rate: row.try_get("daily_rate").map_err(pg_error)?,
            max_days: row.try_get("max_days").map_err(pg_error)?,
            cap_multiplier: row.try_get("cap_multiplier").map_err(pg_error)?,
            grace_period_hours: row.try_get("grace_period_hours").map_err(pg_error)?,
        }),
        None => Ok(OverdueFeeConfig::default()),
    }
}

async fn escalate_one_pg(
    connection: &mut sqlx::PgConnection,
    tenant: &str,
    overdue_id: i64,
    order_id: &str,
    days: i64,
    level: i64,
    now: &str,
) -> Result<Option<Value>, RepositoryError> {
    let Some((new_level, new_status, channel)) =
        crate::repositories::overdue::target_escalation_public(days, level)
    else {
        return Ok(None);
    };

    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM overdue_notification_log
          WHERE tenant_id=$1 AND overdue_id=$2 AND escalation_level=$3 LIMIT 1",
    )
    .bind(tenant)
    .bind(overdue_id)
    .bind(new_level as i32)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    if existing.is_some() {
        return Ok(Some(json!({
            "overdueId": overdue_id,
            "orderId": order_id,
            "daysOverdue": days,
            "previousLevel": level,
            "targetLevel": new_level,
            "action": "skipped_duplicate",
        })));
    }

    sqlx::query(
        "UPDATE overdue_records
            SET status=$1,escalation_level=$2,last_escalation_at=$3,updated_at=$3
          WHERE tenant_id=$4 AND id=$5",
    )
    .bind(new_status)
    .bind(new_level as i32)
    .bind(now)
    .bind(tenant)
    .bind(overdue_id)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?;
    sqlx::query(
        "INSERT INTO overdue_notification_log
         (tenant_id,overdue_id,escalation_level,channel,sent_at)
         VALUES ($1,$2,$3,$4,$5)
         ON CONFLICT(tenant_id,overdue_id,escalation_level) DO NOTHING",
    )
    .bind(tenant)
    .bind(overdue_id)
    .bind(new_level as i32)
    .bind(channel)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?;

    Ok(Some(json!({
        "overdueId": overdue_id,
        "orderId": order_id,
        "daysOverdue": days,
        "previousLevel": level,
        "newLevel": new_level,
        "newStatus": new_status,
        "channel": channel,
        "action": "escalated",
        "escalatedAt": now,
    })))
}

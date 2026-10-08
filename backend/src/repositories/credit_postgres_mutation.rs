#![cfg(feature = "postgres")]

use serde_json::{Value, json};
use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::credit::{
    BlacklistAddOutcome, BlacklistRemoveOutcome, CreditMutationError, CreditRecalculateOutcome,
    ViolationRecordOutcome, ViolationTransitionOutcome, contract, map_mutation_error, score_label,
    score_penalty,
};
use crate::repositories::credit_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

#[allow(clippy::too_many_arguments)]
pub(in crate::repositories) fn blacklist_add(
    session: &RepositorySession,
    customer_name: &str,
    customer_phone: &str,
    id_number: Option<&str>,
    reason: &str,
    severity: &str,
    actor_identity_id: &str,
    now: &str,
) -> Result<BlacklistAddOutcome, CreditMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let customer_name = customer_name.to_owned();
    let customer_phone = customer_phone.to_owned();
    let id_number = id_number.map(str::to_owned);
    let reason = reason.to_owned();
    let severity = severity.to_owned();
    let actor_identity_id = actor_identity_id.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let lock_key = format!("credit-blacklist:{tenant_id}:{customer_phone}");
                sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
                    .bind(lock_key)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                let existing: Option<i64> = sqlx::query_scalar(
                    "SELECT id FROM blacklist
                     WHERE tenant_id=$1 AND customer_phone=$2 AND is_active=1
                     LIMIT 1",
                )
                .bind(&tenant_id)
                .bind(&customer_phone)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if existing.is_some() {
                    return Err(contract("credit-blacklist-duplicate".into()));
                }

                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO blacklist
                     (tenant_id,customer_name,customer_phone,id_number,reason,severity,
                      created_by,is_active,created_at,updated_at)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,1,$8,$8)
                     RETURNING id",
                )
                .bind(&tenant_id)
                .bind(&customer_name)
                .bind(&customer_phone)
                .bind(&id_number)
                .bind(&reason)
                .bind(&severity)
                .bind(&actor_identity_id)
                .bind(&now)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(BlacklistAddOutcome {
                    id,
                    created_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn blacklist_remove(
    session: &RepositorySession,
    id: i64,
    removal_reason: &str,
    actor_identity_id: &str,
    now: &str,
) -> Result<BlacklistRemoveOutcome, CreditMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let removal_reason = removal_reason.to_owned();
    let actor_identity_id = actor_identity_id.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let active: Option<i32> = sqlx::query_scalar(
                    "SELECT is_active FROM blacklist
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1 FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                let active = active.ok_or_else(|| contract("credit-blacklist-not-found".into()))?;
                if active == 0 {
                    return Err(contract("credit-blacklist-already-removed".into()));
                }

                sqlx::query(
                    "UPDATE blacklist
                     SET is_active=0,removed_at=$1,removed_by=$2,removal_reason=$3,updated_at=$1
                     WHERE tenant_id=$4 AND id=$5 AND is_active=1",
                )
                .bind(&now)
                .bind(&actor_identity_id)
                .bind(&removal_reason)
                .bind(&tenant_id)
                .bind(id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(BlacklistRemoveOutcome {
                    id,
                    removed_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::repositories) fn violation_record(
    session: &RepositorySession,
    customer_name: &str,
    customer_phone: &str,
    order_id: Option<i64>,
    violation_type: &str,
    severity: &str,
    description: &str,
    evidence: Option<&str>,
    financial_penalty: f64,
    actor_identity_id: &str,
    now: &str,
) -> Result<ViolationRecordOutcome, CreditMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let customer_name = customer_name.to_owned();
    let customer_phone = customer_phone.to_owned();
    let order_id = order_id.map(|value| value.to_string());
    let violation_type = violation_type.to_owned();
    let severity = severity.to_owned();
    let description = description.to_owned();
    let evidence = evidence.map(str::to_owned);
    let actor_identity_id = actor_identity_id.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO violations
                     (tenant_id,customer_name,customer_phone,order_id,violation_type,severity,
                      description,evidence,financial_penalty,reported_by,status,created_at,updated_at)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'recorded',$11,$11)
                     RETURNING id",
                )
                .bind(&tenant_id)
                .bind(&customer_name)
                .bind(&customer_phone)
                .bind(&order_id)
                .bind(&violation_type)
                .bind(&severity)
                .bind(&description)
                .bind(&evidence)
                .bind(financial_penalty)
                .bind(&actor_identity_id)
                .bind(&now)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;

                let penalty = score_penalty(&violation_type, &severity);
                ensure_credit_score_pg(
                    connection,
                    &tenant_id,
                    &customer_name,
                    &customer_phone,
                    &now,
                )
                .await?;
                sqlx::query(
                    "UPDATE credit_scores
                     SET damage_incidents=damage_incidents+1,
                         score=GREATEST(0,score-$1),
                         updated_at=$2,last_calculated_at=$2
                     WHERE tenant_id=$3 AND customer_phone=$4",
                )
                .bind(penalty)
                .bind(&now)
                .bind(&tenant_id)
                .bind(&customer_phone)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(ViolationRecordOutcome {
                    id,
                    penalty,
                    created_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn violation_appeal(
    session: &RepositorySession,
    id: i64,
    appeal_reason: &str,
    now: &str,
) -> Result<ViolationTransitionOutcome, CreditMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let appeal_reason = appeal_reason.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let current: Option<String> = sqlx::query_scalar(
                    "SELECT status FROM violations
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1 FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                let current =
                    current.ok_or_else(|| contract("credit-violation-not-found".into()))?;
                if current != "recorded" {
                    return Err(contract(format!("credit-status-invalid:{current}")));
                }

                sqlx::query(
                    "UPDATE violations
                     SET status='appealed',appeal_reason=$1,appeal_at=$2,updated_at=$2
                     WHERE tenant_id=$3 AND id=$4",
                )
                .bind(&appeal_reason)
                .bind(&now)
                .bind(&tenant_id)
                .bind(id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;
                Ok(ViolationTransitionOutcome {
                    id,
                    status: "appealed".into(),
                    at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn violation_review(
    session: &RepositorySession,
    id: i64,
    status: &str,
    review_notes: &str,
    actor_identity_id: &str,
    now: &str,
) -> Result<ViolationTransitionOutcome, CreditMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let status = status.to_owned();
    let review_notes = review_notes.to_owned();
    let actor_identity_id = actor_identity_id.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let current: Option<String> = sqlx::query_scalar(
                    "SELECT status FROM violations
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1 FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                let current =
                    current.ok_or_else(|| contract("credit-violation-not-found".into()))?;
                if current != "appealed" && current != "under_review" {
                    return Err(contract(format!("credit-status-invalid:{current}")));
                }

                sqlx::query(
                    "UPDATE violations
                     SET status=$1,reviewed_by=$2,reviewed_at=$3,review_notes=$4,updated_at=$3
                     WHERE tenant_id=$5 AND id=$6",
                )
                .bind(&status)
                .bind(&actor_identity_id)
                .bind(&now)
                .bind(&review_notes)
                .bind(&tenant_id)
                .bind(id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;
                Ok(ViolationTransitionOutcome {
                    id,
                    status,
                    at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn credit_get(
    session: &RepositorySession,
    customer_phone: &str,
    now: &str,
) -> Result<Value, CreditMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let customer_phone = customer_phone.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                ensure_credit_score_pg(
                    connection,
                    &tenant_id,
                    &customer_phone,
                    &customer_phone,
                    &now,
                )
                .await?;

                let row = sqlx::query(
                    "SELECT id,customer_name,customer_phone,
                            score::bigint AS score,
                            total_orders::bigint AS total_orders,
                            on_time_returns::bigint AS on_time_returns,
                            late_returns::bigint AS late_returns,
                            damage_incidents::bigint AS damage_incidents,
                            last_calculated_at,created_at,updated_at
                     FROM credit_scores
                     WHERE tenant_id=$1 AND customer_phone=$2 LIMIT 1",
                )
                .bind(&tenant_id)
                .bind(&customer_phone)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;
                Ok(map_credit_record(&row)?)
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn credit_recalculate(
    session: &RepositorySession,
    customer_phone: &str,
    now: &str,
) -> Result<CreditRecalculateOutcome, CreditMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let customer_phone = customer_phone.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT on_time_returns::bigint AS on_time_returns,
                            late_returns::bigint AS late_returns,
                            damage_incidents::bigint AS damage_incidents
                     FROM credit_scores
                     WHERE tenant_id=$1 AND customer_phone=$2
                     LIMIT 1 FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&customer_phone)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("credit-profile-not-found".into()))?;
                let on_time_returns = row.try_get::<i64, _>("on_time_returns").map_err(pg_error)?;
                let late_returns = row.try_get::<i64, _>("late_returns").map_err(pg_error)?;
                let damage_incidents = row
                    .try_get::<i64, _>("damage_incidents")
                    .map_err(pg_error)?;
                let score =
                    (100_i64 + on_time_returns * 2 - late_returns * 5 - damage_incidents * 10)
                        .clamp(0, 200);

                sqlx::query(
                    "UPDATE credit_scores
                     SET score=$1,last_calculated_at=$2,updated_at=$2
                     WHERE tenant_id=$3 AND customer_phone=$4",
                )
                .bind(score)
                .bind(&now)
                .bind(&tenant_id)
                .bind(&customer_phone)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(CreditRecalculateOutcome {
                    customer_phone,
                    score,
                    on_time_returns,
                    late_returns,
                    damage_incidents,
                    recalculated_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

async fn ensure_credit_score_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    customer_name: &str,
    customer_phone: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    sqlx::query(
        "INSERT INTO credit_scores
         (tenant_id,customer_name,customer_phone,score,total_orders,on_time_returns,
          late_returns,damage_incidents,last_calculated_at,created_at,updated_at)
         VALUES ($1,$2,$3,100,0,0,0,0,$4,$4,$4)
         ON CONFLICT (tenant_id,customer_phone) DO NOTHING",
    )
    .bind(tenant_id)
    .bind(customer_name)
    .bind(customer_phone)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?;
    Ok(())
}

fn map_credit_record(row: &sqlx::postgres::PgRow) -> Result<Value, RepositoryError> {
    let score = row.try_get::<i64, _>("score").map_err(pg_error)?;
    Ok(json!({
        "id": row.try_get::<i64, _>("id").map_err(pg_error)?,
        "customerName": row.try_get::<String, _>("customer_name").map_err(pg_error)?,
        "customerPhone": row.try_get::<String, _>("customer_phone").map_err(pg_error)?,
        "score": score,
        "totalOrders": row.try_get::<i64, _>("total_orders").map_err(pg_error)?,
        "onTimeReturns": row.try_get::<i64, _>("on_time_returns").map_err(pg_error)?,
        "lateReturns": row.try_get::<i64, _>("late_returns").map_err(pg_error)?,
        "damageIncidents": row.try_get::<i64, _>("damage_incidents").map_err(pg_error)?,
        "lastCalculatedAt": row.try_get::<String, _>("last_calculated_at").map_err(pg_error)?,
        "createdAt": row.try_get::<String, _>("created_at").map_err(pg_error)?,
        "updatedAt": row.try_get::<String, _>("updated_at").map_err(pg_error)?,
        "label": score_label(score),
    }))
}

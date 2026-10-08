#![cfg(feature = "postgres")]

use sqlx::Row;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::settlement::{
    SettlementConfirmOutcome, SettlementGenerateOutcome, SettlementMutationError, contract,
    map_mutation_error,
};
use crate::repositories::settlement_postgres_common::pg_error;

pub(in crate::repositories) fn generate(
    session: &RepositorySession,
    period_type: &str,
    period_key: &str,
    date_start: &str,
    date_end: &str,
    now: &str,
) -> Result<SettlementGenerateOutcome, SettlementMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let period_type = period_type.to_owned();
    let period_key = period_key.to_owned();
    let date_start = date_start.to_owned();
    let date_end = date_end.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let lock_key = format!("settlement:{tenant_id}:{period_type}:{period_key}");
                sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
                    .bind(lock_key)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                let existing = sqlx::query(
                    "SELECT id,confirmed
                     FROM settlements
                     WHERE tenant_id=$1 AND period_type=$2 AND period_key=$3
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&period_type)
                .bind(&period_key)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;

                if let Some(row) = existing {
                    let settlement_id = row.try_get::<String, _>("id").map_err(pg_error)?;
                    let confirmed = row.try_get::<i32, _>("confirmed").map_err(pg_error)? != 0;
                    if confirmed {
                        return Ok(SettlementGenerateOutcome {
                            settlement_id,
                            period_type,
                            period_key,
                            total_revenue: None,
                            total_deposits: None,
                            total_refunds: None,
                            existing: true,
                            confirmed: true,
                        });
                    }

                    let (revenue, deposits, refunds) =
                        aggregate_pg(connection, &tenant_id, &date_start, &date_end).await?;
                    sqlx::query(
                        "UPDATE settlements
                         SET total_revenue=$1,total_deposits=$2,total_refunds=$3,created_at=$4
                         WHERE tenant_id=$5 AND id=$6 AND confirmed=0",
                    )
                    .bind(revenue)
                    .bind(deposits)
                    .bind(refunds)
                    .bind(&now)
                    .bind(&tenant_id)
                    .bind(&settlement_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    return Ok(SettlementGenerateOutcome {
                        settlement_id,
                        period_type,
                        period_key,
                        total_revenue: Some(revenue),
                        total_deposits: Some(deposits),
                        total_refunds: Some(refunds),
                        existing: true,
                        confirmed: false,
                    });
                }

                let (revenue, deposits, refunds) =
                    aggregate_pg(connection, &tenant_id, &date_start, &date_end).await?;
                let settlement_id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO settlements
                     (id,period_type,period_key,total_revenue,total_deposits,total_refunds,
                      confirmed,tenant_id,created_at)
                     VALUES ($1,$2,$3,$4,$5,$6,0,$7,$8)",
                )
                .bind(&settlement_id)
                .bind(&period_type)
                .bind(&period_key)
                .bind(revenue)
                .bind(deposits)
                .bind(refunds)
                .bind(&tenant_id)
                .bind(&now)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(SettlementGenerateOutcome {
                    settlement_id,
                    period_type,
                    period_key,
                    total_revenue: Some(revenue),
                    total_deposits: Some(deposits),
                    total_refunds: Some(refunds),
                    existing: false,
                    confirmed: false,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn confirm(
    session: &RepositorySession,
    settlement_id: &str,
    now: &str,
) -> Result<SettlementConfirmOutcome, SettlementMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let settlement_id = settlement_id.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT period_type,period_key,confirmed
                     FROM settlements
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&settlement_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("settlement-not-found".into()))?;

                let period_type = row.try_get::<String, _>("period_type").map_err(pg_error)?;
                let period_key = row.try_get::<String, _>("period_key").map_err(pg_error)?;
                let confirmed = row.try_get::<i32, _>("confirmed").map_err(pg_error)? != 0;
                if confirmed {
                    return Err(contract("settlement-already-confirmed".into()));
                }

                let updated = sqlx::query(
                    "UPDATE settlements SET confirmed=1,confirmed_at=$1
                     WHERE tenant_id=$2 AND id=$3 AND confirmed=0",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(&settlement_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?
                .rows_affected();
                if updated != 1 {
                    return Err(contract("settlement-already-confirmed".into()));
                }

                Ok(SettlementConfirmOutcome {
                    settlement_id,
                    period_type,
                    period_key,
                })
            })
        })
        .map_err(map_mutation_error)
}

async fn aggregate_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    date_start: &str,
    date_end: &str,
) -> Result<(f64, f64, f64), crate::repositories::RepositoryError> {
    let revenue: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount),0)::double precision
         FROM revenue_records
         WHERE tenant_id=$1 AND recognition_date>=$2 AND recognition_date<=$3",
    )
    .bind(tenant_id)
    .bind(&date_start[..10])
    .bind(&date_end[..10])
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;

    let deposits: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount),0)::double precision
         FROM deposit_ledger
         WHERE tenant_id=$1 AND entry_type='collect' AND created_at>=$2 AND created_at<=$3",
    )
    .bind(tenant_id)
    .bind(date_start)
    .bind(date_end)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;

    let refunds: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(amount),0)::double precision
         FROM deposit_ledger
         WHERE tenant_id=$1 AND entry_type IN ('release','refund')
           AND created_at>=$2 AND created_at<=$3",
    )
    .bind(tenant_id)
    .bind(date_start)
    .bind(date_end)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;

    Ok((revenue, deposits, refunds))
}

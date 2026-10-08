#![cfg(feature = "postgres")]

use sqlx::Row;
use uuid::Uuid;

use crate::repositories::RepositoryError;
use crate::repositories::deposit::{
    DepositCalculateOutcome, DepositMutationError, DepositProjection, contract, map_mutation_error,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn calculate(
    session: &RepositorySession,
    order_id: &str,
    default_per_device: f64,
    now: &str,
) -> Result<DepositCalculateOutcome, DepositMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let order_exists = sqlx::query("SELECT 1 FROM orders WHERE id=$1 AND tenant_id=$2")
                    .bind(&order_id)
                    .bind(&tenant_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .is_some();
                if !order_exists {
                    return Err(contract(format!("deposit-order-not-found:{order_id}")));
                }

                let existing = sqlx::query(
                    "SELECT id,order_id,amount::double precision AS amount,status,
                            paid_at,released_at,forfeited_at,created_at,updated_at
                     FROM deposits
                     WHERE order_id=$1 AND tenant_id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&order_id)
                .bind(&tenant_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if let Some(row) = existing {
                    return Ok(DepositCalculateOutcome {
                        deposit: map_deposit(&row).map_err(pg_error)?,
                        existing: true,
                        device_count: None,
                    });
                }

                let device_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*)::bigint
                     FROM order_devices od
                     JOIN devices d ON d.serialno=od.serialno
                     WHERE od.orderid=$1 AND od.tenant_id=$2 AND d.tenant_id=$2",
                )
                .bind(&order_id)
                .bind(&tenant_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;
                let amount = if device_count > 0 {
                    device_count as f64 * default_per_device
                } else {
                    default_per_device
                };

                let id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO deposits
                     (id,order_id,amount,status,created_at,updated_at,tenant_id)
                     VALUES ($1,$2,$3,'pending',$4,$4,$5)",
                )
                .bind(&id)
                .bind(&order_id)
                .bind(amount)
                .bind(&now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(DepositCalculateOutcome {
                    deposit: DepositProjection {
                        id,
                        order_id,
                        amount,
                        status: "pending".into(),
                        paid_at: None,
                        released_at: None,
                        forfeited_at: None,
                        created_at: now.clone(),
                        updated_at: now,
                    },
                    existing: false,
                    device_count: Some(device_count),
                })
            })
        })
        .map_err(map_mutation_error)
}

fn map_deposit(row: &sqlx::postgres::PgRow) -> Result<DepositProjection, sqlx::Error> {
    Ok(DepositProjection {
        id: row.try_get("id")?,
        order_id: row.try_get("order_id")?,
        amount: row.try_get("amount")?,
        status: row.try_get("status")?,
        paid_at: row.try_get("paid_at")?,
        released_at: row.try_get("released_at")?,
        forfeited_at: row.try_get("forfeited_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

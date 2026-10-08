#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::deposit::{
    DepositMutationError, DepositReleaseOutcome, contract, map_mutation_error,
};
use crate::repositories::deposit_postgres_common::{
    insert_accounting_pair, insert_ledger, pg_error,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn release(
    session: &RepositorySession,
    order_id: &str,
    reason: &str,
    operator: &str,
    now: &str,
) -> Result<DepositReleaseOutcome, DepositMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let reason = reason.to_owned();
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT id,status,amount::double precision AS amount,released_at
                     FROM deposits
                     WHERE order_id=$1 AND tenant_id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&order_id)
                .bind(&tenant_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract(format!("deposit-not-found:{order_id}")))?;

                let deposit_id = row.try_get::<String, _>("id").map_err(pg_error)?;
                let status = row.try_get::<String, _>("status").map_err(pg_error)?;
                let total_amount = row.try_get::<f64, _>("amount").map_err(pg_error)?;
                let released_at = row
                    .try_get::<Option<String>, _>("released_at")
                    .map_err(pg_error)?;

                if released_at.is_some() {
                    return Err(contract(format!("deposit-already-released:{order_id}")));
                }
                if status != "paid" && status != "partially_forfeited" {
                    return Err(contract(format!("deposit-not-paid:{status}")));
                }

                let forfeited_total: f64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(amount)::double precision,0::double precision)
                     FROM deposit_ledger
                     WHERE deposit_id=$1 AND tenant_id=$2 AND entry_type='forfeit'",
                )
                .bind(&deposit_id)
                .bind(&tenant_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;
                let release_amount = total_amount - forfeited_total;

                sqlx::query(
                    "UPDATE deposits
                     SET status='released',released_at=$1,updated_at=$1
                     WHERE id=$2 AND tenant_id=$3",
                )
                .bind(&now)
                .bind(&deposit_id)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                let description = if reason.is_empty() {
                    format!("释放押金 ¥{release_amount:.2}")
                } else {
                    format!("释放押金 ¥{release_amount:.2} — {reason}")
                };
                insert_ledger(
                    connection,
                    &tenant_id,
                    &deposit_id,
                    &order_id,
                    "release",
                    release_amount,
                    0.0,
                    &description,
                    &operator,
                    &now,
                )
                .await?;
                insert_accounting_pair(
                    connection,
                    &tenant_id,
                    &order_id,
                    "deposit",
                    "cash",
                    release_amount,
                    &format!("押金释放 ¥{release_amount:.2}"),
                    &format!("押金退回 ¥{release_amount:.2}"),
                    &now,
                )
                .await?;

                Ok(DepositReleaseOutcome {
                    deposit_id,
                    order_id,
                    released_amount: release_amount,
                    forfeited_amount: forfeited_total,
                })
            })
        })
        .map_err(map_mutation_error)
}

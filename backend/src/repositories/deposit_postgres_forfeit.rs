#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::deposit::{
    DepositForfeitOutcome, DepositMutationError, contract, map_mutation_error,
};
use crate::repositories::deposit_postgres_common::{
    insert_accounting_pair, insert_ledger, pg_error,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn forfeit(
    session: &RepositorySession,
    order_id: &str,
    amount: f64,
    reason: &str,
    operator: &str,
    now: &str,
) -> Result<DepositForfeitOutcome, DepositMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let reason = reason.to_owned();
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT id,status,amount::double precision AS amount
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

                if status != "paid" && status != "partially_forfeited" {
                    return Err(contract(format!("deposit-not-paid:{status}")));
                }

                let already_forfeited: f64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(amount)::double precision,0::double precision)
                     FROM deposit_ledger
                     WHERE deposit_id=$1 AND tenant_id=$2 AND entry_type='forfeit'",
                )
                .bind(&deposit_id)
                .bind(&tenant_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;
                let available = total_amount - already_forfeited;
                if amount > available {
                    return Err(contract(format!(
                        "deposit-exceeds-available:{amount}:{available}"
                    )));
                }

                let total_forfeited = already_forfeited + amount;
                let remaining_balance = total_amount - total_forfeited;
                let new_status = if remaining_balance <= 0.01 {
                    "forfeited"
                } else {
                    "partially_forfeited"
                };

                sqlx::query(
                    "UPDATE deposits
                     SET status=$1,forfeited_at=$2,updated_at=$2
                     WHERE id=$3 AND tenant_id=$4",
                )
                .bind(new_status)
                .bind(&now)
                .bind(&deposit_id)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                let description = if reason.is_empty() {
                    format!("罚没押金 ¥{amount:.2}")
                } else {
                    format!("罚没押金 ¥{amount:.2} — {reason}")
                };
                insert_ledger(
                    connection,
                    &tenant_id,
                    &deposit_id,
                    &order_id,
                    "forfeit",
                    amount,
                    remaining_balance,
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
                    "revenue",
                    amount,
                    &format!("押金罚没负债减少 ¥{amount:.2}"),
                    &format!("押金罚没收入 ¥{amount:.2}"),
                    &now,
                )
                .await?;

                Ok(DepositForfeitOutcome {
                    deposit_id,
                    order_id,
                    forfeited_amount: amount,
                    total_forfeited,
                    remaining_balance,
                    status: new_status.into(),
                })
            })
        })
        .map_err(map_mutation_error)
}

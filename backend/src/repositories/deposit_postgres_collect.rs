#![cfg(feature = "postgres")]

use sqlx::Row;
use uuid::Uuid;

use crate::repositories::deposit::{
    DepositCollectOutcome, DepositMutationError, contract, map_mutation_error,
};
use crate::repositories::deposit_postgres_common::{
    insert_accounting_pair, insert_ledger, pg_error,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn collect(
    session: &RepositorySession,
    order_id: &str,
    amount: f64,
    operator: &str,
    now: &str,
) -> Result<DepositCollectOutcome, DepositMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let operator = operator.to_owned();
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

                let current = sqlx::query(
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
                .map_err(pg_error)?;

                let (deposit_id, status, current_amount) = match current {
                    Some(row) => (
                        row.try_get::<String, _>("id").map_err(pg_error)?,
                        row.try_get::<String, _>("status").map_err(pg_error)?,
                        row.try_get::<f64, _>("amount").map_err(pg_error)?,
                    ),
                    None => {
                        let id = Uuid::new_v4().to_string();
                        sqlx::query(
                            "INSERT INTO deposits
                             (id,order_id,amount,status,created_at,updated_at,tenant_id)
                             VALUES ($1,$2,0,'pending',$3,$3,$4)",
                        )
                        .bind(&id)
                        .bind(&order_id)
                        .bind(&now)
                        .bind(&tenant_id)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?;
                        (id, "pending".into(), 0.0)
                    }
                };

                if status == "paid" || status == "released" {
                    return Err(contract(format!("deposit-already-paid:{order_id}")));
                }

                let new_amount = if current_amount > 0.0 {
                    current_amount
                } else {
                    amount
                };
                sqlx::query(
                    "UPDATE deposits
                     SET amount=$1,status='paid',paid_at=$2,updated_at=$2
                     WHERE id=$3 AND tenant_id=$4",
                )
                .bind(new_amount)
                .bind(&now)
                .bind(&deposit_id)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                insert_ledger(
                    connection,
                    &tenant_id,
                    &deposit_id,
                    &order_id,
                    "collect",
                    new_amount,
                    new_amount,
                    &format!("deposit collect {new_amount:.2}"),
                    &operator,
                    &now,
                )
                .await?;
                insert_accounting_pair(
                    connection,
                    &tenant_id,
                    &order_id,
                    "cash",
                    "deposit",
                    new_amount,
                    &format!("deposit cash {new_amount:.2}"),
                    &format!("deposit liability {new_amount:.2}"),
                    &now,
                )
                .await?;

                Ok(DepositCollectOutcome {
                    deposit_id,
                    order_id,
                    amount: new_amount,
                })
            })
        })
        .map_err(map_mutation_error)
}

#![cfg(feature = "postgres")]

use sqlx::Row;
use uuid::Uuid;

use crate::repositories::refund::{
    RefundMutationError, RefundRequestOutcome, contract, map_mutation_error,
};
use crate::repositories::refund_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn request(
    session: &RepositorySession,
    order_id: &str,
    amount: f64,
    reason: &str,
    operator: &str,
    now: &str,
) -> Result<RefundRequestOutcome, RefundMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let reason = reason.to_owned();
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let deposit = sqlx::query(
                    "SELECT id FROM deposits
                     WHERE order_id=$1 AND tenant_id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&order_id)
                .bind(&tenant_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract(format!("refund-deposit-not-found:{order_id}")))?;

                let deposit_id = deposit.try_get::<String, _>("id").map_err(pg_error)?;
                let refund_id = Uuid::new_v4().to_string();
                sqlx::query(
                    "INSERT INTO refunds
                     (id,deposit_id,order_id,amount,reason,status,requested_by,created_at,updated_at,tenant_id)
                     VALUES ($1,$2,$3,$4,$5,'pending',$6,$7,$7,$8)",
                )
                .bind(&refund_id)
                .bind(&deposit_id)
                .bind(&order_id)
                .bind(amount)
                .bind(&reason)
                .bind(&operator)
                .bind(&now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(RefundRequestOutcome {
                    refund_id,
                    deposit_id,
                    order_id,
                    amount,
                })
            })
        })
        .map_err(map_mutation_error)
}

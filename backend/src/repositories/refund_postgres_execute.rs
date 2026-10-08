#![cfg(feature = "postgres")]

use sqlx::Row;
use uuid::Uuid;

use crate::repositories::refund::{
    RefundExecuteOutcome, RefundMutationError, contract, map_mutation_error,
};
use crate::repositories::refund_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn execute(
    session: &RepositorySession,
    refund_id: &str,
    operator: &str,
    now: &str,
) -> Result<RefundExecuteOutcome, RefundMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let refund_id = refund_id.to_owned();
    let operator = operator.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT deposit_id,order_id,status,amount::double precision AS amount
                     FROM refunds
                     WHERE id=$1 AND tenant_id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&refund_id)
                .bind(&tenant_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract(format!("refund-not-found:{refund_id}")))?;

                let deposit_id = row.try_get::<String, _>("deposit_id").map_err(pg_error)?;
                let order_id = row.try_get::<String, _>("order_id").map_err(pg_error)?;
                let status = row.try_get::<String, _>("status").map_err(pg_error)?;
                let amount = row.try_get::<f64, _>("amount").map_err(pg_error)?;
                if status != "approved" {
                    return Err(contract(format!("refund-not-approved:{status}")));
                }

                sqlx::query(
                    "UPDATE refunds
                     SET status='executed',executed_by=$1,executed_at=$2,updated_at=$2
                     WHERE id=$3 AND tenant_id=$4",
                )
                .bind(&operator)
                .bind(&now)
                .bind(&refund_id)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                sqlx::query(
                    "INSERT INTO deposit_ledger
                     (id,deposit_id,order_id,entry_type,amount,balance_after,description,operator,created_at,tenant_id)
                     VALUES ($1,$2,$3,'refund',$4,0,$5,$6,$7,$8)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&deposit_id)
                .bind(&order_id)
                .bind(amount)
                .bind(format!("退款执行 ¥{amount:.2}"))
                .bind(&operator)
                .bind(&now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(RefundExecuteOutcome { refund_id, amount })
            })
        })
        .map_err(map_mutation_error)
}

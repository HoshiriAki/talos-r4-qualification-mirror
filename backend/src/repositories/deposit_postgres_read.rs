#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::deposit::{
    DepositDetailProjection, DepositLedgerProjection, DepositProjection,
};
use crate::repositories::{RepositoryError, RepositorySession};

pub(in crate::repositories) fn get(
    session: &RepositorySession,
    order_id: &str,
) -> Result<DepositDetailProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let deposit = sqlx::query(
                "SELECT id,order_id,amount::double precision AS amount,status,
                        paid_at,released_at,forfeited_at,created_at,updated_at
                 FROM deposits
                 WHERE order_id=$1 AND tenant_id=$2
                 LIMIT 1",
            )
            .bind(&order_id)
            .bind(&tenant_id)
            .fetch_optional(&mut *connection)
            .await?;

            let deposit = deposit.as_ref().map(map_deposit).transpose()?;
            let ledger = if let Some(deposit) = &deposit {
                sqlx::query(
                    "SELECT entry_type,amount::double precision AS amount,
                            balance_after::double precision AS balance_after,
                            description,operator,created_at
                     FROM deposit_ledger
                     WHERE deposit_id=$1 AND tenant_id=$2
                     ORDER BY created_at ASC",
                )
                .bind(&deposit.id)
                .bind(&tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(map_ledger)
                .collect::<Result<Vec<_>, _>>()?
            } else {
                Vec::new()
            };

            Ok(DepositDetailProjection { deposit, ledger })
        })
    })
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

fn map_ledger(row: &sqlx::postgres::PgRow) -> Result<DepositLedgerProjection, sqlx::Error> {
    Ok(DepositLedgerProjection {
        entry_type: row.try_get("entry_type")?,
        amount: row.try_get("amount")?,
        balance_after: row.try_get("balance_after")?,
        description: row.try_get("description")?,
        operator: row.try_get("operator")?,
        created_at: row.try_get("created_at")?,
    })
}

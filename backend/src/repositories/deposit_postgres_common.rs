#![cfg(feature = "postgres")]

use uuid::Uuid;

use crate::repositories::RepositoryError;

pub(in crate::repositories) async fn insert_ledger(
    connection: &mut sqlx::postgres::PgConnection,
    tenant_id: &str,
    deposit_id: &str,
    order_id: &str,
    entry_type: &str,
    amount: f64,
    balance_after: f64,
    description: &str,
    operator: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    sqlx::query(
        "INSERT INTO deposit_ledger
         (id,deposit_id,order_id,entry_type,amount,balance_after,description,operator,created_at,tenant_id)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(deposit_id)
    .bind(order_id)
    .bind(entry_type)
    .bind(amount)
    .bind(balance_after)
    .bind(description)
    .bind(operator)
    .bind(now)
    .bind(tenant_id)
    .execute(connection)
    .await
    .map_err(pg_error)?;
    Ok(())
}

pub(in crate::repositories) async fn insert_accounting_pair(
    connection: &mut sqlx::postgres::PgConnection,
    tenant_id: &str,
    order_id: &str,
    debit_account: &str,
    credit_account: &str,
    amount: f64,
    debit_description: &str,
    credit_description: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    for (entry_type, account, description) in [
        ("debit", debit_account, debit_description),
        ("credit", credit_account, credit_description),
    ] {
        sqlx::query(
            "INSERT INTO accounting_entries
             (id,order_id,entry_type,account,amount,description,created_at,tenant_id)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(order_id)
        .bind(entry_type)
        .bind(account)
        .bind(amount)
        .bind(description)
        .bind(now)
        .bind(tenant_id)
        .execute(&mut *connection)
        .await
        .map_err(pg_error)?;
    }
    Ok(())
}

pub(in crate::repositories) fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

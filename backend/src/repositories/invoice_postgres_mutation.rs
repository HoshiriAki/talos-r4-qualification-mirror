#![cfg(feature = "postgres")]

use sqlx::Row;
use uuid::Uuid;

use crate::repositories::invoice::{
    InvoiceIssueOutcome, InvoiceMutationError, InvoiceRedFlushOutcome, InvoiceVoidOutcome,
    contract, map_mutation_error,
};
use crate::repositories::invoice_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn issue(
    session: &RepositorySession,
    order_id: &str,
    amount: f64,
    invoice_type: &str,
    tax_rate: Option<f64>,
    now: &str,
) -> Result<InvoiceIssueOutcome, InvoiceMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let invoice_type = invoice_type.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let tax_rate = match tax_rate {
                    Some(value) => value,
                    None => sqlx::query_scalar::<_, f64>(
                        "SELECT ROUND(rate::numeric, 8)::double precision
                         FROM tax_config
                         WHERE tenant_id=$1 AND tax_type='vat' AND is_active=1
                         ORDER BY effective_from DESC LIMIT 1",
                    )
                    .bind(&tenant_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .unwrap_or(0.13),
                };

                let invoice_no = next_invoice_no_pg(connection, &tenant_id, &now).await?;
                let tax_amount = round_money(amount * tax_rate);
                let invoice_id = Uuid::new_v4().to_string();

                sqlx::query(
                    "INSERT INTO invoices
                     (id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,
                      tenant_id,issued_at,created_at)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,'issued',$8,$9,$9)",
                )
                .bind(&invoice_id)
                .bind(&order_id)
                .bind(&invoice_no)
                .bind(&invoice_type)
                .bind(amount)
                .bind(tax_rate)
                .bind(tax_amount)
                .bind(&tenant_id)
                .bind(&now)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                sqlx::query(
                    "INSERT INTO revenue_records
                     (id,order_id,amount,recognition_date,source,created_at,tenant_id)
                     VALUES ($1,$2,$3,$4,'order_complete',$5,$6)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&order_id)
                .bind(amount)
                .bind(&now[..10])
                .bind(&now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                insert_accounting_entry_pg(
                    connection,
                    &tenant_id,
                    &order_id,
                    "debit",
                    "receivable",
                    amount,
                    &format!("发票 {invoice_no} 应收账款"),
                    &now,
                )
                .await?;
                insert_accounting_entry_pg(
                    connection,
                    &tenant_id,
                    &order_id,
                    "credit",
                    "revenue",
                    amount,
                    &format!("发票 {invoice_no} 收入确认"),
                    &now,
                )
                .await?;

                Ok(InvoiceIssueOutcome {
                    invoice_id,
                    invoice_no,
                    order_id,
                    amount,
                    tax_rate,
                    tax_amount,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn void(
    session: &RepositorySession,
    invoice_id: &str,
    now: &str,
) -> Result<InvoiceVoidOutcome, InvoiceMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let invoice_id = invoice_id.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT order_id,status,invoice_no
                     FROM invoices
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&invoice_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("invoice-not-found".into()))?;
                let order_id = row.try_get::<String, _>("order_id").map_err(pg_error)?;
                let status = row.try_get::<String, _>("status").map_err(pg_error)?;
                let invoice_no = row.try_get::<String, _>("invoice_no").map_err(pg_error)?;
                if status != "issued" {
                    return Err(contract(format!("invoice-not-issued:{status}")));
                }

                sqlx::query(
                    "UPDATE invoices SET status='voided',voided_at=$1
                     WHERE tenant_id=$2 AND id=$3 AND status='issued'",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(&invoice_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                insert_accounting_entry_pg(
                    connection,
                    &tenant_id,
                    &order_id,
                    "credit",
                    "receivable",
                    0.0,
                    &format!("发票 {invoice_no} 作废冲回应收"),
                    &now,
                )
                .await?;

                Ok(InvoiceVoidOutcome { invoice_id })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn red_flush(
    session: &RepositorySession,
    invoice_id: &str,
    reason: &str,
    now: &str,
) -> Result<InvoiceRedFlushOutcome, InvoiceMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let invoice_id = invoice_id.to_owned();
    let reason = reason.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT order_id,
                            amount::double precision AS amount,
                            ROUND(tax_rate::numeric, 8)::double precision AS tax_rate,
                            tax_amount::double precision AS tax_amount,
                            invoice_no,type,status
                     FROM invoices
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&invoice_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?
                .ok_or_else(|| contract("invoice-not-found".into()))?;
                let order_id = row.try_get::<String, _>("order_id").map_err(pg_error)?;
                let amount = row.try_get::<f64, _>("amount").map_err(pg_error)?;
                let tax_rate = row.try_get::<f64, _>("tax_rate").map_err(pg_error)?;
                let tax_amount = row.try_get::<f64, _>("tax_amount").map_err(pg_error)?;
                let invoice_no = row.try_get::<String, _>("invoice_no").map_err(pg_error)?;
                let invoice_type = row.try_get::<String, _>("type").map_err(pg_error)?;
                let status = row.try_get::<String, _>("status").map_err(pg_error)?;
                if status != "issued" {
                    return Err(contract(format!("invoice-not-issued:{status}")));
                }

                let red_invoice_no = next_invoice_no_pg(connection, &tenant_id, &now).await?;

                sqlx::query(
                    "UPDATE invoices SET status='voided',voided_at=$1
                     WHERE tenant_id=$2 AND id=$3 AND status='issued'",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(&invoice_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                let red_invoice_id = Uuid::new_v4().to_string();
                let red_amount = -amount;
                let red_tax_amount = -tax_amount;
                sqlx::query(
                    "INSERT INTO invoices
                     (id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,
                      tenant_id,issued_at,created_at)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,'issued',$8,$9,$9)",
                )
                .bind(&red_invoice_id)
                .bind(&order_id)
                .bind(&red_invoice_no)
                .bind(&invoice_type)
                .bind(red_amount)
                .bind(tax_rate)
                .bind(red_tax_amount)
                .bind(&tenant_id)
                .bind(&now)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                sqlx::query(
                    "INSERT INTO revenue_records
                     (id,order_id,amount,recognition_date,source,created_at,tenant_id)
                     VALUES ($1,$2,$3,$4,'other',$5,$6)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&order_id)
                .bind(red_amount)
                .bind(&now[..10])
                .bind(&now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                let description = if reason.is_empty() {
                    format!("红字冲销 原发票 {invoice_no}")
                } else {
                    format!("红字冲销 原发票 {invoice_no} — {reason}")
                };
                insert_accounting_entry_pg(
                    connection,
                    &tenant_id,
                    &order_id,
                    "credit",
                    "receivable",
                    amount,
                    &description,
                    &now,
                )
                .await?;
                insert_accounting_entry_pg(
                    connection,
                    &tenant_id,
                    &order_id,
                    "debit",
                    "revenue",
                    amount,
                    &description,
                    &now,
                )
                .await?;

                Ok(InvoiceRedFlushOutcome {
                    original_invoice_id: invoice_id,
                    red_invoice_id,
                    red_invoice_no,
                    red_amount,
                })
            })
        })
        .map_err(map_mutation_error)
}

async fn next_invoice_no_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    now: &str,
) -> Result<String, crate::repositories::RepositoryError> {
    let date = now.get(..10).unwrap_or("1970-01-01").replace('-', "");
    let prefix = format!("INV-{date}-");
    let lock_key = format!("invoice-number:{tenant_id}:{date}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(lock_key)
        .execute(&mut *connection)
        .await
        .map_err(pg_error)?;

    let like_pattern = format!("{prefix}%");
    let max_seq: Option<String> = sqlx::query_scalar(
        "SELECT invoice_no FROM invoices
         WHERE tenant_id=$1 AND invoice_no LIKE $2
         ORDER BY invoice_no DESC LIMIT 1",
    )
    .bind(tenant_id)
    .bind(like_pattern)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;

    let seq = max_seq
        .as_deref()
        .and_then(|value| value.strip_prefix(&prefix))
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
        + 1;
    Ok(format!("{prefix}{seq:04}"))
}

async fn insert_accounting_entry_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    order_id: &str,
    entry_type: &str,
    account: &str,
    amount: f64,
    description: &str,
    now: &str,
) -> Result<(), crate::repositories::RepositoryError> {
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
    Ok(())
}

fn round_money(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

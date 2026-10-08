#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::session::RepositorySession;
use crate::repositories::tax::{
    TaxConfigExportRow, TaxConfigProjection, TaxExportSnapshot, TaxInvoiceExportRow,
};

pub(in crate::repositories) fn get_configs(
    session: &RepositorySession,
    tax_type: &str,
) -> Result<Vec<TaxConfigProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let tax_type = tax_type.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT id,tax_type,ROUND(rate::numeric, 8)::double precision AS rate,effective_from,
                        (is_active <> 0) AS is_active,created_at
                 FROM tax_config
                 WHERE tenant_id=$1 AND tax_type=$2
                 ORDER BY effective_from DESC,created_at DESC",
            )
            .bind(&tenant_id)
            .bind(&tax_type)
            .fetch_all(&mut *connection)
            .await?;

            rows.iter()
                .map(|row| {
                    Ok(TaxConfigProjection {
                        id: row.try_get("id")?,
                        tax_type: row.try_get("tax_type")?,
                        rate: row.try_get("rate")?,
                        effective_from: row.try_get("effective_from")?,
                        is_active: row.try_get("is_active")?,
                        created_at: row.try_get("created_at")?,
                    })
                })
                .collect()
        })
    })
}

pub(in crate::repositories) fn active_rate(
    session: &RepositorySession,
    tax_type: &str,
) -> Result<Option<f64>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let tax_type = tax_type.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query_scalar::<_, f64>(
                "SELECT ROUND(rate::numeric, 8)::double precision
                 FROM tax_config
                 WHERE tenant_id=$1 AND tax_type=$2 AND is_active=1
                 ORDER BY effective_from DESC,created_at DESC LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(&tax_type)
            .fetch_optional(&mut *connection)
            .await
        })
    })
}

pub(in crate::repositories) fn export_snapshot(
    session: &RepositorySession,
    period_prefix: &str,
) -> Result<TaxExportSnapshot, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let date_pattern = format!("{period_prefix}%");

    session.pg_read(move |connection| {
        Box::pin(async move {
            let invoice_rows = sqlx::query(
                "SELECT invoice_no,order_id,type,
                        amount::double precision AS amount,
                        ROUND(tax_rate::numeric, 8)::double precision AS tax_rate,
                        tax_amount::double precision AS tax_amount,
                        status,issued_at
                 FROM invoices
                 WHERE tenant_id=$1 AND issued_at LIKE $2
                 ORDER BY issued_at ASC,invoice_no ASC",
            )
            .bind(&tenant_id)
            .bind(&date_pattern)
            .fetch_all(&mut *connection)
            .await?;
            let invoices = invoice_rows
                .iter()
                .map(|row| {
                    Ok(TaxInvoiceExportRow {
                        invoice_no: row.try_get("invoice_no")?,
                        order_id: row.try_get("order_id")?,
                        invoice_type: row.try_get("type")?,
                        amount: row.try_get("amount")?,
                        tax_rate: row.try_get("tax_rate")?,
                        tax_amount: row.try_get("tax_amount")?,
                        status: row.try_get("status")?,
                        issued_at: row.try_get("issued_at")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;

            let config_rows = sqlx::query(
                "SELECT tax_type,ROUND(rate::numeric, 8)::double precision AS rate,effective_from
                 FROM tax_config
                 WHERE tenant_id=$1 AND is_active=1
                 ORDER BY tax_type ASC,effective_from DESC",
            )
            .bind(&tenant_id)
            .fetch_all(&mut *connection)
            .await?;
            let configs = config_rows
                .iter()
                .map(|row| {
                    Ok(TaxConfigExportRow {
                        tax_type: row.try_get("tax_type")?,
                        rate: row.try_get("rate")?,
                        effective_from: row.try_get("effective_from")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;

            Ok(TaxExportSnapshot { invoices, configs })
        })
    })
}

#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::invoice::{InvoiceListItem, InvoiceListProjection, InvoiceProjection};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn get_by_id(
    session: &RepositorySession,
    invoice_id: &str,
) -> Result<Option<InvoiceProjection>, RepositoryError> {
    get_one(session, "id", invoice_id)
}

pub(in crate::repositories) fn get_by_order(
    session: &RepositorySession,
    order_id: &str,
) -> Result<Option<InvoiceProjection>, RepositoryError> {
    get_one(session, "order_id", order_id)
}

fn get_one(
    session: &RepositorySession,
    column: &'static str,
    value: &str,
) -> Result<Option<InvoiceProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let value = value.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let sql = format!(
                "SELECT id,order_id,invoice_no,type,
                        amount::double precision AS amount,
                        ROUND(tax_rate::numeric, 8)::double precision AS tax_rate,
                        tax_amount::double precision AS tax_amount,
                        status,tenant_id,issued_at,voided_at,created_at
                 FROM invoices
                 WHERE tenant_id=$1 AND {column}=$2
                 ORDER BY created_at DESC LIMIT 1"
            );
            sqlx::query(&sql)
                .bind(&tenant_id)
                .bind(&value)
                .fetch_optional(&mut *connection)
                .await?
                .map(|row| map_projection(&row))
                .transpose()
        })
    })
}

pub(in crate::repositories) fn list(
    session: &RepositorySession,
    status: Option<&str>,
    date_from: Option<&str>,
    date_to: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<InvoiceListProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
    let date_from = date_from
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let date_to = date_to
        .filter(|value| !value.is_empty())
        .map(|value| format!("{value}T23:59:59.999+08:00"));
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;

            let mut count =
                QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM invoices WHERE ");
            push_filters(
                &mut count,
                &tenant_id,
                status.as_deref(),
                date_from.as_deref(),
                date_to.as_deref(),
            );
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,order_id,invoice_no,type,
                        amount::double precision AS amount,
                        ROUND(tax_rate::numeric, 8)::double precision AS tax_rate,
                        tax_amount::double precision AS tax_amount,
                        status,issued_at,voided_at,created_at
                 FROM invoices WHERE ",
            );
            push_filters(
                &mut data,
                &tenant_id,
                status.as_deref(),
                date_from.as_deref(),
                date_to.as_deref(),
            );
            data.push(" ORDER BY created_at DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);

            let rows = data.build().fetch_all(&mut *connection).await?;
            let invoices = rows
                .iter()
                .map(map_list_item)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(InvoiceListProjection {
                invoices,
                page,
                page_size,
                total,
            })
        })
    })
}

fn push_filters<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    status: Option<&'args str>,
    date_from: Option<&'args str>,
    date_to: Option<&'args str>,
) {
    query.push("tenant_id=").push_bind(tenant_id);
    if let Some(status) = status {
        query.push(" AND status=").push_bind(status);
    }
    if let Some(date_from) = date_from {
        query.push(" AND issued_at>=").push_bind(date_from);
    }
    if let Some(date_to) = date_to {
        query.push(" AND issued_at<=").push_bind(date_to);
    }
}

fn map_projection(row: &sqlx::postgres::PgRow) -> Result<InvoiceProjection, sqlx::Error> {
    Ok(InvoiceProjection {
        id: row.try_get("id")?,
        order_id: row.try_get("order_id")?,
        invoice_no: row.try_get("invoice_no")?,
        invoice_type: row.try_get("type")?,
        amount: row.try_get("amount")?,
        tax_rate: row.try_get("tax_rate")?,
        tax_amount: row.try_get("tax_amount")?,
        status: row.try_get("status")?,
        tenant_id: row.try_get("tenant_id")?,
        issued_at: row.try_get("issued_at")?,
        voided_at: row.try_get("voided_at")?,
        created_at: row.try_get("created_at")?,
    })
}

fn map_list_item(row: &sqlx::postgres::PgRow) -> Result<InvoiceListItem, sqlx::Error> {
    Ok(InvoiceListItem {
        id: row.try_get("id")?,
        order_id: row.try_get("order_id")?,
        invoice_no: row.try_get("invoice_no")?,
        invoice_type: row.try_get("type")?,
        amount: row.try_get("amount")?,
        tax_rate: row.try_get("tax_rate")?,
        tax_amount: row.try_get("tax_amount")?,
        status: row.try_get("status")?,
        issued_at: row.try_get("issued_at")?,
        voided_at: row.try_get("voided_at")?,
        created_at: row.try_get("created_at")?,
    })
}

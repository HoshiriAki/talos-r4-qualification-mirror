#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::session::RepositorySession;
use crate::repositories::settlement::{
    SettlementListProjection, SettlementProjection, SettlementRevenueDetail,
};

pub(in crate::repositories) fn get_by_period(
    session: &RepositorySession,
    period_type: &str,
    period_key: &str,
) -> Result<Option<SettlementProjection>, RepositoryError> {
    get_one(session, "period", period_type, period_key)
}

pub(in crate::repositories) fn get_by_id(
    session: &RepositorySession,
    settlement_id: &str,
) -> Result<Option<SettlementProjection>, RepositoryError> {
    get_one(session, "id", settlement_id, "")
}

fn get_one(
    session: &RepositorySession,
    kind: &'static str,
    first: &str,
    second: &str,
) -> Result<Option<SettlementProjection>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let first = first.to_owned();
    let second = second.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            let row = if kind == "period" {
                sqlx::query(
                    "SELECT id,period_type,period_key,
                            total_revenue::double precision AS total_revenue,
                            total_deposits::double precision AS total_deposits,
                            total_refunds::double precision AS total_refunds,
                            (confirmed <> 0) AS confirmed,confirmed_at,tenant_id,created_at
                     FROM settlements
                     WHERE tenant_id=$1 AND period_type=$2 AND period_key=$3
                     LIMIT 1",
                )
                .bind(&tenant_id)
                .bind(&first)
                .bind(&second)
                .fetch_optional(&mut *connection)
                .await?
            } else {
                sqlx::query(
                    "SELECT id,period_type,period_key,
                            total_revenue::double precision AS total_revenue,
                            total_deposits::double precision AS total_deposits,
                            total_refunds::double precision AS total_refunds,
                            (confirmed <> 0) AS confirmed,confirmed_at,tenant_id,created_at
                     FROM settlements
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1",
                )
                .bind(&tenant_id)
                .bind(&first)
                .fetch_optional(&mut *connection)
                .await?
            };
            row.map(|row| map_projection(&row)).transpose()
        })
    })
}

pub(in crate::repositories) fn list(
    session: &RepositorySession,
    period_type: Option<&str>,
    confirmed: Option<bool>,
    page: i64,
    page_size: i64,
) -> Result<SettlementListProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let period_type = period_type.filter(|v| !v.is_empty()).map(str::to_owned);
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;
            let mut count =
                QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM settlements WHERE ");
            push_filters(&mut count, &tenant_id, period_type.as_deref(), confirmed);
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT id,period_type,period_key,
                        total_revenue::double precision AS total_revenue,
                        total_deposits::double precision AS total_deposits,
                        total_refunds::double precision AS total_refunds,
                        (confirmed <> 0) AS confirmed,confirmed_at,tenant_id,created_at
                 FROM settlements WHERE ",
            );
            push_filters(&mut data, &tenant_id, period_type.as_deref(), confirmed);
            data.push(" ORDER BY period_key DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);

            let rows = data.build().fetch_all(&mut *connection).await?;
            let settlements = rows
                .iter()
                .map(map_projection)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(SettlementListProjection {
                settlements,
                page,
                page_size,
                total,
            })
        })
    })
}

pub(in crate::repositories) fn revenue_details(
    session: &RepositorySession,
    date_start: &str,
    date_end: &str,
) -> Result<Vec<SettlementRevenueDetail>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let date_start = date_start.to_owned();
    let date_end = date_end.to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT order_id,amount::double precision AS amount,recognition_date,source
                 FROM revenue_records
                 WHERE tenant_id=$1 AND recognition_date>=$2 AND recognition_date<=$3
                 ORDER BY recognition_date ASC",
            )
            .bind(&tenant_id)
            .bind(&date_start)
            .bind(&date_end)
            .fetch_all(&mut *connection)
            .await?;
            rows.iter()
                .map(|row| {
                    Ok(SettlementRevenueDetail {
                        order_id: row.try_get("order_id")?,
                        amount: row.try_get("amount")?,
                        recognition_date: row.try_get("recognition_date")?,
                        source: row.try_get("source")?,
                    })
                })
                .collect()
        })
    })
}

fn push_filters<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    period_type: Option<&'args str>,
    confirmed: Option<bool>,
) {
    query.push("tenant_id=").push_bind(tenant_id);
    if let Some(period_type) = period_type {
        query.push(" AND period_type=").push_bind(period_type);
    }
    if let Some(confirmed) = confirmed {
        query
            .push(" AND confirmed=")
            .push_bind(if confirmed { 1_i32 } else { 0_i32 });
    }
}

fn map_projection(row: &sqlx::postgres::PgRow) -> Result<SettlementProjection, sqlx::Error> {
    Ok(SettlementProjection {
        id: row.try_get("id")?,
        period_type: row.try_get("period_type")?,
        period_key: row.try_get("period_key")?,
        total_revenue: row.try_get("total_revenue")?,
        total_deposits: row.try_get("total_deposits")?,
        total_refunds: row.try_get("total_refunds")?,
        confirmed: row.try_get("confirmed")?,
        confirmed_at: row.try_get("confirmed_at")?,
        tenant_id: row.try_get("tenant_id")?,
        created_at: row.try_get("created_at")?,
    })
}

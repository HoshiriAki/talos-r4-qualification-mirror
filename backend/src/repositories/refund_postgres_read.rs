#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::refund::{RefundListItem, RefundListProjection};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn list(
    session: &RepositorySession,
    order_id: Option<&str>,
    status: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<RefundListProjection, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
    let page = page.max(1);
    let page_size = page_size.clamp(1, 100);

    session.pg_read(move |connection| {
        Box::pin(async move {
            let offset = (page - 1) * page_size;

            let mut count =
                QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM refunds r WHERE ");
            push_predicates(
                &mut count,
                &tenant_id,
                order_id.as_deref(),
                status.as_deref(),
            );
            let total: i64 = count
                .build_query_scalar()
                .fetch_one(&mut *connection)
                .await?;

            let mut data = QueryBuilder::<Postgres>::new(
                "SELECT r.id,r.deposit_id,r.order_id,
                        r.amount::double precision AS amount,
                        r.reason,r.status,r.requested_by,r.approved_by,r.rejected_by,r.executed_by,
                        r.approved_at,r.rejected_at,r.executed_at,r.created_at
                 FROM refunds r WHERE ",
            );
            push_predicates(
                &mut data,
                &tenant_id,
                order_id.as_deref(),
                status.as_deref(),
            );
            data.push(" ORDER BY r.created_at DESC LIMIT ")
                .push_bind(page_size)
                .push(" OFFSET ")
                .push_bind(offset);

            let rows = data.build().fetch_all(&mut *connection).await?;
            let refunds = rows
                .iter()
                .map(map_list_item)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(RefundListProjection {
                refunds,
                page,
                page_size,
                total,
            })
        })
    })
}

fn push_predicates<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    order_id: Option<&'args str>,
    status: Option<&'args str>,
) {
    query.push("r.tenant_id=").push_bind(tenant_id);
    if let Some(order_id) = order_id {
        query.push(" AND r.order_id=").push_bind(order_id);
    }
    if let Some(status) = status {
        query.push(" AND r.status=").push_bind(status);
    }
}

fn map_list_item(row: &sqlx::postgres::PgRow) -> Result<RefundListItem, sqlx::Error> {
    Ok(RefundListItem {
        id: row.try_get("id")?,
        deposit_id: row.try_get("deposit_id")?,
        order_id: row.try_get("order_id")?,
        amount: row.try_get("amount")?,
        reason: row.try_get("reason")?,
        status: row.try_get("status")?,
        requested_by: row.try_get("requested_by")?,
        approved_by: row.try_get("approved_by")?,
        rejected_by: row.try_get("rejected_by")?,
        executed_by: row.try_get("executed_by")?,
        approved_at: row.try_get("approved_at")?,
        rejected_at: row.try_get("rejected_at")?,
        executed_at: row.try_get("executed_at")?,
        created_at: row.try_get("created_at")?,
    })
}

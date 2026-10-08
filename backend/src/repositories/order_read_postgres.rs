#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::order_read::{
    OrderListRequest, OrderReadPage, OrderReadProjection, OrderSortDirection, OrderSortField,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresOrderReadRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresOrderReadRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn list(
        &self,
        request: &OrderListRequest,
    ) -> Result<OrderReadPage, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let request = request.clone();
        self.session.pg_read(move |connection| {
            Box::pin(async move { list_pg(connection, &tenant_id, &request).await })
        })
    }

    pub(in crate::repositories) fn get_by_id(
        &self,
        id: &str,
    ) -> Result<Option<OrderReadProjection>, RepositoryError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "order read requires a non-empty id".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move { get_pg(connection, &tenant_id, &id).await })
        })
    }
}

async fn list_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    request: &OrderListRequest,
) -> Result<OrderReadPage, sqlx::Error> {
    let page = request.page.max(1);
    let page_size = request.page_size.clamp(1, 200);
    let offset = request
        .offset
        .unwrap_or_else(|| u64::from(page - 1) * u64::from(page_size));

    let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM orders o WHERE ");
    push_predicates(&mut count, tenant_id, request);
    let total_i64: i64 = count
        .build_query_scalar()
        .fetch_one(&mut *connection)
        .await?;
    let total = u32::try_from(total_i64).unwrap_or(u32::MAX);

    let mut data = QueryBuilder::<Postgres>::new(
        "SELECT o.id, o.orderno, o.startdate, o.enddate, \
         COALESCE(o.deliverydate, '') AS deliverydate, \
         COALESCE(o.pickupmethods, '[]') AS pickupmethods, \
         COALESCE(o.address, '') AS address, COALESCE(o.notes, '') AS notes, \
         COALESCE(o.deviceserialno, '') AS deviceserialno, \
         COALESCE(o.totalprice, 0)::float8 AS totalprice, \
         COALESCE(o.province, '') AS province, \
         COALESCE(o.sendwarehouseid, '') AS sendwarehouseid, \
         COALESCE(o.returnwarehouseid, '') AS returnwarehouseid, \
         COALESCE(o.createdat, '') AS createdat, \
         COALESCE(o.accessories::text, '[]') AS accessories, \
         COALESCE(o.status, '') AS status, \
         COALESCE(o.trackingno, '') AS trackingno, \
         COALESCE(o.devicemodels::text, '{}') AS devicemodels \
         FROM orders o WHERE ",
    );
    push_predicates(&mut data, tenant_id, request);
    data.push(" ORDER BY ")
        .push(pg_sort_column(request.sort_by))
        .push(" ")
        .push(pg_sort_direction(request.sort_direction))
        .push(" LIMIT ")
        .push_bind(i64::from(page_size))
        .push(" OFFSET ")
        .push_bind(i64::try_from(offset).unwrap_or(i64::MAX));

    let rows = data.build().fetch_all(&mut *connection).await?;
    let mut orders = Vec::with_capacity(rows.len());
    for row in rows {
        let id: String = row.try_get("id")?;
        let devices = load_devices(&mut *connection, tenant_id, &id).await?;
        orders.push(map_pg_order(&row, devices)?);
    }

    let total_pages = if total == 0 {
        0
    } else {
        total.div_ceil(page_size)
    };
    Ok(OrderReadPage {
        orders,
        page,
        page_size,
        total,
        total_pages,
    })
}

async fn get_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    id: &str,
) -> Result<Option<OrderReadProjection>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT o.id, o.orderno, o.startdate, o.enddate, \
         COALESCE(o.deliverydate, '') AS deliverydate, \
         COALESCE(o.pickupmethods, '[]') AS pickupmethods, \
         COALESCE(o.address, '') AS address, COALESCE(o.notes, '') AS notes, \
         COALESCE(o.deviceserialno, '') AS deviceserialno, \
         COALESCE(o.totalprice, 0)::float8 AS totalprice, \
         COALESCE(o.province, '') AS province, \
         COALESCE(o.sendwarehouseid, '') AS sendwarehouseid, \
         COALESCE(o.returnwarehouseid, '') AS returnwarehouseid, \
         COALESCE(o.createdat, '') AS createdat, \
         COALESCE(o.accessories::text, '[]') AS accessories, \
         COALESCE(o.status, '') AS status, \
         COALESCE(o.trackingno, '') AS trackingno, \
         COALESCE(o.devicemodels::text, '{}') AS devicemodels \
         FROM orders o WHERE o.tenant_id = $1 AND o.id = $2",
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };
    let id: String = row.try_get("id")?;
    let devices = load_devices(&mut *connection, tenant_id, &id).await?;
    Ok(Some(map_pg_order(&row, devices)?))
}

async fn load_devices(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    order_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT serialno FROM order_devices \
         WHERE tenant_id = $1 AND orderid = $2 ORDER BY serialno",
    )
    .bind(tenant_id)
    .bind(order_id)
    .fetch_all(connection)
    .await
}

fn map_pg_order(
    row: &sqlx::postgres::PgRow,
    devices: Vec<String>,
) -> Result<OrderReadProjection, sqlx::Error> {
    let pickup_methods: String = row.try_get("pickupmethods")?;
    Ok(OrderReadProjection {
        id: row.try_get("id")?,
        order_no: row.try_get("orderno")?,
        start_date: row.try_get("startdate")?,
        end_date: row.try_get("enddate")?,
        delivery_date: row.try_get("deliverydate")?,
        pickup_methods: serde_json::from_str(&pickup_methods).unwrap_or_default(),
        address: row.try_get("address")?,
        notes: row.try_get("notes")?,
        device_serial_no: row.try_get("deviceserialno")?,
        total_price: row.try_get("totalprice")?,
        province: row.try_get("province")?,
        send_warehouse_id: row.try_get("sendwarehouseid")?,
        return_warehouse_id: row.try_get("returnwarehouseid")?,
        created_at: row.try_get("createdat")?,
        accessories: row.try_get("accessories")?,
        status: row.try_get("status")?,
        tracking_no: row.try_get("trackingno")?,
        device_models: row.try_get("devicemodels")?,
        devices,
    })
}

fn push_predicates<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    request: &'args OrderListRequest,
) {
    query.push("o.tenant_id = ").push_bind(tenant_id);

    push_like(query, "o.orderno", request.order_no.as_deref());
    push_exact(query, "o.status", request.status.as_deref());
    push_like(query, "o.address", request.address.as_deref());
    if let Some(province) = normalized_filter(request.province.as_deref())
        && province != "__all__"
    {
        query.push(" AND o.province = ").push_bind(province);
    }
    if let Some(keyword) = normalized_filter(request.keyword.as_deref()) {
        let pattern = format!("%{keyword}%");
        query
            .push(" AND (o.address ILIKE ")
            .push_bind(pattern.clone())
            .push(" OR o.notes ILIKE ")
            .push_bind(pattern)
            .push(")");
    }

    for (value, column, operator) in [
        (request.start_date_from.as_deref(), "o.startdate", ">="),
        (request.start_date_to.as_deref(), "o.startdate", "<="),
        (request.end_date_from.as_deref(), "o.enddate", ">="),
        (request.end_date_to.as_deref(), "o.enddate", "<="),
        (
            request.delivery_date_from.as_deref(),
            "o.deliverydate",
            ">=",
        ),
        (request.delivery_date_to.as_deref(), "o.deliverydate", "<="),
    ] {
        push_date(query, column, operator, value);
    }

    if let Some(value) = normalized_date_filter(request.included_date.as_deref()) {
        query
            .push(" AND o.startdate <= ")
            .push_bind(value)
            .push(" AND o.enddate >= ")
            .push_bind(value);
    }
    push_date(
        query,
        "o.deliverydate",
        "=",
        request.delivery_date.as_deref(),
    );
    push_date(query, "o.startdate", ">=", request.start_date.as_deref());
    // Preserve the established compatibility behavior: endDate constrains
    // startDate rather than the persisted endDate column.
    push_date(query, "o.startdate", "<=", request.end_date.as_deref());

    let methods: Vec<&str> = request
        .pickup_methods
        .iter()
        .filter_map(|method| normalized_filter(Some(method)))
        .collect();
    if !methods.is_empty() {
        query.push(" AND (");
        for (index, method) in methods.into_iter().enumerate() {
            if index > 0 {
                query.push(" OR ");
            }
            query
                .push("o.pickupmethods ILIKE ")
                .push_bind(format!("%{method}%"));
        }
        query.push(")");
    }

    if let Some(serial_no) = normalized_filter(request.serial_no.as_deref()) {
        let pattern = format!("%{serial_no}%");
        query
            .push(" AND (o.deviceserialno ILIKE ")
            .push_bind(pattern.clone())
            .push(" OR EXISTS (SELECT 1 FROM order_devices od WHERE od.tenant_id = o.tenant_id AND od.orderid = o.id AND od.serialno ILIKE ")
            .push_bind(pattern)
            .push("))");
    }
    push_like(query, "o.trackingno", request.tracking_no.as_deref());
}

fn push_like<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    column: &str,
    value: Option<&'args str>,
) {
    if let Some(value) = normalized_filter(value) {
        query
            .push(" AND ")
            .push(column)
            .push(" ILIKE ")
            .push_bind(format!("%{value}%"));
    }
}

fn push_exact<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    column: &str,
    value: Option<&'args str>,
) {
    if let Some(value) = normalized_filter(value) {
        query
            .push(" AND ")
            .push(column)
            .push(" = ")
            .push_bind(value);
    }
}

fn push_date<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    column: &str,
    operator: &str,
    value: Option<&'args str>,
) {
    if let Some(value) = normalized_date_filter(value) {
        query
            .push(" AND ")
            .push(column)
            .push(" ")
            .push(operator)
            .push(" ")
            .push_bind(value);
    }
}

fn normalized_filter(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn normalized_date_filter(value: Option<&str>) -> Option<&str> {
    normalized_filter(value).filter(|value| is_date_key(value))
}

fn is_date_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn pg_sort_column(field: OrderSortField) -> &'static str {
    match field {
        OrderSortField::StartDate => "o.startdate",
        OrderSortField::EndDate => "o.enddate",
        OrderSortField::TotalPrice => "o.totalprice",
        OrderSortField::CreatedAt => "o.createdat",
    }
}

fn pg_sort_direction(direction: OrderSortDirection) -> &'static str {
    match direction {
        OrderSortDirection::Asc => "ASC",
        OrderSortDirection::Desc => "DESC",
    }
}

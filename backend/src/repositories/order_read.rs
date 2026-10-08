use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::repositories::sqlite::SqliteRepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OrderSortField {
    StartDate,
    EndDate,
    TotalPrice,
    #[default]
    CreatedAt,
}

impl OrderSortField {
    fn sqlite_column(self) -> &'static str {
        match self {
            Self::StartDate => "o.startDate",
            Self::EndDate => "o.endDate",
            Self::TotalPrice => "o.totalPrice",
            Self::CreatedAt => "o.createdAt",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum OrderSortDirection {
    Asc,
    #[default]
    Desc,
}

impl OrderSortDirection {
    fn sqlite_keyword(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderListRequest {
    pub page: u32,
    pub page_size: u32,
    pub offset: Option<u64>,
    pub order_no: Option<String>,
    pub status: Option<String>,
    pub keyword: Option<String>,
    pub address: Option<String>,
    pub province: Option<String>,
    pub start_date_from: Option<String>,
    pub start_date_to: Option<String>,
    pub end_date_from: Option<String>,
    pub end_date_to: Option<String>,
    pub included_date: Option<String>,
    pub delivery_date_from: Option<String>,
    pub delivery_date_to: Option<String>,
    pub delivery_date: Option<String>,
    pub pickup_methods: Vec<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub serial_no: Option<String>,
    pub tracking_no: Option<String>,
    pub sort_by: OrderSortField,
    pub sort_direction: OrderSortDirection,
}

impl Default for OrderListRequest {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: 30,
            offset: None,
            order_no: None,
            status: None,
            keyword: None,
            address: None,
            province: None,
            start_date_from: None,
            start_date_to: None,
            end_date_from: None,
            end_date_to: None,
            included_date: None,
            delivery_date_from: None,
            delivery_date_to: None,
            delivery_date: None,
            pickup_methods: Vec::new(),
            start_date: None,
            end_date: None,
            serial_no: None,
            tracking_no: None,
            sort_by: OrderSortField::CreatedAt,
            sort_direction: OrderSortDirection::Desc,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderReadProjection {
    pub id: String,
    pub order_no: String,
    pub start_date: String,
    pub end_date: String,
    pub delivery_date: String,
    pub pickup_methods: Vec<String>,
    pub address: String,
    pub notes: String,
    pub device_serial_no: String,
    pub total_price: f64,
    pub province: String,
    pub send_warehouse_id: String,
    pub return_warehouse_id: String,
    pub created_at: String,
    pub accessories: String,
    pub status: String,
    pub tracking_no: String,
    pub device_models: String,
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderReadPage {
    pub orders: Vec<OrderReadProjection>,
    pub page: u32,
    pub page_size: u32,
    pub total: u32,
    pub total_pages: u32,
}

pub struct ScopedOrderReadRepository<'a> {
    session: &'a SqliteRepositorySession,
}

impl<'a> ScopedOrderReadRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub fn list(&self, request: &OrderListRequest) -> Result<OrderReadPage, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let page = request.page.max(1);
        let page_size = request.page_size.clamp(1, 200);
        let offset = request
            .offset
            .unwrap_or_else(|| u64::from(page - 1) * u64::from(page_size));

        self.session.read(|connection| {
            let mut predicates = vec!["o.tenant_id = ?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id.clone())];

            push_like_filter(
                &mut predicates,
                &mut values,
                "o.orderNo LIKE ?",
                request.order_no.as_deref(),
            );
            push_exact_filter(
                &mut predicates,
                &mut values,
                "o.status = ?",
                request.status.as_deref(),
            );
            push_like_filter(
                &mut predicates,
                &mut values,
                "o.address LIKE ?",
                request.address.as_deref(),
            );
            if let Some(province) = normalized_filter(request.province.as_deref())
                && province != "__all__"
            {
                predicates.push("o.province = ?".to_owned());
                values.push(SqlValue::Text(province.to_owned()));
            }
            if let Some(keyword) = normalized_filter(request.keyword.as_deref()) {
                predicates.push("(o.address LIKE ? OR o.notes LIKE ?)".to_owned());
                let pattern = SqlValue::Text(format!("%{keyword}%"));
                values.push(pattern.clone());
                values.push(pattern);
            }

            for (value, column, operator) in [
                (request.start_date_from.as_deref(), "o.startDate", ">="),
                (request.start_date_to.as_deref(), "o.startDate", "<="),
                (request.end_date_from.as_deref(), "o.endDate", ">="),
                (request.end_date_to.as_deref(), "o.endDate", "<="),
                (
                    request.delivery_date_from.as_deref(),
                    "o.deliveryDate",
                    ">=",
                ),
                (request.delivery_date_to.as_deref(), "o.deliveryDate", "<="),
            ] {
                push_date_filter(&mut predicates, &mut values, column, operator, value);
            }

            if let Some(value) = normalized_date_filter(request.included_date.as_deref()) {
                predicates.push("o.startDate <= ?".to_owned());
                values.push(SqlValue::Text(value.to_owned()));
                predicates.push("o.endDate >= ?".to_owned());
                values.push(SqlValue::Text(value.to_owned()));
            }
            push_date_filter(
                &mut predicates,
                &mut values,
                "o.deliveryDate",
                "=",
                request.delivery_date.as_deref(),
            );
            push_date_filter(
                &mut predicates,
                &mut values,
                "o.startDate",
                ">=",
                request.start_date.as_deref(),
            );
            // Preserve the current official compatibility semantics:
            // endDate constrains the order start date rather than o.endDate.
            push_date_filter(
                &mut predicates,
                &mut values,
                "o.startDate",
                "<=",
                request.end_date.as_deref(),
            );

            let methods: Vec<_> = request
                .pickup_methods
                .iter()
                .map(String::as_str)
                .filter_map(|method| normalized_filter(Some(method)))
                .collect();
            if !methods.is_empty() {
                predicates.push(format!(
                    "({})",
                    vec!["o.pickupMethods LIKE ?"; methods.len()].join(" OR ")
                ));
                values.extend(
                    methods
                        .into_iter()
                        .map(|method| SqlValue::Text(format!("%{method}%"))),
                );
            }

            if let Some(serial_no) = normalized_filter(request.serial_no.as_deref()) {
                predicates.push(
                    "(o.deviceSerialNo LIKE ? OR EXISTS (\
                     SELECT 1 FROM order_devices od \
                     WHERE od.tenant_id = o.tenant_id \
                     AND od.orderId = o.id AND od.serialNo LIKE ?))"
                        .to_owned(),
                );
                let pattern = SqlValue::Text(format!("%{serial_no}%"));
                values.push(pattern.clone());
                values.push(pattern);
            }
            push_like_filter(
                &mut predicates,
                &mut values,
                "o.trackingNo LIKE ?",
                request.tracking_no.as_deref(),
            );

            let where_clause = format!("WHERE {}", predicates.join(" AND "));
            let count_sql = format!("SELECT COUNT(*) FROM orders o {where_clause}");
            let total: u32 =
                connection.query_row(&count_sql, params_from_iter(values.iter()), |row| {
                    row.get(0)
                })?;

            let data_sql = format!(
                "SELECT o.id, o.orderNo, o.startDate, o.endDate, o.deliveryDate, \
                 o.pickupMethods, o.address, o.notes, o.deviceSerialNo, o.totalPrice, \
                 o.province, o.sendWarehouseId, o.returnWarehouseId, o.createdAt, \
                 o.accessories, o.status, o.trackingNo, o.deviceModels \
                 FROM orders o {where_clause} ORDER BY {} {} LIMIT ? OFFSET ?",
                request.sort_by.sqlite_column(),
                request.sort_direction.sqlite_keyword(),
            );
            let mut data_values = values;
            data_values.push(SqlValue::Integer(i64::from(page_size)));
            data_values.push(SqlValue::Integer(i64::try_from(offset).unwrap_or(i64::MAX)));

            let mut statement = connection.prepare(&data_sql)?;
            let rows = statement.query_map(params_from_iter(data_values.iter()), |row| {
                map_order(connection, &tenant_id, row)
            })?;
            let orders = rows.collect::<Result<Vec<_>, _>>()?;
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
        })
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<OrderReadProjection>, RepositoryError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "order read requires a non-empty id".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();

        self.session.read(|connection| {
            connection
                .query_row(
                    "SELECT o.id, o.orderNo, o.startDate, o.endDate, o.deliveryDate, \
                     o.pickupMethods, o.address, o.notes, o.deviceSerialNo, o.totalPrice, \
                     o.province, o.sendWarehouseId, o.returnWarehouseId, o.createdAt, \
                     o.accessories, o.status, o.trackingNo, o.deviceModels \
                     FROM orders o WHERE o.tenant_id = ?1 AND o.id = ?2",
                    params![tenant_id, id],
                    |row| map_order(connection, &tenant_id, row),
                )
                .optional()
        })
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

fn push_like_filter(
    predicates: &mut Vec<String>,
    values: &mut Vec<SqlValue>,
    predicate: &str,
    value: Option<&str>,
) {
    if let Some(value) = normalized_filter(value) {
        predicates.push(predicate.to_owned());
        values.push(SqlValue::Text(format!("%{value}%")));
    }
}

fn push_exact_filter(
    predicates: &mut Vec<String>,
    values: &mut Vec<SqlValue>,
    predicate: &str,
    value: Option<&str>,
) {
    if let Some(value) = normalized_filter(value) {
        predicates.push(predicate.to_owned());
        values.push(SqlValue::Text(value.to_owned()));
    }
}

fn push_date_filter(
    predicates: &mut Vec<String>,
    values: &mut Vec<SqlValue>,
    column: &str,
    operator: &str,
    value: Option<&str>,
) {
    if let Some(value) = normalized_date_filter(value) {
        predicates.push(format!("{column} {operator} ?"));
        values.push(SqlValue::Text(value.to_owned()));
    }
}

fn map_order(
    connection: &rusqlite::Connection,
    tenant_id: &str,
    row: &rusqlite::Row<'_>,
) -> Result<OrderReadProjection, rusqlite::Error> {
    let id: String = row.get(0)?;
    let mut device_statement = connection.prepare(
        "SELECT serialNo FROM order_devices \
         WHERE tenant_id = ?1 AND orderId = ?2 ORDER BY serialNo",
    )?;
    let devices = device_statement
        .query_map(params![tenant_id, id.as_str()], |device_row| {
            device_row.get(0)
        })?
        .collect::<Result<Vec<String>, _>>()?;

    Ok(OrderReadProjection {
        id,
        order_no: row.get(1)?,
        start_date: row.get(2)?,
        end_date: row.get(3)?,
        delivery_date: row.get::<_, String>(4).unwrap_or_default(),
        pickup_methods: serde_json::from_str(
            &row.get::<_, String>(5).unwrap_or_else(|_| "[]".into()),
        )
        .unwrap_or_default(),
        address: row.get::<_, String>(6).unwrap_or_default(),
        notes: row.get::<_, String>(7).unwrap_or_default(),
        device_serial_no: row.get::<_, String>(8).unwrap_or_default(),
        total_price: row.get::<_, f64>(9).unwrap_or_default(),
        province: row.get::<_, String>(10).unwrap_or_default(),
        send_warehouse_id: row.get::<_, String>(11).unwrap_or_default(),
        return_warehouse_id: row.get::<_, String>(12).unwrap_or_default(),
        created_at: row.get::<_, String>(13).unwrap_or_default(),
        accessories: row.get::<_, String>(14).unwrap_or_default(),
        status: row.get::<_, String>(15).unwrap_or_default(),
        tracking_no: row.get::<_, String>(16).unwrap_or_default(),
        device_models: row.get::<_, String>(17).unwrap_or_default(),
        devices,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use crate::repositories::{
        OrderListRequest, OrderSortDirection, OrderSortField, RepositoryProvider,
        SqliteRepositoryProvider,
    };

    fn pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        {
            let connection = pool.get().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE orders (\
                     id TEXT NOT NULL, orderNo TEXT NOT NULL, startDate TEXT NOT NULL, \
                     endDate TEXT NOT NULL, deliveryDate TEXT NOT NULL DEFAULT '', \
                     pickupMethods TEXT NOT NULL DEFAULT '[]', address TEXT NOT NULL DEFAULT '', \
                     notes TEXT NOT NULL DEFAULT '', deviceSerialNo TEXT NOT NULL DEFAULT '', \
                     totalPrice REAL NOT NULL DEFAULT 0, province TEXT NOT NULL DEFAULT '', \
                     sendWarehouseId TEXT NOT NULL DEFAULT '', returnWarehouseId TEXT NOT NULL DEFAULT '', \
                     createdAt TEXT NOT NULL, accessories TEXT NOT NULL DEFAULT '', \
                     status TEXT NOT NULL, trackingNo TEXT NOT NULL DEFAULT '', \
                     deviceModels TEXT NOT NULL DEFAULT '', tenant_id TEXT NOT NULL, \
                     PRIMARY KEY (tenant_id, id));\
                     CREATE TABLE order_devices (\
                     tenant_id TEXT NOT NULL, orderId TEXT NOT NULL, serialNo TEXT NOT NULL);",
                )
                .unwrap();
            insert_order(
                &connection,
                "tenant-a",
                "a-1",
                "1001",
                "2026-08-02",
                "paid",
                "上海",
                "浦东新区",
                "2026-08-01",
                "2026-08-10",
                "2026-08-02",
                r#"["delivery"]"#,
                "SF-A",
                "LEGACY-A",
            );
            insert_order(
                &connection,
                "tenant-a",
                "a-2",
                "1002",
                "2026-08-03",
                "draft",
                "浙江",
                "杭州市",
                "2026-08-11",
                "2026-08-20",
                "2026-08-12",
                r#"["pickup"]"#,
                "SF-B",
                "",
            );
            insert_order(
                &connection,
                "tenant-b",
                "b-1",
                "2001",
                "2026-08-04",
                "paid",
                "上海",
                "浦东新区",
                "2026-08-01",
                "2026-08-10",
                "2026-08-02",
                r#"["delivery"]"#,
                "SF-C",
                "",
            );
            connection
                .execute(
                    "INSERT INTO order_devices (tenant_id, orderId, serialNo) VALUES (?1, ?2, ?3)",
                    params!["tenant-a", "a-1", "DEVICE-A"],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO order_devices (tenant_id, orderId, serialNo) VALUES (?1, ?2, ?3)",
                    params!["tenant-b", "b-1", "DEVICE-B"],
                )
                .unwrap();
        }
        pool
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_order(
        connection: &rusqlite::Connection,
        tenant: &str,
        id: &str,
        order_no: &str,
        created_at: &str,
        status: &str,
        province: &str,
        address: &str,
        start_date: &str,
        end_date: &str,
        delivery_date: &str,
        pickup_methods: &str,
        tracking_no: &str,
        device_serial_no: &str,
    ) {
        connection
            .execute(
                "INSERT INTO orders (\
                 id, orderNo, startDate, endDate, deliveryDate, pickupMethods, address, notes, \
                 deviceSerialNo, totalPrice, province, sendWarehouseId, returnWarehouseId, \
                 createdAt, accessories, status, trackingNo, deviceModels, tenant_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'notes', ?8, 100, ?9, \
                 'send', 'return', ?10, '', ?11, ?12, '', ?13)",
                params![
                    id,
                    order_no,
                    start_date,
                    end_date,
                    delivery_date,
                    pickup_methods,
                    address,
                    device_serial_no,
                    province,
                    created_at,
                    status,
                    tracking_no,
                    tenant,
                ],
            )
            .unwrap();
    }

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("tenant-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("order-revision").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn order_reads_are_tenant_bound_without_tenant_arguments() {
        let provider = SqliteRepositoryProvider::new(pool());
        let tenant_a = provider.bind(&context("tenant-a", "request-a")).unwrap();
        let tenant_b = provider.bind(&context("tenant-b", "request-b")).unwrap();

        assert_eq!(
            tenant_a
                .orders()
                .list(&OrderListRequest::default())
                .unwrap()
                .total,
            2
        );
        assert_eq!(
            tenant_b
                .orders()
                .list(&OrderListRequest::default())
                .unwrap()
                .total,
            1
        );
    }

    #[test]
    fn extended_compatibility_filters_share_the_same_tenant_bound_count_and_rows() {
        let provider = SqliteRepositoryProvider::new(pool());
        let scoped = provider
            .bind(&context("tenant-a", "request-filter"))
            .unwrap();
        let request = OrderListRequest {
            address: Some("浦东".into()),
            province: Some("上海".into()),
            start_date_from: Some("2026-08-01".into()),
            start_date_to: Some("2026-08-01".into()),
            end_date_from: Some("2026-08-10".into()),
            end_date_to: Some("2026-08-10".into()),
            included_date: Some("2026-08-05".into()),
            delivery_date_from: Some("2026-08-02".into()),
            delivery_date_to: Some("2026-08-02".into()),
            delivery_date: Some("2026-08-02".into()),
            pickup_methods: vec!["delivery".into()],
            start_date: Some("2026-08-01".into()),
            end_date: Some("2026-08-01".into()),
            serial_no: Some("DEVICE-A".into()),
            tracking_no: Some("SF-A".into()),
            sort_by: OrderSortField::CreatedAt,
            sort_direction: OrderSortDirection::Asc,
            ..OrderListRequest::default()
        };

        let page = scoped.orders().list(&request).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.orders.len(), 1);
        assert_eq!(page.orders[0].id, "a-1");
        assert_eq!(page.orders[0].devices, vec!["DEVICE-A"]);
    }

    #[test]
    fn invalid_date_filters_are_ignored_like_the_official_compatibility_path() {
        let provider = SqliteRepositoryProvider::new(pool());
        let scoped = provider.bind(&context("tenant-a", "request-date")).unwrap();
        let page = scoped
            .orders()
            .list(&OrderListRequest {
                start_date_from: Some("not-a-date".into()),
                included_date: Some("2026/08/05".into()),
                ..OrderListRequest::default()
            })
            .unwrap();

        assert_eq!(page.total, 2);
    }
}

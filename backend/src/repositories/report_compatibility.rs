use official_order::report::{MonthlyRevenue, ProvinceRevenue, RevenueData, RevenueSummary};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params_from_iter;

use super::RepositoryError;

#[derive(Clone)]
pub(crate) struct SqliteReportCompatibilityRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteReportCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn revenue_data(
        &self,
        tenant_id: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<RevenueData, RepositoryError> {
        let conn = self
            .pool
            .get()
            .map_err(|error| RepositoryError::PoolUnavailable(error.to_string()))?;
        let (where_clause, values) = sqlite_filters(tenant_id, start_date, end_date);

        let summary_sql = format!(
            "SELECT COUNT(*), COALESCE(SUM(o.totalPrice), 0), COALESCE(AVG(o.totalPrice), 0)
             FROM orders o {where_clause}"
        );
        let (total_orders, total_revenue, avg_order_value): (i64, f64, f64) = conn
            .query_row(&summary_sql, params_from_iter(values.iter()), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(sqlite_storage)?;

        let device_sql = format!(
            "SELECT COUNT(DISTINCT od.serialNo)
             FROM order_devices od
             JOIN orders o ON o.id = od.orderId AND o.tenant_id = od.tenant_id
             {where_clause}"
        );
        let devices_rented: i64 = conn
            .query_row(&device_sql, params_from_iter(values.iter()), |row| {
                row.get(0)
            })
            .map_err(sqlite_storage)?;

        let province_sql = format!(
            "SELECT COALESCE(o.province, '未知'), COUNT(*),
                    COALESCE(SUM(o.totalPrice), 0), COALESCE(AVG(o.totalPrice), 0)
             FROM orders o {where_clause}
             GROUP BY o.province
             ORDER BY SUM(o.totalPrice) DESC"
        );
        let mut province_stmt = conn.prepare(&province_sql).map_err(sqlite_storage)?;
        let by_province = province_stmt
            .query_map(params_from_iter(values.iter()), |row| {
                Ok(ProvinceRevenue {
                    province: row.get(0)?,
                    order_count: row.get(1)?,
                    revenue: row.get(2)?,
                    avg_value: row.get(3)?,
                })
            })
            .map_err(sqlite_storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_storage)?;

        let month_sql = format!(
            "SELECT substr(o.startDate, 1, 7), COUNT(*),
                    COALESCE(SUM(o.totalPrice), 0), COALESCE(AVG(o.totalPrice), 0)
             FROM orders o {where_clause}
             GROUP BY substr(o.startDate, 1, 7)
             ORDER BY substr(o.startDate, 1, 7) ASC"
        );
        let mut month_stmt = conn.prepare(&month_sql).map_err(sqlite_storage)?;
        let by_month = month_stmt
            .query_map(params_from_iter(values.iter()), |row| {
                Ok(MonthlyRevenue {
                    month: row.get(0)?,
                    order_count: row.get(1)?,
                    revenue: row.get(2)?,
                    avg_value: row.get(3)?,
                })
            })
            .map_err(sqlite_storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_storage)?;

        Ok(RevenueData {
            summary: RevenueSummary {
                total_orders,
                total_revenue,
                avg_order_value,
                devices_rented,
            },
            by_province,
            by_month,
        })
    }
}

fn sqlite_filters(tenant_id: &str, start_date: &str, end_date: &str) -> (String, Vec<String>) {
    let mut clauses = vec!["o.tenant_id = ?".to_owned()];
    let mut values = vec![tenant_id.to_owned()];
    if !start_date.is_empty() {
        clauses.push("o.startDate >= ?".into());
        values.push(start_date.to_owned());
    }
    if !end_date.is_empty() {
        clauses.push("o.startDate <= ?".into());
        values.push(end_date.to_owned());
    }
    (format!("WHERE {}", clauses.join(" AND ")), values)
}

fn sqlite_storage(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

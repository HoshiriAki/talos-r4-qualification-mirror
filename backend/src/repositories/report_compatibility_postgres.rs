use std::future::Future;

use official_order::report::{MonthlyRevenue, ProvinceRevenue, RevenueData, RevenueSummary};
use sqlx::postgres::PgPool;
use sqlx::{Postgres, QueryBuilder, Row};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::RepositoryError;

#[derive(Clone)]
pub(crate) struct PostgresReportCompatibilityRepository {
    pool: PgPool,
}

impl PostgresReportCompatibilityRepository {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn revenue_data(
        &self,
        tenant_id: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<RevenueData, RepositoryError> {
        let pool = self.pool.clone();
        let tenant_id = tenant_id.to_owned();
        let start_date = start_date.to_owned();
        let end_date = end_date.to_owned();
        run_pg_report(async move {
            let mut summary = QueryBuilder::<Postgres>::new(
                "SELECT COUNT(*)::BIGINT,
                        COALESCE(SUM(o.totalprice), 0)::DOUBLE PRECISION,
                        COALESCE(AVG(o.totalprice), 0)::DOUBLE PRECISION
                 FROM orders o",
            );
            push_filters(&mut summary, &tenant_id, &start_date, &end_date);
            let summary_row = summary.build().fetch_one(&pool).await.map_err(pg_storage)?;
            let total_orders: i64 = summary_row.try_get(0).map_err(pg_storage)?;
            let total_revenue: f64 = summary_row.try_get(1).map_err(pg_storage)?;
            let avg_order_value: f64 = summary_row.try_get(2).map_err(pg_storage)?;

            let mut devices = QueryBuilder::<Postgres>::new(
                "SELECT COUNT(DISTINCT od.serialno)::BIGINT
                 FROM order_devices od
                 JOIN orders o ON o.id = od.orderid AND o.tenant_id = od.tenant_id",
            );
            push_filters(&mut devices, &tenant_id, &start_date, &end_date);
            let devices_rented: i64 = devices
                .build_query_scalar()
                .fetch_one(&pool)
                .await
                .map_err(pg_storage)?;

            let mut province = QueryBuilder::<Postgres>::new(
                "SELECT COALESCE(o.province, '未知'), COUNT(*)::BIGINT,
                        COALESCE(SUM(o.totalprice), 0)::DOUBLE PRECISION,
                        COALESCE(AVG(o.totalprice), 0)::DOUBLE PRECISION
                 FROM orders o",
            );
            push_filters(&mut province, &tenant_id, &start_date, &end_date);
            province.push(
                " GROUP BY o.province
                  ORDER BY SUM(o.totalprice) DESC",
            );
            let by_province = province
                .build()
                .fetch_all(&pool)
                .await
                .map_err(pg_storage)?
                .into_iter()
                .map(|row| {
                    Ok(ProvinceRevenue {
                        province: row.try_get(0).map_err(pg_storage)?,
                        order_count: row.try_get(1).map_err(pg_storage)?,
                        revenue: row.try_get(2).map_err(pg_storage)?,
                        avg_value: row.try_get(3).map_err(pg_storage)?,
                    })
                })
                .collect::<Result<Vec<_>, RepositoryError>>()?;

            let mut month = QueryBuilder::<Postgres>::new(
                "SELECT substr(o.startdate, 1, 7), COUNT(*)::BIGINT,
                        COALESCE(SUM(o.totalprice), 0)::DOUBLE PRECISION,
                        COALESCE(AVG(o.totalprice), 0)::DOUBLE PRECISION
                 FROM orders o",
            );
            push_filters(&mut month, &tenant_id, &start_date, &end_date);
            month.push(
                " GROUP BY substr(o.startdate, 1, 7)
                  ORDER BY substr(o.startdate, 1, 7) ASC",
            );
            let by_month = month
                .build()
                .fetch_all(&pool)
                .await
                .map_err(pg_storage)?
                .into_iter()
                .map(|row| {
                    Ok(MonthlyRevenue {
                        month: row.try_get(0).map_err(pg_storage)?,
                        order_count: row.try_get(1).map_err(pg_storage)?,
                        revenue: row.try_get(2).map_err(pg_storage)?,
                        avg_value: row.try_get(3).map_err(pg_storage)?,
                    })
                })
                .collect::<Result<Vec<_>, RepositoryError>>()?;

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
        })
    }
}

fn push_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    tenant_id: &str,
    start_date: &str,
    end_date: &str,
) {
    builder
        .push(" WHERE o.tenant_id = ")
        .push_bind(tenant_id.to_owned());
    if !start_date.is_empty() {
        builder
            .push(" AND o.startdate >= ")
            .push_bind(start_date.to_owned());
    }
    if !end_date.is_empty() {
        builder
            .push(" AND o.startdate <= ")
            .push_bind(end_date.to_owned());
    }
}

fn pg_storage(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn run_pg_report<T, F>(future: F) -> Result<T, RepositoryError>
where
    F: Future<Output = Result<T, RepositoryError>>,
{
    let handle = Handle::try_current().map_err(|_| {
        RepositoryError::AdapterUnavailable("report compatibility runtime unavailable".into())
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(RepositoryError::AdapterUnavailable(
            "report compatibility requires the multi-thread runtime".into(),
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::dashboard_read::{
    DashboardCountPoint, DashboardModelRankingPoint, DashboardOrderBucket, DashboardOrderWindow,
    DashboardOverdueRow, DashboardOverviewProjection, DashboardProvincePiePoint,
    DashboardProvinceTrendPoint, DashboardRecentOrder, DashboardRevenuePoint,
    DashboardWarehousePoint,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresDashboardReadRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresDashboardReadRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn overview(
        &self,
        today: &str,
        overdue_cutoff: &str,
    ) -> Result<DashboardOverviewProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let today = today.to_owned();
        let overdue_cutoff = overdue_cutoff.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let active_orders: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*)::BIGINT FROM orders
                     WHERE tenant_id=$1 AND startdate<=$2 AND enddate>=$2
                       AND status!='completed'",
                )
                .bind(&tenant_id)
                .bind(&today)
                .fetch_one(&mut *connection)
                .await?;

                let devices_out: i64 = sqlx::query_scalar(
                    "SELECT COUNT(DISTINCT od.serialno)::BIGINT
                     FROM order_devices od
                     JOIN orders o ON o.id=od.orderid AND o.tenant_id=od.tenant_id
                     WHERE od.tenant_id=$1 AND o.startdate<=$2 AND o.enddate>=$2
                       AND o.status!='completed'",
                )
                .bind(&tenant_id)
                .bind(&today)
                .fetch_one(&mut *connection)
                .await?;

                let total_devices: i64 =
                    sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM devices WHERE tenant_id=$1")
                        .bind(&tenant_id)
                        .fetch_one(&mut *connection)
                        .await?;

                let returns_due_today: i64 = sqlx::query_scalar(
                    "SELECT COUNT(DISTINCT od.serialno)::BIGINT
                     FROM order_devices od
                     JOIN orders o ON o.id=od.orderid AND o.tenant_id=od.tenant_id
                     WHERE od.tenant_id=$1 AND o.enddate=$2 AND o.status!='completed'",
                )
                .bind(&tenant_id)
                .bind(&today)
                .fetch_one(&mut *connection)
                .await?;

                let overdue_returns: i64 = sqlx::query_scalar(
                    "SELECT COUNT(DISTINCT od.serialno)::BIGINT
                     FROM order_devices od
                     JOIN orders o ON o.id=od.orderid AND o.tenant_id=od.tenant_id
                     WHERE od.tenant_id=$1 AND o.enddate<$2 AND o.status!='completed'",
                )
                .bind(&tenant_id)
                .bind(&overdue_cutoff)
                .fetch_one(&mut *connection)
                .await?;

                let today_new_orders: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*)::BIGINT FROM orders
                     WHERE tenant_id=$1 AND substr(createdat,1,10)=$2",
                )
                .bind(&tenant_id)
                .bind(&today)
                .fetch_one(&mut *connection)
                .await?;

                let recent_orders = sqlx::query(
                    "SELECT id,orderno,startdate,enddate,COALESCE(province,'') AS province,
                            COALESCE(totalprice,0)::DOUBLE PRECISION AS totalprice,createdat
                     FROM orders WHERE tenant_id=$1
                     ORDER BY createdat DESC LIMIT 5",
                )
                .bind(&tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardRecentOrder {
                        id: row.try_get("id")?,
                        order_no: row.try_get("orderno")?,
                        start_date: row.try_get("startdate")?,
                        end_date: row.try_get("enddate")?,
                        province: row.try_get("province")?,
                        total_price: row.try_get("totalprice")?,
                        created_at: row.try_get("createdat")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;

                let overdue_rows = sqlx::query(
                    "SELECT od.serialno,o.orderno,o.enddate,COALESCE(o.province,'') AS province,
                            GREATEST(0, ($2::date-o.enddate::date))::BIGINT AS daysoverdue
                     FROM order_devices od
                     JOIN orders o ON o.id=od.orderid AND o.tenant_id=od.tenant_id
                     WHERE od.tenant_id=$1 AND o.enddate<$3 AND o.status!='completed'
                     ORDER BY o.enddate ASC",
                )
                .bind(&tenant_id)
                .bind(&today)
                .bind(&overdue_cutoff)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardOverdueRow {
                        serial_no: row.try_get("serialno")?,
                        order_no: row.try_get("orderno")?,
                        end_date: row.try_get("enddate")?,
                        province: row.try_get("province")?,
                        days_overdue: row.try_get("daysoverdue")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;

                let order_buckets = sqlx::query(
                    "SELECT COALESCE(status,'') AS status,COUNT(*)::BIGINT AS count
                     FROM orders WHERE tenant_id=$1
                     GROUP BY status ORDER BY status",
                )
                .bind(&tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardOrderBucket {
                        status: row.try_get("status")?,
                        count: row.try_get("count")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;

                Ok(DashboardOverviewProjection {
                    active_orders,
                    devices_out,
                    total_devices,
                    returns_due_today,
                    overdue_returns,
                    today_new_orders,
                    recent_orders,
                    overdue_rows,
                    order_buckets,
                })
            })
        })
    }

    pub(in crate::repositories) fn order_windows(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<DashboardOrderWindow>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let start_date = start_date.to_owned();
        let end_date = end_date.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT startdate,enddate FROM orders
                     WHERE tenant_id=$1 AND startdate<=$2 AND enddate>=$3
                       AND status!='completed'",
                )
                .bind(tenant_id)
                .bind(end_date)
                .bind(start_date)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardOrderWindow {
                        start_date: row.try_get("startdate")?,
                        end_date: row.try_get("enddate")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn order_daily(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardCountPoint>, RepositoryError> {
        self.daily_count_query(
            "SELECT substr(createdat,1,10) AS date,COUNT(*)::BIGINT AS count
             FROM orders WHERE tenant_id=$1 AND substr(createdat,1,10)>=$2
             GROUP BY substr(createdat,1,10) ORDER BY substr(createdat,1,10)",
            cutoff,
        )
    }

    pub(in crate::repositories) fn revenue_daily(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardRevenuePoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT substr(createdat,1,10) AS date,COUNT(*)::BIGINT AS ordercount,
                            COALESCE(SUM(totalprice),0)::DOUBLE PRECISION AS totalrevenue
                     FROM orders WHERE tenant_id=$1 AND substr(createdat,1,10)>=$2
                     GROUP BY substr(createdat,1,10) ORDER BY substr(createdat,1,10)",
                )
                .bind(tenant_id)
                .bind(cutoff)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardRevenuePoint {
                        date: row.try_get("date")?,
                        order_count: row.try_get("ordercount")?,
                        total_revenue: row.try_get("totalrevenue")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn cancel_daily(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardCountPoint>, RepositoryError> {
        self.daily_count_query(
            "SELECT substr(occurred_at,1,10) AS date,COUNT(*)::BIGINT AS count
             FROM audit_events
             WHERE tenant_id=$1 AND action='order_delete'
               AND substr(occurred_at,1,10)>=$2
             GROUP BY substr(occurred_at,1,10) ORDER BY substr(occurred_at,1,10)",
            cutoff,
        )
    }

    fn daily_count_query(
        &self,
        sql: &'static str,
        cutoff: &str,
    ) -> Result<Vec<DashboardCountPoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(sql)
                    .bind(tenant_id)
                    .bind(cutoff)
                    .fetch_all(&mut *connection)
                    .await?
                    .iter()
                    .map(|row| {
                        Ok(DashboardCountPoint {
                            date: row.try_get("date")?,
                            count: row.try_get("count")?,
                        })
                    })
                    .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn device_status_distribution(
        &self,
    ) -> Result<Vec<DashboardOrderBucket>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT rentalstatus AS status,COUNT(*)::BIGINT AS count
                     FROM devices WHERE tenant_id=$1
                     GROUP BY rentalstatus ORDER BY rentalstatus",
                )
                .bind(tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardOrderBucket {
                        status: row.try_get("status")?,
                        count: row.try_get("count")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn province_pie(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardProvincePiePoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT province AS name,COUNT(*)::BIGINT AS value
                     FROM orders
                     WHERE tenant_id=$1 AND province IS NOT NULL AND province!=''
                       AND substr(createdat,1,10)>=$2
                     GROUP BY province ORDER BY COUNT(*) DESC,province",
                )
                .bind(tenant_id)
                .bind(cutoff)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardProvincePiePoint {
                        name: row.try_get("name")?,
                        value: row.try_get("value")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn province_trend(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardProvinceTrendPoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT substr(createdat,1,10) AS date,province,
                            COUNT(*)::BIGINT AS count
                     FROM orders
                     WHERE tenant_id=$1 AND province IS NOT NULL AND province!=''
                       AND substr(createdat,1,10)>=$2
                     GROUP BY substr(createdat,1,10),province
                     ORDER BY substr(createdat,1,10),province",
                )
                .bind(tenant_id)
                .bind(cutoff)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardProvinceTrendPoint {
                        date: row.try_get("date")?,
                        province: row.try_get("province")?,
                        count: row.try_get("count")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn model_ranking(
        &self,
        cutoff: &str,
        limit: i64,
    ) -> Result<Vec<DashboardModelRankingPoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT dm.name AS modelname,COUNT(od.orderid)::BIGINT AS ordercount,
                            COALESCE(SUM(o.totalprice),0)::DOUBLE PRECISION AS revenue
                     FROM order_devices od
                     JOIN orders o ON o.id=od.orderid AND o.tenant_id=od.tenant_id
                     JOIN devices d ON d.serialno=od.serialno AND d.tenant_id=od.tenant_id
                     JOIN device_models dm ON dm.id=d.modelid AND dm.tenant_id=d.tenant_id
                     WHERE od.tenant_id=$1 AND substr(o.createdat,1,10)>=$2
                     GROUP BY dm.name
                     ORDER BY SUM(o.totalprice) DESC,dm.name LIMIT $3",
                )
                .bind(tenant_id)
                .bind(cutoff)
                .bind(limit)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardModelRankingPoint {
                        model_name: row.try_get("modelname")?,
                        order_count: row.try_get("ordercount")?,
                        revenue: row.try_get("revenue")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn warehouse_stats(
        &self,
    ) -> Result<Vec<DashboardWarehousePoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let current_stock: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*)::BIGINT FROM devices
                     WHERE tenant_id=$1 AND rentalstatus='已入库'",
                )
                .bind(&tenant_id)
                .fetch_one(&mut *connection)
                .await?;

                sqlx::query(
                    "SELECT w.id,w.name,
                            (SELECT COUNT(*)::BIGINT FROM orders o
                             WHERE o.tenant_id=w.tenant_id AND o.sendwarehouseid=w.id
                               AND o.status!='completed') AS outgoing
                     FROM warehouses w
                     WHERE w.tenant_id=$1
                     ORDER BY w.name,w.id",
                )
                .bind(tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(|row| {
                    Ok(DashboardWarehousePoint {
                        warehouse_id: row.try_get("id")?,
                        name: row.try_get("name")?,
                        outgoing: row.try_get("outgoing")?,
                        current_stock,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }
}

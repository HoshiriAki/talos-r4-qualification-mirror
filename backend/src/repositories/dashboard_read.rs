use rusqlite::params;
use serde::Serialize;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardRecentOrder {
    pub id: String,
    pub order_no: String,
    pub start_date: String,
    pub end_date: String,
    pub province: String,
    pub total_price: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardOverdueRow {
    pub serial_no: String,
    pub order_no: String,
    pub end_date: String,
    pub province: String,
    pub days_overdue: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardOrderBucket {
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DashboardOverviewProjection {
    pub active_orders: i64,
    pub devices_out: i64,
    pub total_devices: i64,
    pub returns_due_today: i64,
    pub overdue_returns: i64,
    pub today_new_orders: i64,
    pub recent_orders: Vec<DashboardRecentOrder>,
    pub overdue_rows: Vec<DashboardOverdueRow>,
    pub order_buckets: Vec<DashboardOrderBucket>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardOrderWindow {
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardCountPoint {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardRevenuePoint {
    pub date: String,
    pub order_count: i64,
    pub total_revenue: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardProvincePiePoint {
    pub name: String,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardProvinceTrendPoint {
    pub date: String,
    pub province: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardModelRankingPoint {
    pub model_name: String,
    pub order_count: i64,
    pub revenue: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DashboardWarehousePoint {
    pub warehouse_id: String,
    pub name: String,
    pub outgoing: i64,
    pub current_stock: i64,
}

pub(in crate::repositories) struct SqliteDashboardReadRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteDashboardReadRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn overview(
        &self,
        today: &str,
        overdue_cutoff: &str,
    ) -> Result<DashboardOverviewProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let today = today.to_owned();
        let overdue_cutoff = overdue_cutoff.to_owned();
        self.session.read(move |connection| {
            let active_orders = connection.query_row(
                "SELECT COUNT(*) FROM orders
                 WHERE tenant_id=?1 AND startDate<=?2 AND endDate>=?2
                   AND status!='completed'",
                params![tenant_id, today],
                |row| row.get(0),
            )?;
            let devices_out = connection.query_row(
                "SELECT COUNT(DISTINCT od.serialNo)
                 FROM order_devices od
                 JOIN orders o ON o.id=od.orderId AND o.tenant_id=od.tenant_id
                 WHERE od.tenant_id=?1 AND o.startDate<=?2 AND o.endDate>=?2
                   AND o.status!='completed'",
                params![tenant_id, today],
                |row| row.get(0),
            )?;
            let total_devices = connection.query_row(
                "SELECT COUNT(*) FROM devices WHERE tenant_id=?1",
                params![tenant_id],
                |row| row.get(0),
            )?;
            let returns_due_today = connection.query_row(
                "SELECT COUNT(DISTINCT od.serialNo)
                 FROM order_devices od
                 JOIN orders o ON o.id=od.orderId AND o.tenant_id=od.tenant_id
                 WHERE od.tenant_id=?1 AND o.endDate=?2 AND o.status!='completed'",
                params![tenant_id, today],
                |row| row.get(0),
            )?;
            let overdue_returns = connection.query_row(
                "SELECT COUNT(DISTINCT od.serialNo)
                 FROM order_devices od
                 JOIN orders o ON o.id=od.orderId AND o.tenant_id=od.tenant_id
                 WHERE od.tenant_id=?1 AND o.endDate<?2 AND o.status!='completed'",
                params![tenant_id, overdue_cutoff],
                |row| row.get(0),
            )?;
            let today_new_orders = connection.query_row(
                "SELECT COUNT(*) FROM orders
                 WHERE tenant_id=?1 AND substr(createdAt,1,10)=?2",
                params![tenant_id, today],
                |row| row.get(0),
            )?;

            let recent_orders = {
                let mut statement = connection.prepare(
                    "SELECT id,orderNo,startDate,endDate,COALESCE(province,''),
                            COALESCE(totalPrice,0),createdAt
                     FROM orders WHERE tenant_id=?1
                     ORDER BY createdAt DESC LIMIT 5",
                )?;
                statement
                    .query_map(params![tenant_id], |row| {
                        Ok(DashboardRecentOrder {
                            id: row.get(0)?,
                            order_no: row.get(1)?,
                            start_date: row.get(2)?,
                            end_date: row.get(3)?,
                            province: row.get(4)?,
                            total_price: row.get(5)?,
                            created_at: row.get(6)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };

            let overdue_rows = {
                let mut statement = connection.prepare(
                    "SELECT od.serialNo,o.orderNo,o.endDate,COALESCE(o.province,''),
                            CAST(julianday(?2)-julianday(o.endDate) AS INTEGER)
                     FROM order_devices od
                     JOIN orders o ON o.id=od.orderId AND o.tenant_id=od.tenant_id
                     WHERE od.tenant_id=?1 AND o.endDate<?3 AND o.status!='completed'
                     ORDER BY o.endDate ASC",
                )?;
                statement
                    .query_map(params![tenant_id, today, overdue_cutoff], |row| {
                        Ok(DashboardOverdueRow {
                            serial_no: row.get(0)?,
                            order_no: row.get(1)?,
                            end_date: row.get(2)?,
                            province: row.get(3)?,
                            days_overdue: row.get(4)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };

            let order_buckets = {
                let mut statement = connection.prepare(
                    "SELECT COALESCE(status,''),COUNT(*)
                     FROM orders WHERE tenant_id=?1
                     GROUP BY status ORDER BY status",
                )?;
                statement
                    .query_map(params![tenant_id], |row| {
                        Ok(DashboardOrderBucket {
                            status: row.get(0)?,
                            count: row.get(1)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };

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
    }

    pub(in crate::repositories) fn order_windows(
        &self,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<DashboardOrderWindow>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let start_date = start_date.to_owned();
        let end_date = end_date.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT startDate,endDate FROM orders
                 WHERE tenant_id=?1 AND startDate<=?2 AND endDate>=?3
                   AND status!='completed'",
            )?;
            statement
                .query_map(params![tenant_id, end_date, start_date], |row| {
                    Ok(DashboardOrderWindow {
                        start_date: row.get(0)?,
                        end_date: row.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn order_daily(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardCountPoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT substr(createdAt,1,10),COUNT(*)
                 FROM orders
                 WHERE tenant_id=?1 AND substr(createdAt,1,10)>=?2
                 GROUP BY substr(createdAt,1,10)
                 ORDER BY substr(createdAt,1,10)",
            )?;
            statement
                .query_map(params![tenant_id, cutoff], |row| {
                    Ok(DashboardCountPoint {
                        date: row.get(0)?,
                        count: row.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn revenue_daily(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardRevenuePoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT substr(createdAt,1,10),COUNT(*),COALESCE(SUM(totalPrice),0)
                 FROM orders
                 WHERE tenant_id=?1 AND substr(createdAt,1,10)>=?2
                 GROUP BY substr(createdAt,1,10)
                 ORDER BY substr(createdAt,1,10)",
            )?;
            statement
                .query_map(params![tenant_id, cutoff], |row| {
                    Ok(DashboardRevenuePoint {
                        date: row.get(0)?,
                        order_count: row.get(1)?,
                        total_revenue: row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn cancel_daily(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardCountPoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT substr(occurred_at,1,10),COUNT(*)
                 FROM audit_events
                 WHERE tenant_id=?1 AND action='order_delete'
                   AND substr(occurred_at,1,10)>=?2
                 GROUP BY substr(occurred_at,1,10)
                 ORDER BY substr(occurred_at,1,10)",
            )?;
            statement
                .query_map(params![tenant_id, cutoff], |row| {
                    Ok(DashboardCountPoint {
                        date: row.get(0)?,
                        count: row.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn device_status_distribution(
        &self,
    ) -> Result<Vec<DashboardOrderBucket>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT rentalStatus,COUNT(*) FROM devices
                 WHERE tenant_id=?1 GROUP BY rentalStatus ORDER BY rentalStatus",
            )?;
            statement
                .query_map(params![tenant_id], |row| {
                    Ok(DashboardOrderBucket {
                        status: row.get(0)?,
                        count: row.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn province_pie(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardProvincePiePoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT province,COUNT(*) FROM orders
                 WHERE tenant_id=?1 AND province IS NOT NULL AND province!=''
                   AND substr(createdAt,1,10)>=?2
                 GROUP BY province ORDER BY COUNT(*) DESC,province",
            )?;
            statement
                .query_map(params![tenant_id, cutoff], |row| {
                    Ok(DashboardProvincePiePoint {
                        name: row.get(0)?,
                        value: row.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn province_trend(
        &self,
        cutoff: &str,
    ) -> Result<Vec<DashboardProvinceTrendPoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT substr(createdAt,1,10),province,COUNT(*) FROM orders
                 WHERE tenant_id=?1 AND province IS NOT NULL AND province!=''
                   AND substr(createdAt,1,10)>=?2
                 GROUP BY substr(createdAt,1,10),province
                 ORDER BY substr(createdAt,1,10),province",
            )?;
            statement
                .query_map(params![tenant_id, cutoff], |row| {
                    Ok(DashboardProvinceTrendPoint {
                        date: row.get(0)?,
                        province: row.get(1)?,
                        count: row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn model_ranking(
        &self,
        cutoff: &str,
        limit: i64,
    ) -> Result<Vec<DashboardModelRankingPoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let cutoff = cutoff.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT dm.name,COUNT(od.orderId),COALESCE(SUM(o.totalPrice),0)
                 FROM order_devices od
                 JOIN orders o ON o.id=od.orderId AND o.tenant_id=od.tenant_id
                 JOIN devices d ON d.serialNo=od.serialNo AND d.tenant_id=od.tenant_id
                 JOIN device_models dm ON dm.id=d.modelId AND dm.tenant_id=d.tenant_id
                 WHERE od.tenant_id=?1 AND substr(o.createdAt,1,10)>=?2
                 GROUP BY dm.name ORDER BY SUM(o.totalPrice) DESC,dm.name LIMIT ?3",
            )?;
            statement
                .query_map(params![tenant_id, cutoff, limit], |row| {
                    Ok(DashboardModelRankingPoint {
                        model_name: row.get(0)?,
                        order_count: row.get(1)?,
                        revenue: row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn warehouse_stats(
        &self,
    ) -> Result<Vec<DashboardWarehousePoint>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(move |connection| {
            let current_stock: i64 = connection.query_row(
                "SELECT COUNT(*) FROM devices
                 WHERE tenant_id=?1 AND rentalStatus='已入库'",
                params![tenant_id],
                |row| row.get(0),
            )?;
            let mut statement = connection.prepare(
                "SELECT w.id,w.name,
                        (SELECT COUNT(*) FROM orders o
                         WHERE o.tenant_id=w.tenant_id AND o.sendWarehouseId=w.id
                           AND o.status!='completed')
                 FROM warehouses w
                 WHERE w.tenant_id=?1
                 ORDER BY w.name,w.id",
            )?;
            statement
                .query_map(params![tenant_id], |row| {
                    Ok(DashboardWarehousePoint {
                        warehouse_id: row.get(0)?,
                        name: row.get(1)?,
                        outgoing: row.get(2)?,
                        current_stock,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()
        })
    }
}

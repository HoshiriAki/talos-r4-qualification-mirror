use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{Datelike, Duration, NaiveDate};
use serde_json::{Value, json};
use system_core::ExecutionContext;

use crate::repositories::{
    DashboardCountPoint, DashboardOverviewProjection, DashboardRevenuePoint, RepositoryError,
    RepositoryProvider,
};
use crate::utils::time;

const OVERDUE_GRACE_DAYS: i64 = 4;

#[derive(Debug)]
pub enum DashboardReadAuthorityError {
    Persistence { code: &'static str },
}

impl From<RepositoryError> for DashboardReadAuthorityError {
    fn from(value: RepositoryError) -> Self {
        Self::Persistence { code: value.code() }
    }
}

#[derive(Clone)]
pub struct DashboardReadAuthorityService {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl DashboardReadAuthorityService {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    pub fn overview(&self, ctx: &ExecutionContext) -> Result<Value, DashboardReadAuthorityError> {
        let today = time::shanghai_now_date_key();
        let overdue_cutoff = time::add_days_to_date_key(&today, -OVERDUE_GRACE_DAYS);
        let scoped = self.repository_provider.bind(ctx)?;
        let projection = scoped.dashboards().overview(&today, &overdue_cutoff)?;
        Ok(overview_json(projection, &today))
    }

    pub fn daily_order_counts(
        &self,
        ctx: &ExecutionContext,
        days: i64,
    ) -> Result<Vec<Value>, DashboardReadAuthorityError> {
        let days = days.clamp(1, 60);
        let today = time::shanghai_now_date_key();
        let last_date = time::add_days_to_date_key(&today, days - 1);
        let scoped = self.repository_provider.bind(ctx)?;
        let active_orders = scoped.dashboards().order_windows(&today, &last_date)?;

        Ok((0..days)
            .map(|offset| {
                let date_key = time::add_days_to_date_key(&today, offset);
                let count = active_orders
                    .iter()
                    .filter(|order| {
                        order.start_date.as_str() <= date_key.as_str()
                            && order.end_date.as_str() >= date_key.as_str()
                    })
                    .count() as i64;
                json!({ "dateKey": date_key, "count": count })
            })
            .collect())
    }

    pub fn order_trend(
        &self,
        ctx: &ExecutionContext,
        granularity: &str,
        days: i64,
    ) -> Result<Value, DashboardReadAuthorityError> {
        let days = days.clamp(1, 365);
        let cutoff = cutoff_date(days);
        let scoped = self.repository_provider.bind(ctx)?;
        let rows = scoped.dashboards().order_daily(&cutoff)?;
        Ok(json!({
            "data": rebucket_counts(rows, granularity),
            "granularity": granularity,
            "days": days,
        }))
    }

    pub fn revenue_trend(
        &self,
        ctx: &ExecutionContext,
        granularity: &str,
        days: i64,
    ) -> Result<Value, DashboardReadAuthorityError> {
        let days = days.clamp(1, 365);
        let cutoff = cutoff_date(days);
        let scoped = self.repository_provider.bind(ctx)?;
        let rows = scoped.dashboards().revenue_daily(&cutoff)?;
        Ok(json!({
            "data": rebucket_revenue(rows, granularity),
            "granularity": granularity,
            "days": days,
        }))
    }

    pub fn cancel_trend(
        &self,
        ctx: &ExecutionContext,
        days: i64,
    ) -> Result<Value, DashboardReadAuthorityError> {
        let days = days.clamp(1, 365);
        let cutoff = cutoff_date(days);
        let scoped = self.repository_provider.bind(ctx)?;
        let rows = scoped.dashboards().cancel_daily(&cutoff)?;
        Ok(json!({ "data": rows, "days": days }))
    }

    pub fn device_status_distribution(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<Value, DashboardReadAuthorityError> {
        let scoped = self.repository_provider.bind(ctx)?;
        let rows = scoped.dashboards().device_status_distribution()?;
        Ok(json!({ "data": rows }))
    }

    pub fn province_stats(
        &self,
        ctx: &ExecutionContext,
        stat_type: &str,
        days: i64,
    ) -> Result<Value, DashboardReadAuthorityError> {
        let days = days.clamp(1, 365);
        let cutoff = cutoff_date(days);
        let scoped = self.repository_provider.bind(ctx)?;
        let pie = scoped.dashboards().province_pie(&cutoff)?;
        let trend = if stat_type == "trend" {
            scoped.dashboards().province_trend(&cutoff)?
        } else {
            Vec::new()
        };
        Ok(json!({ "pie": pie, "trend": trend, "days": days }))
    }

    pub fn model_ranking(
        &self,
        ctx: &ExecutionContext,
        days: i64,
        limit: i64,
    ) -> Result<Value, DashboardReadAuthorityError> {
        let days = days.clamp(1, 365);
        let limit = limit.clamp(1, 100);
        let cutoff = cutoff_date(days);
        let scoped = self.repository_provider.bind(ctx)?;
        let rows = scoped.dashboards().model_ranking(&cutoff, limit)?;
        Ok(json!({ "data": rows, "days": days }))
    }

    pub fn warehouse_stats(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<Value, DashboardReadAuthorityError> {
        let scoped = self.repository_provider.bind(ctx)?;
        Ok(json!({ "data": scoped.dashboards().warehouse_stats()? }))
    }
}

fn cutoff_date(days: i64) -> String {
    time::add_days_to_date_key(&time::shanghai_now_date_key(), -days)
}

fn overview_json(projection: DashboardOverviewProjection, today: &str) -> Value {
    let DashboardOverviewProjection {
        active_orders,
        devices_out,
        total_devices,
        returns_due_today,
        overdue_returns,
        today_new_orders,
        recent_orders,
        overdue_rows,
        order_buckets,
    } = projection;

    let mut overdue_map: HashMap<String, Value> = HashMap::new();
    for row in overdue_rows {
        if let Some(existing) = overdue_map.get_mut(&row.serial_no) {
            let existing_days = existing["daysOverdue"].as_i64().unwrap_or(0);
            if row.days_overdue > existing_days {
                existing["daysOverdue"] = json!(row.days_overdue);
            }
            if row.end_date.as_str() < existing["endDate"].as_str().unwrap_or("") {
                existing["endDate"] = json!(row.end_date);
            }
            if let Some(order_nos) = existing["orderNos"].as_array_mut() {
                order_nos.push(json!(row.order_no));
            }
        } else {
            overdue_map.insert(
                row.serial_no.clone(),
                json!({
                    "serialNo": row.serial_no,
                    "orderNos": [row.order_no],
                    "endDate": row.end_date,
                    "province": row.province,
                    "daysOverdue": row.days_overdue,
                }),
            );
        }
    }

    let scene_nodes = vec![
        json!({
            "id": "node-active",
            "kind": "device-cluster",
            "label": "在租设备",
            "status": "normal",
            "count": devices_out,
            "route": {"name": "devices"}
        }),
        json!({
            "id": "node-returns",
            "kind": "return-risk",
            "label": "今日待还",
            "status": if returns_due_today > 8 { "warning" } else { "normal" },
            "count": returns_due_today,
            "route": {"name": "customers", "query": {"endDate": today}}
        }),
        json!({
            "id": "node-overdue",
            "kind": "return-risk",
            "label": "逾期设备",
            "status": if overdue_returns > 0 { "critical" } else { "normal" },
            "count": overdue_returns,
            "route": {"name": "customers"}
        }),
    ];

    json!({
        "version": 2,
        "asOf": today,
        "activeOrders": active_orders,
        "devicesOut": devices_out,
        "totalDevices": total_devices,
        "returnsDueToday": returns_due_today,
        "overdueReturns": overdue_returns,
        "todayNewOrders": today_new_orders,
        "recentOrders": recent_orders,
        "overdueDetails": overdue_map.into_values().collect::<Vec<_>>(),
        "orderBuckets": order_buckets,
        "sceneNodes": scene_nodes,
    })
}

fn bucket_key_and_date(date: &str, granularity: &str) -> (String, String) {
    if granularity == "month" {
        let month = date.get(..7).unwrap_or(date).to_owned();
        return (month.clone(), format!("{month}-01"));
    }
    if granularity == "week"
        && let Ok(day) = NaiveDate::parse_from_str(date, "%Y-%m-%d")
    {
        let key = day.format("%Y-%W").to_string();
        let days_to_sunday = i64::from((7 - day.weekday().num_days_from_sunday()) % 7);
        let label = (day + Duration::days(days_to_sunday))
            .format("%Y-%m-%d")
            .to_string();
        return (key, label);
    }
    (date.to_owned(), date.to_owned())
}

fn rebucket_counts(rows: Vec<DashboardCountPoint>, granularity: &str) -> Vec<Value> {
    let mut buckets: BTreeMap<String, (String, i64)> = BTreeMap::new();
    for row in rows {
        let (key, date) = bucket_key_and_date(&row.date, granularity);
        let bucket = buckets.entry(key).or_insert((date, 0));
        bucket.1 += row.count;
    }
    buckets
        .into_values()
        .map(|(date, count)| json!({ "date": date, "count": count }))
        .collect()
}

fn rebucket_revenue(rows: Vec<DashboardRevenuePoint>, granularity: &str) -> Vec<Value> {
    let mut buckets: BTreeMap<String, (String, i64, f64)> = BTreeMap::new();
    for row in rows {
        let (key, date) = bucket_key_and_date(&row.date, granularity);
        let bucket = buckets.entry(key).or_insert((date, 0, 0.0));
        bucket.1 += row.order_count;
        bucket.2 += row.total_revenue;
    }
    buckets
        .into_values()
        .map(|(date, order_count, total_revenue)| {
            json!({
                "date": date,
                "orderCount": order_count,
                "totalRevenue": total_revenue,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{bucket_key_and_date, rebucket_counts};
    use crate::repositories::DashboardCountPoint;

    #[test]
    fn dashboard_week_bucket_uses_legacy_sunday_label() {
        let (key, date) = bucket_key_and_date("2026-09-25", "week");
        assert_eq!(key, "2026-38");
        assert_eq!(date, "2026-09-27");
    }

    #[test]
    fn dashboard_rebucket_sums_daily_counts() {
        let rows = vec![
            DashboardCountPoint {
                date: "2026-09-01".into(),
                count: 2,
            },
            DashboardCountPoint {
                date: "2026-09-15".into(),
                count: 3,
            },
        ];
        let result = rebucket_counts(rows, "month");
        assert_eq!(result, vec![json!({"date":"2026-09-01","count":5})]);
    }
}

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{TransactionBehavior, params};

use crate::error::AppError;

const OVERDUE_GRACE_DAYS: i64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MaintenanceStatusSync {
    pub reserved: usize,
    pub activated: usize,
}

#[derive(Clone)]
pub(crate) struct SqliteMaintenanceCompatibilityRepository {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteMaintenanceCompatibilityRepository {
    pub(crate) fn new(pool: Pool<SqliteConnectionManager>) -> Self {
        Self { pool }
    }

    pub(crate) fn sync_order_statuses(
        &self,
        today: &str,
    ) -> Result<MaintenanceStatusSync, AppError> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let reserved = tx.execute(
            "UPDATE orders
             SET status='reserved'
             WHERE deliveryDate > ?1 AND status != 'completed'",
            params![today],
        )?;
        let activated = tx.execute(
            "UPDATE orders
             SET status='active'
             WHERE deliveryDate <= ?1 AND status='reserved'",
            params![today],
        )?;
        tx.commit()?;
        Ok(MaintenanceStatusSync {
            reserved,
            activated,
        })
    }

    pub(crate) fn seed_overdue_tasks(&self, today: &str, now: &str) -> Result<usize, AppError> {
        let mut conn = self.pool.get()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut stmt = tx.prepare(
            "SELECT COALESCE(od.serialNo, o.deviceSerialNo), o.id, o.orderNo, o.endDate,
                    CAST(julianday(?1) - julianday(o.endDate) AS INTEGER), o.tenant_id
             FROM orders o
             LEFT JOIN order_devices od
               ON o.id=od.orderId AND o.tenant_id=od.tenant_id
             WHERE COALESCE(od.serialNo, o.deviceSerialNo) IS NOT NULL
               AND o.endDate < date(?2, '-' || ?3 || ' days')
               AND o.status IN ('active','in_use')
             ORDER BY o.endDate ASC",
        )?;
        let rows = stmt
            .query_map(params![today, today, OVERDUE_GRACE_DAYS], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut created = 0usize;
        for (serial_no, order_id, order_no, end_date, overdue_days, tenant_id) in rows {
            let task_id = format!("overdue-device-{order_id}-{serial_no}");
            let risk = overdue_risk(overdue_days);
            let capabilities = overdue_capabilities(risk);
            let work_route = serde_json::json!({
                "name": "customers",
                "query": { "orderId": order_id }
            })
            .to_string();
            let changed = tx.execute(
                "INSERT OR IGNORE INTO work_tasks
                 (id,kind,status,risk,source_type,source_id,title,summary,reason,due_at,
                  capabilities_json,work_route_json,created_at,updated_at,tenant_id)
                 VALUES (?1,'overdue-device','queued',?2,'device',?3,?4,?5,?6,?7,?8,?9,?10,?10,?11)",
                params![
                    task_id,
                    risk,
                    serial_no,
                    format!("{serial_no} / {order_no}"),
                    format!("逾期{overdue_days}天 (截止{end_date})"),
                    format!("设备超期，截止{end_date}"),
                    format!("{end_date}T23:59:59+08:00"),
                    capabilities,
                    work_route,
                    now,
                    tenant_id,
                ],
            )?;
            created += changed;
        }
        tx.commit()?;
        Ok(created)
    }
}

pub(crate) fn overdue_risk(overdue_days: i64) -> &'static str {
    if overdue_days > 7 {
        "high"
    } else if overdue_days > 3 {
        "medium"
    } else {
        "low"
    }
}

pub(crate) fn overdue_capabilities(risk: &str) -> &'static str {
    if risk == "high" {
        r#"["defer","open_work","send_to_pc"]"#
    } else {
        r#"["acknowledge","defer","open_work","send_to_pc"]"#
    }
}

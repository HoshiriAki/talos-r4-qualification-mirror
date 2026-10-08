use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::Serialize;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyReservationProjection {
    pub id: i64,
    pub device_serial_no: String,
    pub warehouse_id: Option<String>,
    pub order_id: Option<String>,
    pub customer_name: String,
    pub customer_phone: String,
    pub start_date: String,
    pub end_date: String,
    pub status: String,
    pub notes: Option<String>,
    pub reserved_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyReservationConflict {
    pub id: i64,
    pub customer_name: String,
    pub customer_phone: String,
    pub start_date: String,
    pub end_date: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LegacyReservationList {
    pub items: Vec<LegacyReservationProjection>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyReservationRule {
    pub id: i64,
    pub rule_name: String,
    pub max_days_ahead: i64,
    pub max_concurrent_per_customer: i64,
    pub auto_release_minutes: i64,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LegacyReservationRulePatch {
    pub max_days_ahead: Option<i64>,
    pub max_concurrent_per_customer: Option<i64>,
    pub auto_release_minutes: Option<i64>,
    pub is_active: Option<bool>,
}

pub(in crate::repositories) struct SqliteLegacyReservationRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteLegacyReservationRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn list(
        &self,
        status: Option<&str>,
        device_serial_no: Option<&str>,
        customer_phone: Option<&str>,
        warehouse_id: Option<i64>,
        page: i64,
        page_size: i64,
    ) -> Result<LegacyReservationList, RepositoryError> {
        let tenant_id = self.tenant_id();
        let status = nonempty(status);
        let device_serial_no = nonempty(device_serial_no);
        let customer_phone = nonempty(customer_phone);
        let warehouse_id = warehouse_id.map(|value| value.to_string());
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut predicates = vec!["tenant_id = ?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id)];
            if let Some(status) = status {
                predicates.push("status = ?".into());
                values.push(SqlValue::Text(status));
            }
            if let Some(serial) = device_serial_no {
                predicates.push("device_serial_no = ?".into());
                values.push(SqlValue::Text(serial));
            }
            if let Some(phone) = customer_phone {
                predicates.push("customer_phone = ?".into());
                values.push(SqlValue::Text(phone));
            }
            if let Some(warehouse_id) = warehouse_id {
                predicates.push("CAST(warehouse_id AS TEXT) = ?".into());
                values.push(SqlValue::Text(warehouse_id));
            }
            let where_sql = predicates.join(" AND ");

            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM inventory_reservations WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;

            let sql = format!(
                "SELECT id,device_serial_no,warehouse_id,order_id,customer_name,customer_phone,
                        start_date,end_date,status,notes,reserved_by,created_at,updated_at
                 FROM inventory_reservations
                 WHERE {where_sql}
                 ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
            );
            let mut statement = connection.prepare(&sql)?;
            let items = statement
                .query_map(params_from_iter(values.iter()), map_projection)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(LegacyReservationList {
                items,
                total,
                page,
                page_size,
            })
        })
    }

    pub(in crate::repositories) fn get(
        &self,
        id: i64,
    ) -> Result<Option<LegacyReservationProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,device_serial_no,warehouse_id,order_id,customer_name,customer_phone,
                            start_date,end_date,status,notes,reserved_by,created_at,updated_at
                     FROM inventory_reservations
                     WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                    params![tenant_id, id],
                    map_projection,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn conflicts(
        &self,
        device_serial_no: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<LegacyReservationConflict>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let serial = device_serial_no.to_owned();
        let start = start_date.to_owned();
        let end = end_date.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT id,customer_name,customer_phone,start_date,end_date,status
                 FROM inventory_reservations
                 WHERE tenant_id=?1 AND device_serial_no=?2
                   AND status IN ('reserved','confirmed')
                   AND ?3 < end_date AND start_date < ?4
                 ORDER BY created_at,id",
            )?;
            statement
                .query_map(params![tenant_id, serial, start, end], |row| {
                    Ok(LegacyReservationConflict {
                        id: row.get(0)?,
                        customer_name: row.get(1)?,
                        customer_phone: row.get(2)?,
                        start_date: row.get(3)?,
                        end_date: row.get(4)?,
                        status: row.get(5)?,
                    })
                })?
                .collect()
        })
    }

    pub(in crate::repositories) fn get_rule(
        &self,
    ) -> Result<Option<LegacyReservationRule>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,rule_name,max_days_ahead,max_concurrent_per_customer,
                            auto_release_minutes,is_active,created_at,updated_at
                     FROM reservation_rules
                     WHERE tenant_id=?1
                     ORDER BY is_active DESC,id LIMIT 1",
                    params![tenant_id],
                    |row| {
                        Ok(LegacyReservationRule {
                            id: row.get(0)?,
                            rule_name: row.get(1)?,
                            max_days_ahead: row.get(2)?,
                            max_concurrent_per_customer: row.get(3)?,
                            auto_release_minutes: row.get(4)?,
                            is_active: row.get::<_, i64>(5)? != 0,
                            created_at: row.get(6)?,
                            updated_at: row.get(7)?,
                        })
                    },
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn update_rule(
        &self,
        patch: &LegacyReservationRulePatch,
        now: &str,
    ) -> Result<LegacyReservationRule, RepositoryError> {
        let tenant_id = self.tenant_id();
        let patch = patch.clone();
        let now = now.to_owned();
        self.session.write_immediate(move |transaction| {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO reservation_rules
                     (rule_name,max_days_ahead,max_concurrent_per_customer,
                      auto_release_minutes,is_active,created_at,updated_at,tenant_id)
                     VALUES ('default',90,2,30,1,?1,?1,?2)",
                    params![now, tenant_id],
                )
                .map_err(sqlite_error)?;

            let current = load_rule_sqlite(transaction, &tenant_id)?;
            let max_days_ahead = patch.max_days_ahead.unwrap_or(current.max_days_ahead);
            let max_concurrent = patch
                .max_concurrent_per_customer
                .unwrap_or(current.max_concurrent_per_customer);
            let auto_release = patch
                .auto_release_minutes
                .unwrap_or(current.auto_release_minutes);
            let is_active = patch.is_active.unwrap_or(current.is_active);

            transaction
                .execute(
                    "UPDATE reservation_rules
                     SET max_days_ahead=?1,max_concurrent_per_customer=?2,
                         auto_release_minutes=?3,is_active=?4,updated_at=?5
                     WHERE tenant_id=?6 AND id=?7",
                    params![
                        max_days_ahead,
                        max_concurrent,
                        auto_release,
                        if is_active { 1_i32 } else { 0_i32 },
                        now,
                        tenant_id,
                        current.id,
                    ],
                )
                .map_err(sqlite_error)?;
            load_rule_sqlite(transaction, &tenant_id)
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn load_rule_sqlite(
    connection: &rusqlite::Connection,
    tenant_id: &str,
) -> Result<LegacyReservationRule, RepositoryError> {
    connection
        .query_row(
            "SELECT id,rule_name,max_days_ahead,max_concurrent_per_customer,
                    auto_release_minutes,is_active,created_at,updated_at
             FROM reservation_rules
             WHERE tenant_id=?1
             ORDER BY is_active DESC,id LIMIT 1",
            params![tenant_id],
            |row| {
                Ok(LegacyReservationRule {
                    id: row.get(0)?,
                    rule_name: row.get(1)?,
                    max_days_ahead: row.get(2)?,
                    max_concurrent_per_customer: row.get(3)?,
                    auto_release_minutes: row.get(4)?,
                    is_active: row.get::<_, i64>(5)? != 0,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .map_err(sqlite_error)
}

fn map_projection(row: &rusqlite::Row<'_>) -> rusqlite::Result<LegacyReservationProjection> {
    Ok(LegacyReservationProjection {
        id: row.get(0)?,
        device_serial_no: row.get(1)?,
        warehouse_id: value_ref_string(row.get_ref(2)?),
        order_id: value_ref_string(row.get_ref(3)?),
        customer_name: row.get(4)?,
        customer_phone: row.get(5)?,
        start_date: row.get(6)?,
        end_date: row.get(7)?,
        status: row.get(8)?,
        notes: row.get(9)?,
        reserved_by: value_ref_string(row.get_ref(10)?).unwrap_or_default(),
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn value_ref_string(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Null => None,
        ValueRef::Integer(value) => Some(value.to_string()),
        ValueRef::Real(value) => Some(value.to_string()),
        ValueRef::Text(value) => Some(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(_) => None,
    }
}

fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use crate::repositories::{
        LegacyReservationRulePatch, RepositoryProvider, SqliteRepositoryProvider,
    };

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("reservation-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(
                tenant_id,
                Revision::new("reservation-test-revision").unwrap(),
            )
            .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_legacy_reservation_compatibility_preserves_scope_conflicts_and_rules() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE inventory_reservations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    device_serial_no TEXT NOT NULL,
                    warehouse_id TEXT,
                    order_id TEXT,
                    customer_name TEXT NOT NULL,
                    customer_phone TEXT NOT NULL,
                    start_date TEXT NOT NULL,
                    end_date TEXT NOT NULL,
                    status TEXT NOT NULL,
                    notes TEXT,
                    reserved_by TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE reservation_rules (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    rule_name TEXT NOT NULL,
                    max_days_ahead INTEGER NOT NULL,
                    max_concurrent_per_customer INTEGER NOT NULL,
                    auto_release_minutes INTEGER NOT NULL,
                    is_active INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL UNIQUE
                );
                INSERT INTO inventory_reservations
                    (device_serial_no,warehouse_id,order_id,customer_name,customer_phone,
                     start_date,end_date,status,notes,reserved_by,created_at,updated_at,tenant_id)
                VALUES
                    ('LEG-A-1','warehouse-a','order-a','Alice','13800000000',
                     '2026-10-10','2026-10-12','reserved','legacy',
                     'identity-a','2026-10-02T10:00:00+08:00','2026-10-02T10:00:00+08:00','tenant-a'),
                    ('LEG-B-1','warehouse-b','order-b','Bob','13900000000',
                     '2026-10-10','2026-10-12','confirmed',NULL,
                     'identity-b','2026-10-02T10:00:00+08:00','2026-10-02T10:00:00+08:00','tenant-b');",
            )
            .unwrap();
        connection
            .execute_batch(
                "INSERT INTO reservation_rules
                    (rule_name,max_days_ahead,max_concurrent_per_customer,
                     auto_release_minutes,is_active,created_at,updated_at,tenant_id)
                 VALUES
                    ('default',90,2,30,1,'now','now','tenant-a'),
                    ('default',90,2,30,1,'now','now','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let scoped_a = provider
            .bind(&context("tenant-a", "reservation-a"))
            .unwrap();
        let scoped_b = provider
            .bind(&context("tenant-b", "reservation-b"))
            .unwrap();

        let a = scoped_a
            .legacy_reservations()
            .list(None, None, None, None, 1, 20)
            .unwrap();
        let b = scoped_b
            .legacy_reservations()
            .list(None, None, None, None, 1, 20)
            .unwrap();
        assert_eq!(a.total, 1);
        assert_eq!(b.total, 1);
        assert_eq!(a.items[0].device_serial_no, "LEG-A-1");
        assert_eq!(a.items[0].reserved_by, "identity-a");
        assert_eq!(a.items[0].order_id.as_deref(), Some("order-a"));
        assert!(
            scoped_a
                .legacy_reservations()
                .get(b.items[0].id)
                .unwrap()
                .is_none()
        );

        let overlap = scoped_a
            .legacy_reservations()
            .conflicts("LEG-A-1", "2026-10-11", "2026-10-13")
            .unwrap();
        assert_eq!(overlap.len(), 1);
        let adjacent = scoped_a
            .legacy_reservations()
            .conflicts("LEG-A-1", "2026-10-12", "2026-10-13")
            .unwrap();
        assert!(adjacent.is_empty());

        let rule_a = scoped_a.legacy_reservations().get_rule().unwrap().unwrap();
        let rule_b = scoped_b.legacy_reservations().get_rule().unwrap().unwrap();
        assert_eq!(rule_a.max_concurrent_per_customer, 2);
        assert_eq!(rule_b.max_concurrent_per_customer, 2);

        let updated = scoped_a
            .legacy_reservations()
            .update_rule(
                &LegacyReservationRulePatch {
                    max_concurrent_per_customer: Some(7),
                    auto_release_minutes: Some(45),
                    ..Default::default()
                },
                "2026-10-02T10:11:00+08:00",
            )
            .unwrap();
        assert_eq!(updated.max_concurrent_per_customer, 7);
        assert_eq!(updated.auto_release_minutes, 45);

        let still_b = scoped_b.legacy_reservations().get_rule().unwrap().unwrap();
        assert_eq!(still_b.max_concurrent_per_customer, 2);
        assert_eq!(still_b.auto_release_minutes, 30);
    }
}

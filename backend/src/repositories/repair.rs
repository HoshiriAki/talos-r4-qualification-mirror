use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const DAMAGE_NOT_FOUND: &str = "repair-damage-not-found";
const DAMAGE_NOT_ADJUDICATED_PREFIX: &str = "repair-damage-not-adjudicated:";
const ALREADY_EXISTS_PREFIX: &str = "repair-already-exists:";
const NOT_FOUND: &str = "repair-not-found";
const NOT_PENDING_PREFIX: &str = "repair-not-pending:";
const NOT_IN_PROGRESS_PREFIX: &str = "repair-not-in-progress:";
const NOT_COMPLETED_PREFIX: &str = "repair-not-completed:";
const DEVICE_NOT_FOUND: &str = "repair-device-not-found";

#[derive(Debug, Clone, PartialEq)]
pub struct RepairCreateOutcome {
    pub repair_id: String,
    pub damage_report_id: String,
    pub device_serial_no: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepairTransitionOutcome {
    pub repair_id: String,
    pub status: String,
    pub device_serial_no: Option<String>,
    pub repair_cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairProjection {
    pub id: String,
    pub damage_report_id: String,
    pub device_serial_no: String,
    pub status: String,
    pub repair_description: String,
    pub repair_cost: f64,
    pub vendor: String,
    pub created_by: String,
    pub completed_by: Option<String>,
    pub returned_by: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub returned_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairListItem {
    pub id: String,
    pub damage_report_id: String,
    pub device_serial_no: String,
    pub status: String,
    pub repair_cost: f64,
    pub vendor: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub returned_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepairListProjection {
    pub repairs: Vec<RepairListItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairHistoryItem {
    pub id: String,
    pub status: String,
    pub repair_cost: f64,
    pub vendor: String,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepairDeviceStatsProjection {
    pub total_repairs: i64,
    pub total_repair_cost: f64,
    pub recent_repairs: Vec<RepairHistoryItem>,
}

#[derive(Debug, thiserror::Error)]
pub enum RepairMutationError {
    #[error("damage report not found")]
    DamageNotFound,
    #[error("damage report is not adjudicated: {0}")]
    DamageNotAdjudicated(String),
    #[error("repair order already exists: {0}")]
    AlreadyExists(String),
    #[error("repair order not found")]
    NotFound,
    #[error("repair order is not pending: {0}")]
    NotPending(String),
    #[error("repair order is not in progress: {0}")]
    NotInProgress(String),
    #[error("repair order is not completed: {0}")]
    NotCompleted(String),
    #[error("repair device not found")]
    DeviceNotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteRepairRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteRepairRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn create(
        &self,
        damage_report_id: &str,
        repair_description: &str,
        vendor: &str,
        operator: &str,
        now: &str,
    ) -> Result<RepairCreateOutcome, RepairMutationError> {
        let tenant_id = self.tenant_id();
        let damage_report_id = damage_report_id.to_owned();
        let repair_description = repair_description.to_owned();
        let vendor = vendor.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let damage = transaction
                    .query_row(
                        "SELECT device_serial_no,status
                         FROM damage_reports
                         WHERE tenant_id=?1 AND id=?2
                         LIMIT 1",
                        params![tenant_id, damage_report_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(DAMAGE_NOT_FOUND.into()))?;
                let (device_serial_no, damage_status) = damage;

                let existing = transaction
                    .query_row(
                        "SELECT id FROM repair_orders
                         WHERE tenant_id=?1 AND damage_report_id=?2
                         LIMIT 1",
                        params![tenant_id, damage_report_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if let Some(existing) = existing {
                    return Err(contract(format!("{ALREADY_EXISTS_PREFIX}{existing}")));
                }
                if damage_status != "adjudicated" {
                    return Err(contract(format!(
                        "{DAMAGE_NOT_ADJUDICATED_PREFIX}{damage_status}"
                    )));
                }

                let repair_id = Uuid::new_v4().to_string();
                transaction
                    .execute(
                        "INSERT INTO repair_orders
                         (id,damage_report_id,device_serial_no,repair_description,vendor,
                          created_by,created_at,updated_at,tenant_id)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?7,?8)",
                        params![
                            repair_id,
                            damage_report_id,
                            device_serial_no,
                            repair_description,
                            vendor,
                            operator,
                            now,
                            tenant_id,
                        ],
                    )
                    .map_err(sqlite_error)?;

                Ok(RepairCreateOutcome {
                    repair_id,
                    damage_report_id,
                    device_serial_no,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn start(
        &self,
        repair_id: &str,
        now: &str,
    ) -> Result<RepairTransitionOutcome, RepairMutationError> {
        let tenant_id = self.tenant_id();
        let repair_id = repair_id.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let status = transaction
                    .query_row(
                        "SELECT status FROM repair_orders
                         WHERE tenant_id=?1 AND id=?2
                         LIMIT 1",
                        params![tenant_id, repair_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(NOT_FOUND.into()))?;
                if status != "pending" {
                    return Err(contract(format!("{NOT_PENDING_PREFIX}{status}")));
                }

                transaction
                    .execute(
                        "UPDATE repair_orders
                         SET status='in_progress',started_at=?1,updated_at=?1
                         WHERE tenant_id=?2 AND id=?3 AND status='pending'",
                        params![now, tenant_id, repair_id],
                    )
                    .map_err(sqlite_error)?;

                Ok(RepairTransitionOutcome {
                    repair_id,
                    status: "in_progress".into(),
                    device_serial_no: None,
                    repair_cost: None,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn complete(
        &self,
        repair_id: &str,
        repair_cost: f64,
        operator: &str,
        now: &str,
    ) -> Result<RepairTransitionOutcome, RepairMutationError> {
        let tenant_id = self.tenant_id();
        let repair_id = repair_id.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let status = transaction
                    .query_row(
                        "SELECT status FROM repair_orders
                         WHERE tenant_id=?1 AND id=?2
                         LIMIT 1",
                        params![tenant_id, repair_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(NOT_FOUND.into()))?;
                if status != "in_progress" {
                    return Err(contract(format!("{NOT_IN_PROGRESS_PREFIX}{status}")));
                }

                transaction
                    .execute(
                        "UPDATE repair_orders
                         SET status='completed',repair_cost=?1,completed_by=?2,
                             completed_at=?3,updated_at=?3
                         WHERE tenant_id=?4 AND id=?5 AND status='in_progress'",
                        params![repair_cost, operator, now, tenant_id, repair_id],
                    )
                    .map_err(sqlite_error)?;

                Ok(RepairTransitionOutcome {
                    repair_id,
                    status: "completed".into(),
                    device_serial_no: None,
                    repair_cost: Some(repair_cost),
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn return_to_stock(
        &self,
        repair_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<RepairTransitionOutcome, RepairMutationError> {
        let tenant_id = self.tenant_id();
        let repair_id = repair_id.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let repair = transaction
                    .query_row(
                        "SELECT status,device_serial_no FROM repair_orders
                         WHERE tenant_id=?1 AND id=?2
                         LIMIT 1",
                        params![tenant_id, repair_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(NOT_FOUND.into()))?;
                let (status, device_serial_no) = repair;
                if status != "completed" {
                    return Err(contract(format!("{NOT_COMPLETED_PREFIX}{status}")));
                }

                let device_updated = transaction
                    .execute(
                        "UPDATE devices SET rentalStatus='available'
                         WHERE tenant_id=?1 AND serialNo=?2",
                        params![tenant_id, device_serial_no],
                    )
                    .map_err(sqlite_error)?;
                if device_updated != 1 {
                    return Err(contract(DEVICE_NOT_FOUND.into()));
                }

                let repair_updated = transaction
                    .execute(
                        "UPDATE repair_orders
                         SET status='returned',returned_by=?1,returned_at=?2,updated_at=?2
                         WHERE tenant_id=?3 AND id=?4 AND status='completed'",
                        params![operator, now, tenant_id, repair_id],
                    )
                    .map_err(sqlite_error)?;
                if repair_updated != 1 {
                    return Err(contract(format!("{NOT_COMPLETED_PREFIX}changed")));
                }

                Ok(RepairTransitionOutcome {
                    repair_id,
                    status: "returned".into(),
                    device_serial_no: Some(device_serial_no),
                    repair_cost: None,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn get(
        &self,
        repair_id: &str,
    ) -> Result<Option<RepairProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let repair_id = repair_id.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,damage_report_id,device_serial_no,status,repair_description,
                            repair_cost,vendor,created_by,completed_by,returned_by,started_at,
                            completed_at,returned_at,created_at,updated_at
                     FROM repair_orders
                     WHERE tenant_id=?1 AND id=?2
                     LIMIT 1",
                    params![tenant_id, repair_id],
                    map_projection,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn list(
        &self,
        status: Option<&str>,
        device_serial_no: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<RepairListProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
        let device_serial_no = device_serial_no
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut conditions = vec!["tenant_id = ?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id)];
            if let Some(status) = status {
                conditions.push("status = ?".into());
                values.push(SqlValue::Text(status));
            }
            if let Some(device_serial_no) = device_serial_no {
                conditions.push("device_serial_no = ?".into());
                values.push(SqlValue::Text(device_serial_no));
            }
            let where_sql = conditions.join(" AND ");

            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM repair_orders WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;

            let sql = format!(
                "SELECT id,damage_report_id,device_serial_no,status,repair_cost,vendor,
                        started_at,completed_at,returned_at,created_at
                 FROM repair_orders
                 WHERE {where_sql}
                 ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
            );
            let mut statement = connection.prepare(&sql)?;
            let repairs = statement
                .query_map(params_from_iter(values.iter()), map_list_item)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(RepairListProjection {
                repairs,
                page,
                page_size,
                total,
            })
        })
    }

    pub(in crate::repositories) fn device_stats(
        &self,
        device_serial_no: &str,
    ) -> Result<RepairDeviceStatsProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let device_serial_no = device_serial_no.to_owned();
        self.session.read(move |connection| {
            let (total_repairs, total_repair_cost): (i64, f64) = connection.query_row(
                "SELECT COUNT(*),COALESCE(SUM(repair_cost),0)
                 FROM repair_orders
                 WHERE tenant_id=?1 AND device_serial_no=?2",
                params![tenant_id, device_serial_no],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;

            let mut statement = connection.prepare(
                "SELECT id,status,repair_cost,vendor,created_at,completed_at
                 FROM repair_orders
                 WHERE tenant_id=?1 AND device_serial_no=?2
                 ORDER BY created_at DESC LIMIT 10",
            )?;
            let recent_repairs = statement
                .query_map(params![tenant_id, device_serial_no], map_history_item)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(RepairDeviceStatsProjection {
                total_repairs,
                total_repair_cost,
                recent_repairs,
            })
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub(in crate::repositories) fn map_mutation_error(error: RepositoryError) -> RepairMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message == DAMAGE_NOT_FOUND {
            return RepairMutationError::DamageNotFound;
        }
        if let Some(status) = message.strip_prefix(DAMAGE_NOT_ADJUDICATED_PREFIX) {
            return RepairMutationError::DamageNotAdjudicated(status.to_owned());
        }
        if let Some(existing) = message.strip_prefix(ALREADY_EXISTS_PREFIX) {
            return RepairMutationError::AlreadyExists(existing.to_owned());
        }
        if message == NOT_FOUND {
            return RepairMutationError::NotFound;
        }
        if let Some(status) = message.strip_prefix(NOT_PENDING_PREFIX) {
            return RepairMutationError::NotPending(status.to_owned());
        }
        if let Some(status) = message.strip_prefix(NOT_IN_PROGRESS_PREFIX) {
            return RepairMutationError::NotInProgress(status.to_owned());
        }
        if let Some(status) = message.strip_prefix(NOT_COMPLETED_PREFIX) {
            return RepairMutationError::NotCompleted(status.to_owned());
        }
        if message == DEVICE_NOT_FOUND {
            return RepairMutationError::DeviceNotFound;
        }
    }
    RepairMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn map_projection(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepairProjection> {
    Ok(RepairProjection {
        id: row.get(0)?,
        damage_report_id: row.get(1)?,
        device_serial_no: row.get(2)?,
        status: row.get(3)?,
        repair_description: row.get(4)?,
        repair_cost: row.get(5)?,
        vendor: row.get(6)?,
        created_by: row.get(7)?,
        completed_by: row.get(8)?,
        returned_by: row.get(9)?,
        started_at: row.get(10)?,
        completed_at: row.get(11)?,
        returned_at: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn map_list_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepairListItem> {
    Ok(RepairListItem {
        id: row.get(0)?,
        damage_report_id: row.get(1)?,
        device_serial_no: row.get(2)?,
        status: row.get(3)?,
        repair_cost: row.get(4)?,
        vendor: row.get(5)?,
        started_at: row.get(6)?,
        completed_at: row.get(7)?,
        returned_at: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn map_history_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepairHistoryItem> {
    Ok(RepairHistoryItem {
        id: row.get(0)?,
        status: row.get(1)?,
        repair_cost: row.get(2)?,
        vendor: row.get(3)?,
        created_at: row.get(4)?,
        completed_at: row.get(5)?,
    })
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

    use crate::repositories::{RepairMutationError, RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("repair-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("repair-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_repair_authority_preserves_scope_lifecycle_device_return_and_stats() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE devices (
                    serialNo TEXT PRIMARY KEY,
                    rentalStatus TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE damage_reports (
                    id TEXT PRIMARY KEY,
                    device_serial_no TEXT NOT NULL,
                    status TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE repair_orders (
                    id TEXT PRIMARY KEY,
                    damage_report_id TEXT NOT NULL,
                    device_serial_no TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    repair_description TEXT NOT NULL DEFAULT '',
                    repair_cost REAL NOT NULL DEFAULT 0,
                    vendor TEXT NOT NULL DEFAULT '',
                    created_by TEXT NOT NULL DEFAULT '',
                    completed_by TEXT,
                    returned_by TEXT,
                    started_at TEXT,
                    completed_at TEXT,
                    returned_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                INSERT INTO devices VALUES
                    ('device-a','repairing','tenant-a'),
                    ('device-b','repairing','tenant-b');
                INSERT INTO damage_reports VALUES
                    ('damage-a','device-a','adjudicated','tenant-a'),
                    ('damage-a-pending','device-a','reported','tenant-a'),
                    ('damage-b','device-b','adjudicated','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool.clone());
        let scoped_a = provider.bind(&context("tenant-a", "repair-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "repair-b")).unwrap();

        let created = scoped_a
            .repairs()
            .create(
                "damage-a",
                "replace screen",
                "vendor-a",
                "repair-test-actor",
                "2026-09-29T20:50:00+08:00",
            )
            .unwrap();
        assert_eq!(created.device_serial_no, "device-a");
        assert_eq!(scoped_a.repairs().list(None, None, 1, 20).unwrap().total, 1);
        assert_eq!(scoped_b.repairs().list(None, None, 1, 20).unwrap().total, 0);

        let cross_tenant = scoped_b
            .repairs()
            .start(&created.repair_id, "2026-09-29T20:51:00+08:00");
        assert!(matches!(cross_tenant, Err(RepairMutationError::NotFound)));

        let duplicate = scoped_a.repairs().create(
            "damage-a",
            "",
            "",
            "repair-test-actor",
            "2026-09-29T20:52:00+08:00",
        );
        assert!(matches!(
            duplicate,
            Err(RepairMutationError::AlreadyExists(_))
        ));

        let not_adjudicated = scoped_a.repairs().create(
            "damage-a-pending",
            "",
            "",
            "repair-test-actor",
            "2026-09-29T20:53:00+08:00",
        );
        assert!(matches!(
            not_adjudicated,
            Err(RepairMutationError::DamageNotAdjudicated(status)) if status == "reported"
        ));

        let premature = scoped_a.repairs().complete(
            &created.repair_id,
            500.0,
            "repair-test-actor",
            "2026-09-29T20:54:00+08:00",
        );
        assert!(matches!(
            premature,
            Err(RepairMutationError::NotInProgress(status)) if status == "pending"
        ));

        let started = scoped_a
            .repairs()
            .start(&created.repair_id, "2026-09-29T20:55:00+08:00")
            .unwrap();
        assert_eq!(started.status, "in_progress");

        let completed = scoped_a
            .repairs()
            .complete(
                &created.repair_id,
                500.0,
                "repair-test-actor",
                "2026-09-29T20:56:00+08:00",
            )
            .unwrap();
        assert_eq!(completed.status, "completed");
        assert_eq!(completed.repair_cost, Some(500.0));

        let returned = scoped_a
            .repairs()
            .return_to_stock(
                &created.repair_id,
                "repair-test-actor",
                "2026-09-29T20:57:00+08:00",
            )
            .unwrap();
        assert_eq!(returned.status, "returned");
        assert_eq!(returned.device_serial_no.as_deref(), Some("device-a"));

        let persisted = scoped_a.repairs().get(&created.repair_id).unwrap().unwrap();
        assert_eq!(persisted.status, "returned");
        assert_eq!(persisted.repair_cost, 500.0);

        let stats = scoped_a.repairs().device_stats("device-a").unwrap();
        assert_eq!(stats.total_repairs, 1);
        assert_eq!(stats.total_repair_cost, 500.0);
        assert_eq!(stats.recent_repairs.len(), 1);

        let connection = pool.get().unwrap();
        let status: String = connection
            .query_row(
                "SELECT rentalStatus FROM devices
                 WHERE tenant_id='tenant-a' AND serialNo='device-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "available");
    }
}

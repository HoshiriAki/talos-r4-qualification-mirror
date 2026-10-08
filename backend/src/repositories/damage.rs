use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const RESOURCE_NOT_FOUND: &str = "damage-resource-not-found";
const NOT_FOUND_PREFIX: &str = "damage-not-found:";
const NOT_REPORTED_PREFIX: &str = "damage-not-reported:";
const NOT_ASSESSED_PREFIX: &str = "damage-not-assessed:";

#[derive(Debug, Clone, PartialEq)]
pub struct DamageReportOutcome {
    pub damage_id: String,
    pub order_id: String,
    pub device_serial_no: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DamageTransitionOutcome {
    pub damage_id: String,
    pub order_id: String,
    pub device_serial_no: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageProjection {
    pub id: String,
    pub order_id: String,
    pub device_serial_no: String,
    pub appearance_ok: bool,
    pub accessories_ok: bool,
    pub function_ok: bool,
    pub damage_description: String,
    pub estimated_damage_amount: f64,
    pub liability: String,
    pub status: String,
    pub reported_by: String,
    pub assessed_by: Option<String>,
    pub adjudicated_by: Option<String>,
    pub reported_at: String,
    pub assessed_at: Option<String>,
    pub adjudicated_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DamageListItem {
    pub id: String,
    pub order_id: String,
    pub device_serial_no: String,
    pub estimated_damage_amount: f64,
    pub liability: String,
    pub status: String,
    pub reported_at: String,
    pub assessed_at: Option<String>,
    pub adjudicated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DamageListProjection {
    pub reports: Vec<DamageListItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum DamageMutationError {
    #[error("order or device is outside data scope")]
    ResourceNotFound,
    #[error("damage report not found")]
    NotFound,
    #[error("damage report status is not reported: {0}")]
    NotReported(String),
    #[error("damage report status is not assessed: {0}")]
    NotAssessed(String),
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteDamageRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteDamageRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn report(
        &self,
        order_id: &str,
        device_serial_no: &str,
        appearance_ok: bool,
        accessories_ok: bool,
        function_ok: bool,
        damage_description: &str,
        operator: &str,
        now: &str,
    ) -> Result<DamageReportOutcome, DamageMutationError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        let device_serial_no = device_serial_no.to_owned();
        let damage_description = damage_description.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let exists = transaction
                    .query_row(
                        "SELECT 1 FROM orders o JOIN devices d ON d.serialNo=?1
                         WHERE o.tenant_id=?2 AND d.tenant_id=?2 AND o.id=?3
                         LIMIT 1",
                        params![device_serial_no, tenant_id, order_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if exists.is_none() {
                    return Err(contract(RESOURCE_NOT_FOUND.into()));
                }

                let damage_id = Uuid::new_v4().to_string();
                transaction
                    .execute(
                        "INSERT INTO damage_reports
                         (id,order_id,device_serial_no,appearance_ok,accessories_ok,function_ok,
                          damage_description,status,reported_by,reported_at,created_at,updated_at,tenant_id)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,'reported',?8,?9,?9,?9,?10)",
                        params![
                            damage_id,
                            order_id,
                            device_serial_no,
                            appearance_ok as i32,
                            accessories_ok as i32,
                            function_ok as i32,
                            damage_description,
                            operator,
                            now,
                            tenant_id,
                        ],
                    )
                    .map_err(sqlite_error)?;

                Ok(DamageReportOutcome {
                    damage_id,
                    order_id,
                    device_serial_no,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn assess(
        &self,
        damage_id: &str,
        estimated_damage_amount: f64,
        liability: &str,
        notes: &str,
        operator: &str,
        now: &str,
    ) -> Result<DamageTransitionOutcome, DamageMutationError> {
        let tenant_id = self.tenant_id();
        let damage_id = damage_id.to_owned();
        let liability = liability.to_owned();
        let notes = notes.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let row = transaction
                    .query_row(
                        "SELECT status,order_id,device_serial_no FROM damage_reports
                         WHERE tenant_id=?1 AND id=?2
                         LIMIT 1",
                        params![tenant_id, damage_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(format!("{NOT_FOUND_PREFIX}{damage_id}")))?;
                let (status, order_id, device_serial_no) = row;
                if status != "reported" {
                    return Err(contract(format!("{NOT_REPORTED_PREFIX}{status}")));
                }

                let note = if notes.is_empty() {
                    String::new()
                } else {
                    format!(" | 定损备注: {notes}")
                };
                transaction
                    .execute(
                        "UPDATE damage_reports
                         SET estimated_damage_amount=?1,liability=?2,
                             damage_description=damage_description || ?3,
                             status='assessed',assessed_by=?4,assessed_at=?5,updated_at=?5
                         WHERE tenant_id=?6 AND id=?7",
                        params![
                            estimated_damage_amount,
                            liability,
                            note,
                            operator,
                            now,
                            tenant_id,
                            damage_id,
                        ],
                    )
                    .map_err(sqlite_error)?;

                Ok(DamageTransitionOutcome {
                    damage_id,
                    order_id,
                    device_serial_no,
                    status: "assessed".into(),
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn adjudicate(
        &self,
        damage_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<DamageTransitionOutcome, DamageMutationError> {
        let tenant_id = self.tenant_id();
        let damage_id = damage_id.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let row = transaction
                    .query_row(
                        "SELECT status,order_id,device_serial_no FROM damage_reports
                         WHERE tenant_id=?1 AND id=?2
                         LIMIT 1",
                        params![tenant_id, damage_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(format!("{NOT_FOUND_PREFIX}{damage_id}")))?;
                let (status, order_id, device_serial_no) = row;
                if status != "assessed" {
                    return Err(contract(format!("{NOT_ASSESSED_PREFIX}{status}")));
                }

                transaction
                    .execute(
                        "UPDATE damage_reports
                         SET status='adjudicated',adjudicated_by=?1,adjudicated_at=?2,updated_at=?2
                         WHERE tenant_id=?3 AND id=?4",
                        params![operator, now, tenant_id, damage_id],
                    )
                    .map_err(sqlite_error)?;

                Ok(DamageTransitionOutcome {
                    damage_id,
                    order_id,
                    device_serial_no,
                    status: "adjudicated".into(),
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn get_by_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<DamageProjection>, RepositoryError> {
        self.get_by("order_id", order_id)
    }

    pub(in crate::repositories) fn get_by_device(
        &self,
        device_serial_no: &str,
    ) -> Result<Vec<DamageProjection>, RepositoryError> {
        self.get_by("device_serial_no", device_serial_no)
    }

    fn get_by(&self, column: &str, value: &str) -> Result<Vec<DamageProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let value = value.to_owned();
        let sql = format!(
            "SELECT id,order_id,device_serial_no,appearance_ok,accessories_ok,function_ok,
                    damage_description,estimated_damage_amount,liability,status,
                    reported_by,assessed_by,adjudicated_by,reported_at,assessed_at,adjudicated_at,
                    created_at,updated_at
             FROM damage_reports
             WHERE tenant_id=?1 AND {column}=?2
             ORDER BY created_at DESC"
        );
        self.session.read(move |connection| {
            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(params![tenant_id, value], map_projection)?
                .collect()
        })
    }

    pub(in crate::repositories) fn list(
        &self,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<DamageListProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let total: i64 = if let Some(status) = &status {
                connection.query_row(
                    "SELECT COUNT(*) FROM damage_reports WHERE tenant_id=?1 AND status=?2",
                    params![tenant_id, status],
                    |row| row.get(0),
                )?
            } else {
                connection.query_row(
                    "SELECT COUNT(*) FROM damage_reports WHERE tenant_id=?1",
                    params![tenant_id],
                    |row| row.get(0),
                )?
            };

            let (sql, filtered) = if status.is_some() {
                (
                    format!(
                        "SELECT id,order_id,device_serial_no,estimated_damage_amount,liability,status,
                                reported_at,assessed_at,adjudicated_at
                         FROM damage_reports
                         WHERE tenant_id=?1 AND status=?2
                         ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
                    ),
                    true,
                )
            } else {
                (
                    format!(
                        "SELECT id,order_id,device_serial_no,estimated_damage_amount,liability,status,
                                reported_at,assessed_at,adjudicated_at
                         FROM damage_reports
                         WHERE tenant_id=?1
                         ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
                    ),
                    false,
                )
            };
            let mut statement = connection.prepare(&sql)?;
            let reports = if filtered {
                statement
                    .query_map(
                        params![tenant_id, status.as_deref().unwrap_or_default()],
                        map_list_item,
                    )?
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                statement
                    .query_map(params![tenant_id], map_list_item)?
                    .collect::<Result<Vec<_>, _>>()?
            };

            Ok(DamageListProjection {
                reports,
                page,
                page_size,
                total,
            })
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub(in crate::repositories) fn map_mutation_error(error: RepositoryError) -> DamageMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message == RESOURCE_NOT_FOUND {
            return DamageMutationError::ResourceNotFound;
        }
        if message.starts_with(NOT_FOUND_PREFIX) {
            return DamageMutationError::NotFound;
        }
        if let Some(status) = message.strip_prefix(NOT_REPORTED_PREFIX) {
            return DamageMutationError::NotReported(status.to_owned());
        }
        if let Some(status) = message.strip_prefix(NOT_ASSESSED_PREFIX) {
            return DamageMutationError::NotAssessed(status.to_owned());
        }
    }
    DamageMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn map_projection(row: &rusqlite::Row<'_>) -> rusqlite::Result<DamageProjection> {
    Ok(DamageProjection {
        id: row.get(0)?,
        order_id: row.get(1)?,
        device_serial_no: row.get(2)?,
        appearance_ok: row.get(3)?,
        accessories_ok: row.get(4)?,
        function_ok: row.get(5)?,
        damage_description: row.get(6)?,
        estimated_damage_amount: row.get(7)?,
        liability: row.get(8)?,
        status: row.get(9)?,
        reported_by: row.get(10)?,
        assessed_by: row.get(11)?,
        adjudicated_by: row.get(12)?,
        reported_at: row.get(13)?,
        assessed_at: row.get(14)?,
        adjudicated_at: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}

fn map_list_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<DamageListItem> {
    Ok(DamageListItem {
        id: row.get(0)?,
        order_id: row.get(1)?,
        device_serial_no: row.get(2)?,
        estimated_damage_amount: row.get(3)?,
        liability: row.get(4)?,
        status: row.get(5)?,
        reported_at: row.get(6)?,
        assessed_at: row.get(7)?,
        adjudicated_at: row.get(8)?,
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

    use crate::repositories::{DamageMutationError, RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("damage-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("damage-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_damage_authority_preserves_scope_and_state_machine() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE orders (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE devices (
                    serialNo TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE damage_reports (
                    id TEXT PRIMARY KEY,
                    order_id TEXT NOT NULL,
                    device_serial_no TEXT NOT NULL,
                    appearance_ok INTEGER NOT NULL,
                    accessories_ok INTEGER NOT NULL,
                    function_ok INTEGER NOT NULL,
                    damage_description TEXT NOT NULL,
                    estimated_damage_amount REAL NOT NULL DEFAULT 0,
                    liability TEXT NOT NULL DEFAULT 'unknown',
                    status TEXT NOT NULL DEFAULT 'reported',
                    reported_by TEXT NOT NULL,
                    assessed_by TEXT,
                    adjudicated_by TEXT,
                    reported_at TEXT NOT NULL,
                    assessed_at TEXT,
                    adjudicated_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                INSERT INTO orders VALUES
                    ('order-a','tenant-a'),
                    ('order-b','tenant-b');
                INSERT INTO devices VALUES
                    ('device-a','tenant-a'),
                    ('device-b','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let scoped_a = provider.bind(&context("tenant-a", "damage-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "damage-b")).unwrap();

        let requested = scoped_a
            .damages()
            .report(
                "order-a",
                "device-a",
                false,
                true,
                true,
                "screen damage",
                "damage-test-actor",
                "2026-09-29T19:20:00+08:00",
            )
            .unwrap();

        assert_eq!(scoped_a.damages().list(None, 1, 20).unwrap().total, 1);
        assert_eq!(scoped_b.damages().list(None, 1, 20).unwrap().total, 0);
        assert_eq!(scoped_a.damages().get_by_order("order-a").unwrap().len(), 1);

        let cross_tenant = scoped_b.damages().assess(
            &requested.damage_id,
            1000.0,
            "customer",
            "",
            "damage-test-actor",
            "2026-09-29T19:21:00+08:00",
        );
        assert!(matches!(cross_tenant, Err(DamageMutationError::NotFound)));

        let premature = scoped_a.damages().adjudicate(
            &requested.damage_id,
            "damage-test-actor",
            "2026-09-29T19:22:00+08:00",
        );
        assert!(matches!(
            premature,
            Err(DamageMutationError::NotAssessed(status)) if status == "reported"
        ));

        let assessed = scoped_a
            .damages()
            .assess(
                &requested.damage_id,
                1000.0,
                "customer",
                "confirmed",
                "damage-test-actor",
                "2026-09-29T19:23:00+08:00",
            )
            .unwrap();
        assert_eq!(assessed.status, "assessed");

        let adjudicated = scoped_a
            .damages()
            .adjudicate(
                &requested.damage_id,
                "damage-test-actor",
                "2026-09-29T19:24:00+08:00",
            )
            .unwrap();
        assert_eq!(adjudicated.status, "adjudicated");
        assert_eq!(
            scoped_a
                .damages()
                .list(Some("adjudicated"), 1, 20)
                .unwrap()
                .total,
            1
        );

        let missing_resource = scoped_a.damages().report(
            "order-a",
            "device-b",
            false,
            false,
            false,
            "cross tenant",
            "damage-test-actor",
            "2026-09-29T19:25:00+08:00",
        );
        assert!(matches!(
            missing_resource,
            Err(DamageMutationError::ResourceNotFound)
        ));
    }
}

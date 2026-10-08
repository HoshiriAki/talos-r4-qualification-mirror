use std::collections::BTreeMap;

use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const CHECKLIST_NOT_FOUND: &str = "optical-sop-checklist-not-found";
const ORDER_NOT_FOUND: &str = "optical-sop-order-not-found";
const INVALID_ORDER_STATUS_PREFIX: &str = "optical-sop-order-status:";
const DEVICE_NOT_IN_ORDER: &str = "optical-sop-device-not-in-order";
const DUPLICATE: &str = "optical-sop-duplicate";
const COMPLETED: &str = "optical-sop-completed";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpticalInspectionProjection {
    pub id: i64,
    pub order_id: String,
    pub device_serial_no: String,
    pub inspector_id: String,
    pub body_ok: bool,
    pub lens_ok: bool,
    pub screen_ok: bool,
    pub accessory_ok: bool,
    pub function_ok: bool,
    pub overall_grade: String,
    pub damage_report_id: Option<String>,
    pub body_note: Option<String>,
    pub lens_note: Option<String>,
    pub screen_note: Option<String>,
    pub accessory_note: Option<String>,
    pub function_note: Option<String>,
    pub photo_urls: Option<String>,
    pub notes: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpticalCreateOutcome {
    pub id: i64,
    pub order_id: String,
    pub device_serial_no: String,
    pub overall_grade: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpticalUpdateOutcome {
    pub id: i64,
    pub step: String,
    pub ok: bool,
    pub overall_grade: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpticalCompleteOutcome {
    pub id: i64,
    pub order_id: String,
    pub device_serial_no: String,
    pub overall_grade: String,
    pub damage_report_id: Option<String>,
    pub completed_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpticalListProjection {
    pub items: Vec<OpticalInspectionProjection>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpticalStatsProjection {
    pub total: i64,
    pub pass_count: i64,
    pub damage_count: i64,
    pub by_grade: BTreeMap<String, i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum OpticalSopMutationError {
    #[error("inspection checklist not found")]
    ChecklistNotFound,
    #[error("order not found")]
    OrderNotFound,
    #[error("order status is not inspectable: {0}")]
    InvalidOrderStatus(String),
    #[error("device is not assigned to order in current tenant")]
    DeviceNotInOrder,
    #[error("inspection checklist already exists")]
    Duplicate,
    #[error("inspection checklist is already completed")]
    Completed,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteOpticalSopRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteOpticalSopRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn create(
        &self,
        order_id: &str,
        device_serial_no: &str,
        inspector_id: &str,
        now: &str,
    ) -> Result<OpticalCreateOutcome, OpticalSopMutationError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        let device_serial_no = device_serial_no.to_owned();
        let inspector_id = inspector_id.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let order_status = transaction
                    .query_row(
                        "SELECT status FROM orders
                         WHERE tenant_id=?1 AND id=?2
                         LIMIT 1",
                        params![tenant_id, order_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(ORDER_NOT_FOUND.into()))?;

                if order_status != "in_use" && order_status != "shipped" {
                    return Err(contract(format!(
                        "{INVALID_ORDER_STATUS_PREFIX}{order_status}"
                    )));
                }

                let assigned = transaction
                    .query_row(
                        "SELECT 1
                         FROM order_devices od
                         JOIN devices d
                           ON d.serialNo=od.serialNo
                          AND d.tenant_id=od.tenant_id
                         WHERE od.tenant_id=?1
                           AND od.orderId=?2
                           AND od.serialNo=?3
                         LIMIT 1",
                        params![tenant_id, order_id, device_serial_no],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if assigned.is_none() {
                    return Err(contract(DEVICE_NOT_IN_ORDER.into()));
                }

                let existing = transaction
                    .query_row(
                        "SELECT id FROM inspection_checklists
                         WHERE tenant_id=?1 AND order_id=?2 AND device_serial_no=?3
                         LIMIT 1",
                        params![tenant_id, order_id, device_serial_no],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if existing.is_some() {
                    return Err(contract(DUPLICATE.into()));
                }

                transaction
                    .execute(
                        "INSERT INTO inspection_checklists
                         (order_id,device_serial_no,inspector_id,
                          body_ok,lens_ok,screen_ok,accessory_ok,function_ok,
                          overall_grade,created_at,updated_at,tenant_id)
                         VALUES (?1,?2,?3,1,1,1,1,1,'pass',?4,?4,?5)",
                        params![order_id, device_serial_no, inspector_id, now, tenant_id],
                    )
                    .map_err(sqlite_error)?;
                let id = transaction.last_insert_rowid();

                Ok(OpticalCreateOutcome {
                    id,
                    order_id,
                    device_serial_no,
                    overall_grade: "pass".into(),
                    created_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn update_step(
        &self,
        id: i64,
        step: &str,
        ok: bool,
        note: Option<&str>,
        now: &str,
    ) -> Result<OpticalUpdateOutcome, OpticalSopMutationError> {
        let tenant_id = self.tenant_id();
        let step = step.to_owned();
        let note = note.map(str::to_owned);
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let row = transaction
                    .query_row(
                        "SELECT body_ok,lens_ok,screen_ok,accessory_ok,function_ok,completed_at
                         FROM inspection_checklists
                         WHERE tenant_id=?1 AND id=?2
                         LIMIT 1",
                        params![tenant_id, id],
                        |row| {
                            Ok((
                                row.get::<_, i32>(0)? != 0,
                                row.get::<_, i32>(1)? != 0,
                                row.get::<_, i32>(2)? != 0,
                                row.get::<_, i32>(3)? != 0,
                                row.get::<_, i32>(4)? != 0,
                                row.get::<_, Option<String>>(5)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(CHECKLIST_NOT_FOUND.into()))?;

                let (
                    mut body_ok,
                    mut lens_ok,
                    mut screen_ok,
                    mut accessory_ok,
                    mut function_ok,
                    completed_at,
                ) = row;
                if completed_at.is_some() {
                    return Err(contract(COMPLETED.into()));
                }

                let (column, note_column) = match step.as_str() {
                    "body" => {
                        body_ok = ok;
                        ("body_ok", "body_note")
                    }
                    "lens" => {
                        lens_ok = ok;
                        ("lens_ok", "lens_note")
                    }
                    "screen" => {
                        screen_ok = ok;
                        ("screen_ok", "screen_note")
                    }
                    "accessory" => {
                        accessory_ok = ok;
                        ("accessory_ok", "accessory_note")
                    }
                    "function" => {
                        function_ok = ok;
                        ("function_ok", "function_note")
                    }
                    _ => return Err(contract("optical-sop-invalid-step".into())),
                };

                let grade =
                    compute_overall_grade(body_ok, lens_ok, screen_ok, accessory_ok, function_ok);
                let sql = format!(
                    "UPDATE inspection_checklists
                     SET {column}=?1,{note_column}=?2,overall_grade=?3,updated_at=?4
                     WHERE tenant_id=?5 AND id=?6 AND completed_at IS NULL"
                );
                let changed = transaction
                    .execute(
                        &sql,
                        params![
                            if ok { 1_i32 } else { 0_i32 },
                            note,
                            grade,
                            now,
                            tenant_id,
                            id
                        ],
                    )
                    .map_err(sqlite_error)?;
                if changed != 1 {
                    return Err(contract(COMPLETED.into()));
                }

                Ok(OpticalUpdateOutcome {
                    id,
                    step,
                    ok,
                    overall_grade: grade.into(),
                    updated_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn complete(
        &self,
        id: i64,
        operator: &str,
        now: &str,
    ) -> Result<OpticalCompleteOutcome, OpticalSopMutationError> {
        let tenant_id = self.tenant_id();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let checklist = transaction
                    .query_row(
                        "SELECT order_id,device_serial_no,
                                body_ok,lens_ok,screen_ok,accessory_ok,function_ok,
                                body_note,lens_note,screen_note,accessory_note,function_note,
                                damage_report_id,completed_at
                         FROM inspection_checklists
                         WHERE tenant_id=?1 AND id=?2
                         LIMIT 1",
                        params![tenant_id, id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, i32>(2)? != 0,
                                row.get::<_, i32>(3)? != 0,
                                row.get::<_, i32>(4)? != 0,
                                row.get::<_, i32>(5)? != 0,
                                row.get::<_, i32>(6)? != 0,
                                row.get::<_, Option<String>>(7)?,
                                row.get::<_, Option<String>>(8)?,
                                row.get::<_, Option<String>>(9)?,
                                row.get::<_, Option<String>>(10)?,
                                row.get::<_, Option<String>>(11)?,
                                row.get::<_, Option<String>>(12)?,
                                row.get::<_, Option<String>>(13)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(CHECKLIST_NOT_FOUND.into()))?;

                let (
                    order_id,
                    device_serial_no,
                    body_ok,
                    lens_ok,
                    screen_ok,
                    accessory_ok,
                    function_ok,
                    body_note,
                    lens_note,
                    screen_note,
                    accessory_note,
                    function_note,
                    persisted_damage_id,
                    completed_at,
                ) = checklist;

                let grade =
                    compute_overall_grade(body_ok, lens_ok, screen_ok, accessory_ok, function_ok);

                if let Some(completed_at) = completed_at {
                    return Ok(OpticalCompleteOutcome {
                        id,
                        order_id,
                        device_serial_no,
                        overall_grade: grade.into(),
                        damage_report_id: persisted_damage_id,
                        completed_at,
                    });
                }

                let damage_report_id = if grade == "pass" {
                    None
                } else {
                    let damage_id = Uuid::new_v4().to_string();
                    let description = damage_description(
                        body_ok,
                        lens_ok,
                        screen_ok,
                        accessory_ok,
                        function_ok,
                        body_note.as_deref(),
                        lens_note.as_deref(),
                        screen_note.as_deref(),
                        accessory_note.as_deref(),
                        function_note.as_deref(),
                    );
                    transaction
                        .execute(
                            "INSERT INTO damage_reports
                             (id,order_id,device_serial_no,
                              appearance_ok,accessories_ok,function_ok,
                              damage_description,status,reported_by,reported_at,
                              created_at,updated_at,tenant_id)
                             VALUES (?1,?2,?3,?4,?5,?6,?7,'reported',?8,?9,?9,?9,?10)",
                            params![
                                damage_id,
                                order_id,
                                device_serial_no,
                                if body_ok && lens_ok && screen_ok {
                                    1_i32
                                } else {
                                    0_i32
                                },
                                if accessory_ok { 1_i32 } else { 0_i32 },
                                if function_ok { 1_i32 } else { 0_i32 },
                                description,
                                operator,
                                now,
                                tenant_id,
                            ],
                        )
                        .map_err(sqlite_error)?;
                    Some(damage_id)
                };

                let changed = transaction
                    .execute(
                        "UPDATE inspection_checklists
                         SET overall_grade=?1,damage_report_id=?2,completed_at=?3,updated_at=?3
                         WHERE tenant_id=?4 AND id=?5 AND completed_at IS NULL",
                        params![grade, damage_report_id, now, tenant_id, id],
                    )
                    .map_err(sqlite_error)?;
                if changed != 1 {
                    return Err(contract(COMPLETED.into()));
                }

                Ok(OpticalCompleteOutcome {
                    id,
                    order_id,
                    device_serial_no,
                    overall_grade: grade.into(),
                    damage_report_id,
                    completed_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn get(
        &self,
        id: i64,
    ) -> Result<Option<OpticalInspectionProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,order_id,device_serial_no,inspector_id,
                            body_ok,lens_ok,screen_ok,accessory_ok,function_ok,
                            overall_grade,damage_report_id,
                            body_note,lens_note,screen_note,accessory_note,function_note,
                            photo_urls,notes,completed_at,created_at,updated_at
                     FROM inspection_checklists
                     WHERE tenant_id=?1 AND id=?2
                     LIMIT 1",
                    params![tenant_id, id],
                    map_projection,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn list(
        &self,
        order_id: Option<&str>,
        device_serial_no: Option<&str>,
        overall_grade: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<OpticalListProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.filter(|v| !v.is_empty()).map(str::to_owned);
        let device_serial_no = device_serial_no
            .filter(|v| !v.is_empty())
            .map(str::to_owned);
        let overall_grade = overall_grade.filter(|v| !v.is_empty()).map(str::to_owned);
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut predicates = vec!["tenant_id = ?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id)];
            if let Some(value) = order_id {
                predicates.push("order_id = ?".into());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = device_serial_no {
                predicates.push("device_serial_no = ?".into());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = overall_grade {
                predicates.push("overall_grade = ?".into());
                values.push(SqlValue::Text(value));
            }
            let where_sql = predicates.join(" AND ");

            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM inspection_checklists WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;

            let sql = format!(
                "SELECT id,order_id,device_serial_no,inspector_id,
                        body_ok,lens_ok,screen_ok,accessory_ok,function_ok,
                        overall_grade,damage_report_id,
                        body_note,lens_note,screen_note,accessory_note,function_note,
                        photo_urls,notes,completed_at,created_at,updated_at
                 FROM inspection_checklists
                 WHERE {where_sql}
                 ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
            );
            let mut statement = connection.prepare(&sql)?;
            let items = statement
                .query_map(params_from_iter(values.iter()), map_projection)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(OpticalListProjection {
                items,
                total,
                page,
                page_size,
            })
        })
    }

    pub(in crate::repositories) fn stats(&self) -> Result<OpticalStatsProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            let total: i64 = connection.query_row(
                "SELECT COUNT(*) FROM inspection_checklists WHERE tenant_id=?1",
                params![tenant_id],
                |row| row.get(0),
            )?;
            let pass_count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM inspection_checklists
                 WHERE tenant_id=?1 AND overall_grade='pass'",
                params![tenant_id],
                |row| row.get(0),
            )?;
            let damage_count = total - pass_count;

            let mut statement = connection.prepare(
                "SELECT overall_grade,COUNT(*)
                 FROM inspection_checklists
                 WHERE tenant_id=?1
                 GROUP BY overall_grade
                 ORDER BY overall_grade",
            )?;
            let by_grade = statement
                .query_map(params![tenant_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<Result<BTreeMap<_, _>, _>>()?;

            Ok(OpticalStatsProjection {
                total,
                pass_count,
                damage_count,
                by_grade,
            })
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub(in crate::repositories) fn compute_overall_grade(
    body_ok: bool,
    lens_ok: bool,
    screen_ok: bool,
    accessory_ok: bool,
    function_ok: bool,
) -> &'static str {
    let failed = [!body_ok, !lens_ok, !screen_ok, !accessory_ok, !function_ok]
        .iter()
        .filter(|&&failed| failed)
        .count();
    match failed {
        0 => "pass",
        1..=2 => "minor_damage",
        3..=4 => "major_damage",
        _ => "total_loss",
    }
}

pub(in crate::repositories) fn damage_description(
    body_ok: bool,
    lens_ok: bool,
    screen_ok: bool,
    accessory_ok: bool,
    function_ok: bool,
    body_note: Option<&str>,
    lens_note: Option<&str>,
    screen_note: Option<&str>,
    accessory_note: Option<&str>,
    function_note: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if !body_ok {
        parts.push(format!("外观异常: {}", body_note.unwrap_or("无备注")));
    }
    if !lens_ok {
        parts.push(format!("镜头异常: {}", lens_note.unwrap_or("无备注")));
    }
    if !screen_ok {
        parts.push(format!("屏幕异常: {}", screen_note.unwrap_or("无备注")));
    }
    if !accessory_ok {
        parts.push(format!("配件异常: {}", accessory_note.unwrap_or("无备注")));
    }
    if !function_ok {
        parts.push(format!("功能异常: {}", function_note.unwrap_or("无备注")));
    }
    parts.join("; ")
}

pub(in crate::repositories) fn map_mutation_error(
    error: RepositoryError,
) -> OpticalSopMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message == CHECKLIST_NOT_FOUND {
            return OpticalSopMutationError::ChecklistNotFound;
        }
        if message == ORDER_NOT_FOUND {
            return OpticalSopMutationError::OrderNotFound;
        }
        if let Some(status) = message.strip_prefix(INVALID_ORDER_STATUS_PREFIX) {
            return OpticalSopMutationError::InvalidOrderStatus(status.to_owned());
        }
        if message == DEVICE_NOT_IN_ORDER {
            return OpticalSopMutationError::DeviceNotInOrder;
        }
        if message == DUPLICATE {
            return OpticalSopMutationError::Duplicate;
        }
        if message == COMPLETED {
            return OpticalSopMutationError::Completed;
        }
    }
    OpticalSopMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn map_projection(row: &rusqlite::Row<'_>) -> rusqlite::Result<OpticalInspectionProjection> {
    Ok(OpticalInspectionProjection {
        id: row.get(0)?,
        order_id: row.get(1)?,
        device_serial_no: row.get(2)?,
        inspector_id: row.get(3)?,
        body_ok: row.get::<_, i32>(4)? != 0,
        lens_ok: row.get::<_, i32>(5)? != 0,
        screen_ok: row.get::<_, i32>(6)? != 0,
        accessory_ok: row.get::<_, i32>(7)? != 0,
        function_ok: row.get::<_, i32>(8)? != 0,
        overall_grade: row.get(9)?,
        damage_report_id: row.get(10)?,
        body_note: row.get(11)?,
        lens_note: row.get(12)?,
        screen_note: row.get(13)?,
        accessory_note: row.get(14)?,
        function_note: row.get(15)?,
        photo_urls: row.get(16)?,
        notes: row.get(17)?,
        completed_at: row.get(18)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
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

    use crate::repositories::{
        OpticalSopMutationError, RepositoryProvider, SqliteRepositoryProvider,
    };

    fn context(tenant: &str, actor: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated(actor, "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("optical-sop-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_optical_sop_authority_preserves_scope_completion_and_damage_link() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE orders (
                    id TEXT PRIMARY KEY,
                    status TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE devices (
                    serialNo TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    PRIMARY KEY(serialNo, tenant_id)
                );
                CREATE TABLE order_devices (
                    id TEXT PRIMARY KEY,
                    orderId TEXT NOT NULL,
                    serialNo TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE inspection_checklists (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    order_id TEXT NOT NULL,
                    device_serial_no TEXT NOT NULL,
                    inspector_id TEXT NOT NULL,
                    body_ok INTEGER NOT NULL DEFAULT 1,
                    body_note TEXT,
                    lens_ok INTEGER NOT NULL DEFAULT 1,
                    lens_note TEXT,
                    screen_ok INTEGER NOT NULL DEFAULT 1,
                    screen_note TEXT,
                    accessory_ok INTEGER NOT NULL DEFAULT 1,
                    accessory_note TEXT,
                    function_ok INTEGER NOT NULL DEFAULT 1,
                    function_note TEXT,
                    overall_grade TEXT NOT NULL DEFAULT 'pass',
                    damage_report_id TEXT,
                    photo_urls TEXT,
                    notes TEXT,
                    completed_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    UNIQUE(tenant_id, order_id, device_serial_no)
                );
                CREATE TABLE damage_reports (
                    id TEXT PRIMARY KEY,
                    order_id TEXT NOT NULL,
                    device_serial_no TEXT NOT NULL,
                    appearance_ok INTEGER NOT NULL DEFAULT 1,
                    accessories_ok INTEGER NOT NULL DEFAULT 1,
                    function_ok INTEGER NOT NULL DEFAULT 1,
                    damage_description TEXT NOT NULL DEFAULT '',
                    estimated_damage_amount REAL NOT NULL DEFAULT 0,
                    liability TEXT NOT NULL DEFAULT 'unknown',
                    status TEXT NOT NULL DEFAULT 'reported',
                    reported_by TEXT NOT NULL DEFAULT '',
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
                    ('order-a','in_use','tenant-a'),
                    ('order-b','shipped','tenant-b'),
                    ('order-a-draft','draft','tenant-a');
                INSERT INTO devices VALUES
                    ('OPT-A-1','tenant-a'),
                    ('OPT-B-1','tenant-b');
                INSERT INTO order_devices VALUES
                    ('od-a','order-a','OPT-A-1','tenant-a'),
                    ('od-b','order-b','OPT-B-1','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool.clone());
        let ctx_a = context("tenant-a", "optical-actor-a", "optical-a");
        let ctx_b = context("tenant-b", "optical-actor-b", "optical-b");
        let scoped_a = provider.bind(&ctx_a).unwrap();
        let scoped_b = provider.bind(&ctx_b).unwrap();

        let created = scoped_a
            .optical_sops()
            .create(
                "order-a",
                "OPT-A-1",
                "optical-actor-a",
                "2026-10-02T10:00:00+08:00",
            )
            .unwrap();
        assert_eq!(created.overall_grade, "pass");
        assert_eq!(
            scoped_a
                .optical_sops()
                .list(None, None, None, 1, 20)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            scoped_b
                .optical_sops()
                .list(None, None, None, 1, 20)
                .unwrap()
                .total,
            0
        );
        assert!(scoped_b.optical_sops().get(created.id).unwrap().is_none());

        let duplicate = scoped_a.optical_sops().create(
            "order-a",
            "OPT-A-1",
            "optical-actor-a",
            "2026-10-02T10:00:01+08:00",
        );
        assert!(matches!(duplicate, Err(OpticalSopMutationError::Duplicate)));

        let cross_tenant_device = scoped_a.optical_sops().create(
            "order-a",
            "OPT-B-1",
            "optical-actor-a",
            "2026-10-02T10:00:02+08:00",
        );
        assert!(matches!(
            cross_tenant_device,
            Err(OpticalSopMutationError::DeviceNotInOrder)
        ));

        let invalid_status = scoped_a.optical_sops().create(
            "order-a-draft",
            "OPT-A-1",
            "optical-actor-a",
            "2026-10-02T10:00:03+08:00",
        );
        assert!(matches!(
            invalid_status,
            Err(OpticalSopMutationError::InvalidOrderStatus(status)) if status == "draft"
        ));

        let updated = scoped_a
            .optical_sops()
            .update_step(
                created.id,
                "body",
                false,
                Some("scratch"),
                "2026-10-02T10:01:00+08:00",
            )
            .unwrap();
        assert_eq!(updated.overall_grade, "minor_damage");

        let completed = scoped_a
            .optical_sops()
            .complete(created.id, "optical-actor-a", "2026-10-02T10:02:00+08:00")
            .unwrap();
        assert_eq!(completed.overall_grade, "minor_damage");
        let damage_id = completed.damage_report_id.clone().expect("damage id");

        let second = scoped_a
            .optical_sops()
            .complete(created.id, "optical-actor-a", "2026-10-02T10:02:30+08:00")
            .unwrap();
        assert_eq!(second.damage_report_id.as_deref(), Some(damage_id.as_str()));
        assert_eq!(second.completed_at, completed.completed_at);

        let immutable = scoped_a.optical_sops().update_step(
            created.id,
            "lens",
            false,
            Some("late mutation"),
            "2026-10-02T10:03:00+08:00",
        );
        assert!(matches!(immutable, Err(OpticalSopMutationError::Completed)));

        let record = scoped_a.optical_sops().get(created.id).unwrap().unwrap();
        assert_eq!(record.damage_report_id.as_deref(), Some(damage_id.as_str()));
        assert_eq!(record.body_note.as_deref(), Some("scratch"));
        assert!(record.completed_at.is_some());

        let reports = scoped_a.damages().get_by_order("order-a").unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].id, damage_id);
        assert_eq!(reports[0].status, "reported");
        assert_eq!(reports[0].reported_by, "optical-actor-a");
        assert!(!reports[0].appearance_ok);
        assert!(reports[0].accessories_ok);
        assert!(reports[0].function_ok);
        assert!(reports[0].damage_description.contains("外观异常: scratch"));

        let connection = pool.get().unwrap();
        let damage_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM damage_reports
                 WHERE tenant_id='tenant-a' AND order_id='order-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(damage_count, 1);
        drop(connection);

        let stats = scoped_a.optical_sops().stats().unwrap();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.pass_count, 0);
        assert_eq!(stats.damage_count, 1);
        assert_eq!(stats.by_grade.get("minor_damage"), Some(&1));
    }
}

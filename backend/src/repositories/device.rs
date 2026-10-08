use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DeviceSortField {
    RentalStatus,
    Notes,
    WarningStatus,
    CreatedAt,
    #[default]
    SerialNo,
}

impl DeviceSortField {
    fn sqlite_column(self) -> &'static str {
        match self {
            Self::RentalStatus => "d.rentalStatus",
            Self::Notes => "d.notes",
            Self::WarningStatus => "d.warning_status",
            Self::CreatedAt => "d.createdAt",
            Self::SerialNo => "d.serialNo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DeviceSortDirection {
    #[default]
    Asc,
    Desc,
}

impl DeviceSortDirection {
    fn sqlite_keyword(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeviceListRequest {
    pub status: Option<String>,
    pub model_id: Option<String>,
    pub warehouse_id: Option<String>,
    pub serial_no: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DevicePagedRequest {
    pub page: u32,
    pub page_size: u32,
    pub keyword: Option<String>,
    pub rental_status: Option<String>,
    pub notes: Option<String>,
    pub warning_status: Option<String>,
    pub sort_by: DeviceSortField,
    pub sort_direction: DeviceSortDirection,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceCompatibilityReadRequest {
    pub keyword: Option<String>,
    pub rental_status: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCompatibilityReadRow {
    pub id: String,
    pub serial_no: String,
    pub rental_status: String,
    pub notes: String,
    pub fallback_return_node: String,
    pub model_id: String,
    pub current_warehouse_id: String,
    pub expected_warehouse_id: String,
    pub expected_available_date: String,
    pub created_at: String,
    pub order_return_node: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCheckinStatusMutation {
    pub before_status: String,
    pub after_status: String,
    pub already_checked_in: bool,
}

#[derive(Debug, Clone)]
pub struct ImportedDeviceDraft {
    pub id: String,
    pub serial_no: String,
    pub status: String,
    pub notes: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct NewDevice {
    pub id: String,
    pub serial_no: String,
    pub model_id: String,
    pub warehouse_id: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct DevicePatch {
    pub model_id: Option<String>,
    pub warehouse_id: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProjection {
    pub serial_no: String,
    pub model_id: String,
    pub model_name: Option<String>,
    pub warehouse_id: Option<String>,
    pub warehouse_name: Option<String>,
    pub status: String,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePageRow {
    pub serial_no: String,
    pub rental_status: String,
    pub notes: String,
    pub warning_status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePage {
    pub devices: Vec<DevicePageRow>,
    pub pagination: DevicePagination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePagination {
    pub page: u32,
    pub page_size: u32,
    pub total: u32,
    pub total_pages: u32,
}

pub(in crate::repositories) struct SqliteDeviceRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteDeviceRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn list(
        &self,
        request: &DeviceListRequest,
    ) -> Result<Vec<DeviceProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(|connection| {
            let mut sql = String::from(
                "SELECT d.serialNo, COALESCE(d.modelId, ''), dm.name, d.currentWarehouseId, w.name,
                        d.rentalStatus, d.notes, d.createdAt, d.createdAt
                 FROM devices d
                 LEFT JOIN device_models dm ON dm.id = d.modelId AND dm.tenant_id = d.tenant_id
                 LEFT JOIN warehouses w ON w.id = d.currentWarehouseId AND w.tenant_id = d.tenant_id
                 WHERE d.tenant_id = ?",
            );
            let mut values = vec![SqlValue::Text(tenant_id)];
            push_exact(
                &mut sql,
                &mut values,
                "d.rentalStatus",
                request.status.as_deref(),
            );
            push_exact(
                &mut sql,
                &mut values,
                "d.modelId",
                request.model_id.as_deref(),
            );
            push_exact(
                &mut sql,
                &mut values,
                "d.currentWarehouseId",
                request.warehouse_id.as_deref(),
            );
            push_exact(
                &mut sql,
                &mut values,
                "d.serialNo",
                request.serial_no.as_deref(),
            );
            sql.push_str(" ORDER BY d.serialNo");
            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(params_from_iter(values.iter()), map_device)?
                .collect()
        })
    }

    pub(in crate::repositories) fn list_paged(
        &self,
        request: &DevicePagedRequest,
    ) -> Result<DevicePage, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let page = request.page.max(1);
        let page_size = request.page_size.clamp(1, 200);
        self.session.read(|connection| {
            let mut predicates = vec!["d.tenant_id = ?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id)];

            if let Some(value) = normalized(request.keyword.as_deref()) {
                predicates.push("(d.serialNo LIKE ? OR COALESCE(d.notes, '') LIKE ?)".into());
                let pattern = SqlValue::Text(format!("%{value}%"));
                values.push(pattern.clone());
                values.push(pattern);
            }
            push_predicate_exact(
                &mut predicates,
                &mut values,
                "d.rentalStatus = ?",
                request.rental_status.as_deref(),
            );
            if let Some(value) = normalized(request.notes.as_deref()) {
                predicates.push("COALESCE(d.notes, '') LIKE ?".into());
                values.push(SqlValue::Text(format!("%{value}%")));
            }
            push_predicate_exact(
                &mut predicates,
                &mut values,
                "d.warning_status = ?",
                request.warning_status.as_deref(),
            );

            let where_sql = format!("WHERE {}", predicates.join(" AND "));
            let count_sql = format!("SELECT COUNT(*) FROM devices d {where_sql}");
            let total: u32 =
                connection.query_row(&count_sql, params_from_iter(values.iter()), |row| {
                    row.get(0)
                })?;
            let offset = u64::from(page - 1) * u64::from(page_size);
            let data_sql = format!(
                "SELECT d.serialNo, d.rentalStatus, COALESCE(d.notes, ''), d.warning_status, d.createdAt
                 FROM devices d {where_sql}
                 ORDER BY {} {}, d.serialNo ASC LIMIT ? OFFSET ?",
                request.sort_by.sqlite_column(),
                request.sort_direction.sqlite_keyword(),
            );
            let mut data_values = values;
            data_values.push(SqlValue::Integer(i64::from(page_size)));
            data_values.push(SqlValue::Integer(i64::try_from(offset).unwrap_or(i64::MAX)));
            let mut statement = connection.prepare(&data_sql)?;
            let devices = statement
                .query_map(params_from_iter(data_values.iter()), |row| {
                    Ok(DevicePageRow {
                        serial_no: row.get(0)?,
                        rental_status: row.get(1)?,
                        notes: row.get(2)?,
                        warning_status: row.get(3)?,
                        created_at: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(DevicePage {
                devices,
                pagination: DevicePagination {
                    page,
                    page_size,
                    total,
                    total_pages: if total == 0 {
                        0
                    } else {
                        total.div_ceil(page_size)
                    },
                },
            })
        })
    }

    pub(in crate::repositories) fn compatibility_count(
        &self,
        request: &DeviceCompatibilityReadRequest,
    ) -> Result<u32, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let request = request.clone();
        self.session.read(move |connection| {
            let (where_sql, values) = compatibility_where_sql(&tenant_id, &request);
            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM devices d {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;
            Ok(u32::try_from(total).unwrap_or(u32::MAX))
        })
    }

    pub(in crate::repositories) fn compatibility_rows(
        &self,
        request: &DeviceCompatibilityReadRequest,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<DeviceCompatibilityReadRow>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let request = request.clone();
        self.session.read(move |connection| {
            let (where_sql, mut values) = compatibility_where_sql(&tenant_id, &request);
            let mut sql = format!(
                "SELECT d.id,d.serialNo,d.rentalStatus,COALESCE(d.notes,''),
                        COALESCE(d.fallbackReturnNode,''),COALESCE(d.modelId,''),
                        COALESCE(d.currentWarehouseId,''),COALESCE(d.expectedWarehouseId,''),
                        COALESCE(d.expectedAvailableDate,''),d.createdAt,
                        COALESCE((
                            SELECT MAX(o.endDate)
                            FROM order_devices od
                            JOIN orders o ON o.id=od.orderId AND o.tenant_id=od.tenant_id
                            WHERE od.tenant_id=d.tenant_id AND od.serialNo=d.serialNo
                        ),'')
                 FROM devices d {where_sql}
                 ORDER BY d.createdAt DESC"
            );
            if let Some(limit) = limit {
                sql.push_str(" LIMIT ?");
                values.push(SqlValue::Integer(i64::from(limit)));
                if let Some(offset) = offset {
                    sql.push_str(" OFFSET ?");
                    values.push(SqlValue::Integer(i64::from(offset)));
                }
            }
            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(params_from_iter(values.iter()), map_compatibility_read_row)?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn compatibility_rows_by_serials(
        &self,
        serial_nos: &[String],
    ) -> Result<Vec<DeviceCompatibilityReadRow>, RepositoryError> {
        if serial_nos.is_empty() {
            return Ok(Vec::new());
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_nos = serial_nos.to_vec();
        self.session.read(move |connection| {
            let placeholders = vec!["?"; serial_nos.len()].join(",");
            let sql = format!(
                "SELECT d.id,d.serialNo,d.rentalStatus,COALESCE(d.notes,''),
                        COALESCE(d.fallbackReturnNode,''),COALESCE(d.modelId,''),
                        COALESCE(d.currentWarehouseId,''),COALESCE(d.expectedWarehouseId,''),
                        COALESCE(d.expectedAvailableDate,''),d.createdAt,
                        COALESCE((
                            SELECT MAX(o.endDate)
                            FROM order_devices od
                            JOIN orders o ON o.id=od.orderId AND o.tenant_id=od.tenant_id
                            WHERE od.tenant_id=d.tenant_id AND od.serialNo=d.serialNo
                        ),'')
                 FROM devices d
                 WHERE d.tenant_id=? AND d.serialNo IN ({placeholders})
                 ORDER BY d.createdAt DESC"
            );
            let mut values = vec![SqlValue::Text(tenant_id)];
            values.extend(serial_nos.into_iter().map(SqlValue::Text));
            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(params_from_iter(values.iter()), map_compatibility_read_row)?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn checkin_status(
        &self,
        serial_no: &str,
        checked_in_status: &str,
    ) -> Result<Option<DeviceCheckinStatusMutation>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_no = serial_no.to_owned();
        let checked_in_status = checked_in_status.to_owned();
        self.session.write_immediate(move |transaction| {
            let before_status = transaction
                .query_row(
                    "SELECT rentalStatus FROM devices
                     WHERE tenant_id=?1 AND serialNo=?2 LIMIT 1",
                    params![tenant_id, serial_no],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(sqlite_repository_error)?;
            let Some(before_status) = before_status else {
                return Ok(None);
            };
            if before_status == checked_in_status {
                return Ok(Some(DeviceCheckinStatusMutation {
                    before_status: before_status.clone(),
                    after_status: before_status,
                    already_checked_in: true,
                }));
            }
            let changed = transaction
                .execute(
                    "UPDATE devices SET rentalStatus=?1
                     WHERE tenant_id=?2 AND serialNo=?3",
                    params![checked_in_status, tenant_id, serial_no],
                )
                .map_err(sqlite_repository_error)?;
            if changed == 0 {
                return Ok(None);
            }
            Ok(Some(DeviceCheckinStatusMutation {
                before_status,
                after_status: checked_in_status,
                already_checked_in: false,
            }))
        })
    }

    pub(in crate::repositories) fn complete_legacy_orders_after_checkin(
        &self,
        serial_no: &str,
        checked_in_status: &str,
    ) -> Result<Vec<String>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_no = serial_no.to_owned();
        let checked_in_status = checked_in_status.to_owned();
        self.session.write_immediate(move |transaction| {
            let order_ids = {
                let mut statement = transaction
                    .prepare(
                        "SELECT DISTINCT od.orderId
                         FROM order_devices od
                         JOIN orders o
                           ON o.id=od.orderId AND o.tenant_id=od.tenant_id
                         WHERE od.tenant_id=?1 AND od.serialNo=?2
                           AND o.status IN ('reserved','active')
                         ORDER BY od.orderId",
                    )
                    .map_err(sqlite_repository_error)?;
                statement
                    .query_map(params![tenant_id, serial_no], |row| row.get::<_, String>(0))
                    .map_err(sqlite_repository_error)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(sqlite_repository_error)?
            };

            let mut completed = Vec::new();
            for order_id in order_ids {
                let pending: i64 = transaction
                    .query_row(
                        "SELECT COUNT(*)
                         FROM order_devices od
                         JOIN devices d
                           ON d.serialNo=od.serialNo AND d.tenant_id=od.tenant_id
                         WHERE od.tenant_id=?1 AND od.orderId=?2
                           AND d.rentalStatus<>?3",
                        params![tenant_id, order_id, checked_in_status],
                        |row| row.get(0),
                    )
                    .map_err(sqlite_repository_error)?;
                if pending != 0 {
                    continue;
                }
                let changed = transaction
                    .execute(
                        "UPDATE orders
                         SET status='completed'
                         WHERE tenant_id=?1 AND id=?2
                           AND status IN ('reserved','active')",
                        params![tenant_id, order_id],
                    )
                    .map_err(sqlite_repository_error)?;
                if changed == 1 {
                    completed.push(order_id);
                }
            }
            Ok(completed)
        })
    }

    pub(in crate::repositories) fn restore_status(
        &self,
        serial_no: &str,
        previous_status: &str,
    ) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_no = serial_no.to_owned();
        let previous_status = previous_status.to_owned();
        self.session
            .write(move |transaction| {
                transaction.execute(
                    "UPDATE devices SET rentalStatus=?1
                 WHERE tenant_id=?2 AND serialNo=?3",
                    params![previous_status, tenant_id, serial_no],
                )
            })
            .map(|changed| changed > 0)
    }

    pub(in crate::repositories) fn update_status_and_notes(
        &self,
        serial_no: &str,
        status: &str,
        notes: &str,
    ) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_no = serial_no.to_owned();
        let status = status.to_owned();
        let notes = notes.to_owned();
        self.session
            .write(move |transaction| {
                transaction.execute(
                    "UPDATE devices SET rentalStatus=?1,notes=?2
                 WHERE tenant_id=?3 AND serialNo=?4",
                    params![status, notes, tenant_id, serial_no],
                )
            })
            .map(|changed| changed > 0)
    }

    pub(in crate::repositories) fn import_device(
        &self,
        draft: &ImportedDeviceDraft,
    ) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let draft = draft.clone();
        self.session.write_immediate(move |transaction| {
            let exists: bool = transaction
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM devices
                        WHERE tenant_id=?1 AND serialNo=?2
                     )",
                    params![tenant_id, draft.serial_no],
                    |row| row.get(0),
                )
                .map_err(sqlite_repository_error)?;
            if exists {
                return Ok(false);
            }
            transaction
                .execute(
                    "INSERT INTO devices
                     (id,serialNo,rentalStatus,notes,createdAt,tenant_id)
                     VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        draft.id,
                        draft.serial_no,
                        draft.status,
                        draft.notes,
                        draft.created_at,
                        tenant_id,
                    ],
                )
                .map_err(sqlite_repository_error)?;
            Ok(true)
        })
    }

    pub(in crate::repositories) fn get(
        &self,
        serial_no: &str,
    ) -> Result<Option<DeviceProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_no = serial_no.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT d.serialNo, COALESCE(d.modelId, ''), dm.name, d.currentWarehouseId, w.name,
                            d.rentalStatus, d.notes, d.createdAt, d.createdAt
                     FROM devices d
                     LEFT JOIN device_models dm ON dm.id = d.modelId AND dm.tenant_id = d.tenant_id
                     LEFT JOIN warehouses w ON w.id = d.currentWarehouseId AND w.tenant_id = d.tenant_id
                     WHERE d.tenant_id = ?1 AND d.serialNo = ?2",
                    params![tenant_id, serial_no],
                    map_device,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn create(
        &self,
        input: &NewDevice,
    ) -> Result<DeviceProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let input = input.clone();
        self.session.write(|tx| {
            tx.execute(
                "INSERT INTO devices
                 (id, serialNo, modelId, currentWarehouseId, rentalStatus, createdAt, tenant_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    input.id,
                    input.serial_no,
                    input.model_id,
                    input.warehouse_id,
                    input.status,
                    input.created_at,
                    tenant_id,
                ],
            )?;
            Ok(())
        })?;
        self.get(&input.serial_no)?.ok_or_else(|| {
            RepositoryError::ContractViolation("created device could not be reloaded".into())
        })
    }

    pub(in crate::repositories) fn update(
        &self,
        serial_no: &str,
        patch: &DevicePatch,
    ) -> Result<Option<DeviceProjection>, RepositoryError> {
        if patch.model_id.is_none() && patch.warehouse_id.is_none() && patch.status.is_none() {
            return self.get(serial_no);
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_no_owned = serial_no.to_owned();
        let patch = patch.clone();
        let changed = self.session.write(|tx| {
            let mut sets = Vec::new();
            let mut values = Vec::<SqlValue>::new();
            if let Some(value) = patch.model_id {
                sets.push("modelId = ?".to_owned());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = patch.warehouse_id {
                sets.push("currentWarehouseId = ?".to_owned());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = patch.status {
                sets.push("rentalStatus = ?".to_owned());
                values.push(SqlValue::Text(value));
            }
            values.push(SqlValue::Text(tenant_id));
            values.push(SqlValue::Text(serial_no_owned.clone()));
            tx.execute(
                &format!(
                    "UPDATE devices SET {} WHERE tenant_id = ? AND serialNo = ?",
                    sets.join(", ")
                ),
                params_from_iter(values.iter()),
            )
        })?;
        if changed == 0 {
            return Ok(None);
        }
        self.get(serial_no)
    }

    pub(in crate::repositories) fn delete(&self, serial_no: &str) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_no = serial_no.to_owned();
        let changed = self.session.write(|tx| {
            tx.execute(
                "DELETE FROM devices WHERE tenant_id = ?1 AND serialNo = ?2",
                params![tenant_id, serial_no],
            )
        })?;
        Ok(changed > 0)
    }

    pub(in crate::repositories) fn list_serials(&self) -> Result<Vec<String>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(move |connection| {
            let mut statement = connection
                .prepare("SELECT serialNo FROM devices WHERE tenant_id = ?1 ORDER BY serialNo")?;
            statement
                .query_map([tenant_id], |row| row.get::<_, String>(0))?
                .collect()
        })
    }
}

fn map_device(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeviceProjection> {
    Ok(DeviceProjection {
        serial_no: row.get(0)?,
        model_id: row.get::<_, String>(1).unwrap_or_default(),
        model_name: row.get(2)?,
        warehouse_id: row.get::<_, Option<String>>(3)?,
        warehouse_name: row.get(4)?,
        status: row.get(5)?,
        notes: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn sqlite_repository_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn compatibility_where_sql(
    tenant_id: &str,
    request: &DeviceCompatibilityReadRequest,
) -> (String, Vec<SqlValue>) {
    let mut predicates = vec!["d.tenant_id = ?".to_owned()];
    let mut values = vec![SqlValue::Text(tenant_id.to_owned())];

    if let Some(value) = normalized(request.keyword.as_deref()) {
        predicates.push("d.serialNo LIKE ?".into());
        values.push(SqlValue::Text(format!("%{value}%")));
    }
    push_predicate_exact(
        &mut predicates,
        &mut values,
        "d.rentalStatus = ?",
        request.rental_status.as_deref(),
    );
    if let Some(value) = normalized(request.notes.as_deref()) {
        predicates.push("COALESCE(d.notes, '') LIKE ?".into());
        values.push(SqlValue::Text(format!("%{value}%")));
    }

    (format!("WHERE {}", predicates.join(" AND ")), values)
}

fn map_compatibility_read_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<DeviceCompatibilityReadRow> {
    Ok(DeviceCompatibilityReadRow {
        id: row.get(0)?,
        serial_no: row.get(1)?,
        rental_status: row.get(2)?,
        notes: row.get(3)?,
        fallback_return_node: row.get(4)?,
        model_id: row.get(5)?,
        current_warehouse_id: row.get(6)?,
        expected_warehouse_id: row.get(7)?,
        expected_available_date: row.get(8)?,
        created_at: row.get(9)?,
        order_return_node: row.get(10)?,
    })
}

fn normalized(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn push_exact(sql: &mut String, values: &mut Vec<SqlValue>, column: &str, value: Option<&str>) {
    if let Some(value) = normalized(value) {
        sql.push_str(" AND ");
        sql.push_str(column);
        sql.push_str(" = ?");
        values.push(SqlValue::Text(value.to_owned()));
    }
}

fn push_predicate_exact(
    predicates: &mut Vec<String>,
    values: &mut Vec<SqlValue>,
    predicate: &str,
    value: Option<&str>,
) {
    if let Some(value) = normalized(value) {
        predicates.push(predicate.to_owned());
        values.push(SqlValue::Text(value.to_owned()));
    }
}

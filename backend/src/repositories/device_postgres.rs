#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::device::{
    DeviceCheckinStatusMutation, DeviceCompatibilityReadRequest, DeviceCompatibilityReadRow,
    DeviceListRequest, DevicePage, DevicePagedRequest, DevicePagination, DevicePatch,
    DeviceProjection, DeviceSortDirection, DeviceSortField, ImportedDeviceDraft, NewDevice,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresDeviceRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresDeviceRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn list(
        &self,
        request: &DeviceListRequest,
    ) -> Result<Vec<DeviceProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let request = request.clone();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let mut query = QueryBuilder::<Postgres>::new(
                    "SELECT d.serialno, COALESCE(d.modelid, '') AS modelid, dm.name AS modelname,
                            d.currentwarehouseid, w.name AS warehousename, d.rentalstatus,
                            d.notes, d.createdat, d.createdat AS updatedat
                     FROM devices d
                     LEFT JOIN device_models dm ON dm.id = d.modelid AND dm.tenant_id = d.tenant_id
                     LEFT JOIN warehouses w ON w.id = d.currentwarehouseid AND w.tenant_id = d.tenant_id
                     WHERE d.tenant_id = ",
                );
                query.push_bind(tenant_id);
                push_exact(&mut query, "d.rentalstatus", request.status.as_deref());
                push_exact(&mut query, "d.modelid", request.model_id.as_deref());
                push_exact(
                    &mut query,
                    "d.currentwarehouseid",
                    request.warehouse_id.as_deref(),
                );
                push_exact(&mut query, "d.serialno", request.serial_no.as_deref());
                query.push(" ORDER BY d.serialno");
                query
                    .build()
                    .fetch_all(&mut *connection)
                    .await?
                    .iter()
                    .map(map_device)
                    .collect()
            })
        })
    }

    pub(in crate::repositories) fn list_paged(
        &self,
        request: &DevicePagedRequest,
    ) -> Result<DevicePage, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let request = request.clone();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let page = request.page.max(1);
                let page_size = request.page_size.clamp(1, 200);
                let mut count =
                    QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM devices d WHERE ");
                push_paged_predicates(&mut count, &tenant_id, &request);
                let total_i64: i64 = count
                    .build_query_scalar()
                    .fetch_one(&mut *connection)
                    .await?;
                let total = u32::try_from(total_i64).unwrap_or(u32::MAX);

                let mut data = QueryBuilder::<Postgres>::new(
                    "SELECT d.serialno, d.rentalstatus, COALESCE(d.notes, '') AS notes,
                            d.warning_status, d.createdat
                     FROM devices d WHERE ",
                );
                push_paged_predicates(&mut data, &tenant_id, &request);
                data.push(" ORDER BY ")
                    .push(pg_sort_column(request.sort_by))
                    .push(" ")
                    .push(pg_sort_direction(request.sort_direction))
                    .push(", d.serialno ASC LIMIT ")
                    .push_bind(i64::from(page_size))
                    .push(" OFFSET ")
                    .push_bind(i64::from(page - 1) * i64::from(page_size));
                let rows = data.build().fetch_all(&mut *connection).await?;
                let devices = rows
                    .iter()
                    .map(|row| {
                        Ok(crate::repositories::device::DevicePageRow {
                            serial_no: row.try_get("serialno")?,
                            rental_status: row.try_get("rentalstatus")?,
                            notes: row.try_get("notes")?,
                            warning_status: row.try_get("warning_status")?,
                            created_at: row.try_get("createdat")?,
                        })
                    })
                    .collect::<Result<Vec<_>, sqlx::Error>>()?;
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
        })
    }

    pub(in crate::repositories) fn compatibility_count(
        &self,
        request: &DeviceCompatibilityReadRequest,
    ) -> Result<u32, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let request = request.clone();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let mut query =
                    QueryBuilder::<Postgres>::new("SELECT COUNT(*)::bigint FROM devices d WHERE ");
                push_compatibility_predicates(&mut query, &tenant_id, &request);
                let total: i64 = query
                    .build_query_scalar()
                    .fetch_one(&mut *connection)
                    .await?;
                Ok(u32::try_from(total).unwrap_or(u32::MAX))
            })
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
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let mut query = QueryBuilder::<Postgres>::new(
                    "SELECT d.id,d.serialno,d.rentalstatus,COALESCE(d.notes,'') AS notes,
                            COALESCE(d.fallbackreturnnode,'') AS fallbackreturnnode,
                            COALESCE(d.modelid,'') AS modelid,
                            COALESCE(d.currentwarehouseid,'') AS currentwarehouseid,
                            COALESCE(d.expectedwarehouseid,'') AS expectedwarehouseid,
                            COALESCE(d.expectedavailabledate,'') AS expectedavailabledate,
                            d.createdat,
                            COALESCE((
                                SELECT MAX(o.enddate)
                                FROM order_devices od
                                JOIN orders o ON o.id=od.orderid AND o.tenant_id=od.tenant_id
                                WHERE od.tenant_id=d.tenant_id AND od.serialno=d.serialno
                            ),'') AS orderreturnnode
                     FROM devices d WHERE ",
                );
                push_compatibility_predicates(&mut query, &tenant_id, &request);
                query.push(" ORDER BY d.createdat DESC");
                if let Some(limit) = limit {
                    query.push(" LIMIT ").push_bind(i64::from(limit));
                    if let Some(offset) = offset {
                        query.push(" OFFSET ").push_bind(i64::from(offset));
                    }
                }
                query
                    .build()
                    .fetch_all(&mut *connection)
                    .await?
                    .iter()
                    .map(map_compatibility_read_row)
                    .collect()
            })
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
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let mut query = QueryBuilder::<Postgres>::new(
                    "SELECT d.id,d.serialno,d.rentalstatus,COALESCE(d.notes,'') AS notes,
                            COALESCE(d.fallbackreturnnode,'') AS fallbackreturnnode,
                            COALESCE(d.modelid,'') AS modelid,
                            COALESCE(d.currentwarehouseid,'') AS currentwarehouseid,
                            COALESCE(d.expectedwarehouseid,'') AS expectedwarehouseid,
                            COALESCE(d.expectedavailabledate,'') AS expectedavailabledate,
                            d.createdat,
                            COALESCE((
                                SELECT MAX(o.enddate)
                                FROM order_devices od
                                JOIN orders o ON o.id=od.orderid AND o.tenant_id=od.tenant_id
                                WHERE od.tenant_id=d.tenant_id AND od.serialno=d.serialno
                            ),'') AS orderreturnnode
                     FROM devices d WHERE d.tenant_id = ",
                );
                query.push_bind(tenant_id).push(" AND d.serialno IN (");
                {
                    let mut separated = query.separated(", ");
                    for serial_no in serial_nos {
                        separated.push_bind(serial_no);
                    }
                }
                query.push(") ORDER BY d.createdat DESC");
                query
                    .build()
                    .fetch_all(&mut *connection)
                    .await?
                    .iter()
                    .map(map_compatibility_read_row)
                    .collect()
            })
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
        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let before_status = sqlx::query_scalar::<_, String>(
                        "SELECT rentalstatus FROM devices
                         WHERE tenant_id=$1 AND serialno=$2
                         FOR UPDATE",
                    )
                    .bind(&tenant_id)
                    .bind(&serial_no)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?;
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
                    let changed = sqlx::query(
                        "UPDATE devices SET rentalstatus=$1
                         WHERE tenant_id=$2 AND serialno=$3",
                    )
                    .bind(&checked_in_status)
                    .bind(&tenant_id)
                    .bind(&serial_no)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();
                    if changed == 0 {
                        return Ok(None);
                    }
                    Ok(Some(DeviceCheckinStatusMutation {
                        before_status,
                        after_status: checked_in_status,
                        already_checked_in: false,
                    }))
                })
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
        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let order_ids = sqlx::query_scalar::<_, String>(
                        "SELECT DISTINCT od.orderid
                         FROM order_devices od
                         JOIN orders o
                           ON o.id=od.orderid AND o.tenant_id=od.tenant_id
                         WHERE od.tenant_id=$1 AND od.serialno=$2
                           AND o.status IN ('reserved','active')
                         ORDER BY od.orderid",
                    )
                    .bind(&tenant_id)
                    .bind(&serial_no)
                    .fetch_all(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let mut completed = Vec::new();
                    for order_id in order_ids {
                        let pending = sqlx::query_scalar::<_, i64>(
                            "SELECT COUNT(*)::bigint
                             FROM order_devices od
                             JOIN devices d
                               ON d.serialno=od.serialno AND d.tenant_id=od.tenant_id
                             WHERE od.tenant_id=$1 AND od.orderid=$2
                               AND d.rentalstatus<>$3",
                        )
                        .bind(&tenant_id)
                        .bind(&order_id)
                        .bind(&checked_in_status)
                        .fetch_one(&mut *connection)
                        .await
                        .map_err(pg_error)?;
                        if pending != 0 {
                            continue;
                        }

                        let changed = sqlx::query(
                            "UPDATE orders
                             SET status='completed'
                             WHERE tenant_id=$1 AND id=$2
                               AND status IN ('reserved','active')",
                        )
                        .bind(&tenant_id)
                        .bind(&order_id)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?
                        .rows_affected();
                        if changed == 1 {
                            completed.push(order_id);
                        }
                    }
                    Ok(completed)
                })
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
        self.session.pg_write(move |connection| {
            Box::pin(async move {
                Ok(sqlx::query(
                    "UPDATE devices SET rentalstatus=$1
                     WHERE tenant_id=$2 AND serialno=$3",
                )
                .bind(previous_status)
                .bind(tenant_id)
                .bind(serial_no)
                .execute(&mut *connection)
                .await?
                .rows_affected()
                    > 0)
            })
        })
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
        self.session.pg_write(move |connection| {
            Box::pin(async move {
                Ok(sqlx::query(
                    "UPDATE devices SET rentalstatus=$1,notes=$2
                     WHERE tenant_id=$3 AND serialno=$4",
                )
                .bind(status)
                .bind(notes)
                .bind(tenant_id)
                .bind(serial_no)
                .execute(&mut *connection)
                .await?
                .rows_affected()
                    > 0)
            })
        })
    }

    pub(in crate::repositories) fn import_device(
        &self,
        draft: &ImportedDeviceDraft,
    ) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let draft = draft.clone();
        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let exists = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(
                            SELECT 1 FROM devices
                            WHERE tenant_id=$1 AND serialno=$2
                         )",
                    )
                    .bind(&tenant_id)
                    .bind(&draft.serial_no)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    if exists {
                        return Ok(false);
                    }
                    sqlx::query(
                        "INSERT INTO devices
                         (id,serialno,rentalstatus,notes,createdat,tenant_id)
                         VALUES ($1,$2,$3,$4,$5,$6)",
                    )
                    .bind(draft.id)
                    .bind(draft.serial_no)
                    .bind(draft.status)
                    .bind(draft.notes)
                    .bind(draft.created_at)
                    .bind(tenant_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    Ok(true)
                })
            })
    }

    pub(in crate::repositories) fn get(
        &self,
        serial_no: &str,
    ) -> Result<Option<DeviceProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_no = serial_no.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT d.serialno, COALESCE(d.modelid, '') AS modelid, dm.name AS modelname,
                            d.currentwarehouseid, w.name AS warehousename, d.rentalstatus,
                            d.notes, d.createdat, d.createdat AS updatedat
                     FROM devices d
                     LEFT JOIN device_models dm ON dm.id = d.modelid AND dm.tenant_id = d.tenant_id
                     LEFT JOIN warehouses w ON w.id = d.currentwarehouseid AND w.tenant_id = d.tenant_id
                     WHERE d.tenant_id = $1 AND d.serialno = $2",
                )
                .bind(tenant_id)
                .bind(serial_no)
                .fetch_optional(&mut *connection)
                .await?;
                row.as_ref().map(map_device).transpose()
            })
        })
    }

    pub(in crate::repositories) fn create(
        &self,
        input: &NewDevice,
    ) -> Result<DeviceProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let input = input.clone();
        let serial_no = input.serial_no.clone();
        self.session.pg_write(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "INSERT INTO devices
                     (id, serialno, modelid, currentwarehouseid, rentalstatus, createdat, tenant_id)
                     VALUES ($1,$2,$3,$4,$5,$6,$7)",
                )
                .bind(input.id)
                .bind(input.serial_no)
                .bind(input.model_id)
                .bind(input.warehouse_id)
                .bind(input.status)
                .bind(input.created_at)
                .bind(tenant_id)
                .execute(&mut *connection)
                .await?;
                Ok(())
            })
        })?;
        self.get(&serial_no)?.ok_or_else(|| {
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
        let changed = self.session.pg_write(move |connection| {
            Box::pin(async move {
                Ok(sqlx::query(
                    "UPDATE devices
                     SET modelid = COALESCE($1, modelid),
                         currentwarehouseid = COALESCE($2, currentwarehouseid),
                         rentalstatus = COALESCE($3, rentalstatus)
                     WHERE tenant_id = $4 AND serialno = $5",
                )
                .bind(patch.model_id)
                .bind(patch.warehouse_id)
                .bind(patch.status)
                .bind(tenant_id)
                .bind(serial_no_owned)
                .execute(&mut *connection)
                .await?
                .rows_affected())
            })
        })?;
        if changed == 0 {
            return Ok(None);
        }
        self.get(serial_no)
    }

    pub(in crate::repositories) fn delete(&self, serial_no: &str) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial_no = serial_no.to_owned();
        let changed = self.session.pg_write(move |connection| {
            Box::pin(async move {
                Ok(
                    sqlx::query("DELETE FROM devices WHERE tenant_id = $1 AND serialno = $2")
                        .bind(tenant_id)
                        .bind(serial_no)
                        .execute(&mut *connection)
                        .await?
                        .rows_affected(),
                )
            })
        })?;
        Ok(changed > 0)
    }

    pub(in crate::repositories) fn list_serials(&self) -> Result<Vec<String>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query_scalar::<_, String>(
                    "SELECT serialno FROM devices WHERE tenant_id = $1 ORDER BY serialno",
                )
                .bind(tenant_id)
                .fetch_all(&mut *connection)
                .await
            })
        })
    }
}

fn map_device(row: &sqlx::postgres::PgRow) -> Result<DeviceProjection, sqlx::Error> {
    Ok(DeviceProjection {
        serial_no: row.try_get("serialno")?,
        model_id: row.try_get("modelid")?,
        model_name: row.try_get("modelname")?,
        warehouse_id: row.try_get("currentwarehouseid")?,
        warehouse_name: row.try_get("warehousename")?,
        status: row.try_get("rentalstatus")?,
        notes: row.try_get("notes")?,
        created_at: row.try_get("createdat")?,
        updated_at: row.try_get("updatedat")?,
    })
}

fn push_compatibility_predicates<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    request: &'args DeviceCompatibilityReadRequest,
) {
    query.push("d.tenant_id = ").push_bind(tenant_id);
    if let Some(value) = normalized(request.keyword.as_deref()) {
        query
            .push(" AND d.serialno ILIKE ")
            .push_bind(format!("%{value}%"));
    }
    push_exact(query, "d.rentalstatus", request.rental_status.as_deref());
    if let Some(value) = normalized(request.notes.as_deref()) {
        query
            .push(" AND COALESCE(d.notes, '') ILIKE ")
            .push_bind(format!("%{value}%"));
    }
}

fn map_compatibility_read_row(
    row: &sqlx::postgres::PgRow,
) -> Result<DeviceCompatibilityReadRow, sqlx::Error> {
    Ok(DeviceCompatibilityReadRow {
        id: row.try_get("id")?,
        serial_no: row.try_get("serialno")?,
        rental_status: row.try_get("rentalstatus")?,
        notes: row.try_get("notes")?,
        fallback_return_node: row.try_get("fallbackreturnnode")?,
        model_id: row.try_get("modelid")?,
        current_warehouse_id: row.try_get("currentwarehouseid")?,
        expected_warehouse_id: row.try_get("expectedwarehouseid")?,
        expected_available_date: row.try_get("expectedavailabledate")?,
        created_at: row.try_get("createdat")?,
        order_return_node: row.try_get("orderreturnnode")?,
    })
}

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn normalized(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn push_exact<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    column: &str,
    value: Option<&'args str>,
) {
    if let Some(value) = normalized(value) {
        query
            .push(" AND ")
            .push(column)
            .push(" = ")
            .push_bind(value);
    }
}

fn push_paged_predicates<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    request: &'args DevicePagedRequest,
) {
    query.push("d.tenant_id = ").push_bind(tenant_id);
    if let Some(value) = normalized(request.keyword.as_deref()) {
        let pattern = format!("%{value}%");
        query
            .push(" AND (d.serialno ILIKE ")
            .push_bind(pattern.clone())
            .push(" OR COALESCE(d.notes, '') ILIKE ")
            .push_bind(pattern)
            .push(")");
    }
    push_exact(query, "d.rentalstatus", request.rental_status.as_deref());
    if let Some(value) = normalized(request.notes.as_deref()) {
        query
            .push(" AND COALESCE(d.notes, '') ILIKE ")
            .push_bind(format!("%{value}%"));
    }
    push_exact(query, "d.warning_status", request.warning_status.as_deref());
}

fn pg_sort_column(field: DeviceSortField) -> &'static str {
    match field {
        DeviceSortField::RentalStatus => "d.rentalstatus",
        DeviceSortField::Notes => "d.notes",
        DeviceSortField::WarningStatus => "d.warning_status",
        DeviceSortField::CreatedAt => "d.createdat",
        DeviceSortField::SerialNo => "d.serialno",
    }
}

fn pg_sort_direction(direction: DeviceSortDirection) -> &'static str {
    match direction {
        DeviceSortDirection::Asc => "ASC",
        DeviceSortDirection::Desc => "DESC",
    }
}

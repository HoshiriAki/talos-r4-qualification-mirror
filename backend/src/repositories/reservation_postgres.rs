#![cfg(feature = "postgres")]

use chrono::{NaiveDate, SecondsFormat, Utc};
use sqlx::Row;

use crate::domain::{AllocationId, ReservationId, ReservationRequirementId};
use crate::repositories::RepositoryError;
use crate::repositories::reservation::{
    AllocationProjection, CapacityProjection, DeviceAllocationRequest,
    ReservationMigrationExceptionProjection, ReservationProjection,
    ReservationRequirementProjection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresReservationRepository<'a> {
    session: &'a RepositorySession,
}

struct RawRequirement {
    id: String,
    model_id: String,
    quantity: i64,
}

struct RawAllocation {
    id: String,
    reservation_id: String,
    order_id: Option<String>,
    device_serial_no: String,
    model_id: String,
    start_date: String,
    end_date: String,
    status: String,
    allocated_at: String,
    released_at: Option<String>,
}

struct RawReservation {
    id: String,
    order_id: Option<String>,
    source_kind: String,
    status: String,
    start_date: String,
    end_date: String,
    expires_at: String,
    version: i64,
    requirements: Vec<RawRequirement>,
    allocations: Vec<RawAllocation>,
    created_at: String,
    updated_at: String,
}

impl<'a> PostgresReservationRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn create_from_order(
        &self,
        _id: ReservationId,
        _order_id: &str,
        _hold_minutes: i64,
    ) -> Result<ReservationProjection, RepositoryError> {
        write_unavailable()
    }

    pub(in crate::repositories) fn find_by_order(
        &self,
        order_id: &str,
    ) -> Result<Option<ReservationProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        let raw_id = self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query_scalar::<_, String>(
                    "SELECT id FROM rental_reservations \
                     WHERE tenant_id = $1 AND order_id = $2 \
                     ORDER BY created_at DESC LIMIT 1",
                )
                .bind(&tenant_id)
                .bind(&order_id)
                .fetch_optional(connection)
                .await
            })
        })?;
        let Some(raw_id) = raw_id else {
            return Ok(None);
        };
        let id = ReservationId::parse(raw_id).map_err(|_| {
            RepositoryError::ContractViolation("persisted reservation id is not a UUID".into())
        })?;
        self.get(&id)
    }

    pub(in crate::repositories) fn allocation_complete_for_order(
        &self,
        order_id: &str,
    ) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        let counts = self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT COALESCE(SUM(rr.quantity), 0)::bigint AS required, \
                            (SELECT COUNT(*)::bigint FROM allocations a \
                             WHERE a.tenant_id = r.tenant_id \
                               AND a.reservation_id = r.id \
                               AND a.status = 'allocated') AS allocated \
                     FROM rental_reservations r \
                     JOIN reservation_requirements rr \
                       ON rr.tenant_id = r.tenant_id AND rr.reservation_id = r.id \
                     WHERE r.tenant_id = $1 AND r.order_id = $2 AND r.status = 'confirmed' \
                     GROUP BY r.id, r.tenant_id, r.created_at \
                     ORDER BY r.created_at DESC LIMIT 1",
                )
                .bind(&tenant_id)
                .bind(&order_id)
                .fetch_optional(connection)
                .await?;
                row.map(|row| {
                    Ok((
                        row.try_get::<i64, _>("required")?,
                        row.try_get::<i64, _>("allocated")?,
                    ))
                })
                .transpose()
            })
        })?;
        Ok(matches!(counts, Some((required, allocated)) if required > 0 && allocated >= required))
    }

    pub(in crate::repositories) fn create_legacy_device_hold(
        &self,
        _id: ReservationId,
        _device_serial_no: &str,
        _start_date: &str,
        _end_date: &str,
        _hold_minutes: i64,
    ) -> Result<ReservationProjection, RepositoryError> {
        write_unavailable()
    }

    pub(in crate::repositories) fn capacity(
        &self,
        model_id: &str,
        start_date: &str,
        end_date: &str,
        quantity: u32,
        device_serial_no: Option<&str>,
    ) -> Result<CapacityProjection, RepositoryError> {
        ensure_valid_half_open_interval(start_date, end_date)?;
        let model_id = model_id.trim();
        if model_id.is_empty() || quantity == 0 {
            return Err(RepositoryError::ContractViolation(
                "modelId and positive quantity are required".into(),
            ));
        }

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let model_id = model_id.to_owned();
        let start_date = start_date.trim().to_owned();
        let end_date = end_date.trim().to_owned();
        let serial = device_serial_no
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let model_for_projection = model_id.clone();
        let start_for_projection = start_date.clone();
        let end_for_projection = end_date.clone();
        let serial_for_projection = serial.clone();

        let (total, reserved, device_available) = self.session.pg_read(move |connection| {
            Box::pin(async move {
                let total = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*)::bigint FROM devices \
                     WHERE tenant_id = $1 AND COALESCE(modelid, '') = $2 \
                       AND COALESCE(rentalstatus, '') NOT IN ('scrapped', 'lost')",
                )
                .bind(&tenant_id)
                .bind(&model_id)
                .fetch_one(&mut *connection)
                .await?;

                let reserved = sqlx::query_scalar::<_, i64>(
                    "SELECT COALESCE(SUM(rr.quantity), 0)::bigint \
                     FROM reservation_requirements rr \
                     JOIN rental_reservations r \
                       ON r.id = rr.reservation_id AND r.tenant_id = rr.tenant_id \
                     WHERE rr.tenant_id = $1 AND rr.model_id = $2 \
                       AND r.status IN ('hold', 'confirmed') \
                       AND (r.status = 'confirmed' OR r.expires_at > $5) \
                       AND $3 < r.end_date AND r.start_date < $4",
                )
                .bind(&tenant_id)
                .bind(&model_id)
                .bind(&start_date)
                .bind(&end_date)
                .bind(&now)
                .fetch_one(&mut *connection)
                .await?;

                let device_available = if let Some(serial) = serial.as_deref() {
                    let model_matches = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM devices \
                         WHERE tenant_id = $1 AND serialno = $2 AND COALESCE(modelid, '') = $3)",
                    )
                    .bind(&tenant_id)
                    .bind(serial)
                    .bind(&model_id)
                    .fetch_one(&mut *connection)
                    .await?;
                    let overlaps = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM allocations \
                         WHERE tenant_id = $1 AND device_serial_no = $2 AND status = 'allocated' \
                           AND $3 < end_date AND start_date < $4)",
                    )
                    .bind(&tenant_id)
                    .bind(serial)
                    .bind(&start_date)
                    .bind(&end_date)
                    .fetch_one(&mut *connection)
                    .await?;
                    Some(model_matches && !overlaps)
                } else {
                    None
                };

                Ok((total, reserved, device_available))
            })
        })?;

        let available = total.saturating_sub(reserved).max(0) as u32;
        Ok(CapacityProjection {
            model_id: model_for_projection,
            start_date: start_for_projection,
            end_date: end_for_projection,
            total_capacity: total.max(0) as u32,
            reserved_capacity: reserved.max(0) as u32,
            available_capacity: available,
            requested_quantity: quantity,
            can_reserve: available >= quantity && device_available.unwrap_or(true),
            device_serial_no: serial_for_projection,
            device_available,
        })
    }

    pub(in crate::repositories) fn confirm(
        &self,
        _id: &ReservationId,
        _attach_order_id: Option<&str>,
    ) -> Result<ReservationProjection, RepositoryError> {
        write_unavailable()
    }

    pub(in crate::repositories) fn expire_due(&self) -> Result<u64, RepositoryError> {
        write_unavailable()
    }

    pub(in crate::repositories) fn allocate_device(
        &self,
        _reservation_id: &ReservationId,
        _allocation_id: AllocationId,
        _device_serial_no: &str,
    ) -> Result<AllocationProjection, RepositoryError> {
        write_unavailable()
    }

    pub(in crate::repositories) fn allocate_devices_batch(
        &self,
        _requests: &[DeviceAllocationRequest],
    ) -> Result<Vec<AllocationProjection>, RepositoryError> {
        write_unavailable()
    }

    pub(in crate::repositories) fn release_allocation(
        &self,
        _id: &AllocationId,
    ) -> Result<AllocationProjection, RepositoryError> {
        write_unavailable()
    }

    pub(in crate::repositories) fn get(
        &self,
        id: &ReservationId,
    ) -> Result<Option<ReservationProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.as_str().to_owned();
        let raw = self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT id, order_id, source_kind, status, start_date, end_date, expires_at, \
                            version, created_at, updated_at \
                     FROM rental_reservations WHERE tenant_id = $1 AND id = $2",
                )
                .bind(&tenant_id)
                .bind(&id)
                .fetch_optional(&mut *connection)
                .await?;
                let Some(row) = row else {
                    return Ok(None);
                };

                let reservation_id = row.try_get::<String, _>("id")?;
                let requirement_rows = sqlx::query(
                    "SELECT id, model_id, quantity FROM reservation_requirements \
                     WHERE tenant_id = $1 AND reservation_id = $2 ORDER BY model_id",
                )
                .bind(&tenant_id)
                .bind(&reservation_id)
                .fetch_all(&mut *connection)
                .await?;
                let requirements = requirement_rows
                    .into_iter()
                    .map(|row| {
                        Ok(RawRequirement {
                            id: row.try_get("id")?,
                            model_id: row.try_get("model_id")?,
                            quantity: row.try_get("quantity")?,
                        })
                    })
                    .collect::<Result<Vec<_>, sqlx::Error>>()?;

                let allocation_rows = sqlx::query(
                    "SELECT id, reservation_id, order_id, device_serial_no, model_id, \
                            start_date, end_date, status, allocated_at, released_at \
                     FROM allocations WHERE tenant_id = $1 AND reservation_id = $2 \
                     ORDER BY allocated_at, id",
                )
                .bind(&tenant_id)
                .bind(&reservation_id)
                .fetch_all(&mut *connection)
                .await?;
                let allocations = allocation_rows
                    .into_iter()
                    .map(|row| {
                        Ok(RawAllocation {
                            id: row.try_get("id")?,
                            reservation_id: row.try_get("reservation_id")?,
                            order_id: row.try_get("order_id")?,
                            device_serial_no: row.try_get("device_serial_no")?,
                            model_id: row.try_get("model_id")?,
                            start_date: row.try_get("start_date")?,
                            end_date: row.try_get("end_date")?,
                            status: row.try_get("status")?,
                            allocated_at: row.try_get("allocated_at")?,
                            released_at: row.try_get("released_at")?,
                        })
                    })
                    .collect::<Result<Vec<_>, sqlx::Error>>()?;

                Ok(Some(RawReservation {
                    id: reservation_id,
                    order_id: row.try_get("order_id")?,
                    source_kind: row.try_get("source_kind")?,
                    status: row.try_get("status")?,
                    start_date: row.try_get("start_date")?,
                    end_date: row.try_get("end_date")?,
                    expires_at: row.try_get("expires_at")?,
                    version: row.try_get("version")?,
                    requirements,
                    allocations,
                    created_at: row.try_get("created_at")?,
                    updated_at: row.try_get("updated_at")?,
                }))
            })
        })?;

        raw.map(project_raw_reservation).transpose()
    }

    pub(in crate::repositories) fn list_migration_exceptions(
        &self,
    ) -> Result<Vec<ReservationMigrationExceptionProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    "SELECT id, source_table, source_id, reason, status, canonical_reservation_id, created_at \
                     FROM reservation_migration_exceptions \
                     WHERE tenant_id = $1 ORDER BY created_at, source_table, source_id",
                )
                .bind(&tenant_id)
                .fetch_all(connection)
                .await?;
                rows.into_iter()
                    .map(|row| {
                        Ok(ReservationMigrationExceptionProjection {
                            id: row.try_get("id")?,
                            source_table: row.try_get("source_table")?,
                            source_id: row.try_get("source_id")?,
                            reason: row.try_get("reason")?,
                            status: row.try_get("status")?,
                            canonical_reservation_id: row.try_get("canonical_reservation_id")?,
                            created_at: row.try_get("created_at")?,
                        })
                    })
                    .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }
}

fn project_raw_reservation(raw: RawReservation) -> Result<ReservationProjection, RepositoryError> {
    let id = ReservationId::parse(raw.id).map_err(|_| {
        RepositoryError::ContractViolation("persisted reservation id is not a UUID".into())
    })?;
    let requirements = raw
        .requirements
        .into_iter()
        .map(|requirement| {
            let id = ReservationRequirementId::parse(requirement.id).map_err(|_| {
                RepositoryError::ContractViolation(
                    "persisted reservation requirement id is not a UUID".into(),
                )
            })?;
            let quantity = u32::try_from(requirement.quantity).map_err(|_| {
                RepositoryError::ContractViolation(
                    "persisted reservation requirement quantity is out of range".into(),
                )
            })?;
            Ok(ReservationRequirementProjection {
                id,
                model_id: requirement.model_id,
                quantity,
            })
        })
        .collect::<Result<Vec<_>, RepositoryError>>()?;
    let allocations = raw
        .allocations
        .into_iter()
        .map(|allocation| {
            Ok(AllocationProjection {
                id: AllocationId::parse(allocation.id).map_err(|_| {
                    RepositoryError::ContractViolation(
                        "persisted allocation id is not a UUID".into(),
                    )
                })?,
                reservation_id: ReservationId::parse(allocation.reservation_id).map_err(|_| {
                    RepositoryError::ContractViolation(
                        "persisted allocation reservation id is not a UUID".into(),
                    )
                })?,
                order_id: allocation.order_id,
                device_serial_no: allocation.device_serial_no,
                model_id: allocation.model_id,
                start_date: allocation.start_date,
                end_date: allocation.end_date,
                status: allocation.status,
                allocated_at: allocation.allocated_at,
                released_at: allocation.released_at,
            })
        })
        .collect::<Result<Vec<_>, RepositoryError>>()?;

    Ok(ReservationProjection {
        id,
        order_id: raw.order_id,
        source_kind: raw.source_kind,
        status: raw.status,
        start_date: raw.start_date,
        end_date: raw.end_date,
        expires_at: raw.expires_at,
        version: raw.version,
        requirements,
        allocations,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
    })
}

fn ensure_valid_half_open_interval(
    start_date: &str,
    end_date: &str,
) -> Result<(), RepositoryError> {
    let start = NaiveDate::parse_from_str(start_date.trim(), "%Y-%m-%d")
        .map_err(|_| RepositoryError::ContractViolation("startDate must be YYYY-MM-DD".into()))?;
    let end = NaiveDate::parse_from_str(end_date.trim(), "%Y-%m-%d")
        .map_err(|_| RepositoryError::ContractViolation("endDate must be YYYY-MM-DD".into()))?;
    if start >= end {
        return Err(RepositoryError::ContractViolation(
            "reservation interval must satisfy startDate < endDate for [start,end)".into(),
        ));
    }
    Ok(())
}

fn write_unavailable<T>() -> Result<T, RepositoryError> {
    Err(RepositoryError::AdapterUnavailable(
        "PostgreSQL Reservation/Allocation write adapter is not migrated yet; P8-C keeps this path fail-closed"
            .into(),
    ))
}

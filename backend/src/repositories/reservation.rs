use chrono::{Duration, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::domain::{AllocationId, ReservationId, ReservationRequirementId};
use crate::repositories::sqlite::SqliteRepositorySession;
use crate::repositories::workflow::append_outbox_tx;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationRequirementProjection {
    pub id: ReservationRequirementId,
    pub model_id: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationProjection {
    pub id: AllocationId,
    pub reservation_id: ReservationId,
    pub order_id: Option<String>,
    pub device_serial_no: String,
    pub model_id: String,
    pub start_date: String,
    pub end_date: String,
    pub status: String,
    pub allocated_at: String,
    pub released_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationProjection {
    pub id: ReservationId,
    pub order_id: Option<String>,
    pub source_kind: String,
    pub status: String,
    pub start_date: String,
    pub end_date: String,
    pub expires_at: String,
    pub version: i64,
    pub requirements: Vec<ReservationRequirementProjection>,
    pub allocations: Vec<AllocationProjection>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapacityProjection {
    pub model_id: String,
    pub start_date: String,
    pub end_date: String,
    pub total_capacity: u32,
    pub reserved_capacity: u32,
    pub available_capacity: u32,
    pub requested_quantity: u32,
    pub can_reserve: bool,
    pub device_serial_no: Option<String>,
    pub device_available: Option<bool>,
}

/// A tenant-scoped request for one canonical device allocation in a batch.
///
/// Allocation identifiers are generated inside the repository transaction so a
/// caller cannot smuggle a pre-created identifier or a different tenant scope
/// into the batch command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAllocationRequest {
    pub order_id: String,
    pub device_serial_no: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationMigrationExceptionProjection {
    pub id: String,
    pub source_table: String,
    pub source_id: String,
    pub reason: String,
    pub status: String,
    pub canonical_reservation_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
struct OrderDemand {
    start_date: String,
    end_date: String,
    requirements: Vec<(String, u32)>,
}

pub struct ScopedReservationRepository<'a> {
    session: &'a SqliteRepositorySession,
}

impl<'a> ScopedReservationRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub fn create_from_order(
        &self,
        id: ReservationId,
        order_id: &str,
        hold_minutes: i64,
    ) -> Result<ReservationProjection, RepositoryError> {
        let order_id = order_id.trim();
        if order_id.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "orderId must not be blank".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        let expires_at = (Utc::now() + Duration::minutes(hold_minutes.clamp(1, 1440)))
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        let id_for_tx = id.clone();
        self.session.write_immediate(|transaction| {
            let demand = load_order_demand(transaction, &tenant_id, order_id)?;
            ensure_valid_half_open_interval(&demand.start_date, &demand.end_date)?;
            ensure_capacity(
                transaction,
                &tenant_id,
                &demand.start_date,
                &demand.end_date,
                &demand.requirements,
                &now,
            )?;
            transaction
                .execute(
                    "INSERT INTO rental_reservations
                     (id, tenant_id, order_id, source_kind, status, start_date, end_date,
                      expires_at, version, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'order', 'hold', ?4, ?5, ?6, 1, ?7, ?7)",
                    params![
                        id_for_tx.as_str(),
                        tenant_id,
                        order_id,
                        demand.start_date,
                        demand.end_date,
                        expires_at,
                        now,
                    ],
                )
                .map_err(sqlite_error)?;
            insert_requirements(
                transaction,
                &tenant_id,
                &id_for_tx,
                &demand.requirements,
                &now,
            )?;
            Ok(())
        })?;
        self.get(&id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("created reservation could not be reloaded".into())
        })
    }

    pub fn find_by_order(
        &self,
        order_id: &str,
    ) -> Result<Option<ReservationProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        self.session.read(|connection| {
            let id: Option<String> = connection
                .query_row(
                    "SELECT id FROM rental_reservations WHERE tenant_id=?1 AND order_id=?2 ORDER BY created_at DESC LIMIT 1",
                    params![tenant_id, order_id],
                    |row| row.get(0),
                )
                .optional()?;
            id.map(|value| ReservationId::parse(value).map_err(|_| rusqlite::Error::InvalidQuery))
                .transpose()
        }).and_then(|id| match id { Some(id) => self.get(&id), None => Ok(None) })
    }

    pub fn allocation_complete_for_order(&self, order_id: &str) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = order_id.trim().to_owned();
        self.session.read(|connection| {
            let counts: Option<(i64, i64)> = connection.query_row(
                "SELECT COALESCE(SUM(rr.quantity),0),
                        (SELECT COUNT(*) FROM allocations a WHERE a.tenant_id=r.tenant_id AND a.reservation_id=r.id AND a.status='allocated')
                 FROM rental_reservations r
                 JOIN reservation_requirements rr ON rr.tenant_id=r.tenant_id AND rr.reservation_id=r.id
                 WHERE r.tenant_id=?1 AND r.order_id=?2 AND r.status='confirmed'
                 GROUP BY r.id ORDER BY r.created_at DESC LIMIT 1",
                params![tenant_id, order_id], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
            Ok(matches!(counts, Some((required, allocated)) if required > 0 && allocated >= required))
        })
    }

    pub fn create_legacy_device_hold(
        &self,
        id: ReservationId,
        device_serial_no: &str,
        start_date: &str,
        end_date: &str,
        hold_minutes: i64,
    ) -> Result<ReservationProjection, RepositoryError> {
        ensure_valid_half_open_interval(start_date, end_date)?;
        let serial = device_serial_no.trim();
        if serial.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "deviceSerialNo must not be blank".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        let expires_at = (Utc::now() + Duration::minutes(hold_minutes.clamp(1, 1440)))
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        let id_for_tx = id.clone();
        self.session.write_immediate(|transaction| {
            let model_id: String = transaction
                .query_row(
                    "SELECT COALESCE(modelId, '') FROM devices
                     WHERE tenant_id = ?1 AND serialNo = ?2",
                    params![tenant_id, serial],
                    |row| row.get(0),
                )
                .map_err(|_| {
                    RepositoryError::ContractViolation(
                        "legacy booking device does not exist in active tenant".into(),
                    )
                })?;
            if model_id.trim().is_empty() {
                return Err(RepositoryError::ContractViolation(
                    "legacy booking device has no modelId".into(),
                ));
            }
            let requirements = vec![(model_id, 1_u32)];
            ensure_capacity(
                transaction,
                &tenant_id,
                start_date,
                end_date,
                &requirements,
                &now,
            )?;
            ensure_device_unallocated(transaction, &tenant_id, serial, start_date, end_date)?;
            transaction
                .execute(
                    "INSERT INTO rental_reservations
                     (id, tenant_id, order_id, source_kind, status, start_date, end_date,
                      expires_at, version, created_at, updated_at)
                     VALUES (?1, ?2, NULL, 'legacy_booking', 'hold', ?3, ?4, ?5, 1, ?6, ?6)",
                    params![
                        id_for_tx.as_str(),
                        tenant_id,
                        start_date,
                        end_date,
                        expires_at,
                        now
                    ],
                )
                .map_err(sqlite_error)?;
            insert_requirements(transaction, &tenant_id, &id_for_tx, &requirements, &now)?;
            Ok(())
        })?;
        self.get(&id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("created legacy hold could not be reloaded".into())
        })
    }

    pub fn capacity(
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
        let now = utc_now();
        self.session.read(|connection| {
            let total: i64 = connection.query_row(
                "SELECT COUNT(*) FROM devices
                 WHERE tenant_id = ?1 AND COALESCE(modelId, '') = ?2
                   AND COALESCE(rentalStatus, '') NOT IN ('scrapped', 'lost')",
                params![tenant_id, model_id],
                |row| row.get(0),
            )?;
            let reserved: i64 = connection.query_row(
                "SELECT COALESCE(SUM(rr.quantity), 0)
                 FROM reservation_requirements rr
                 JOIN rental_reservations r
                   ON r.id = rr.reservation_id AND r.tenant_id = rr.tenant_id
                 WHERE rr.tenant_id = ?1 AND rr.model_id = ?2
                   AND r.status IN ('hold', 'confirmed')
                   AND (r.status = 'confirmed' OR r.expires_at > ?5)
                   AND ?3 < r.end_date AND r.start_date < ?4",
                params![tenant_id, model_id, start_date, end_date, now],
                |row| row.get(0),
            )?;
            let device_available = if let Some(serial) = device_serial_no {
                let model_matches: bool = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM devices
                     WHERE tenant_id = ?1 AND serialNo = ?2 AND COALESCE(modelId, '') = ?3)",
                    params![tenant_id, serial, model_id],
                    |row| row.get(0),
                )?;
                let overlaps: bool = connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM allocations
                     WHERE tenant_id = ?1 AND device_serial_no = ?2 AND status = 'allocated'
                       AND ?3 < end_date AND start_date < ?4)",
                    params![tenant_id, serial, start_date, end_date],
                    |row| row.get(0),
                )?;
                Some(model_matches && !overlaps)
            } else {
                None
            };
            let available = total.saturating_sub(reserved).max(0) as u32;
            Ok(CapacityProjection {
                model_id: model_id.to_string(),
                start_date: start_date.to_string(),
                end_date: end_date.to_string(),
                total_capacity: total.max(0) as u32,
                reserved_capacity: reserved.max(0) as u32,
                available_capacity: available,
                requested_quantity: quantity,
                can_reserve: available >= quantity && device_available.unwrap_or(true),
                device_serial_no: device_serial_no.map(str::to_owned),
                device_available,
            })
        })
    }

    pub fn confirm(
        &self,
        id: &ReservationId,
        attach_order_id: Option<&str>,
    ) -> Result<ReservationProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write_immediate(|transaction| {
            let current: (String, Option<String>, String, String, String) = transaction
                .query_row(
                    "SELECT status, order_id, start_date, end_date, expires_at
                     FROM rental_reservations WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, id.as_str()],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .map_err(|_| RepositoryError::ContractViolation("reservation not found".into()))?;
            if current.0 != "hold" {
                return Err(RepositoryError::ContractViolation(
                    "only hold reservations can be confirmed".into(),
                ));
            }
            if current.4 <= now {
                return Err(RepositoryError::ContractViolation(
                    "expired hold cannot be confirmed".into(),
                ));
            }
            let mut order_id = current.1;
            if let Some(candidate) = attach_order_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                let demand = load_order_demand(transaction, &tenant_id, candidate)?;
                if demand.start_date != current.2 || demand.end_date != current.3 {
                    return Err(RepositoryError::ContractViolation(
                        "attached order interval must match reservation exactly".into(),
                    ));
                }
                ensure_order_covers_requirements(
                    transaction,
                    &tenant_id,
                    id,
                    &demand.requirements,
                )?;
                order_id = Some(candidate.to_string());
            }
            if order_id.is_none() {
                return Err(RepositoryError::ContractViolation(
                    "reservation must be attached to an order before confirmation".into(),
                ));
            }
            transaction
                .execute(
                    "UPDATE rental_reservations
                     SET order_id = ?3, status = 'confirmed', version = version + 1, updated_at = ?4
                     WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, id.as_str(), order_id, now],
                )
                .map_err(sqlite_error)?;
            let attached_order_id = order_id.as_deref().expect("validated attached order");
            append_outbox_tx(
                transaction,
                &tenant_id,
                "reservation",
                id.as_str(),
                "ReservationConfirmed",
                &format!("reservation:{}:confirmed", id.as_str()),
                &serde_json::json!({"reservationId": id.as_str(), "orderId": attached_order_id}),
                &now,
            )?;
            Ok(())
        })?;
        self.get(id)?
            .ok_or_else(|| RepositoryError::ContractViolation("reservation not found".into()))
    }

    pub fn expire_due(&self) -> Result<u64, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write_immediate(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE rental_reservations
                     SET status = 'expired', version = version + 1, updated_at = ?2
                     WHERE tenant_id = ?1 AND status = 'hold' AND expires_at <= ?2",
                    params![tenant_id, now],
                )
                .map_err(sqlite_error)?;
            Ok(changed as u64)
        })
    }

    pub fn allocate_device(
        &self,
        reservation_id: &ReservationId,
        allocation_id: AllocationId,
        device_serial_no: &str,
    ) -> Result<AllocationProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial = device_serial_no.trim();
        if serial.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "deviceSerialNo must not be blank".into(),
            ));
        }
        let now = utc_now();
        let allocation_id_for_tx = allocation_id.clone();
        self.session.write_immediate(|transaction| {
            allocate_device_tx(
                transaction,
                &tenant_id,
                reservation_id,
                &allocation_id_for_tx,
                serial,
                &now,
            )
        })
    }

    /// Allocate every validated device in one canonical repository transaction.
    ///
    /// The transaction-scoped helper is shared with `allocate_device`, so the
    /// confirmed-reservation, device existence, overlap, capacity and outbox
    /// invariants cannot drift between single-row and import execution.
    pub fn allocate_devices_batch(
        &self,
        requests: &[DeviceAllocationRequest],
    ) -> Result<Vec<AllocationProjection>, RepositoryError> {
        if requests.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "device allocation batch must contain at least one row".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write_immediate(|transaction| {
            let mut allocations = Vec::with_capacity(requests.len());
            for request in requests {
                let order_id = request.order_id.trim();
                let serial = request.device_serial_no.trim();
                if order_id.is_empty() || serial.is_empty() {
                    return Err(RepositoryError::ContractViolation(
                        "each device allocation row requires orderId and deviceSerialNo".into(),
                    ));
                }
                let reservation_raw: String = transaction
                    .query_row(
                        "SELECT id FROM rental_reservations
                         WHERE tenant_id = ?1 AND order_id = ?2
                         ORDER BY created_at DESC LIMIT 1",
                        params![tenant_id, order_id],
                        |row| row.get(0),
                    )
                    .map_err(|_| {
                        RepositoryError::ContractViolation(
                            "canonical reservation not found for Order".into(),
                        )
                    })?;
                let reservation_id = ReservationId::parse(reservation_raw).map_err(|_| {
                    RepositoryError::ContractViolation("canonical reservation id is invalid".into())
                })?;
                allocations.push(allocate_device_tx(
                    transaction,
                    &tenant_id,
                    &reservation_id,
                    &AllocationId::new(),
                    serial,
                    &now,
                )?);
            }
            Ok(allocations)
        })
    }

    pub fn release_allocation(
        &self,
        id: &AllocationId,
    ) -> Result<AllocationProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write_immediate(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE allocations SET status = 'released', released_at = ?3
                     WHERE tenant_id = ?1 AND id = ?2 AND status = 'allocated'",
                    params![tenant_id, id.as_str(), now],
                )
                .map_err(sqlite_error)?;
            if changed != 1 {
                return Err(RepositoryError::ContractViolation(
                    "active allocation not found".into(),
                ));
            }
            Ok(())
        })?;
        self.get_allocation(id)?
            .ok_or_else(|| RepositoryError::ContractViolation("allocation not found".into()))
    }

    pub fn get(
        &self,
        id: &ReservationId,
    ) -> Result<Option<ReservationProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(|connection| {
            let base = connection
                .query_row(
                    "SELECT id, order_id, source_kind, status, start_date, end_date, expires_at,
                            version, created_at, updated_at
                     FROM rental_reservations WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, id.as_str()],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, i64>(7)?,
                            row.get::<_, String>(8)?,
                            row.get::<_, String>(9)?,
                        ))
                    },
                )
                .optional()?;
            let Some(base) = base else {
                return Ok(None);
            };
            let requirements = load_requirements(connection, &tenant_id, &base.0)?;
            let allocations = load_allocations(connection, &tenant_id, &base.0)?;
            Ok(Some(ReservationProjection {
                id: ReservationId::parse(base.0).map_err(|_| rusqlite::Error::InvalidQuery)?,
                order_id: base.1,
                source_kind: base.2,
                status: base.3,
                start_date: base.4,
                end_date: base.5,
                expires_at: base.6,
                version: base.7,
                requirements,
                allocations,
                created_at: base.8,
                updated_at: base.9,
            }))
        })
    }

    pub fn list_migration_exceptions(
        &self,
    ) -> Result<Vec<ReservationMigrationExceptionProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, source_table, source_id, reason, status, canonical_reservation_id, created_at
                 FROM reservation_migration_exceptions
                 WHERE tenant_id = ?1 ORDER BY created_at, source_table, source_id",
            )?;
            let rows = statement.query_map(params![tenant_id], |row| {
                Ok(ReservationMigrationExceptionProjection {
                    id: row.get(0)?, source_table: row.get(1)?, source_id: row.get(2)?,
                    reason: row.get(3)?, status: row.get(4)?, canonical_reservation_id: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?;
            rows.collect()
        })
    }

    fn get_allocation(
        &self,
        id: &AllocationId,
    ) -> Result<Option<AllocationProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(|connection| {
            connection
                .query_row(
                    "SELECT id, reservation_id, order_id, device_serial_no, model_id,
                            start_date, end_date, status, allocated_at, released_at
                     FROM allocations WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, id.as_str()],
                    allocation_from_row,
                )
                .optional()
        })
    }
}

fn allocate_device_tx(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    reservation_id: &ReservationId,
    allocation_id: &AllocationId,
    serial: &str,
    now: &str,
) -> Result<AllocationProjection, RepositoryError> {
    let reservation: (Option<String>, String, String, String) = transaction
        .query_row(
            "SELECT order_id, status, start_date, end_date
             FROM rental_reservations WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, reservation_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| RepositoryError::ContractViolation("reservation not found".into()))?;
    if reservation.1 != "confirmed" {
        return Err(RepositoryError::ContractViolation(
            "device allocation requires confirmed reservation".into(),
        ));
    }
    let model_id: String = transaction
        .query_row(
            "SELECT COALESCE(modelId, '') FROM devices
             WHERE tenant_id = ?1 AND serialNo = ?2",
            params![tenant_id, serial],
            |row| row.get(0),
        )
        .map_err(|_| RepositoryError::ContractViolation("device not found".into()))?;
    transaction
        .execute(
            "INSERT INTO allocations
             (id, tenant_id, reservation_id, order_id, device_serial_no, model_id,
              start_date, end_date, status, allocated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'allocated', ?9)",
            params![
                allocation_id.as_str(),
                tenant_id,
                reservation_id.as_str(),
                reservation.0.as_deref(),
                serial,
                &model_id,
                &reservation.2,
                &reservation.3,
                now,
            ],
        )
        .map_err(|error| RepositoryError::ContractViolation(error.to_string()))?;
    let required: i64 = transaction
        .query_row(
            "SELECT COALESCE(SUM(quantity),0) FROM reservation_requirements
             WHERE tenant_id=?1 AND reservation_id=?2",
            params![tenant_id, reservation_id.as_str()],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    let allocated: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM allocations
             WHERE tenant_id=?1 AND reservation_id=?2 AND status='allocated'",
            params![tenant_id, reservation_id.as_str()],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if required > 0 && allocated >= required {
        if let Some(order_id) = reservation.0.as_deref() {
            append_outbox_tx(
                transaction,
                tenant_id,
                "allocation",
                reservation_id.as_str(),
                "AllocationComplete",
                &format!(
                    "reservation:{}:allocation-complete",
                    reservation_id.as_str()
                ),
                &serde_json::json!({"reservationId": reservation_id.as_str(), "orderId": order_id}),
                now,
            )?;
        }
    }
    Ok(AllocationProjection {
        id: allocation_id.clone(),
        reservation_id: reservation_id.clone(),
        order_id: reservation.0,
        device_serial_no: serial.to_owned(),
        model_id,
        start_date: reservation.2,
        end_date: reservation.3,
        status: "allocated".into(),
        allocated_at: now.to_owned(),
        released_at: None,
    })
}

fn load_order_demand(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    order_id: &str,
) -> Result<OrderDemand, RepositoryError> {
    let (start_date, end_date): (String, String) = transaction
        .query_row(
            "SELECT startDate, endDate FROM orders WHERE tenant_id = ?1 AND id = ?2",
            params![tenant_id, order_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| {
            RepositoryError::ContractViolation("order not found in active tenant".into())
        })?;
    let mut statement = transaction
        .prepare(
            "SELECT reference_id, SUM(quantity)
             FROM order_lines
             WHERE tenant_id = ?1 AND order_id = ?2 AND line_kind = 'model'
             GROUP BY reference_id ORDER BY reference_id",
        )
        .map_err(sqlite_error)?;
    let requirements = statement
        .query_map(params![tenant_id, order_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })
        .map_err(sqlite_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_error)?;
    if requirements.is_empty() {
        return Err(RepositoryError::ContractViolation(
            "order has no model OrderLines to reserve".into(),
        ));
    }
    Ok(OrderDemand {
        start_date,
        end_date,
        requirements,
    })
}

fn ensure_capacity(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    start_date: &str,
    end_date: &str,
    requirements: &[(String, u32)],
    now: &str,
) -> Result<(), RepositoryError> {
    for (model_id, quantity) in requirements {
        let total: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM devices
                 WHERE tenant_id = ?1 AND COALESCE(modelId, '') = ?2
                   AND COALESCE(rentalStatus, '') NOT IN ('scrapped', 'lost')",
                params![tenant_id, model_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        let reserved: i64 = transaction
            .query_row(
                "SELECT COALESCE(SUM(rr.quantity), 0)
                 FROM reservation_requirements rr
                 JOIN rental_reservations r ON r.id = rr.reservation_id AND r.tenant_id = rr.tenant_id
                 WHERE rr.tenant_id = ?1 AND rr.model_id = ?2
                   AND r.status IN ('hold', 'confirmed')
                   AND (r.status = 'confirmed' OR r.expires_at > ?5)
                   AND ?3 < r.end_date AND r.start_date < ?4",
                params![tenant_id, model_id, start_date, end_date, now],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        if total.saturating_sub(reserved) < i64::from(*quantity) {
            return Err(RepositoryError::ContractViolation(format!(
                "insufficient model capacity for {model_id}: requested {quantity}, available {}",
                total.saturating_sub(reserved).max(0)
            )));
        }
    }
    Ok(())
}

fn ensure_device_unallocated(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    serial: &str,
    start_date: &str,
    end_date: &str,
) -> Result<(), RepositoryError> {
    let overlap: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM allocations
         WHERE tenant_id = ?1 AND device_serial_no = ?2 AND status = 'allocated'
           AND ?3 < end_date AND start_date < ?4)",
            params![tenant_id, serial, start_date, end_date],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if overlap {
        return Err(RepositoryError::ContractViolation(
            "device already allocated in overlapping interval".into(),
        ));
    }
    Ok(())
}

fn insert_requirements(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    reservation_id: &ReservationId,
    requirements: &[(String, u32)],
    now: &str,
) -> Result<(), RepositoryError> {
    for (model_id, quantity) in requirements {
        let id = ReservationRequirementId::new();
        transaction
            .execute(
                "INSERT INTO reservation_requirements
             (id, tenant_id, reservation_id, model_id, quantity, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id.as_str(),
                    tenant_id,
                    reservation_id.as_str(),
                    model_id,
                    quantity,
                    now
                ],
            )
            .map_err(sqlite_error)?;
    }
    Ok(())
}

fn ensure_order_covers_requirements(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    reservation_id: &ReservationId,
    order_requirements: &[(String, u32)],
) -> Result<(), RepositoryError> {
    let mut statement = transaction
        .prepare(
            "SELECT model_id, quantity FROM reservation_requirements
         WHERE tenant_id = ?1 AND reservation_id = ?2 ORDER BY model_id",
        )
        .map_err(sqlite_error)?;
    let reservation_requirements = statement
        .query_map(params![tenant_id, reservation_id.as_str()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })
        .map_err(sqlite_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sqlite_error)?;
    for (model_id, quantity) in reservation_requirements {
        let available = order_requirements
            .iter()
            .find(|(candidate, _)| candidate == &model_id)
            .map(|(_, qty)| *qty)
            .unwrap_or(0);
        if available < quantity {
            return Err(RepositoryError::ContractViolation(
                "attached order does not cover legacy reservation model demand".into(),
            ));
        }
    }
    Ok(())
}

fn load_requirements(
    connection: &rusqlite::Connection,
    tenant_id: &str,
    reservation_id: &str,
) -> rusqlite::Result<Vec<ReservationRequirementProjection>> {
    let mut statement = connection.prepare(
        "SELECT id, model_id, quantity FROM reservation_requirements
         WHERE tenant_id = ?1 AND reservation_id = ?2 ORDER BY model_id",
    )?;
    statement
        .query_map(params![tenant_id, reservation_id], |row| {
            let raw: String = row.get(0)?;
            Ok(ReservationRequirementProjection {
                id: ReservationRequirementId::parse(raw)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
                model_id: row.get(1)?,
                quantity: row.get(2)?,
            })
        })?
        .collect()
}

fn load_allocations(
    connection: &rusqlite::Connection,
    tenant_id: &str,
    reservation_id: &str,
) -> rusqlite::Result<Vec<AllocationProjection>> {
    let mut statement = connection.prepare(
        "SELECT id, reservation_id, order_id, device_serial_no, model_id,
                start_date, end_date, status, allocated_at, released_at
         FROM allocations WHERE tenant_id = ?1 AND reservation_id = ?2 ORDER BY allocated_at, id",
    )?;
    statement
        .query_map(params![tenant_id, reservation_id], allocation_from_row)?
        .collect()
}

fn allocation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AllocationProjection> {
    let raw_id: String = row.get(0)?;
    let raw_reservation: String = row.get(1)?;
    Ok(AllocationProjection {
        id: AllocationId::parse(raw_id).map_err(|_| rusqlite::Error::InvalidQuery)?,
        reservation_id: ReservationId::parse(raw_reservation)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        order_id: row.get(2)?,
        device_serial_no: row.get(3)?,
        model_id: row.get(4)?,
        start_date: row.get(5)?,
        end_date: row.get(6)?,
        status: row.get(7)?,
        allocated_at: row.get(8)?,
        released_at: row.get(9)?,
    })
}

fn ensure_valid_half_open_interval(
    start_date: &str,
    end_date: &str,
) -> Result<(), RepositoryError> {
    let start = chrono::NaiveDate::parse_from_str(start_date.trim(), "%Y-%m-%d")
        .map_err(|_| RepositoryError::ContractViolation("startDate must be YYYY-MM-DD".into()))?;
    let end = chrono::NaiveDate::parse_from_str(end_date.trim(), "%Y-%m-%d")
        .map_err(|_| RepositoryError::ContractViolation("endDate must be YYYY-MM-DD".into()))?;
    if start >= end {
        return Err(RepositoryError::ContractViolation(
            "reservation interval must satisfy startDate < endDate for [start,end)".into(),
        ));
    }
    Ok(())
}

fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

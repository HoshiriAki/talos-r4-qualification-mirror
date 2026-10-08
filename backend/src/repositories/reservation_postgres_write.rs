#![cfg(feature = "postgres")]

use chrono::{Duration, NaiveDate, SecondsFormat, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::domain::{AllocationId, ReservationId, ReservationRequirementId};
use crate::repositories::RepositoryError;
use crate::repositories::reservation::{AllocationProjection, DeviceAllocationRequest};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresReservationWriteRepository<'a> {
    session: &'a RepositorySession,
}

#[derive(Debug, Clone)]
struct OrderDemand {
    start_date: String,
    end_date: String,
    requirements: Vec<(String, u32)>,
}

#[derive(Debug, Clone)]
struct AllocationReservation {
    order_id: Option<String>,
    status: String,
    start_date: String,
    end_date: String,
}

impl<'a> PostgresReservationWriteRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn create_from_order(
        &self,
        id: ReservationId,
        order_id: &str,
        hold_minutes: i64,
    ) -> Result<(), RepositoryError> {
        let order_id = order_id.trim().to_owned();
        if order_id.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "orderId must not be blank".into(),
            ));
        }

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let reservation_id = id.as_str().to_owned();
        let now = utc_now();
        let expires_at = (Utc::now() + Duration::minutes(hold_minutes.clamp(1, 1440)))
            .to_rfc3339_opts(SecondsFormat::Secs, true);

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let demand = load_order_demand_pg(connection, &tenant_id, &order_id).await?;
                    ensure_valid_half_open_interval(&demand.start_date, &demand.end_date)?;
                    ensure_capacity_pg(
                        connection,
                        &tenant_id,
                        &demand.start_date,
                        &demand.end_date,
                        &demand.requirements,
                        &now,
                    )
                    .await?;

                    sqlx::query(
                        "INSERT INTO rental_reservations \
                         (id, tenant_id, order_id, source_kind, status, start_date, end_date, \
                          expires_at, version, created_at, updated_at) \
                         VALUES ($1,$2,$3,'order','hold',$4,$5,$6,1,$7,$7)",
                    )
                    .bind(&reservation_id)
                    .bind(&tenant_id)
                    .bind(&order_id)
                    .bind(&demand.start_date)
                    .bind(&demand.end_date)
                    .bind(&expires_at)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    insert_requirements_pg(
                        connection,
                        &tenant_id,
                        &reservation_id,
                        &demand.requirements,
                        &now,
                    )
                    .await?;
                    Ok(())
                })
            })
    }

    pub(in crate::repositories) fn create_legacy_device_hold(
        &self,
        id: ReservationId,
        device_serial_no: &str,
        start_date: &str,
        end_date: &str,
        hold_minutes: i64,
    ) -> Result<(), RepositoryError> {
        ensure_valid_half_open_interval(start_date, end_date)?;
        let serial = device_serial_no.trim().to_owned();
        if serial.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "deviceSerialNo must not be blank".into(),
            ));
        }

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let reservation_id = id.as_str().to_owned();
        let start_date = start_date.trim().to_owned();
        let end_date = end_date.trim().to_owned();
        let now = utc_now();
        let expires_at = (Utc::now() + Duration::minutes(hold_minutes.clamp(1, 1440)))
            .to_rfc3339_opts(SecondsFormat::Secs, true);

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let model_id = sqlx::query_scalar::<_, String>(
                        "SELECT COALESCE(modelid, '') FROM devices \
                         WHERE tenant_id = $1 AND serialno = $2 FOR SHARE",
                    )
                    .bind(&tenant_id)
                    .bind(&serial)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .ok_or_else(|| {
                        RepositoryError::ContractViolation(
                            "legacy booking device does not exist in active tenant".into(),
                        )
                    })?;
                    if model_id.trim().is_empty() {
                        return Err(RepositoryError::ContractViolation(
                            "legacy booking device has no modelId".into(),
                        ));
                    }

                    let requirements = vec![(model_id.clone(), 1_u32)];
                    ensure_capacity_pg(
                        connection,
                        &tenant_id,
                        &start_date,
                        &end_date,
                        &requirements,
                        &now,
                    )
                    .await?;
                    ensure_device_unallocated_pg(
                        connection,
                        &tenant_id,
                        &serial,
                        &start_date,
                        &end_date,
                    )
                    .await?;

                    sqlx::query(
                        "INSERT INTO rental_reservations \
                         (id, tenant_id, order_id, source_kind, status, start_date, end_date, \
                          expires_at, version, created_at, updated_at) \
                         VALUES ($1,$2,NULL,'legacy_booking','hold',$3,$4,$5,1,$6,$6)",
                    )
                    .bind(&reservation_id)
                    .bind(&tenant_id)
                    .bind(&start_date)
                    .bind(&end_date)
                    .bind(&expires_at)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    insert_requirements_pg(
                        connection,
                        &tenant_id,
                        &reservation_id,
                        &requirements,
                        &now,
                    )
                    .await?;
                    Ok(())
                })
            })
    }

    pub(in crate::repositories) fn confirm(
        &self,
        id: &ReservationId,
        attach_order_id: Option<&str>,
    ) -> Result<(), RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let reservation_id = id.as_str().to_owned();
        let attach_order_id = attach_order_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let now = utc_now();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let row = sqlx::query(
                        "SELECT status, order_id, start_date, end_date, expires_at \
                         FROM rental_reservations \
                         WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
                    )
                    .bind(&tenant_id)
                    .bind(&reservation_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .ok_or_else(|| {
                        RepositoryError::ContractViolation("reservation not found".into())
                    })?;

                    let status: String = row.try_get("status").map_err(pg_error)?;
                    let mut order_id: Option<String> = row.try_get("order_id").map_err(pg_error)?;
                    let start_date: String = row.try_get("start_date").map_err(pg_error)?;
                    let end_date: String = row.try_get("end_date").map_err(pg_error)?;
                    let expires_at: String = row.try_get("expires_at").map_err(pg_error)?;

                    if status != "hold" {
                        return Err(RepositoryError::ContractViolation(
                            "only hold reservations can be confirmed".into(),
                        ));
                    }
                    if expires_at <= now {
                        return Err(RepositoryError::ContractViolation(
                            "expired hold cannot be confirmed".into(),
                        ));
                    }

                    if let Some(candidate) = attach_order_id.as_deref() {
                        let demand = load_order_demand_pg(connection, &tenant_id, candidate).await?;
                        if demand.start_date != start_date || demand.end_date != end_date {
                            return Err(RepositoryError::ContractViolation(
                                "attached order interval must match reservation exactly".into(),
                            ));
                        }
                        ensure_order_covers_requirements_pg(
                            connection,
                            &tenant_id,
                            &reservation_id,
                            &demand.requirements,
                        )
                        .await?;
                        order_id = Some(candidate.to_owned());
                    }

                    let order_id = order_id.ok_or_else(|| {
                        RepositoryError::ContractViolation(
                            "reservation must be attached to an order before confirmation".into(),
                        )
                    })?;

                    sqlx::query(
                        "UPDATE rental_reservations \
                         SET order_id = $3, status = 'confirmed', version = version + 1, updated_at = $4 \
                         WHERE tenant_id = $1 AND id = $2",
                    )
                    .bind(&tenant_id)
                    .bind(&reservation_id)
                    .bind(&order_id)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    append_outbox_pg(
                        connection,
                        &tenant_id,
                        "reservation",
                        &reservation_id,
                        "ReservationConfirmed",
                        &format!("reservation:{reservation_id}:confirmed"),
                        &serde_json::json!({
                            "reservationId": reservation_id,
                            "orderId": order_id
                        }),
                        &now,
                    )
                    .await?;
                    Ok(())
                })
            })
    }

    pub(in crate::repositories) fn expire_due(&self) -> Result<u64, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        "UPDATE rental_reservations \
                         SET status = 'expired', version = version + 1, updated_at = $2 \
                         WHERE tenant_id = $1 AND status = 'hold' AND expires_at <= $2",
                    )
                    .bind(&tenant_id)
                    .bind(&now)
                    .execute(connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();
                    Ok(changed)
                })
            })
    }

    pub(in crate::repositories) fn allocate_device(
        &self,
        reservation_id: &ReservationId,
        allocation_id: AllocationId,
        device_serial_no: &str,
    ) -> Result<AllocationProjection, RepositoryError> {
        let serial = device_serial_no.trim().to_owned();
        if serial.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "deviceSerialNo must not be blank".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let reservation_id = reservation_id.clone();
        let now = utc_now();
        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    allocate_device_pg(
                        connection,
                        &tenant_id,
                        &reservation_id,
                        &allocation_id,
                        &serial,
                        &now,
                    )
                    .await
                })
            })
    }

    pub(in crate::repositories) fn allocate_devices_batch(
        &self,
        requests: &[DeviceAllocationRequest],
    ) -> Result<Vec<AllocationProjection>, RepositoryError> {
        if requests.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "device allocation batch must contain at least one row".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let requests = requests.to_vec();
        let now = utc_now();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let mut allocations = Vec::with_capacity(requests.len());
                    for request in requests {
                        let order_id = request.order_id.trim();
                        let serial = request.device_serial_no.trim();
                        if order_id.is_empty() || serial.is_empty() {
                            return Err(RepositoryError::ContractViolation(
                                "each device allocation row requires orderId and deviceSerialNo"
                                    .into(),
                            ));
                        }

                        let reservation_raw = sqlx::query_scalar::<_, String>(
                            "SELECT id FROM rental_reservations \
                             WHERE tenant_id = $1 AND order_id = $2 \
                             ORDER BY created_at DESC LIMIT 1 FOR UPDATE",
                        )
                        .bind(&tenant_id)
                        .bind(order_id)
                        .fetch_optional(&mut *connection)
                        .await
                        .map_err(pg_error)?
                        .ok_or_else(|| {
                            RepositoryError::ContractViolation(
                                "canonical reservation not found for Order".into(),
                            )
                        })?;
                        let reservation_id =
                            ReservationId::parse(reservation_raw).map_err(|_| {
                                RepositoryError::ContractViolation(
                                    "canonical reservation id is invalid".into(),
                                )
                            })?;

                        allocations.push(
                            allocate_device_pg(
                                connection,
                                &tenant_id,
                                &reservation_id,
                                &AllocationId::new(),
                                serial,
                                &now,
                            )
                            .await?,
                        );
                    }
                    Ok(allocations)
                })
            })
    }

    pub(in crate::repositories) fn release_allocation(
        &self,
        id: &AllocationId,
    ) -> Result<AllocationProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let allocation_id = id.as_str().to_owned();
        let now = utc_now();

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let row = sqlx::query(
                        "SELECT id, reservation_id, order_id, device_serial_no, model_id, \
                                start_date, end_date, status, allocated_at, released_at \
                         FROM allocations WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
                    )
                    .bind(&tenant_id)
                    .bind(&allocation_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .ok_or_else(|| {
                        RepositoryError::ContractViolation("active allocation not found".into())
                    })?;

                    let status: String = row.try_get("status").map_err(pg_error)?;
                    if status != "allocated" {
                        return Err(RepositoryError::ContractViolation(
                            "active allocation not found".into(),
                        ));
                    }

                    sqlx::query(
                        "UPDATE allocations SET status = 'released', released_at = $3 \
                         WHERE tenant_id = $1 AND id = $2 AND status = 'allocated'",
                    )
                    .bind(&tenant_id)
                    .bind(&allocation_id)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    project_allocation_row(row, "released", Some(now))
                })
            })
    }
}

async fn allocate_device_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    reservation_id: &ReservationId,
    allocation_id: &AllocationId,
    serial: &str,
    now: &str,
) -> Result<AllocationProjection, RepositoryError> {
    let row = sqlx::query(
        "SELECT order_id, status, start_date, end_date \
         FROM rental_reservations \
         WHERE tenant_id = $1 AND id = $2 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(reservation_id.as_str())
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| RepositoryError::ContractViolation("reservation not found".into()))?;

    let reservation = AllocationReservation {
        order_id: row.try_get("order_id").map_err(pg_error)?,
        status: row.try_get("status").map_err(pg_error)?,
        start_date: row.try_get("start_date").map_err(pg_error)?,
        end_date: row.try_get("end_date").map_err(pg_error)?,
    };
    if reservation.status != "confirmed" {
        return Err(RepositoryError::ContractViolation(
            "device allocation requires confirmed reservation".into(),
        ));
    }

    let model_id = sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(modelid, '') FROM devices \
         WHERE tenant_id = $1 AND serialno = $2 FOR SHARE",
    )
    .bind(tenant_id)
    .bind(serial)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| RepositoryError::ContractViolation("device not found".into()))?;

    let overlap = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM allocations \
         WHERE tenant_id = $1 AND device_serial_no = $2 AND status = 'allocated' \
           AND $3 < end_date AND start_date < $4)",
    )
    .bind(tenant_id)
    .bind(serial)
    .bind(&reservation.start_date)
    .bind(&reservation.end_date)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;
    if overlap {
        return Err(RepositoryError::ContractViolation(
            "device already allocated in overlapping interval".into(),
        ));
    }

    let required = sqlx::query_scalar::<_, i64>(
        "SELECT quantity FROM reservation_requirements \
         WHERE tenant_id = $1 AND reservation_id = $2 AND model_id = $3",
    )
    .bind(tenant_id)
    .bind(reservation_id.as_str())
    .bind(&model_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| {
        RepositoryError::ContractViolation("allocation model is not required by reservation".into())
    })?;

    let allocated_for_model = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM allocations \
         WHERE tenant_id = $1 AND reservation_id = $2 AND model_id = $3 AND status = 'allocated'",
    )
    .bind(tenant_id)
    .bind(reservation_id.as_str())
    .bind(&model_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;
    if allocated_for_model >= required {
        return Err(RepositoryError::ContractViolation(
            "allocation exceeds reservation model requirement".into(),
        ));
    }

    sqlx::query(
        "INSERT INTO allocations \
         (id, tenant_id, reservation_id, order_id, device_serial_no, model_id, \
          start_date, end_date, status, allocated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'allocated',$9)",
    )
    .bind(allocation_id.as_str())
    .bind(tenant_id)
    .bind(reservation_id.as_str())
    .bind(reservation.order_id.as_deref())
    .bind(serial)
    .bind(&model_id)
    .bind(&reservation.start_date)
    .bind(&reservation.end_date)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?;

    let required_total = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(quantity), 0)::bigint FROM reservation_requirements \
         WHERE tenant_id = $1 AND reservation_id = $2",
    )
    .bind(tenant_id)
    .bind(reservation_id.as_str())
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;
    let allocated_total = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM allocations \
         WHERE tenant_id = $1 AND reservation_id = $2 AND status = 'allocated'",
    )
    .bind(tenant_id)
    .bind(reservation_id.as_str())
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;

    if required_total > 0 && allocated_total >= required_total {
        if let Some(order_id) = reservation.order_id.as_deref() {
            append_outbox_pg(
                connection,
                tenant_id,
                "allocation",
                reservation_id.as_str(),
                "AllocationComplete",
                &format!(
                    "reservation:{}:allocation-complete",
                    reservation_id.as_str()
                ),
                &serde_json::json!({
                    "reservationId": reservation_id.as_str(),
                    "orderId": order_id
                }),
                now,
            )
            .await?;
        }
    }

    Ok(AllocationProjection {
        id: allocation_id.clone(),
        reservation_id: reservation_id.clone(),
        order_id: reservation.order_id,
        device_serial_no: serial.to_owned(),
        model_id,
        start_date: reservation.start_date,
        end_date: reservation.end_date,
        status: "allocated".into(),
        allocated_at: now.to_owned(),
        released_at: None,
    })
}

async fn load_order_demand_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    order_id: &str,
) -> Result<OrderDemand, RepositoryError> {
    let row = sqlx::query(
        "SELECT startdate, enddate FROM orders \
         WHERE tenant_id = $1 AND id = $2 FOR SHARE",
    )
    .bind(tenant_id)
    .bind(order_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| RepositoryError::ContractViolation("order not found in active tenant".into()))?;
    let start_date: String = row.try_get("startdate").map_err(pg_error)?;
    let end_date: String = row.try_get("enddate").map_err(pg_error)?;

    let rows = sqlx::query(
        "SELECT reference_id, SUM(quantity)::bigint AS quantity \
         FROM order_lines \
         WHERE tenant_id = $1 AND order_id = $2 AND line_kind = 'model' \
         GROUP BY reference_id ORDER BY reference_id",
    )
    .bind(tenant_id)
    .bind(order_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(pg_error)?;

    let mut requirements = Vec::with_capacity(rows.len());
    for row in rows {
        let model_id: String = row.try_get("reference_id").map_err(pg_error)?;
        let quantity: i64 = row.try_get("quantity").map_err(pg_error)?;
        let quantity = u32::try_from(quantity).map_err(|_| {
            RepositoryError::ContractViolation("order model quantity is out of range".into())
        })?;
        requirements.push((model_id, quantity));
    }
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

async fn ensure_capacity_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    start_date: &str,
    end_date: &str,
    requirements: &[(String, u32)],
    now: &str,
) -> Result<(), RepositoryError> {
    for (model_id, quantity) in requirements {
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::bigint FROM devices \
             WHERE tenant_id = $1 AND COALESCE(modelid, '') = $2 \
               AND COALESCE(rentalstatus, '') NOT IN ('scrapped', 'lost')",
        )
        .bind(tenant_id)
        .bind(model_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(pg_error)?;
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
        .bind(tenant_id)
        .bind(model_id)
        .bind(start_date)
        .bind(end_date)
        .bind(now)
        .fetch_one(&mut *connection)
        .await
        .map_err(pg_error)?;

        if total.saturating_sub(reserved) < i64::from(*quantity) {
            return Err(RepositoryError::ContractViolation(format!(
                "insufficient model capacity for {model_id}: requested {quantity}, available {}",
                total.saturating_sub(reserved).max(0)
            )));
        }
    }
    Ok(())
}

async fn ensure_device_unallocated_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    serial: &str,
    start_date: &str,
    end_date: &str,
) -> Result<(), RepositoryError> {
    let overlap = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM allocations \
         WHERE tenant_id = $1 AND device_serial_no = $2 AND status = 'allocated' \
           AND $3 < end_date AND start_date < $4)",
    )
    .bind(tenant_id)
    .bind(serial)
    .bind(start_date)
    .bind(end_date)
    .fetch_one(connection)
    .await
    .map_err(pg_error)?;
    if overlap {
        return Err(RepositoryError::ContractViolation(
            "device already allocated in overlapping interval".into(),
        ));
    }
    Ok(())
}

async fn insert_requirements_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    reservation_id: &str,
    requirements: &[(String, u32)],
    now: &str,
) -> Result<(), RepositoryError> {
    for (model_id, quantity) in requirements {
        sqlx::query(
            "INSERT INTO reservation_requirements \
             (id, tenant_id, reservation_id, model_id, quantity, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(ReservationRequirementId::new().as_str().to_owned())
        .bind(tenant_id)
        .bind(reservation_id)
        .bind(model_id)
        .bind(i64::from(*quantity))
        .bind(now)
        .execute(&mut *connection)
        .await
        .map_err(pg_error)?;
    }
    Ok(())
}

async fn ensure_order_covers_requirements_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    reservation_id: &str,
    order_requirements: &[(String, u32)],
) -> Result<(), RepositoryError> {
    let rows = sqlx::query(
        "SELECT model_id, quantity FROM reservation_requirements \
         WHERE tenant_id = $1 AND reservation_id = $2 ORDER BY model_id",
    )
    .bind(tenant_id)
    .bind(reservation_id)
    .fetch_all(connection)
    .await
    .map_err(pg_error)?;

    for row in rows {
        let model_id: String = row.try_get("model_id").map_err(pg_error)?;
        let quantity: i64 = row.try_get("quantity").map_err(pg_error)?;
        let quantity = u32::try_from(quantity).map_err(|_| {
            RepositoryError::ContractViolation(
                "persisted reservation requirement quantity is out of range".into(),
            )
        })?;
        let available = order_requirements
            .iter()
            .find(|(candidate, _)| candidate == &model_id)
            .map(|(_, quantity)| *quantity)
            .unwrap_or(0);
        if available < quantity {
            return Err(RepositoryError::ContractViolation(
                "attached order does not cover legacy reservation model demand".into(),
            ));
        }
    }
    Ok(())
}

async fn append_outbox_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    source_kind: &str,
    source_id: &str,
    message_type: &str,
    idempotency_key: &str,
    payload: &serde_json::Value,
    now: &str,
) -> Result<(), RepositoryError> {
    sqlx::query(
        "INSERT INTO domain_outbox \
         (id, tenant_id, source_kind, source_id, message_type, idempotency_key, \
          payload_json, payload_version, state, available_at, created_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,1,'pending',$8,$8) \
         ON CONFLICT (tenant_id, idempotency_key) DO NOTHING",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(tenant_id)
    .bind(source_kind)
    .bind(source_id)
    .bind(message_type)
    .bind(idempotency_key)
    .bind(payload.to_string())
    .bind(now)
    .execute(connection)
    .await
    .map_err(pg_error)?;
    Ok(())
}

fn project_allocation_row(
    row: sqlx::postgres::PgRow,
    status: &str,
    released_at: Option<String>,
) -> Result<AllocationProjection, RepositoryError> {
    let raw_id: String = row.try_get("id").map_err(pg_error)?;
    let raw_reservation: String = row.try_get("reservation_id").map_err(pg_error)?;
    Ok(AllocationProjection {
        id: AllocationId::parse(raw_id).map_err(|_| {
            RepositoryError::ContractViolation("persisted allocation id is not a UUID".into())
        })?,
        reservation_id: ReservationId::parse(raw_reservation).map_err(|_| {
            RepositoryError::ContractViolation(
                "persisted allocation reservation id is not a UUID".into(),
            )
        })?,
        order_id: row.try_get("order_id").map_err(pg_error)?,
        device_serial_no: row.try_get("device_serial_no").map_err(pg_error)?,
        model_id: row.try_get("model_id").map_err(pg_error)?,
        start_date: row.try_get("start_date").map_err(pg_error)?,
        end_date: row.try_get("end_date").map_err(pg_error)?,
        status: status.to_owned(),
        allocated_at: row.try_get("allocated_at").map_err(pg_error)?,
        released_at,
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

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

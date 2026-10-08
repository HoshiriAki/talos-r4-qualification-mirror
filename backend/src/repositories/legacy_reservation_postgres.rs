#![cfg(feature = "postgres")]

use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::legacy_reservation::{
    LegacyReservationConflict, LegacyReservationList, LegacyReservationProjection,
    LegacyReservationRule, LegacyReservationRulePatch,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresLegacyReservationRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresLegacyReservationRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
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
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let status = nonempty(status);
        let device_serial_no = nonempty(device_serial_no);
        let customer_phone = nonempty(customer_phone);
        let warehouse_id = warehouse_id.map(|value| value.to_string());
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);

        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let offset = (page - 1) * page_size;
                let mut count = QueryBuilder::<Postgres>::new(
                    "SELECT COUNT(*)::bigint FROM inventory_reservations WHERE ",
                );
                push_filters(
                    &mut count,
                    &tenant_id,
                    status.as_deref(),
                    device_serial_no.as_deref(),
                    customer_phone.as_deref(),
                    warehouse_id.as_deref(),
                );
                let total: i64 = count
                    .build_query_scalar()
                    .fetch_one(&mut *connection)
                    .await?;

                let mut data = QueryBuilder::<Postgres>::new(
                    "SELECT id::bigint AS id,device_serial_no,warehouse_id::text AS warehouse_id,
                            order_id::text AS order_id,customer_name,customer_phone,
                            start_date,end_date,status,notes,reserved_by::text AS reserved_by,
                            created_at,updated_at
                     FROM inventory_reservations WHERE ",
                );
                push_filters(
                    &mut data,
                    &tenant_id,
                    status.as_deref(),
                    device_serial_no.as_deref(),
                    customer_phone.as_deref(),
                    warehouse_id.as_deref(),
                );
                data.push(" ORDER BY created_at DESC LIMIT ")
                    .push_bind(page_size)
                    .push(" OFFSET ")
                    .push_bind(offset);

                let rows = data.build().fetch_all(&mut *connection).await?;
                let items = rows
                    .iter()
                    .map(map_projection)
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(LegacyReservationList {
                    items,
                    total,
                    page,
                    page_size,
                })
            })
        })
    }

    pub(in crate::repositories) fn get(
        &self,
        id: i64,
    ) -> Result<Option<LegacyReservationProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT id::bigint AS id,device_serial_no,warehouse_id::text AS warehouse_id,
                            order_id::text AS order_id,customer_name,customer_phone,
                            start_date,end_date,status,notes,reserved_by::text AS reserved_by,
                            created_at,updated_at
                     FROM inventory_reservations
                     WHERE tenant_id=$1 AND id=$2 LIMIT 1",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await?
                .map(|row| map_projection(&row))
                .transpose()
            })
        })
    }

    pub(in crate::repositories) fn conflicts(
        &self,
        device_serial_no: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<LegacyReservationConflict>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial = device_serial_no.to_owned();
        let start = start_date.to_owned();
        let end = end_date.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    "SELECT id::bigint AS id,customer_name,customer_phone,start_date,end_date,status
                     FROM inventory_reservations
                     WHERE tenant_id=$1 AND device_serial_no=$2
                       AND status IN ('reserved','confirmed')
                       AND $3 < end_date AND start_date < $4
                     ORDER BY created_at,id",
                )
                .bind(&tenant_id)
                .bind(&serial)
                .bind(&start)
                .bind(&end)
                .fetch_all(&mut *connection)
                .await?;
                rows.iter()
                    .map(|row| {
                        Ok(LegacyReservationConflict {
                            id: row.try_get("id")?,
                            customer_name: row.try_get("customer_name")?,
                            customer_phone: row.try_get("customer_phone")?,
                            start_date: row.try_get("start_date")?,
                            end_date: row.try_get("end_date")?,
                            status: row.try_get("status")?,
                        })
                    })
                    .collect::<Result<Vec<_>, sqlx::Error>>()
            })
        })
    }

    pub(in crate::repositories) fn get_rule(
        &self,
    ) -> Result<Option<LegacyReservationRule>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT id::bigint AS id,rule_name,
                            max_days_ahead::bigint AS max_days_ahead,
                            max_concurrent_per_customer::bigint AS max_concurrent_per_customer,
                            auto_release_minutes::bigint AS auto_release_minutes,is_active,
                            created_at,updated_at
                     FROM reservation_rules
                     WHERE tenant_id=$1
                     ORDER BY is_active DESC,id LIMIT 1",
                )
                .bind(&tenant_id)
                .fetch_optional(&mut *connection)
                .await?;
                row.map(|row| map_rule(&row)).transpose()
            })
        })
    }

    pub(in crate::repositories) fn update_rule(
        &self,
        patch: &LegacyReservationRulePatch,
        now: &str,
    ) -> Result<LegacyReservationRule, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let patch = patch.clone();
        let now = now.to_owned();
        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        "INSERT INTO reservation_rules
                     (rule_name,max_days_ahead,max_concurrent_per_customer,
                      auto_release_minutes,is_active,created_at,updated_at,tenant_id)
                     VALUES ('default',90,2,30,1,$1,$1,$2)
                     ON CONFLICT (tenant_id) DO NOTHING",
                    )
                    .bind(&now)
                    .bind(&tenant_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let current = sqlx::query(
                        "SELECT id::bigint AS id,rule_name,
                            max_days_ahead::bigint AS max_days_ahead,
                            max_concurrent_per_customer::bigint AS max_concurrent_per_customer,
                            auto_release_minutes::bigint AS auto_release_minutes,is_active,
                            created_at,updated_at
                     FROM reservation_rules
                     WHERE tenant_id=$1
                     ORDER BY is_active DESC,id LIMIT 1
                     FOR UPDATE",
                    )
                    .bind(&tenant_id)
                    .fetch_one(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    let current = map_rule(&current).map_err(pg_error)?;

                    let max_days_ahead = patch.max_days_ahead.unwrap_or(current.max_days_ahead);
                    let max_concurrent = patch
                        .max_concurrent_per_customer
                        .unwrap_or(current.max_concurrent_per_customer);
                    let auto_release = patch
                        .auto_release_minutes
                        .unwrap_or(current.auto_release_minutes);
                    let is_active = patch.is_active.unwrap_or(current.is_active);

                    sqlx::query(
                        "UPDATE reservation_rules
                     SET max_days_ahead=$1,max_concurrent_per_customer=$2,
                         auto_release_minutes=$3,is_active=$4,updated_at=$5
                     WHERE tenant_id=$6 AND id=$7",
                    )
                    .bind(max_days_ahead)
                    .bind(max_concurrent)
                    .bind(auto_release)
                    .bind(if is_active { 1_i32 } else { 0_i32 })
                    .bind(&now)
                    .bind(&tenant_id)
                    .bind(current.id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    load_rule_pg(connection, &tenant_id).await
                })
            })
    }
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn push_filters<'args>(
    query: &mut QueryBuilder<'args, Postgres>,
    tenant_id: &'args str,
    status: Option<&'args str>,
    device_serial_no: Option<&'args str>,
    customer_phone: Option<&'args str>,
    warehouse_id: Option<&'args str>,
) {
    query.push("tenant_id=").push_bind(tenant_id);
    if let Some(status) = status {
        query.push(" AND status=").push_bind(status);
    }
    if let Some(serial) = device_serial_no {
        query.push(" AND device_serial_no=").push_bind(serial);
    }
    if let Some(phone) = customer_phone {
        query.push(" AND customer_phone=").push_bind(phone);
    }
    if let Some(warehouse_id) = warehouse_id {
        query
            .push(" AND warehouse_id::text=")
            .push_bind(warehouse_id);
    }
}

async fn load_rule_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
) -> Result<LegacyReservationRule, RepositoryError> {
    let row = sqlx::query(
        "SELECT id::bigint AS id,rule_name,max_days_ahead::bigint AS max_days_ahead,
                max_concurrent_per_customer::bigint AS max_concurrent_per_customer,
                auto_release_minutes::bigint AS auto_release_minutes,is_active,
                created_at,updated_at
         FROM reservation_rules
         WHERE tenant_id=$1
         ORDER BY is_active DESC,id LIMIT 1",
    )
    .bind(tenant_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(pg_error)?;
    map_rule(&row).map_err(pg_error)
}

fn map_projection(row: &sqlx::postgres::PgRow) -> Result<LegacyReservationProjection, sqlx::Error> {
    Ok(LegacyReservationProjection {
        id: row.try_get("id")?,
        device_serial_no: row.try_get("device_serial_no")?,
        warehouse_id: row.try_get("warehouse_id")?,
        order_id: row.try_get("order_id")?,
        customer_name: row.try_get("customer_name")?,
        customer_phone: row.try_get("customer_phone")?,
        start_date: row.try_get("start_date")?,
        end_date: row.try_get("end_date")?,
        status: row.try_get("status")?,
        notes: row.try_get("notes")?,
        reserved_by: row.try_get("reserved_by")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_rule(row: &sqlx::postgres::PgRow) -> Result<LegacyReservationRule, sqlx::Error> {
    Ok(LegacyReservationRule {
        id: row.try_get("id")?,
        rule_name: row.try_get("rule_name")?,
        max_days_ahead: row.try_get("max_days_ahead")?,
        max_concurrent_per_customer: row.try_get("max_concurrent_per_customer")?,
        auto_release_minutes: row.try_get("auto_release_minutes")?,
        is_active: row.try_get::<i32, _>("is_active")? != 0,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

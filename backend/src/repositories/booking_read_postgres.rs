#![cfg(feature = "postgres")]

use std::collections::HashMap;

use chrono::NaiveDate;
use sqlx::{Postgres, QueryBuilder, Row};

use crate::repositories::RepositoryError;
use crate::repositories::booking_read::{
    BookingAvailabilityProjection, BookingDeviceSummary, BookingPriceProjection, BookingSearchItem,
    build_availability, round_money,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresBookingReadRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresBookingReadRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn availability(
        &self,
        device_serial_no: Option<&str>,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<BookingAvailabilityProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial = device_serial_no.map(str::to_owned);
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let devices = if let Some(serial) = serial.as_deref() {
                    sqlx::query(
                        "SELECT serialno,COALESCE(modelid,'') AS model_id
                         FROM devices
                         WHERE tenant_id=$1 AND serialno=$2
                         ORDER BY serialno",
                    )
                    .bind(&tenant_id)
                    .bind(serial)
                    .fetch_all(&mut *connection)
                    .await?
                } else {
                    sqlx::query(
                        "SELECT serialno,COALESCE(modelid,'') AS model_id
                         FROM devices
                         WHERE tenant_id=$1
                         ORDER BY serialno",
                    )
                    .bind(&tenant_id)
                    .fetch_all(&mut *connection)
                    .await?
                }
                .into_iter()
                .map(|row| {
                    Ok(BookingDeviceSummary {
                        serial_no: row.try_get("serialno")?,
                        model: row.try_get("model_id")?,
                    })
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()?;

                let start = start_date.format("%Y-%m-%d").to_string();
                let end = end_date.format("%Y-%m-%d").to_string();
                let rows = sqlx::query(
                    "SELECT date,device_serial_no,is_available
                     FROM booking_availability
                     WHERE tenant_id=$1 AND date>=$2 AND date<=$3",
                )
                .bind(&tenant_id)
                .bind(&start)
                .bind(&end)
                .fetch_all(&mut *connection)
                .await?;
                let overrides = rows
                    .into_iter()
                    .map(|row| {
                        Ok((
                            (
                                row.try_get::<String, _>("date")?,
                                row.try_get::<String, _>("device_serial_no")?,
                            ),
                            row.try_get::<i32, _>("is_available")? == 1,
                        ))
                    })
                    .collect::<Result<HashMap<_, _>, sqlx::Error>>()?;

                Ok(build_availability(devices, overrides, start_date, end_date))
            })
        })
    }

    pub(in crate::repositories) fn price(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<BookingPriceProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let serial = device_serial_no.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT COALESCE(m.name,'') AS model_name,
                            COALESCE(bp.weekdayprice::double precision,8.5) AS weekday_price,
                            COALESCE(bp.weekendprice::double precision,14.0) AS weekend_price
                     FROM devices d
                     LEFT JOIN device_models m
                       ON d.modelid=m.id AND m.tenant_id=d.tenant_id
                     LEFT JOIN model_base_prices bp
                       ON d.modelid=bp.modelid AND bp.tenant_id=d.tenant_id
                     WHERE d.tenant_id=$1 AND d.serialno=$2
                     LIMIT 1",
                )
                .bind(&tenant_id)
                .bind(&serial)
                .fetch_optional(&mut *connection)
                .await?;
                row.map(|row| {
                    Ok(BookingPriceProjection {
                        model_name: row.try_get("model_name")?,
                        weekday_price: row.try_get("weekday_price")?,
                        weekend_price: row.try_get("weekend_price")?,
                    })
                })
                .transpose()
            })
        })
    }

    pub(in crate::repositories) fn search(
        &self,
        query: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<BookingSearchItem>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let query = query.filter(|value| !value.is_empty()).map(str::to_owned);
        let model = model.filter(|value| !value.is_empty()).map(str::to_owned);
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let mut builder = QueryBuilder::<Postgres>::new(
                    "SELECT d.serialno,COALESCE(d.modelid,'') AS model_id,
                            COALESCE(m.name,'') AS model_name,
                            COALESCE(bp.weekdayprice::double precision,8.5) AS weekday_price,
                            COALESCE(bp.weekendprice::double precision,14.0) AS weekend_price
                     FROM devices d
                     LEFT JOIN device_models m
                       ON d.modelid=m.id AND m.tenant_id=d.tenant_id
                     LEFT JOIN model_base_prices bp
                       ON d.modelid=bp.modelid AND bp.tenant_id=d.tenant_id
                     WHERE d.tenant_id=",
                );
                builder.push_bind(&tenant_id);
                if let Some(query) = query {
                    let pattern = format!("%{query}%");
                    builder
                        .push(" AND (d.serialno LIKE ")
                        .push_bind(pattern.clone())
                        .push(" OR m.name LIKE ")
                        .push_bind(pattern.clone())
                        .push(" OR COALESCE(d.modelid,'') LIKE ")
                        .push_bind(pattern)
                        .push(")");
                }
                if let Some(model) = model {
                    builder
                        .push(" AND COALESCE(d.modelid,'')=")
                        .push_bind(model);
                }
                builder.push(" ORDER BY d.serialno");

                let rows = builder.build().fetch_all(&mut *connection).await?;
                rows.into_iter()
                    .map(|row| {
                        let weekday_price: f64 = row.try_get("weekday_price")?;
                        let weekend_price: f64 = row.try_get("weekend_price")?;
                        Ok(BookingSearchItem {
                            serial_no: row.try_get("serialno")?,
                            model_id: row.try_get("model_id")?,
                            model_name: row.try_get("model_name")?,
                            weekday_price,
                            weekend_price,
                            daily_rate: round_money((weekday_price + weekend_price) / 2.0),
                        })
                    })
                    .collect()
            })
        })
    }
}

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::params;
use serde::Serialize;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingAvailabilityDay {
    pub date: String,
    pub available: bool,
    pub device_serial_no: String,
    pub device_model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingDeviceSummary {
    pub serial_no: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BookingAvailabilityProjection {
    pub dates: Vec<BookingAvailabilityDay>,
    pub devices: Vec<BookingDeviceSummary>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BookingPriceProjection {
    pub model_name: String,
    pub weekday_price: f64,
    pub weekend_price: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookingSearchItem {
    pub serial_no: String,
    pub model_id: String,
    pub model_name: String,
    pub weekday_price: f64,
    pub weekend_price: f64,
    pub daily_rate: f64,
}

pub(in crate::repositories) struct SqliteBookingReadRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteBookingReadRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn availability(
        &self,
        device_serial_no: Option<&str>,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<BookingAvailabilityProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let serial = device_serial_no.map(str::to_owned);
        self.session.read(move |connection| {
            let devices = if let Some(serial) = serial.as_deref() {
                let mut statement = connection.prepare(
                    "SELECT serialNo,COALESCE(modelId,'') FROM devices
                     WHERE tenant_id=?1 AND serialNo=?2 ORDER BY serialNo",
                )?;
                statement
                    .query_map(params![tenant_id, serial], |row| {
                        Ok(BookingDeviceSummary {
                            serial_no: row.get(0)?,
                            model: row.get(1)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                let mut statement = connection.prepare(
                    "SELECT serialNo,COALESCE(modelId,'') FROM devices
                     WHERE tenant_id=?1 ORDER BY serialNo",
                )?;
                statement
                    .query_map(params![tenant_id], |row| {
                        Ok(BookingDeviceSummary {
                            serial_no: row.get(0)?,
                            model: row.get(1)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };

            let start = start_date.format("%Y-%m-%d").to_string();
            let end = end_date.format("%Y-%m-%d").to_string();
            let mut statement = connection.prepare(
                "SELECT date,device_serial_no,is_available FROM booking_availability
                 WHERE tenant_id=?1 AND date>=?2 AND date<=?3",
            )?;
            let overrides = statement
                .query_map(params![tenant_id, start, end], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i32>(2)? == 1,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|(date, serial, available)| ((date, serial), available))
                .collect::<HashMap<_, _>>();

            Ok(build_availability(devices, overrides, start_date, end_date))
        })
    }

    pub(in crate::repositories) fn price(
        &self,
        device_serial_no: &str,
    ) -> Result<Option<BookingPriceProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let serial = device_serial_no.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT COALESCE(m.name,''),bp.weekdayPrice,bp.weekendPrice
                 FROM devices d
                 LEFT JOIN device_models m
                   ON d.modelId=m.id AND m.tenant_id=d.tenant_id
                 LEFT JOIN model_base_prices bp
                   ON d.modelId=bp.modelId AND bp.tenant_id=d.tenant_id
                 WHERE d.tenant_id=?1 AND d.serialNo=?2 LIMIT 1",
            )?;
            let mut rows = statement.query(params![tenant_id, serial])?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            Ok(Some(BookingPriceProjection {
                model_name: row.get(0)?,
                weekday_price: row.get::<_, Option<f64>>(1)?.unwrap_or(8.5),
                weekend_price: row.get::<_, Option<f64>>(2)?.unwrap_or(14.0),
            }))
        })
    }

    pub(in crate::repositories) fn search(
        &self,
        query: Option<&str>,
        model: Option<&str>,
    ) -> Result<Vec<BookingSearchItem>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let query = query.filter(|value| !value.is_empty()).map(str::to_owned);
        let model = model.filter(|value| !value.is_empty()).map(str::to_owned);
        self.session.read(move |connection| {
            let mut sql = String::from(
                "SELECT d.serialNo,COALESCE(d.modelId,''),COALESCE(m.name,''),
                        COALESCE(bp.weekdayPrice,8.5),COALESCE(bp.weekendPrice,14.0)
                 FROM devices d
                 LEFT JOIN device_models m
                   ON d.modelId=m.id AND m.tenant_id=d.tenant_id
                 LEFT JOIN model_base_prices bp
                   ON d.modelId=bp.modelId AND bp.tenant_id=d.tenant_id
                 WHERE d.tenant_id=?1",
            );
            let mut values = vec![rusqlite::types::Value::Text(tenant_id)];
            if let Some(query) = query {
                sql.push_str(
                    " AND (d.serialNo LIKE ? OR m.name LIKE ? OR COALESCE(d.modelId,'') LIKE ?)",
                );
                let pattern = format!("%{query}%");
                values.extend([
                    rusqlite::types::Value::Text(pattern.clone()),
                    rusqlite::types::Value::Text(pattern.clone()),
                    rusqlite::types::Value::Text(pattern),
                ]);
            }
            if let Some(model) = model {
                sql.push_str(" AND COALESCE(d.modelId,'')=?");
                values.push(rusqlite::types::Value::Text(model));
            }
            sql.push_str(" ORDER BY d.serialNo");

            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(rusqlite::params_from_iter(values.iter()), map_search_item)?
                .collect()
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub(in crate::repositories) fn build_availability(
    devices: Vec<BookingDeviceSummary>,
    overrides: HashMap<(String, String), bool>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> BookingAvailabilityProjection {
    let mut dates = Vec::new();
    let mut current = start_date;
    while current <= end_date {
        let date = current.format("%Y-%m-%d").to_string();
        for device in &devices {
            let available = overrides
                .get(&(date.clone(), device.serial_no.clone()))
                .copied()
                .unwrap_or(true);
            dates.push(BookingAvailabilityDay {
                date: date.clone(),
                available,
                device_serial_no: device.serial_no.clone(),
                device_model: device.model.clone(),
            });
        }
        current += chrono::Duration::days(1);
    }
    BookingAvailabilityProjection { dates, devices }
}

fn map_search_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<BookingSearchItem> {
    let weekday_price: f64 = row.get(3)?;
    let weekend_price: f64 = row.get(4)?;
    Ok(BookingSearchItem {
        serial_no: row.get(0)?,
        model_id: row.get(1)?,
        model_name: row.get(2)?,
        weekday_price,
        weekend_price,
        daily_rate: round_money((weekday_price + weekend_price) / 2.0),
    })
}

pub(in crate::repositories) fn round_money(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::NaiveDate;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("booking-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("booking-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_booking_reads_preserve_scope_availability_and_pricing() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE devices (
                    serialNo TEXT PRIMARY KEY,
                    modelId TEXT,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE device_models (
                    id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, id)
                );
                CREATE TABLE model_base_prices (
                    modelId TEXT NOT NULL,
                    weekdayPrice REAL NOT NULL,
                    weekendPrice REAL NOT NULL,
                    tenant_id TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, modelId)
                );
                CREATE TABLE booking_availability (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    device_serial_no TEXT NOT NULL,
                    date TEXT NOT NULL,
                    is_available INTEGER NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                INSERT INTO device_models VALUES
                    ('model-a','Camera A','tenant-a'),
                    ('model-b','Camera B','tenant-b');
                INSERT INTO model_base_prices VALUES
                    ('model-a',10.0,20.0,'tenant-a'),
                    ('model-b',30.0,40.0,'tenant-b');
                INSERT INTO devices VALUES
                    ('A-001','model-a','tenant-a'),
                    ('B-001','model-b','tenant-b');
                INSERT INTO booking_availability
                    (device_serial_no,date,is_available,tenant_id)
                VALUES
                    ('A-001','2026-10-10',0,'tenant-a'),
                    ('B-001','2026-10-10',0,'tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let scoped_a = provider.bind(&context("tenant-a", "booking-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "booking-b")).unwrap();
        let start = NaiveDate::from_ymd_opt(2026, 10, 10).unwrap();
        let end = NaiveDate::from_ymd_opt(2026, 10, 11).unwrap();

        let a = scoped_a
            .booking_reads()
            .availability(None, start, end)
            .unwrap();
        assert_eq!(a.devices.len(), 1);
        assert_eq!(a.devices[0].serial_no, "A-001");
        assert_eq!(a.dates.len(), 2);
        assert!(!a.dates[0].available);
        assert!(a.dates[1].available);

        let b = scoped_b
            .booking_reads()
            .availability(None, start, end)
            .unwrap();
        assert_eq!(b.devices.len(), 1);
        assert_eq!(b.devices[0].serial_no, "B-001");

        let price = scoped_a.booking_reads().price("A-001").unwrap().unwrap();
        assert_eq!(price.model_name, "Camera A");
        assert_eq!(price.weekday_price, 10.0);
        assert_eq!(price.weekend_price, 20.0);

        let search = scoped_a
            .booking_reads()
            .search(Some("Camera"), Some("model-a"))
            .unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].serial_no, "A-001");
        assert_eq!(search[0].daily_rate, 15.0);

        assert!(scoped_a.booking_reads().price("B-001").unwrap().is_none());
    }
}

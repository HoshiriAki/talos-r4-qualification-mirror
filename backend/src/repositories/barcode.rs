use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const DEVICE_NOT_FOUND: &str = "barcode-device-not-found";
const SCAN_REFERENCES_NOT_FOUND: &str = "barcode-scan-references-not-found";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarcodeLabelProjection {
    pub id: i64,
    pub device_serial_no: String,
    pub barcode_text: String,
    pub barcode_type: String,
    pub label_format: String,
    pub generated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarcodeDeviceInfoProjection {
    pub serial_no: Option<String>,
    pub model_id: Option<String>,
    pub rental_status: Option<String>,
    pub status: Option<String>,
    pub current_warehouse_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarcodeLookupProjection {
    pub barcode_id: i64,
    pub device_serial_no: String,
    pub barcode_text: String,
    pub barcode_type: String,
    pub label_format: String,
    pub generated_at: String,
    pub device_info: BarcodeDeviceInfoProjection,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEventProjection {
    pub id: i64,
    pub device_serial_no: String,
    pub barcode_text: Option<String>,
    pub scan_type: String,
    pub scanned_by: String,
    pub warehouse_id: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScanHistoryProjection {
    pub events: Vec<ScanEventProjection>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatsByType {
    pub checkout: i64,
    pub checkin: i64,
    pub inventory: i64,
    pub transfer: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatsProjection {
    pub today_scans: i64,
    pub this_week_scans: i64,
    pub by_type: ScanStatsByType,
}

#[derive(Debug, thiserror::Error)]
pub enum BarcodeMutationError {
    #[error("device not found")]
    DeviceNotFound,
    #[error("scan references not found")]
    ScanReferencesNotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteBarcodeRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteBarcodeRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn generate(
        &self,
        device_serial_no: &str,
        now: &str,
    ) -> Result<(BarcodeLabelProjection, bool), BarcodeMutationError> {
        let tenant_id = self.tenant_id();
        let device_serial_no = device_serial_no.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let existing = transaction
                    .query_row(
                        "SELECT bl.id,bl.device_serial_no,bl.barcode_text,bl.barcode_type,
                                bl.label_format,bl.generated_at
                         FROM barcode_labels bl
                         JOIN devices d ON d.serialNo=bl.device_serial_no
                         WHERE d.tenant_id=?1 AND bl.device_serial_no=?2
                         LIMIT 1",
                        params![tenant_id, device_serial_no],
                        map_label,
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if let Some(existing) = existing {
                    return Ok((existing, true));
                }

                let owns_device = transaction
                    .query_row(
                        "SELECT 1 FROM devices
                         WHERE tenant_id=?1 AND serialNo=?2
                         LIMIT 1",
                        params![tenant_id, device_serial_no],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if owns_device.is_none() {
                    return Err(contract(DEVICE_NOT_FOUND.into()));
                }

                let barcode_text = barcode_text(&device_serial_no);
                transaction
                    .execute(
                        "INSERT INTO barcode_labels
                         (device_serial_no,barcode_text,barcode_type,label_format,generated_at)
                         VALUES (?1,?2,'CODE128','50x25mm',?3)",
                        params![device_serial_no, barcode_text, now],
                    )
                    .map_err(sqlite_error)?;
                let id = transaction.last_insert_rowid();

                Ok((
                    BarcodeLabelProjection {
                        id,
                        device_serial_no,
                        barcode_text,
                        barcode_type: "CODE128".into(),
                        label_format: "50x25mm".into(),
                        generated_at: now,
                    },
                    false,
                ))
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn batch_generate(
        &self,
        now: &str,
    ) -> Result<i64, BarcodeMutationError> {
        let tenant_id = self.tenant_id();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let unmarked = {
                    let mut statement = transaction
                        .prepare(
                            "SELECT d.serialNo
                             FROM devices d
                             LEFT JOIN barcode_labels bl ON bl.device_serial_no=d.serialNo
                             WHERE d.tenant_id=?1 AND bl.id IS NULL
                             ORDER BY d.serialNo",
                        )
                        .map_err(sqlite_error)?;
                    statement
                        .query_map(params![tenant_id], |row| row.get::<_, String>(0))
                        .map_err(sqlite_error)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(sqlite_error)?
                };

                let mut generated = 0_i64;
                for serial_no in unmarked {
                    let text = barcode_text(&serial_no);
                    transaction
                        .execute(
                            "INSERT INTO barcode_labels
                             (device_serial_no,barcode_text,barcode_type,label_format,generated_at)
                             VALUES (?1,?2,'CODE128','50x25mm',?3)",
                            params![serial_no, text, now],
                        )
                        .map_err(sqlite_error)?;
                    generated += 1;
                }
                Ok(generated)
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn lookup(
        &self,
        barcode_text: &str,
    ) -> Result<Option<BarcodeLookupProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let barcode_text = barcode_text.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT bl.id,bl.device_serial_no,bl.barcode_text,bl.barcode_type,
                            bl.label_format,bl.generated_at,
                            d.serialNo,d.modelId,d.rentalStatus,d.warning_status,d.currentWarehouseId
                     FROM barcode_labels bl
                     JOIN devices d ON d.serialNo=bl.device_serial_no
                     WHERE d.tenant_id=?1 AND bl.barcode_text=?2
                     LIMIT 1",
                    params![tenant_id, barcode_text],
                    map_lookup,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn record_scan(
        &self,
        device_serial_no: &str,
        barcode_text: Option<&str>,
        scan_type: &str,
        scanned_by: &str,
        warehouse_id: Option<&str>,
        notes: Option<&str>,
        now: &str,
    ) -> Result<ScanEventProjection, BarcodeMutationError> {
        let tenant_id = self.tenant_id();
        let device_serial_no = device_serial_no.to_owned();
        let barcode_text = barcode_text.map(str::to_owned);
        let scan_type = scan_type.to_owned();
        let scanned_by = scanned_by.to_owned();
        let warehouse_id = warehouse_id.map(str::to_owned);
        let notes = notes.map(str::to_owned);
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let device_exists = transaction
                    .query_row(
                        "SELECT 1 FROM devices
                         WHERE tenant_id=?1 AND serialNo=?2
                         LIMIT 1",
                        params![tenant_id, device_serial_no],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .is_some();

                let membership_exists = transaction
                    .query_row(
                        "SELECT 1 FROM tenant_memberships
                         WHERE tenant_id=?1 AND identity_id=?2 AND status='active'
                         LIMIT 1",
                        params![tenant_id, scanned_by],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .is_some();

                let warehouse_exists = if let Some(warehouse_id) = &warehouse_id {
                    transaction
                        .query_row(
                            "SELECT 1 FROM warehouses
                             WHERE tenant_id=?1 AND id=?2
                             LIMIT 1",
                            params![tenant_id, warehouse_id],
                            |row| row.get::<_, i64>(0),
                        )
                        .optional()
                        .map_err(sqlite_error)?
                        .is_some()
                } else {
                    true
                };

                if !device_exists || !membership_exists || !warehouse_exists {
                    return Err(contract(SCAN_REFERENCES_NOT_FOUND.into()));
                }

                transaction
                    .execute(
                        "INSERT INTO scan_events
                         (device_serial_no,barcode_text,scan_type,scanned_by,warehouse_id,
                          notes,created_at,tenant_id)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                        params![
                            device_serial_no,
                            barcode_text,
                            scan_type,
                            scanned_by,
                            warehouse_id,
                            notes,
                            now,
                            tenant_id,
                        ],
                    )
                    .map_err(sqlite_error)?;

                Ok(ScanEventProjection {
                    id: transaction.last_insert_rowid(),
                    device_serial_no,
                    barcode_text,
                    scan_type,
                    scanned_by,
                    warehouse_id,
                    notes,
                    created_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn history(
        &self,
        device_serial_no: Option<&str>,
        scan_type: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<ScanHistoryProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let device_serial_no = non_empty(device_serial_no);
        let scan_type = non_empty(scan_type);
        let start_date = non_empty(start_date);
        let end_date = non_empty(end_date);
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut predicates = vec!["tenant_id=?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id)];
            if let Some(value) = device_serial_no {
                predicates.push("device_serial_no=?".into());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = scan_type {
                predicates.push("scan_type=?".into());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = start_date {
                predicates.push("substr(created_at,1,10)>=?".into());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = end_date {
                predicates.push("substr(created_at,1,10)<=?".into());
                values.push(SqlValue::Text(value));
            }
            let where_sql = predicates.join(" AND ");

            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM scan_events WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;

            let sql = format!(
                "SELECT id,device_serial_no,barcode_text,scan_type,scanned_by,
                        warehouse_id,notes,created_at
                 FROM scan_events
                 WHERE {where_sql}
                 ORDER BY created_at DESC,id DESC
                 LIMIT {page_size} OFFSET {offset}"
            );
            let mut statement = connection.prepare(&sql)?;
            let events = statement
                .query_map(params_from_iter(values.iter()), map_scan_event)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(ScanHistoryProjection {
                events,
                total,
                page,
                page_size,
            })
        })
    }

    pub(in crate::repositories) fn stats(
        &self,
        today: &str,
        week_start: &str,
    ) -> Result<ScanStatsProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let today = today.to_owned();
        let week_start = week_start.to_owned();

        self.session.read(move |connection| {
            let today_scans: i64 = connection.query_row(
                "SELECT COUNT(*) FROM scan_events
                 WHERE tenant_id=?1 AND substr(created_at,1,10)=?2",
                params![tenant_id, today],
                |row| row.get(0),
            )?;
            let this_week_scans: i64 = connection.query_row(
                "SELECT COUNT(*) FROM scan_events
                 WHERE tenant_id=?1
                   AND replace(substr(created_at,1,19),'T',' ')>=?2",
                params![tenant_id, week_start],
                |row| row.get(0),
            )?;

            let mut statement = connection.prepare(
                "SELECT scan_type,COUNT(*)
                 FROM scan_events
                 WHERE tenant_id=?1
                   AND replace(substr(created_at,1,19),'T',' ')>=?2
                 GROUP BY scan_type",
            )?;
            let rows = statement
                .query_map(params![tenant_id, week_start], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(stats_projection(today_scans, this_week_scans, rows))
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub(in crate::repositories) fn map_mutation_error(error: RepositoryError) -> BarcodeMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message == DEVICE_NOT_FOUND {
            return BarcodeMutationError::DeviceNotFound;
        }
        if message == SCAN_REFERENCES_NOT_FOUND {
            return BarcodeMutationError::ScanReferencesNotFound;
        }
    }
    BarcodeMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

pub(in crate::repositories) fn barcode_text(serial_no: &str) -> String {
    let suffix = Uuid::new_v4().simple().to_string();
    format!("PKT-{serial_no}-{}", &suffix[..4])
}

pub(in crate::repositories) fn stats_projection(
    today_scans: i64,
    this_week_scans: i64,
    rows: Vec<(String, i64)>,
) -> ScanStatsProjection {
    let mut by_type = ScanStatsByType {
        checkout: 0,
        checkin: 0,
        inventory: 0,
        transfer: 0,
    };
    for (scan_type, count) in rows {
        match scan_type.as_str() {
            "checkout" => by_type.checkout = count,
            "checkin" => by_type.checkin = count,
            "inventory" => by_type.inventory = count,
            "transfer" => by_type.transfer = count,
            _ => {}
        }
    }
    ScanStatsProjection {
        today_scans,
        this_week_scans,
        by_type,
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.filter(|value| !value.is_empty()).map(str::to_owned)
}

fn map_label(row: &rusqlite::Row<'_>) -> rusqlite::Result<BarcodeLabelProjection> {
    Ok(BarcodeLabelProjection {
        id: row.get(0)?,
        device_serial_no: row.get(1)?,
        barcode_text: row.get(2)?,
        barcode_type: row.get(3)?,
        label_format: row.get(4)?,
        generated_at: row.get(5)?,
    })
}

fn map_lookup(row: &rusqlite::Row<'_>) -> rusqlite::Result<BarcodeLookupProjection> {
    Ok(BarcodeLookupProjection {
        barcode_id: row.get(0)?,
        device_serial_no: row.get(1)?,
        barcode_text: row.get(2)?,
        barcode_type: row.get(3)?,
        label_format: row.get(4)?,
        generated_at: row.get(5)?,
        device_info: BarcodeDeviceInfoProjection {
            serial_no: row.get(6)?,
            model_id: row.get(7)?,
            rental_status: row.get(8)?,
            status: row.get(9)?,
            current_warehouse_id: row.get(10)?,
        },
    })
}

fn map_scan_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScanEventProjection> {
    Ok(ScanEventProjection {
        id: row.get(0)?,
        device_serial_no: row.get(1)?,
        barcode_text: row.get(2)?,
        scan_type: row.get(3)?,
        scanned_by: row.get(4)?,
        warehouse_id: row.get(5)?,
        notes: row.get(6)?,
        created_at: row.get(7)?,
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

    use crate::repositories::{BarcodeMutationError, RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, actor: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated(actor, "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("barcode-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_barcode_authority_preserves_scope_idempotence_scan_references_and_stats() {
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
                    rentalStatus TEXT,
                    warning_status TEXT,
                    currentWarehouseId TEXT,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE barcode_labels (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    device_serial_no TEXT NOT NULL UNIQUE,
                    barcode_text TEXT NOT NULL UNIQUE,
                    barcode_type TEXT NOT NULL,
                    label_format TEXT NOT NULL,
                    generated_at TEXT NOT NULL
                );
                CREATE TABLE scan_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    device_serial_no TEXT NOT NULL,
                    barcode_text TEXT,
                    scan_type TEXT NOT NULL,
                    scanned_by TEXT NOT NULL,
                    warehouse_id TEXT,
                    notes TEXT,
                    created_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE tenant_memberships (
                    id TEXT PRIMARY KEY,
                    identity_id TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    status TEXT NOT NULL
                );
                CREATE TABLE warehouses (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL
                );
                INSERT INTO devices VALUES
                    ('A-001','model-a','available','正常','wh-a','tenant-a'),
                    ('A-002','model-a','available','正常','wh-a','tenant-a'),
                    ('B-001','model-b','available','正常','wh-b','tenant-b');
                INSERT INTO tenant_memberships VALUES
                    ('membership-a','identity-a','tenant-a','staff','active'),
                    ('membership-b','identity-b','tenant-b','staff','active');
                INSERT INTO warehouses VALUES
                    ('wh-a','tenant-a'),
                    ('wh-b','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let ctx_a = context("tenant-a", "identity-a", "barcode-a");
        let ctx_b = context("tenant-b", "identity-b", "barcode-b");
        let scoped_a = provider.bind(&ctx_a).unwrap();
        let scoped_b = provider.bind(&ctx_b).unwrap();

        let (generated, already_exists) = scoped_a
            .barcodes()
            .generate("A-001", "2026-10-01 21:00:00")
            .unwrap();
        assert!(!already_exists);
        assert!(generated.barcode_text.starts_with("PKT-A-001-"));

        let (same, already_exists) = scoped_a
            .barcodes()
            .generate("A-001", "2026-10-01 21:01:00")
            .unwrap();
        assert!(already_exists);
        assert_eq!(same.id, generated.id);
        assert_eq!(same.barcode_text, generated.barcode_text);

        assert!(
            scoped_b
                .barcodes()
                .lookup(&generated.barcode_text)
                .unwrap()
                .is_none()
        );
        let lookup = scoped_a
            .barcodes()
            .lookup(&generated.barcode_text)
            .unwrap()
            .unwrap();
        assert_eq!(lookup.device_serial_no, "A-001");
        assert_eq!(
            lookup.device_info.current_warehouse_id.as_deref(),
            Some("wh-a")
        );
        assert_eq!(lookup.device_info.status.as_deref(), Some("正常"));

        assert_eq!(
            scoped_a
                .barcodes()
                .batch_generate("2026-10-01 21:02:00")
                .unwrap(),
            1
        );

        let event = scoped_a
            .barcodes()
            .record_scan(
                "A-001",
                Some(&generated.barcode_text),
                "inventory",
                "identity-a",
                Some("wh-a"),
                Some("counted"),
                "2026-10-01 21:03:00",
            )
            .unwrap();
        assert_eq!(event.scanned_by, "identity-a");
        assert_eq!(event.warehouse_id.as_deref(), Some("wh-a"));

        let cross_tenant_device = scoped_b.barcodes().record_scan(
            "A-001",
            None,
            "inventory",
            "identity-b",
            Some("wh-b"),
            None,
            "2026-10-01 21:04:00",
        );
        assert!(matches!(
            cross_tenant_device,
            Err(BarcodeMutationError::ScanReferencesNotFound)
        ));

        let cross_tenant_warehouse = scoped_a.barcodes().record_scan(
            "A-001",
            None,
            "inventory",
            "identity-a",
            Some("wh-b"),
            None,
            "2026-10-01 21:05:00",
        );
        assert!(matches!(
            cross_tenant_warehouse,
            Err(BarcodeMutationError::ScanReferencesNotFound)
        ));

        let history = scoped_a
            .barcodes()
            .history(None, None, Some("2026-10-01"), Some("2026-10-01"), 1, 20)
            .unwrap();
        assert_eq!(history.total, 1);
        assert_eq!(history.events[0].scanned_by, "identity-a");
        assert_eq!(
            scoped_b
                .barcodes()
                .history(None, None, None, None, 1, 20)
                .unwrap()
                .total,
            0
        );

        let stats = scoped_a
            .barcodes()
            .stats("2026-10-01", "2026-09-24 00:00:00")
            .unwrap();
        assert_eq!(stats.today_scans, 1);
        assert_eq!(stats.this_week_scans, 1);
        assert_eq!(stats.by_type.inventory, 1);
    }
}

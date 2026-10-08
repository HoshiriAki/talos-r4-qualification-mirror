use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxConfigProjection {
    pub id: String,
    pub tax_type: String,
    pub rate: f64,
    pub effective_from: String,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaxUpsertOutcome {
    pub config_id: String,
    pub tax_type: String,
    pub rate: f64,
    pub effective_from: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaxInvoiceExportRow {
    pub invoice_no: String,
    pub order_id: String,
    pub invoice_type: String,
    pub amount: f64,
    pub tax_rate: f64,
    pub tax_amount: f64,
    pub status: String,
    pub issued_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaxConfigExportRow {
    pub tax_type: String,
    pub rate: f64,
    pub effective_from: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaxExportSnapshot {
    pub invoices: Vec<TaxInvoiceExportRow>,
    pub configs: Vec<TaxConfigExportRow>,
}

pub(in crate::repositories) struct SqliteTaxRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteTaxRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn get_configs(
        &self,
        tax_type: &str,
    ) -> Result<Vec<TaxConfigProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let tax_type = tax_type.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT id,tax_type,rate,effective_from,is_active,created_at
                 FROM tax_config
                 WHERE tenant_id=?1 AND tax_type=?2
                 ORDER BY effective_from DESC,created_at DESC",
            )?;
            statement
                .query_map(params![tenant_id, tax_type], |row| {
                    Ok(TaxConfigProjection {
                        id: row.get(0)?,
                        tax_type: row.get(1)?,
                        rate: row.get(2)?,
                        effective_from: row.get(3)?,
                        is_active: row.get::<_, i64>(4)? != 0,
                        created_at: row.get(5)?,
                    })
                })?
                .collect()
        })
    }

    pub(in crate::repositories) fn active_rate(
        &self,
        tax_type: &str,
    ) -> Result<Option<f64>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let tax_type = tax_type.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT rate FROM tax_config
                     WHERE tenant_id=?1 AND tax_type=?2 AND is_active=1
                     ORDER BY effective_from DESC,created_at DESC LIMIT 1",
                    params![tenant_id, tax_type],
                    |row| row.get(0),
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn upsert_config(
        &self,
        tax_type: &str,
        rate: f64,
        effective_from: &str,
        now: &str,
    ) -> Result<TaxUpsertOutcome, RepositoryError> {
        let tenant_id = self.tenant_id();
        let tax_type = tax_type.to_owned();
        let effective_from = effective_from.to_owned();
        let now = now.to_owned();

        self.session.write_immediate(move |transaction| {
            transaction
                .execute(
                    "UPDATE tax_config
                     SET is_active=0
                     WHERE tenant_id=?1 AND tax_type=?2 AND is_active<>0",
                    params![tenant_id, tax_type],
                )
                .map_err(sqlite_error)?;

            let config_id = Uuid::new_v4().to_string();
            transaction
                .execute(
                    "INSERT INTO tax_config
                     (id,tax_type,rate,effective_from,is_active,created_at,tenant_id)
                     VALUES (?1,?2,?3,?4,1,?5,?6)",
                    params![config_id, tax_type, rate, effective_from, now, tenant_id],
                )
                .map_err(sqlite_error)?;

            Ok(TaxUpsertOutcome {
                config_id,
                tax_type,
                rate,
                effective_from,
            })
        })
    }

    pub(in crate::repositories) fn export_snapshot(
        &self,
        period_prefix: &str,
    ) -> Result<TaxExportSnapshot, RepositoryError> {
        let tenant_id = self.tenant_id();
        let date_pattern = format!("{period_prefix}%");

        self.session.read(move |connection| {
            let mut invoices_statement = connection.prepare(
                "SELECT invoice_no,order_id,type,amount,tax_rate,tax_amount,status,issued_at
                 FROM invoices
                 WHERE tenant_id=?1 AND issued_at LIKE ?2
                 ORDER BY issued_at ASC,invoice_no ASC",
            )?;
            let invoices = invoices_statement
                .query_map(params![tenant_id, date_pattern], |row| {
                    Ok(TaxInvoiceExportRow {
                        invoice_no: row.get(0)?,
                        order_id: row.get(1)?,
                        invoice_type: row.get(2)?,
                        amount: row.get(3)?,
                        tax_rate: row.get(4)?,
                        tax_amount: row.get(5)?,
                        status: row.get(6)?,
                        issued_at: row.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut configs_statement = connection.prepare(
                "SELECT tax_type,rate,effective_from
                 FROM tax_config
                 WHERE tenant_id=?1 AND is_active=1
                 ORDER BY tax_type ASC,effective_from DESC",
            )?;
            let configs = configs_statement
                .query_map(params![tenant_id], |row| {
                    Ok(TaxConfigExportRow {
                        tax_type: row.get(0)?,
                        rate: row.get(1)?,
                        effective_from: row.get(2)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(TaxExportSnapshot { invoices, configs })
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
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

    use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("tax-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("tax-test-revision").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_tax_authority_preserves_scope_active_rate_and_export_snapshot() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE tax_config (
                    id TEXT PRIMARY KEY,
                    tax_type TEXT NOT NULL,
                    rate REAL NOT NULL,
                    effective_from TEXT NOT NULL,
                    is_active INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE invoices (
                    id TEXT PRIMARY KEY,
                    order_id TEXT NOT NULL,
                    invoice_no TEXT NOT NULL,
                    type TEXT NOT NULL,
                    amount REAL NOT NULL,
                    tax_rate REAL NOT NULL,
                    tax_amount REAL NOT NULL,
                    status TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    issued_at TEXT NOT NULL,
                    voided_at TEXT,
                    created_at TEXT NOT NULL
                );
                INSERT INTO invoices VALUES
                    ('invoice-a','order-a','INV-A','普通发票',106,0.06,6,'issued','tenant-a','2026-09-30T09:00:00+08:00',NULL,'now'),
                    ('invoice-b','order-b','INV-B','专用发票',113,0.13,13,'issued','tenant-b','2026-09-30T09:00:00+08:00',NULL,'now');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool.clone());
        let scoped_a = provider.bind(&context("tenant-a", "tax-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "tax-b")).unwrap();

        scoped_a
            .taxes()
            .upsert_config("vat", 0.06, "2026-01-01", "2026-09-30T09:10:00+08:00")
            .unwrap();
        scoped_a
            .taxes()
            .upsert_config("vat", 0.09, "2026-07-01", "2026-09-30T09:11:00+08:00")
            .unwrap();
        scoped_b
            .taxes()
            .upsert_config("vat", 0.13, "2026-01-01", "2026-09-30T09:12:00+08:00")
            .unwrap();

        let configs_a = scoped_a.taxes().get_configs("vat").unwrap();
        assert_eq!(configs_a.len(), 2);
        assert_eq!(configs_a.iter().filter(|row| row.is_active).count(), 1);
        assert_eq!(scoped_a.taxes().active_rate("vat").unwrap(), Some(0.09));
        assert_eq!(scoped_b.taxes().active_rate("vat").unwrap(), Some(0.13));

        let export_a = scoped_a.taxes().export_snapshot("2026-09").unwrap();
        assert_eq!(export_a.invoices.len(), 1);
        assert_eq!(export_a.invoices[0].invoice_no, "INV-A");
        assert_eq!(export_a.configs.len(), 1);
        assert_eq!(export_a.configs[0].rate, 0.09);

        let connection = pool.get().unwrap();
        let active_a: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM tax_config
                 WHERE tenant_id='tenant-a' AND tax_type='vat' AND is_active=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_a, 1);
    }
}

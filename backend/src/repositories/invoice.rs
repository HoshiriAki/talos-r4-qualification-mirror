use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const NOT_FOUND: &str = "invoice-not-found";
const NOT_ISSUED_PREFIX: &str = "invoice-not-issued:";

#[derive(Debug, Clone, PartialEq)]
pub struct InvoiceIssueOutcome {
    pub invoice_id: String,
    pub invoice_no: String,
    pub order_id: String,
    pub amount: f64,
    pub tax_rate: f64,
    pub tax_amount: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvoiceVoidOutcome {
    pub invoice_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvoiceRedFlushOutcome {
    pub original_invoice_id: String,
    pub red_invoice_id: String,
    pub red_invoice_no: String,
    pub red_amount: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceProjection {
    pub id: String,
    pub order_id: String,
    pub invoice_no: String,
    #[serde(rename = "type")]
    pub invoice_type: String,
    pub amount: f64,
    pub tax_rate: f64,
    pub tax_amount: f64,
    pub status: String,
    pub tenant_id: String,
    pub issued_at: String,
    pub voided_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceListItem {
    pub id: String,
    pub order_id: String,
    pub invoice_no: String,
    #[serde(rename = "type")]
    pub invoice_type: String,
    pub amount: f64,
    pub tax_rate: f64,
    pub tax_amount: f64,
    pub status: String,
    pub issued_at: String,
    pub voided_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InvoiceListProjection {
    pub invoices: Vec<InvoiceListItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum InvoiceMutationError {
    #[error("invoice not found")]
    NotFound,
    #[error("invoice is not issued: {0}")]
    NotIssued(String),
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteInvoiceRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteInvoiceRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn issue(
        &self,
        order_id: &str,
        amount: f64,
        invoice_type: &str,
        tax_rate: Option<f64>,
        now: &str,
    ) -> Result<InvoiceIssueOutcome, InvoiceMutationError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        let invoice_type = invoice_type.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let tax_rate = match tax_rate {
                    Some(value) => value,
                    None => transaction
                        .query_row(
                            "SELECT rate FROM tax_config
                             WHERE tenant_id=?1 AND tax_type='vat' AND is_active=1
                             ORDER BY effective_from DESC LIMIT 1",
                            params![tenant_id],
                            |row| row.get::<_, f64>(0),
                        )
                        .optional()
                        .map_err(sqlite_error)?
                        .unwrap_or(0.13),
                };
                let invoice_no = next_invoice_no_sqlite(transaction, &tenant_id, &now)?;
                let tax_amount = round_money(amount * tax_rate);
                let invoice_id = Uuid::new_v4().to_string();

                transaction
                    .execute(
                        "INSERT INTO invoices
                         (id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,
                          tenant_id,issued_at,created_at)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,'issued',?8,?9,?9)",
                        params![
                            invoice_id,
                            order_id,
                            invoice_no,
                            invoice_type,
                            amount,
                            tax_rate,
                            tax_amount,
                            tenant_id,
                            now,
                        ],
                    )
                    .map_err(sqlite_error)?;

                let revenue_id = Uuid::new_v4().to_string();
                transaction
                    .execute(
                        "INSERT INTO revenue_records
                         (id,order_id,amount,recognition_date,source,created_at,tenant_id)
                         VALUES (?1,?2,?3,?4,'order_complete',?5,?6)",
                        params![revenue_id, order_id, amount, &now[..10], now, tenant_id],
                    )
                    .map_err(sqlite_error)?;

                insert_accounting_entry_sqlite(
                    transaction,
                    &tenant_id,
                    &order_id,
                    "debit",
                    "receivable",
                    amount,
                    &format!("发票 {invoice_no} 应收账款"),
                    &now,
                )?;
                insert_accounting_entry_sqlite(
                    transaction,
                    &tenant_id,
                    &order_id,
                    "credit",
                    "revenue",
                    amount,
                    &format!("发票 {invoice_no} 收入确认"),
                    &now,
                )?;

                Ok(InvoiceIssueOutcome {
                    invoice_id,
                    invoice_no,
                    order_id,
                    amount,
                    tax_rate,
                    tax_amount,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn void(
        &self,
        invoice_id: &str,
        now: &str,
    ) -> Result<InvoiceVoidOutcome, InvoiceMutationError> {
        let tenant_id = self.tenant_id();
        let invoice_id = invoice_id.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let row = transaction
                    .query_row(
                        "SELECT order_id,status,invoice_no FROM invoices
                         WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, invoice_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(NOT_FOUND.into()))?;
                let (order_id, status, invoice_no) = row;
                if status != "issued" {
                    return Err(contract(format!("{NOT_ISSUED_PREFIX}{status}")));
                }

                transaction
                    .execute(
                        "UPDATE invoices SET status='voided',voided_at=?1
                         WHERE tenant_id=?2 AND id=?3 AND status='issued'",
                        params![now, tenant_id, invoice_id],
                    )
                    .map_err(sqlite_error)?;

                insert_accounting_entry_sqlite(
                    transaction,
                    &tenant_id,
                    &order_id,
                    "credit",
                    "receivable",
                    0.0,
                    &format!("发票 {invoice_no} 作废冲回应收"),
                    &now,
                )?;

                Ok(InvoiceVoidOutcome { invoice_id })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn red_flush(
        &self,
        invoice_id: &str,
        reason: &str,
        now: &str,
    ) -> Result<InvoiceRedFlushOutcome, InvoiceMutationError> {
        let tenant_id = self.tenant_id();
        let invoice_id = invoice_id.to_owned();
        let reason = reason.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let row = transaction
                    .query_row(
                        "SELECT order_id,amount,tax_rate,tax_amount,invoice_no,type,status
                         FROM invoices WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, invoice_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, f64>(1)?,
                                row.get::<_, f64>(2)?,
                                row.get::<_, f64>(3)?,
                                row.get::<_, String>(4)?,
                                row.get::<_, String>(5)?,
                                row.get::<_, String>(6)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(NOT_FOUND.into()))?;
                let (order_id, amount, tax_rate, tax_amount, invoice_no, invoice_type, status) =
                    row;
                if status != "issued" {
                    return Err(contract(format!("{NOT_ISSUED_PREFIX}{status}")));
                }

                let red_invoice_no = next_invoice_no_sqlite(transaction, &tenant_id, &now)?;
                transaction
                    .execute(
                        "UPDATE invoices SET status='voided',voided_at=?1
                         WHERE tenant_id=?2 AND id=?3 AND status='issued'",
                        params![now, tenant_id, invoice_id],
                    )
                    .map_err(sqlite_error)?;

                let red_invoice_id = Uuid::new_v4().to_string();
                let red_amount = -amount;
                let red_tax_amount = -tax_amount;
                transaction
                    .execute(
                        "INSERT INTO invoices
                         (id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,
                          tenant_id,issued_at,created_at)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,'issued',?8,?9,?9)",
                        params![
                            red_invoice_id,
                            order_id,
                            red_invoice_no,
                            invoice_type,
                            red_amount,
                            tax_rate,
                            red_tax_amount,
                            tenant_id,
                            now,
                        ],
                    )
                    .map_err(sqlite_error)?;

                let revenue_id = Uuid::new_v4().to_string();
                transaction
                    .execute(
                        "INSERT INTO revenue_records
                         (id,order_id,amount,recognition_date,source,created_at,tenant_id)
                         VALUES (?1,?2,?3,?4,'other',?5,?6)",
                        params![revenue_id, order_id, red_amount, &now[..10], now, tenant_id],
                    )
                    .map_err(sqlite_error)?;

                let description = if reason.is_empty() {
                    format!("红字冲销 原发票 {invoice_no}")
                } else {
                    format!("红字冲销 原发票 {invoice_no} — {reason}")
                };
                insert_accounting_entry_sqlite(
                    transaction,
                    &tenant_id,
                    &order_id,
                    "credit",
                    "receivable",
                    amount,
                    &description,
                    &now,
                )?;
                insert_accounting_entry_sqlite(
                    transaction,
                    &tenant_id,
                    &order_id,
                    "debit",
                    "revenue",
                    amount,
                    &description,
                    &now,
                )?;

                Ok(InvoiceRedFlushOutcome {
                    original_invoice_id: invoice_id,
                    red_invoice_id,
                    red_invoice_no,
                    red_amount,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn get_by_id(
        &self,
        invoice_id: &str,
    ) -> Result<Option<InvoiceProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let invoice_id = invoice_id.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,
                            tenant_id,issued_at,voided_at,created_at
                     FROM invoices
                     WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                    params![tenant_id, invoice_id],
                    map_projection,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn get_by_order(
        &self,
        order_id: &str,
    ) -> Result<Option<InvoiceProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,
                            tenant_id,issued_at,voided_at,created_at
                     FROM invoices
                     WHERE tenant_id=?1 AND order_id=?2
                     ORDER BY created_at DESC LIMIT 1",
                    params![tenant_id, order_id],
                    map_projection,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn list(
        &self,
        status: Option<&str>,
        date_from: Option<&str>,
        date_to: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<InvoiceListProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
        let date_from = date_from
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let date_to = date_to
            .filter(|value| !value.is_empty())
            .map(|value| format!("{value}T23:59:59.999+08:00"));
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut predicates = vec!["tenant_id = ?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id)];
            if let Some(status) = status {
                predicates.push("status = ?".into());
                values.push(SqlValue::Text(status));
            }
            if let Some(date_from) = date_from {
                predicates.push("issued_at >= ?".into());
                values.push(SqlValue::Text(date_from));
            }
            if let Some(date_to) = date_to {
                predicates.push("issued_at <= ?".into());
                values.push(SqlValue::Text(date_to));
            }
            let where_sql = predicates.join(" AND ");

            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM invoices WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;

            let sql = format!(
                "SELECT id,order_id,invoice_no,type,amount,tax_rate,tax_amount,status,
                        issued_at,voided_at,created_at
                 FROM invoices WHERE {where_sql}
                 ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
            );
            let mut statement = connection.prepare(&sql)?;
            let invoices = statement
                .query_map(params_from_iter(values.iter()), map_list_item)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(InvoiceListProjection {
                invoices,
                page,
                page_size,
                total,
            })
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub(in crate::repositories) fn map_mutation_error(error: RepositoryError) -> InvoiceMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message == NOT_FOUND {
            return InvoiceMutationError::NotFound;
        }
        if let Some(status) = message.strip_prefix(NOT_ISSUED_PREFIX) {
            return InvoiceMutationError::NotIssued(status.to_owned());
        }
    }
    InvoiceMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn next_invoice_no_sqlite(
    transaction: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    now: &str,
) -> Result<String, RepositoryError> {
    let date = now.get(..10).unwrap_or("1970-01-01").replace('-', "");
    let prefix = format!("INV-{date}-");
    let like_pattern = format!("{prefix}%");
    let max_seq: Option<String> = transaction
        .query_row(
            "SELECT invoice_no FROM invoices
             WHERE tenant_id=?1 AND invoice_no LIKE ?2
             ORDER BY invoice_no DESC LIMIT 1",
            params![tenant_id, like_pattern],
            |row| row.get(0),
        )
        .optional()
        .map_err(sqlite_error)?;
    let seq = max_seq
        .as_deref()
        .and_then(|value| value.strip_prefix(&prefix))
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
        + 1;
    Ok(format!("{prefix}{seq:04}"))
}

fn insert_accounting_entry_sqlite(
    transaction: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    order_id: &str,
    entry_type: &str,
    account: &str,
    amount: f64,
    description: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    transaction
        .execute(
            "INSERT INTO accounting_entries
             (id,order_id,entry_type,account,amount,description,created_at,tenant_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                Uuid::new_v4().to_string(),
                order_id,
                entry_type,
                account,
                amount,
                description,
                now,
                tenant_id,
            ],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn round_money(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn map_projection(row: &rusqlite::Row<'_>) -> rusqlite::Result<InvoiceProjection> {
    Ok(InvoiceProjection {
        id: row.get(0)?,
        order_id: row.get(1)?,
        invoice_no: row.get(2)?,
        invoice_type: row.get(3)?,
        amount: row.get(4)?,
        tax_rate: row.get(5)?,
        tax_amount: row.get(6)?,
        status: row.get(7)?,
        tenant_id: row.get(8)?,
        issued_at: row.get(9)?,
        voided_at: row.get(10)?,
        created_at: row.get(11)?,
    })
}

fn map_list_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<InvoiceListItem> {
    Ok(InvoiceListItem {
        id: row.get(0)?,
        order_id: row.get(1)?,
        invoice_no: row.get(2)?,
        invoice_type: row.get(3)?,
        amount: row.get(4)?,
        tax_rate: row.get(5)?,
        tax_amount: row.get(6)?,
        status: row.get(7)?,
        issued_at: row.get(8)?,
        voided_at: row.get(9)?,
        created_at: row.get(10)?,
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

    use crate::repositories::{InvoiceMutationError, RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("invoice-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("invoice-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_invoice_authority_preserves_tenant_numbering_and_atomic_accounting() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE invoices (
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
                    created_at TEXT NOT NULL,
                    UNIQUE(tenant_id, invoice_no)
                );
                CREATE TABLE tax_config (
                    id TEXT PRIMARY KEY,
                    tax_type TEXT NOT NULL,
                    rate REAL NOT NULL,
                    effective_from TEXT NOT NULL,
                    is_active INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE revenue_records (
                    id TEXT PRIMARY KEY,
                    order_id TEXT NOT NULL,
                    amount REAL NOT NULL,
                    recognition_date TEXT NOT NULL,
                    source TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE accounting_entries (
                    id TEXT PRIMARY KEY,
                    order_id TEXT NOT NULL,
                    entry_type TEXT NOT NULL,
                    account TEXT NOT NULL,
                    amount REAL NOT NULL,
                    description TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                INSERT INTO tax_config VALUES
                    ('vat-a','vat',0.06,'2026-01-01',1,'now','tenant-a');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool.clone());
        let scoped_a = provider.bind(&context("tenant-a", "invoice-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "invoice-b")).unwrap();
        let now = "2026-09-29T21:30:00.000+08:00";

        let a1 = scoped_a
            .invoices()
            .issue("order-a", 100.0, "普通发票", None, now)
            .unwrap();
        let b1 = scoped_b
            .invoices()
            .issue("order-b", 200.0, "普通发票", None, now)
            .unwrap();

        assert_eq!(a1.invoice_no, "INV-20260929-0001");
        assert_eq!(b1.invoice_no, "INV-20260929-0001");
        assert_eq!(a1.tax_rate, 0.06);
        assert_eq!(a1.tax_amount, 6.0);
        assert_eq!(b1.tax_rate, 0.13);
        assert_eq!(b1.tax_amount, 26.0);
        assert!(
            scoped_b
                .invoices()
                .get_by_id(&a1.invoice_id)
                .unwrap()
                .is_none()
        );

        let a2 = scoped_a
            .invoices()
            .issue("order-a-2", 50.0, "专用发票", Some(0.10), now)
            .unwrap();
        assert_eq!(a2.invoice_no, "INV-20260929-0002");

        scoped_a
            .invoices()
            .void(&a2.invoice_id, "2026-09-29T21:31:00.000+08:00")
            .unwrap();
        let repeated_void = scoped_a
            .invoices()
            .void(&a2.invoice_id, "2026-09-29T21:32:00.000+08:00");
        assert!(matches!(
            repeated_void,
            Err(InvoiceMutationError::NotIssued(status)) if status == "voided"
        ));

        let red = scoped_a
            .invoices()
            .red_flush(
                &a1.invoice_id,
                "customer correction",
                "2026-09-29T21:33:00.000+08:00",
            )
            .unwrap();
        assert_eq!(red.red_invoice_no, "INV-20260929-0003");
        assert_eq!(red.red_amount, -100.0);

        let original = scoped_a
            .invoices()
            .get_by_id(&a1.invoice_id)
            .unwrap()
            .unwrap();
        let red_invoice = scoped_a
            .invoices()
            .get_by_id(&red.red_invoice_id)
            .unwrap()
            .unwrap();
        assert_eq!(original.status, "voided");
        assert_eq!(red_invoice.status, "issued");
        assert_eq!(red_invoice.amount, -100.0);

        let list = scoped_a
            .invoices()
            .list(None, Some("2026-09-29"), Some("2026-09-29"), 1, 20)
            .unwrap();
        assert_eq!(list.total, 3);
        assert_eq!(
            scoped_b
                .invoices()
                .list(None, None, None, 1, 20)
                .unwrap()
                .total,
            1
        );

        let connection = pool.get().unwrap();
        let revenue_sum: f64 = connection
            .query_row(
                "SELECT COALESCE(SUM(amount),0) FROM revenue_records WHERE tenant_id='tenant-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let accounting_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM accounting_entries WHERE tenant_id='tenant-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(revenue_sum, 50.0);
        assert_eq!(accounting_count, 7);
    }
}

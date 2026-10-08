use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const NOT_FOUND: &str = "settlement-not-found";
const ALREADY_CONFIRMED: &str = "settlement-already-confirmed";

#[derive(Debug, Clone, PartialEq)]
pub struct SettlementGenerateOutcome {
    pub settlement_id: String,
    pub period_type: String,
    pub period_key: String,
    pub total_revenue: Option<f64>,
    pub total_deposits: Option<f64>,
    pub total_refunds: Option<f64>,
    pub existing: bool,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettlementConfirmOutcome {
    pub settlement_id: String,
    pub period_type: String,
    pub period_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementProjection {
    pub id: String,
    pub period_type: String,
    pub period_key: String,
    pub total_revenue: f64,
    pub total_deposits: f64,
    pub total_refunds: f64,
    pub confirmed: bool,
    pub confirmed_at: Option<String>,
    pub tenant_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettlementListProjection {
    pub settlements: Vec<SettlementProjection>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettlementRevenueDetail {
    pub order_id: String,
    pub amount: f64,
    pub recognition_date: String,
    pub source: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SettlementMutationError {
    #[error("settlement not found")]
    NotFound,
    #[error("settlement already confirmed")]
    AlreadyConfirmed,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteSettlementRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteSettlementRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn generate(
        &self,
        period_type: &str,
        period_key: &str,
        date_start: &str,
        date_end: &str,
        now: &str,
    ) -> Result<SettlementGenerateOutcome, SettlementMutationError> {
        let tenant_id = self.tenant_id();
        let period_type = period_type.to_owned();
        let period_key = period_key.to_owned();
        let date_start = date_start.to_owned();
        let date_end = date_end.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let existing = transaction
                    .query_row(
                        "SELECT id, confirmed FROM settlements
                     WHERE tenant_id=?1 AND period_type=?2 AND period_key=?3
                     LIMIT 1",
                        params![tenant_id, period_type, period_key],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                    )
                    .optional()
                    .map_err(sqlite_error)?;

                if let Some((settlement_id, confirmed)) = existing {
                    if confirmed != 0 {
                        return Ok(SettlementGenerateOutcome {
                            settlement_id,
                            period_type,
                            period_key,
                            total_revenue: None,
                            total_deposits: None,
                            total_refunds: None,
                            existing: true,
                            confirmed: true,
                        });
                    }

                    let (revenue, deposits, refunds) =
                        aggregate_sqlite(transaction, &tenant_id, &date_start, &date_end)?;
                    transaction
                        .execute(
                            "UPDATE settlements
                         SET total_revenue=?1,total_deposits=?2,total_refunds=?3,created_at=?4
                         WHERE tenant_id=?5 AND id=?6 AND confirmed=0",
                            params![revenue, deposits, refunds, now, tenant_id, settlement_id],
                        )
                        .map_err(sqlite_error)?;

                    return Ok(SettlementGenerateOutcome {
                        settlement_id,
                        period_type,
                        period_key,
                        total_revenue: Some(revenue),
                        total_deposits: Some(deposits),
                        total_refunds: Some(refunds),
                        existing: true,
                        confirmed: false,
                    });
                }

                let (revenue, deposits, refunds) =
                    aggregate_sqlite(transaction, &tenant_id, &date_start, &date_end)?;
                let settlement_id = Uuid::new_v4().to_string();
                transaction
                    .execute(
                        "INSERT INTO settlements
                     (id,period_type,period_key,total_revenue,total_deposits,total_refunds,
                      confirmed,tenant_id,created_at)
                     VALUES (?1,?2,?3,?4,?5,?6,0,?7,?8)",
                        params![
                            settlement_id,
                            period_type,
                            period_key,
                            revenue,
                            deposits,
                            refunds,
                            tenant_id,
                            now,
                        ],
                    )
                    .map_err(sqlite_error)?;

                Ok(SettlementGenerateOutcome {
                    settlement_id,
                    period_type,
                    period_key,
                    total_revenue: Some(revenue),
                    total_deposits: Some(deposits),
                    total_refunds: Some(refunds),
                    existing: false,
                    confirmed: false,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn confirm(
        &self,
        settlement_id: &str,
        now: &str,
    ) -> Result<SettlementConfirmOutcome, SettlementMutationError> {
        let tenant_id = self.tenant_id();
        let settlement_id = settlement_id.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let row = transaction
                    .query_row(
                        "SELECT period_type,period_key,confirmed FROM settlements
                     WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, settlement_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, i64>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(NOT_FOUND.into()))?;
                let (period_type, period_key, confirmed) = row;
                if confirmed != 0 {
                    return Err(contract(ALREADY_CONFIRMED.into()));
                }

                let updated = transaction
                    .execute(
                        "UPDATE settlements SET confirmed=1,confirmed_at=?1
                     WHERE tenant_id=?2 AND id=?3 AND confirmed=0",
                        params![now, tenant_id, settlement_id],
                    )
                    .map_err(sqlite_error)?;
                if updated != 1 {
                    return Err(contract(ALREADY_CONFIRMED.into()));
                }

                Ok(SettlementConfirmOutcome {
                    settlement_id,
                    period_type,
                    period_key,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn get_by_period(
        &self,
        period_type: &str,
        period_key: &str,
    ) -> Result<Option<SettlementProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let period_type = period_type.to_owned();
        let period_key = period_key.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,period_type,period_key,total_revenue,total_deposits,total_refunds,
                            confirmed,confirmed_at,tenant_id,created_at
                     FROM settlements
                     WHERE tenant_id=?1 AND period_type=?2 AND period_key=?3 LIMIT 1",
                    params![tenant_id, period_type, period_key],
                    map_projection,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn get_by_id(
        &self,
        settlement_id: &str,
    ) -> Result<Option<SettlementProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let settlement_id = settlement_id.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,period_type,period_key,total_revenue,total_deposits,total_refunds,
                            confirmed,confirmed_at,tenant_id,created_at
                     FROM settlements
                     WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                    params![tenant_id, settlement_id],
                    map_projection,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn list(
        &self,
        period_type: Option<&str>,
        confirmed: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<SettlementListProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let period_type = period_type.filter(|v| !v.is_empty()).map(str::to_owned);
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut conditions = vec!["tenant_id = ?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id)];
            if let Some(period_type) = period_type {
                conditions.push("period_type = ?".into());
                values.push(SqlValue::Text(period_type));
            }
            if let Some(confirmed) = confirmed {
                conditions.push("confirmed = ?".into());
                values.push(SqlValue::Integer(if confirmed { 1_i64 } else { 0_i64 }));
            }
            let where_sql = conditions.join(" AND ");

            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM settlements WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;

            let sql = format!(
                "SELECT id,period_type,period_key,total_revenue,total_deposits,total_refunds,
                        confirmed,confirmed_at,tenant_id,created_at
                 FROM settlements WHERE {where_sql}
                 ORDER BY period_key DESC LIMIT {page_size} OFFSET {offset}"
            );
            let mut statement = connection.prepare(&sql)?;
            let settlements = statement
                .query_map(params_from_iter(values.iter()), map_projection)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(SettlementListProjection {
                settlements,
                page,
                page_size,
                total,
            })
        })
    }

    pub(in crate::repositories) fn revenue_details(
        &self,
        date_start: &str,
        date_end: &str,
    ) -> Result<Vec<SettlementRevenueDetail>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let date_start = date_start.to_owned();
        let date_end = date_end.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT order_id,amount,recognition_date,source
                 FROM revenue_records
                 WHERE tenant_id=?1 AND recognition_date>=?2 AND recognition_date<=?3
                 ORDER BY recognition_date ASC",
            )?;
            statement
                .query_map(params![tenant_id, date_start, date_end], |row| {
                    Ok(SettlementRevenueDetail {
                        order_id: row.get(0)?,
                        amount: row.get(1)?,
                        recognition_date: row.get(2)?,
                        source: row.get(3)?,
                    })
                })?
                .collect()
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

fn aggregate_sqlite(
    transaction: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    date_start: &str,
    date_end: &str,
) -> Result<(f64, f64, f64), RepositoryError> {
    let revenue: f64 = transaction
        .query_row(
            "SELECT COALESCE(SUM(amount),0) FROM revenue_records
             WHERE tenant_id=?1 AND recognition_date>=?2 AND recognition_date<=?3",
            params![tenant_id, &date_start[..10], &date_end[..10]],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    let deposits: f64 = transaction
        .query_row(
            "SELECT COALESCE(SUM(amount),0) FROM deposit_ledger
             WHERE tenant_id=?1 AND entry_type='collect' AND created_at>=?2 AND created_at<=?3",
            params![tenant_id, date_start, date_end],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    let refunds: f64 = transaction
        .query_row(
            "SELECT COALESCE(SUM(amount),0) FROM deposit_ledger
             WHERE tenant_id=?1 AND entry_type IN ('release','refund')
               AND created_at>=?2 AND created_at<=?3",
            params![tenant_id, date_start, date_end],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    Ok((revenue, deposits, refunds))
}

pub(in crate::repositories) fn map_mutation_error(
    error: RepositoryError,
) -> SettlementMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message == NOT_FOUND {
            return SettlementMutationError::NotFound;
        }
        if message == ALREADY_CONFIRMED {
            return SettlementMutationError::AlreadyConfirmed;
        }
    }
    SettlementMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn map_projection(row: &rusqlite::Row<'_>) -> rusqlite::Result<SettlementProjection> {
    Ok(SettlementProjection {
        id: row.get(0)?,
        period_type: row.get(1)?,
        period_key: row.get(2)?,
        total_revenue: row.get(3)?,
        total_deposits: row.get(4)?,
        total_refunds: row.get(5)?,
        confirmed: row.get::<_, i64>(6)? != 0,
        confirmed_at: row.get(7)?,
        tenant_id: row.get(8)?,
        created_at: row.get(9)?,
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

    use crate::repositories::{
        RepositoryProvider, SettlementMutationError, SqliteRepositoryProvider,
    };

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("settlement-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(
                tenant_id,
                Revision::new("settlement-test-revision").unwrap(),
            )
            .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_settlement_authority_preserves_scope_reaggregation_and_confirmation_freeze() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE settlements (
                    id TEXT PRIMARY KEY,
                    period_type TEXT NOT NULL,
                    period_key TEXT NOT NULL,
                    total_revenue REAL NOT NULL DEFAULT 0,
                    total_deposits REAL NOT NULL DEFAULT 0,
                    total_refunds REAL NOT NULL DEFAULT 0,
                    confirmed INTEGER NOT NULL DEFAULT 0,
                    confirmed_at TEXT,
                    tenant_id TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    UNIQUE(period_type,period_key,tenant_id)
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
                CREATE TABLE deposit_ledger (
                    id TEXT PRIMARY KEY,
                    deposit_id TEXT NOT NULL,
                    order_id TEXT NOT NULL,
                    entry_type TEXT NOT NULL,
                    amount REAL NOT NULL,
                    balance_after REAL NOT NULL,
                    description TEXT NOT NULL,
                    operator TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                INSERT INTO revenue_records VALUES
                    ('rev-a','order-a',100,'2026-09-29','order_complete','now','tenant-a'),
                    ('rev-b','order-b',900,'2026-09-29','order_complete','now','tenant-b');
                INSERT INTO deposit_ledger VALUES
                    ('dep-a-1','deposit-a','order-a','collect',50,50,'','','2026-09-29T10:00:00','tenant-a'),
                    ('dep-a-2','deposit-a','order-a','release',20,30,'','','2026-09-29T11:00:00','tenant-a'),
                    ('dep-a-3','deposit-a','order-a','refund',10,20,'','','2026-09-29T12:00:00','tenant-a'),
                    ('dep-b-1','deposit-b','order-b','collect',500,500,'','','2026-09-29T10:00:00','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool.clone());
        let scoped_a = provider.bind(&context("tenant-a", "settlement-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "settlement-b")).unwrap();

        let first = scoped_a
            .settlements()
            .generate(
                "daily",
                "2026-09-29",
                "2026-09-29T00:00:00",
                "2026-09-29T23:59:59",
                "2026-09-29T21:00:00+08:00",
            )
            .unwrap();
        assert_eq!(first.total_revenue, Some(100.0));
        assert_eq!(first.total_deposits, Some(50.0));
        assert_eq!(first.total_refunds, Some(30.0));

        let other = scoped_b
            .settlements()
            .generate(
                "daily",
                "2026-09-29",
                "2026-09-29T00:00:00",
                "2026-09-29T23:59:59",
                "2026-09-29T21:00:00+08:00",
            )
            .unwrap();
        assert_eq!(other.total_revenue, Some(900.0));
        assert_eq!(other.total_deposits, Some(500.0));
        assert_ne!(other.settlement_id, first.settlement_id);

        let connection = pool.get().unwrap();
        connection
            .execute(
                "INSERT INTO revenue_records VALUES
                 ('rev-a-2','order-a-2',40,'2026-09-29','other','now','tenant-a')",
                [],
            )
            .unwrap();
        drop(connection);

        let regenerated = scoped_a
            .settlements()
            .generate(
                "daily",
                "2026-09-29",
                "2026-09-29T00:00:00",
                "2026-09-29T23:59:59",
                "2026-09-29T21:05:00+08:00",
            )
            .unwrap();
        assert_eq!(regenerated.settlement_id, first.settlement_id);
        assert_eq!(regenerated.total_revenue, Some(140.0));

        let cross_tenant = scoped_b
            .settlements()
            .confirm(&first.settlement_id, "2026-09-29T21:06:00+08:00");
        assert!(matches!(
            cross_tenant,
            Err(SettlementMutationError::NotFound)
        ));

        scoped_a
            .settlements()
            .confirm(&first.settlement_id, "2026-09-29T21:07:00+08:00")
            .unwrap();
        let repeated = scoped_a
            .settlements()
            .confirm(&first.settlement_id, "2026-09-29T21:08:00+08:00");
        assert!(matches!(
            repeated,
            Err(SettlementMutationError::AlreadyConfirmed)
        ));

        let connection = pool.get().unwrap();
        connection
            .execute(
                "INSERT INTO revenue_records VALUES
                 ('rev-a-3','order-a-3',60,'2026-09-29','other','now','tenant-a')",
                [],
            )
            .unwrap();
        drop(connection);

        let frozen = scoped_a
            .settlements()
            .generate(
                "daily",
                "2026-09-29",
                "2026-09-29T00:00:00",
                "2026-09-29T23:59:59",
                "2026-09-29T21:09:00+08:00",
            )
            .unwrap();
        assert_eq!(frozen.settlement_id, first.settlement_id);
        assert!(frozen.existing);
        assert!(frozen.confirmed);
        assert_eq!(frozen.total_revenue, None);

        let persisted = scoped_a
            .settlements()
            .get_by_period("daily", "2026-09-29")
            .unwrap()
            .unwrap();
        assert!(persisted.confirmed);
        assert_eq!(persisted.total_revenue, 140.0);
        assert_eq!(
            scoped_a
                .settlements()
                .list(None, None, 1, 20)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            scoped_b
                .settlements()
                .list(None, None, 1, 20)
                .unwrap()
                .total,
            1
        );

        let details = scoped_a
            .settlements()
            .revenue_details("2026-09-29", "2026-09-29")
            .unwrap();
        assert_eq!(details.len(), 3);
        assert!(details.iter().all(|row| row.amount != 900.0));
    }
}

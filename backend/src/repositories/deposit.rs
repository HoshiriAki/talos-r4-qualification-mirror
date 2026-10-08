use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const ORDER_NOT_FOUND_PREFIX: &str = "deposit-order-not-found:";
const ALREADY_PAID_PREFIX: &str = "deposit-already-paid:";
const NOT_FOUND_PREFIX: &str = "deposit-not-found:";
const ALREADY_RELEASED_PREFIX: &str = "deposit-already-released:";
const NOT_PAID_PREFIX: &str = "deposit-not-paid:";
const EXCEEDS_AVAILABLE_PREFIX: &str = "deposit-exceeds-available:";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositProjection {
    pub id: String,
    pub order_id: String,
    pub amount: f64,
    pub status: String,
    pub paid_at: Option<String>,
    pub released_at: Option<String>,
    pub forfeited_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositLedgerProjection {
    pub entry_type: String,
    pub amount: f64,
    pub balance_after: f64,
    pub description: String,
    pub operator: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DepositCalculateOutcome {
    pub deposit: DepositProjection,
    pub existing: bool,
    pub device_count: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DepositCollectOutcome {
    pub deposit_id: String,
    pub order_id: String,
    pub amount: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DepositReleaseOutcome {
    pub deposit_id: String,
    pub order_id: String,
    pub released_amount: f64,
    pub forfeited_amount: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DepositForfeitOutcome {
    pub deposit_id: String,
    pub order_id: String,
    pub forfeited_amount: f64,
    pub total_forfeited: f64,
    pub remaining_balance: f64,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositDetailProjection {
    pub deposit: Option<DepositProjection>,
    pub ledger: Vec<DepositLedgerProjection>,
}

#[derive(Debug, thiserror::Error)]
pub enum DepositMutationError {
    #[error("order not found")]
    OrderNotFound,
    #[error("deposit already paid or released")]
    AlreadyPaid,
    #[error("deposit not found")]
    NotFound,
    #[error("deposit already released")]
    AlreadyReleased,
    #[error("deposit status is not releasable: {0}")]
    NotPaid(String),
    #[error("requested amount {requested} exceeds available {available}")]
    ExceedsAvailable { requested: f64, available: f64 },
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteDepositRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteDepositRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn calculate(
        &self,
        order_id: &str,
        default_per_device: f64,
        now: &str,
    ) -> Result<DepositCalculateOutcome, DepositMutationError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let order_exists = transaction
                    .query_row(
                        "SELECT 1 FROM orders WHERE id=?1 AND tenant_id=?2",
                        params![order_id, tenant_id],
                        |_| Ok(()),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .is_some();
                if !order_exists {
                    return Err(contract(format!("{ORDER_NOT_FOUND_PREFIX}{order_id}")));
                }

                let existing = transaction
                    .query_row(
                        "SELECT id,order_id,amount,status,paid_at,released_at,forfeited_at,created_at,updated_at
                         FROM deposits
                         WHERE order_id=?1 AND tenant_id=?2
                         LIMIT 1",
                        params![order_id, tenant_id],
                        map_deposit,
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if let Some(deposit) = existing {
                    return Ok(DepositCalculateOutcome {
                        deposit,
                        existing: true,
                        device_count: None,
                    });
                }

                let device_count: i64 = transaction
                    .query_row(
                        "SELECT COUNT(*)
                         FROM order_devices od
                         JOIN devices d ON d.serialNo=od.serialNo
                         WHERE od.orderId=?1
                           AND od.tenant_id=?2
                           AND d.tenant_id=?2",
                        params![order_id, tenant_id],
                        |row| row.get(0),
                    )
                    .map_err(sqlite_error)?;
                let amount = if device_count > 0 {
                    device_count as f64 * default_per_device
                } else {
                    default_per_device
                };
                let id = Uuid::new_v4().to_string();
                transaction
                    .execute(
                        "INSERT INTO deposits
                         (id,order_id,amount,status,created_at,updated_at,tenant_id)
                         VALUES (?1,?2,?3,'pending',?4,?4,?5)",
                        params![id, order_id, amount, now, tenant_id],
                    )
                    .map_err(sqlite_error)?;

                Ok(DepositCalculateOutcome {
                    deposit: DepositProjection {
                        id,
                        order_id,
                        amount,
                        status: "pending".into(),
                        paid_at: None,
                        released_at: None,
                        forfeited_at: None,
                        created_at: now.clone(),
                        updated_at: now,
                    },
                    existing: false,
                    device_count: Some(device_count),
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn collect(
        &self,
        order_id: &str,
        amount: f64,
        operator: &str,
        now: &str,
    ) -> Result<DepositCollectOutcome, DepositMutationError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let order_exists = transaction
                    .query_row(
                        "SELECT 1 FROM orders WHERE id=?1 AND tenant_id=?2",
                        params![order_id, tenant_id],
                        |_| Ok(()),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .is_some();
                if !order_exists {
                    return Err(contract(format!("{ORDER_NOT_FOUND_PREFIX}{order_id}")));
                }

                let current = transaction
                    .query_row(
                        "SELECT id,status,amount FROM deposits
                         WHERE order_id=?1 AND tenant_id=?2 LIMIT 1",
                        params![order_id, tenant_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, f64>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?;

                let (deposit_id, status, current_amount) = match current {
                    Some(row) => row,
                    None => {
                        let id = Uuid::new_v4().to_string();
                        transaction
                            .execute(
                                "INSERT INTO deposits
                                 (id,order_id,amount,status,created_at,updated_at,tenant_id)
                                 VALUES (?1,?2,0,'pending',?3,?3,?4)",
                                params![id, order_id, now, tenant_id],
                            )
                            .map_err(sqlite_error)?;
                        (id, "pending".into(), 0.0)
                    }
                };

                if status == "paid" || status == "released" {
                    return Err(contract(format!("{ALREADY_PAID_PREFIX}{order_id}")));
                }

                let new_amount = if current_amount > 0.0 {
                    current_amount
                } else {
                    amount
                };
                transaction
                    .execute(
                        "UPDATE deposits
                         SET amount=?1,status='paid',paid_at=?2,updated_at=?2
                         WHERE id=?3 AND tenant_id=?4",
                        params![new_amount, now, deposit_id, tenant_id],
                    )
                    .map_err(sqlite_error)?;

                insert_ledger_sqlite(
                    transaction,
                    &tenant_id,
                    &deposit_id,
                    &order_id,
                    "collect",
                    new_amount,
                    new_amount,
                    &format!("收取押金 ¥{new_amount:.2}"),
                    &operator,
                    &now,
                )?;
                insert_accounting_pair_sqlite(
                    transaction,
                    &tenant_id,
                    &order_id,
                    "cash",
                    "deposit",
                    new_amount,
                    &format!("押金收款 ¥{new_amount:.2}"),
                    &format!("押金负债 ¥{new_amount:.2}"),
                    &now,
                )?;

                Ok(DepositCollectOutcome {
                    deposit_id,
                    order_id,
                    amount: new_amount,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn release(
        &self,
        order_id: &str,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<DepositReleaseOutcome, DepositMutationError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        let reason = reason.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let row = transaction
                    .query_row(
                        "SELECT id,status,amount,released_at
                         FROM deposits WHERE order_id=?1 AND tenant_id=?2 LIMIT 1",
                        params![order_id, tenant_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, f64>(2)?,
                                row.get::<_, Option<String>>(3)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(format!("{NOT_FOUND_PREFIX}{order_id}")))?;
                let (deposit_id, status, total_amount, released_at) = row;
                if released_at.is_some() {
                    return Err(contract(format!("{ALREADY_RELEASED_PREFIX}{order_id}")));
                }
                if status != "paid" && status != "partially_forfeited" {
                    return Err(contract(format!("{NOT_PAID_PREFIX}{status}")));
                }

                let forfeited_total: f64 = transaction
                    .query_row(
                        "SELECT COALESCE(SUM(amount),0)
                         FROM deposit_ledger
                         WHERE deposit_id=?1 AND tenant_id=?2 AND entry_type='forfeit'",
                        params![deposit_id, tenant_id],
                        |row| row.get(0),
                    )
                    .map_err(sqlite_error)?;
                let release_amount = total_amount - forfeited_total;

                transaction
                    .execute(
                        "UPDATE deposits
                         SET status='released',released_at=?1,updated_at=?1
                         WHERE id=?2 AND tenant_id=?3",
                        params![now, deposit_id, tenant_id],
                    )
                    .map_err(sqlite_error)?;

                let description = if reason.is_empty() {
                    format!("释放押金 ¥{release_amount:.2}")
                } else {
                    format!("释放押金 ¥{release_amount:.2} — {reason}")
                };
                insert_ledger_sqlite(
                    transaction,
                    &tenant_id,
                    &deposit_id,
                    &order_id,
                    "release",
                    release_amount,
                    0.0,
                    &description,
                    &operator,
                    &now,
                )?;
                insert_accounting_pair_sqlite(
                    transaction,
                    &tenant_id,
                    &order_id,
                    "deposit",
                    "cash",
                    release_amount,
                    &format!("押金释放 ¥{release_amount:.2}"),
                    &format!("押金退回 ¥{release_amount:.2}"),
                    &now,
                )?;

                Ok(DepositReleaseOutcome {
                    deposit_id,
                    order_id,
                    released_amount: release_amount,
                    forfeited_amount: forfeited_total,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn forfeit(
        &self,
        order_id: &str,
        amount: f64,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<DepositForfeitOutcome, DepositMutationError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        let reason = reason.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let row = transaction
                    .query_row(
                        "SELECT id,status,amount FROM deposits
                         WHERE order_id=?1 AND tenant_id=?2 LIMIT 1",
                        params![order_id, tenant_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, f64>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(format!("{NOT_FOUND_PREFIX}{order_id}")))?;
                let (deposit_id, status, total_amount) = row;
                if status != "paid" && status != "partially_forfeited" {
                    return Err(contract(format!("{NOT_PAID_PREFIX}{status}")));
                }

                let already_forfeited: f64 = transaction
                    .query_row(
                        "SELECT COALESCE(SUM(amount),0)
                         FROM deposit_ledger
                         WHERE deposit_id=?1 AND tenant_id=?2 AND entry_type='forfeit'",
                        params![deposit_id, tenant_id],
                        |row| row.get(0),
                    )
                    .map_err(sqlite_error)?;
                let available = total_amount - already_forfeited;
                if amount > available {
                    return Err(contract(format!(
                        "{EXCEEDS_AVAILABLE_PREFIX}{amount}:{available}"
                    )));
                }

                let total_forfeited = already_forfeited + amount;
                let remaining_balance = total_amount - total_forfeited;
                let new_status = if remaining_balance <= 0.01 {
                    "forfeited"
                } else {
                    "partially_forfeited"
                };

                transaction
                    .execute(
                        "UPDATE deposits
                         SET status=?1,forfeited_at=?2,updated_at=?2
                         WHERE id=?3 AND tenant_id=?4",
                        params![new_status, now, deposit_id, tenant_id],
                    )
                    .map_err(sqlite_error)?;

                let description = if reason.is_empty() {
                    format!("罚没押金 ¥{amount:.2}")
                } else {
                    format!("罚没押金 ¥{amount:.2} — {reason}")
                };
                insert_ledger_sqlite(
                    transaction,
                    &tenant_id,
                    &deposit_id,
                    &order_id,
                    "forfeit",
                    amount,
                    remaining_balance,
                    &description,
                    &operator,
                    &now,
                )?;
                insert_accounting_pair_sqlite(
                    transaction,
                    &tenant_id,
                    &order_id,
                    "deposit",
                    "revenue",
                    amount,
                    &format!("押金罚没负债减少 ¥{amount:.2}"),
                    &format!("押金罚没收入 ¥{amount:.2}"),
                    &now,
                )?;

                Ok(DepositForfeitOutcome {
                    deposit_id,
                    order_id,
                    forfeited_amount: amount,
                    total_forfeited,
                    remaining_balance,
                    status: new_status.into(),
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn get(
        &self,
        order_id: &str,
    ) -> Result<DepositDetailProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        self.session.read(move |connection| {
            let deposit = connection
                .query_row(
                    "SELECT id,order_id,amount,status,paid_at,released_at,forfeited_at,created_at,updated_at
                     FROM deposits WHERE order_id=?1 AND tenant_id=?2 LIMIT 1",
                    params![order_id, tenant_id],
                    map_deposit,
                )
                .optional()?;

            let ledger = if let Some(deposit) = &deposit {
                let mut statement = connection.prepare(
                    "SELECT entry_type,amount,balance_after,description,operator,created_at
                     FROM deposit_ledger
                     WHERE deposit_id=?1 AND tenant_id=?2
                     ORDER BY created_at ASC",
                )?;
                statement
                    .query_map(params![deposit.id, tenant_id], map_ledger)?
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                Vec::new()
            };

            Ok(DepositDetailProjection { deposit, ledger })
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

fn insert_ledger_sqlite(
    transaction: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    deposit_id: &str,
    order_id: &str,
    entry_type: &str,
    amount: f64,
    balance_after: f64,
    description: &str,
    operator: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    transaction
        .execute(
            "INSERT INTO deposit_ledger
             (id,deposit_id,order_id,entry_type,amount,balance_after,description,operator,created_at,tenant_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                Uuid::new_v4().to_string(),
                deposit_id,
                order_id,
                entry_type,
                amount,
                balance_after,
                description,
                operator,
                now,
                tenant_id,
            ],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn insert_accounting_pair_sqlite(
    transaction: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    order_id: &str,
    debit_account: &str,
    credit_account: &str,
    amount: f64,
    debit_description: &str,
    credit_description: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    for (entry_type, account, description) in [
        ("debit", debit_account, debit_description),
        ("credit", credit_account, credit_description),
    ] {
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
    }
    Ok(())
}

pub(in crate::repositories) fn map_mutation_error(error: RepositoryError) -> DepositMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message.starts_with(ORDER_NOT_FOUND_PREFIX) {
            return DepositMutationError::OrderNotFound;
        }
        if message.starts_with(ALREADY_PAID_PREFIX) {
            return DepositMutationError::AlreadyPaid;
        }
        if message.starts_with(NOT_FOUND_PREFIX) {
            return DepositMutationError::NotFound;
        }
        if message.starts_with(ALREADY_RELEASED_PREFIX) {
            return DepositMutationError::AlreadyReleased;
        }
        if let Some(status) = message.strip_prefix(NOT_PAID_PREFIX) {
            return DepositMutationError::NotPaid(status.to_owned());
        }
        if let Some(values) = message.strip_prefix(EXCEEDS_AVAILABLE_PREFIX) {
            let mut parts = values.splitn(2, ':');
            if let (Some(requested), Some(available)) = (parts.next(), parts.next())
                && let (Ok(requested), Ok(available)) =
                    (requested.parse::<f64>(), available.parse::<f64>())
            {
                return DepositMutationError::ExceedsAvailable {
                    requested,
                    available,
                };
            }
        }
    }
    DepositMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn map_deposit(row: &rusqlite::Row<'_>) -> rusqlite::Result<DepositProjection> {
    Ok(DepositProjection {
        id: row.get(0)?,
        order_id: row.get(1)?,
        amount: row.get(2)?,
        status: row.get(3)?,
        paid_at: row.get(4)?,
        released_at: row.get(5)?,
        forfeited_at: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn map_ledger(row: &rusqlite::Row<'_>) -> rusqlite::Result<DepositLedgerProjection> {
    Ok(DepositLedgerProjection {
        entry_type: row.get(0)?,
        amount: row.get(1)?,
        balance_after: row.get(2)?,
        description: row.get(3)?,
        operator: row.get(4)?,
        created_at: row.get(5)?,
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

    use crate::repositories::{DepositMutationError, RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("deposit-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("deposit-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_deposit_authority_preserves_scope_and_ledger() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE orders (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE devices (
                    id TEXT PRIMARY KEY,
                    serialNo TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE order_devices (
                    id TEXT PRIMARY KEY,
                    orderId TEXT NOT NULL,
                    serialNo TEXT NOT NULL,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE deposits (
                    id TEXT PRIMARY KEY,
                    order_id TEXT NOT NULL,
                    amount REAL NOT NULL,
                    status TEXT NOT NULL,
                    paid_at TEXT,
                    released_at TEXT,
                    forfeited_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
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
                INSERT INTO orders VALUES ('order-a', 'tenant-a');
                INSERT INTO devices VALUES
                    ('device-a1', 'SER-A1', 'tenant-a'),
                    ('device-a2', 'SER-A2', 'tenant-a');
                INSERT INTO order_devices VALUES
                    ('link-a1', 'order-a', 'SER-A1', 'tenant-a'),
                    ('link-a2', 'order-a', 'SER-A2', 'tenant-a');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let scoped_a = provider.bind(&context("tenant-a", "deposit-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "deposit-b")).unwrap();

        let calculated = scoped_a
            .deposits()
            .calculate("order-a", 2000.0, "2026-09-29T17:00:00+08:00")
            .unwrap();
        assert_eq!(calculated.device_count, Some(2));
        assert_eq!(calculated.deposit.amount, 4000.0);

        let foreign = scoped_b
            .deposits()
            .calculate("order-a", 2000.0, "2026-09-29T17:01:00+08:00");
        assert!(matches!(foreign, Err(DepositMutationError::OrderNotFound)));

        let collected = scoped_a
            .deposits()
            .collect(
                "order-a",
                4500.0,
                "deposit-test-actor",
                "2026-09-29T17:02:00+08:00",
            )
            .unwrap();
        assert_eq!(collected.amount, 4000.0);

        let forfeited = scoped_a
            .deposits()
            .forfeit(
                "order-a",
                500.0,
                "damage",
                "deposit-test-actor",
                "2026-09-29T17:03:00+08:00",
            )
            .unwrap();
        assert_eq!(forfeited.remaining_balance, 3500.0);

        let released = scoped_a
            .deposits()
            .release(
                "order-a",
                "close",
                "deposit-test-actor",
                "2026-09-29T17:04:00+08:00",
            )
            .unwrap();
        assert_eq!(released.released_amount, 3500.0);

        let detail = scoped_a.deposits().get("order-a").unwrap();
        assert_eq!(detail.deposit.unwrap().status, "released");
        assert_eq!(
            detail
                .ledger
                .iter()
                .map(|entry| entry.entry_type.as_str())
                .collect::<Vec<_>>(),
            vec!["collect", "forfeit", "release"]
        );
        assert!(
            scoped_b
                .deposits()
                .get("order-a")
                .unwrap()
                .deposit
                .is_none()
        );
    }
}

use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::Serialize;
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const DEPOSIT_NOT_FOUND_PREFIX: &str = "refund-deposit-not-found:";
const NOT_FOUND_PREFIX: &str = "refund-not-found:";
const NOT_PENDING_PREFIX: &str = "refund-not-pending:";
const NOT_APPROVED_PREFIX: &str = "refund-not-approved:";

#[derive(Debug, Clone, PartialEq)]
pub struct RefundRequestOutcome {
    pub refund_id: String,
    pub deposit_id: String,
    pub order_id: String,
    pub amount: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefundStatusOutcome {
    pub refund_id: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefundExecuteOutcome {
    pub refund_id: String,
    pub amount: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundListItem {
    pub id: String,
    pub deposit_id: String,
    pub order_id: String,
    pub amount: f64,
    pub reason: String,
    pub status: String,
    pub requested_by: String,
    pub approved_by: Option<String>,
    pub rejected_by: Option<String>,
    pub executed_by: Option<String>,
    pub approved_at: Option<String>,
    pub rejected_at: Option<String>,
    pub executed_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefundListProjection {
    pub refunds: Vec<RefundListItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum RefundMutationError {
    #[error("deposit not found")]
    DepositNotFound,
    #[error("refund not found")]
    NotFound,
    #[error("refund status is not pending: {0}")]
    NotPending(String),
    #[error("refund status is not approved: {0}")]
    NotApproved(String),
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteRefundRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteRefundRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn request(
        &self,
        order_id: &str,
        amount: f64,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundRequestOutcome, RefundMutationError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        let reason = reason.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let deposit_id = transaction
                    .query_row(
                        "SELECT id FROM deposits
                         WHERE order_id=?1 AND tenant_id=?2
                         LIMIT 1",
                        params![order_id, tenant_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(format!("{DEPOSIT_NOT_FOUND_PREFIX}{order_id}")))?;

                let refund_id = Uuid::new_v4().to_string();
                transaction
                    .execute(
                        "INSERT INTO refunds
                         (id,deposit_id,order_id,amount,reason,status,requested_by,created_at,updated_at,tenant_id)
                         VALUES (?1,?2,?3,?4,?5,'pending',?6,?7,?7,?8)",
                        params![
                            refund_id,
                            deposit_id,
                            order_id,
                            amount,
                            reason,
                            operator,
                            now,
                            tenant_id,
                        ],
                    )
                    .map_err(sqlite_error)?;

                Ok(RefundRequestOutcome {
                    refund_id,
                    deposit_id,
                    order_id,
                    amount,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn approve(
        &self,
        refund_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundStatusOutcome, RefundMutationError> {
        self.transition_pending(refund_id, operator, "", now, true)
    }

    pub(in crate::repositories) fn reject(
        &self,
        refund_id: &str,
        reason: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundStatusOutcome, RefundMutationError> {
        self.transition_pending(refund_id, operator, reason, now, false)
    }

    fn transition_pending(
        &self,
        refund_id: &str,
        operator: &str,
        reason: &str,
        now: &str,
        approve: bool,
    ) -> Result<RefundStatusOutcome, RefundMutationError> {
        let tenant_id = self.tenant_id();
        let refund_id = refund_id.to_owned();
        let operator = operator.to_owned();
        let reason = reason.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let status = transaction
                    .query_row(
                        "SELECT status FROM refunds
                         WHERE id=?1 AND tenant_id=?2
                         LIMIT 1",
                        params![refund_id, tenant_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(format!("{NOT_FOUND_PREFIX}{refund_id}")))?;
                if status != "pending" {
                    return Err(contract(format!("{NOT_PENDING_PREFIX}{status}")));
                }

                let next_status = if approve { "approved" } else { "rejected" };
                if approve {
                    transaction
                        .execute(
                            "UPDATE refunds
                             SET status='approved',approved_by=?1,approved_at=?2,updated_at=?2
                             WHERE id=?3 AND tenant_id=?4",
                            params![operator, now, refund_id, tenant_id],
                        )
                        .map_err(sqlite_error)?;
                } else {
                    transaction
                        .execute(
                            "UPDATE refunds
                             SET status='rejected',rejected_by=?1,rejected_at=?2,
                                 reason=CASE WHEN ?3!='' THEN reason || ' | 驳回: ' || ?3 ELSE reason END,
                                 updated_at=?2
                             WHERE id=?4 AND tenant_id=?5",
                            params![operator, now, reason, refund_id, tenant_id],
                        )
                        .map_err(sqlite_error)?;
                }

                Ok(RefundStatusOutcome {
                    refund_id,
                    status: next_status.into(),
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn execute(
        &self,
        refund_id: &str,
        operator: &str,
        now: &str,
    ) -> Result<RefundExecuteOutcome, RefundMutationError> {
        let tenant_id = self.tenant_id();
        let refund_id = refund_id.to_owned();
        let operator = operator.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let row = transaction
                    .query_row(
                        "SELECT deposit_id,order_id,status,amount
                         FROM refunds
                         WHERE id=?1 AND tenant_id=?2
                         LIMIT 1",
                        params![refund_id, tenant_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, f64>(3)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(format!("{NOT_FOUND_PREFIX}{refund_id}")))?;
                let (deposit_id, order_id, status, amount) = row;
                if status != "approved" {
                    return Err(contract(format!("{NOT_APPROVED_PREFIX}{status}")));
                }

                transaction
                    .execute(
                        "UPDATE refunds
                         SET status='executed',executed_by=?1,executed_at=?2,updated_at=?2
                         WHERE id=?3 AND tenant_id=?4",
                        params![operator, now, refund_id, tenant_id],
                    )
                    .map_err(sqlite_error)?;

                transaction
                    .execute(
                        "INSERT INTO deposit_ledger
                         (id,deposit_id,order_id,entry_type,amount,balance_after,description,operator,created_at,tenant_id)
                         VALUES (?1,?2,?3,'refund',?4,0,?5,?6,?7,?8)",
                        params![
                            Uuid::new_v4().to_string(),
                            deposit_id,
                            order_id,
                            amount,
                            format!("退款执行 ¥{amount:.2}"),
                            operator,
                            now,
                            tenant_id,
                        ],
                    )
                    .map_err(sqlite_error)?;

                Ok(RefundExecuteOutcome { refund_id, amount })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn list(
        &self,
        order_id: Option<&str>,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<RefundListProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut conditions = vec!["r.tenant_id=?1".to_string()];
            let mut values = vec![tenant_id];
            if let Some(order_id) = order_id {
                values.push(order_id);
                conditions.push(format!("r.order_id=?{}", values.len()));
            }
            if let Some(status) = status {
                values.push(status);
                conditions.push(format!("r.status=?{}", values.len()));
            }
            let where_clause = conditions.join(" AND ");

            let count_sql = format!("SELECT COUNT(*) FROM refunds r WHERE {where_clause}");
            let total: i64 =
                connection.query_row(&count_sql, params_from_iter(values.iter()), |row| {
                    row.get(0)
                })?;

            let data_sql = format!(
                "SELECT r.id,r.deposit_id,r.order_id,r.amount,r.reason,r.status,
                        r.requested_by,r.approved_by,r.rejected_by,r.executed_by,
                        r.approved_at,r.rejected_at,r.executed_at,r.created_at
                 FROM refunds r
                 WHERE {where_clause}
                 ORDER BY r.created_at DESC
                 LIMIT {page_size} OFFSET {offset}"
            );
            let mut statement = connection.prepare(&data_sql)?;
            let refunds = statement
                .query_map(params_from_iter(values.iter()), map_list_item)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(RefundListProjection {
                refunds,
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

pub(in crate::repositories) fn map_mutation_error(error: RepositoryError) -> RefundMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message.starts_with(DEPOSIT_NOT_FOUND_PREFIX) {
            return RefundMutationError::DepositNotFound;
        }
        if message.starts_with(NOT_FOUND_PREFIX) {
            return RefundMutationError::NotFound;
        }
        if let Some(status) = message.strip_prefix(NOT_PENDING_PREFIX) {
            return RefundMutationError::NotPending(status.to_owned());
        }
        if let Some(status) = message.strip_prefix(NOT_APPROVED_PREFIX) {
            return RefundMutationError::NotApproved(status.to_owned());
        }
    }
    RefundMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn map_list_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<RefundListItem> {
    Ok(RefundListItem {
        id: row.get(0)?,
        deposit_id: row.get(1)?,
        order_id: row.get(2)?,
        amount: row.get(3)?,
        reason: row.get(4)?,
        status: row.get(5)?,
        requested_by: row.get(6)?,
        approved_by: row.get(7)?,
        rejected_by: row.get(8)?,
        executed_by: row.get(9)?,
        approved_at: row.get(10)?,
        rejected_at: row.get(11)?,
        executed_at: row.get(12)?,
        created_at: row.get(13)?,
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

    use crate::repositories::{RefundMutationError, RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("refund-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("refund-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_refund_authority_preserves_scope_transitions_and_ledger() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE deposits (
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
                CREATE TABLE refunds (
                    id TEXT PRIMARY KEY,
                    deposit_id TEXT NOT NULL,
                    order_id TEXT NOT NULL,
                    amount REAL NOT NULL,
                    reason TEXT NOT NULL,
                    status TEXT NOT NULL,
                    requested_by TEXT NOT NULL,
                    approved_by TEXT,
                    rejected_by TEXT,
                    executed_by TEXT,
                    approved_at TEXT,
                    rejected_at TEXT,
                    executed_at TEXT,
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
                INSERT INTO deposits VALUES
                    ('deposit-a','order-a',4000,'paid','now',NULL,NULL,'now','now','tenant-a'),
                    ('deposit-b','order-b',2000,'paid','now',NULL,NULL,'now','now','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let scoped_a = provider.bind(&context("tenant-a", "refund-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "refund-b")).unwrap();

        let requested = scoped_a
            .refunds()
            .request(
                "order-a",
                1000.0,
                "customer request",
                "refund-test-actor",
                "2026-09-29T18:40:00+08:00",
            )
            .unwrap();
        assert_eq!(requested.deposit_id, "deposit-a");
        assert_eq!(scoped_a.refunds().list(None, None, 1, 20).unwrap().total, 1);
        assert_eq!(scoped_b.refunds().list(None, None, 1, 20).unwrap().total, 0);

        let cross_tenant = scoped_b.refunds().approve(
            &requested.refund_id,
            "refund-test-actor",
            "2026-09-29T18:41:00+08:00",
        );
        assert!(matches!(cross_tenant, Err(RefundMutationError::NotFound)));

        let premature = scoped_a.refunds().execute_refund(
            &requested.refund_id,
            "refund-test-actor",
            "2026-09-29T18:42:00+08:00",
        );
        assert!(matches!(
            premature,
            Err(RefundMutationError::NotApproved(status)) if status == "pending"
        ));

        scoped_a
            .refunds()
            .approve(
                &requested.refund_id,
                "refund-test-actor",
                "2026-09-29T18:43:00+08:00",
            )
            .unwrap();
        scoped_a
            .refunds()
            .execute_refund(
                &requested.refund_id,
                "refund-test-actor",
                "2026-09-29T18:44:00+08:00",
            )
            .unwrap();

        assert_eq!(
            scoped_a
                .refunds()
                .list(Some("order-a"), Some("executed"), 1, 20)
                .unwrap()
                .total,
            1
        );
        let ledger = scoped_a.deposits().get("order-a").unwrap();
        assert_eq!(
            ledger
                .ledger
                .iter()
                .map(|entry| entry.entry_type.as_str())
                .collect::<Vec<_>>(),
            vec!["refund"]
        );
    }
}

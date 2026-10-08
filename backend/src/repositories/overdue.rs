use chrono::NaiveDate;
use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const NOT_FOUND: &str = "overdue-not-found";
const STATUS_INVALID_PREFIX: &str = "overdue-status-invalid:";
const ALREADY_WAIVED: &str = "overdue-already-waived";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueFeeConfig {
    pub daily_rate: f64,
    pub max_days: i64,
    pub cap_multiplier: f64,
    pub grace_period_hours: i64,
}

impl Default for OverdueFeeConfig {
    fn default() -> Self {
        Self {
            daily_rate: 50.0,
            max_days: 30,
            cap_multiplier: 3.0,
            grace_period_hours: 4,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OverdueMutationError {
    #[error("overdue record not found")]
    NotFound,
    #[error("overdue record status is invalid: {0}")]
    StatusInvalid(String),
    #[error("overdue record is already waived")]
    AlreadyWaived,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteOverdueRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteOverdueRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn detect(
        &self,
        today: &str,
        now: &str,
    ) -> Result<Value, RepositoryError> {
        let tenant = self.tenant_id();
        let today = today.to_owned();
        let now = now.to_owned();

        self.session.write_immediate(move |tx| {
            let config = load_config_sqlite(tx, &tenant).map_err(sqlite_error)?;
            let mut stmt = tx
                .prepare(
                    "SELECT o.id,o.customer_id,o.endDate,c.display_name,
                            (SELECT cc.raw_value
                               FROM customer_contacts cc
                              WHERE cc.tenant_id=o.tenant_id
                                AND cc.customer_id=o.customer_id
                                AND cc.kind='phone'
                              ORDER BY cc.is_primary DESC,cc.created_at,cc.id
                              LIMIT 1)
                     FROM orders o
                     JOIN order_lifecycle l
                       ON l.tenant_id=o.tenant_id AND l.order_id=o.id
                     JOIN customers c
                       ON c.tenant_id=o.tenant_id AND c.id=o.customer_id
                     WHERE o.tenant_id=?1
                       AND o.customer_id IS NOT NULL
                       AND o.endDate < ?2
                       AND l.commercial_status NOT IN ('closed','cancelled')
                       AND l.fulfilment_status IN ('shipped','in_use','return_pending')
                     ORDER BY o.endDate,o.id",
                )
                .map_err(sqlite_error)?;
            let candidates = stmt
                .query_map(params![tenant, today], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                })
                .map_err(sqlite_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_error)?;
            drop(stmt);

            let mut records = Vec::new();
            let mut skipped_unresolved_contact = 0_i64;
            for (order_id, customer_id, expected_date, customer_name, phone) in candidates {
                let Some(customer_phone) = phone.filter(|value| !value.trim().is_empty()) else {
                    skipped_unresolved_contact += 1;
                    continue;
                };
                let days = days_between_public(&expected_date, &today).max(0);
                let (capped_days, total_fee) = calculate_fee_public(days, &config);

                let existing = tx
                    .query_row(
                        "SELECT id,status,escalation_level
                           FROM overdue_records
                          WHERE tenant_id=?1 AND order_id=?2
                            AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')
                          ORDER BY id DESC LIMIT 1",
                        params![tenant, order_id],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, i64>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?;

                let (record_id, action, status, level) =
                    if let Some((record_id, status, level)) = existing {
                        tx.execute(
                            "UPDATE overdue_records
                                SET customer_id=?1,customer_name=?2,customer_phone=?3,
                                    expected_return_date=?4,days_overdue=?5,daily_rate=?6,
                                    total_fee=?7,updated_at=?8
                              WHERE tenant_id=?9 AND id=?10",
                            params![
                                customer_id,
                                customer_name,
                                customer_phone,
                                expected_date,
                                capped_days,
                                config.daily_rate,
                                total_fee,
                                now,
                                tenant,
                                record_id
                            ],
                        )
                        .map_err(sqlite_error)?;
                        (record_id, "updated", status, level)
                    } else {
                        tx.execute(
                            "INSERT INTO overdue_records
                             (tenant_id,order_id,customer_id,customer_name,customer_phone,
                              expected_return_date,days_overdue,daily_rate,total_fee,status,
                              created_at,updated_at)
                             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'active',?10,?10)",
                            params![
                                tenant,
                                order_id,
                                customer_id,
                                customer_name,
                                customer_phone,
                                expected_date,
                                capped_days,
                                config.daily_rate,
                                total_fee,
                                now
                            ],
                        )
                        .map_err(sqlite_error)?;
                        (tx.last_insert_rowid(), "created", "active".into(), 0)
                    };

                let escalation = if days >= 1 && config.grace_period_hours == 0 {
                    escalate_one_sqlite(
                        tx, &tenant, record_id, &order_id, days, &status, level, &now,
                    )?
                } else {
                    None
                };

                records.push(json!({
                    "id": record_id,
                    "orderId": order_id,
                    "customerName": customer_name,
                    "customerPhone": customer_phone,
                    "expectedReturnDate": expected_date,
                    "daysOverdue": capped_days,
                    "totalFee": total_fee,
                    "action": action,
                    "escalation": escalation,
                }));
            }

            Ok(json!({
                "detected": records.len(),
                "records": records,
                "scannedAt": now,
                "skippedUnresolvedContact": skipped_unresolved_contact,
            }))
        })
    }

    pub(in crate::repositories) fn calc(
        &self,
        order_id: &str,
        today: &str,
    ) -> Result<Value, RepositoryError> {
        let tenant = self.tenant_id();
        let order_id = order_id.to_owned();
        let today = today.to_owned();
        self.session.read(move |connection| {
            let config = load_config_sqlite(connection, &tenant)?;
            let (expected_date, fulfilment_status): (String, String) = connection.query_row(
                "SELECT o.endDate,l.fulfilment_status
                       FROM orders o
                       JOIN order_lifecycle l
                         ON l.tenant_id=o.tenant_id AND l.order_id=o.id
                      WHERE o.tenant_id=?1 AND o.id=?2",
                params![tenant, order_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let days = days_between_public(&expected_date, &today).max(0);
            let raw_fee = days as f64 * config.daily_rate;
            let cap_amount = config.daily_rate * config.max_days as f64 * config.cap_multiplier;
            let actual_fee = raw_fee.min(cap_amount).max(0.0);
            Ok(json!({
                "orderId": order_id,
                "daysOverdue": days,
                "dailyRate": config.daily_rate,
                "totalFee": raw_fee,
                "capAmount": cap_amount,
                "actualFee": actual_fee,
                "orderStatus": fulfilment_status,
            }))
        })
    }

    /// Compatibility command only. The legacy implementation attempted to write
    /// nonexistent pricing-snapshot columns and then marked the overdue fee as
    /// paid without an external financial effect. R3 settlement owns charge
    /// authorization, so this method deliberately returns a delegation fact and
    /// does not fabricate payment state.
    pub(in crate::repositories) fn apply_delegated(
        &self,
        overdue_id: i64,
    ) -> Result<Value, OverdueMutationError> {
        let tenant = self.tenant_id();
        self.session
            .read(move |connection| {
                let row = connection
                    .query_row(
                        "SELECT CAST(order_id AS TEXT),total_fee,waived_amount,paid_amount,status
                           FROM overdue_records
                          WHERE tenant_id=?1 AND id=?2",
                        params![tenant, overdue_id],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, f64>(1)?,
                                row.get::<_, f64>(2)?,
                                row.get::<_, f64>(3)?,
                                row.get::<_, String>(4)?,
                            ))
                        },
                    )
                    .optional()?
                    .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                Ok(row)
            })
            .map_err(|error| match error {
                RepositoryError::Sqlite(detail) if detail.contains("Query returned no rows") => {
                    OverdueMutationError::NotFound
                }
                other => OverdueMutationError::Storage(other),
            })
            .and_then(|(order_id, total, waived, paid, status)| {
                if matches!(status.as_str(), "waived" | "paid" | "cancelled") {
                    return Err(OverdueMutationError::StatusInvalid(status));
                }
                let amount = (total - waived - paid).max(0.0);
                Ok(json!({
                    "overdueId": overdue_id,
                    "orderId": order_id,
                    "requestedAmount": amount,
                    "appliedAmount": 0.0,
                    "financialEffectApplied": false,
                    "settlementAuthority": "r3_settlement",
                    "status": status,
                }))
            })
    }

    pub(in crate::repositories) fn waive(
        &self,
        overdue_id: i64,
        reason: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<Value, OverdueMutationError> {
        let tenant = self.tenant_id();
        let reason = reason.to_owned();
        let actor = actor_identity_id.to_owned();
        let now = now.to_owned();
        self.session
            .write_immediate(move |tx| {
                let (status, total_fee) = tx
                    .query_row(
                        "SELECT status,total_fee FROM overdue_records
                          WHERE tenant_id=?1 AND id=?2",
                        params![tenant, overdue_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(NOT_FOUND.into()))?;
                if status == "waived" {
                    return Err(contract(ALREADY_WAIVED.into()));
                }
                if matches!(status.as_str(), "paid" | "cancelled") {
                    return Err(contract(format!("{STATUS_INVALID_PREFIX}{status}")));
                }
                tx.execute(
                    "UPDATE overdue_records
                        SET waived_amount=?1,status='waived',waived_by=?2,waived_reason=?3,
                            updated_at=?4
                      WHERE tenant_id=?5 AND id=?6",
                    params![total_fee, actor, reason, now, tenant, overdue_id],
                )
                .map_err(sqlite_error)?;
                Ok(json!({
                    "overdueId": overdue_id,
                    "waivedAmount": total_fee,
                    "waivedBy": actor,
                    "waivedAt": now,
                }))
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn list(
        &self,
        status: Option<&str>,
        customer_phone: Option<&str>,
        order_id: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Value, RepositoryError> {
        let tenant = self.tenant_id();
        let status = nonblank(status);
        let phone = nonblank(customer_phone);
        let order = nonblank(order_id);
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut predicates = vec!["tenant_id = ?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant)];
            if let Some(value) = status {
                predicates.push("status = ?".into());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = phone {
                predicates.push("customer_phone = ?".into());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = order {
                predicates.push("order_id = ?".into());
                values.push(SqlValue::Text(value));
            }
            let where_sql = predicates.join(" AND ");
            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM overdue_records WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;
            let mut statement = connection.prepare(&format!(
                "SELECT id,CAST(order_id AS TEXT),customer_id,customer_name,customer_phone,
                        expected_return_date,actual_return_date,days_overdue,daily_rate,
                        total_fee,waived_amount,paid_amount,status,escalation_level,
                        last_escalation_at,CAST(waived_by AS TEXT),waived_reason,created_at,updated_at
                   FROM overdue_records WHERE {where_sql}
                  ORDER BY created_at DESC,id DESC LIMIT {page_size} OFFSET {offset}"
            ))?;
            let items = statement
                .query_map(params_from_iter(values.iter()), overdue_row_json)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "items": items,
                "total": total,
                "page": page,
                "pageSize": page_size,
            }))
        })
    }

    pub(in crate::repositories) fn get(
        &self,
        overdue_id: i64,
    ) -> Result<Option<Value>, RepositoryError> {
        let tenant = self.tenant_id();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,CAST(order_id AS TEXT),customer_id,customer_name,customer_phone,
                            expected_return_date,actual_return_date,days_overdue,daily_rate,
                            total_fee,waived_amount,paid_amount,status,escalation_level,
                            last_escalation_at,CAST(waived_by AS TEXT),waived_reason,created_at,updated_at
                       FROM overdue_records WHERE tenant_id=?1 AND id=?2",
                    params![tenant, overdue_id],
                    overdue_row_json,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn config_get(&self) -> Result<OverdueFeeConfig, RepositoryError> {
        let tenant = self.tenant_id();
        self.session
            .read(move |connection| load_config_sqlite(connection, &tenant))
    }

    pub(in crate::repositories) fn config_upsert(
        &self,
        patch: OverdueFeeConfigPatch,
        now: &str,
    ) -> Result<OverdueFeeConfig, RepositoryError> {
        let tenant = self.tenant_id();
        let now = now.to_owned();
        self.session.write_immediate(move |tx| {
            let current = load_config_sqlite(tx, &tenant).map_err(sqlite_error)?;
            let next = patch.apply_public(current);
            tx.execute(
                "INSERT INTO overdue_fee_config
                 (tenant_id,daily_rate,max_days,cap_multiplier,grace_period_hours,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?6)
                 ON CONFLICT(tenant_id) DO UPDATE SET
                   daily_rate=excluded.daily_rate,
                   max_days=excluded.max_days,
                   cap_multiplier=excluded.cap_multiplier,
                   grace_period_hours=excluded.grace_period_hours,
                   updated_at=excluded.updated_at",
                params![
                    tenant,
                    next.daily_rate,
                    next.max_days,
                    next.cap_multiplier,
                    next.grace_period_hours,
                    now
                ],
            )
            .map_err(sqlite_error)?;
            Ok(next)
        })
    }

    pub(in crate::repositories) fn escalate(&self, now: &str) -> Result<Value, RepositoryError> {
        let tenant = self.tenant_id();
        let now = now.to_owned();
        self.session.write_immediate(move |tx| {
            let mut statement = tx
                .prepare(
                    "SELECT id,CAST(order_id AS TEXT),days_overdue,status,escalation_level
                       FROM overdue_records
                      WHERE tenant_id=?1
                        AND status IN ('active','escalated_d1','escalated_d3')
                      ORDER BY id",
                )
                .map_err(sqlite_error)?;
            let rows = statement
                .query_map(params![tenant], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(sqlite_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sqlite_error)?;
            drop(statement);

            let mut results = Vec::new();
            for (id, order_id, days, status, level) in rows {
                let result =
                    escalate_one_sqlite(tx, &tenant, id, &order_id, days, &status, level, &now)?;
                results.push(result.unwrap_or_else(|| {
                    json!({
                        "overdueId": id,
                        "orderId": order_id,
                        "daysOverdue": days,
                        "previousLevel": level,
                        "action": "none",
                    })
                }));
            }
            let escalated = results
                .iter()
                .filter(|item| item.get("action").and_then(Value::as_str) == Some("escalated"))
                .count();
            Ok(json!({
                "escalated": escalated,
                "checked": results.len(),
                "results": results,
            }))
        })
    }

    pub(in crate::repositories) fn escalation_history(
        &self,
        overdue_id: i64,
    ) -> Result<Vec<Value>, RepositoryError> {
        let tenant = self.tenant_id();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT id,overdue_id,escalation_level,channel,sent_at
                   FROM overdue_notification_log
                  WHERE tenant_id=?1 AND overdue_id=?2
                  ORDER BY escalation_level,id",
            )?;
            statement
                .query_map(params![tenant, overdue_id], |row| {
                    Ok(json!({
                        "id": row.get::<_, i64>(0)?,
                        "overdueId": row.get::<_, i64>(1)?,
                        "escalationLevel": row.get::<_, i64>(2)?,
                        "channel": row.get::<_, String>(3)?,
                        "sentAt": row.get::<_, String>(4)?,
                    }))
                })?
                .collect()
        })
    }

    pub(in crate::repositories) fn stats(&self) -> Result<Value, RepositoryError> {
        let tenant = self.tenant_id();
        self.session.read(move |connection| {
            let (total_active, total_amount): (i64, f64) = connection.query_row(
                "SELECT COUNT(*),
                        COALESCE(SUM(total_fee-waived_amount-paid_amount),0)
                   FROM overdue_records
                  WHERE tenant_id=?1
                    AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')",
                params![tenant],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;

            let mut by_statement = connection.prepare(
                "SELECT escalation_level,COUNT(*),
                        COALESCE(SUM(total_fee-waived_amount-paid_amount),0)
                   FROM overdue_records
                  WHERE tenant_id=?1
                    AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')
                  GROUP BY escalation_level ORDER BY escalation_level",
            )?;
            let by_level = by_statement
                .query_map(params![tenant], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        json!({
                            "escalationLevel": row.get::<_, i64>(0)?,
                            "count": row.get::<_, i64>(1)?,
                            "totalAmount": row.get::<_, f64>(2)?,
                        }),
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut top_statement = connection.prepare(
                "SELECT id,CAST(order_id AS TEXT),customer_name,customer_phone,days_overdue,total_fee,status
                   FROM overdue_records
                  WHERE tenant_id=?1
                    AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')
                  ORDER BY days_overdue DESC,id DESC LIMIT 5",
            )?;
            let top_overdue = top_statement
                .query_map(params![tenant], |row| {
                    Ok(json!({
                        "id": row.get::<_, i64>(0)?,
                        "orderId": row.get::<_, String>(1)?,
                        "customerName": row.get::<_, String>(2)?,
                        "customerPhone": row.get::<_, String>(3)?,
                        "daysOverdue": row.get::<_, i64>(4)?,
                        "totalFee": row.get::<_, f64>(5)?,
                        "status": row.get::<_, String>(6)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let by_escalation = by_level
                .into_iter()
                .map(|(level, value)| (format!("level_{level}"), value))
                .collect::<serde_json::Map<_, _>>();

            Ok(json!({
                "totalActive": total_active,
                "totalAmount": total_amount,
                "byEscalationLevel": by_escalation,
                "topOverdue": top_overdue,
            }))
        })
    }

    pub(in crate::repositories) fn check_before_order(
        &self,
        customer_phone: &str,
    ) -> Result<Value, RepositoryError> {
        let tenant = self.tenant_id();
        let phone = customer_phone.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT id,CAST(order_id AS TEXT),days_overdue,total_fee,waived_amount,paid_amount,status
                   FROM overdue_records
                  WHERE tenant_id=?1 AND customer_phone=?2
                    AND status IN ('active','escalated_d1','escalated_d3','escalated_d7')
                  ORDER BY days_overdue DESC,id DESC",
            )?;
            let active = statement
                .query_map(params![tenant, phone], |row| {
                    Ok(json!({
                        "id": row.get::<_, i64>(0)?,
                        "orderId": row.get::<_, String>(1)?,
                        "daysOverdue": row.get::<_, i64>(2)?,
                        "totalFee": row.get::<_, f64>(3)?,
                        "waivedAmount": row.get::<_, f64>(4)?,
                        "paidAmount": row.get::<_, f64>(5)?,
                        "status": row.get::<_, String>(6)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let has_severe = active.iter().any(|record| {
                record.get("status").and_then(Value::as_str) == Some("escalated_d7")
            });
            let total_unpaid = active
                .iter()
                .map(|record| {
                    let total = record.get("totalFee").and_then(Value::as_f64).unwrap_or(0.0);
                    let waived = record
                        .get("waivedAmount")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    let paid = record
                        .get("paidAmount")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0);
                    (total - waived - paid).max(0.0)
                })
                .sum::<f64>();

            Ok(json!({
                "hasOverdue": !active.is_empty(),
                "activeRecords": active,
                "totalUnpaid": total_unpaid,
                "blockReason": if has_severe {
                    Some("该客户存在严重逾期记录(已进入法律告知阶段), 请先处理逾期再下单")
                } else {
                    None
                },
                "allowed": !has_severe,
            }))
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

#[derive(Debug, Clone, Default)]
pub struct OverdueFeeConfigPatch {
    pub daily_rate: Option<f64>,
    pub max_days: Option<i64>,
    pub cap_multiplier: Option<f64>,
    pub grace_period_hours: Option<i64>,
}

impl OverdueFeeConfigPatch {
    pub(in crate::repositories) fn apply_public(
        self,
        current: OverdueFeeConfig,
    ) -> OverdueFeeConfig {
        OverdueFeeConfig {
            daily_rate: self.daily_rate.unwrap_or(current.daily_rate),
            max_days: self.max_days.unwrap_or(current.max_days),
            cap_multiplier: self.cap_multiplier.unwrap_or(current.cap_multiplier),
            grace_period_hours: self
                .grace_period_hours
                .unwrap_or(current.grace_period_hours),
        }
    }
}

pub(in crate::repositories) fn map_mutation_error(error: RepositoryError) -> OverdueMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message == NOT_FOUND {
            return OverdueMutationError::NotFound;
        }
        if message == ALREADY_WAIVED {
            return OverdueMutationError::AlreadyWaived;
        }
        if let Some(status) = message.strip_prefix(STATUS_INVALID_PREFIX) {
            return OverdueMutationError::StatusInvalid(status.to_owned());
        }
    }
    OverdueMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn load_config_sqlite(
    connection: &rusqlite::Connection,
    tenant: &str,
) -> rusqlite::Result<OverdueFeeConfig> {
    connection
        .query_row(
            "SELECT daily_rate,max_days,cap_multiplier,grace_period_hours
               FROM overdue_fee_config WHERE tenant_id=?1 LIMIT 1",
            params![tenant],
            |row| {
                Ok(OverdueFeeConfig {
                    daily_rate: row.get(0)?,
                    max_days: row.get(1)?,
                    cap_multiplier: row.get(2)?,
                    grace_period_hours: row.get(3)?,
                })
            },
        )
        .optional()
        .map(|value| value.unwrap_or_default())
}

pub(in crate::repositories) fn calculate_fee_public(
    days: i64,
    config: &OverdueFeeConfig,
) -> (i64, f64) {
    let cap_days = (config.max_days as f64 * config.cap_multiplier).max(0.0) as i64;
    let capped_days = days.max(0).min(cap_days);
    (
        capped_days,
        (capped_days as f64 * config.daily_rate).max(0.0),
    )
}

pub(in crate::repositories) fn days_between_public(earlier: &str, later: &str) -> i64 {
    let earlier = NaiveDate::parse_from_str(earlier.get(..10).unwrap_or(earlier), "%Y-%m-%d");
    let later = NaiveDate::parse_from_str(later.get(..10).unwrap_or(later), "%Y-%m-%d");
    match (earlier, later) {
        (Ok(earlier), Ok(later)) => (later - earlier).num_days(),
        _ => 0,
    }
}

pub(in crate::repositories) fn target_escalation_public(
    days: i64,
    level: i64,
) -> Option<(i64, &'static str, &'static str)> {
    if days >= 7 && level < 3 {
        Some((3, "escalated_d7", "legal_notice"))
    } else if days >= 3 && level < 2 {
        Some((2, "escalated_d3", "email"))
    } else if days >= 1 && level < 1 {
        Some((1, "escalated_d1", "sms"))
    } else {
        None
    }
}

fn escalate_one_sqlite(
    tx: &rusqlite::Transaction<'_>,
    tenant: &str,
    overdue_id: i64,
    order_id: &str,
    days: i64,
    _status: &str,
    level: i64,
    now: &str,
) -> Result<Option<Value>, RepositoryError> {
    let Some((new_level, new_status, channel)) = target_escalation_public(days, level) else {
        return Ok(None);
    };

    let existing = tx
        .query_row(
            "SELECT id FROM overdue_notification_log
              WHERE tenant_id=?1 AND overdue_id=?2 AND escalation_level=?3 LIMIT 1",
            params![tenant, overdue_id, new_level],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(sqlite_error)?;
    if existing.is_some() {
        return Ok(Some(json!({
            "overdueId": overdue_id,
            "orderId": order_id,
            "daysOverdue": days,
            "previousLevel": level,
            "targetLevel": new_level,
            "action": "skipped_duplicate",
        })));
    }

    tx.execute(
        "UPDATE overdue_records
            SET status=?1,escalation_level=?2,last_escalation_at=?3,updated_at=?3
          WHERE tenant_id=?4 AND id=?5",
        params![new_status, new_level, now, tenant, overdue_id],
    )
    .map_err(sqlite_error)?;
    tx.execute(
        "INSERT INTO overdue_notification_log
         (tenant_id,overdue_id,escalation_level,channel,sent_at)
         VALUES (?1,?2,?3,?4,?5)",
        params![tenant, overdue_id, new_level, channel, now],
    )
    .map_err(sqlite_error)?;

    Ok(Some(json!({
        "overdueId": overdue_id,
        "orderId": order_id,
        "daysOverdue": days,
        "previousLevel": level,
        "newLevel": new_level,
        "newStatus": new_status,
        "channel": channel,
        "action": "escalated",
        "escalatedAt": now,
    })))
}

fn overdue_row_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "orderId": row.get::<_, String>(1)?,
        "customerId": row.get::<_, Option<String>>(2)?,
        "customerName": row.get::<_, String>(3)?,
        "customerPhone": row.get::<_, String>(4)?,
        "expectedReturnDate": row.get::<_, String>(5)?,
        "actualReturnDate": row.get::<_, Option<String>>(6)?,
        "daysOverdue": row.get::<_, i64>(7)?,
        "dailyRate": row.get::<_, f64>(8)?,
        "totalFee": row.get::<_, f64>(9)?,
        "waivedAmount": row.get::<_, f64>(10)?,
        "paidAmount": row.get::<_, f64>(11)?,
        "status": row.get::<_, String>(12)?,
        "escalationLevel": row.get::<_, i64>(13)?,
        "lastEscalationAt": row.get::<_, Option<String>>(14)?,
        "waivedBy": row.get::<_, Option<String>>(15)?,
        "waivedReason": row.get::<_, Option<String>>(16)?,
        "createdAt": row.get::<_, String>(17)?,
        "updatedAt": row.get::<_, String>(18)?,
    }))
}

fn nonblank(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
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
        OverdueFeeConfigPatch, RepositoryProvider, SqliteRepositoryProvider,
    };

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("overdue-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("overdue-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_overdue_authority_preserves_scope_identity_escalation_and_financial_delegation() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE orders (
                    id TEXT PRIMARY KEY,
                    endDate TEXT NOT NULL,
                    customer_id TEXT,
                    tenant_id TEXT NOT NULL
                );
                CREATE TABLE order_lifecycle (
                    order_id TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    commercial_status TEXT NOT NULL,
                    fulfilment_status TEXT NOT NULL,
                    PRIMARY KEY(order_id,tenant_id)
                );
                CREATE TABLE customers (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL,
                    display_name TEXT NOT NULL
                );
                CREATE TABLE customer_contacts (
                    id TEXT PRIMARY KEY,
                    tenant_id TEXT NOT NULL,
                    customer_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    raw_value TEXT NOT NULL,
                    is_primary INTEGER NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE overdue_fee_config (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    tenant_id TEXT NOT NULL,
                    daily_rate REAL NOT NULL,
                    max_days INTEGER NOT NULL,
                    cap_multiplier REAL NOT NULL,
                    grace_period_hours INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(tenant_id)
                );
                CREATE TABLE overdue_records (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    tenant_id TEXT NOT NULL,
                    order_id INTEGER NOT NULL,
                    customer_id TEXT,
                    customer_name TEXT NOT NULL,
                    customer_phone TEXT NOT NULL,
                    expected_return_date TEXT NOT NULL,
                    actual_return_date TEXT,
                    days_overdue INTEGER NOT NULL DEFAULT 0,
                    daily_rate REAL NOT NULL,
                    total_fee REAL NOT NULL DEFAULT 0,
                    waived_amount REAL NOT NULL DEFAULT 0,
                    paid_amount REAL NOT NULL DEFAULT 0,
                    status TEXT NOT NULL DEFAULT 'active',
                    escalation_level INTEGER NOT NULL DEFAULT 0,
                    last_escalation_at TEXT,
                    waived_by TEXT,
                    waived_reason TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE overdue_notification_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    tenant_id TEXT NOT NULL,
                    overdue_id INTEGER NOT NULL,
                    escalation_level INTEGER NOT NULL,
                    channel TEXT NOT NULL,
                    sent_at TEXT NOT NULL,
                    UNIQUE(tenant_id,overdue_id,escalation_level)
                );

                INSERT INTO customers VALUES
                    ('customer-a','tenant-a','Customer A'),
                    ('customer-a-unresolved','tenant-a','Customer A unresolved'),
                    ('customer-b','tenant-b','Customer B');
                INSERT INTO customer_contacts VALUES
                    ('contact-a','tenant-a','customer-a','phone','13800000000',1,'now'),
                    ('contact-b','tenant-b','customer-b','phone','13800000000',1,'now');
                INSERT INTO orders VALUES
                    ('order-a','2026-09-20','customer-a','tenant-a'),
                    ('order-a-unresolved','2026-09-19','customer-a-unresolved','tenant-a'),
                    ('order-b','2026-09-28','customer-b','tenant-b');
                INSERT INTO order_lifecycle VALUES
                    ('order-a','tenant-a','confirmed','in_use'),
                    ('order-a-unresolved','tenant-a','confirmed','shipped'),
                    ('order-b','tenant-b','confirmed','return_pending');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool.clone());
        let scoped_a = provider.bind(&context("tenant-a", "overdue-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "overdue-b")).unwrap();

        scoped_a
            .overdues()
            .config_upsert(
                OverdueFeeConfigPatch {
                    daily_rate: Some(50.0),
                    max_days: Some(30),
                    cap_multiplier: Some(3.0),
                    grace_period_hours: Some(0),
                },
                "2026-09-30T20:00:00+08:00",
            )
            .unwrap();
        scoped_b
            .overdues()
            .config_upsert(
                OverdueFeeConfigPatch {
                    daily_rate: Some(10.0),
                    max_days: Some(30),
                    cap_multiplier: Some(3.0),
                    grace_period_hours: Some(4),
                },
                "2026-09-30T20:00:00+08:00",
            )
            .unwrap();

        let detected_a = scoped_a
            .overdues()
            .detect("2026-09-30", "2026-09-30T20:01:00+08:00")
            .unwrap();
        assert_eq!(detected_a["detected"], 1);
        assert_eq!(detected_a["skippedUnresolvedContact"], 1);
        assert_eq!(detected_a["records"][0]["orderId"], "order-a");
        assert_eq!(detected_a["records"][0]["totalFee"], 500.0);

        let detected_b = scoped_b
            .overdues()
            .detect("2026-09-30", "2026-09-30T20:01:00+08:00")
            .unwrap();
        assert_eq!(detected_b["detected"], 1);
        assert_eq!(detected_b["records"][0]["orderId"], "order-b");
        assert_eq!(detected_b["records"][0]["totalFee"], 20.0);

        let list_a = scoped_a.overdues().list(None, None, None, 1, 20).unwrap();
        let list_b = scoped_b.overdues().list(None, None, None, 1, 20).unwrap();
        assert_eq!(list_a["total"], 1);
        assert_eq!(list_b["total"], 1);
        assert_eq!(list_a["items"][0]["status"], "escalated_d7");
        assert_eq!(list_b["items"][0]["status"], "active");

        let overdue_id = list_a["items"][0]["id"].as_i64().unwrap();
        let history = scoped_a.overdues().escalation_history(overdue_id).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["escalationLevel"], 3);

        // Re-running detection must not create a second active fact or duplicate escalation log.
        scoped_a
            .overdues()
            .detect("2026-09-30", "2026-09-30T20:02:00+08:00")
            .unwrap();
        assert_eq!(
            scoped_a.overdues().list(None, None, None, 1, 20).unwrap()["total"],
            1
        );
        assert_eq!(
            scoped_a
                .overdues()
                .escalation_history(overdue_id)
                .unwrap()
                .len(),
            1
        );

        let calc = scoped_a.overdues().calc("order-a", "2026-09-30").unwrap();
        assert_eq!(calc["daysOverdue"], 10);
        assert_eq!(calc["actualFee"], 500.0);

        let delegated = scoped_a.overdues().apply_delegated(overdue_id).unwrap();
        assert_eq!(delegated["requestedAmount"], 500.0);
        assert_eq!(delegated["appliedAmount"], 0.0);
        assert_eq!(delegated["financialEffectApplied"], false);
        assert_eq!(delegated["settlementAuthority"], "r3_settlement");

        let before_waive = scoped_a
            .overdues()
            .check_before_order("13800000000")
            .unwrap();
        assert_eq!(before_waive["allowed"], false);

        let waived = scoped_a
            .overdues()
            .waive(
                overdue_id,
                "manual exception",
                "identity-admin-a",
                "2026-09-30T20:03:00+08:00",
            )
            .unwrap();
        assert_eq!(waived["waivedBy"], "identity-admin-a");
        assert_eq!(
            scoped_a.overdues().get(overdue_id).unwrap().unwrap()["status"],
            "waived"
        );
        assert_eq!(
            scoped_a
                .overdues()
                .check_before_order("13800000000")
                .unwrap()["allowed"],
            true
        );

        // Legacy INTEGER order/identity values remain readable as canonical text.
        let connection = pool.get().unwrap();
        connection
            .execute(
                "INSERT INTO overdue_records
                 (tenant_id,order_id,customer_name,customer_phone,expected_return_date,
                  days_overdue,daily_rate,total_fee,status,waived_by,created_at,updated_at)
                 VALUES ('tenant-a',42,'Legacy','13900000000','2026-09-01',
                         1,1,1,'cancelled',7,'now','now')",
                [],
            )
            .unwrap();
        let legacy_id = connection.last_insert_rowid();
        drop(connection);
        let legacy = scoped_a.overdues().get(legacy_id).unwrap().unwrap();
        assert_eq!(legacy["orderId"], "42");
        assert_eq!(legacy["waivedBy"], "7");

        let connection = pool.get().unwrap();
        let a_configs: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM overdue_fee_config WHERE tenant_id='tenant-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(a_configs, 1);
    }
}

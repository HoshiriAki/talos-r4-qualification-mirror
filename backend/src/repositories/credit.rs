use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde_json::{Value, json};

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const DUPLICATE_BLACKLIST: &str = "credit-blacklist-duplicate";
const BLACKLIST_NOT_FOUND: &str = "credit-blacklist-not-found";
const BLACKLIST_ALREADY_REMOVED: &str = "credit-blacklist-already-removed";
const VIOLATION_NOT_FOUND: &str = "credit-violation-not-found";
const STATUS_INVALID_PREFIX: &str = "credit-status-invalid:";
const CREDIT_NOT_FOUND: &str = "credit-profile-not-found";

#[derive(Debug, Clone, PartialEq)]
pub struct BlacklistAddOutcome {
    pub id: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlacklistRemoveOutcome {
    pub id: i64,
    pub removed_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViolationRecordOutcome {
    pub id: i64,
    pub penalty: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViolationTransitionOutcome {
    pub id: i64,
    pub status: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreditRecalculateOutcome {
    pub customer_phone: String,
    pub score: i64,
    pub on_time_returns: i64,
    pub late_returns: i64,
    pub damage_incidents: i64,
    pub recalculated_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CreditMutationError {
    #[error("customer already has an active blacklist record")]
    DuplicateBlacklist,
    #[error("blacklist record not found")]
    BlacklistNotFound,
    #[error("blacklist record is already removed")]
    BlacklistAlreadyRemoved,
    #[error("violation record not found")]
    ViolationNotFound,
    #[error("status transition is invalid: {0}")]
    StatusInvalid(String),
    #[error("credit profile not found")]
    CreditNotFound,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteCreditRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteCreditRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn blacklist_add(
        &self,
        customer_name: &str,
        customer_phone: &str,
        id_number: Option<&str>,
        reason: &str,
        severity: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<BlacklistAddOutcome, CreditMutationError> {
        let tenant_id = self.tenant_id();
        let customer_name = customer_name.to_owned();
        let customer_phone = customer_phone.to_owned();
        let id_number = id_number.map(str::to_owned);
        let reason = reason.to_owned();
        let severity = severity.to_owned();
        let actor_identity_id = actor_identity_id.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let existing = transaction
                    .query_row(
                        "SELECT id FROM blacklist
                         WHERE tenant_id=?1 AND customer_phone=?2 AND is_active=1
                         LIMIT 1",
                        params![tenant_id, customer_phone],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if existing.is_some() {
                    return Err(contract(DUPLICATE_BLACKLIST.into()));
                }

                transaction
                    .execute(
                        "INSERT INTO blacklist
                         (tenant_id,customer_name,customer_phone,id_number,reason,severity,
                          created_by,is_active,created_at,updated_at)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,?8)",
                        params![
                            tenant_id,
                            customer_name,
                            customer_phone,
                            id_number,
                            reason,
                            severity,
                            actor_identity_id,
                            now,
                        ],
                    )
                    .map_err(sqlite_error)?;
                Ok(BlacklistAddOutcome {
                    id: transaction.last_insert_rowid(),
                    created_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn blacklist_remove(
        &self,
        id: i64,
        removal_reason: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<BlacklistRemoveOutcome, CreditMutationError> {
        let tenant_id = self.tenant_id();
        let removal_reason = removal_reason.to_owned();
        let actor_identity_id = actor_identity_id.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let active = transaction
                    .query_row(
                        "SELECT is_active FROM blacklist
                         WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, id],
                        |row| Ok(row.get::<_, i32>(0)? != 0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(BLACKLIST_NOT_FOUND.into()))?;
                if !active {
                    return Err(contract(BLACKLIST_ALREADY_REMOVED.into()));
                }

                transaction
                    .execute(
                        "UPDATE blacklist
                         SET is_active=0,removed_at=?1,removed_by=?2,removal_reason=?3,updated_at=?1
                         WHERE tenant_id=?4 AND id=?5 AND is_active=1",
                        params![now, actor_identity_id, removal_reason, tenant_id, id],
                    )
                    .map_err(sqlite_error)?;

                Ok(BlacklistRemoveOutcome {
                    id,
                    removed_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn blacklist_check(
        &self,
        customer_phone: &str,
    ) -> Result<Value, RepositoryError> {
        let tenant_id = self.tenant_id();
        let customer_phone = customer_phone.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT id,customer_name,customer_phone,id_number,reason,severity,created_by,
                        is_active,removed_at,removed_by,removal_reason,created_at,updated_at
                 FROM blacklist
                 WHERE tenant_id=?1 AND customer_phone=?2 AND is_active=1
                 ORDER BY created_at DESC",
            )?;
            let records = statement
                .query_map(params![tenant_id, customer_phone], map_blacklist_record)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "blacklisted": !records.is_empty(),
                "records": records,
            }))
        })
    }

    pub(in crate::repositories) fn blacklist_list(
        &self,
        is_active: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<Value, RepositoryError> {
        let tenant_id = self.tenant_id();
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;
        self.session.read(move |connection| {
            let filter_sql = match is_active {
                Some(true) => " AND is_active=1",
                Some(false) => " AND is_active=0",
                None => "",
            };
            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM blacklist WHERE tenant_id=?1{filter_sql}"),
                params![tenant_id],
                |row| row.get(0),
            )?;
            let sql = format!(
                "SELECT id,customer_name,customer_phone,id_number,reason,severity,created_by,
                        is_active,removed_at,removed_by,removal_reason,created_at,updated_at
                 FROM blacklist
                 WHERE tenant_id=?1{filter_sql}
                 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3"
            );
            let mut statement = connection.prepare(&sql)?;
            let items = statement
                .query_map(params![tenant_id, page_size, offset], map_blacklist_record)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "items": items,
                "total": total,
                "page": page,
                "pageSize": page_size,
            }))
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::repositories) fn violation_record(
        &self,
        customer_name: &str,
        customer_phone: &str,
        order_id: Option<i64>,
        violation_type: &str,
        severity: &str,
        description: &str,
        evidence: Option<&str>,
        financial_penalty: f64,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<ViolationRecordOutcome, CreditMutationError> {
        let tenant_id = self.tenant_id();
        let customer_name = customer_name.to_owned();
        let customer_phone = customer_phone.to_owned();
        let violation_type = violation_type.to_owned();
        let severity = severity.to_owned();
        let description = description.to_owned();
        let evidence = evidence.map(str::to_owned);
        let actor_identity_id = actor_identity_id.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                transaction
                    .execute(
                        "INSERT INTO violations
                         (tenant_id,customer_name,customer_phone,order_id,violation_type,severity,
                          description,evidence,financial_penalty,reported_by,status,created_at,updated_at)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'recorded',?11,?11)",
                        params![
                            tenant_id,
                            customer_name,
                            customer_phone,
                            order_id,
                            violation_type,
                            severity,
                            description,
                            evidence,
                            financial_penalty,
                            actor_identity_id,
                            now,
                        ],
                    )
                    .map_err(sqlite_error)?;
                let id = transaction.last_insert_rowid();
                let penalty = score_penalty(&violation_type, &severity);
                ensure_credit_score_sqlite(
                    transaction,
                    &tenant_id,
                    &customer_name,
                    &customer_phone,
                    &now,
                )?;
                transaction
                    .execute(
                        "UPDATE credit_scores
                         SET damage_incidents=damage_incidents+1,
                             score=MAX(0,score-?1),
                             updated_at=?2,last_calculated_at=?2
                         WHERE tenant_id=?3 AND customer_phone=?4",
                        params![penalty, now, tenant_id, customer_phone],
                    )
                    .map_err(sqlite_error)?;
                Ok(ViolationRecordOutcome {
                    id,
                    penalty,
                    created_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn violation_appeal(
        &self,
        id: i64,
        appeal_reason: &str,
        now: &str,
    ) -> Result<ViolationTransitionOutcome, CreditMutationError> {
        self.violation_transition(id, "recorded", "appealed", None, appeal_reason, now)
    }

    pub(in crate::repositories) fn violation_review(
        &self,
        id: i64,
        status: &str,
        review_notes: &str,
        actor_identity_id: &str,
        now: &str,
    ) -> Result<ViolationTransitionOutcome, CreditMutationError> {
        let tenant_id = self.tenant_id();
        let status = status.to_owned();
        let review_notes = review_notes.to_owned();
        let actor_identity_id = actor_identity_id.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let current = transaction
                    .query_row(
                        "SELECT status FROM violations WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(VIOLATION_NOT_FOUND.into()))?;
                if current != "appealed" && current != "under_review" {
                    return Err(contract(format!("{STATUS_INVALID_PREFIX}{current}")));
                }

                transaction
                    .execute(
                        "UPDATE violations
                         SET status=?1,reviewed_by=?2,reviewed_at=?3,review_notes=?4,updated_at=?3
                         WHERE tenant_id=?5 AND id=?6",
                        params![status, actor_identity_id, now, review_notes, tenant_id, id],
                    )
                    .map_err(sqlite_error)?;

                Ok(ViolationTransitionOutcome {
                    id,
                    status,
                    at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    fn violation_transition(
        &self,
        id: i64,
        expected: &str,
        next: &str,
        _actor_identity_id: Option<&str>,
        note: &str,
        now: &str,
    ) -> Result<ViolationTransitionOutcome, CreditMutationError> {
        let tenant_id = self.tenant_id();
        let expected = expected.to_owned();
        let next = next.to_owned();
        let note = note.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let current = transaction
                    .query_row(
                        "SELECT status FROM violations WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(VIOLATION_NOT_FOUND.into()))?;
                if current != expected {
                    return Err(contract(format!("{STATUS_INVALID_PREFIX}{current}")));
                }
                transaction
                    .execute(
                        "UPDATE violations
                         SET status=?1,appeal_reason=?2,appeal_at=?3,updated_at=?3
                         WHERE tenant_id=?4 AND id=?5",
                        params![next, note, now, tenant_id, id],
                    )
                    .map_err(sqlite_error)?;
                Ok(ViolationTransitionOutcome {
                    id,
                    status: next,
                    at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn violation_list(
        &self,
        customer_phone: Option<&str>,
        status: Option<&str>,
        violation_type: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<Value, RepositoryError> {
        let tenant_id = self.tenant_id();
        let customer_phone = customer_phone.map(str::to_owned);
        let status = status.map(str::to_owned);
        let violation_type = violation_type.map(str::to_owned);
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut predicates = vec!["tenant_id=?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id)];
            if let Some(value) = customer_phone {
                predicates.push("customer_phone=?".into());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = status {
                predicates.push("status=?".into());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = violation_type {
                predicates.push("violation_type=?".into());
                values.push(SqlValue::Text(value));
            }
            let where_sql = predicates.join(" AND ");
            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM violations WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;
            let sql = format!(
                "SELECT id,customer_name,customer_phone,order_id,violation_type,severity,
                        description,evidence,financial_penalty,reported_by,status,appeal_reason,
                        appeal_at,reviewed_by,reviewed_at,review_notes,created_at,updated_at
                 FROM violations WHERE {where_sql}
                 ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
            );
            let mut statement = connection.prepare(&sql)?;
            let items = statement
                .query_map(params_from_iter(values.iter()), map_violation_record)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({
                "items": items,
                "total": total,
                "page": page,
                "pageSize": page_size,
            }))
        })
    }

    pub(in crate::repositories) fn violation_get(
        &self,
        id: i64,
    ) -> Result<Option<Value>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,customer_name,customer_phone,order_id,violation_type,severity,
                            description,evidence,financial_penalty,reported_by,status,appeal_reason,
                            appeal_at,reviewed_by,reviewed_at,review_notes,created_at,updated_at
                     FROM violations WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                    params![tenant_id, id],
                    map_violation_record,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn credit_get(
        &self,
        customer_phone: &str,
        now: &str,
    ) -> Result<Value, CreditMutationError> {
        let tenant_id = self.tenant_id();
        let customer_phone = customer_phone.to_owned();
        let now = now.to_owned();
        self.session
            .write_immediate(move |transaction| {
                ensure_credit_score_sqlite(
                    transaction,
                    &tenant_id,
                    &customer_phone,
                    &customer_phone,
                    &now,
                )?;
                transaction
                    .query_row(
                        "SELECT id,customer_name,customer_phone,score,total_orders,on_time_returns,
                                late_returns,damage_incidents,last_calculated_at,created_at,updated_at
                         FROM credit_scores
                         WHERE tenant_id=?1 AND customer_phone=?2 LIMIT 1",
                        params![tenant_id, customer_phone],
                        map_credit_record,
                    )
                    .map_err(sqlite_error)
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn credit_history(
        &self,
        customer_phone: &str,
    ) -> Result<Value, RepositoryError> {
        let tenant_id = self.tenant_id();
        let customer_phone = customer_phone.to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT id,violation_type,severity,description,status,created_at
                 FROM violations
                 WHERE tenant_id=?1 AND customer_phone=?2
                 ORDER BY created_at DESC",
            )?;
            let history = statement
                .query_map(params![tenant_id, customer_phone], |row| {
                    let violation_type = row.get::<_, String>(1)?;
                    let severity = row.get::<_, String>(2)?;
                    Ok(json!({
                        "id": row.get::<_, i64>(0)?,
                        "eventType": "violation",
                        "violationType": violation_type,
                        "severity": severity,
                        "description": row.get::<_, String>(3)?,
                        "scoreImpact": -score_penalty(&violation_type, &severity),
                        "status": row.get::<_, String>(4)?,
                        "createdAt": row.get::<_, String>(5)?,
                    }))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let current_score = connection
                .query_row(
                    "SELECT score,on_time_returns,late_returns,damage_incidents
                     FROM credit_scores
                     WHERE tenant_id=?1 AND customer_phone=?2 LIMIT 1",
                    params![tenant_id, customer_phone],
                    |row| {
                        Ok(json!({
                            "score": row.get::<_, i64>(0)?,
                            "onTimeReturns": row.get::<_, i64>(1)?,
                            "lateReturns": row.get::<_, i64>(2)?,
                            "damageIncidents": row.get::<_, i64>(3)?,
                        }))
                    },
                )
                .optional()?
                .unwrap_or_else(|| {
                    json!({
                        "score": 100,
                        "onTimeReturns": 0,
                        "lateReturns": 0,
                        "damageIncidents": 0,
                    })
                });

            Ok(json!({
                "currentScore": current_score,
                "history": history,
            }))
        })
    }

    pub(in crate::repositories) fn credit_recalculate(
        &self,
        customer_phone: &str,
        now: &str,
    ) -> Result<CreditRecalculateOutcome, CreditMutationError> {
        let tenant_id = self.tenant_id();
        let customer_phone = customer_phone.to_owned();
        let now = now.to_owned();
        self.session
            .write_immediate(move |transaction| {
                let counters = transaction
                    .query_row(
                        "SELECT on_time_returns,late_returns,damage_incidents
                         FROM credit_scores
                         WHERE tenant_id=?1 AND customer_phone=?2 LIMIT 1",
                        params![tenant_id, customer_phone],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, i64>(1)?,
                                row.get::<_, i64>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(CREDIT_NOT_FOUND.into()))?;
                let (on_time_returns, late_returns, damage_incidents) = counters;
                let score =
                    (100_i64 + on_time_returns * 2 - late_returns * 5 - damage_incidents * 10)
                        .clamp(0, 200);
                transaction
                    .execute(
                        "UPDATE credit_scores
                         SET score=?1,last_calculated_at=?2,updated_at=?2
                         WHERE tenant_id=?3 AND customer_phone=?4",
                        params![score, now, tenant_id, customer_phone],
                    )
                    .map_err(sqlite_error)?;
                Ok(CreditRecalculateOutcome {
                    customer_phone,
                    score,
                    on_time_returns,
                    late_returns,
                    damage_incidents,
                    recalculated_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn check_before_order(
        &self,
        customer_phone: &str,
    ) -> Result<Value, RepositoryError> {
        let tenant_id = self.tenant_id();
        let customer_phone = customer_phone.to_owned();
        self.session.read(move |connection| {
            let mut warnings = Vec::<String>::new();
            let mut statement = connection.prepare(
                "SELECT severity,reason FROM blacklist
                 WHERE tenant_id=?1 AND customer_phone=?2 AND is_active=1",
            )?;
            let blacklist = statement
                .query_map(params![tenant_id, customer_phone], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            for (severity, reason) in &blacklist {
                if severity == "critical" || severity == "high" {
                    return Ok(json!({
                        "allowed": false,
                        "blockReason": format!("黑名单({severity}): {reason}"),
                        "creditScore": Value::Null,
                        "creditWarning": Value::Null,
                        "warnings": [format!("该客户在黑名单中，严重程度: {severity}，原因: {reason}")],
                    }));
                }
                warnings.push(format!("该客户在黑名单中（{severity}）, 原因: {reason}"));
            }

            let score_info = connection
                .query_row(
                    "SELECT score FROM credit_scores
                     WHERE tenant_id=?1 AND customer_phone=?2 LIMIT 1",
                    params![tenant_id, customer_phone],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            let (credit_score, credit_warning) = match score_info {
                Some(score) if score < 50 => {
                    return Ok(json!({
                        "allowed": false,
                        "blockReason": format!("信用评分过低({score})，拒绝下单"),
                        "creditScore": score,
                        "creditWarning": format!("信用评分极低({score}分)，建议人工审核"),
                        "warnings": [format!("信用评分 {score} 分，已低于最低标准 50 分")],
                    }));
                }
                Some(score) if score < 80 => {
                    (score, Some(format!("信用评分较低({score}分)，建议关注")))
                }
                Some(score) => (score, None),
                None => (100, None),
            };

            let mut statement = connection.prepare(
                "SELECT violation_type,severity FROM violations
                 WHERE tenant_id=?1 AND customer_phone=?2
                   AND severity IN ('major','critical')
                   AND status IN ('recorded','appealed','under_review')",
            )?;
            let pending = statement
                .query_map(params![tenant_id, customer_phone], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            for (violation_type, severity) in pending {
                warnings.push(format!("未处理的严重违规: {violation_type} ({severity})"));
            }

            Ok(json!({
                "allowed": true,
                "blockReason": Value::Null,
                "creditScore": credit_score,
                "creditWarning": credit_warning,
                "warnings": warnings,
            }))
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

pub fn score_label(score: i64) -> &'static str {
    match score {
        150..=200 => "优秀",
        120..=149 => "良好",
        90..=119 => "一般",
        60..=89 => "较差",
        _ => "差",
    }
}

pub(in crate::repositories) fn score_penalty(violation_type: &str, severity: &str) -> i64 {
    if (violation_type == "damage" || violation_type == "lost_device")
        && (severity == "major" || severity == "critical")
    {
        20
    } else {
        10
    }
}

fn ensure_credit_score_sqlite(
    transaction: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    customer_name: &str,
    customer_phone: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    transaction
        .execute(
            "INSERT OR IGNORE INTO credit_scores
             (tenant_id,customer_name,customer_phone,score,total_orders,on_time_returns,
              late_returns,damage_incidents,last_calculated_at,created_at,updated_at)
             VALUES (?1,?2,?3,100,0,0,0,0,?4,?4,?4)",
            params![tenant_id, customer_name, customer_phone, now],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

pub(in crate::repositories) fn map_mutation_error(error: RepositoryError) -> CreditMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        match message.as_str() {
            DUPLICATE_BLACKLIST => return CreditMutationError::DuplicateBlacklist,
            BLACKLIST_NOT_FOUND => return CreditMutationError::BlacklistNotFound,
            BLACKLIST_ALREADY_REMOVED => return CreditMutationError::BlacklistAlreadyRemoved,
            VIOLATION_NOT_FOUND => return CreditMutationError::ViolationNotFound,
            CREDIT_NOT_FOUND => return CreditMutationError::CreditNotFound,
            _ => {}
        }
        if let Some(status) = message.strip_prefix(STATUS_INVALID_PREFIX) {
            return CreditMutationError::StatusInvalid(status.to_owned());
        }
    }
    CreditMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

fn actor_json(actor: String) -> Value {
    actor
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(actor))
}

fn map_blacklist_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "customerName": row.get::<_, String>(1)?,
        "customerPhone": row.get::<_, String>(2)?,
        "idNumber": row.get::<_, Option<String>>(3)?,
        "reason": row.get::<_, String>(4)?,
        "severity": row.get::<_, String>(5)?,
        "createdBy": actor_json(row.get::<_, String>(6)?),
        "isActive": row.get::<_, i32>(7)? != 0,
        "removedAt": row.get::<_, Option<String>>(8)?,
        "removedBy": row
            .get::<_, Option<String>>(9)?
            .map(actor_json),
        "removalReason": row.get::<_, Option<String>>(10)?,
        "createdAt": row.get::<_, String>(11)?,
        "updatedAt": row.get::<_, String>(12)?,
    }))
}

fn map_violation_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "customerName": row.get::<_, String>(1)?,
        "customerPhone": row.get::<_, String>(2)?,
        "orderId": row.get::<_, Option<i64>>(3)?,
        "violationType": row.get::<_, String>(4)?,
        "severity": row.get::<_, String>(5)?,
        "description": row.get::<_, String>(6)?,
        "evidence": row.get::<_, Option<String>>(7)?,
        "financialPenalty": row.get::<_, f64>(8)?,
        "reportedBy": actor_json(row.get::<_, String>(9)?),
        "status": row.get::<_, String>(10)?,
        "appealReason": row.get::<_, Option<String>>(11)?,
        "appealAt": row.get::<_, Option<String>>(12)?,
        "reviewedBy": row
            .get::<_, Option<String>>(13)?
            .map(actor_json),
        "reviewedAt": row.get::<_, Option<String>>(14)?,
        "reviewNotes": row.get::<_, Option<String>>(15)?,
        "createdAt": row.get::<_, String>(16)?,
        "updatedAt": row.get::<_, String>(17)?,
    }))
}

fn map_credit_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let score = row.get::<_, i64>(3)?;
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "customerName": row.get::<_, String>(1)?,
        "customerPhone": row.get::<_, String>(2)?,
        "score": score,
        "totalOrders": row.get::<_, i64>(4)?,
        "onTimeReturns": row.get::<_, i64>(5)?,
        "lateReturns": row.get::<_, i64>(6)?,
        "damageIncidents": row.get::<_, i64>(7)?,
        "lastCalculatedAt": row.get::<_, String>(8)?,
        "createdAt": row.get::<_, String>(9)?,
        "updatedAt": row.get::<_, String>(10)?,
        "label": score_label(score),
    }))
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

    use crate::repositories::{CreditMutationError, RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("credit-actor", "admin").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("credit-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_credit_authority_preserves_scope_atomic_score_and_state_machine() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE blacklist (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    tenant_id TEXT NOT NULL,
                    customer_name TEXT NOT NULL,
                    customer_phone TEXT NOT NULL,
                    id_number TEXT,
                    reason TEXT NOT NULL,
                    severity TEXT NOT NULL,
                    created_by TEXT NOT NULL,
                    is_active INTEGER NOT NULL DEFAULT 1,
                    removed_at TEXT,
                    removed_by TEXT,
                    removal_reason TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE violations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    tenant_id TEXT NOT NULL,
                    customer_name TEXT NOT NULL,
                    customer_phone TEXT NOT NULL,
                    order_id INTEGER,
                    violation_type TEXT NOT NULL,
                    severity TEXT NOT NULL,
                    description TEXT NOT NULL,
                    evidence TEXT,
                    financial_penalty REAL DEFAULT 0,
                    reported_by TEXT NOT NULL,
                    status TEXT NOT NULL,
                    appeal_reason TEXT,
                    appeal_at TEXT,
                    reviewed_by TEXT,
                    reviewed_at TEXT,
                    review_notes TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE credit_scores (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    tenant_id TEXT NOT NULL,
                    customer_name TEXT NOT NULL,
                    customer_phone TEXT NOT NULL,
                    score INTEGER NOT NULL DEFAULT 100,
                    total_orders INTEGER NOT NULL DEFAULT 0,
                    on_time_returns INTEGER NOT NULL DEFAULT 0,
                    late_returns INTEGER NOT NULL DEFAULT 0,
                    damage_incidents INTEGER NOT NULL DEFAULT 0,
                    last_calculated_at TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(tenant_id, customer_phone)
                );",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let scoped_a = provider.bind(&context("tenant-a", "credit-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "credit-b")).unwrap();
        let phone = "13800000000";

        let black_a = scoped_a
            .credits()
            .blacklist_add(
                "Alice",
                phone,
                None,
                "critical risk",
                "critical",
                "credit-admin-a",
                "2026-09-30 14:00:00",
            )
            .unwrap();
        let black_b = scoped_b
            .credits()
            .blacklist_add(
                "Bob",
                phone,
                None,
                "watch",
                "low",
                "credit-admin-b",
                "2026-09-30 14:00:01",
            )
            .unwrap();
        assert_ne!(black_a.id, 0);
        assert_ne!(black_b.id, 0);

        let duplicate = scoped_a.credits().blacklist_add(
            "Alice",
            phone,
            None,
            "again",
            "high",
            "credit-admin-a",
            "2026-09-30 14:00:02",
        );
        assert!(matches!(
            duplicate,
            Err(CreditMutationError::DuplicateBlacklist)
        ));

        let check_a = scoped_a.credits().blacklist_check(phone).unwrap();
        assert_eq!(check_a["blacklisted"], true);
        assert_eq!(check_a["records"][0]["createdBy"], "credit-admin-a");
        let decision_a = scoped_a.credits().check_before_order(phone).unwrap();
        assert_eq!(decision_a["allowed"], false);

        let decision_b = scoped_b.credits().check_before_order(phone).unwrap();
        assert_eq!(decision_b["allowed"], true);

        let violation = scoped_a
            .credits()
            .violation_record(
                "Alice",
                phone,
                None,
                "damage",
                "major",
                "screen destroyed",
                Some("photo"),
                500.0,
                "credit-reporter-a",
                "2026-09-30 14:01:00",
            )
            .unwrap();
        assert_eq!(violation.penalty, 20);

        let profile = scoped_a
            .credits()
            .credit_get(phone, "2026-09-30 14:01:01")
            .unwrap();
        assert_eq!(profile["score"], 80);
        assert_eq!(profile["damageIncidents"], 1);

        let cross_tenant =
            scoped_b
                .credits()
                .violation_appeal(violation.id, "foreign", "2026-09-30 14:02:00");
        assert!(matches!(
            cross_tenant,
            Err(CreditMutationError::ViolationNotFound)
        ));

        let appealed = scoped_a
            .credits()
            .violation_appeal(violation.id, "please review", "2026-09-30 14:02:01")
            .unwrap();
        assert_eq!(appealed.status, "appealed");

        let reviewed = scoped_a
            .credits()
            .violation_review(
                violation.id,
                "upheld",
                "confirmed",
                "credit-admin-a",
                "2026-09-30 14:03:00",
            )
            .unwrap();
        assert_eq!(reviewed.status, "upheld");

        let history = scoped_a.credits().credit_history(phone).unwrap();
        assert_eq!(history["history"].as_array().unwrap().len(), 1);
        assert_eq!(history["history"][0]["scoreImpact"], -20);

        let recalculated = scoped_a
            .credits()
            .credit_recalculate(phone, "2026-09-30 14:04:00")
            .unwrap();
        assert_eq!(recalculated.score, 90);

        scoped_a
            .credits()
            .blacklist_remove(
                black_a.id,
                "resolved",
                "credit-admin-a",
                "2026-09-30 14:05:00",
            )
            .unwrap();
        assert_eq!(
            scoped_a.credits().blacklist_check(phone).unwrap()["blacklisted"],
            false
        );
        assert_eq!(
            scoped_b.credits().blacklist_check(phone).unwrap()["blacklisted"],
            true
        );
    }
}

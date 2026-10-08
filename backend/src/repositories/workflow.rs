use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::repositories::sqlite::SqliteRepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

pub const RENTAL_DEFINITION_ID: &str = "maxwell.rental.v1";
pub const RENTAL_DEFINITION_VERSION: i64 = 1;
pub const RENTAL_DEFINITION_HASH: &str =
    "sha256:31baf0a8f2737e54c9bcce31801bdc416cb2c2f06921ef8da874b52383cf5f34";
pub const RENTAL_STEPS: &[&str] = &[
    "order_confirmed",
    "contract_required",
    "payment_required",
    "reservation_confirmed",
    "allocation_complete",
    "shipment_create_requested",
    "shipment_delivered",
    "return_received",
    "inspection_complete",
    "risk_cases_resolved",
    "settlement_complete",
    "close_order",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepState {
    Pending,
    Running,
    Succeeded,
    RetryScheduled,
    Blocked,
    Compensating,
    ManualReview,
}

impl WorkflowStepState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::RetryScheduled => "retry_scheduled",
            Self::Blocked => "blocked",
            Self::Compensating => "compensating",
            Self::ManualReview => "manual_review",
        }
    }

    fn parse(value: &str) -> Result<Self, RepositoryError> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "retry_scheduled" => Ok(Self::RetryScheduled),
            "blocked" => Ok(Self::Blocked),
            "compensating" => Ok(Self::Compensating),
            "manual_review" => Ok(Self::ManualReview),
            _ => Err(RepositoryError::ContractViolation(format!(
                "unknown workflow step state: {value}"
            ))),
        }
    }

    fn allows(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Running)
                | (Self::RetryScheduled, Self::Running)
                | (
                    Self::Running,
                    Self::Succeeded
                        | Self::RetryScheduled
                        | Self::Blocked
                        | Self::Compensating
                        | Self::ManualReview
                )
                | (Self::Blocked, Self::Pending | Self::ManualReview)
                | (Self::ManualReview, Self::Pending)
                | (
                    Self::Compensating,
                    Self::Succeeded | Self::RetryScheduled | Self::ManualReview
                )
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowInstance {
    pub id: String,
    pub source_id: String,
    pub definition_id: String,
    pub definition_version: i64,
    pub definition_hash: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub workflow_instance_id: String,
    pub step_key: String,
    pub sequence_no: i64,
    pub state: WorkflowStepState,
    pub attempt_count: i64,
    pub next_eligible_at: Option<String>,
    pub lease_owner: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedWorkflowStep {
    pub instance: WorkflowInstance,
    pub step: WorkflowStep,
}

pub struct ScopedWorkflowRepository<'a> {
    session: &'a SqliteRepositorySession,
}

impl<'a> ScopedWorkflowRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub fn start_from_message(
        &self,
        message_id: &str,
        order_id: &str,
        now: DateTime<Utc>,
    ) -> Result<WorkflowInstance, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let message_id = required(message_id, "messageId")?.to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        let now = instant(now);
        self.session.write_immediate(|tx| {
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO domain_inbox (id, tenant_id, message_id, message_type, payload_version, received_at, state)
                 VALUES (?1, ?2, ?3, 'OrderConfirmed', 1, ?4, 'received')",
                params![Uuid::new_v4().to_string(), tenant, message_id, now],
            ).map_err(sqlite)?;
            let existing = load_instance_by_source(tx, &tenant, &order_id)?;
            if inserted == 0 {
                return existing.ok_or_else(|| RepositoryError::ContractViolation("duplicate inbox message has no workflow instance".into()));
            }
            let instance = if let Some(existing) = existing { existing } else {
                create_instance(tx, &tenant, &order_id, &now)?
            };
            tx.execute("UPDATE domain_inbox SET state = 'processed', processed_at = ?3 WHERE tenant_id = ?1 AND message_id = ?2",
                params![tenant, message_id, now]).map_err(sqlite)?;
            Ok(instance)
        })
    }

    pub fn consume_domain_events(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let now = instant(now);
        self.session.write_immediate(|tx| {
            let messages: Vec<(String, String, String)> = {
                let mut statement = tx.prepare(
                    "SELECT id,message_type,payload_json FROM domain_outbox WHERE tenant_id=?1 AND state='pending' AND available_at<=?2 AND message_type IN ('OrderConfirmed','ReservationConfirmed','AllocationComplete','ReturnReceived','InspectionComplete','RiskCasesResolved','SettlementComplete') ORDER BY created_at,id LIMIT ?3",
                ).map_err(sqlite)?;
                statement
                    .query_map(params![tenant, now, limit.clamp(1, 100) as i64], |row| {
                        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                    })
                    .map_err(sqlite)?
                    .collect::<Result<_, _>>()
                    .map_err(sqlite)?
            };
            let mut processed = 0;
            for (message_id, message_type, payload_json) in messages {
                let inserted = tx.execute(
                    "INSERT OR IGNORE INTO domain_inbox (id,tenant_id,message_id,message_type,payload_version,received_at,state) VALUES (?1,?2,?1,?3,1,?4,'received')",
                    params![message_id, tenant, message_type, now],
                ).map_err(sqlite)?;
                if inserted == 1 {
                    let payload: serde_json::Value = serde_json::from_str(&payload_json)
                        .map_err(|error| RepositoryError::ContractViolation(format!("invalid durable event payload: {error}")))?;
                    let order_id = payload.get("orderId").and_then(|value| value.as_str())
                        .ok_or_else(|| RepositoryError::ContractViolation("durable event payload missing orderId".into()))?;
                    match message_type.as_str() {
                        "OrderConfirmed" => {
                            if load_instance_by_source(tx, &tenant, order_id)?.is_none() {
                                create_instance(tx, &tenant, order_id, &now)?;
                            }
                        }
                        "ReservationConfirmed" => reopen_step(tx, &tenant, order_id, "reservation_confirmed", &now)?,
                        "AllocationComplete" => reopen_step(tx, &tenant, order_id, "allocation_complete", &now)?,
                        "ReturnReceived" => reopen_step(tx, &tenant, order_id, "return_received", &now)?,
                        "InspectionComplete" => reopen_step(tx, &tenant, order_id, "inspection_complete", &now)?,
                        "RiskCasesResolved" => reopen_step(tx, &tenant, order_id, "risk_cases_resolved", &now)?,
                        "SettlementComplete" => reopen_step(tx, &tenant, order_id, "settlement_complete", &now)?,
                        _ => continue,
                    }
                    tx.execute("UPDATE domain_inbox SET state='processed',processed_at=?3 WHERE tenant_id=?1 AND message_id=?2", params![tenant,message_id,now]).map_err(sqlite)?;
                }
                tx.execute("UPDATE domain_outbox SET state='delivered',delivered_at=?3 WHERE tenant_id=?1 AND id=?2", params![tenant,message_id,now]).map_err(sqlite)?;
                processed += 1;
            }
            Ok(processed)
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<WorkflowInstance>, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        self.session
            .read(|conn| load_instance(conn, &tenant, &id).map_err(to_sqlite_read))
    }

    pub fn order_has_open_blockers(&self, order_id: &str) -> Result<bool, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        self.session.read(|connection| {
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM workflow_blockers b JOIN workflow_instances w ON w.tenant_id=b.tenant_id AND w.id=b.workflow_instance_id WHERE b.tenant_id=?1 AND w.source_kind='order' AND w.source_id=?2 AND b.status='open'",
                params![tenant, order_id], |row| row.get(0),
            )?;
            Ok(count > 0)
        })
    }

    pub fn steps(&self, instance_id: &str) -> Result<Vec<WorkflowStep>, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let instance_id = instance_id.to_owned();
        self.session.read(|conn| {
            let mut stmt = conn.prepare("SELECT id, workflow_instance_id, step_key, sequence_no, state, attempt_count, next_eligible_at, lease_owner, last_error FROM workflow_steps WHERE tenant_id=?1 AND workflow_instance_id=?2 ORDER BY sequence_no")?;
            let rows = stmt.query_map(params![tenant, instance_id], map_step)?;
            rows.collect()
        })
    }

    pub fn claim_due(
        &self,
        worker: &str,
        now: DateTime<Utc>,
        lease_seconds: i64,
    ) -> Result<Option<ClaimedWorkflowStep>, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let worker = required(worker, "worker")?.to_owned();
        let now_text = instant(now);
        let lease_until = instant(now + Duration::seconds(lease_seconds.clamp(1, 300)));
        self.session.write_immediate(|tx| {
            let candidate: Option<(String, String)> = tx.query_row(
                "SELECT s.id, s.workflow_instance_id FROM workflow_steps s JOIN workflow_instances i ON i.id=s.workflow_instance_id AND i.tenant_id=s.tenant_id
                 WHERE s.tenant_id=?1 AND i.status='active' AND s.state IN ('pending','retry_scheduled','running')
                   AND (s.next_eligible_at IS NULL OR s.next_eligible_at<=?2)
                   AND (s.state!='running' OR s.lease_expires_at IS NULL OR s.lease_expires_at<=?2)
                   AND NOT EXISTS (SELECT 1 FROM workflow_steps prior WHERE prior.tenant_id=s.tenant_id AND prior.workflow_instance_id=s.workflow_instance_id AND prior.sequence_no<s.sequence_no AND prior.state!='succeeded')
                 ORDER BY COALESCE(s.next_eligible_at,s.created_at), s.sequence_no, s.id LIMIT 1",
                params![tenant, now_text], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(sqlite)?;
            let Some((step_id, instance_id)) = candidate else { return Ok(None) };
            let changed = tx.execute(
                "UPDATE workflow_steps SET state='running', attempt_count=attempt_count+1, lease_owner=?3, lease_expires_at=?4, updated_at=?2
                 WHERE tenant_id=?1 AND id=?5 AND (state IN ('pending','retry_scheduled') OR (state='running' AND (lease_expires_at IS NULL OR lease_expires_at<=?2)))",
                params![tenant, now_text, worker, lease_until, step_id]).map_err(sqlite)?;
            if changed != 1 { return Ok(None) }
            let instance = load_instance(tx, &tenant, &instance_id)?.ok_or_else(|| RepositoryError::ContractViolation("claimed workflow instance missing".into()))?;
            let step = load_step(tx, &tenant, &step_id)?.ok_or_else(|| RepositoryError::ContractViolation("claimed workflow step missing".into()))?;
            Ok(Some(ClaimedWorkflowStep { instance, step }))
        })
    }

    pub fn transition(
        &self,
        step_id: &str,
        next: WorkflowStepState,
        now: DateTime<Utc>,
        next_eligible_at: Option<DateTime<Utc>>,
        error: Option<&str>,
    ) -> Result<WorkflowStep, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let step_id = required(step_id, "stepId")?.to_owned();
        let now = instant(now);
        self.session.write_immediate(|tx| {
            let current = load_step(tx, &tenant, &step_id)?.ok_or_else(|| RepositoryError::ContractViolation("workflow step not found".into()))?;
            if !current.state.allows(next) { return Err(RepositoryError::ContractViolation(format!("illegal workflow transition: {} -> {}", current.state.as_str(), next.as_str()))) }
            if next == WorkflowStepState::RetryScheduled && next_eligible_at.is_none() { return Err(RepositoryError::ContractViolation("retry requires next eligibility".into())) }
            tx.execute("UPDATE workflow_steps SET state=?3,next_eligible_at=?4,lease_owner=NULL,lease_expires_at=NULL,last_error=?5,updated_at=?6 WHERE tenant_id=?1 AND id=?2",
                params![tenant, step_id, next.as_str(), next_eligible_at.map(instant), error, now]).map_err(sqlite)?;
            if next == WorkflowStepState::Succeeded {
                tx.execute(
                    "UPDATE workflow_instances SET status='completed',updated_at=?3 WHERE tenant_id=?1 AND id=?2 AND NOT EXISTS (SELECT 1 FROM workflow_steps WHERE tenant_id=?1 AND workflow_instance_id=?2 AND state!='succeeded')",
                    params![tenant, current.workflow_instance_id, now],
                ).map_err(sqlite)?;
            }
            load_step(tx, &tenant, &step_id)?.ok_or_else(|| RepositoryError::ContractViolation("updated workflow step missing".into()))
        })
    }

    pub fn add_blocker(
        &self,
        instance_id: &str,
        step_key: &str,
        code: &str,
        detail: &str,
        now: DateTime<Utc>,
    ) -> Result<String, RepositoryError> {
        self.add_resolution_record(
            "workflow_blockers",
            instance_id,
            step_key,
            code,
            detail,
            now,
        )
    }

    pub fn add_manual_task(
        &self,
        instance_id: &str,
        step_key: &str,
        code: &str,
        detail: &str,
        now: DateTime<Utc>,
    ) -> Result<String, RepositoryError> {
        self.add_resolution_record(
            "manual_decision_tasks",
            instance_id,
            step_key,
            code,
            detail,
            now,
        )
    }

    pub fn resolve_blocker(
        &self,
        blocker_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        self.resolve_record("workflow_blockers", blocker_id, None, now)
    }

    pub fn resolve_manual_task(
        &self,
        task_id: &str,
        decision: &str,
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        self.resolve_record(
            "manual_decision_tasks",
            task_id,
            Some(required(decision, "decision")?),
            now,
        )
    }

    fn resolve_record(
        &self,
        table: &str,
        record_id: &str,
        decision: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let record_id = required(record_id, "recordId")?.to_owned();
        self.session.write_immediate(|tx| {
            let resolved_at = instant(now);
            let changed = if table == "workflow_blockers" {
                tx.execute(
                    "UPDATE workflow_blockers SET status='resolved',resolved_at=?3 WHERE tenant_id=?1 AND id=?2 AND status='open'",
                    params![tenant, record_id, resolved_at],
                )
            } else {
                tx.execute(
                    "UPDATE manual_decision_tasks SET status='resolved',decision=?4,resolved_at=?3 WHERE tenant_id=?1 AND id=?2 AND status='open'",
                    params![tenant, record_id, resolved_at, decision],
                )
            }
            .map_err(sqlite)?;
            if changed != 1 {
                return Err(RepositoryError::ContractViolation(
                    "resolution record not found or already resolved".into(),
                ));
            }
            Ok(())
        })
    }

    fn add_resolution_record(
        &self,
        table: &str,
        instance_id: &str,
        step_key: &str,
        code: &str,
        detail: &str,
        now: DateTime<Utc>,
    ) -> Result<String, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let id = Uuid::new_v4().to_string();
        let code_column = if table == "workflow_blockers" {
            "blocker_code"
        } else {
            "decision_code"
        };
        let sql = format!(
            "INSERT OR IGNORE INTO {table} (id,tenant_id,workflow_instance_id,step_key,{code_column},detail,status,created_at) VALUES (?1,?2,?3,?4,?5,?6,'open',?7)"
        );
        self.session.write_immediate(|tx| {
            tx.execute(
                &sql,
                params![
                    id,
                    tenant,
                    instance_id,
                    step_key,
                    code,
                    detail,
                    instant(now)
                ],
            )
            .map_err(sqlite)?;
            Ok(id.clone())
        })
    }
}

pub(in crate::repositories) fn append_outbox_tx(
    tx: &Transaction<'_>,
    tenant: &str,
    source_kind: &str,
    source_id: &str,
    message_type: &str,
    idempotency_key: &str,
    payload: &serde_json::Value,
    now: &str,
) -> Result<(), RepositoryError> {
    tx.execute("INSERT OR IGNORE INTO domain_outbox (id,tenant_id,source_kind,source_id,message_type,idempotency_key,payload_json,payload_version,state,available_at,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,1,'pending',?8,?8)",
        params![Uuid::new_v4().to_string(),tenant,source_kind,source_id,message_type,idempotency_key,payload.to_string(),now]).map_err(sqlite)?;
    Ok(())
}

fn create_instance(
    tx: &Transaction<'_>,
    tenant: &str,
    order_id: &str,
    now: &str,
) -> Result<WorkflowInstance, RepositoryError> {
    let id = Uuid::new_v4().to_string();
    tx.execute("INSERT INTO workflow_instances (id,tenant_id,source_kind,source_id,definition_id,definition_version,definition_hash,status,created_at,updated_at) VALUES (?1,?2,'order',?3,?4,?5,?6,'active',?7,?7)", params![id,tenant,order_id,RENTAL_DEFINITION_ID,RENTAL_DEFINITION_VERSION,RENTAL_DEFINITION_HASH,now]).map_err(sqlite)?;
    for (sequence, key) in RENTAL_STEPS.iter().enumerate() {
        tx.execute("INSERT INTO workflow_steps (id,tenant_id,workflow_instance_id,step_key,sequence_no,state,idempotency_key,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'pending',?6,?7,?7)", params![Uuid::new_v4().to_string(),tenant,id,key,sequence as i64,format!("workflow:{id}:{key}"),now]).map_err(sqlite)?;
    }
    load_instance(tx, tenant, &id)?
        .ok_or_else(|| RepositoryError::ContractViolation("created workflow missing".into()))
}

fn reopen_step(
    tx: &Transaction<'_>,
    tenant: &str,
    order_id: &str,
    step_key: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    tx.execute(
        "UPDATE workflow_steps SET state='pending',next_eligible_at=NULL,lease_owner=NULL,lease_expires_at=NULL,last_error=NULL,updated_at=?4
         WHERE tenant_id=?1 AND step_key=?3 AND state='blocked' AND workflow_instance_id IN
         (SELECT id FROM workflow_instances WHERE tenant_id=?1 AND source_kind='order' AND source_id=?2)",
        params![tenant, order_id, step_key, now],
    ).map_err(sqlite)?;
    Ok(())
}

fn load_instance_by_source(
    tx: &Transaction<'_>,
    tenant: &str,
    source_id: &str,
) -> Result<Option<WorkflowInstance>, RepositoryError> {
    tx.query_row("SELECT id,source_id,definition_id,definition_version,definition_hash,status FROM workflow_instances WHERE tenant_id=?1 AND source_kind='order' AND source_id=?2 AND definition_id=?3", params![tenant,source_id,RENTAL_DEFINITION_ID], map_instance).optional().map_err(sqlite)
}
fn load_instance(
    conn: &rusqlite::Connection,
    tenant: &str,
    id: &str,
) -> Result<Option<WorkflowInstance>, RepositoryError> {
    conn.query_row("SELECT id,source_id,definition_id,definition_version,definition_hash,status FROM workflow_instances WHERE tenant_id=?1 AND id=?2", params![tenant,id], map_instance).optional().map_err(sqlite)
}
fn load_step(
    conn: &rusqlite::Connection,
    tenant: &str,
    id: &str,
) -> Result<Option<WorkflowStep>, RepositoryError> {
    conn.query_row("SELECT id,workflow_instance_id,step_key,sequence_no,state,attempt_count,next_eligible_at,lease_owner,last_error FROM workflow_steps WHERE tenant_id=?1 AND id=?2", params![tenant,id], map_step).optional().map_err(sqlite)
}
fn map_instance(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowInstance> {
    Ok(WorkflowInstance {
        id: row.get(0)?,
        source_id: row.get(1)?,
        definition_id: row.get(2)?,
        definition_version: row.get(3)?,
        definition_hash: row.get(4)?,
        status: row.get(5)?,
    })
}
fn map_step(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkflowStep> {
    let state: String = row.get(4)?;
    let state = WorkflowStepState::parse(&state).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(WorkflowStep {
        id: row.get(0)?,
        workflow_instance_id: row.get(1)?,
        step_key: row.get(2)?,
        sequence_no: row.get(3)?,
        state,
        attempt_count: row.get(5)?,
        next_eligible_at: row.get(6)?,
        lease_owner: row.get(7)?,
        last_error: row.get(8)?,
    })
}
fn required<'a>(value: &'a str, name: &str) -> Result<&'a str, RepositoryError> {
    let value = value.trim();
    if value.is_empty() {
        Err(RepositoryError::ContractViolation(format!(
            "{name} must not be blank"
        )))
    } else {
        Ok(value)
    }
}
fn instant(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn sqlite(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}
fn to_sqlite_read(error: RepositoryError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

#![cfg(feature = "postgres")]

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sqlx::Row;
use sqlx::postgres::PgConnection;
use uuid::Uuid;

use crate::repositories::workflow::{
    ClaimedWorkflowStep, RENTAL_DEFINITION_HASH, RENTAL_DEFINITION_ID, RENTAL_DEFINITION_VERSION,
    RENTAL_STEPS, WorkflowInstance, WorkflowStep, WorkflowStepState,
};
use crate::repositories::{RepositoryError, RepositorySession};

pub(in crate::repositories) struct PostgresWorkflowRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresWorkflowRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn start_from_message(
        &self,
        message_id: &str,
        order_id: &str,
        now: DateTime<Utc>,
    ) -> Result<WorkflowInstance, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let message_id = required(message_id, "messageId")?.to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        let now = instant(now);

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let inserted = sqlx::query(
                        "INSERT INTO domain_inbox \
                         (id,tenant_id,message_id,message_type,payload_version,received_at,state) \
                         VALUES ($1,$2,$3,'OrderConfirmed',1,$4,'received') \
                         ON CONFLICT (tenant_id,message_id) DO NOTHING",
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&tenant)
                    .bind(&message_id)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();

                    let existing = load_instance_by_source(connection, &tenant, &order_id).await?;
                    if inserted == 0 {
                        return existing.ok_or_else(|| {
                            RepositoryError::ContractViolation(
                                "duplicate inbox message has no workflow instance".into(),
                            )
                        });
                    }

                    let instance = match existing {
                        Some(existing) => existing,
                        None => create_instance(connection, &tenant, &order_id, &now).await?,
                    };

                    sqlx::query(
                        "UPDATE domain_inbox SET state='processed',processed_at=$3 \
                         WHERE tenant_id=$1 AND message_id=$2",
                    )
                    .bind(&tenant)
                    .bind(&message_id)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    Ok(instance)
                })
            })
    }

    pub(in crate::repositories) fn consume_domain_events(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> Result<usize, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let now = instant(now);
        let limit = limit.clamp(1, 100) as i64;

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let rows = sqlx::query(
                        "SELECT id,message_type,payload_json \
                         FROM domain_outbox \
                         WHERE tenant_id=$1 AND state='pending' AND available_at<=$2 \
                           AND message_type IN \
                           ('OrderConfirmed','ReservationConfirmed','AllocationComplete', \
                            'ReturnReceived','InspectionComplete','RiskCasesResolved','SettlementComplete') \
                         ORDER BY created_at,id \
                         LIMIT $3 \
                         FOR UPDATE SKIP LOCKED",
                    )
                    .bind(&tenant)
                    .bind(&now)
                    .bind(limit)
                    .fetch_all(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let mut processed = 0usize;
                    for row in rows {
                        let message_id: String = row.try_get("id").map_err(pg_error)?;
                        let message_type: String =
                            row.try_get("message_type").map_err(pg_error)?;
                        let payload_json: String =
                            row.try_get("payload_json").map_err(pg_error)?;

                        let inserted = sqlx::query(
                            "INSERT INTO domain_inbox \
                             (id,tenant_id,message_id,message_type,payload_version,received_at,state) \
                             VALUES ($1,$2,$1,$3,1,$4,'received') \
                             ON CONFLICT (tenant_id,message_id) DO NOTHING",
                        )
                        .bind(&message_id)
                        .bind(&tenant)
                        .bind(&message_type)
                        .bind(&now)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?
                        .rows_affected();

                        if inserted == 1 {
                            let payload: serde_json::Value = serde_json::from_str(&payload_json)
                                .map_err(|error| {
                                    RepositoryError::ContractViolation(format!(
                                        "invalid durable event payload: {error}"
                                    ))
                                })?;
                            let order_id = payload
                                .get("orderId")
                                .and_then(|value| value.as_str())
                                .ok_or_else(|| {
                                    RepositoryError::ContractViolation(
                                        "durable event payload missing orderId".into(),
                                    )
                                })?;

                            match message_type.as_str() {
                                "OrderConfirmed" => {
                                    if load_instance_by_source(connection, &tenant, order_id)
                                        .await?
                                        .is_none()
                                    {
                                        create_instance(connection, &tenant, order_id, &now)
                                            .await?;
                                    }
                                }
                                "ReservationConfirmed" => {
                                    reopen_step(
                                        connection,
                                        &tenant,
                                        order_id,
                                        "reservation_confirmed",
                                        &now,
                                    )
                                    .await?;
                                }
                                "AllocationComplete" => {
                                    reopen_step(
                                        connection,
                                        &tenant,
                                        order_id,
                                        "allocation_complete",
                                        &now,
                                    )
                                    .await?;
                                }
                                "ReturnReceived" => {
                                    reopen_step(
                                        connection,
                                        &tenant,
                                        order_id,
                                        "return_received",
                                        &now,
                                    )
                                    .await?;
                                }
                                "InspectionComplete" => {
                                    reopen_step(
                                        connection,
                                        &tenant,
                                        order_id,
                                        "inspection_complete",
                                        &now,
                                    )
                                    .await?;
                                }
                                "RiskCasesResolved" => {
                                    reopen_step(
                                        connection,
                                        &tenant,
                                        order_id,
                                        "risk_cases_resolved",
                                        &now,
                                    )
                                    .await?;
                                }
                                "SettlementComplete" => {
                                    reopen_step(
                                        connection,
                                        &tenant,
                                        order_id,
                                        "settlement_complete",
                                        &now,
                                    )
                                    .await?;
                                }
                                _ => {}
                            }

                            sqlx::query(
                                "UPDATE domain_inbox SET state='processed',processed_at=$3 \
                                 WHERE tenant_id=$1 AND message_id=$2",
                            )
                            .bind(&tenant)
                            .bind(&message_id)
                            .bind(&now)
                            .execute(&mut *connection)
                            .await
                            .map_err(pg_error)?;
                        }

                        sqlx::query(
                            "UPDATE domain_outbox SET state='delivered',delivered_at=$3 \
                             WHERE tenant_id=$1 AND id=$2",
                        )
                        .bind(&tenant)
                        .bind(&message_id)
                        .bind(&now)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?;
                        processed += 1;
                    }

                    Ok(processed)
                })
            })
    }

    pub(in crate::repositories) fn get(
        &self,
        id: &str,
    ) -> Result<Option<WorkflowInstance>, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move { load_instance_sqlx(connection, &tenant, &id).await })
        })
    }

    pub(in crate::repositories) fn order_has_open_blockers(
        &self,
        order_id: &str,
    ) -> Result<bool, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let order_id = required(order_id, "orderId")?.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let count = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) \
                     FROM workflow_blockers b \
                     JOIN workflow_instances w \
                       ON w.tenant_id=b.tenant_id AND w.id=b.workflow_instance_id \
                     WHERE b.tenant_id=$1 AND w.source_kind='order' \
                       AND w.source_id=$2 AND b.status='open'",
                )
                .bind(&tenant)
                .bind(&order_id)
                .fetch_one(&mut *connection)
                .await?;
                Ok(count > 0)
            })
        })
    }

    pub(in crate::repositories) fn steps(
        &self,
        instance_id: &str,
    ) -> Result<Vec<WorkflowStep>, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let instance_id = instance_id.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    "SELECT id,workflow_instance_id,step_key,sequence_no,state,attempt_count, \
                            next_eligible_at,lease_owner,last_error \
                     FROM workflow_steps \
                     WHERE tenant_id=$1 AND workflow_instance_id=$2 \
                     ORDER BY sequence_no",
                )
                .bind(&tenant)
                .bind(&instance_id)
                .fetch_all(&mut *connection)
                .await?;
                rows.into_iter().map(map_step_row_sqlx).collect()
            })
        })
    }

    pub(in crate::repositories) fn claim_due(
        &self,
        worker: &str,
        now: DateTime<Utc>,
        lease_seconds: i64,
    ) -> Result<Option<ClaimedWorkflowStep>, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let worker = required(worker, "worker")?.to_owned();
        let now_text = instant(now);
        let lease_until = instant(now + Duration::seconds(lease_seconds.clamp(1, 300)));

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let candidate = sqlx::query(
                        "SELECT s.id,s.workflow_instance_id \
                         FROM workflow_steps s \
                         JOIN workflow_instances i \
                           ON i.id=s.workflow_instance_id AND i.tenant_id=s.tenant_id \
                         WHERE s.tenant_id=$1 AND i.status='active' \
                           AND s.state IN ('pending','retry_scheduled','running') \
                           AND (s.next_eligible_at IS NULL OR s.next_eligible_at<=$2) \
                           AND (s.state!='running' OR s.lease_expires_at IS NULL OR s.lease_expires_at<=$2) \
                           AND NOT EXISTS ( \
                             SELECT 1 FROM workflow_steps prior \
                             WHERE prior.tenant_id=s.tenant_id \
                               AND prior.workflow_instance_id=s.workflow_instance_id \
                               AND prior.sequence_no<s.sequence_no \
                               AND prior.state!='succeeded') \
                         ORDER BY COALESCE(s.next_eligible_at,s.created_at),s.sequence_no,s.id \
                         LIMIT 1 \
                         FOR UPDATE OF s SKIP LOCKED",
                    )
                    .bind(&tenant)
                    .bind(&now_text)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    let Some(candidate) = candidate else {
                        return Ok(None);
                    };
                    let step_id: String = candidate.try_get("id").map_err(pg_error)?;
                    let instance_id: String = candidate
                        .try_get("workflow_instance_id")
                        .map_err(pg_error)?;

                    let changed = sqlx::query(
                        "UPDATE workflow_steps \
                         SET state='running',attempt_count=attempt_count+1,lease_owner=$3, \
                             lease_expires_at=$4,updated_at=$2 \
                         WHERE tenant_id=$1 AND id=$5 \
                           AND (state IN ('pending','retry_scheduled') \
                                OR (state='running' AND \
                                    (lease_expires_at IS NULL OR lease_expires_at<=$2)))",
                    )
                    .bind(&tenant)
                    .bind(&now_text)
                    .bind(&worker)
                    .bind(&lease_until)
                    .bind(&step_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?
                    .rows_affected();

                    if changed != 1 {
                        return Ok(None);
                    }

                    let instance = load_instance_repo(connection, &tenant, &instance_id)
                        .await?
                        .ok_or_else(|| {
                            RepositoryError::ContractViolation(
                                "claimed workflow instance missing".into(),
                            )
                        })?;
                    let step = load_step_repo(connection, &tenant, &step_id)
                        .await?
                        .ok_or_else(|| {
                            RepositoryError::ContractViolation(
                                "claimed workflow step missing".into(),
                            )
                        })?;
                    Ok(Some(ClaimedWorkflowStep { instance, step }))
                })
            })
    }

    pub(in crate::repositories) fn transition(
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
        let next_eligible_at = next_eligible_at.map(instant);
        let error = error.map(str::to_owned);

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let row = sqlx::query(
                        "SELECT id,workflow_instance_id,step_key,sequence_no,state,attempt_count, \
                                next_eligible_at,lease_owner,last_error \
                         FROM workflow_steps \
                         WHERE tenant_id=$1 AND id=$2 \
                         FOR UPDATE",
                    )
                    .bind(&tenant)
                    .bind(&step_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                    let Some(row) = row else {
                        return Err(RepositoryError::ContractViolation(
                            "workflow step not found".into(),
                        ));
                    };
                    let current = map_step_row(row)?;
                    if !allows_transition(current.state, next) {
                        return Err(RepositoryError::ContractViolation(format!(
                            "illegal workflow transition: {} -> {}",
                            current.state.as_str(),
                            next.as_str()
                        )));
                    }
                    if next == WorkflowStepState::RetryScheduled && next_eligible_at.is_none() {
                        return Err(RepositoryError::ContractViolation(
                            "retry requires next eligibility".into(),
                        ));
                    }

                    sqlx::query(
                        "UPDATE workflow_steps \
                         SET state=$3,next_eligible_at=$4,lease_owner=NULL,lease_expires_at=NULL, \
                             last_error=$5,updated_at=$6 \
                         WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(&tenant)
                    .bind(&step_id)
                    .bind(next.as_str())
                    .bind(&next_eligible_at)
                    .bind(&error)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;

                    if next == WorkflowStepState::Succeeded {
                        sqlx::query(
                            "UPDATE workflow_instances \
                             SET status='completed',updated_at=$3 \
                             WHERE tenant_id=$1 AND id=$2 \
                               AND NOT EXISTS ( \
                                 SELECT 1 FROM workflow_steps \
                                 WHERE tenant_id=$1 AND workflow_instance_id=$2 \
                                   AND state!='succeeded')",
                        )
                        .bind(&tenant)
                        .bind(&current.workflow_instance_id)
                        .bind(&now)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?;
                    }

                    load_step_repo(connection, &tenant, &step_id)
                        .await?
                        .ok_or_else(|| {
                            RepositoryError::ContractViolation(
                                "updated workflow step missing".into(),
                            )
                        })
                })
            })
    }

    pub(in crate::repositories) fn add_blocker(
        &self,
        instance_id: &str,
        step_key: &str,
        code: &str,
        detail: &str,
        now: DateTime<Utc>,
    ) -> Result<String, RepositoryError> {
        self.add_resolution_record(
            ResolutionKind::Blocker,
            instance_id,
            step_key,
            code,
            detail,
            now,
        )
    }

    pub(in crate::repositories) fn add_manual_task(
        &self,
        instance_id: &str,
        step_key: &str,
        code: &str,
        detail: &str,
        now: DateTime<Utc>,
    ) -> Result<String, RepositoryError> {
        self.add_resolution_record(
            ResolutionKind::ManualTask,
            instance_id,
            step_key,
            code,
            detail,
            now,
        )
    }

    pub(in crate::repositories) fn resolve_blocker(
        &self,
        blocker_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        self.resolve_record(ResolutionKind::Blocker, blocker_id, None, now)
    }

    pub(in crate::repositories) fn resolve_manual_task(
        &self,
        task_id: &str,
        decision: &str,
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        self.resolve_record(
            ResolutionKind::ManualTask,
            task_id,
            Some(required(decision, "decision")?.to_owned()),
            now,
        )
    }

    fn add_resolution_record(
        &self,
        kind: ResolutionKind,
        instance_id: &str,
        step_key: &str,
        code: &str,
        detail: &str,
        now: DateTime<Utc>,
    ) -> Result<String, RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let instance_id = required(instance_id, "instanceId")?.to_owned();
        let step_key = required(step_key, "stepKey")?.to_owned();
        let code = required(code, "code")?.to_owned();
        let detail = required(detail, "detail")?.to_owned();
        let id = Uuid::new_v4().to_string();
        let id_for_tx = id.clone();
        let now = instant(now);

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    match kind {
                        ResolutionKind::Blocker => {
                            sqlx::query(
                                "INSERT INTO workflow_blockers \
                                 (id,tenant_id,workflow_instance_id,step_key,blocker_code,detail,status,created_at) \
                                 VALUES ($1,$2,$3,$4,$5,$6,'open',$7) \
                                 ON CONFLICT DO NOTHING",
                            )
                            .bind(&id_for_tx)
                            .bind(&tenant)
                            .bind(&instance_id)
                            .bind(&step_key)
                            .bind(&code)
                            .bind(&detail)
                            .bind(&now)
                            .execute(&mut *connection)
                            .await
                            .map_err(pg_error)?;
                        }
                        ResolutionKind::ManualTask => {
                            sqlx::query(
                                "INSERT INTO manual_decision_tasks \
                                 (id,tenant_id,workflow_instance_id,step_key,decision_code,detail,status,created_at) \
                                 VALUES ($1,$2,$3,$4,$5,$6,'open',$7) \
                                 ON CONFLICT DO NOTHING",
                            )
                            .bind(&id_for_tx)
                            .bind(&tenant)
                            .bind(&instance_id)
                            .bind(&step_key)
                            .bind(&code)
                            .bind(&detail)
                            .bind(&now)
                            .execute(&mut *connection)
                            .await
                            .map_err(pg_error)?;
                        }
                    }
                    Ok(())
                })
            })?;

        Ok(id)
    }

    fn resolve_record(
        &self,
        kind: ResolutionKind,
        record_id: &str,
        decision: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), RepositoryError> {
        let tenant = self.session.binding().tenant_id().as_str().to_owned();
        let record_id = required(record_id, "recordId")?.to_owned();
        let resolved_at = instant(now);

        self.session
            .pg_write_serializable_repository(move |connection| {
                Box::pin(async move {
                    let changed = match kind {
                        ResolutionKind::Blocker => sqlx::query(
                            "UPDATE workflow_blockers \
                             SET status='resolved',resolved_at=$3 \
                             WHERE tenant_id=$1 AND id=$2 AND status='open'",
                        )
                        .bind(&tenant)
                        .bind(&record_id)
                        .bind(&resolved_at)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?
                        .rows_affected(),
                        ResolutionKind::ManualTask => sqlx::query(
                            "UPDATE manual_decision_tasks \
                             SET status='resolved',decision=$4,resolved_at=$3 \
                             WHERE tenant_id=$1 AND id=$2 AND status='open'",
                        )
                        .bind(&tenant)
                        .bind(&record_id)
                        .bind(&resolved_at)
                        .bind(&decision)
                        .execute(&mut *connection)
                        .await
                        .map_err(pg_error)?
                        .rows_affected(),
                    };
                    if changed != 1 {
                        return Err(RepositoryError::ContractViolation(
                            "resolution record not found or already resolved".into(),
                        ));
                    }
                    Ok(())
                })
            })
    }
}

#[derive(Clone, Copy)]
enum ResolutionKind {
    Blocker,
    ManualTask,
}

async fn create_instance(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
    now: &str,
) -> Result<WorkflowInstance, RepositoryError> {
    let id = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO workflow_instances \
         (id,tenant_id,source_kind,source_id,definition_id,definition_version,definition_hash,status,created_at,updated_at) \
         VALUES ($1,$2,'order',$3,$4,$5,$6,'active',$7,$7)",
    )
    .bind(&id)
    .bind(tenant)
    .bind(order_id)
    .bind(RENTAL_DEFINITION_ID)
    .bind(RENTAL_DEFINITION_VERSION)
    .bind(RENTAL_DEFINITION_HASH)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?;

    for (sequence, key) in RENTAL_STEPS.iter().enumerate() {
        sqlx::query(
            "INSERT INTO workflow_steps \
             (id,tenant_id,workflow_instance_id,step_key,sequence_no,state,idempotency_key,created_at,updated_at) \
             VALUES ($1,$2,$3,$4,$5,'pending',$6,$7,$7)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(tenant)
        .bind(&id)
        .bind(*key)
        .bind(sequence as i64)
        .bind(format!("workflow:{id}:{key}"))
        .bind(now)
        .execute(&mut *connection)
        .await
        .map_err(pg_error)?;
    }

    load_instance_repo(connection, tenant, &id)
        .await?
        .ok_or_else(|| RepositoryError::ContractViolation("created workflow missing".into()))
}

async fn reopen_step(
    connection: &mut PgConnection,
    tenant: &str,
    order_id: &str,
    step_key: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    sqlx::query(
        "UPDATE workflow_steps \
         SET state='pending',next_eligible_at=NULL,lease_owner=NULL,lease_expires_at=NULL, \
             last_error=NULL,updated_at=$4 \
         WHERE tenant_id=$1 AND step_key=$3 AND state='blocked' \
           AND workflow_instance_id IN ( \
             SELECT id FROM workflow_instances \
             WHERE tenant_id=$1 AND source_kind='order' AND source_id=$2)",
    )
    .bind(tenant)
    .bind(order_id)
    .bind(step_key)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?;
    Ok(())
}

async fn load_instance_by_source(
    connection: &mut PgConnection,
    tenant: &str,
    source_id: &str,
) -> Result<Option<WorkflowInstance>, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,source_id,definition_id,definition_version,definition_hash,status \
         FROM workflow_instances \
         WHERE tenant_id=$1 AND source_kind='order' AND source_id=$2 AND definition_id=$3",
    )
    .bind(tenant)
    .bind(source_id)
    .bind(RENTAL_DEFINITION_ID)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    row.map(map_instance_row).transpose()
}

async fn load_instance_sqlx(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<Option<WorkflowInstance>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id,source_id,definition_id,definition_version,definition_hash,status \
         FROM workflow_instances WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;
    row.map(map_instance_row_sqlx).transpose()
}

async fn load_instance_repo(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<Option<WorkflowInstance>, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,source_id,definition_id,definition_version,definition_hash,status \
         FROM workflow_instances WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    row.map(map_instance_row).transpose()
}

async fn load_step_repo(
    connection: &mut PgConnection,
    tenant: &str,
    id: &str,
) -> Result<Option<WorkflowStep>, RepositoryError> {
    let row = sqlx::query(
        "SELECT id,workflow_instance_id,step_key,sequence_no,state,attempt_count, \
                next_eligible_at,lease_owner,last_error \
         FROM workflow_steps WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;
    row.map(map_step_row).transpose()
}

fn map_instance_row(row: sqlx::postgres::PgRow) -> Result<WorkflowInstance, RepositoryError> {
    map_instance_row_sqlx(row).map_err(pg_error)
}

fn map_instance_row_sqlx(row: sqlx::postgres::PgRow) -> Result<WorkflowInstance, sqlx::Error> {
    Ok(WorkflowInstance {
        id: row.try_get("id")?,
        source_id: row.try_get("source_id")?,
        definition_id: row.try_get("definition_id")?,
        definition_version: row.try_get("definition_version")?,
        definition_hash: row.try_get("definition_hash")?,
        status: row.try_get("status")?,
    })
}

fn map_step_row(row: sqlx::postgres::PgRow) -> Result<WorkflowStep, RepositoryError> {
    map_step_row_sqlx(row).map_err(pg_error)
}

fn map_step_row_sqlx(row: sqlx::postgres::PgRow) -> Result<WorkflowStep, sqlx::Error> {
    let raw_state: String = row.try_get("state")?;
    let state = parse_state(&raw_state).map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
    Ok(WorkflowStep {
        id: row.try_get("id")?,
        workflow_instance_id: row.try_get("workflow_instance_id")?,
        step_key: row.try_get("step_key")?,
        sequence_no: row.try_get("sequence_no")?,
        state,
        attempt_count: row.try_get("attempt_count")?,
        next_eligible_at: row.try_get("next_eligible_at")?,
        lease_owner: row.try_get("lease_owner")?,
        last_error: row.try_get("last_error")?,
    })
}

fn parse_state(value: &str) -> Result<WorkflowStepState, RepositoryError> {
    match value {
        "pending" => Ok(WorkflowStepState::Pending),
        "running" => Ok(WorkflowStepState::Running),
        "succeeded" => Ok(WorkflowStepState::Succeeded),
        "retry_scheduled" => Ok(WorkflowStepState::RetryScheduled),
        "blocked" => Ok(WorkflowStepState::Blocked),
        "compensating" => Ok(WorkflowStepState::Compensating),
        "manual_review" => Ok(WorkflowStepState::ManualReview),
        _ => Err(RepositoryError::ContractViolation(format!(
            "unknown workflow step state: {value}"
        ))),
    }
}

fn allows_transition(current: WorkflowStepState, next: WorkflowStepState) -> bool {
    matches!(
        (current, next),
        (WorkflowStepState::Pending, WorkflowStepState::Running)
            | (
                WorkflowStepState::RetryScheduled,
                WorkflowStepState::Running
            )
            | (
                WorkflowStepState::Running,
                WorkflowStepState::Succeeded
                    | WorkflowStepState::RetryScheduled
                    | WorkflowStepState::Blocked
                    | WorkflowStepState::Compensating
                    | WorkflowStepState::ManualReview
            )
            | (
                WorkflowStepState::Blocked,
                WorkflowStepState::Pending | WorkflowStepState::ManualReview
            )
            | (WorkflowStepState::ManualReview, WorkflowStepState::Pending)
            | (
                WorkflowStepState::Compensating,
                WorkflowStepState::Succeeded
                    | WorkflowStepState::RetryScheduled
                    | WorkflowStepState::ManualReview
            )
    )
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

fn pg_error(error: sqlx::Error) -> RepositoryError {
    RepositoryError::Postgres(error.to_string())
}

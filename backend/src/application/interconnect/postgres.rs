//! PostgreSQL 18 durable reference driver for the R4 Interconnect Fabric.
//!
//! This driver owns generic Event / Work / State durability only. It does not
//! dispatch business Commands and it does not decide ExternalOperation outcome.

use std::collections::BTreeSet;

use sqlx::{PgPool, Postgres, Row, Transaction};
use system_core::ExecutionContext;
use system_core::transport::interconnect::{
    CallRequirements, ConsumerId, ContractVersion, DriverDescriptor, EventCursor,
    InterconnectError, InterconnectErrorCode, LeaseOwner, MessageEnvelope, MessageId, MessageKind,
    ResumeDirective, RetentionMetadata, StateRevision, Subject, TransportCapability, WatchCursor,
    WorkClaim,
};

use super::{
    EventRecord, StateDeltaRecord, StateSnapshotRecord, StateWatchBatch, WorkMetrics,
    trusted_tenant, validate_lane,
};

const MIGRATION_SQL: &str =
    include_str!("../../db/migrations/postgres/067_r4_interconnect_fabric.sql");

#[derive(Clone)]
pub struct PostgresDurableDriver {
    pool: PgPool,
}

impl PostgresDurableDriver {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn descriptor(&self) -> DriverDescriptor {
        DriverDescriptor {
            name: "postgresql18_durable".into(),
            version: ContractVersion::new("1.0.0").expect("static driver version"),
            capabilities: BTreeSet::from([
                TransportCapability::Durable,
                TransportCapability::Replayable,
                TransportCapability::Fanout,
                TransportCapability::CompetingConsumers,
                TransportCapability::OrderedByKey,
                TransportCapability::ConsumerCheckpoint,
                TransportCapability::DelayedDelivery,
                TransportCapability::DeadLetter,
                TransportCapability::TransactionalPublish,
                TransportCapability::FlowControl,
                TransportCapability::ContentReference,
            ]),
        }
    }

    pub fn require(&self, requirements: &CallRequirements) -> Result<(), InterconnectError> {
        self.descriptor().satisfies(requirements)
    }

    /// Test-only/idempotent bootstrap for isolated schemas. Production startup
    /// uses `db::run_all_pg_migrations`, so this helper is not a second schema
    /// authority.
    pub async fn ensure_schema(&self) -> Result<(), InterconnectError> {
        sqlx::raw_sql(MIGRATION_SQL)
            .execute(&self.pool)
            .await
            .map_err(db_error)?;
        Ok(())
    }

    pub async fn append_event(
        &self,
        ctx: &ExecutionContext,
        envelope: MessageEnvelope,
    ) -> Result<u64, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        validate_lane(&envelope, MessageKind::Event, ctx)?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let sequence = append_event_tx(&mut tx, &tenant, &envelope).await?;
        tx.commit().await.map_err(db_error)?;
        Ok(sequence)
    }

    pub async fn event_retention(
        &self,
        ctx: &ExecutionContext,
        subject: &Subject,
    ) -> Result<RetentionMetadata, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let row = sqlx::query(
            "SELECT MIN(sequence) AS earliest, MAX(sequence) AS latest \
             FROM interconnect_events WHERE tenant_id=$1 AND subject=$2",
        )
        .bind(&tenant)
        .bind(subject.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        let earliest: Option<i64> = row.try_get("earliest").map_err(db_error)?;
        let latest: Option<i64> = row.try_get("latest").map_err(db_error)?;
        Ok(RetentionMetadata {
            earliest_sequence: earliest
                .map(|value| from_i64(value, "event retention cursor"))
                .transpose()?
                .unwrap_or(0),
            latest_sequence: latest
                .map(|value| from_i64(value, "event retention cursor"))
                .transpose()?
                .unwrap_or(0),
            retention_seconds: None,
        })
    }

    pub async fn replay_events(
        &self,
        ctx: &ExecutionContext,
        consumer: ConsumerId,
        subject: Subject,
        after_sequence: u64,
        limit: usize,
        now_ms: u64,
    ) -> Result<Vec<EventRecord>, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        if limit == 0 || limit > 1000 {
            return Err(resource_error("event replay limit must be 1..=1000"));
        }
        let after = to_i64(after_sequence, "event cursor")?;
        let rows = sqlx::query(
            "SELECT sequence,tenant_id,envelope::text AS envelope_text \
             FROM interconnect_events \
             WHERE tenant_id=$1 AND subject=$2 AND sequence>$3 \
             ORDER BY sequence ASC LIMIT $4",
        )
        .bind(&tenant)
        .bind(subject.as_str())
        .bind(after)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            records.push(EventRecord {
                sequence: from_i64(row.try_get("sequence").map_err(db_error)?, "event sequence")?,
                tenant_id: row.try_get("tenant_id").map_err(db_error)?,
                envelope: decode_envelope(row.try_get("envelope_text").map_err(db_error)?)?,
            });
        }
        if let Some(last) = records.last() {
            self.checkpoint_event(ctx, consumer, subject, last.sequence, now_ms)
                .await?;
        }
        Ok(records)
    }

    pub async fn checkpoint_event(
        &self,
        ctx: &ExecutionContext,
        consumer: ConsumerId,
        subject: Subject,
        sequence: u64,
        now_ms: u64,
    ) -> Result<EventCursor, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let sequence_i64 = to_i64(sequence, "event cursor")?;
        let now = to_i64(now_ms, "checkpoint time")?;
        let cursor: i64 = sqlx::query_scalar(
            "INSERT INTO interconnect_event_consumers \
                (tenant_id,consumer_id,subject,cursor_sequence,updated_at_ms) \
             VALUES ($1,$2,$3,$4,$5) \
             ON CONFLICT (tenant_id,consumer_id,subject) DO UPDATE SET \
                cursor_sequence=GREATEST(interconnect_event_consumers.cursor_sequence,EXCLUDED.cursor_sequence), \
                updated_at_ms=EXCLUDED.updated_at_ms \
             RETURNING cursor_sequence",
        )
        .bind(&tenant)
        .bind(consumer.as_str())
        .bind(subject.as_str())
        .bind(sequence_i64)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(EventCursor {
            consumer,
            subject,
            sequence: from_i64(cursor, "event cursor")?,
        })
    }

    pub async fn enqueue_work(
        &self,
        ctx: &ExecutionContext,
        envelope: MessageEnvelope,
        retry_budget: u32,
        available_at_ms: u64,
    ) -> Result<MessageId, InterconnectError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let (work_id, _) = self
            .enqueue_work_in_transaction(&mut tx, ctx, &envelope, retry_budget, available_at_ms)
            .await?;
        let plugin_pinned: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM interconnect_plugin_work_pins WHERE work_id=$1)",
        )
        .bind(work_id.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if plugin_pinned {
            return Err(identity_conflict(
                "generic work admission cannot reuse plugin-backed work identity",
            ));
        }
        tx.commit().await.map_err(db_error)?;
        Ok(work_id)
    }

    /// Transaction-scoped Work admission. The boolean is true only if this
    /// transaction inserted the Work row. Plugin admission uses this to make
    /// executable pinning and Work identity one atomic durability boundary.
    pub(crate) async fn enqueue_work_in_transaction(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        ctx: &ExecutionContext,
        envelope: &MessageEnvelope,
        retry_budget: u32,
        available_at_ms: u64,
    ) -> Result<(MessageId, bool), InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        validate_lane(envelope, MessageKind::Work, ctx)?;
        if retry_budget > 100 {
            return Err(resource_error("work retry budget exceeds 100"));
        }
        enqueue_work_tx(tx, &tenant, envelope, retry_budget, available_at_ms).await
    }

    pub async fn claim_work(
        &self,
        ctx: &ExecutionContext,
        subject: &Subject,
        lease_owner: LeaseOwner,
        now_ms: u64,
        lease_ms: u64,
    ) -> Result<Option<WorkClaim>, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        if lease_ms == 0 || lease_ms > 3_600_000 {
            return Err(resource_error("work lease must be 1..=3600000 ms"));
        }
        let now = to_i64(now_ms, "claim time")?;
        let deadline = to_i64(now_ms.saturating_add(lease_ms), "lease deadline")?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        sqlx::query(
            "UPDATE interconnect_work SET status='dead_letter',lease_owner=NULL,lease_deadline_ms=NULL \
             WHERE tenant_id=$1 AND subject=$2 AND status='leased' AND lease_deadline_ms<=$3 \
               AND attempt >= retry_budget + 1",
        )
        .bind(&tenant)
        .bind(subject.as_str())
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        let row = sqlx::query(
            "WITH candidate AS ( \
                SELECT work_id FROM interconnect_work \
                WHERE tenant_id=$1 AND subject=$2 AND available_at_ms<=$3 \
                  AND attempt < retry_budget + 1 \
                  AND (status='ready' OR (status='leased' AND lease_deadline_ms<=$3)) \
                ORDER BY available_at_ms,created_at_ms,work_id \
                FOR UPDATE SKIP LOCKED LIMIT 1 \
             ) \
             UPDATE interconnect_work AS w SET \
                status='leased',claim_generation=w.claim_generation+1,attempt=w.attempt+1, \
                lease_owner=$4,lease_deadline_ms=$5 \
             FROM candidate WHERE w.work_id=candidate.work_id \
             RETURNING w.work_id,w.claim_generation,w.attempt,w.retry_budget,w.lease_deadline_ms",
        )
        .bind(&tenant)
        .bind(subject.as_str())
        .bind(now)
        .bind(lease_owner.as_str())
        .bind(deadline)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(WorkClaim {
            work_id: MessageId::new(row.try_get::<String, _>("work_id").map_err(db_error)?)?,
            claim_generation: from_i64(
                row.try_get("claim_generation").map_err(db_error)?,
                "claim generation",
            )?,
            lease_owner,
            lease_deadline_ms: from_i64(
                row.try_get("lease_deadline_ms").map_err(db_error)?,
                "lease deadline",
            )?,
            attempt: row.try_get::<i32, _>("attempt").map_err(db_error)? as u32,
            retry_budget: row.try_get::<i32, _>("retry_budget").map_err(db_error)? as u32,
        }))
    }

    pub async fn ack_work(
        &self,
        ctx: &ExecutionContext,
        claim: &WorkClaim,
    ) -> Result<(), InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let generation = to_i64(claim.claim_generation, "claim generation")?;
        let affected = sqlx::query(
            "UPDATE interconnect_work SET status='succeeded',lease_owner=NULL,lease_deadline_ms=NULL \
             WHERE work_id=$1 AND tenant_id=$2 AND status='leased' \
               AND claim_generation=$3 AND lease_owner=$4",
        )
        .bind(claim.work_id.as_str())
        .bind(&tenant)
        .bind(generation)
        .bind(claim.lease_owner.as_str())
        .execute(&self.pool)
        .await
        .map_err(db_error)?
        .rows_affected();
        require_current_claim(affected)
    }

    pub async fn nack_work(
        &self,
        ctx: &ExecutionContext,
        claim: &WorkClaim,
        retryable: bool,
        error: &str,
        available_at_ms: u64,
    ) -> Result<(), InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let generation = to_i64(claim.claim_generation, "claim generation")?;
        let available = to_i64(available_at_ms, "retry availability")?;
        let affected = sqlx::query(
            "UPDATE interconnect_work SET \
                status=CASE WHEN $5 AND attempt <= retry_budget THEN 'ready' ELSE 'dead_letter' END, \
                lease_owner=NULL,lease_deadline_ms=NULL,last_error=$6,available_at_ms=$7 \
             WHERE work_id=$1 AND tenant_id=$2 AND status='leased' \
               AND claim_generation=$3 AND lease_owner=$4",
        )
        .bind(claim.work_id.as_str())
        .bind(&tenant)
        .bind(generation)
        .bind(claim.lease_owner.as_str())
        .bind(retryable)
        .bind(error)
        .bind(available)
        .execute(&self.pool)
        .await
        .map_err(db_error)?
        .rows_affected();
        require_current_claim(affected)
    }

    pub async fn work_metrics(
        &self,
        ctx: &ExecutionContext,
        subject: &Subject,
        now_ms: u64,
    ) -> Result<WorkMetrics, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        let row = sqlx::query(
            "SELECT \
                COUNT(*) FILTER (WHERE status='ready') AS ready, \
                COUNT(*) FILTER (WHERE status='leased') AS leased, \
                COUNT(*) FILTER (WHERE status='dead_letter') AS dead_letter, \
                MIN(created_at_ms) FILTER (WHERE status='ready') AS oldest_ready \
             FROM interconnect_work WHERE tenant_id=$1 AND subject=$2",
        )
        .bind(&tenant)
        .bind(subject.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        let oldest: Option<i64> = row.try_get("oldest_ready").map_err(db_error)?;
        Ok(WorkMetrics {
            ready: row.try_get::<i64, _>("ready").map_err(db_error)? as usize,
            leased: row.try_get::<i64, _>("leased").map_err(db_error)? as usize,
            dead_letter: row.try_get::<i64, _>("dead_letter").map_err(db_error)? as usize,
            oldest_ready_age_ms: oldest
                .and_then(|value| u64::try_from(value).ok())
                .map_or(0, |created| now_ms.saturating_sub(created)),
        })
    }

    pub async fn put_state_snapshot(
        &self,
        ctx: &ExecutionContext,
        revision: StateRevision,
        envelope: MessageEnvelope,
        now_ms: u64,
    ) -> Result<(), InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        validate_lane(&envelope, MessageKind::StateSnapshot, ctx)?;
        let encoded = encode_envelope(&envelope)?;
        let revision_i64 = to_i64(revision.0, "state revision")?;
        let now = to_i64(now_ms, "state update time")?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        advance_state_head(
            &mut tx,
            &tenant,
            envelope.subject.as_str(),
            revision_i64,
            now,
        )
        .await?;
        sqlx::query(
            "INSERT INTO interconnect_state_snapshots \
                (tenant_id,subject,revision,envelope,updated_at_ms) \
             VALUES ($1,$2,$3,$4::jsonb,$5) \
             ON CONFLICT (tenant_id,subject) DO UPDATE SET \
                revision=EXCLUDED.revision,envelope=EXCLUDED.envelope,updated_at_ms=EXCLUDED.updated_at_ms",
        )
        .bind(&tenant)
        .bind(envelope.subject.as_str())
        .bind(revision_i64)
        .bind(&encoded)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    pub async fn append_state_delta(
        &self,
        ctx: &ExecutionContext,
        revision: StateRevision,
        envelope: MessageEnvelope,
    ) -> Result<WatchCursor, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        validate_lane(&envelope, MessageKind::StateDelta, ctx)?;
        let encoded = encode_envelope(&envelope)?;
        let revision_i64 = to_i64(revision.0, "state revision")?;
        let created = to_i64(envelope.created_at_ms, "state delta time")?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        advance_state_head(
            &mut tx,
            &tenant,
            envelope.subject.as_str(),
            revision_i64,
            created,
        )
        .await?;
        let cursor: i64 = sqlx::query_scalar(
            "INSERT INTO interconnect_state_deltas \
                (tenant_id,subject,revision,envelope,created_at_ms) \
             VALUES ($1,$2,$3,$4::jsonb,$5) RETURNING cursor",
        )
        .bind(&tenant)
        .bind(envelope.subject.as_str())
        .bind(revision_i64)
        .bind(&encoded)
        .bind(created)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(WatchCursor(from_i64(cursor, "watch cursor")?))
    }

    pub async fn watch_state(
        &self,
        ctx: &ExecutionContext,
        subject: Subject,
        after: WatchCursor,
        limit: usize,
    ) -> Result<StateWatchBatch, InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        if limit == 0 || limit > 1000 {
            return Err(resource_error("state watch limit must be 1..=1000"));
        }
        let after_i64 = to_i64(after.0, "watch cursor")?;
        let snapshot_row = sqlx::query(
            "SELECT revision,envelope::text AS envelope_text FROM interconnect_state_snapshots \
             WHERE tenant_id=$1 AND subject=$2",
        )
        .bind(&tenant)
        .bind(subject.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        let snapshot = match snapshot_row {
            Some(row) => Some(StateSnapshotRecord {
                tenant_id: tenant.clone(),
                subject: subject.clone(),
                revision: StateRevision(from_i64(
                    row.try_get("revision").map_err(db_error)?,
                    "state revision",
                )?),
                envelope: decode_envelope(row.try_get("envelope_text").map_err(db_error)?)?,
            }),
            None => None,
        };
        let latest_revision: Option<i64> = sqlx::query_scalar(
            "SELECT latest_revision FROM interconnect_state_heads WHERE tenant_id=$1 AND subject=$2",
        )
        .bind(&tenant)
        .bind(subject.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(db_error)?;
        let latest_revision = StateRevision(
            latest_revision
                .map(|value| from_i64(value, "state revision"))
                .transpose()?
                .unwrap_or(0),
        );
        let rows = sqlx::query(
            "SELECT cursor,revision,envelope::text AS envelope_text \
             FROM interconnect_state_deltas \
             WHERE tenant_id=$1 AND subject=$2 AND cursor>$3 \
             ORDER BY cursor ASC LIMIT $4",
        )
        .bind(&tenant)
        .bind(subject.as_str())
        .bind(after_i64)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        let mut deltas = Vec::with_capacity(rows.len());
        for row in rows {
            deltas.push(StateDeltaRecord {
                cursor: WatchCursor(from_i64(
                    row.try_get("cursor").map_err(db_error)?,
                    "watch cursor",
                )?),
                tenant_id: tenant.clone(),
                subject: subject.clone(),
                revision: StateRevision(from_i64(
                    row.try_get("revision").map_err(db_error)?,
                    "state revision",
                )?),
                envelope: decode_envelope(row.try_get("envelope_text").map_err(db_error)?)?,
            });
        }
        let cursor = deltas.last().map(|delta| delta.cursor).unwrap_or(after);
        let _ = latest_revision;
        Ok(StateWatchBatch {
            directive: ResumeDirective::Resume { cursor },
            snapshot,
            deltas,
        })
    }

    pub(crate) async fn enqueue_outbox_in_transaction(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        ctx: &ExecutionContext,
        envelope: MessageEnvelope,
    ) -> Result<(), InterconnectError> {
        let tenant = trusted_tenant(ctx)?;
        validate_lane(&envelope, MessageKind::Event, ctx)?;
        let encoded = encode_envelope(&envelope)?;
        let created = to_i64(envelope.created_at_ms, "outbox creation time")?;
        let inserted = sqlx::query(
            "INSERT INTO interconnect_outbox \
                (outbox_id,tenant_id,subject,envelope,status,created_at_ms) \
             VALUES ($1,$2,$3,$4::jsonb,'pending',$5) ON CONFLICT DO NOTHING",
        )
        .bind(envelope.id.as_str())
        .bind(&tenant)
        .bind(envelope.subject.as_str())
        .bind(&encoded)
        .bind(created)
        .execute(&mut **tx)
        .await
        .map_err(db_error)?
        .rows_affected();
        if inserted == 1 {
            return Ok(());
        }
        let row = sqlx::query(
            "SELECT tenant_id,envelope::text AS envelope_text \
             FROM interconnect_outbox WHERE outbox_id=$1",
        )
        .bind(envelope.id.as_str())
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_error)?
        .ok_or_else(|| db_invariant("outbox conflict without an existing row"))?;
        let existing_tenant: String = row.try_get("tenant_id").map_err(db_error)?;
        let existing = decode_envelope(row.try_get("envelope_text").map_err(db_error)?)?;
        if existing_tenant == tenant && existing == envelope {
            Ok(())
        } else {
            Err(identity_conflict("outbox message id"))
        }
    }

    pub async fn flush_outbox(
        &self,
        limit: usize,
        published_at_ms: u64,
    ) -> Result<usize, InterconnectError> {
        if limit == 0 || limit > 1000 {
            return Err(resource_error("outbox flush limit must be 1..=1000"));
        }
        let published = to_i64(published_at_ms, "outbox publish time")?;
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let rows = sqlx::query(
            "SELECT outbox_id,tenant_id,subject,envelope::text AS envelope_text \
             FROM interconnect_outbox WHERE status='pending' \
             ORDER BY created_at_ms,outbox_id FOR UPDATE SKIP LOCKED LIMIT $1",
        )
        .bind(limit as i64)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_error)?;
        let mut published_count = 0usize;
        for row in rows {
            let outbox_id: String = row.try_get("outbox_id").map_err(db_error)?;
            let tenant: String = row.try_get("tenant_id").map_err(db_error)?;
            let subject: String = row.try_get("subject").map_err(db_error)?;
            let envelope = decode_envelope(row.try_get("envelope_text").map_err(db_error)?)?;
            envelope.validate()?;
            if envelope.kind != MessageKind::Event
                || envelope.subject.as_str() != subject
                || envelope.id.as_str() != outbox_id
            {
                return Err(db_invariant(
                    "outbox row and embedded event envelope disagree",
                ));
            }
            append_event_tx(&mut tx, &tenant, &envelope).await?;
            let affected = sqlx::query(
                "UPDATE interconnect_outbox SET status='published',published_at_ms=$2 \
                 WHERE outbox_id=$1 AND status='pending'",
            )
            .bind(&outbox_id)
            .bind(published)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?
            .rows_affected();
            if affected != 1 {
                return Err(db_invariant("locked outbox row changed before publication"));
            }
            published_count += 1;
        }
        tx.commit().await.map_err(db_error)?;
        Ok(published_count)
    }
}

async fn enqueue_work_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    envelope: &MessageEnvelope,
    retry_budget: u32,
    available_at_ms: u64,
) -> Result<(MessageId, bool), InterconnectError> {
    let encoded = encode_envelope(envelope)?;
    let available = to_i64(available_at_ms, "work availability")?;
    let created = to_i64(envelope.created_at_ms, "work creation time")?;
    let inserted = sqlx::query_scalar::<_, String>(
        "INSERT INTO interconnect_work \
            (work_id,tenant_id,subject,envelope,status,claim_generation,attempt,retry_budget,available_at_ms,created_at_ms,idempotency_key) \
         VALUES ($1,$2,$3,$4::jsonb,'ready',0,0,$5,$6,$7,$8) \
         ON CONFLICT DO NOTHING RETURNING work_id",
    )
    .bind(envelope.id.as_str())
    .bind(tenant)
    .bind(envelope.subject.as_str())
    .bind(&encoded)
    .bind(retry_budget as i32)
    .bind(available)
    .bind(created)
    .bind(envelope.idempotency_key.as_ref().map(|key| key.as_str()))
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?;
    if let Some(work_id) = inserted {
        return Ok((MessageId::new(work_id)?, true));
    }
    let existing = sqlx::query(
        "SELECT work_id,tenant_id,retry_budget,available_at_ms,envelope::text AS envelope_text \
         FROM interconnect_work \
         WHERE work_id=$1 OR (tenant_id=$2 AND subject=$3 AND idempotency_key IS NOT DISTINCT FROM $4) \
         ORDER BY CASE WHEN work_id=$1 THEN 0 ELSE 1 END LIMIT 1",
    )
    .bind(envelope.id.as_str())
    .bind(tenant)
    .bind(envelope.subject.as_str())
    .bind(envelope.idempotency_key.as_ref().map(|key| key.as_str()))
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?
    .ok_or_else(|| db_invariant("work insert conflicted without a discoverable existing row"))?;
    let existing_tenant: String = existing.try_get("tenant_id").map_err(db_error)?;
    let existing_retry: i32 = existing.try_get("retry_budget").map_err(db_error)?;
    let existing_available: i64 = existing.try_get("available_at_ms").map_err(db_error)?;
    let existing_envelope = decode_envelope(existing.try_get("envelope_text").map_err(db_error)?)?;
    if existing_tenant != tenant
        || existing_envelope != *envelope
        || existing_retry != retry_budget as i32
        || existing_available != available
    {
        return Err(identity_conflict("work message/idempotency identity"));
    }
    Ok((
        MessageId::new(existing.try_get::<String, _>("work_id").map_err(db_error)?)?,
        false,
    ))
}

async fn append_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    envelope: &MessageEnvelope,
) -> Result<u64, InterconnectError> {
    envelope.validate()?;
    if envelope.kind != MessageKind::Event {
        return Err(InterconnectError::new(
            InterconnectErrorCode::ContractIncompatible,
            "durable event append requires MessageKind::Event",
        ));
    }
    let encoded = encode_envelope(envelope)?;
    let created = to_i64(envelope.created_at_ms, "event creation time")?;
    let deadline = envelope
        .deadline_ms
        .map(|value| to_i64(value, "event deadline"))
        .transpose()?;
    let inserted: Option<i64> = sqlx::query_scalar(
        "INSERT INTO interconnect_events \
            (message_id,tenant_id,subject,contract_ref,contract_version,schema_ref,correlation_id,causation_id,created_at_ms,deadline_ms,ordering_key,idempotency_key,envelope) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13::jsonb) \
         ON CONFLICT DO NOTHING RETURNING sequence",
    )
    .bind(envelope.id.as_str())
    .bind(tenant)
    .bind(envelope.subject.as_str())
    .bind(envelope.contract.contract.as_str())
    .bind(envelope.contract.version.as_str())
    .bind(envelope.contract.schema.as_str())
    .bind(envelope.correlation_id.as_str())
    .bind(envelope.causation_id.as_ref().map(|id| id.as_str()))
    .bind(created)
    .bind(deadline)
    .bind(envelope.ordering_key.as_ref().map(|key| key.as_str()))
    .bind(envelope.idempotency_key.as_ref().map(|key| key.as_str()))
    .bind(&encoded)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?;
    if let Some(sequence) = inserted {
        return from_i64(sequence, "event sequence");
    }
    let existing = sqlx::query(
        "SELECT sequence,tenant_id,envelope::text AS envelope_text FROM interconnect_events \
         WHERE message_id=$1 OR (tenant_id=$2 AND subject=$3 AND idempotency_key IS NOT DISTINCT FROM $4) \
         ORDER BY CASE WHEN message_id=$1 THEN 0 ELSE 1 END LIMIT 1",
    )
    .bind(envelope.id.as_str())
    .bind(tenant)
    .bind(envelope.subject.as_str())
    .bind(envelope.idempotency_key.as_ref().map(|key| key.as_str()))
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?
    .ok_or_else(|| db_invariant("event insert conflicted without a discoverable existing row"))?;
    let existing_tenant: String = existing.try_get("tenant_id").map_err(db_error)?;
    let existing_envelope = decode_envelope(existing.try_get("envelope_text").map_err(db_error)?)?;
    if existing_tenant != tenant || existing_envelope != *envelope {
        return Err(identity_conflict("event message/idempotency identity"));
    }
    from_i64(
        existing.try_get("sequence").map_err(db_error)?,
        "event sequence",
    )
}

async fn advance_state_head(
    tx: &mut Transaction<'_, Postgres>,
    tenant: &str,
    subject: &str,
    revision: i64,
    now_ms: i64,
) -> Result<(), InterconnectError> {
    let advanced: Option<i64> = sqlx::query_scalar(
        "INSERT INTO interconnect_state_heads \
            (tenant_id,subject,latest_revision,updated_at_ms) VALUES ($1,$2,$3,$4) \
         ON CONFLICT (tenant_id,subject) DO UPDATE SET \
            latest_revision=EXCLUDED.latest_revision,updated_at_ms=EXCLUDED.updated_at_ms \
         WHERE interconnect_state_heads.latest_revision < EXCLUDED.latest_revision \
         RETURNING latest_revision",
    )
    .bind(tenant)
    .bind(subject)
    .bind(revision)
    .bind(now_ms)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_error)?;
    if advanced == Some(revision) {
        Ok(())
    } else {
        Err(InterconnectError::new(
            InterconnectErrorCode::ContractIncompatible,
            "state revision must increase monotonically across snapshots and deltas",
        ))
    }
}

fn require_current_claim(rows_affected: u64) -> Result<(), InterconnectError> {
    if rows_affected == 1 {
        Ok(())
    } else {
        Err(InterconnectError::new(
            InterconnectErrorCode::StaleClaimGeneration,
            "stale or foreign work claim cannot mutate the current generation",
        ))
    }
}

fn encode_envelope(envelope: &MessageEnvelope) -> Result<String, InterconnectError> {
    serde_json::to_string(envelope).map_err(|_| {
        InterconnectError::new(
            InterconnectErrorCode::InvalidPayload,
            "message envelope cannot be serialized",
        )
    })
}

fn decode_envelope(encoded: String) -> Result<MessageEnvelope, InterconnectError> {
    serde_json::from_str::<MessageEnvelope>(&encoded).map_err(|error| {
        InterconnectError::new(
            InterconnectErrorCode::SchemaIncompatible,
            format!("persisted message envelope is incompatible: {error}"),
        )
    })
}

fn to_i64(value: u64, kind: &str) -> Result<i64, InterconnectError> {
    i64::try_from(value).map_err(|_| resource_error(format!("{kind} exceeds PostgreSQL BIGINT")))
}

fn from_i64(value: i64, kind: &str) -> Result<u64, InterconnectError> {
    u64::try_from(value).map_err(|_| db_invariant(format!("negative {kind} in durable store")))
}

fn identity_conflict(kind: &str) -> InterconnectError {
    InterconnectError::new(
        InterconnectErrorCode::ContractIncompatible,
        format!("{kind} was reused for a materially different or foreign-tenant message"),
    )
}

fn resource_error(message: impl Into<String>) -> InterconnectError {
    InterconnectError::new(InterconnectErrorCode::ResourceBudgetExceeded, message)
}

fn db_invariant(message: impl Into<String>) -> InterconnectError {
    InterconnectError::new(InterconnectErrorCode::DriverFailure, message)
}

fn db_error(error: impl std::fmt::Display) -> InterconnectError {
    InterconnectError::new(
        InterconnectErrorCode::DriverFailure,
        format!("PostgreSQL durable driver failure: {error}"),
    )
    .retryable(true)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use sqlx::postgres::{PgConnection, PgPoolOptions};
    use sqlx::{Connection, Executor};
    use system_core::transport::interconnect::{
        ContractBinding, ContractRef, CorrelationId, Extensions, IdempotencyKey, MessageKind,
        OrderingKey, PayloadRef, SchemaRef,
    };
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode,
        NoopHttpClient, RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
    };

    use super::*;

    struct LiveFixture {
        database_url: String,
        schema: String,
        pool: PgPool,
    }

    impl LiveFixture {
        async fn create() -> anyhow::Result<Self> {
            let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
            let schema = format!("r4_interconnect_{}", uuid::Uuid::new_v4().simple());
            let mut admin = PgConnection::connect(&database_url).await?;
            admin
                .execute(format!("CREATE SCHEMA {schema}").as_str())
                .await?;
            drop(admin);
            let schema_for_pool = schema.clone();
            let pool = PgPoolOptions::new()
                .max_connections(8)
                .after_connect(move |connection, _| {
                    let schema = schema_for_pool.clone();
                    Box::pin(async move {
                        connection
                            .execute(format!("SET search_path TO {schema}").as_str())
                            .await?;
                        Ok(())
                    })
                })
                .connect(&database_url)
                .await?;
            sqlx::query(
                "CREATE TABLE tenants (id TEXT PRIMARY KEY,name TEXT NOT NULL,slug TEXT UNIQUE NOT NULL,status TEXT NOT NULL,plan TEXT NOT NULL,settings TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL)",
            )
            .execute(&pool)
            .await?;
            sqlx::query(
                "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) VALUES ('tenant-a','A','a','active','test','now','now'),('tenant-b','B','b','active','test','now','now')",
            )
            .execute(&pool)
            .await?;
            sqlx::raw_sql(MIGRATION_SQL).execute(&pool).await?;
            Ok(Self {
                database_url,
                schema,
                pool,
            })
        }

        async fn cleanup(self) -> anyhow::Result<()> {
            self.pool.close().await;
            let mut admin = PgConnection::connect(&self.database_url).await?;
            admin
                .execute(format!("DROP SCHEMA {} CASCADE", self.schema).as_str())
                .await?;
            Ok(())
        }
    }

    fn ctx(tenant_text: &str, correlation: &str) -> ExecutionContext {
        let tenant = TenantId::new(tenant_text).unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                format!("staff-{tenant_text}"),
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new(format!("membership-{tenant_text}"))
                        .unwrap(),
                    tenant_id: tenant.clone(),
                    role: TenantRole::Staff,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(correlation).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn envelope(kind: MessageKind, subject: &str, id: &str, correlation: &str) -> MessageEnvelope {
        MessageEnvelope {
            id: MessageId::new(id).unwrap(),
            kind,
            subject: Subject::new(subject).unwrap(),
            contract: ContractBinding {
                contract: ContractRef::new(subject).unwrap(),
                version: ContractVersion::new("1.0.0").unwrap(),
                schema: SchemaRef::new(format!("{subject}.v1")).unwrap(),
            },
            correlation_id: CorrelationId::new(correlation).unwrap(),
            causation_id: None,
            created_at_ms: 100,
            deadline_ms: Some(10_000),
            ordering_key: Some(OrderingKey::new("aggregate-a").unwrap()),
            idempotency_key: Some(IdempotencyKey::new(format!("idem-{id}")).unwrap()),
            payload: PayloadRef::Inline(json!({"id": id})),
            extensions: Extensions::empty(),
        }
    }

    #[tokio::test]
    #[ignore = "requires TALOS_TEST_POSTGRES_URL"]
    async fn live_pg18_event_replay_dedup_and_tenant_isolation_survive_driver_restart()
    -> anyhow::Result<()> {
        let fixture = LiveFixture::create().await?;
        let driver_a = PostgresDurableDriver::new(fixture.pool.clone());
        let tenant_a = ctx("tenant-a", "corr-a");
        let tenant_b = ctx("tenant-b", "corr-a");
        let event = envelope(MessageKind::Event, "order.closed", "event-1", "corr-a");
        let sequence = driver_a.append_event(&tenant_a, event.clone()).await?;
        assert_eq!(
            driver_a.append_event(&tenant_a, event.clone()).await?,
            sequence
        );
        assert_eq!(
            driver_a
                .append_event(&tenant_b, event.clone())
                .await
                .unwrap_err()
                .code,
            InterconnectErrorCode::ContractIncompatible
        );
        let mut changed = event.clone();
        changed.payload = PayloadRef::Inline(json!({"id": "changed"}));
        assert_eq!(
            driver_a
                .append_event(&tenant_a, changed)
                .await
                .unwrap_err()
                .code,
            InterconnectErrorCode::ContractIncompatible
        );
        let driver_b = PostgresDurableDriver::new(fixture.pool.clone());
        let replay = driver_b
            .replay_events(
                &tenant_a,
                ConsumerId::new("projection-a")?,
                Subject::new("order.closed")?,
                0,
                10,
                200,
            )
            .await?;
        assert_eq!(replay.len(), 1);
        let foreign = driver_b
            .replay_events(
                &tenant_b,
                ConsumerId::new("projection-b")?,
                Subject::new("order.closed")?,
                0,
                10,
                200,
            )
            .await?;
        assert!(foreign.is_empty());
        fixture.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires TALOS_TEST_POSTGRES_URL"]
    async fn live_pg18_work_restart_reclaim_fences_stale_generation_and_identity_reuse()
    -> anyhow::Result<()> {
        let fixture = LiveFixture::create().await?;
        let trusted = ctx("tenant-a", "corr-a");
        let foreign = ctx("tenant-b", "corr-a");
        let subject = Subject::new("workflow.step")?;
        let driver_a = PostgresDurableDriver::new(fixture.pool.clone());
        let work = envelope(MessageKind::Work, "workflow.step", "work-1", "corr-a");
        driver_a
            .enqueue_work(&trusted, work.clone(), 2, 100)
            .await?;
        assert_eq!(
            driver_a
                .enqueue_work(&trusted, work.clone(), 3, 100)
                .await
                .unwrap_err()
                .code,
            InterconnectErrorCode::ContractIncompatible
        );
        assert_eq!(
            driver_a
                .enqueue_work(&trusted, work.clone(), 2, 101)
                .await
                .unwrap_err()
                .code,
            InterconnectErrorCode::ContractIncompatible
        );
        assert_eq!(
            driver_a
                .enqueue_work(&foreign, work, 2, 100)
                .await
                .unwrap_err()
                .code,
            InterconnectErrorCode::ContractIncompatible
        );
        let claim_a = driver_a
            .claim_work(&trusted, &subject, LeaseOwner::new("worker-a")?, 100, 10)
            .await?
            .expect("first claim");
        let driver_b = PostgresDurableDriver::new(fixture.pool.clone());
        let claim_b = driver_b
            .claim_work(&trusted, &subject, LeaseOwner::new("worker-b")?, 111, 10)
            .await?
            .expect("reclaimed work");
        assert!(claim_b.claim_generation > claim_a.claim_generation);
        assert_eq!(
            driver_a
                .ack_work(&trusted, &claim_a)
                .await
                .unwrap_err()
                .code,
            InterconnectErrorCode::StaleClaimGeneration
        );
        driver_b.ack_work(&trusted, &claim_b).await?;
        fixture.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires TALOS_TEST_POSTGRES_URL"]
    async fn live_pg18_state_head_rejects_revision_regression() -> anyhow::Result<()> {
        let fixture = LiveFixture::create().await?;
        let trusted = ctx("tenant-a", "corr-a");
        let driver = PostgresDurableDriver::new(fixture.pool.clone());
        driver
            .put_state_snapshot(
                &trusted,
                StateRevision(1),
                envelope(
                    MessageKind::StateSnapshot,
                    "order.state",
                    "snap-1",
                    "corr-a",
                ),
                100,
            )
            .await?;
        driver
            .append_state_delta(
                &trusted,
                StateRevision(5),
                envelope(MessageKind::StateDelta, "order.state", "delta-5", "corr-a"),
            )
            .await?;
        assert_eq!(
            driver
                .append_state_delta(
                    &trusted,
                    StateRevision(3),
                    envelope(MessageKind::StateDelta, "order.state", "delta-3", "corr-a"),
                )
                .await
                .unwrap_err()
                .code,
            InterconnectErrorCode::ContractIncompatible
        );
        fixture.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires TALOS_TEST_POSTGRES_URL"]
    async fn live_pg18_business_transaction_outbox_is_atomic_and_flush_is_durable()
    -> anyhow::Result<()> {
        let fixture = LiveFixture::create().await?;
        let trusted = ctx("tenant-a", "corr-a");
        let driver = PostgresDurableDriver::new(fixture.pool.clone());
        sqlx::query("CREATE TABLE business_probe (id TEXT PRIMARY KEY)")
            .execute(&fixture.pool)
            .await?;
        let mut rolled_back = fixture.pool.begin().await?;
        sqlx::query("INSERT INTO business_probe (id) VALUES ('rollback')")
            .execute(&mut *rolled_back)
            .await?;
        driver
            .enqueue_outbox_in_transaction(
                &mut rolled_back,
                &trusted,
                envelope(
                    MessageKind::Event,
                    "order.committed",
                    "event-rollback",
                    "corr-a",
                ),
            )
            .await?;
        rolled_back.rollback().await?;
        let rollback_business: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM business_probe WHERE id='rollback'")
                .fetch_one(&fixture.pool)
                .await?;
        let rollback_outbox: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM interconnect_outbox WHERE outbox_id='event-rollback'",
        )
        .fetch_one(&fixture.pool)
        .await?;
        assert_eq!((rollback_business, rollback_outbox), (0, 0));
        let mut committed = fixture.pool.begin().await?;
        sqlx::query("INSERT INTO business_probe (id) VALUES ('commit')")
            .execute(&mut *committed)
            .await?;
        driver
            .enqueue_outbox_in_transaction(
                &mut committed,
                &trusted,
                envelope(
                    MessageKind::Event,
                    "order.committed",
                    "event-commit",
                    "corr-a",
                ),
            )
            .await?;
        committed.commit().await?;
        assert_eq!(driver.flush_outbox(10, 300).await?, 1);
        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM interconnect_events WHERE message_id='event-commit'",
        )
        .fetch_one(&fixture.pool)
        .await?;
        let published_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM interconnect_outbox WHERE outbox_id='event-commit' AND status='published'")
            .fetch_one(&fixture.pool)
            .await?;
        assert_eq!((event_count, published_count), (1, 1));
        fixture.cleanup().await?;
        Ok(())
    }
}

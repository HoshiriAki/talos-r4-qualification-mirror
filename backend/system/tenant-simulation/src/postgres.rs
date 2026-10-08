use std::future::Future;

use chrono::{DateTime, Utc};
use chrono_tz::Asia::Shanghai;
use sqlx::{Acquire, Executor, PgConnection, PgPool, Row};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::*;

pub(super) fn create(
    pool: &PgPool,
    input: CreateInput,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let actor = FeatureTenantSimulation::require_platform(ctx)?.to_owned();
    let tenant = required(&input.tenant_id, "tenantId")?.to_string();
    let scenario = plain(&input.scenario_name, "scenarioName", 80)?;
    let intent = plain(&input.change_intent, "changeIntent", 500)?;
    let ttl = input.ttl_minutes.unwrap_or(30);
    if !(1..=MAX_TTL_MINUTES).contains(&ttl) {
        return Err(err("VAL_TTL", "ttlMinutes must be between 1 and 120"));
    }
    let idem = valid_idempotency(&input.idempotency_key)?.to_owned();
    let keys = normalize_keys(input.planned_absent_keys.unwrap_or_default())?;
    let digest = digest_json(
        &json!({"tenantId":tenant,"scenarioName":scenario,"changeIntent":intent,"ttlMinutes":ttl,"plannedAbsentKeys":keys}),
    );
    let key_hash = digest_str(&idem);
    let now = shanghai_now();
    let expires = now + Duration::minutes(ttl);
    let id = uuid::Uuid::new_v4().to_string();
    let session = Session {
        id: id.clone(),
        actor_id: actor.clone(),
        tenant_id: tenant.clone(),
        namespace_id: id,
        base_revision: 0,
        generation: 1,
        scenario_name: scenario,
        change_intent: intent,
        status: "provisioning".into(),
        created_at: now.to_rfc3339(),
        expires_at: expires.to_rfc3339(),
        discarded_at: None,
        failure_code: None,
        create_request_digest: digest.clone(),
    };
    let pool = pool.clone();

    run_pg(async move {
        let mut tx = begin_serializable(&pool).await?;

        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1 AND status='active')",
        )
        .bind(&tenant)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if !active {
            return Err(err("SIMULATION_NOT_FOUND", "active tenant not found"));
        }

        if let Some(existing) = load_by_create_key(&mut tx, &actor, &key_hash).await? {
            if existing.create_request_digest != digest {
                return Err(err(
                    "IDEMPOTENCY_CONFLICT",
                    "idempotency key was used for another request",
                ));
            }
            tx.commit().await.map_err(db)?;
            return serde_json::to_value(existing).map_err(serialization);
        }

        admit_rate(&mut tx, &actor, "__create__", "create", 600, 5).await?;

        let inserted: Option<String> = sqlx::query_scalar(
            "INSERT INTO simulation_sessions
             (id,actor_id,target_tenant_id,namespace_id,base_revision,generation,
              idempotency_key_hash,create_request_digest,scenario_name,change_intent,
              status,created_at,expires_at,provisioning_lease_until)
             VALUES ($1,$2,$3,$4,0,1,$5,$6,$7,$8,'provisioning',
                     $9::timestamptz,$10::timestamptz,$11::timestamptz)
             ON CONFLICT (actor_id,idempotency_key_hash) DO NOTHING
             RETURNING id",
        )
        .bind(&session.id)
        .bind(&session.actor_id)
        .bind(&tenant)
        .bind(&session.namespace_id)
        .bind(&key_hash)
        .bind(&digest)
        .bind(&session.scenario_name)
        .bind(&session.change_intent)
        .bind(&session.created_at)
        .bind(&session.expires_at)
        .bind((now + Duration::minutes(5)).to_rfc3339())
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;

        if inserted.is_none() {
            let existing = load_by_create_key(&mut tx, &actor, &key_hash)
                .await?
                .ok_or_else(|| err("SYS_DB", "simulation idempotency conflict lost owner row"))?;
            if existing.create_request_digest != digest {
                return Err(err(
                    "IDEMPOTENCY_CONFLICT",
                    "idempotency key was used for another request",
                ));
            }
            tx.commit().await.map_err(db)?;
            return serde_json::to_value(existing).map_err(serialization);
        }

        tx.commit().await.map_err(db)?;

        if let Err(failure) = provision(&pool, &session, &keys).await {
            let changed = sqlx::query(
                "UPDATE simulation_sessions
                 SET status='failed',failure_code=$2
                 WHERE id=$1 AND status='provisioning'",
            )
            .bind(&session.id)
            .bind(code_of(&failure))
            .execute(&pool)
            .await
            .map_err(|error| {
                err(
                    "SIMULATION_FAILURE_PERSIST_FAILED",
                    &format!("provisioning failed and failed-state persistence failed: {error}"),
                )
            })?
            .rows_affected();
            if changed != 1 {
                return Err(err(
                    "SIMULATION_FAILURE_PERSIST_FAILED",
                    "provisioning failed and failed-state persistence changed no row",
                ));
            }
            return Err(failure);
        }

        let mut connection = pool.acquire().await.map_err(db)?;
        let active = load_session(&mut connection, &session.id, &actor).await?;
        serde_json::to_value(active).map_err(serialization)
    })
}

pub(super) fn get(pool: &PgPool, input: IdInput, ctx: &ExecutionContext) -> Result<Value, String> {
    let actor = FeatureTenantSimulation::require_platform(ctx)?.to_owned();
    let id = required(&input.id, "id")?.to_owned();
    let pool = pool.clone();
    run_pg(async move {
        reconcile_lifecycle(&pool, &id, &actor).await?;
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = load_session(&mut connection, &id, &actor).await?;
        serde_json::to_value(session).map_err(serialization)
    })
}

pub(super) fn discard(
    pool: &PgPool,
    input: IdInput,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let actor = FeatureTenantSimulation::require_platform(ctx)?.to_owned();
    let id = required(&input.id, "id")?.to_owned();
    let pool = pool.clone();

    run_pg(async move {
        let mut tx = begin_serializable(&pool).await?;
        let row = sqlx::query(
            "SELECT target_tenant_id,generation
             FROM simulation_sessions
             WHERE id=$1 AND actor_id=$2 AND status IN ('active','expired')
             FOR UPDATE",
        )
        .bind(&id)
        .bind(&actor)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;

        let now = shanghai_now().to_rfc3339();
        if let Some(row) = row {
            let tenant = row.try_get::<String, _>("target_tenant_id").map_err(db)?;
            let generation = row.try_get::<i64, _>("generation").map_err(db)?;
            materialize_terminal_inputs(&mut tx, &id, &tenant, generation + 1, &now).await?;
        }

        let changed = sqlx::query(
            "UPDATE simulation_sessions
             SET status='discarded',generation=generation+1,
                 discarded_at=COALESCE(discarded_at,$3::timestamptz)
             WHERE id=$1 AND actor_id=$2 AND status IN ('active','expired')",
        )
        .bind(&id)
        .bind(&actor)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(db)?
        .rows_affected();

        if changed == 0 {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM simulation_sessions WHERE id=$1 AND actor_id=$2
                 )",
            )
            .bind(&id)
            .bind(&actor)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
            if !exists {
                return Err(err("SIMULATION_NOT_FOUND", "simulation not found"));
            }
        }

        sqlx::query(
            "INSERT INTO simulation_cleanup_jobs
             (simulation_id,tenant_id,job_kind,state,attempts,next_attempt_at,created_at,updated_at)
             SELECT id,target_tenant_id,'terminal_evidence','pending',0,
                    $2::timestamptz,$2::timestamptz,$2::timestamptz
             FROM simulation_sessions WHERE id=$1
             ON CONFLICT (simulation_id,tenant_id,job_kind) DO NOTHING",
        )
        .bind(&id)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(db)?;

        tx.commit().await.map_err(db)?;
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = load_session(&mut connection, &id, &actor).await?;
        serde_json::to_value(session).map_err(serialization)
    })
}

async fn simulation_capability(
    connection: &mut PgConnection,
    id: &str,
    ctx: &ExecutionContext,
) -> Result<Session, String> {
    let actor = ctx
        .actor()
        .id()
        .ok_or_else(|| err("SIMULATION_NOT_FOUND", "simulation not found"))?;
    let session = load_session(connection, id, actor).await?;
    let expected = SimulationId::new(id.to_owned())
        .map_err(|_| err("SIMULATION_NOT_FOUND", "simulation not found"))?;
    if !ctx.actor().has_platform_authority()
        || ctx.data_scope().tenant_id().as_str() != session.tenant_id
        || !matches!(ctx.data_scope().namespace(), Namespace::Simulation(actual) if actual == &expected)
        || ctx.execution_mode() != &ExecutionMode::Simulation(expected)
        || session.status != "active"
        || parse_time(&session.expires_at)? <= Utc::now().with_timezone(&Shanghai)
    {
        return Err(err(
            "SESSION_NOT_EXECUTABLE",
            "simulation session is not executable",
        ));
    }

    let tenant_active: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1 AND status='active')")
            .bind(&session.tenant_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(db)?;
    if !tenant_active {
        return Err(err(
            "SESSION_NOT_EXECUTABLE",
            "simulation tenant is not active",
        ));
    }
    Ok(session)
}

async fn evidence_capability(
    connection: &mut PgConnection,
    id: &str,
    ctx: &ExecutionContext,
) -> Result<Session, String> {
    let actor = ctx
        .actor()
        .id()
        .ok_or_else(|| err("SIMULATION_NOT_FOUND", "simulation not found"))?;
    if !ctx.actor().has_platform_authority() {
        return Err(err("SIMULATION_NOT_FOUND", "simulation not found"));
    }
    let session = load_session(connection, id, actor).await?;
    if !matches!(session.status.as_str(), "active" | "expired" | "discarded") {
        return Err(err("SIMULATION_NOT_FOUND", "simulation not found"));
    }
    Ok(session)
}

async fn provision(pool: &PgPool, session: &Session, keys: &[String]) -> Result<(), String> {
    let mut tx = begin_serializable(pool).await?;
    let now = shanghai_now().to_rfc3339();

    sync_pricing_revision(&mut tx, &session.tenant_id).await?;

    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM simulation_tenant_revisions WHERE tenant_id=$1")
            .bind(&session.tenant_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db)?
            .unwrap_or(0);

    let source = sqlx::query(
        "SELECT COUNT(*)::bigint AS rows,
                COALESCE(SUM(length(COALESCE(c.value_json::text,''))),0)::bigint AS bytes
         FROM simulation_reference_resource_revisions r
         LEFT JOIN simulation_reference_configs c
           ON c.tenant_id=r.tenant_id AND c.key=r.key
         WHERE r.tenant_id=$1",
    )
    .bind(&session.tenant_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    let source_rows = source.try_get::<i64, _>("rows").map_err(db)?;
    let source_bytes = source.try_get::<i64, _>("bytes").map_err(db)?;

    let pricing_bytes: i64 = sqlx::query_scalar(
        "SELECT length(document_json::text)::bigint
         FROM simulation_pricing_resource_revisions
         WHERE tenant_id=$1 AND resource_key=$2",
    )
    .bind(&session.tenant_id)
    .bind(PRICING_KEY)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;

    let mut missing_planned = 0_i64;
    for key in keys {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM simulation_reference_resource_revisions
                WHERE tenant_id=$1 AND key=$2
             )",
        )
        .bind(&session.tenant_id)
        .bind(key)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if !exists {
            missing_planned += 1;
        }
    }

    if source_rows + missing_planned + 1 > MAX_PROOF_ROWS
        || source_bytes + pricing_bytes > MAX_PROOF_BYTES
    {
        return Err(err(
            "SIMULATION_QUOTA_EXCEEDED",
            "proof source exceeds provisioning quota",
        ));
    }

    let rows = sqlx::query(
        "SELECT r.key,r.present,r.resource_version,c.value_json::text AS value_json
         FROM simulation_reference_resource_revisions r
         LEFT JOIN simulation_reference_configs c
           ON c.tenant_id=r.tenant_id AND c.key=r.key
         WHERE r.tenant_id=$1
         ORDER BY r.key",
    )
    .bind(&session.tenant_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(db)?;

    for row in rows {
        let key = row.try_get::<String, _>("key").map_err(db)?;
        let present = row.try_get::<bool, _>("present").map_err(db)?;
        let version = row.try_get::<i64, _>("resource_version").map_err(db)?;
        let doc = row.try_get::<Option<String>, _>("value_json").map_err(db)?;
        insert_manifest(
            &mut tx,
            session,
            &key,
            present,
            version,
            doc.as_deref(),
            &now,
            KIND,
        )
        .await?;
    }

    let pricing = sqlx::query(
        "SELECT present,resource_version,document_json::text AS document_json
         FROM simulation_pricing_resource_revisions
         WHERE tenant_id=$1 AND resource_key=$2",
    )
    .bind(&session.tenant_id)
    .bind(PRICING_KEY)
    .fetch_one(&mut *tx)
    .await
    .map_err(db)?;
    let pricing_present = pricing.try_get::<i16, _>("present").map_err(db)? != 0;
    let pricing_version = pricing.try_get::<i64, _>("resource_version").map_err(db)?;
    let pricing_doc = pricing.try_get::<String, _>("document_json").map_err(db)?;
    insert_manifest(
        &mut tx,
        session,
        PRICING_KEY,
        pricing_present,
        pricing_version,
        Some(&pricing_doc),
        &now,
        PRICING_KIND,
    )
    .await?;

    for key in keys {
        let existing = sqlx::query(
            "SELECT present,resource_version
             FROM simulation_reference_resource_revisions
             WHERE tenant_id=$1 AND key=$2",
        )
        .bind(&session.tenant_id)
        .bind(key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;

        if existing
            .as_ref()
            .is_some_and(|row| row.try_get::<bool, _>("present").unwrap_or(false))
        {
            return Err(err("VAL_PLANNED_KEY", "planned absent key already exists"));
        }
        if existing.is_none() {
            insert_manifest(&mut tx, session, key, false, 0, None, &now, KIND).await?;
        }
    }

    sqlx::query("INSERT INTO simulation_usage (simulation_id,tenant_id) VALUES ($1,$2)")
        .bind(&session.id)
        .bind(&session.tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(db)?;

    sqlx::query(
        "UPDATE simulation_sessions
         SET base_revision=$2,status='active',provisioning_lease_until=NULL
         WHERE id=$1 AND status='provisioning'",
    )
    .bind(&session.id)
    .bind(revision)
    .execute(&mut *tx)
    .await
    .map_err(db)?;

    tx.commit().await.map_err(db)?;
    Ok(())
}

async fn reconcile_lifecycle(pool: &PgPool, id: &str, actor: &str) -> Result<(), String> {
    let mut tx = begin_serializable(pool).await?;
    let row = sqlx::query(
        "SELECT status,target_tenant_id,generation,provisioning_lease_until
         FROM simulation_sessions
         WHERE id=$1 AND actor_id=$2
         FOR UPDATE",
    )
    .bind(id)
    .bind(actor)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db)?;

    let Some(row) = row else {
        return Err(err("SIMULATION_NOT_FOUND", "simulation not found"));
    };
    let status = row.try_get::<String, _>("status").map_err(db)?;
    let tenant = row.try_get::<String, _>("target_tenant_id").map_err(db)?;
    let generation = row.try_get::<i64, _>("generation").map_err(db)?;
    let lease = row
        .try_get::<Option<DateTime<Utc>>, _>("provisioning_lease_until")
        .map_err(db)?;
    let now = shanghai_now();
    let now_text = now.to_rfc3339();

    if status == "active" {
        let expired: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM simulation_sessions
                WHERE id=$1 AND actor_id=$2 AND status='active'
                  AND expires_at <= $3::timestamptz
             )",
        )
        .bind(id)
        .bind(actor)
        .bind(&now_text)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if expired {
            materialize_terminal_inputs(&mut tx, id, &tenant, generation + 1, &now_text).await?;
            sqlx::query(
                "UPDATE simulation_sessions
                 SET status='expired',generation=generation+1
                 WHERE id=$1 AND actor_id=$2 AND status='active'
                   AND expires_at <= $3::timestamptz",
            )
            .bind(id)
            .bind(actor)
            .bind(&now_text)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
            insert_cleanup_job(&mut tx, id, &tenant, "terminal_evidence", &now_text).await?;
        }
    } else if status == "provisioning"
        && lease.is_some_and(|lease| lease.with_timezone(&Shanghai) <= now)
    {
        sqlx::query(
            "UPDATE simulation_sessions
             SET status='failed',failure_code='SIMULATION_PROVISION_LEASE_EXPIRED'
             WHERE id=$1 AND actor_id=$2 AND status='provisioning'
               AND provisioning_lease_until <= $3::timestamptz",
        )
        .bind(id)
        .bind(actor)
        .bind(&now_text)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
        insert_cleanup_job(&mut tx, id, &tenant, "orphan_cleanup", &now_text).await?;
    }

    tx.commit().await.map_err(db)
}

async fn begin_serializable(
    pool: &PgPool,
) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, String> {
    let mut tx = pool.begin().await.map_err(db)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    Ok(tx)
}

fn run_pg<T, F>(future: F) -> Result<T, String>
where
    T: Send,
    F: Future<Output = Result<T, String>> + Send,
{
    let handle = Handle::try_current().map_err(|_| {
        err(
            "SYS_DB",
            "PostgreSQL simulation adapter requires a Tokio runtime",
        )
    })?;
    if !matches!(handle.runtime_flavor(), RuntimeFlavor::MultiThread) {
        return Err(err(
            "SYS_DB",
            "PostgreSQL simulation adapter requires the multi-thread Tokio runtime",
        ));
    }
    tokio::task::block_in_place(|| handle.block_on(future))
}

fn timestamp_text(value: DateTime<Utc>) -> String {
    value.with_timezone(&Shanghai).to_rfc3339()
}

async fn load_session(
    connection: &mut PgConnection,
    id: &str,
    actor: &str,
) -> Result<Session, String> {
    let row = sqlx::query(
        "SELECT id,actor_id,target_tenant_id,namespace_id,base_revision,generation,
                scenario_name,change_intent,status,created_at,expires_at,discarded_at,
                failure_code,create_request_digest
         FROM simulation_sessions
         WHERE id=$1 AND actor_id=$2",
    )
    .bind(id)
    .bind(actor)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?
    .ok_or_else(|| err("SIMULATION_NOT_FOUND", "simulation not found"))?;

    Ok(Session {
        id: row.try_get("id").map_err(db)?,
        actor_id: row.try_get("actor_id").map_err(db)?,
        tenant_id: row.try_get("target_tenant_id").map_err(db)?,
        namespace_id: row.try_get("namespace_id").map_err(db)?,
        base_revision: row.try_get("base_revision").map_err(db)?,
        generation: row.try_get("generation").map_err(db)?,
        scenario_name: row.try_get("scenario_name").map_err(db)?,
        change_intent: row.try_get("change_intent").map_err(db)?,
        status: row.try_get("status").map_err(db)?,
        created_at: timestamp_text(row.try_get("created_at").map_err(db)?),
        expires_at: timestamp_text(row.try_get("expires_at").map_err(db)?),
        discarded_at: row
            .try_get::<Option<DateTime<Utc>>, _>("discarded_at")
            .map_err(db)?
            .map(timestamp_text),
        failure_code: row.try_get("failure_code").map_err(db)?,
        create_request_digest: row.try_get("create_request_digest").map_err(db)?,
    })
}

async fn load_by_create_key(
    connection: &mut PgConnection,
    actor: &str,
    key: &str,
) -> Result<Option<Session>, String> {
    let id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM simulation_sessions
         WHERE actor_id=$1 AND idempotency_key_hash=$2",
    )
    .bind(actor)
    .bind(key)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?;

    match id {
        Some(id) => load_session(connection, &id, actor).await.map(Some),
        None => Ok(None),
    }
}

async fn insert_manifest(
    connection: &mut PgConnection,
    session: &Session,
    key: &str,
    present: bool,
    version: i64,
    doc: Option<&str>,
    now: &str,
    kind: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO simulation_revision_evidence
         (simulation_id,tenant_id,resource_kind,resource_key,base_presence,captured_resource_version)
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(&session.id)
    .bind(&session.tenant_id)
    .bind(kind)
    .bind(key)
    .bind(present)
    .bind(version)
    .execute(&mut *connection)
    .await
    .map_err(db)?;

    if let Some(doc) = doc {
        sqlx::query(
            "INSERT INTO simulation_base_documents
             (simulation_id,tenant_id,resource_kind,resource_key,base_resource_version,
              document_json,payload_policy,captured_at)
             VALUES ($1,$2,$3,$4,$5,$6::jsonb,'REFERENCE_ONLY',$7::timestamptz)",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(kind)
        .bind(key)
        .bind(version)
        .bind(doc)
        .bind(now)
        .execute(&mut *connection)
        .await
        .map_err(db)?;
    }
    Ok(())
}

async fn insert_cleanup_job(
    connection: &mut PgConnection,
    simulation_id: &str,
    tenant_id: &str,
    job_kind: &str,
    now: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO simulation_cleanup_jobs
         (simulation_id,tenant_id,job_kind,state,attempts,next_attempt_at,created_at,updated_at)
         VALUES ($1,$2,$3,'pending',0,$4::timestamptz,$4::timestamptz,$4::timestamptz)
         ON CONFLICT (simulation_id,tenant_id,job_kind) DO NOTHING",
    )
    .bind(simulation_id)
    .bind(tenant_id)
    .bind(job_kind)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(db)?;
    Ok(())
}

async fn admit_rate(
    connection: &mut PgConnection,
    actor: &str,
    simulation_id: &str,
    operation: &str,
    seconds: i64,
    limit: i64,
) -> Result<(), String> {
    let now = shanghai_now();
    let start = now - Duration::seconds(now.timestamp().rem_euclid(seconds));
    let bucket = start.with_nanosecond(0).unwrap().to_rfc3339();
    let count: Option<i32> = sqlx::query_scalar(
        "INSERT INTO simulation_rate_buckets
         (actor_id,simulation_id,operation_class,window_start,request_count)
         VALUES ($1,$2,$3,$4::timestamptz,1)
         ON CONFLICT (actor_id,simulation_id,operation_class,window_start)
         DO UPDATE SET request_count=simulation_rate_buckets.request_count+1
         WHERE simulation_rate_buckets.request_count < $5
         RETURNING request_count",
    )
    .bind(actor)
    .bind(simulation_id)
    .bind(operation)
    .bind(&bucket)
    .bind(limit as i32)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?;
    if count.is_none() {
        return Err(err(
            "SIMULATION_RATE_LIMITED",
            "simulation rate limit exceeded",
        ));
    }
    Ok(())
}

async fn ensure_active_generation(
    connection: &mut PgConnection,
    session: &Session,
) -> Result<(), String> {
    let now = shanghai_now().to_rfc3339();
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM simulation_sessions
            WHERE id=$1 AND target_tenant_id=$2 AND status='active'
              AND generation=$3 AND expires_at > $4::timestamptz
         )",
    )
    .bind(&session.id)
    .bind(&session.tenant_id)
    .bind(session.generation)
    .bind(now)
    .fetch_one(&mut *connection)
    .await
    .map_err(db)?;
    if ok {
        Ok(())
    } else {
        Err(err(
            "SESSION_NOT_EXECUTABLE",
            "simulation session is not executable",
        ))
    }
}

pub(super) fn list(
    pool: &PgPool,
    input: PageInput,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let pool = pool.clone();
    let actor_ctx = ctx;
    run_pg(async move {
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = simulation_capability(&mut connection, &input.id, actor_ctx).await?;
        let limit = page_limit(input.limit)? as i64;
        let cursor = input.cursor.unwrap_or_default();
        let rows = sqlx::query(
            "SELECT resource_key,operation,document_json
             FROM (
               SELECT resource_key,operation,document_json::text AS document_json
               FROM simulation_overlay_entries
               WHERE simulation_id=$1 AND tenant_id=$2
                 AND resource_kind='reference_config' AND resource_key>$3
               UNION ALL
               SELECT b.resource_key,'base',b.document_json::text
               FROM simulation_base_documents b
               WHERE b.simulation_id=$1 AND b.tenant_id=$2
                 AND b.resource_kind='reference_config' AND b.resource_key>$3
                 AND NOT EXISTS(
                   SELECT 1 FROM simulation_overlay_entries o
                   WHERE o.simulation_id=b.simulation_id
                     AND o.tenant_id=b.tenant_id
                     AND o.resource_kind=b.resource_kind
                     AND o.resource_key=b.resource_key
                 )
             ) visible
             ORDER BY resource_key
             LIMIT $4",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(cursor)
        .bind(limit)
        .fetch_all(&mut *connection)
        .await
        .map_err(db)?;

        let mut items = Vec::new();
        let mut next = None;
        for row in rows {
            let key = row.try_get::<String, _>("resource_key").map_err(db)?;
            let operation = row.try_get::<String, _>("operation").map_err(db)?;
            let document = row
                .try_get::<Option<String>, _>("document_json")
                .map_err(db)?;
            next = Some(key.clone());
            if operation != "tombstone" {
                let document = document
                    .ok_or_else(|| err("SYS_DB", "visible simulation document is missing"))?;
                items.push(json!({
                    "key": key,
                    "value": serde_json::from_str::<Value>(&document).map_err(serialization)?
                }));
            }
        }
        Ok(json!({"items":items,"nextCursor":next}))
    })
}

pub(super) fn get_reference(
    pool: &PgPool,
    input: KeyInput,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let key = valid_key(&input.key)?.to_owned();
    let pool = pool.clone();
    let actor_ctx = ctx;
    run_pg(async move {
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = simulation_capability(&mut connection, &input.id, actor_ctx).await?;

        let overlay = sqlx::query(
            "SELECT operation,document_json::text AS document_json
             FROM simulation_overlay_entries
             WHERE simulation_id=$1 AND tenant_id=$2
               AND resource_kind=$3 AND resource_key=$4",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(KIND)
        .bind(&key)
        .fetch_optional(&mut *connection)
        .await
        .map_err(db)?;

        if let Some(row) = overlay {
            let operation = row.try_get::<String, _>("operation").map_err(db)?;
            if operation == "tombstone" {
                return Err(err("SIMULATION_NOT_FOUND", "reference config not found"));
            }
            let document = row
                .try_get::<Option<String>, _>("document_json")
                .map_err(db)?
                .ok_or_else(|| err("SYS_DB", "visible simulation document is missing"))?;
            return Ok(json!({
                "key":key,
                "value":serde_json::from_str::<Value>(&document).map_err(serialization)?
            }));
        }

        let document: Option<String> = sqlx::query_scalar(
            "SELECT document_json::text
             FROM simulation_base_documents
             WHERE simulation_id=$1 AND tenant_id=$2
               AND resource_kind=$3 AND resource_key=$4",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(KIND)
        .bind(&key)
        .fetch_optional(&mut *connection)
        .await
        .map_err(db)?;

        let document =
            document.ok_or_else(|| err("SIMULATION_NOT_FOUND", "reference config not found"))?;
        Ok(json!({
            "key":key,
            "value":serde_json::from_str::<Value>(&document).map_err(serialization)?
        }))
    })
}

pub(super) fn mutate(
    pool: &PgPool,
    input: MutationInput,
    delete: bool,
    effect: Option<EffectInput>,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let pool = pool.clone();
    let actor_ctx = ctx;
    let key = valid_key(&input.key)?.to_owned();
    let idem = valid_idempotency(&input.idempotency_key)?.to_owned();
    let command = if effect.is_some() {
        "reference_config.record_effect"
    } else if delete {
        "reference_config.delete"
    } else {
        "reference_config.put"
    }
    .to_owned();
    let value = if delete {
        None
    } else {
        Some(valid_value(
            input
                .value
                .ok_or_else(|| err("VAL_REQUIRED", "value is required"))?,
        )?)
    };
    let request = digest_json(&json!({"key":key,"value":value,"effect":effect}));
    let key_hash = digest_str(&idem);

    run_pg(async move {
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = simulation_capability(&mut connection, &input.id, actor_ctx).await?;
        let actor = actor_ctx
            .actor()
            .id()
            .ok_or_else(|| err("AUTH_ACTOR_REQUIRED", "actor id required"))?
            .to_owned();
        drop(connection);

        let mut tx = begin_serializable(&pool).await?;
        if let Some(row) = sqlx::query(
            "SELECT request_digest,response_json::text AS response_json
             FROM simulation_command_results
             WHERE simulation_id=$1 AND actor_id=$2
               AND command=$3 AND command_idempotency_key_hash=$4",
        )
        .bind(&session.id)
        .bind(&actor)
        .bind(&command)
        .bind(&key_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        {
            let prior_digest = row.try_get::<String, _>("request_digest").map_err(db)?;
            let response = row.try_get::<String, _>("response_json").map_err(db)?;
            if prior_digest != request {
                return Err(err(
                    "IDEMPOTENCY_CONFLICT",
                    "idempotency key was used for another request",
                ));
            }
            tx.commit().await.map_err(db)?;
            return serde_json::from_str(&response).map_err(serialization);
        }

        admit_rate(&mut tx, &actor, &session.id, "data", 60, 120).await?;
        ensure_active_generation(&mut tx, &session).await?;
        sync_pricing_revision(&mut tx, &session.tenant_id).await?;

        let base = sqlx::query(
            "SELECT base_presence,captured_resource_version
             FROM simulation_revision_evidence
             WHERE simulation_id=$1 AND tenant_id=$2
               AND resource_kind=$3 AND resource_key=$4",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(KIND)
        .bind(&key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or_else(|| {
            err(
                "UNPROVISIONED_RESOURCE_KEY",
                "reference key was not provisioned",
            )
        })?;
        let base_present = base.try_get::<bool, _>("base_presence").map_err(db)?;
        let base_version = base
            .try_get::<i64, _>("captured_resource_version")
            .map_err(db)?;

        let old = sqlx::query(
            "SELECT operation,document_json::text AS document_json
             FROM simulation_overlay_entries
             WHERE simulation_id=$1 AND tenant_id=$2
               AND resource_kind=$3 AND resource_key=$4",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(KIND)
        .bind(&key)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;

        if delete && !base_present && old.is_none() {
            let response = json!({"key":key,"deleted":false});
            store_result(
                &mut tx,
                &session,
                &actor,
                &command,
                &key_hash,
                &request,
                &response,
                &shanghai_now().to_rfc3339(),
            )
            .await?;
            tx.commit().await.map_err(db)?;
            return Ok(response);
        }

        let now = shanghai_now().to_rfc3339();
        if delete && !base_present {
            sqlx::query(
                "DELETE FROM simulation_overlay_entries
                 WHERE simulation_id=$1 AND tenant_id=$2
                   AND resource_kind=$3 AND resource_key=$4",
            )
            .bind(&session.id)
            .bind(&session.tenant_id)
            .bind(KIND)
            .bind(&key)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        } else {
            let document = value.as_ref().map(canonical_json);
            sqlx::query(
                "INSERT INTO simulation_overlay_entries
                 (simulation_id,tenant_id,resource_kind,resource_key,operation,
                  document_json,base_resource_version,overlay_version,created_at,updated_at)
                 VALUES ($1,$2,$3,$4,$5,$6::jsonb,$7,1,$8::timestamptz,$8::timestamptz)
                 ON CONFLICT (simulation_id,tenant_id,resource_kind,resource_key)
                 DO UPDATE SET operation=excluded.operation,
                               document_json=excluded.document_json,
                               overlay_version=simulation_overlay_entries.overlay_version+1,
                               updated_at=excluded.updated_at",
            )
            .bind(&session.id)
            .bind(&session.tenant_id)
            .bind(KIND)
            .bind(&key)
            .bind(if delete { "tombstone" } else { "value" })
            .bind(document)
            .bind(base_version)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }

        refresh_usage(&mut tx, &session).await?;
        let response = json!({"key":key,"deleted":delete,"value":value});

        if let Some(effect) = effect {
            let note = plain(&effect.note, "effect.note", 200)?;
            if effect.kind != "reference" {
                return Err(err("VAL_EFFECT", "unsupported proof effect"));
            }
            let admitted: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM simulation_usage
                    WHERE simulation_id=$1 AND tenant_id=$2 AND effect_count < 1000
                 )",
            )
            .bind(&session.id)
            .bind(&session.tenant_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db)?;
            if !admitted {
                return Err(err("SIMULATION_QUOTA_EXCEEDED", "effect quota exceeded"));
            }

            sqlx::query(
                "INSERT INTO simulation_effect_records
                 (id,simulation_id,tenant_id,actor_id,command,command_idempotency_key_hash,
                  request_digest,effect_ordinal,effect_kind,payload_policy,payload_preview,
                  deterministic_result,correlation_id,created_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,0,'ReferenceEffect','REFERENCE_ONLY',
                         $8,$9::jsonb,$10,$11::timestamptz)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&session.id)
            .bind(&session.tenant_id)
            .bind(&actor)
            .bind(&command)
            .bind(&key_hash)
            .bind(&request)
            .bind(note)
            .bind(canonical_json(&json!({"recorded":true,"kind":"reference"})))
            .bind(actor_ctx.correlation_id().as_str())
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(db)?;

            sqlx::query(
                "UPDATE simulation_usage
                 SET effect_count=effect_count+1
                 WHERE simulation_id=$1 AND tenant_id=$2",
            )
            .bind(&session.id)
            .bind(&session.tenant_id)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }

        store_result(
            &mut tx, &session, &actor, &command, &key_hash, &request, &response, &now,
        )
        .await?;
        tx.commit().await.map_err(db)?;
        Ok(response)
    })
}

async fn refresh_usage(connection: &mut PgConnection, session: &Session) -> Result<(), String> {
    let row = sqlx::query(
        "SELECT COUNT(*)::bigint AS rows,
                COALESCE(SUM(length(COALESCE(document_json::text,''))),0)::bigint AS bytes
         FROM simulation_overlay_entries
         WHERE simulation_id=$1 AND tenant_id=$2",
    )
    .bind(&session.id)
    .bind(&session.tenant_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(db)?;
    let rows = row.try_get::<i64, _>("rows").map_err(db)?;
    let bytes = row.try_get::<i64, _>("bytes").map_err(db)?;
    if rows > MAX_OVERLAY_ROWS || bytes > MAX_DOCUMENT_BYTES {
        return Err(err("SIMULATION_QUOTA_EXCEEDED", "overlay quota exceeded"));
    }

    sqlx::query(
        "UPDATE simulation_usage
         SET overlay_rows=$3,document_bytes=$4
         WHERE simulation_id=$1 AND tenant_id=$2",
    )
    .bind(&session.id)
    .bind(&session.tenant_id)
    .bind(rows)
    .bind(bytes)
    .execute(&mut *connection)
    .await
    .map_err(db)?;
    Ok(())
}

async fn store_result(
    connection: &mut PgConnection,
    session: &Session,
    actor: &str,
    command: &str,
    key: &str,
    request: &str,
    response: &Value,
    now: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO simulation_command_results
         (simulation_id,tenant_id,actor_id,command,command_idempotency_key_hash,
          request_digest,response_status,response_json,created_at)
         VALUES ($1,$2,$3,$4,$5,$6,200,$7::jsonb,$8::timestamptz)",
    )
    .bind(&session.id)
    .bind(&session.tenant_id)
    .bind(actor)
    .bind(command)
    .bind(key)
    .bind(request)
    .bind(canonical_json(response))
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(db)?;
    Ok(())
}

pub(super) fn get_pricing(
    pool: &PgPool,
    input: IdInput,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let pool = pool.clone();
    run_pg(async move {
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = simulation_capability(&mut connection, &input.id, ctx).await?;
        pricing_visible_document(&mut connection, &session).await
    })
}

pub(super) fn put_pricing(
    pool: &PgPool,
    input: PricingMutationInput,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let value = valid_pricing_config(input.value)?;
    let idem = valid_idempotency(&input.idempotency_key)?.to_owned();
    let request = digest_json(&json!({"value":value}));
    let key_hash = digest_str(&idem);
    let pool = pool.clone();

    run_pg(async move {
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = simulation_capability(&mut connection, &input.id, ctx).await?;
        let actor = ctx
            .actor()
            .id()
            .ok_or_else(|| err("AUTH_ACTOR_REQUIRED", "actor id required"))?
            .to_owned();
        drop(connection);

        let mut tx = begin_serializable(&pool).await?;
        if let Some(row) = sqlx::query(
            "SELECT request_digest,response_json::text AS response_json
             FROM simulation_command_results
             WHERE simulation_id=$1 AND actor_id=$2
               AND command='pricing_config.put' AND command_idempotency_key_hash=$3",
        )
        .bind(&session.id)
        .bind(&actor)
        .bind(&key_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        {
            let prior_digest = row.try_get::<String, _>("request_digest").map_err(db)?;
            let response = row.try_get::<String, _>("response_json").map_err(db)?;
            if prior_digest != request {
                return Err(err(
                    "IDEMPOTENCY_CONFLICT",
                    "idempotency key was used for another request",
                ));
            }
            tx.commit().await.map_err(db)?;
            return serde_json::from_str(&response).map_err(serialization);
        }

        admit_rate(&mut tx, &actor, &session.id, "data", 60, 120).await?;
        ensure_active_generation(&mut tx, &session).await?;
        sync_pricing_revision(&mut tx, &session.tenant_id).await?;

        let base = sqlx::query(
            "SELECT base_presence,captured_resource_version
             FROM simulation_revision_evidence
             WHERE simulation_id=$1 AND tenant_id=$2
               AND resource_kind=$3 AND resource_key=$4",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(PRICING_KIND)
        .bind(PRICING_KEY)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .ok_or_else(|| err("SYS_MANIFEST", "pricing config was not provisioned"))?;
        let base_version = base
            .try_get::<i64, _>("captured_resource_version")
            .map_err(db)?;

        let now = shanghai_now().to_rfc3339();
        let document = canonical_json(&value);
        sqlx::query(
            "INSERT INTO simulation_overlay_entries
             (simulation_id,tenant_id,resource_kind,resource_key,operation,
              document_json,base_resource_version,overlay_version,created_at,updated_at)
             VALUES ($1,$2,$3,$4,'value',$5::jsonb,$6,1,$7::timestamptz,$7::timestamptz)
             ON CONFLICT (simulation_id,tenant_id,resource_kind,resource_key)
             DO UPDATE SET operation='value',
                           document_json=excluded.document_json,
                           overlay_version=simulation_overlay_entries.overlay_version+1,
                           updated_at=excluded.updated_at",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(PRICING_KIND)
        .bind(PRICING_KEY)
        .bind(document)
        .bind(base_version)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(db)?;

        refresh_usage(&mut tx, &session).await?;
        let response = json!({"key":PRICING_KEY,"value":value});
        store_result(
            &mut tx,
            &session,
            &actor,
            "pricing_config.put",
            &key_hash,
            &request,
            &response,
            &now,
        )
        .await?;

        tx.commit().await.map_err(db)?;
        Ok(response)
    })
}

pub(super) fn estimate_pricing(
    pool: &PgPool,
    input: PricingEstimateInput,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let start = chrono::NaiveDate::parse_from_str(&input.start_date, "%Y-%m-%d")
        .map_err(|_| err("VAL_START_DATE", "startDate must be YYYY-MM-DD"))?;
    let end = chrono::NaiveDate::parse_from_str(&input.end_date, "%Y-%m-%d")
        .map_err(|_| err("VAL_END_DATE", "endDate must be YYYY-MM-DD"))?;
    if end < start {
        return Err(err("VAL_DATE_RANGE", "endDate must not precede startDate"));
    }
    if (end - start).num_days() > 90 {
        return Err(err(
            "VAL_DATE_RANGE",
            "simulation estimate range must not exceed 90 days",
        ));
    }

    let pool = pool.clone();
    run_pg(async move {
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = simulation_capability(&mut connection, &input.id, ctx).await?;
        drop(connection);

        {
            let mut tx = begin_serializable(&pool).await?;
            ensure_active_generation(&mut tx, &session).await?;
            admit_rate(
                &mut tx,
                ctx.actor().id().unwrap_or_default(),
                &session.id,
                "estimate",
                60,
                60,
            )
            .await?;
            tx.commit().await.map_err(db)?;
        }

        let mut connection = pool.acquire().await.map_err(db)?;
        let config = pricing_visible_document(&mut connection, &session).await?["value"].clone();
        let weekday = config
            .get("baseWeekdayPrice")
            .and_then(Value::as_f64)
            .ok_or_else(|| err("SYS_PRICING", "pricing document is invalid"))?;
        let weekend = config
            .get("baseWeekendPrice")
            .and_then(Value::as_f64)
            .ok_or_else(|| err("SYS_PRICING", "pricing document is invalid"))?;
        let holiday = config
            .get("holidayRules")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let dynamic = config.get("dynamicPriceMap").and_then(Value::as_object);

        let mut days = Vec::new();
        let mut total = 0.0;
        let mut day = start;
        while day <= end {
            let date = day.format("%Y-%m-%d").to_string();
            let is_weekend = matches!(
                day.weekday(),
                chrono::Weekday::Fri | chrono::Weekday::Sat | chrono::Weekday::Sun
            );
            let mut price = if is_weekend { weekend } else { weekday };
            let mut source = if is_weekend { "weekend" } else { "weekday" };

            for rule in &holiday {
                let rule_start = rule.get("startDate").and_then(Value::as_str);
                let rule_end = rule.get("endDate").and_then(Value::as_str);
                let include_previous = rule
                    .get("includePreviousDay")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let previous = rule_start
                    .and_then(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
                    .and_then(|value| value.pred_opt())
                    .map(|value| value.format("%Y-%m-%d").to_string());

                if (rule_start
                    .zip(rule_end)
                    .is_some_and(|(left, right)| left <= date.as_str() && date.as_str() <= right)
                    || (include_previous && previous.as_deref() == Some(date.as_str())))
                    && rule.get("price").and_then(Value::as_f64).is_some()
                {
                    price = rule.get("price").and_then(Value::as_f64).unwrap();
                    source = "holiday";
                    break;
                }
            }

            if let Some(value) = dynamic
                .and_then(|map| map.get(&date))
                .and_then(Value::as_f64)
            {
                price = value;
                source = "dynamic";
            }

            total += price;
            days.push(json!({"date":date,"price":price,"source":source}));
            day = day
                .succ_opt()
                .ok_or_else(|| err("VAL_DATE_RANGE", "invalid date"))?;
        }

        Ok(json!({
            "startDate":input.start_date,
            "endDate":input.end_date,
            "days":days,
            "total":total,
            "currency":"CNY"
        }))
    })
}

async fn pricing_document(connection: &mut PgConnection, tenant_id: &str) -> Result<Value, String> {
    let row = sqlx::query(
        "SELECT baseweekdayprice,
                baseweekendprice,
                holidayrulesjson::text AS holiday_rules,
                COALESCE(receiveshippingfeesjson,'{}'::jsonb)::text AS shipping
         FROM pricing_configs
         WHERE tenant_id=$1
         LIMIT 1",
    )
    .bind(tenant_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?
    .ok_or_else(|| err("SIMULATION_NOT_FOUND", "tenant pricing config not found"))?;

    let weekday = row.try_get::<f64, _>("baseweekdayprice").map_err(db)?;
    let weekend = row.try_get::<f64, _>("baseweekendprice").map_err(db)?;
    let holiday_text = row.try_get::<String, _>("holiday_rules").map_err(db)?;
    let shipping_text = row.try_get::<String, _>("shipping").map_err(db)?;
    let holidays = serde_json::from_str::<Value>(&holiday_text).map_err(serialization)?;
    let shipping = serde_json::from_str::<Value>(&shipping_text).map_err(serialization)?;

    let today = shanghai_now().date_naive();
    let end = (today + Duration::days(14)).format("%Y-%m-%d").to_string();
    let rows = sqlx::query(
        "SELECT datekey,price
         FROM dynamic_daily_prices
         WHERE tenant_id=$1 AND datekey >= $2 AND datekey <= $3
         ORDER BY datekey",
    )
    .bind(tenant_id)
    .bind(today.format("%Y-%m-%d").to_string())
    .bind(end)
    .fetch_all(&mut *connection)
    .await
    .map_err(db)?;

    let mut dynamic = Map::new();
    for row in rows {
        dynamic.insert(
            row.try_get::<String, _>("datekey").map_err(db)?,
            json!(row.try_get::<f64, _>("price").map_err(db)?),
        );
    }

    valid_pricing_config(json!({
        "baseWeekdayPrice":weekday,
        "baseWeekendPrice":weekend,
        "holidayRules":holidays,
        "receiveShippingFees":shipping,
        "dynamicPriceMap":dynamic
    }))
}

async fn sync_pricing_revision(
    connection: &mut PgConnection,
    tenant_id: &str,
) -> Result<(), String> {
    let document = pricing_document(connection, tenant_id).await?;
    let canonical = canonical_json(&document);

    let current = sqlx::query(
        "SELECT document_json::text AS document_json,resource_version
         FROM simulation_pricing_resource_revisions
         WHERE tenant_id=$1 AND resource_key=$2
         FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(PRICING_KEY)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?;

    if current.as_ref().is_some_and(|row| {
        row.try_get::<String, _>("document_json")
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .map(|value| canonical_json(&value) == canonical)
            .unwrap_or(false)
    }) {
        return Ok(());
    }

    let version = current
        .as_ref()
        .and_then(|row| row.try_get::<i64, _>("resource_version").ok())
        .unwrap_or(0)
        + 1;
    let now = shanghai_now().to_rfc3339();

    sqlx::query(
        "INSERT INTO simulation_pricing_resource_revisions
         (tenant_id,resource_key,present,resource_version,document_json,updated_at)
         VALUES ($1,$2,1,$3,$4::jsonb,$5::timestamptz)
         ON CONFLICT (tenant_id,resource_key)
         DO UPDATE SET present=1,
                       resource_version=excluded.resource_version,
                       document_json=excluded.document_json,
                       updated_at=excluded.updated_at",
    )
    .bind(tenant_id)
    .bind(PRICING_KEY)
    .bind(version)
    .bind(&canonical)
    .bind(&now)
    .execute(&mut *connection)
    .await
    .map_err(db)?;

    sqlx::query(
        "INSERT INTO simulation_tenant_revisions (tenant_id,revision,updated_at)
         VALUES ($1,1,$2::timestamptz)
         ON CONFLICT (tenant_id)
         DO UPDATE SET revision=simulation_tenant_revisions.revision+1,
                       updated_at=excluded.updated_at",
    )
    .bind(tenant_id)
    .bind(&now)
    .execute(&mut *connection)
    .await
    .map_err(db)?;

    Ok(())
}

async fn pricing_visible_document(
    connection: &mut PgConnection,
    session: &Session,
) -> Result<Value, String> {
    let overlay = sqlx::query(
        "SELECT operation,document_json::text AS document_json
         FROM simulation_overlay_entries
         WHERE simulation_id=$1 AND tenant_id=$2
           AND resource_kind=$3 AND resource_key=$4",
    )
    .bind(&session.id)
    .bind(&session.tenant_id)
    .bind(PRICING_KIND)
    .bind(PRICING_KEY)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?;

    if let Some(row) = overlay {
        let operation = row.try_get::<String, _>("operation").map_err(db)?;
        let document = row
            .try_get::<Option<String>, _>("document_json")
            .map_err(db)?;
        if operation == "value" {
            if let Some(document) = document {
                return Ok(json!({
                    "key":PRICING_KEY,
                    "value":serde_json::from_str::<Value>(&document).map_err(serialization)?
                }));
            }
        }
    }

    let document: String = sqlx::query_scalar(
        "SELECT document_json::text
         FROM simulation_base_documents
         WHERE simulation_id=$1 AND tenant_id=$2
           AND resource_kind=$3 AND resource_key=$4",
    )
    .bind(&session.id)
    .bind(&session.tenant_id)
    .bind(PRICING_KIND)
    .bind(PRICING_KEY)
    .fetch_optional(&mut *connection)
    .await
    .map_err(db)?
    .ok_or_else(|| {
        err(
            "SIMULATION_NOT_FOUND",
            "simulation pricing config not found",
        )
    })?;

    Ok(json!({
        "key":PRICING_KEY,
        "value":serde_json::from_str::<Value>(&document).map_err(serialization)?
    }))
}

pub(super) fn diff(pool: &PgPool, input: IdInput, ctx: &ExecutionContext) -> Result<Value, String> {
    let pool = pool.clone();
    run_pg(async move {
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = simulation_capability(&mut connection, &input.id, ctx).await?;
        drop(connection);

        let mut tx = begin_serializable(&pool).await?;
        ensure_active_generation(&mut tx, &session).await?;
        sync_pricing_revision(&mut tx, &session.tenant_id).await?;
        admit_rate(
            &mut tx,
            ctx.actor().id().unwrap_or_default(),
            &session.id,
            "diff",
            60,
            10,
        )
        .await?;

        let current: i64 = sqlx::query_scalar(
            "SELECT revision FROM simulation_tenant_revisions WHERE tenant_id=$1",
        )
        .bind(&session.tenant_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?
        .unwrap_or(0);

        let rows = sqlx::query(
            "SELECT e.resource_key,
                    e.base_presence,
                    e.captured_resource_version,
                    o.operation,
                    o.document_json::text AS overlay_document,
                    r.present AS current_present,
                    r.resource_version AS current_version
             FROM simulation_revision_evidence e
             LEFT JOIN simulation_overlay_entries o
               ON o.simulation_id=e.simulation_id
              AND o.tenant_id=e.tenant_id
              AND o.resource_kind=e.resource_kind
              AND o.resource_key=e.resource_key
             LEFT JOIN simulation_reference_resource_revisions r
               ON r.tenant_id=e.tenant_id AND r.key=e.resource_key
             WHERE e.simulation_id=$1 AND e.tenant_id=$2
               AND e.resource_kind='reference_config'
             ORDER BY e.resource_key",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(db)?;

        let mut items = Vec::new();
        for row in rows {
            let key = row.try_get::<String, _>("resource_key").map_err(db)?;
            let base_present = row.try_get::<bool, _>("base_presence").map_err(db)?;
            let base_version = row
                .try_get::<i64, _>("captured_resource_version")
                .map_err(db)?;
            let operation = row.try_get::<Option<String>, _>("operation").map_err(db)?;
            let current_present = row
                .try_get::<Option<bool>, _>("current_present")
                .map_err(db)?;
            let current_version = row
                .try_get::<Option<i64>, _>("current_version")
                .map_err(db)?;
            let changed = operation.is_some();
            let equal = current_present.unwrap_or(false) == base_present
                && current_version.unwrap_or(0) == base_version;

            let status = if changed && equal {
                "clean"
            } else if changed {
                "conflict"
            } else if !equal {
                "stale"
            } else {
                continue;
            };

            items.push(json!({
                "resourceKind":KIND,
                "resourceKey":key,
                "operation":operation.unwrap_or_else(||"none".into()),
                "conflictStatus":status,
                "baseVersion":base_version.to_string(),
                "currentVersion":current_version.unwrap_or(0).to_string(),
                "before":{"policy":"REFERENCE_ONLY"},
                "after":{"policy":"REFERENCE_ONLY"}
            }));
        }

        let pricing = sqlx::query(
            "SELECT e.base_presence,
                    e.captured_resource_version,
                    o.operation,
                    r.resource_version AS current_version
             FROM simulation_revision_evidence e
             LEFT JOIN simulation_overlay_entries o
               ON o.simulation_id=e.simulation_id
              AND o.tenant_id=e.tenant_id
              AND o.resource_kind=e.resource_kind
              AND o.resource_key=e.resource_key
             LEFT JOIN simulation_pricing_resource_revisions r
               ON r.tenant_id=e.tenant_id AND r.resource_key=e.resource_key
             WHERE e.simulation_id=$1 AND e.tenant_id=$2
               AND e.resource_kind=$3 AND e.resource_key=$4",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(PRICING_KIND)
        .bind(PRICING_KEY)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        let pricing_base_version = pricing
            .try_get::<i64, _>("captured_resource_version")
            .map_err(db)?;
        let pricing_operation = pricing
            .try_get::<Option<String>, _>("operation")
            .map_err(db)?;
        let pricing_current_version = pricing
            .try_get::<Option<i64>, _>("current_version")
            .map_err(db)?;
        let pricing_changed = pricing_operation.is_some();
        let pricing_equal = pricing_current_version.unwrap_or(0) == pricing_base_version;

        if pricing_changed || !pricing_equal {
            items.push(json!({
                "resourceKind":PRICING_KIND,
                "resourceKey":PRICING_KEY,
                "operation":pricing_operation.unwrap_or_else(||"none".into()),
                "conflictStatus":if pricing_changed && pricing_equal {
                    "clean"
                } else if pricing_changed {
                    "conflict"
                } else {
                    "stale"
                },
                "baseVersion":pricing_base_version.to_string(),
                "currentVersion":pricing_current_version.unwrap_or(0).to_string(),
                "before":{"policy":"REFERENCE_ONLY"},
                "after":{"policy":"REFERENCE_ONLY"}
            }));
        }

        let canonical = canonical_json(&Value::Array(items.clone()));
        let overlay_digest = digest_str(&canonical);
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT evaluation_id
             FROM simulation_diff_evidence
             WHERE simulation_id=$1 AND generation=$2
               AND evaluated_revision=$3 AND overlay_digest=$4",
        )
        .bind(&session.id)
        .bind(session.generation)
        .bind(current)
        .bind(&overlay_digest)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;

        if let Some(existing) = existing {
            tx.commit().await.map_err(db)?;
            return Ok(json!({
                "evaluationId":existing,
                "items":items,
                "nextCursor":Value::Null
            }));
        }

        let evidence_bytes = canonical.len() as i64;
        let admitted: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM simulation_usage
                WHERE simulation_id=$1 AND tenant_id=$2
                  AND evaluation_count < 20
                  AND evidence_bytes + $3 <= 26214400
             )",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(evidence_bytes)
        .fetch_one(&mut *tx)
        .await
        .map_err(db)?;
        if !admitted {
            return Err(err(
                "SIMULATION_QUOTA_EXCEEDED",
                "diff evidence quota exceeded",
            ));
        }

        let evaluation = uuid::Uuid::new_v4().to_string();
        let now = shanghai_now().to_rfc3339();
        sqlx::query(
            "INSERT INTO simulation_diff_evidence
             (simulation_id,tenant_id,evaluation_id,evaluated_revision,generation,
              overlay_digest,item_count,total_bytes,digest,status,created_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,'ready',$10::timestamptz)",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(&evaluation)
        .bind(current)
        .bind(session.generation)
        .bind(&overlay_digest)
        .bind(items.len() as i64)
        .bind(canonical.len() as i64)
        .bind(digest_str(&canonical))
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(db)?;

        for (ordinal, item) in items.iter().enumerate() {
            let item = canonical_json(item);
            sqlx::query(
                "INSERT INTO simulation_diff_evidence_items
                 (simulation_id,tenant_id,evaluation_id,ordinal,canonical_item_json,item_bytes)
                 VALUES ($1,$2,$3,$4,$5::jsonb,$6)",
            )
            .bind(&session.id)
            .bind(&session.tenant_id)
            .bind(&evaluation)
            .bind(ordinal as i32)
            .bind(&item)
            .bind(item.len() as i32)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        }

        sqlx::query(
            "UPDATE simulation_usage
             SET evaluation_count=evaluation_count+1,evidence_bytes=evidence_bytes+$3
             WHERE simulation_id=$1 AND tenant_id=$2",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(evidence_bytes)
        .execute(&mut *tx)
        .await
        .map_err(db)?;

        tx.commit().await.map_err(db)?;
        Ok(json!({
            "evaluationId":evaluation,
            "items":items,
            "nextCursor":Value::Null
        }))
    })
}

pub(super) fn diff_get(
    pool: &PgPool,
    input: EvidencePageInput,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let pool = pool.clone();
    run_pg(async move {
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = evidence_capability(&mut connection, &input.id, ctx).await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM simulation_diff_evidence
                WHERE simulation_id=$1 AND tenant_id=$2
                  AND evaluation_id=$3 AND status='ready'
             )",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(&input.evaluation_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(db)?;
        if !exists {
            return Err(err(
                "DIFF_EVALUATION_NOT_FOUND",
                "diff evaluation not found",
            ));
        }

        let limit = page_limit(input.limit)? as i64;
        let cursor = input.cursor.unwrap_or(-1);
        let rows = sqlx::query(
            "SELECT ordinal,canonical_item_json::text AS canonical_item_json
             FROM simulation_diff_evidence_items
             WHERE simulation_id=$1 AND tenant_id=$2
               AND evaluation_id=$3 AND ordinal>$4
             ORDER BY ordinal
             LIMIT $5",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(&input.evaluation_id)
        .bind(cursor)
        .bind(limit)
        .fetch_all(&mut *connection)
        .await
        .map_err(db)?;

        let mut items = Vec::new();
        let mut last = None;
        for row in rows {
            let ordinal = row.try_get::<i32, _>("ordinal").map_err(db)? as i64;
            let item = row
                .try_get::<String, _>("canonical_item_json")
                .map_err(db)?;
            last = Some(ordinal);
            items.push(serde_json::from_str::<Value>(&item).map_err(serialization)?);
        }

        Ok(json!({
            "evaluationId":input.evaluation_id,
            "items":items,
            "nextCursor":last
        }))
    })
}

pub(super) fn effects_list(
    pool: &PgPool,
    input: PageInput,
    ctx: &ExecutionContext,
) -> Result<Value, String> {
    let pool = pool.clone();
    run_pg(async move {
        let mut connection = pool.acquire().await.map_err(db)?;
        let session = evidence_capability(&mut connection, &input.id, ctx).await?;
        let limit = page_limit(input.limit)? as i64;
        let cursor = input.cursor.unwrap_or_default();

        let rows = sqlx::query(
            "SELECT id,command,effect_kind,payload_policy,payload_preview,
                    deterministic_result::text AS deterministic_result,created_at
             FROM simulation_effect_records
             WHERE simulation_id=$1 AND tenant_id=$2 AND id>$3
             ORDER BY id
             LIMIT $4",
        )
        .bind(&session.id)
        .bind(&session.tenant_id)
        .bind(cursor)
        .bind(limit)
        .fetch_all(&mut *connection)
        .await
        .map_err(db)?;

        let mut items = Vec::new();
        for row in rows {
            let id = row.try_get::<String, _>("id").map_err(db)?;
            let command = row.try_get::<String, _>("command").map_err(db)?;
            let kind = row.try_get::<String, _>("effect_kind").map_err(db)?;
            let policy = row.try_get::<String, _>("payload_policy").map_err(db)?;
            let preview = row.try_get::<String, _>("payload_preview").map_err(db)?;
            let result = row
                .try_get::<String, _>("deterministic_result")
                .map_err(db)?;
            let created = timestamp_text(row.try_get("created_at").map_err(db)?);
            items.push(json!({
                "id":id,
                "command":command,
                "kind":kind,
                "payloadPolicy":policy,
                "payloadPreview":preview,
                "result":serde_json::from_str::<Value>(&result).map_err(serialization)?,
                "createdAt":created
            }));
        }
        let next = items
            .last()
            .and_then(|value| value["id"].as_str())
            .map(str::to_owned);
        Ok(json!({"items":items,"nextCursor":next}))
    })
}

async fn materialize_terminal_inputs(
    connection: &mut PgConnection,
    simulation_id: &str,
    tenant_id: &str,
    generation: i64,
    now: &str,
) -> Result<(), String> {
    sync_pricing_revision(connection, tenant_id).await?;

    let revision: i64 =
        sqlx::query_scalar("SELECT revision FROM simulation_tenant_revisions WHERE tenant_id=$1")
            .bind(tenant_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(db)?
            .unwrap_or(0);

    let candidate = sqlx::query(
        "SELECT COUNT(*)::bigint AS rows,
                COALESCE(
                  SUM(
                    length(COALESCE(o.document_json::text,'')) +
                    length(COALESCE(c.value_json::text,''))
                  ),
                  0
                )::bigint AS bytes
         FROM simulation_revision_evidence e
         LEFT JOIN simulation_overlay_entries o
           ON o.simulation_id=e.simulation_id
          AND o.tenant_id=e.tenant_id
          AND o.resource_kind=e.resource_kind
          AND o.resource_key=e.resource_key
         LEFT JOIN simulation_reference_configs c
           ON c.tenant_id=e.tenant_id AND c.key=e.resource_key
         WHERE e.simulation_id=$1 AND e.tenant_id=$2
           AND e.resource_kind='reference_config'",
    )
    .bind(simulation_id)
    .bind(tenant_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(db)?;
    let candidate_rows = candidate.try_get::<i64, _>("rows").map_err(db)?;
    let candidate_bytes = candidate.try_get::<i64, _>("bytes").map_err(db)?;

    let pricing_candidate_bytes: i64 = sqlx::query_scalar(
        "SELECT (
            length(COALESCE(b.document_json::text,'')) +
            length(COALESCE(o.document_json::text,'')) +
            length(COALESCE(r.document_json::text,''))
         )::bigint
         FROM simulation_revision_evidence e
         LEFT JOIN simulation_base_documents b
           ON b.simulation_id=e.simulation_id
          AND b.tenant_id=e.tenant_id
          AND b.resource_kind=e.resource_kind
          AND b.resource_key=e.resource_key
         LEFT JOIN simulation_overlay_entries o
           ON o.simulation_id=e.simulation_id
          AND o.tenant_id=e.tenant_id
          AND o.resource_kind=e.resource_kind
          AND o.resource_key=e.resource_key
         LEFT JOIN simulation_pricing_resource_revisions r
           ON r.tenant_id=e.tenant_id AND r.resource_key=e.resource_key
         WHERE e.simulation_id=$1 AND e.tenant_id=$2
           AND e.resource_kind=$3 AND e.resource_key=$4",
    )
    .bind(simulation_id)
    .bind(tenant_id)
    .bind(PRICING_KIND)
    .bind(PRICING_KEY)
    .fetch_one(&mut *connection)
    .await
    .map_err(db)?;

    if candidate_rows + 1 > MAX_PROOF_ROWS
        || candidate_bytes + pricing_candidate_bytes > MAX_PROOF_BYTES
    {
        return Err(err(
            "SIMULATION_QUOTA_EXCEEDED",
            "terminal evidence input exceeds quota",
        ));
    }

    let rows = sqlx::query(
        "SELECT e.resource_key,e.base_presence,e.captured_resource_version,
                o.operation,o.document_json::text AS overlay_document,
                r.present AS current_presence,r.resource_version AS current_version,
                c.value_json::text AS current_document
         FROM simulation_revision_evidence e
         LEFT JOIN simulation_overlay_entries o
           ON o.simulation_id=e.simulation_id
          AND o.tenant_id=e.tenant_id
          AND o.resource_kind=e.resource_kind
          AND o.resource_key=e.resource_key
         LEFT JOIN simulation_reference_resource_revisions r
           ON r.tenant_id=e.tenant_id AND r.key=e.resource_key
         LEFT JOIN simulation_reference_configs c
           ON c.tenant_id=e.tenant_id AND c.key=e.resource_key
         WHERE e.simulation_id=$1 AND e.tenant_id=$2
           AND e.resource_kind='reference_config'
         ORDER BY e.resource_key",
    )
    .bind(simulation_id)
    .bind(tenant_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(db)?;

    let mut values = Vec::new();
    for row in rows {
        let overlay = row
            .try_get::<Option<String>, _>("overlay_document")
            .map_err(db)?
            .and_then(|value| serde_json::from_str::<Value>(&value).ok());
        let current = row
            .try_get::<Option<String>, _>("current_document")
            .map_err(db)?
            .and_then(|value| serde_json::from_str::<Value>(&value).ok());
        let base_presence = row.try_get::<bool, _>("base_presence").map_err(db)?;
        let current_presence = row
            .try_get::<Option<bool>, _>("current_presence")
            .map_err(db)?
            .unwrap_or(false);

        values.push(canonical_json(&json!({
            "resourceKind":KIND,
            "resourceKey":row.try_get::<String,_>("resource_key").map_err(db)?,
            "basePresence":if base_presence {1} else {0},
            "baseVersion":row.try_get::<i64,_>("captured_resource_version").map_err(db)?,
            "overlayOperation":row.try_get::<Option<String>,_>("operation").map_err(db)?,
            "overlay":overlay,
            "currentPresence":if current_presence {1} else {0},
            "currentVersion":row.try_get::<Option<i64>,_>("current_version").map_err(db)?.unwrap_or(0),
            "current":current
        })));
    }

    let pricing = sqlx::query(
        "SELECT e.base_presence,e.captured_resource_version,
                b.document_json::text AS base_document,
                o.operation,o.document_json::text AS overlay_document,
                r.present AS current_presence,r.resource_version AS current_version,
                r.document_json::text AS current_document
         FROM simulation_revision_evidence e
         LEFT JOIN simulation_base_documents b
           ON b.simulation_id=e.simulation_id
          AND b.tenant_id=e.tenant_id
          AND b.resource_kind=e.resource_kind
          AND b.resource_key=e.resource_key
         LEFT JOIN simulation_overlay_entries o
           ON o.simulation_id=e.simulation_id
          AND o.tenant_id=e.tenant_id
          AND o.resource_kind=e.resource_kind
          AND o.resource_key=e.resource_key
         LEFT JOIN simulation_pricing_resource_revisions r
           ON r.tenant_id=e.tenant_id AND r.resource_key=e.resource_key
         WHERE e.simulation_id=$1 AND e.tenant_id=$2
           AND e.resource_kind=$3 AND e.resource_key=$4",
    )
    .bind(simulation_id)
    .bind(tenant_id)
    .bind(PRICING_KIND)
    .bind(PRICING_KEY)
    .fetch_one(&mut *connection)
    .await
    .map_err(db)?;

    let base_presence = pricing.try_get::<bool, _>("base_presence").map_err(db)?;
    let base_document =
        serde_json::from_str::<Value>(&pricing.try_get::<String, _>("base_document").map_err(db)?)
            .ok();
    let overlay = pricing
        .try_get::<Option<String>, _>("overlay_document")
        .map_err(db)?
        .and_then(|value| serde_json::from_str::<Value>(&value).ok());
    let current_document = serde_json::from_str::<Value>(
        &pricing
            .try_get::<String, _>("current_document")
            .map_err(db)?,
    )
    .ok();
    let current_presence = pricing.try_get::<i16, _>("current_presence").map_err(db)?;

    values.push(canonical_json(&json!({
        "resourceKind":PRICING_KIND,
        "resourceKey":PRICING_KEY,
        "basePresence":if base_presence {1} else {0},
        "baseVersion":pricing.try_get::<i64,_>("captured_resource_version").map_err(db)?,
        "base":base_document,
        "overlayOperation":pricing.try_get::<Option<String>,_>("operation").map_err(db)?,
        "overlay":overlay,
        "currentPresence":current_presence,
        "currentVersion":pricing.try_get::<i64,_>("current_version").map_err(db)?,
        "current":current_document
    })));

    let canonical_bytes = values.iter().map(|value| value.len() as i64).sum::<i64>();
    if values.len() as i64 > MAX_PROOF_ROWS || canonical_bytes > MAX_PROOF_BYTES {
        return Err(err(
            "SIMULATION_QUOTA_EXCEEDED",
            "canonical terminal evidence exceeds quota",
        ));
    }

    let digest = digest_str(&values.join("\n"));
    sqlx::query(
        "INSERT INTO simulation_terminal_inputs
         (simulation_id,tenant_id,generation,evaluated_revision,overlay_digest,
          item_count,input_bytes,created_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8::timestamptz)",
    )
    .bind(simulation_id)
    .bind(tenant_id)
    .bind(generation)
    .bind(revision)
    .bind(&digest)
    .bind(values.len() as i64)
    .bind(canonical_bytes)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(db)?;

    for (ordinal, value) in values.iter().enumerate() {
        sqlx::query(
            "INSERT INTO simulation_terminal_input_items
             (simulation_id,tenant_id,generation,ordinal,canonical_input_json,item_bytes)
             VALUES ($1,$2,$3,$4,$5::jsonb,$6)",
        )
        .bind(simulation_id)
        .bind(tenant_id)
        .bind(generation)
        .bind(ordinal as i32)
        .bind(value)
        .bind(value.len() as i32)
        .execute(&mut *connection)
        .await
        .map_err(db)?;
    }

    Ok(())
}

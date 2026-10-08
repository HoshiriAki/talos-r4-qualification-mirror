//! Isolated tenant simulation runtime.
//!
//! This crate owns the durable simulation aggregate and the only MVP3 storage
//! adapter. It deliberately exposes a bounded `reference_config` proof resource;
//! it is not a generic proxy to production modules.

use std::sync::Mutex;

#[cfg(feature = "postgres")]
mod postgres;

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use chrono_tz::Asia::Shanghai;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ExecutionMode, ModuleMetadata, ModuleSchema, Namespace, SimulationId, SimulationSupport,
    SystemModule,
};

const KIND: &str = "reference_config";
const PRICING_KIND: &str = "pricing_config";
const PRICING_KEY: &str = "default";
const MAX_TTL_MINUTES: i64 = 120;
const MAX_KEYS: usize = 100;
const MAX_PAGE: u32 = 500;
const MAX_OVERLAY_ROWS: i64 = 10_000;
const MAX_DOCUMENT_BYTES: i64 = 50 * 1024 * 1024;
const MAX_PROOF_ROWS: i64 = 2_000;
const MAX_PROOF_BYTES: i64 = 10 * 1024 * 1024;

enum SimulationBackend {
    Unconfigured,
    Sqlite(Pool<SqliteConnectionManager>),
    #[cfg(feature = "postgres")]
    Postgres(sqlx::PgPool),
}

pub struct FeatureTenantSimulation {
    backend: Mutex<SimulationBackend>,
}

impl FeatureTenantSimulation {
    pub fn new() -> Self {
        Self {
            backend: Mutex::new(SimulationBackend::Unconfigured),
        }
    }

    pub fn with_pool(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            backend: Mutex::new(SimulationBackend::Sqlite(pool)),
        }
    }

    #[cfg(feature = "postgres")]
    pub fn with_postgres(pool: sqlx::PgPool) -> Self {
        Self {
            backend: Mutex::new(SimulationBackend::Postgres(pool)),
        }
    }

    fn pool(&self) -> Result<Pool<SqliteConnectionManager>, String> {
        match &*self
            .backend
            .lock()
            .map_err(|_| err("SYS_LOCK", "simulation backend lock failed"))?
        {
            SimulationBackend::Sqlite(pool) => Ok(pool.clone()),
            SimulationBackend::Unconfigured => {
                Err(err("SYS_NO_DB", "simulation database is not configured"))
            }
            #[cfg(feature = "postgres")]
            SimulationBackend::Postgres(_) => Err(err(
                "SYS_BACKEND_MISMATCH",
                "SQLite simulation adapter requested from PostgreSQL composition",
            )),
        }
    }

    #[cfg(feature = "postgres")]
    fn postgres_pool(&self) -> Result<Option<sqlx::PgPool>, String> {
        match &*self
            .backend
            .lock()
            .map_err(|_| err("SYS_LOCK", "simulation backend lock failed"))?
        {
            SimulationBackend::Postgres(pool) => Ok(Some(pool.clone())),
            SimulationBackend::Sqlite(_) => Ok(None),
            SimulationBackend::Unconfigured => {
                Err(err("SYS_NO_DB", "simulation database is not configured"))
            }
        }
    }

    fn require_platform(ctx: &ExecutionContext) -> Result<&str, String> {
        if !ctx.actor().has_platform_authority()
            || !ctx.data_scope().is_platform()
            || ctx.execution_mode() != &ExecutionMode::Normal
        {
            return Err(err(
                "AUTH_PLATFORM_REQUIRED",
                "platform authority context required",
            ));
        }
        ctx.actor()
            .id()
            .ok_or_else(|| err("AUTH_ACTOR_REQUIRED", "actor id required"))
    }

    fn simulation_capability<'a>(
        &self,
        conn: &Connection,
        id: &str,
        ctx: &'a ExecutionContext,
    ) -> Result<Session, String> {
        let actor = ctx
            .actor()
            .id()
            .ok_or_else(|| err("SIMULATION_NOT_FOUND", "simulation not found"))?;
        let session = load_session(conn, id, actor)?;
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
        let tenant_active: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tenants WHERE id=?1 AND status='active')",
                [&session.tenant_id],
                |r| r.get(0),
            )
            .map_err(db)?;
        if !tenant_active {
            return Err(err(
                "SESSION_NOT_EXECUTABLE",
                "simulation tenant is not active",
            ));
        }
        Ok(session)
    }

    /// Evidence remains readable after expiry or discard. It never grants an
    /// execution context and still resolves ownership before returning data.
    fn evidence_capability(
        &self,
        conn: &Connection,
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
        let session = load_session(conn, id, actor)?;
        if !matches!(session.status.as_str(), "active" | "expired" | "discarded") {
            return Err(err("SIMULATION_NOT_FOUND", "simulation not found"));
        }
        Ok(session)
    }

    fn create(&self, input: CreateInput, ctx: &ExecutionContext) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::create(&pool, input, ctx);
        }
        let actor = Self::require_platform(ctx)?;
        let tenant = required(&input.tenant_id, "tenantId")?.to_string();
        let scenario = plain(&input.scenario_name, "scenarioName", 80)?;
        let intent = plain(&input.change_intent, "changeIntent", 500)?;
        let ttl = input.ttl_minutes.unwrap_or(30);
        if !(1..=MAX_TTL_MINUTES).contains(&ttl) {
            return Err(err("VAL_TTL", "ttlMinutes must be between 1 and 120"));
        }
        let idem = valid_idempotency(&input.idempotency_key)?;
        let keys = normalize_keys(input.planned_absent_keys.unwrap_or_default())?;
        let digest = digest_json(
            &json!({"tenantId":tenant,"scenarioName":scenario,"changeIntent":intent,"ttlMinutes":ttl,"plannedAbsentKeys":keys}),
        );
        let key_hash = digest_str(idem);
        let pool = self.pool()?;
        let mut conn = pool.get().map_err(db)?;
        let active: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tenants WHERE id=?1 AND status='active')",
                [&tenant],
                |r| r.get(0),
            )
            .map_err(db)?;
        if !active {
            return Err(err("SIMULATION_NOT_FOUND", "active tenant not found"));
        }
        let now = shanghai_now();
        let expires = now + Duration::minutes(ttl);
        let id = uuid::Uuid::new_v4().to_string();
        // A namespace is the simulation identity, not an independently supplied
        // routing token. This keeps ExecutionMode and DataScope mechanically aligned.
        let session = Session {
            id: id.clone(),
            actor_id: actor.into(),
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
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if let Some(existing) = load_by_create_key(&tx, actor, &key_hash)? {
            if existing.create_request_digest != digest {
                return Err(err(
                    "IDEMPOTENCY_CONFLICT",
                    "idempotency key was used for another request",
                ));
            }
            tx.commit().map_err(db)?;
            return serde_json::to_value(existing).map_err(serialization);
        }
        admit_rate(&tx, actor, "__create__", "create", 600, 5)?;
        if let Err(insert_error) = tx.execute("INSERT INTO simulation_sessions (id,actor_id,target_tenant_id,namespace_id,base_revision,generation,idempotency_key_hash,create_request_digest,scenario_name,change_intent,status,created_at,expires_at,provisioning_lease_until) VALUES (?1,?2,?3,?4,0,1,?5,?6,?7,?8,'provisioning',?9,?10,?11)", params![session.id,session.actor_id,tenant,session.namespace_id,key_hash,digest,session.scenario_name,session.change_intent,session.created_at,session.expires_at,(now+Duration::minutes(5)).to_rfc3339()]) {
            if insert_error.to_string().contains("UNIQUE constraint failed") {
                let existing = load_by_create_key(&tx, actor, &key_hash)?
                    .ok_or_else(|| db(insert_error))?;
                if existing.create_request_digest != digest {
                    return Err(err("IDEMPOTENCY_CONFLICT", "idempotency key was used for another request"));
                }
                tx.commit().map_err(db)?;
                return serde_json::to_value(existing).map_err(serialization);
            }
            return Err(db(insert_error));
        }
        tx.commit().map_err(db)?;
        // Materialization deliberately happens after provisioning is durable; any failure remains observable.
        if let Err(failure) = provision(&mut conn, &session, &keys) {
            let persisted = conn.execute("UPDATE simulation_sessions SET status='failed', failure_code=?2 WHERE id=?1 AND status='provisioning'", params![session.id, code_of(&failure)]);
            match persisted {
                Ok(1) => {}
                Ok(_) => {
                    return Err(err(
                        "SIMULATION_FAILURE_PERSIST_FAILED",
                        "provisioning failed and failed-state persistence changed no row",
                    ));
                }
                Err(e) => {
                    return Err(err(
                        "SIMULATION_FAILURE_PERSIST_FAILED",
                        &format!("provisioning failed and failed-state persistence failed: {e}"),
                    ));
                }
            }
            return Err(failure);
        }
        serde_json::to_value(load_session(&conn, &session.id, actor)?).map_err(serialization)
    }

    fn get(&self, input: IdInput, ctx: &ExecutionContext) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::get(&pool, input, ctx);
        }
        let actor = Self::require_platform(ctx)?;
        let pool = self.pool()?;
        let mut conn = pool.get().map_err(db)?;
        reconcile_lifecycle(&mut conn, required(&input.id, "id")?, actor)?;
        serde_json::to_value(load_session(&conn, required(&input.id, "id")?, actor)?)
            .map_err(serialization)
    }

    fn discard(&self, input: IdInput, ctx: &ExecutionContext) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::discard(&pool, input, ctx);
        }
        let actor = Self::require_platform(ctx)?;
        let pool = self.pool()?;
        let mut conn = pool.get().map_err(db)?;
        let id = required(&input.id, "id")?;
        let now = shanghai_now().to_rfc3339();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        let terminal: Option<(String, i64)> = tx.query_row(
            "SELECT target_tenant_id,generation FROM simulation_sessions WHERE id=?1 AND actor_id=?2 AND status IN ('active','expired')",
            params![id,actor], |r| Ok((r.get(0)?, r.get(1)?)),
        ).optional().map_err(db)?;
        if let Some((tenant, generation)) = terminal {
            materialize_terminal_inputs(&tx, id, &tenant, generation + 1, &now)?;
        }
        let changed = tx.execute("UPDATE simulation_sessions SET status='discarded', generation=generation+1, discarded_at=COALESCE(discarded_at,?3) WHERE id=?1 AND actor_id=?2 AND status IN ('active','expired')", params![id,actor,now]).map_err(db)?;
        if changed == 0 {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM simulation_sessions WHERE id=?1 AND actor_id=?2)",
                    params![id, actor],
                    |r| r.get(0),
                )
                .map_err(db)?;
            if !exists {
                return Err(err("SIMULATION_NOT_FOUND", "simulation not found"));
            }
        }
        tx.execute("INSERT OR IGNORE INTO simulation_cleanup_jobs (simulation_id,tenant_id,job_kind,state,attempts,next_attempt_at,created_at,updated_at) SELECT id,target_tenant_id,'terminal_evidence','pending',0,?2,?2,?2 FROM simulation_sessions WHERE id=?1", params![id,now]).map_err(db)?;
        tx.commit().map_err(db)?;
        serde_json::to_value(load_session(&conn, id, actor)?).map_err(serialization)
    }

    fn list(&self, input: PageInput, ctx: &ExecutionContext) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::list(&pool, input, ctx);
        }
        let pool = self.pool()?;
        let conn = pool.get().map_err(db)?;
        let session = self.simulation_capability(&conn, &input.id, ctx)?;
        let limit = page_limit(input.limit)?;
        let cursor = input.cursor.unwrap_or_default();
        let mut stmt=conn.prepare("SELECT resource_key,operation,document_json FROM simulation_overlay_entries WHERE simulation_id=?1 AND tenant_id=?2 AND resource_kind='reference_config' AND resource_key>?3 UNION ALL SELECT b.resource_key,'base',b.document_json FROM simulation_base_documents b WHERE b.simulation_id=?1 AND b.tenant_id=?2 AND b.resource_kind='reference_config' AND b.resource_key>?3 AND NOT EXISTS(SELECT 1 FROM simulation_overlay_entries o WHERE o.simulation_id=b.simulation_id AND o.tenant_id=b.tenant_id AND o.resource_kind=b.resource_kind AND o.resource_key=b.resource_key) ORDER BY 1 LIMIT ?4").map_err(db)?;
        let rows = stmt
            .query_map(params![session.id, session.tenant_id, cursor, limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(db)?;
        let mut items = Vec::new();
        let mut next = None;
        for row in rows {
            let (key, op, doc) = row.map_err(db)?;
            next = Some(key.clone());
            if op != "tombstone" {
                let doc =
                    doc.ok_or_else(|| err("SYS_DB", "visible simulation document is missing"))?;
                items.push(json!({"key":key,"value":serde_json::from_str::<Value>(&doc).map_err(serialization)?}));
            }
        }
        Ok(json!({"items":items,"nextCursor":next}))
    }

    fn get_reference(&self, input: KeyInput, ctx: &ExecutionContext) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::get_reference(&pool, input, ctx);
        }
        let pool = self.pool()?;
        let conn = pool.get().map_err(db)?;
        let session = self.simulation_capability(&conn, &input.id, ctx)?;
        let key = valid_key(&input.key)?;
        let overlay:Option<(String,Option<String>)>=conn.query_row("SELECT operation,document_json FROM simulation_overlay_entries WHERE simulation_id=?1 AND tenant_id=?2 AND resource_kind=?3 AND resource_key=?4",params![session.id,session.tenant_id,KIND,key],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?;
        match overlay {
            Some((op, _)) if op == "tombstone" => {
                Err(err("SIMULATION_NOT_FOUND", "reference config not found"))
            }
            Some((_, Some(doc))) => Ok(
                json!({"key":key,"value":serde_json::from_str::<Value>(&doc).map_err(serialization)?}),
            ),
            _ => {
                let doc:Option<String>=conn.query_row("SELECT document_json FROM simulation_base_documents WHERE simulation_id=?1 AND tenant_id=?2 AND resource_kind=?3 AND resource_key=?4",params![session.id,session.tenant_id,KIND,key],|r|r.get(0)).optional().map_err(db)?;
                doc.map(|d|Ok(json!({"key":key,"value":serde_json::from_str::<Value>(&d).map_err(serialization)?}))).unwrap_or_else(||Err(err("SIMULATION_NOT_FOUND","reference config not found")))
            }
        }
    }

    fn mutate(
        &self,
        input: MutationInput,
        delete: bool,
        effect: Option<EffectInput>,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::mutate(&pool, input, delete, effect, ctx);
        }
        let pool = self.pool()?;
        let mut conn = pool.get().map_err(db)?;
        let session = self.simulation_capability(&conn, &input.id, ctx)?;
        let actor = ctx.actor().id().unwrap();
        let key = valid_key(&input.key)?.to_string();
        let idem = valid_idempotency(&input.idempotency_key)?;
        let command = if effect.is_some() {
            "reference_config.record_effect"
        } else if delete {
            "reference_config.delete"
        } else {
            "reference_config.put"
        };
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
        let key_hash = digest_str(idem);
        let now = shanghai_now().to_rfc3339();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if let Some((prior_digest,response))=tx.query_row("SELECT request_digest,response_json FROM simulation_command_results WHERE simulation_id=?1 AND actor_id=?2 AND command=?3 AND command_idempotency_key_hash=?4",params![session.id,actor,command,key_hash],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional().map_err(db)? {if prior_digest!=request{return Err(err("IDEMPOTENCY_CONFLICT","idempotency key was used for another request"));}return serde_json::from_str(&response).map_err(serialization);}
        admit_rate(&tx, actor, &session.id, "data", 60, 120)?;
        ensure_active_generation(&tx, &session)?;
        sync_pricing_revision(&tx, &session.tenant_id)?;
        let base:(i64,i64)=tx.query_row("SELECT base_presence,captured_resource_version FROM simulation_revision_evidence WHERE simulation_id=?1 AND tenant_id=?2 AND resource_kind=?3 AND resource_key=?4",params![session.id,session.tenant_id,KIND,key],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?.ok_or_else(||err("UNPROVISIONED_RESOURCE_KEY","reference key was not provisioned"))?;
        let old:Option<(String,Option<String>)>=tx.query_row("SELECT operation,document_json FROM simulation_overlay_entries WHERE simulation_id=?1 AND tenant_id=?2 AND resource_kind=?3 AND resource_key=?4",params![session.id,session.tenant_id,KIND,key],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?;
        if delete && base.0 == 0 && old.is_none() {
            let response = json!({"key":key,"deleted":false});
            store_result(
                &tx, &session, actor, command, &key_hash, &request, &response, &now,
            )?;
            tx.commit().map_err(db)?;
            return Ok(response);
        }
        if delete && base.0 == 0 {
            tx.execute("DELETE FROM simulation_overlay_entries WHERE simulation_id=?1 AND tenant_id=?2 AND resource_kind=?3 AND resource_key=?4",params![session.id,session.tenant_id,KIND,key]).map_err(db)?;
        } else {
            let doc = value.as_ref().map(canonical_json);
            tx.execute("INSERT INTO simulation_overlay_entries (simulation_id,tenant_id,resource_kind,resource_key,operation,document_json,base_resource_version,overlay_version,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,?8) ON CONFLICT(simulation_id,tenant_id,resource_kind,resource_key) DO UPDATE SET operation=excluded.operation,document_json=excluded.document_json,overlay_version=simulation_overlay_entries.overlay_version+1,updated_at=excluded.updated_at",params![session.id,session.tenant_id,KIND,key,if delete{"tombstone"}else{"value"},doc,base.1,now]).map_err(db)?;
        }
        refresh_usage(&tx, &session)?;
        let response = json!({"key":key,"deleted":delete,"value":value});
        if let Some(effect) = effect {
            let note = plain(&effect.note, "effect.note", 200)?;
            if effect.kind != "reference" {
                return Err(err("VAL_EFFECT", "unsupported proof effect"));
            }
            let admitted: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM simulation_usage WHERE simulation_id=?1 AND tenant_id=?2 AND effect_count < 1000)",
                params![session.id,session.tenant_id], |r| r.get(0),
            ).map_err(db)?;
            if !admitted {
                return Err(err("SIMULATION_QUOTA_EXCEEDED", "effect quota exceeded"));
            }
            tx.execute("INSERT INTO simulation_effect_records (id,simulation_id,tenant_id,actor_id,command,command_idempotency_key_hash,request_digest,effect_ordinal,effect_kind,payload_policy,payload_preview,deterministic_result,correlation_id,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,0,'ReferenceEffect','REFERENCE_ONLY',?8,?9,?10,?11)",params![uuid::Uuid::new_v4().to_string(),session.id,session.tenant_id,actor,command,key_hash,request,note,canonical_json(&json!({"recorded":true,"kind":"reference"})),ctx.correlation_id().as_str(),now]).map_err(db)?;
            tx.execute("UPDATE simulation_usage SET effect_count=effect_count+1 WHERE simulation_id=?1 AND tenant_id=?2",params![session.id,session.tenant_id]).map_err(db)?;
        }
        store_result(
            &tx, &session, actor, command, &key_hash, &request, &response, &now,
        )?;
        tx.commit().map_err(db)?;
        Ok(response)
    }

    fn get_pricing(&self, input: IdInput, ctx: &ExecutionContext) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::get_pricing(&pool, input, ctx);
        }
        let pool = self.pool()?;
        let conn = pool.get().map_err(db)?;
        let session = self.simulation_capability(&conn, &input.id, ctx)?;
        pricing_visible_document(&conn, &session)
    }

    fn put_pricing(
        &self,
        input: PricingMutationInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::put_pricing(&pool, input, ctx);
        }
        let pool = self.pool()?;
        let mut conn = pool.get().map_err(db)?;
        let session = self.simulation_capability(&conn, &input.id, ctx)?;
        let actor = ctx
            .actor()
            .id()
            .ok_or_else(|| err("AUTH_ACTOR_REQUIRED", "actor id required"))?;
        let value = valid_pricing_config(input.value)?;
        let idem = valid_idempotency(&input.idempotency_key)?;
        let request = digest_json(&json!({"value": value}));
        let key_hash = digest_str(idem);
        let now = shanghai_now().to_rfc3339();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        if let Some((prior_digest,response)) = tx.query_row("SELECT request_digest,response_json FROM simulation_command_results WHERE simulation_id=?1 AND actor_id=?2 AND command='pricing_config.put' AND command_idempotency_key_hash=?3",params![session.id,actor,key_hash],|r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional().map_err(db)? {
            if prior_digest != request { return Err(err("IDEMPOTENCY_CONFLICT", "idempotency key was used for another request")); }
            return serde_json::from_str(&response).map_err(serialization);
        }
        admit_rate(&tx, actor, &session.id, "data", 60, 120)?;
        ensure_active_generation(&tx, &session)?;
        sync_pricing_revision(&tx, &session.tenant_id)?;
        let base: (i64, i64) = tx.query_row("SELECT base_presence,captured_resource_version FROM simulation_revision_evidence WHERE simulation_id=?1 AND tenant_id=?2 AND resource_kind=?3 AND resource_key=?4",params![session.id,session.tenant_id,PRICING_KIND,PRICING_KEY],|r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(db)?.ok_or_else(|| err("SYS_MANIFEST", "pricing config was not provisioned"))?;
        tx.execute("INSERT INTO simulation_overlay_entries (simulation_id,tenant_id,resource_kind,resource_key,operation,document_json,base_resource_version,overlay_version,created_at,updated_at) VALUES (?1,?2,?3,?4,'value',?5,?6,1,?7,?7) ON CONFLICT(simulation_id,tenant_id,resource_kind,resource_key) DO UPDATE SET operation='value',document_json=excluded.document_json,overlay_version=simulation_overlay_entries.overlay_version+1,updated_at=excluded.updated_at",params![session.id,session.tenant_id,PRICING_KIND,PRICING_KEY,canonical_json(&value),base.1,now]).map_err(db)?;
        refresh_usage(&tx, &session)?;
        let response = json!({"key": PRICING_KEY, "value": value});
        store_result(
            &tx,
            &session,
            actor,
            "pricing_config.put",
            &key_hash,
            &request,
            &response,
            &now,
        )?;
        tx.commit().map_err(db)?;
        Ok(response)
    }

    fn estimate_pricing(
        &self,
        input: PricingEstimateInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::estimate_pricing(&pool, input, ctx);
        }
        let pool = self.pool()?;
        let mut conn = pool.get().map_err(db)?;
        let session = self.simulation_capability(&conn, &input.id, ctx)?;
        {
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(db)?;
            ensure_active_generation(&tx, &session)?;
            admit_rate(
                &tx,
                ctx.actor().id().unwrap_or_default(),
                &session.id,
                "estimate",
                60,
                60,
            )?;
            tx.commit().map_err(db)?;
        }
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
        let config = pricing_visible_document(&conn, &session)?["value"].clone();
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
                let start = rule.get("startDate").and_then(Value::as_str);
                let end = rule.get("endDate").and_then(Value::as_str);
                let include_previous = rule
                    .get("includePreviousDay")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let previous = start
                    .and_then(|start| chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d").ok())
                    .and_then(|start| start.pred_opt())
                    .map(|day| day.format("%Y-%m-%d").to_string());
                if (start
                    .zip(end)
                    .is_some_and(|(start, end)| start <= date.as_str() && date.as_str() <= end)
                    || (include_previous && previous.as_deref() == Some(date.as_str())))
                    && rule.get("price").and_then(Value::as_f64).is_some()
                {
                    price = rule.get("price").and_then(Value::as_f64).unwrap();
                    source = "holiday";
                    break;
                }
            }
            if let Some(v) = dynamic
                .and_then(|map| map.get(&date))
                .and_then(Value::as_f64)
            {
                price = v;
                source = "dynamic";
            }
            total += price;
            days.push(json!({"date":date,"price":price,"source":source}));
            day = day
                .succ_opt()
                .ok_or_else(|| err("VAL_DATE_RANGE", "invalid date"))?;
        }
        Ok(
            json!({"startDate":input.start_date,"endDate":input.end_date,"days":days,"total":total,"currency":"CNY"}),
        )
    }

    fn diff(&self, input: IdInput, ctx: &ExecutionContext) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::diff(&pool, input, ctx);
        }
        let pool = self.pool()?;
        let mut conn = pool.get().map_err(db)?;
        let session = self.simulation_capability(&conn, &input.id, ctx)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db)?;
        ensure_active_generation(&tx, &session)?;
        sync_pricing_revision(&tx, &session.tenant_id)?;
        admit_rate(
            &tx,
            ctx.actor().id().unwrap_or_default(),
            &session.id,
            "diff",
            60,
            10,
        )?;
        let current: i64 = tx
            .query_row(
                "SELECT revision FROM simulation_tenant_revisions WHERE tenant_id=?1",
                [&session.tenant_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?
            .unwrap_or(0);
        let mut items = Vec::new();
        {
            let mut stmt=tx.prepare("SELECT e.resource_key,e.base_presence,e.captured_resource_version,o.operation,o.document_json,r.present,r.resource_version FROM simulation_revision_evidence e LEFT JOIN simulation_overlay_entries o ON o.simulation_id=e.simulation_id AND o.tenant_id=e.tenant_id AND o.resource_kind=e.resource_kind AND o.resource_key=e.resource_key LEFT JOIN simulation_reference_resource_revisions r ON r.tenant_id=e.tenant_id AND r.key=e.resource_key WHERE e.simulation_id=?1 AND e.tenant_id=?2 AND e.resource_kind='reference_config' ORDER BY e.resource_key").map_err(db)?;
            let rows = stmt
                .query_map(params![session.id, session.tenant_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                        r.get::<_, Option<i64>>(6)?,
                    ))
                })
                .map_err(db)?;
            for row in rows {
                let (
                    key,
                    base_present,
                    base_version,
                    operation,
                    _doc,
                    current_present,
                    current_version,
                ) = row.map_err(db)?;
                let changed = operation.is_some();
                let equal = current_present.unwrap_or(0) == base_present
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
                items.push(json!({"resourceKind":KIND,"resourceKey":key,"operation":operation.unwrap_or_else(||"none".into()),"conflictStatus":status,"baseVersion":base_version.to_string(),"currentVersion":current_version.unwrap_or(0).to_string(),"before":{"policy":"REFERENCE_ONLY"},"after":{"policy":"REFERENCE_ONLY"}}));
            }
        }
        let pricing: (i64, i64, Option<String>, Option<i64>) = tx.query_row(
            "SELECT e.base_presence,e.captured_resource_version,o.operation,r.resource_version FROM simulation_revision_evidence e LEFT JOIN simulation_overlay_entries o ON o.simulation_id=e.simulation_id AND o.tenant_id=e.tenant_id AND o.resource_kind=e.resource_kind AND o.resource_key=e.resource_key LEFT JOIN simulation_pricing_resource_revisions r ON r.tenant_id=e.tenant_id AND r.resource_key=e.resource_key WHERE e.simulation_id=?1 AND e.tenant_id=?2 AND e.resource_kind=?3 AND e.resource_key=?4",
            params![session.id,session.tenant_id,PRICING_KIND,PRICING_KEY], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).map_err(db)?;
        let pricing_changed = pricing.2.is_some();
        let pricing_equal = pricing.3.unwrap_or(0) == pricing.1;
        if pricing_changed || !pricing_equal {
            items.push(json!({"resourceKind":PRICING_KIND,"resourceKey":PRICING_KEY,"operation":pricing.2.unwrap_or_else(||"none".into()),"conflictStatus":if pricing_changed && pricing_equal {"clean"} else if pricing_changed {"conflict"} else {"stale"},"baseVersion":pricing.1.to_string(),"currentVersion":pricing.3.unwrap_or(0).to_string(),"before":{"policy":"REFERENCE_ONLY"},"after":{"policy":"REFERENCE_ONLY"}}));
        }
        let canonical = canonical_json(&Value::Array(items.clone()));
        let overlay_digest = digest_str(&canonical);
        if let Some(existing) = tx.query_row(
            "SELECT evaluation_id FROM simulation_diff_evidence WHERE simulation_id=?1 AND generation=?2 AND evaluated_revision=?3 AND overlay_digest=?4",
            params![session.id,session.generation,current,overlay_digest], |r| r.get::<_, String>(0),
        ).optional().map_err(db)? {
            tx.commit().map_err(db)?;
            return Ok(json!({"evaluationId":existing,"items":items,"nextCursor":Value::Null}));
        }
        let evidence_bytes = canonical.len() as i64;
        let admitted: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM simulation_usage WHERE simulation_id=?1 AND tenant_id=?2 AND evaluation_count < 20 AND evidence_bytes + ?3 <= 26214400)",
            params![session.id,session.tenant_id,evidence_bytes], |r| r.get(0),
        ).map_err(db)?;
        if !admitted {
            return Err(err(
                "SIMULATION_QUOTA_EXCEEDED",
                "diff evidence quota exceeded",
            ));
        }
        let evaluation = uuid::Uuid::new_v4().to_string();
        tx.execute("INSERT INTO simulation_diff_evidence (simulation_id,tenant_id,evaluation_id,evaluated_revision,generation,overlay_digest,item_count,total_bytes,digest,status,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'ready',?10)",params![session.id,session.tenant_id,evaluation,current,session.generation,overlay_digest,items.len() as i64,canonical.len() as i64,digest_str(&canonical),shanghai_now().to_rfc3339()]).map_err(db)?;
        for (ordinal, item) in items.iter().enumerate() {
            let item = canonical_json(item);
            tx.execute("INSERT INTO simulation_diff_evidence_items (simulation_id,tenant_id,evaluation_id,ordinal,canonical_item_json,item_bytes) VALUES (?1,?2,?3,?4,?5,?6)",params![session.id,session.tenant_id,evaluation,ordinal as i64,item,item.len() as i64]).map_err(db)?;
        }
        tx.execute("UPDATE simulation_usage SET evaluation_count=evaluation_count+1,evidence_bytes=evidence_bytes+?3 WHERE simulation_id=?1 AND tenant_id=?2",params![session.id,session.tenant_id,evidence_bytes]).map_err(db)?;
        tx.commit().map_err(db)?;
        Ok(json!({"evaluationId":evaluation,"items":items,"nextCursor":Value::Null}))
    }

    fn diff_get(&self, input: EvidencePageInput, ctx: &ExecutionContext) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::diff_get(&pool, input, ctx);
        }
        let pool = self.pool()?;
        let conn = pool.get().map_err(db)?;
        let session = self.evidence_capability(&conn, &input.id, ctx)?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM simulation_diff_evidence WHERE simulation_id=?1 AND tenant_id=?2 AND evaluation_id=?3 AND status='ready')",
            params![session.id,session.tenant_id,input.evaluation_id], |r| r.get(0),
        ).map_err(db)?;
        if !exists {
            return Err(err(
                "DIFF_EVALUATION_NOT_FOUND",
                "diff evaluation not found",
            ));
        }
        let limit = page_limit(input.limit)? as i64;
        let cursor = input.cursor.unwrap_or(-1);
        let mut stmt = conn.prepare("SELECT ordinal,canonical_item_json FROM simulation_diff_evidence_items WHERE simulation_id=?1 AND tenant_id=?2 AND evaluation_id=?3 AND ordinal>?4 ORDER BY ordinal LIMIT ?5").map_err(db)?;
        let rows = stmt
            .query_map(
                params![
                    session.id,
                    session.tenant_id,
                    input.evaluation_id,
                    cursor,
                    limit
                ],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .map_err(db)?;
        let mut items = Vec::new();
        let mut last = None;
        for row in rows {
            let (ordinal, item) = row.map_err(db)?;
            last = Some(ordinal);
            items.push(serde_json::from_str::<Value>(&item).map_err(serialization)?);
        }
        Ok(json!({"evaluationId": input.evaluation_id, "items":items, "nextCursor":last}))
    }

    fn effects_list(&self, input: PageInput, ctx: &ExecutionContext) -> Result<Value, String> {
        #[cfg(feature = "postgres")]
        if let Some(pool) = self.postgres_pool()? {
            return postgres::effects_list(&pool, input, ctx);
        }
        let pool = self.pool()?;
        let conn = pool.get().map_err(db)?;
        let session = self.evidence_capability(&conn, &input.id, ctx)?;
        let limit = page_limit(input.limit)? as i64;
        let cursor = input.cursor.unwrap_or_default();
        let mut stmt = conn.prepare("SELECT id,command,effect_kind,payload_policy,payload_preview,deterministic_result,created_at FROM simulation_effect_records WHERE simulation_id=?1 AND tenant_id=?2 AND id>?3 ORDER BY id LIMIT ?4").map_err(db)?;
        let rows = stmt
            .query_map(params![session.id, session.tenant_id, cursor, limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })
            .map_err(db)?;
        let mut items = Vec::new();
        for row in rows {
            let (id, command, kind, policy, preview, result, created) = row.map_err(db)?;
            items.push(json!({"id":id,"command":command,"kind":kind,"payloadPolicy":policy,"payloadPreview":preview,"result":serde_json::from_str::<Value>(&result).map_err(serialization)?,"createdAt":created}));
        }
        let next = items
            .last()
            .and_then(|v| v["id"].as_str())
            .map(str::to_owned);
        Ok(json!({"items":items,"nextCursor":next}))
    }
}

impl Default for FeatureTenantSimulation {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureTenantSimulation {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "feature-tenant-simulation".into(),
            version: "0.1.0".into(),
            description: "Isolated tenant simulation overlay runtime".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }
    fn init(&mut self, config: Value) -> Result<(), String> {
        let mut backend = self
            .backend
            .lock()
            .map_err(|_| err("SYS_LOCK", "simulation backend lock failed"))?;
        match &*backend {
            SimulationBackend::Sqlite(_) => return Ok(()),
            #[cfg(feature = "postgres")]
            SimulationBackend::Postgres(_) => return Ok(()),
            SimulationBackend::Unconfigured => {}
        }
        if let Some(path) = config.get("databaseUrl").and_then(Value::as_str) {
            let pool = Pool::builder()
                .max_size(5)
                .build(SqliteConnectionManager::file(path))
                .map_err(|e| err("SYS_DB_POOL", &e.to_string()))?;
            *backend = SimulationBackend::Sqlite(pool);
        }
        Ok(())
    }
    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "session.create",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "session.get",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "session.discard",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "reference_config.list",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "reference_config.get",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "reference_config.put",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "reference_config.delete",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "reference_config.record_effect",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite, EffectClass::ExternalHttp],
                SimulationSupport::SupportedWithStub,
            ),
            CommandMetadata::new(
                "pricing_config.get",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "pricing_config.put",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "pricing_config.estimate",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "diff.evaluate",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead, EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "diff.get",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "effects.list",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }
    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "session.create" => self.create(parse(payload)?, ctx),
            "session.get" => self.get(parse(payload)?, ctx),
            "session.discard" => self.discard(parse(payload)?, ctx),
            "reference_config.list" => self.list(parse(payload)?, ctx),
            "reference_config.get" => self.get_reference(parse(payload)?, ctx),
            "reference_config.put" => self.mutate(parse(payload)?, false, None, ctx),
            "reference_config.delete" => self.mutate(parse(payload)?, true, None, ctx),
            "reference_config.record_effect" => {
                let input: EffectMutationInput = parse(payload)?;
                self.mutate(
                    MutationInput {
                        id: input.id,
                        key: input.key,
                        value: Some(input.value),
                        idempotency_key: input.idempotency_key,
                    },
                    false,
                    Some(input.effect),
                    ctx,
                )
            }
            "pricing_config.get" => self.get_pricing(parse(payload)?, ctx),
            "pricing_config.put" => self.put_pricing(parse(payload)?, ctx),
            "pricing_config.estimate" => self.estimate_pricing(parse(payload)?, ctx),
            "diff.evaluate" => self.diff(parse(payload)?, ctx),
            "diff.get" => self.diff_get(parse(payload)?, ctx),
            "effects.list" => self.effects_list(parse(payload)?, ctx),
            _ => Err(err(
                "SYS_UNKNOWN_COMMAND",
                "unknown tenant simulation command",
            )),
        }
    }
    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "feature-tenant-simulation".into(),
            description: "Isolated tenant simulation overlay runtime".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|m| CommandSchema {
                    name: m.name.into(),
                    description: "tenant simulation command".into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateInput {
    tenant_id: String,
    scenario_name: String,
    change_intent: String,
    ttl_minutes: Option<i64>,
    planned_absent_keys: Option<Vec<String>>,
    idempotency_key: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdInput {
    id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PageInput {
    id: String,
    cursor: Option<String>,
    limit: Option<u32>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EvidencePageInput {
    id: String,
    evaluation_id: String,
    cursor: Option<i64>,
    limit: Option<u32>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyInput {
    id: String,
    key: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MutationInput {
    id: String,
    key: String,
    value: Option<Value>,
    idempotency_key: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PricingMutationInput {
    id: String,
    value: Value,
    idempotency_key: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PricingEstimateInput {
    id: String,
    start_date: String,
    end_date: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EffectInput {
    kind: String,
    note: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EffectMutationInput {
    id: String,
    key: String,
    value: Value,
    effect: EffectInput,
    idempotency_key: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Session {
    id: String,
    #[serde(skip_serializing)]
    actor_id: String,
    #[serde(rename = "tenantId")]
    tenant_id: String,
    namespace_id: String,
    base_revision: i64,
    generation: i64,
    scenario_name: String,
    change_intent: String,
    status: String,
    created_at: String,
    expires_at: String,
    discarded_at: Option<String>,
    failure_code: Option<String>,
    #[serde(skip_serializing)]
    create_request_digest: String,
}

fn provision(conn: &mut Connection, session: &Session, keys: &[String]) -> Result<(), String> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let now = shanghai_now().to_rfc3339();
    // Pricing is a first-class, single-document coverage unit in MVP4.  Its
    // source revision is derived from the canonical production document, so
    // normal pricing writes do not need a simulation-specific branch.
    sync_pricing_revision(&tx, &session.tenant_id)?;
    let revision: i64 = tx
        .query_row(
            "SELECT revision FROM simulation_tenant_revisions WHERE tenant_id=?1",
            [&session.tenant_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?
        .unwrap_or(0);
    let (source_rows, source_bytes): (i64, i64) = tx.query_row(
        "SELECT COUNT(*),COALESCE(SUM(length(COALESCE(c.value_json,''))),0) FROM simulation_reference_resource_revisions r LEFT JOIN simulation_reference_configs c ON c.tenant_id=r.tenant_id AND c.key=r.key WHERE r.tenant_id=?1",
        [&session.tenant_id], |r| Ok((r.get(0)?,r.get(1)?)),
    ).map_err(db)?;
    let pricing_bytes: i64 = tx.query_row(
        "SELECT length(document_json) FROM simulation_pricing_resource_revisions WHERE tenant_id=?1 AND resource_key=?2",
        params![session.tenant_id, PRICING_KEY], |r| r.get(0),
    ).map_err(db)?;
    let mut missing_planned = 0_i64;
    for key in keys {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM simulation_reference_resource_revisions WHERE tenant_id=?1 AND key=?2)",
            params![session.tenant_id,key], |r| r.get(0),
        ).map_err(db)?;
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
    let source: Vec<(String, i64, i64, Option<String>)> = {
        let mut stmt=tx.prepare("SELECT r.key,r.present,r.resource_version,c.value_json FROM simulation_reference_resource_revisions r LEFT JOIN simulation_reference_configs c ON c.tenant_id=r.tenant_id AND c.key=r.key WHERE r.tenant_id=?1").map_err(db)?;
        stmt.query_map([&session.tenant_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(db)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db)?
    };
    for (key, present, version, doc) in source {
        insert_manifest(
            &tx,
            session,
            &key,
            present,
            version,
            doc.as_deref(),
            &now,
            KIND,
        )?;
    }
    let pricing: (i64, i64, Option<String>) = tx.query_row(
        "SELECT present,resource_version,document_json FROM simulation_pricing_resource_revisions WHERE tenant_id=?1 AND resource_key=?2",
        params![session.tenant_id, PRICING_KEY], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).map_err(db)?;
    insert_manifest(
        &tx,
        session,
        PRICING_KEY,
        pricing.0,
        pricing.1,
        pricing.2.as_deref(),
        &now,
        PRICING_KIND,
    )?;
    for key in keys {
        let existing: Option<(i64, i64)> = tx.query_row(
            "SELECT present,resource_version FROM simulation_reference_resource_revisions WHERE tenant_id=?1 AND key=?2",
            params![session.tenant_id,key], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional().map_err(db)?;
        if matches!(existing, Some((1, _))) {
            return Err(err("VAL_PLANNED_KEY", "planned absent key already exists"));
        }
        // A durable absent revision is already in `source` and therefore in the
        // manifest. Do not overwrite it with a synthetic version-zero entry.
        if existing.is_none() {
            insert_manifest(&tx, session, key, 0, 0, None, &now, KIND)?;
        }
    }
    tx.execute(
        "INSERT INTO simulation_usage (simulation_id,tenant_id) VALUES (?1,?2)",
        params![session.id, session.tenant_id],
    )
    .map_err(db)?;
    tx.execute("UPDATE simulation_sessions SET base_revision=?2,status='active',provisioning_lease_until=NULL WHERE id=?1 AND status='provisioning'",params![session.id,revision]).map_err(db)?;
    tx.commit().map_err(db)?;
    Ok(())
}

/// Freeze all inputs required to reproduce terminal evidence before the session
/// is made terminal. Cleanup must never be able to delete a live-only base.
fn materialize_terminal_inputs(
    tx: &rusqlite::Transaction<'_>,
    simulation_id: &str,
    tenant_id: &str,
    generation: i64,
    now: &str,
) -> Result<(), String> {
    sync_pricing_revision(tx, tenant_id)?;
    let revision: i64 = tx
        .query_row(
            "SELECT revision FROM simulation_tenant_revisions WHERE tenant_id=?1",
            [tenant_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?
        .unwrap_or(0);
    let (candidate_rows, candidate_bytes): (i64, i64) = tx.query_row(
        "SELECT COUNT(*),COALESCE(SUM(length(COALESCE(o.document_json,''))+length(COALESCE(c.value_json,''))),0) FROM simulation_revision_evidence e LEFT JOIN simulation_overlay_entries o ON o.simulation_id=e.simulation_id AND o.tenant_id=e.tenant_id AND o.resource_kind=e.resource_kind AND o.resource_key=e.resource_key LEFT JOIN simulation_reference_configs c ON c.tenant_id=e.tenant_id AND c.key=e.resource_key WHERE e.simulation_id=?1 AND e.tenant_id=?2 AND e.resource_kind='reference_config'",
        params![simulation_id,tenant_id], |r| Ok((r.get(0)?,r.get(1)?)),
    ).map_err(db)?;
    let pricing_candidate_bytes: i64 = tx.query_row(
        "SELECT length(COALESCE(b.document_json,''))+length(COALESCE(o.document_json,''))+length(COALESCE(r.document_json,'')) FROM simulation_revision_evidence e LEFT JOIN simulation_base_documents b ON b.simulation_id=e.simulation_id AND b.tenant_id=e.tenant_id AND b.resource_kind=e.resource_kind AND b.resource_key=e.resource_key LEFT JOIN simulation_overlay_entries o ON o.simulation_id=e.simulation_id AND o.tenant_id=e.tenant_id AND o.resource_kind=e.resource_kind AND o.resource_key=e.resource_key LEFT JOIN simulation_pricing_resource_revisions r ON r.tenant_id=e.tenant_id AND r.resource_key=e.resource_key WHERE e.simulation_id=?1 AND e.tenant_id=?2 AND e.resource_kind=?3 AND e.resource_key=?4",
        params![simulation_id,tenant_id,PRICING_KIND,PRICING_KEY], |r| r.get(0),
    ).map_err(db)?;
    if candidate_rows + 1 > MAX_PROOF_ROWS
        || candidate_bytes + pricing_candidate_bytes > MAX_PROOF_BYTES
    {
        return Err(err(
            "SIMULATION_QUOTA_EXCEEDED",
            "terminal evidence input exceeds quota",
        ));
    }
    let mut values: Vec<String> = {
        let mut stmt = tx.prepare("SELECT e.resource_key,e.base_presence,e.captured_resource_version,o.operation,o.document_json,r.present,r.resource_version,c.value_json FROM simulation_revision_evidence e LEFT JOIN simulation_overlay_entries o ON o.simulation_id=e.simulation_id AND o.tenant_id=e.tenant_id AND o.resource_kind=e.resource_kind AND o.resource_key=e.resource_key LEFT JOIN simulation_reference_resource_revisions r ON r.tenant_id=e.tenant_id AND r.key=e.resource_key LEFT JOIN simulation_reference_configs c ON c.tenant_id=e.tenant_id AND c.key=e.resource_key WHERE e.simulation_id=?1 AND e.tenant_id=?2 AND e.resource_kind='reference_config' ORDER BY e.resource_key").map_err(db)?;
        stmt.query_map(params![simulation_id,tenant_id], |r| {
            let key:String=r.get(0)?;let base_presence:i64=r.get(1)?;let base_version:i64=r.get(2)?;let operation:Option<String>=r.get(3)?;let overlay:Option<String>=r.get(4)?;let current_presence:Option<i64>=r.get(5)?;let current_version:Option<i64>=r.get(6)?;let current_doc:Option<String>=r.get(7)?;
            Ok(canonical_json(&json!({"resourceKind":KIND,"resourceKey":key,"basePresence":base_presence,"baseVersion":base_version,"overlayOperation":operation,"overlay":overlay.and_then(|v|serde_json::from_str::<Value>(&v).ok()),"currentPresence":current_presence.unwrap_or(0),"currentVersion":current_version.unwrap_or(0),"current":current_doc.and_then(|v|serde_json::from_str::<Value>(&v).ok())})))
        }).map_err(db)?.collect::<Result<Vec<_>,_>>().map_err(db)?
    };
    let pricing_value: String = tx.query_row(
        "SELECT e.base_presence,e.captured_resource_version,b.document_json,o.operation,o.document_json,r.present,r.resource_version,r.document_json FROM simulation_revision_evidence e LEFT JOIN simulation_base_documents b ON b.simulation_id=e.simulation_id AND b.tenant_id=e.tenant_id AND b.resource_kind=e.resource_kind AND b.resource_key=e.resource_key LEFT JOIN simulation_overlay_entries o ON o.simulation_id=e.simulation_id AND o.tenant_id=e.tenant_id AND o.resource_kind=e.resource_kind AND o.resource_key=e.resource_key LEFT JOIN simulation_pricing_resource_revisions r ON r.tenant_id=e.tenant_id AND r.resource_key=e.resource_key WHERE e.simulation_id=?1 AND e.tenant_id=?2 AND e.resource_kind=?3 AND e.resource_key=?4",
        params![simulation_id,tenant_id,PRICING_KIND,PRICING_KEY], |r| {
            let base_presence:i64=r.get(0)?; let base_version:i64=r.get(1)?; let base_doc:String=r.get(2)?; let operation:Option<String>=r.get(3)?; let overlay:Option<String>=r.get(4)?; let current_presence:i64=r.get(5)?; let current_version:i64=r.get(6)?; let current_doc:String=r.get(7)?;
            Ok(canonical_json(&json!({"resourceKind":PRICING_KIND,"resourceKey":PRICING_KEY,"basePresence":base_presence,"baseVersion":base_version,"base":serde_json::from_str::<Value>(&base_doc).ok(),"overlayOperation":operation,"overlay":overlay.and_then(|v|serde_json::from_str::<Value>(&v).ok()),"currentPresence":current_presence,"currentVersion":current_version,"current":serde_json::from_str::<Value>(&current_doc).ok()})))
        },
    ).map_err(db)?;
    values.push(pricing_value);
    let canonical_bytes = values.iter().map(|value| value.len() as i64).sum::<i64>();
    if values.len() as i64 > MAX_PROOF_ROWS || canonical_bytes > MAX_PROOF_BYTES {
        return Err(err(
            "SIMULATION_QUOTA_EXCEEDED",
            "canonical terminal evidence exceeds quota",
        ));
    }
    let digest = digest_str(&values.join("\n"));
    tx.execute("INSERT INTO simulation_terminal_inputs (simulation_id,tenant_id,generation,evaluated_revision,overlay_digest,item_count,input_bytes,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![simulation_id,tenant_id,generation,revision,digest,values.len() as i64,canonical_bytes,now]).map_err(db)?;
    for (ordinal, value) in values.iter().enumerate() {
        tx.execute("INSERT INTO simulation_terminal_input_items (simulation_id,tenant_id,generation,ordinal,canonical_input_json,item_bytes) VALUES (?1,?2,?3,?4,?5,?6)",params![simulation_id,tenant_id,generation,ordinal as i64,value,value.len() as i64]).map_err(db)?;
    }
    Ok(())
}

/// Request-time lifecycle reconciliation provides the same safety boundary as
/// the periodic worker: expiration becomes terminal before a caller observes
/// the session, and an abandoned provisioner cannot remain executable.
fn reconcile_lifecycle(conn: &mut Connection, id: &str, actor: &str) -> Result<(), String> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    let row: Option<(String, String, i64, Option<String>)> = tx.query_row(
        "SELECT status,target_tenant_id,generation,provisioning_lease_until FROM simulation_sessions WHERE id=?1 AND actor_id=?2",
        params![id,actor], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
    ).optional().map_err(db)?;
    let Some((status, tenant, generation, lease)) = row else {
        return Err(err("SIMULATION_NOT_FOUND", "simulation not found"));
    };
    let now = shanghai_now().to_rfc3339();
    if status == "active" {
        let expired: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM simulation_sessions WHERE id=?1 AND actor_id=?2 AND status='active' AND julianday(expires_at)<=julianday(?3))",
            params![id,actor,now], |r| r.get(0),
        ).map_err(db)?;
        if expired {
            materialize_terminal_inputs(&tx, id, &tenant, generation + 1, &now)?;
            tx.execute("UPDATE simulation_sessions SET status='expired',generation=generation+1 WHERE id=?1 AND actor_id=?2 AND status='active' AND julianday(expires_at)<=julianday(?3)",params![id,actor,now]).map_err(db)?;
            tx.execute("INSERT OR IGNORE INTO simulation_cleanup_jobs (simulation_id,tenant_id,job_kind,state,attempts,next_attempt_at,created_at,updated_at) VALUES (?1,?2,'terminal_evidence','pending',0,?3,?3,?3)",params![id,tenant,now]).map_err(db)?;
        }
    } else if status == "provisioning" && lease.as_deref().is_some_and(|l| l <= now.as_str()) {
        tx.execute("UPDATE simulation_sessions SET status='failed',failure_code='SIMULATION_PROVISION_LEASE_EXPIRED' WHERE id=?1 AND actor_id=?2 AND status='provisioning' AND provisioning_lease_until<=?3",params![id,actor,now]).map_err(db)?;
        tx.execute("INSERT OR IGNORE INTO simulation_cleanup_jobs (simulation_id,tenant_id,job_kind,state,attempts,next_attempt_at,created_at,updated_at) VALUES (?1,?2,'orphan_cleanup','pending',0,?3,?3,?3)",params![id,tenant,now]).map_err(db)?;
    }
    tx.commit().map_err(db)
}

fn admit_rate(
    tx: &rusqlite::Transaction<'_>,
    actor: &str,
    simulation_id: &str,
    operation: &str,
    seconds: i64,
    limit: i64,
) -> Result<(), String> {
    let now = shanghai_now();
    let start = now - Duration::seconds(now.timestamp().rem_euclid(seconds));
    let bucket = start.with_nanosecond(0).unwrap().to_rfc3339();
    tx.execute("INSERT INTO simulation_rate_buckets (actor_id,simulation_id,operation_class,window_start,request_count) VALUES (?1,?2,?3,?4,1) ON CONFLICT(actor_id,simulation_id,operation_class,window_start) DO UPDATE SET request_count=request_count+1 WHERE simulation_rate_buckets.request_count < ?5",params![actor,simulation_id,operation,bucket,limit]).map_err(db)?;
    let count: i64 = tx.query_row("SELECT request_count FROM simulation_rate_buckets WHERE actor_id=?1 AND simulation_id=?2 AND operation_class=?3 AND window_start=?4",params![actor,simulation_id,operation,bucket],|r|r.get(0)).map_err(db)?;
    if count > limit || count == limit && tx.changes() == 0 {
        return Err(err(
            "SIMULATION_RATE_LIMITED",
            "simulation rate limit exceeded",
        ));
    }
    Ok(())
}
fn insert_manifest(
    tx: &rusqlite::Transaction<'_>,
    s: &Session,
    key: &str,
    present: i64,
    version: i64,
    doc: Option<&str>,
    now: &str,
    kind: &str,
) -> Result<(), String> {
    tx.execute("INSERT INTO simulation_revision_evidence (simulation_id,tenant_id,resource_kind,resource_key,base_presence,captured_resource_version) VALUES (?1,?2,?3,?4,?5,?6)",params![s.id,s.tenant_id,kind,key,present,version]).map_err(db)?;
    if let Some(doc) = doc {
        tx.execute("INSERT INTO simulation_base_documents (simulation_id,tenant_id,resource_kind,resource_key,base_resource_version,document_json,payload_policy,captured_at) VALUES (?1,?2,?3,?4,?5,?6,'REFERENCE_ONLY',?7)",params![s.id,s.tenant_id,kind,key,version,doc,now]).map_err(db)?;
    }
    Ok(())
}

fn pricing_document(conn: &Connection, tenant_id: &str) -> Result<Value, String> {
    let row: Option<(f64, f64, String, String)> = conn.query_row(
        "SELECT baseWeekdayPrice,baseWeekendPrice,holidayRulesJson,COALESCE(receiveShippingFeesJson,'{}') FROM pricing_configs WHERE tenant_id=?1 LIMIT 1",
        [tenant_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).optional().map_err(db)?;
    let Some((weekday, weekend, holidays, shipping)) = row else {
        return Err(err(
            "SIMULATION_NOT_FOUND",
            "tenant pricing config not found",
        ));
    };
    let holidays = serde_json::from_str::<Value>(&holidays).map_err(serialization)?;
    let shipping = serde_json::from_str::<Value>(&shipping).map_err(serialization)?;
    let dynamic: Map<String, Value> = {
        let today = shanghai_now().date_naive();
        let end = (today + Duration::days(14)).format("%Y-%m-%d").to_string();
        let mut stmt = conn.prepare("SELECT dateKey,price FROM dynamic_daily_prices WHERE tenant_id=?1 AND dateKey>=?2 AND dateKey<=?3").map_err(db)?;
        stmt.query_map(
            params![tenant_id, today.format("%Y-%m-%d").to_string(), end],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)),
        )
        .map_err(db)?
        .map(|row| row.map(|(date, price)| (date, json!(price))).map_err(db))
        .collect::<Result<_, _>>()?
    };
    valid_pricing_config(
        json!({"baseWeekdayPrice":weekday,"baseWeekendPrice":weekend,"holidayRules":holidays,"receiveShippingFees":shipping,"dynamicPriceMap":dynamic}),
    )
}

fn sync_pricing_revision(tx: &rusqlite::Transaction<'_>, tenant_id: &str) -> Result<(), String> {
    let doc = pricing_document(tx, tenant_id)?;
    let canonical = canonical_json(&doc);
    let current: Option<(String, i64)> = tx.query_row(
        "SELECT document_json,resource_version FROM simulation_pricing_resource_revisions WHERE tenant_id=?1 AND resource_key=?2",
        params![tenant_id, PRICING_KEY], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional().map_err(db)?;
    if current.as_ref().is_some_and(|(old, _)| old == &canonical) {
        return Ok(());
    }
    let version = current.map(|(_, version)| version + 1).unwrap_or(1);
    let now = shanghai_now().to_rfc3339();
    tx.execute("INSERT INTO simulation_pricing_resource_revisions (tenant_id,resource_key,present,resource_version,document_json,updated_at) VALUES (?1,?2,1,?3,?4,?5) ON CONFLICT(tenant_id,resource_key) DO UPDATE SET present=1,resource_version=excluded.resource_version,document_json=excluded.document_json,updated_at=excluded.updated_at",params![tenant_id,PRICING_KEY,version,canonical,now]).map_err(db)?;
    tx.execute("INSERT INTO simulation_tenant_revisions (tenant_id,revision,updated_at) VALUES (?1,1,?2) ON CONFLICT(tenant_id) DO UPDATE SET revision=simulation_tenant_revisions.revision+1,updated_at=excluded.updated_at",params![tenant_id,now]).map_err(db)?;
    Ok(())
}

fn pricing_visible_document(conn: &Connection, session: &Session) -> Result<Value, String> {
    let overlay: Option<(String, Option<String>)> = conn.query_row(
        "SELECT operation,document_json FROM simulation_overlay_entries WHERE simulation_id=?1 AND tenant_id=?2 AND resource_kind=?3 AND resource_key=?4",
        params![session.id,session.tenant_id,PRICING_KIND,PRICING_KEY], |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional().map_err(db)?;
    if let Some((operation, Some(doc))) = overlay {
        if operation == "value" {
            return Ok(
                json!({"key":PRICING_KEY,"value":serde_json::from_str::<Value>(&doc).map_err(serialization)?}),
            );
        }
    }
    let doc: String = conn.query_row("SELECT document_json FROM simulation_base_documents WHERE simulation_id=?1 AND tenant_id=?2 AND resource_kind=?3 AND resource_key=?4",params![session.id,session.tenant_id,PRICING_KIND,PRICING_KEY],|r|r.get(0)).optional().map_err(db)?.ok_or_else(|| err("SIMULATION_NOT_FOUND", "simulation pricing config not found"))?;
    Ok(
        json!({"key":PRICING_KEY,"value":serde_json::from_str::<Value>(&doc).map_err(serialization)?}),
    )
}

fn valid_pricing_config(value: Value) -> Result<Value, String> {
    let object = value
        .as_object()
        .ok_or_else(|| err("VAL_PRICING", "pricing config must be an object"))?;
    let weekday = object
        .get("baseWeekdayPrice")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(|| {
            err(
                "VAL_PRICING",
                "baseWeekdayPrice must be a non-negative number",
            )
        })?;
    let weekend = object
        .get("baseWeekendPrice")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(|| {
            err(
                "VAL_PRICING",
                "baseWeekendPrice must be a non-negative number",
            )
        })?;
    let holidays = object
        .get("holidayRules")
        .cloned()
        .unwrap_or_else(|| json!([]));
    if !holidays.is_array() {
        return Err(err("VAL_PRICING", "holidayRules must be an array"));
    }
    if holidays.as_array().expect("checked array").len() > 128 {
        return Err(err(
            "VAL_PRICING",
            "holidayRules must contain at most 128 entries",
        ));
    }
    for rule in holidays.as_array().expect("checked array") {
        let rule = rule
            .as_object()
            .ok_or_else(|| err("VAL_PRICING", "holidayRules entries must be objects"))?;
        let name = rule
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty() && name.len() <= 100)
            .ok_or_else(|| err("VAL_PRICING", "holiday rule name is required"))?;
        let _ = name;
        let start = rule
            .get("startDate")
            .and_then(Value::as_str)
            .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
            .ok_or_else(|| err("VAL_PRICING", "holiday startDate must be YYYY-MM-DD"))?;
        let end = rule
            .get("endDate")
            .and_then(Value::as_str)
            .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
            .ok_or_else(|| err("VAL_PRICING", "holiday endDate must be YYYY-MM-DD"))?;
        if end < start {
            return Err(err(
                "VAL_PRICING",
                "holiday endDate must not precede startDate",
            ));
        }
        if rule
            .get("price")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= 0.0)
            .is_none()
        {
            return Err(err(
                "VAL_PRICING",
                "holiday price must be a non-negative number",
            ));
        }
        if rule
            .get("includePreviousDay")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err(err(
                "VAL_PRICING",
                "holiday includePreviousDay must be a boolean",
            ));
        }
    }
    let shipping = object
        .get("receiveShippingFees")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !shipping.is_object() {
        return Err(err("VAL_PRICING", "receiveShippingFees must be an object"));
    }
    if shipping.as_object().expect("checked object").len() > 64 {
        return Err(err(
            "VAL_PRICING",
            "receiveShippingFees must contain at most 64 entries",
        ));
    }
    for (province, fee) in shipping.as_object().expect("checked object") {
        if province.trim().is_empty()
            || fee
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .is_none()
        {
            return Err(err(
                "VAL_PRICING",
                "receiveShippingFees must map non-empty provinces to non-negative numbers",
            ));
        }
    }
    let dynamic = object
        .get("dynamicPriceMap")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !dynamic.is_object() {
        return Err(err("VAL_PRICING", "dynamicPriceMap must be an object"));
    }
    for (date, price) in dynamic.as_object().expect("checked object") {
        let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| err("VAL_PRICING", "dynamicPriceMap keys must be YYYY-MM-DD"))?;
        let today = shanghai_now().date_naive();
        if parsed < today || parsed > today + Duration::days(14) {
            return Err(err(
                "VAL_PRICING",
                "dynamicPriceMap dates must be within Shanghai today through +14 days",
            ));
        }
        if price
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .is_none()
        {
            return Err(err(
                "VAL_PRICING",
                "dynamicPriceMap values must be non-negative numbers",
            ));
        }
    }
    Ok(
        json!({"baseWeekdayPrice":weekday,"baseWeekendPrice":weekend,"holidayRules":holidays,"receiveShippingFees":shipping,"dynamicPriceMap":dynamic}),
    )
}
fn ensure_active_generation(tx: &rusqlite::Transaction<'_>, s: &Session) -> Result<(), String> {
    let ok:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM simulation_sessions WHERE id=?1 AND target_tenant_id=?2 AND status='active' AND generation=?3 AND julianday(expires_at)>julianday(?4))",params![s.id,s.tenant_id,s.generation,shanghai_now().to_rfc3339()],|r|r.get(0)).map_err(db)?;
    if ok {
        Ok(())
    } else {
        Err(err(
            "SESSION_NOT_EXECUTABLE",
            "simulation session is not executable",
        ))
    }
}
fn refresh_usage(tx: &rusqlite::Transaction<'_>, s: &Session) -> Result<(), String> {
    let(rows,bytes):(i64,i64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM(length(COALESCE(document_json,''))),0) FROM simulation_overlay_entries WHERE simulation_id=?1 AND tenant_id=?2",params![s.id,s.tenant_id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db)?;
    if rows > MAX_OVERLAY_ROWS || bytes > MAX_DOCUMENT_BYTES {
        return Err(err("SIMULATION_QUOTA_EXCEEDED", "overlay quota exceeded"));
    }
    tx.execute("UPDATE simulation_usage SET overlay_rows=?3,document_bytes=?4 WHERE simulation_id=?1 AND tenant_id=?2",params![s.id,s.tenant_id,rows,bytes]).map_err(db)?;
    Ok(())
}
fn store_result(
    tx: &rusqlite::Transaction<'_>,
    s: &Session,
    actor: &str,
    command: &str,
    key: &str,
    request: &str,
    response: &Value,
    now: &str,
) -> Result<(), String> {
    tx.execute("INSERT INTO simulation_command_results (simulation_id,tenant_id,actor_id,command,command_idempotency_key_hash,request_digest,response_status,response_json,created_at) VALUES (?1,?2,?3,?4,?5,?6,200,?7,?8)",params![s.id,s.tenant_id,actor,command,key,request,canonical_json(response),now]).map_err(db)?;
    Ok(())
}
fn load_session(conn: &Connection, id: &str, actor: &str) -> Result<Session, String> {
    conn.query_row("SELECT id,actor_id,target_tenant_id,namespace_id,base_revision,generation,scenario_name,change_intent,status,created_at,expires_at,discarded_at,failure_code,create_request_digest FROM simulation_sessions WHERE id=?1 AND actor_id=?2",params![id,actor],|r|Ok(Session{id:r.get(0)?,actor_id:r.get(1)?,tenant_id:r.get(2)?,namespace_id:r.get(3)?,base_revision:r.get(4)?,generation:r.get(5)?,scenario_name:r.get(6)?,change_intent:r.get(7)?,status:r.get(8)?,created_at:r.get(9)?,expires_at:r.get(10)?,discarded_at:r.get(11)?,failure_code:r.get(12)?,create_request_digest:r.get(13)?})).optional().map_err(db)?.ok_or_else(||err("SIMULATION_NOT_FOUND","simulation not found"))
}
fn load_by_create_key(
    conn: &Connection,
    actor: &str,
    key: &str,
) -> Result<Option<Session>, String> {
    conn.query_row(
        "SELECT id FROM simulation_sessions WHERE actor_id=?1 AND idempotency_key_hash=?2",
        params![actor, key],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map_err(db)?
    .map(|id| load_session(conn, &id, actor))
    .transpose()
}
fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| err("VAL_INPUT", &e.to_string()))
}
fn required<'a>(s: &'a str, field: &str) -> Result<&'a str, String> {
    let s = s.trim();
    if s.is_empty() {
        Err(err("VAL_REQUIRED", &format!("{field} is required")))
    } else {
        Ok(s)
    }
}
fn plain(s: &str, field: &str, max: usize) -> Result<String, String> {
    let s = required(s, field)?;
    if s.len() > max || s.chars().any(char::is_control) {
        return Err(err("VAL_TEXT", &format!("{field} is invalid")));
    }
    Ok(s.to_owned())
}
fn valid_key(s: &str) -> Result<&str, String> {
    let s = required(s, "key")?;
    if s.len() > 64
        || !s
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        Err(err(
            "VAL_KEY",
            "key must be 1-64 ASCII alphanumeric, dash, or underscore characters",
        ))
    } else {
        Ok(s)
    }
}
fn normalize_keys(keys: Vec<String>) -> Result<Vec<String>, String> {
    if keys.len() > MAX_KEYS {
        return Err(err("VAL_PLANNED_KEYS", "too many planned absent keys"));
    }
    let mut out: Vec<String> = keys
        .iter()
        .map(|k| valid_key(k).map(str::to_owned))
        .collect::<Result<_, _>>()?;
    out.sort();
    out.dedup();
    if out.len() != keys.len() {
        return Err(err(
            "VAL_PLANNED_KEYS",
            "planned absent keys must be unique",
        ));
    }
    Ok(out)
}
fn valid_idempotency(s: &str) -> Result<&str, String> {
    let s = required(s, "idempotencyKey")?;
    if !(16..=128).contains(&s.len()) || !s.bytes().all(|c| c.is_ascii_graphic()) {
        Err(err(
            "VAL_IDEMPOTENCY",
            "idempotencyKey must be 16-128 printable ASCII characters",
        ))
    } else {
        Ok(s)
    }
}
fn valid_value(value: Value) -> Result<Value, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| err("VAL_VALUE", "value must be an object"))?;
    if obj.len() != 3 {
        return Err(err(
            "VAL_VALUE",
            "value must contain enabled, threshold, and label",
        ));
    }
    let enabled = obj
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| err("VAL_VALUE", "enabled must be boolean"))?;
    let threshold = obj
        .get("threshold")
        .and_then(Value::as_i64)
        .filter(|v| (0..=1000).contains(v))
        .ok_or_else(|| err("VAL_VALUE", "threshold must be 0..1000"))?;
    let label = obj
        .get("label")
        .and_then(Value::as_str)
        .filter(|v| v.len() <= 80 && !v.chars().any(char::is_control))
        .ok_or_else(|| err("VAL_VALUE", "label is invalid"))?;
    Ok(json!({"enabled":enabled,"threshold":threshold,"label":label}))
}
fn page_limit(limit: Option<u32>) -> Result<u32, String> {
    let n = limit.unwrap_or(100);
    if n == 0 || n > MAX_PAGE {
        Err(err("VAL_PAGE", "limit must be between 1 and 500"))
    } else {
        Ok(n)
    }
}
fn canonical_json(value: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut entries: Vec<_> = m.iter().collect();
                entries.sort_by_key(|(k, _)| *k);
                let mut out = Map::new();
                for (k, v) in entries {
                    out.insert(k.clone(), sort(v));
                }
                Value::Object(out)
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            _ => v.clone(),
        }
    }
    serde_json::to_string(&sort(value)).unwrap_or_default()
}
fn digest_json(value: &Value) -> String {
    digest_str(&canonical_json(value))
}
fn digest_str(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
fn shanghai_now() -> DateTime<chrono_tz::Tz> {
    Utc::now().with_timezone(&Shanghai)
}
fn parse_time(s: &str) -> Result<DateTime<chrono_tz::Tz>, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Shanghai))
        .map_err(|_| err("SYS_TIME", "invalid simulation timestamp"))
}
fn db<E: std::fmt::Display>(e: E) -> String {
    err("SYS_DB", &e.to_string())
}
fn serialization<E: std::fmt::Display>(e: E) -> String {
    err("SYS_SERIALIZE", &e.to_string())
}
fn code_of(s: &str) -> String {
    serde_json::from_str::<ErrorPayload>(s)
        .map(|e| e.code)
        .unwrap_or_else(|_| "SIMULATION_PROVISION_FAILED".into())
}
fn err(code: &str, message: &str) -> String {
    serde_json::to_string(&ErrorPayload {
        category: if code.starts_with("VAL_") {
            "validation"
        } else if code.starts_with("AUTH_")
            || code.starts_with("SIMULATION_")
            || code.starts_with("SESSION_")
            || code.starts_with("UNPROVISIONED_")
        {
            "auth"
        } else {
            "sys"
        }
        .into(),
        code: code.into(),
        message: message.into(),
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::FeatureTenantSimulation;
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use serde_json::json;
    use std::sync::Arc;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, SimulationId, SystemModule, TenantId, TenantScope,
    };

    fn pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE tenants (id TEXT PRIMARY KEY, status TEXT NOT NULL); INSERT INTO tenants VALUES ('tenant-a','active'),('tenant-b','active');").unwrap();
        conn.execute_batch(include_str!(
            "../../../src/db/migrations/050_tenant_simulation.sql"
        ))
        .unwrap();
        conn.execute_batch(include_str!(
            "../../../src/db/migrations/051_tenant_simulation_pricing.sql"
        ))
        .unwrap();
        conn.execute_batch("CREATE TABLE pricing_configs (tenant_id TEXT PRIMARY KEY, baseWeekdayPrice REAL NOT NULL, baseWeekendPrice REAL NOT NULL, holidayRulesJson TEXT NOT NULL, receiveShippingFeesJson TEXT); CREATE TABLE dynamic_daily_prices (tenant_id TEXT NOT NULL, dateKey TEXT NOT NULL, price REAL NOT NULL); INSERT INTO pricing_configs VALUES ('tenant-a', 8.5, 14, '[]', '{}'), ('tenant-b', 8.5, 14, '[]', '{}');").unwrap();
        conn.execute("INSERT INTO simulation_tenant_revisions VALUES ('tenant-a', 7, '2026-07-22T12:00:00+08:00')",[]).unwrap();
        conn.execute("INSERT INTO simulation_reference_configs VALUES ('tenant-a','base','{\"enabled\":true,\"threshold\":5,\"label\":\"base\"}',3,'2026-07-22T12:00:00+08:00')",[]).unwrap();
        conn.execute("INSERT INTO simulation_reference_resource_revisions VALUES ('tenant-a','base',3,1,'2026-07-22T12:00:00+08:00')",[]).unwrap();
        pool
    }
    fn platform(actor: &str) -> ExecutionContext {
        ExecutionContext::new(
            ActorIdentity::with_authority(
                actor,
                system_core::AuthorityContext::Platform {
                    membership_id: system_core::PlatformMembershipId::new(format!(
                        "membership-{actor}"
                    ))
                    .unwrap(),
                    roles: vec![system_core::PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::platform(),
            DataScope::platform(Revision::new("platform").unwrap()),
            ExecutionMode::Normal,
            RequestId::new("req-platform").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }
    fn simulation(actor: &str, tenant: &str, id: &str) -> ExecutionContext {
        let tenant = TenantId::new(tenant).unwrap();
        let sid = SimulationId::new(id).unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                actor,
                system_core::AuthorityContext::Platform {
                    membership_id: system_core::PlatformMembershipId::new(format!(
                        "membership-{actor}"
                    ))
                    .unwrap(),
                    roles: vec![system_core::PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::new(
                tenant,
                system_core::Namespace::Simulation(sid.clone()),
                Revision::new("7").unwrap(),
            )
            .unwrap(),
            ExecutionMode::Simulation(sid),
            RequestId::new("req-simulation").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }
    fn create(module: &FeatureTenantSimulation) -> String {
        module.execute("session.create",json!({"tenantId":"tenant-a","scenarioName":"proof","changeIntent":"verify","ttlMinutes":30,"plannedAbsentKeys":["new-key"],"idempotencyKey":"create-key-123456"}),&platform("admin-a")).unwrap()["id"].as_str().unwrap().to_owned()
    }

    #[test]
    fn overlay_isolation_tombstone_and_idempotency_are_fail_closed() {
        let pool = pool();
        let module = FeatureTenantSimulation::with_pool(pool.clone());
        let id = create(&module);
        let ctx = simulation("admin-a", "tenant-a", &id);
        let put = json!({"id":id,"key":"base","value":{"enabled":false,"threshold":6,"label":"overlay"},"idempotencyKey":"mutation-key-1234"});
        module
            .execute("reference_config.put", put.clone(), &ctx)
            .unwrap();
        let replay = module.execute("reference_config.put", put, &ctx).unwrap();
        assert_eq!(replay["value"]["label"], "overlay");
        let production:String=pool.get().unwrap().query_row("SELECT value_json FROM simulation_reference_configs WHERE tenant_id='tenant-a' AND key='base'",[],|r|r.get(0)).unwrap();
        assert!(production.contains("base"));
        module
            .execute(
                "reference_config.delete",
                json!({"id":id,"key":"base","idempotencyKey":"delete-key-123456"}),
                &ctx,
            )
            .unwrap();
        assert!(
            module
                .execute("reference_config.get", json!({"id":id,"key":"base"}), &ctx)
                .unwrap_err()
                .contains("SIMULATION_NOT_FOUND")
        );
        assert!(module.execute("reference_config.put",json!({"id":id,"key":"missing","value":{"enabled":true,"threshold":1,"label":"x"},"idempotencyKey":"another-key-123456"}),&ctx).unwrap_err().contains("UNPROVISIONED_RESOURCE_KEY"));
    }

    #[test]
    fn diff_marks_production_race_as_conflict() {
        let pool = pool();
        let module = FeatureTenantSimulation::with_pool(pool.clone());
        let id = create(&module);
        let ctx = simulation("admin-a", "tenant-a", &id);
        module.execute("reference_config.put",json!({"id":id,"key":"base","value":{"enabled":false,"threshold":9,"label":"overlay"},"idempotencyKey":"conflict-key-12345"}),&ctx).unwrap();
        pool.get().unwrap().execute("UPDATE simulation_reference_resource_revisions SET resource_version=4 WHERE tenant_id='tenant-a' AND key='base'",[]).unwrap();
        let diff = module
            .execute("diff.evaluate", json!({"id":id}), &ctx)
            .unwrap();
        assert_eq!(diff["items"][0]["conflictStatus"], "conflict");
    }

    #[test]
    fn terminal_snapshot_and_active_tenant_guard_are_enforced() {
        let pool = pool();
        let module = FeatureTenantSimulation::with_pool(pool.clone());
        let id = create(&module);
        let ctx = simulation("admin-a", "tenant-a", &id);
        let first = module
            .execute("diff.evaluate", json!({"id":id}), &ctx)
            .unwrap();
        let second = module
            .execute("diff.evaluate", json!({"id":id}), &ctx)
            .unwrap();
        assert_eq!(first["evaluationId"], second["evaluationId"]);
        module
            .execute("session.discard", json!({"id":id}), &platform("admin-a"))
            .unwrap();
        let frozen: i64 = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM simulation_terminal_inputs WHERE simulation_id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(frozen, 1);

        let id = create(&module);
        pool.get()
            .unwrap()
            .execute(
                "UPDATE tenants SET status='suspended' WHERE id='tenant-a'",
                [],
            )
            .unwrap();
        let blocked = module
            .execute(
                "reference_config.get",
                json!({"id":id,"key":"base"}),
                &simulation("admin-a", "tenant-a", &id),
            )
            .unwrap_err();
        assert!(blocked.contains("SESSION_NOT_EXECUTABLE"));
    }

    #[test]
    fn planned_absent_key_preserves_existing_tombstone_revision() {
        let pool = pool();
        pool.get().unwrap().execute(
            "INSERT INTO simulation_reference_resource_revisions VALUES ('tenant-a','retired',9,0,'2026-07-22T12:00:00+08:00')", [],
        ).unwrap();
        let module = FeatureTenantSimulation::with_pool(pool.clone());
        let created = module.execute("session.create",json!({"tenantId":"tenant-a","scenarioName":"retired","changeIntent":"verify tombstone","plannedAbsentKeys":["retired"],"idempotencyKey":"retired-create-key"}),&platform("admin-a")).unwrap();
        let id = created["id"].as_str().unwrap();
        let version:i64=pool.get().unwrap().query_row("SELECT captured_resource_version FROM simulation_revision_evidence WHERE simulation_id=?1 AND resource_key='retired'",[id],|r|r.get(0)).unwrap();
        assert_eq!(version, 9);
    }

    #[test]
    fn access_time_expiry_reconciles_and_fixed_window_rate_is_durable() {
        let pool = pool();
        let module = FeatureTenantSimulation::with_pool(pool.clone());
        let id = create(&module);
        pool.get()
            .unwrap()
            .execute(
                "UPDATE simulation_sessions SET created_at='1999-01-01T00:00:00+08:00', expires_at='2000-01-01T00:00:00+08:00' WHERE id=?1",
                [&id],
            )
            .unwrap();
        let session = module
            .execute("session.get", json!({"id":id}), &platform("admin-a"))
            .unwrap();
        assert_eq!(session["status"], "expired");
        let frozen: i64 = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM simulation_terminal_inputs WHERE simulation_id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(frozen, 1);

        let mut conn = pool.get().unwrap();
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        super::admit_rate(&tx, "admin-a", "rate-test", "test", 60, 1).unwrap();
        assert!(
            super::admit_rate(&tx, "admin-a", "rate-test", "test", 60, 1)
                .unwrap_err()
                .contains("SIMULATION_RATE_LIMITED")
        );
    }

    #[test]
    fn replays_bypass_rate_and_tombstone_pages_advance_raw_cursor() {
        let pool = pool();
        let module = FeatureTenantSimulation::with_pool(pool.clone());
        let id = create(&module);
        let ctx = simulation("admin-a", "tenant-a", &id);
        let put = json!({"id":id,"key":"base","value":{"enabled":false,"threshold":10,"label":"overlay"},"idempotencyKey":"replay-before-rate"});
        module
            .execute("reference_config.put", put.clone(), &ctx)
            .unwrap();
        module
            .execute(
                "reference_config.delete",
                json!({"id":id,"key":"base","idempotencyKey":"page-delete-key-1"}),
                &ctx,
            )
            .unwrap();
        pool.get().unwrap().execute("UPDATE simulation_rate_buckets SET request_count=120 WHERE simulation_id=?1 AND operation_class='data'",[&id]).unwrap();
        assert!(module.execute("reference_config.put", put, &ctx).is_ok());
        let page = module
            .execute("reference_config.list", json!({"id":id,"limit":1}), &ctx)
            .unwrap();
        assert_eq!(page["items"], json!([]));
        assert_eq!(page["nextCursor"], "base");
        let missing = module
            .execute(
                "diff.get",
                json!({"id":id,"evaluationId":"foreign-evaluation","limit":10}),
                &ctx,
            )
            .unwrap_err();
        assert!(missing.contains("DIFF_EVALUATION_NOT_FOUND"));
    }

    #[test]
    fn create_replay_is_atomic_and_provision_quota_failure_is_persisted() {
        let pool = pool();
        let module = FeatureTenantSimulation::with_pool(pool.clone());
        let first = create(&module);
        let second = create(&module);
        assert_eq!(first, second);
        let create_count:i64=pool.get().unwrap().query_row("SELECT request_count FROM simulation_rate_buckets WHERE actor_id='admin-a' AND simulation_id='__create__' AND operation_class='create'",[],|r|r.get(0)).unwrap();
        assert_eq!(create_count, 1);

        let mut conn = pool.get().unwrap();
        let tx = conn.transaction().unwrap();
        for n in 0..2000 {
            tx.execute("INSERT INTO simulation_reference_resource_revisions VALUES ('tenant-b',?1,1,0,'2026-07-22T12:00:00+08:00')",[format!("key-{n}")]).unwrap();
        }
        tx.execute("INSERT INTO simulation_reference_resource_revisions VALUES ('tenant-b','overflow',1,0,'2026-07-22T12:00:00+08:00')",[]).unwrap();
        tx.commit().unwrap();
        drop(conn);
        let failure=module.execute("session.create",json!({"tenantId":"tenant-b","scenarioName":"quota","changeIntent":"quota proof","idempotencyKey":"quota-create-key-1"}),&platform("admin-b")).unwrap_err();
        assert!(failure.contains("SIMULATION_QUOTA_EXCEEDED"));
        let status: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT status FROM simulation_sessions WHERE actor_id='admin-b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "failed");
    }

    #[test]
    fn pricing_snapshot_overlay_estimate_and_production_conflict_are_isolated() {
        let pool = pool();
        let module = FeatureTenantSimulation::with_pool(pool.clone());
        let today = super::shanghai_now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let yesterday = (super::shanghai_now().date_naive() - chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let tomorrow = (super::shanghai_now().date_naive() + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        pool.get().unwrap().execute("UPDATE pricing_configs SET holidayRulesJson=?1 WHERE tenant_id='tenant-a'", [json!([{"name":"holiday","startDate":today,"endDate":today,"price":20.0,"includePreviousDay":true}]).to_string()]).unwrap();
        pool.get()
            .unwrap()
            .execute(
                "INSERT INTO dynamic_daily_prices VALUES ('tenant-a',?1,30)",
                [&today],
            )
            .unwrap();
        pool.get()
            .unwrap()
            .execute(
                "INSERT INTO dynamic_daily_prices VALUES ('tenant-a',?1,999)",
                [&yesterday],
            )
            .unwrap();
        let id = create(&module);
        let ctx = simulation("admin-a", "tenant-a", &id);
        let initial = module
            .execute("pricing_config.get", json!({"id":id}), &ctx)
            .unwrap();
        assert_eq!(initial["value"]["dynamicPriceMap"][&today], 30.0);
        assert!(
            initial["value"]["dynamicPriceMap"]
                .get(&yesterday)
                .is_none()
        );
        let overlay = json!({"baseWeekdayPrice":9.0,"baseWeekendPrice":9.0,"holidayRules":[{"name":"first","startDate":tomorrow,"endDate":tomorrow,"price":20.0},{"name":"second","startDate":tomorrow,"endDate":tomorrow,"price":25.0,"includePreviousDay":true}],"receiveShippingFees":{},"dynamicPriceMap":{today.clone():31.0}});
        module
            .execute(
                "pricing_config.put",
                json!({"id":id,"value":overlay,"idempotencyKey":"pricing-mutation-123"}),
                &ctx,
            )
            .unwrap();
        let estimate = module
            .execute(
                "pricing_config.estimate",
                json!({"id":id,"startDate":today,"endDate":today}),
                &ctx,
            )
            .unwrap();
        assert_eq!(estimate["days"][0]["price"], 31.0);
        assert_eq!(estimate["days"][0]["source"], "dynamic");
        let holiday_estimate = module
            .execute(
                "pricing_config.estimate",
                json!({"id":id,"startDate":tomorrow,"endDate":tomorrow}),
                &ctx,
            )
            .unwrap();
        assert_eq!(holiday_estimate["days"][0]["price"], 20.0);
        pool.get()
            .unwrap()
            .execute(
                "UPDATE pricing_configs SET baseWeekdayPrice=99 WHERE tenant_id='tenant-a'",
                [],
            )
            .unwrap();
        let diff = module
            .execute("diff.evaluate", json!({"id":id}), &ctx)
            .unwrap();
        assert!(
            diff["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["resourceKind"] == "pricing_config"
                    && item["conflictStatus"] == "conflict")
        );
        module
            .execute("session.discard", json!({"id":id}), &platform("admin-a"))
            .unwrap();
        let terminal_pricing: i64 = pool.get().unwrap().query_row("SELECT COUNT(*) FROM simulation_terminal_input_items WHERE simulation_id=?1 AND canonical_input_json LIKE '%pricing_config%'", [&id], |r| r.get(0)).unwrap();
        assert_eq!(terminal_pricing, 1);
    }

    #[test]
    fn terminal_input_quota_rolls_back_terminal_cas_and_cleanup() {
        let pool = pool();
        let module = FeatureTenantSimulation::with_pool(pool.clone());
        let id = create(&module);
        let oversized = format!("\"{}\"", "x".repeat((super::MAX_PROOF_BYTES + 1) as usize));
        pool.get().unwrap().execute("INSERT INTO simulation_overlay_entries (simulation_id,tenant_id,resource_kind,resource_key,operation,document_json,base_resource_version,overlay_version,created_at,updated_at) VALUES (?1,'tenant-a','reference_config','base','value',?2,3,1,'2026-07-22T12:00:00+08:00','2026-07-22T12:00:00+08:00')",rusqlite::params![id,oversized]).unwrap();
        let failure = module
            .execute("session.discard", json!({"id":id}), &platform("admin-a"))
            .unwrap_err();
        assert!(failure.contains("SIMULATION_QUOTA_EXCEEDED"));
        let conn = pool.get().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM simulation_sessions WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        let terminals: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM simulation_terminal_inputs WHERE simulation_id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        let jobs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM simulation_cleanup_jobs WHERE simulation_id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "active");
        assert_eq!(terminals, 0);
        assert_eq!(jobs, 0);
    }
}

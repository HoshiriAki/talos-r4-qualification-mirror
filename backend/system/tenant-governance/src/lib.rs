//! Tenant-governance control plane.
//!
//! This module deliberately exposes observation commands plus append-only
//! change-intent capture. Tenant lifecycle mutations and preview sessions are
//! outside MVP1.

use std::sync::Mutex;

use chrono::{NaiveDateTime, TimeZone, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};
use uuid::Uuid;

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;

pub struct FeatureTenantGovernance {
    pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureTenantGovernance {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    pub fn with_pool(pool: Pool<SqliteConnectionManager>) -> Self {
        Self {
            pool: Mutex::new(Some(pool)),
        }
    }

    fn pool(&self) -> Result<Pool<SqliteConnectionManager>, String> {
        self.pool
            .lock()
            .map_err(|_| error("SYS_LOCK", "governance pool lock poisoned", None))?
            .clone()
            .ok_or_else(|| error("SYS_NOT_INIT", "tenant governance is not initialized", None))
    }

    fn require_platform_authority(ctx: &ExecutionContext) -> Result<(), String> {
        if !ctx.actor().has_platform_authority() || !ctx.data_scope().is_platform() {
            return Err(error(
                "AUTH_FORBIDDEN",
                "platform_owner context required",
                None,
            ));
        }
        Ok(())
    }

    fn tenant_list(&self, payload: Value) -> Result<Value, String> {
        let input: PageInput = decode(payload)?;
        let limit = bounded_limit(input.limit)?;
        let cursor = parse_cursor(input.cursor.as_deref())?;
        if let Some(value) = &input.search {
            validate_filter(value, "search")?;
        }
        if let Some(value) = &input.status {
            validate_lifecycle_status(value)?;
        }
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|_| error("SYS_STORAGE", "governance storage unavailable", None))?;
        let mut sql =
            "SELECT id,name,slug,status,plan,created_at,updated_at FROM tenants".to_owned();
        let mut values = Vec::<SqlValue>::new();
        let mut clauses = Vec::new();
        if let Some(search) = input.search {
            clauses.push("(name LIKE ? ESCAPE '\\' OR slug LIKE ? ESCAPE '\\')".to_owned());
            let pattern = format!("%{}%", escape_like(&search));
            values.extend([pattern.clone().into(), pattern.into()]);
        }
        if let Some(status) = input.status {
            clauses.push("status=?".to_owned());
            values.push(status.into());
        }
        if let Some((at, id)) = cursor {
            clauses.push("(julianday(created_at) < julianday(?) OR (julianday(created_at) = julianday(?) AND id < ?))".to_owned());
            values.extend([at.clone().into(), at.into(), id.into()]);
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY julianday(created_at) DESC,id DESC LIMIT ?");
        values.push(((limit + 1) as i64).into());
        let mut stmt = conn.prepare(&sql).map_err(storage_error)?;
        let rows = stmt
            .query_map(params_from_iter(values), map_tenant)
            .map_err(storage_error)?;
        let mut tenants = rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
        let has_more = tenants.len() > limit as usize;
        tenants.truncate(limit as usize);
        let next_cursor = if has_more {
            tenants
                .last()
                .map(|t| make_cursor(&t.created_at, &t.id))
                .transpose()?
        } else {
            None
        };
        Ok(json!({"tenants": tenants, "nextCursor": next_cursor}))
    }

    fn tenant_get(&self, payload: Value) -> Result<Value, String> {
        let input: IdInput = decode(payload)?;
        validate_id(&input.id, "id")?;
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|_| error("SYS_STORAGE", "governance storage unavailable", None))?;
        conn.query_row(
            "SELECT id,name,slug,status,plan,created_at,updated_at FROM tenants WHERE id=?1",
            params![input.id],
            map_tenant,
        )
        .map(|t| json!(t))
        .map_err(not_found_or_storage)
    }

    fn tenant_health(&self, payload: Value) -> Result<Value, String> {
        let input: IdInput = decode(payload)?;
        validate_id(&input.id, "id")?;
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|_| error("SYS_STORAGE", "governance storage unavailable", None))?;
        let now = shanghai_now();
        let since = (Utc::now().with_timezone(&chrono_tz::Asia::Shanghai)
            - chrono::Duration::hours(24))
        .format("%Y-%m-%dT%H:%M:%S%.3f+08:00")
        .to_string();
        let result = conn.query_row(
            "SELECT t.id,t.status,MAX(a.createdAt),COUNT(CASE WHEN a.id IS NOT NULL AND (NOT json_valid(a.detailJson) OR LOWER(COALESCE(json_extract(a.detailJson,'$.result'),'')) <> 'attempted') THEN 1 END), \
             COALESCE(SUM(CASE WHEN a.id IS NOT NULL AND json_valid(a.detailJson) AND json_type(a.detailJson,'$.result')='object' THEN 1 ELSE 0 END),0) \
             FROM tenants t \
             LEFT JOIN audit_logs a ON a.tenant_id=t.id AND julianday(a.createdAt) >= julianday(?2) \
             WHERE t.id=?1 GROUP BY t.id,t.status",
            params![input.id, since],
            |row| {
                let status: String = row.get(1)?;
                let count: i64 = row.get(3)?;
                let failed: i64 = row.get(4)?;
                let health_state = if status != "active" {
                    "unavailable".to_owned()
                } else if failed > 0 {
                    "degraded".to_owned()
                } else if count == 0 {
                    "unknown".into()
                } else {
                    "healthy".into()
                };
                let mut signals = Vec::new();
                if status != "active" {
                    signals.push(HealthSignal::new(
                        "tenant_not_active",
                        "Tenant lifecycle",
                        "unavailable",
                        Some(format!("lifecycle status is {status}")),
                    ));
                }
                if failed > 0 {
                    signals.push(HealthSignal::new(
                        "failed_commands_24h",
                        "Failed commands (24h)",
                        "degraded",
                        Some(format!("{failed} failed command(s) observed")),
                    ));
                }
                if count == 0 {
                    signals.push(HealthSignal::new(
                        "no_recent_activity",
                        "Recent activity",
                        "unknown",
                        Some("no audit activity observed in the last 24 hours".to_owned()),
                    ));
                }
                Ok(TenantHealth {
                    tenant_id: row.get(0)?,
                    status: health_state,
                    lifecycle_status: status,
                    last_activity_at: row.get(2)?,
                    audit_events_24h: count,
                    failed_command_count: failed,
                    checked_at: now.clone(),
                    signals,
                })
            },
        );
        result.map(|h| json!(h)).map_err(not_found_or_storage)
    }

    fn audit_query(&self, payload: Value) -> Result<Value, String> {
        let input: AuditQueryInput = decode(payload)?;
        let limit = bounded_limit(input.limit)?;
        if let Some(v) = &input.tenant_id {
            validate_id(v, "tenantId")?;
        }
        if let Some(v) = &input.actor {
            validate_filter(v, "actor")?;
        }
        if let Some(v) = &input.command {
            validate_filter(v, "command")?;
        }
        if let Some(v) = &input.result {
            validate_audit_result(v)?;
        }
        let (from, to) = normalize_time_range(input.from.as_deref(), input.to.as_deref())?;
        if input.tenant_id.is_none()
            && input.actor.is_none()
            && input.command.is_none()
            && input.result.is_none()
            && input.from.is_none()
        {
            return Err(error(
                "VAL_FILTER_REQUIRED",
                "at least one audit filter is required",
                None,
            ));
        }
        let cursor = parse_cursor(input.cursor.as_deref())?;
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|_| error("SYS_STORAGE", "governance storage unavailable", None))?;
        let mut clauses = Vec::new();
        let mut values = Vec::<SqlValue>::new();
        if let Some(v) = input.tenant_id {
            clauses.push("tenant_id=?".to_owned());
            values.push(v.into());
        }
        if let Some(v) = input.actor {
            clauses.push("COALESCE(CASE WHEN json_valid(detailJson) THEN json_extract(detailJson,'$.actor.id') END,actorIdentityId)=?".to_owned());
            values.push(v.into());
        }
        if let Some(v) = input.command {
            clauses.push("COALESCE(CASE WHEN json_valid(detailJson) THEN json_extract(detailJson,'$.command') END,actionType)=?".to_owned());
            values.push(v.into());
        }
        if let Some(v) = input.result {
            clauses.push("CASE WHEN json_valid(detailJson) AND json_type(detailJson,'$.result')='object' THEN CASE WHEN UPPER(COALESCE(json_extract(detailJson,'$.result.failed.error_code'),json_extract(detailJson,'$.result.Failed.error_code'),'')) LIKE '%AUTH%' OR UPPER(COALESCE(json_extract(detailJson,'$.result.failed.error_code'),json_extract(detailJson,'$.result.Failed.error_code'),'')) LIKE '%DENIED%' OR UPPER(COALESCE(json_extract(detailJson,'$.result.failed.error_code'),json_extract(detailJson,'$.result.Failed.error_code'),'')) LIKE '%FORBIDDEN%' THEN 'denied' ELSE 'failed' END WHEN LOWER(COALESCE(json_extract(detailJson,'$.result'),''))='attempted' THEN 'attempted' WHEN LOWER(COALESCE(json_extract(detailJson,'$.result'),''))='succeeded' THEN 'succeeded' ELSE 'failed' END=?".to_owned());
            values.push(v.into());
        }
        if let Some(v) = from {
            clauses.push("julianday(createdAt)>=julianday(?)".to_owned());
            values.push(v.into());
        }
        if let Some(v) = to {
            clauses.push("julianday(createdAt)<=julianday(?)".to_owned());
            values.push(v.into());
        }
        if let Some((at, id)) = cursor {
            clauses.push("(julianday(createdAt) < julianday(?) OR (julianday(createdAt) = julianday(?) AND id < ?))".to_owned());
            values.extend([at.clone().into(), at.into(), id.into()]);
        }
        let mut sql =
            "SELECT id,tenant_id,actorIdentityId,actionType,createdAt,detailJson FROM audit_logs"
                .to_owned();
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY julianday(createdAt) DESC,id DESC LIMIT ?");
        values.push(((limit + 1) as i64).into());
        let mut stmt = conn.prepare(&sql).map_err(storage_error)?;
        let rows = stmt
            .query_map(params_from_iter(values), map_audit)
            .map_err(storage_error)?;
        let mut events = rows.collect::<Result<Vec<_>, _>>().map_err(storage_error)?;
        let has_more = events.len() > limit as usize;
        events.truncate(limit as usize);
        let next_cursor = if has_more {
            events
                .last()
                .map(|e| make_cursor(&e.occurred_at, &e.id))
                .transpose()?
        } else {
            None
        };
        Ok(json!({"events":events,"nextCursor":next_cursor}))
    }

    fn audit_get(&self, payload: Value) -> Result<Value, String> {
        let input: IdInput = decode(payload)?;
        validate_id(&input.id, "id")?;
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|_| error("SYS_STORAGE", "governance storage unavailable", None))?;
        conn.query_row(
            "SELECT id,tenant_id,actorIdentityId,actionType,createdAt,detailJson FROM audit_logs WHERE id=?1",
            params![input.id], map_audit,
        ).map(|e| json!(e)).map_err(not_found_or_storage)
    }

    fn change_intent_record(
        &self,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let mut input: ChangeIntentInput = decode(payload)?;
        input.trim();
        input.validate()?;
        let id = Uuid::new_v4().to_string();
        let actor_id = ctx
            .actor()
            .id()
            .ok_or_else(|| error("AUTH_FORBIDDEN", "authenticated actor required", None))?;
        let tenant_id = input.target_tenant_id.clone();
        let created_at = shanghai_now();
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|_| error("SYS_STORAGE", "governance storage unavailable", None))?;
        let tenant_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tenants WHERE id=?1)",
                params![tenant_id],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        if !tenant_exists {
            return Err(error("NOT_FOUND", "resource not found", None));
        }
        let tx = conn.unchecked_transaction().map_err(storage_error)?;
        tx.execute(
            "INSERT INTO change_intents (id,target_tenant_id,actor_id,reason,intended_outcome,impact,cost_minor,currency,source,correlation_id,status,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'platform_governance',?9,'recorded',?10)",
            params![&id,&tenant_id,actor_id,&input.reason,&input.intended_outcome,&input.impact,
                input.cost_minor,&input.currency,ctx.correlation_id().as_str(),&created_at],
        ).map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(
            json!({"id":id,"actorId":actor_id,"targetTenantId":tenant_id,
            "source":"platform_governance","status":"recorded",
            "correlationId":ctx.correlation_id().as_str(),"createdAt":created_at}),
        )
    }
}

impl Default for FeatureTenantGovernance {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureTenantGovernance {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "feature-tenant-governance".into(),
            version: "0.1.0".into(),
            description: "Read-only tenant observation, cross-tenant audit, and change intent"
                .into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        let url = config
            .get("databaseUrl")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                error(
                    "SYS_CONFIG",
                    "config.databaseUrl is required",
                    Some("databaseUrl"),
                )
            })?;
        let pool = Pool::builder()
            .max_size(4)
            .build(SqliteConnectionManager::file(url))
            .map_err(|_| error("SYS_POOL", "failed to create governance pool", None))?;
        *self
            .pool
            .lock()
            .map_err(|_| error("SYS_LOCK", "governance pool lock poisoned", None))? = Some(pool);
        Ok(())
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        [
            "tenant.list",
            "tenant.get",
            "tenant.health",
            "audit.query",
            "audit.get",
        ]
        .into_iter()
        .map(|name| {
            CommandMetadata::new(
                name,
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            )
        })
        .chain(std::iter::once(CommandMetadata::new(
            "change_intent.record",
            AccessRequirement::Platform,
            &[EffectClass::DatabaseWrite],
            SimulationSupport::Blocked,
        )))
        .collect()
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        if std::env::var("DB_BACKEND")
            .map(|value| value.eq_ignore_ascii_case("postgres"))
            .unwrap_or(false)
        {
            return Err(error(
                "SYS_BACKEND_UNSUPPORTED",
                "tenant governance is unavailable while PostgreSQL is authoritative",
                None,
            ));
        }
        Self::require_platform_authority(ctx)?;
        match command {
            "tenant.list" => self.tenant_list(payload),
            "tenant.get" => self.tenant_get(payload),
            "tenant.health" => self.tenant_health(payload),
            "audit.query" => self.audit_query(payload),
            "audit.get" => self.audit_get(payload),
            "change_intent.record" => self.change_intent_record(payload, ctx),
            _ => Err(error(
                "SYS_UNKNOWN_COMMAND",
                "unknown governance command",
                Some("command"),
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "feature-tenant-governance".into(),
            description: "Tenant governance control plane".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|m| CommandSchema {
                    name: m.name.to_owned(),
                    description: m.name.to_owned(),
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
struct PageInput {
    limit: Option<u32>,
    cursor: Option<String>,
    search: Option<String>,
    status: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdInput {
    id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuditQueryInput {
    tenant_id: Option<String>,
    actor: Option<String>,
    command: Option<String>,
    result: Option<String>,
    from: Option<String>,
    to: Option<String>,
    limit: Option<u32>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangeIntentInput {
    target_tenant_id: String,
    reason: String,
    intended_outcome: String,
    #[serde(default)]
    impact: String,
    cost_minor: Option<i64>,
    currency: Option<String>,
}
impl ChangeIntentInput {
    fn trim(&mut self) {
        self.target_tenant_id = self.target_tenant_id.trim().into();
        self.reason = self.reason.trim().into();
        self.intended_outcome = self.intended_outcome.trim().into();
        self.impact = self.impact.trim().into();
        self.currency = self.currency.take().map(|v| v.trim().to_ascii_uppercase());
    }
    fn validate(&self) -> Result<(), String> {
        validate_id(&self.target_tenant_id, "targetTenantId")?;
        validate_required_text(&self.reason, "reason", 1_000)?;
        validate_required_text(&self.intended_outcome, "intendedOutcome", 2_000)?;
        if self.impact.chars().count() > 4_000 {
            return Err(error(
                "VAL_TOO_LONG",
                "impact exceeds maximum length",
                Some("impact"),
            ));
        }
        if self.cost_minor.is_some() != self.currency.is_some() {
            return Err(error(
                "VAL_COST_CURRENCY",
                "cost and currency must be provided together",
                Some("currency"),
            ));
        }
        if let Some(cost) = self.cost_minor
            && cost < 0
        {
            return Err(error(
                "VAL_RANGE",
                "estimated cost must not be negative",
                Some("costMinor"),
            ));
        }
        if let Some(c) = &self.currency
            && (c.len() != 3 || !c.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err(error(
                "VAL_CURRENCY",
                "currency must be ISO 4217 code",
                Some("currency"),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TenantView {
    id: String,
    name: String,
    slug: String,
    status: String,
    plan: String,
    created_at: String,
    updated_at: String,
}
fn map_tenant(row: &rusqlite::Row<'_>) -> rusqlite::Result<TenantView> {
    Ok(TenantView {
        id: row.get(0)?,
        name: row.get(1)?,
        slug: row.get(2)?,
        status: row.get(3)?,
        plan: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TenantHealth {
    tenant_id: String,
    status: String,
    lifecycle_status: String,
    last_activity_at: Option<String>,
    audit_events_24h: i64,
    failed_command_count: i64,
    checked_at: String,
    signals: Vec<HealthSignal>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthSignal {
    key: String,
    label: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl HealthSignal {
    fn new(key: &str, label: &str, status: &str, detail: Option<String>) -> Self {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            status: status.to_owned(),
            detail,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditView {
    id: String,
    tenant_id: String,
    actor_id: String,
    command: String,
    result: String,
    correlation_id: String,
    source: Option<String>,
    change_intent_id: Option<String>,
    occurred_at: String,
    detail: Value,
}
fn map_audit(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditView> {
    let raw: String = row
        .get::<_, Option<String>>(5)?
        .unwrap_or_else(|| "{}".into());
    let detail = redact_audit_detail(serde_json::from_str(&raw).unwrap_or(Value::Null));
    let command = detail
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or(row.get(3)?);
    let actor_id = detail
        .pointer("/actor/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or(row.get::<_, Option<String>>(2)?.unwrap_or_default());
    let occurred_at = detail
        .get("occurred_at")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or(row.get(4)?);
    let correlation_id = detail
        .pointer("/correlation_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let result = normalize_result(detail.get("result"));
    let source = detail
        .get("source")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let change_intent_id = detail
        .get("change_intent_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(AuditView {
        id: row.get(0)?,
        tenant_id: row.get(1)?,
        actor_id,
        command,
        result,
        correlation_id,
        source,
        change_intent_id,
        occurred_at,
        detail,
    })
}

fn redact_audit_detail(value: Value) -> Value {
    let mut safe = serde_json::Map::new();
    if let Value::Object(map) = value {
        if let Some(actor_id) = map
            .get("actor")
            .and_then(|actor| actor.get("id"))
            .and_then(Value::as_str)
        {
            safe.insert("actor".into(), json!({ "id": actor_id }));
        }
        if let Some(result) = map.get("result") {
            let projected = match result {
                Value::String(value) => Value::String(value.clone()),
                Value::Object(value) => {
                    let failed = value.get("Failed").or_else(|| value.get("failed"));
                    let code = failed
                        .and_then(|failed| {
                            failed.get("error_code").or_else(|| failed.get("errorCode"))
                        })
                        .and_then(Value::as_str)
                        .unwrap_or("UNKNOWN_FAILURE");
                    json!({ "Failed": { "error_code": code } })
                }
                _ => Value::String("Unknown".into()),
            };
            safe.insert("result".into(), projected);
        }
        for field in [
            "command",
            "occurred_at",
            "correlation_id",
            "payload_policy",
            "source",
            "change_intent_id",
            "target_tenant_id",
        ] {
            if let Some(value) = map.get(field).and_then(Value::as_str) {
                safe.insert(field.to_owned(), Value::String(value.to_owned()));
            }
        }
        let payload = match map.get("payload_policy").and_then(Value::as_str) {
            Some("HashOnly") => Value::String("[HASH_ONLY]".into()),
            _ => Value::Null,
        };
        safe.insert("payload".into(), payload);
    }
    let mut value = Value::Object(safe);
    redact_secrets(&mut value);
    value
}
fn normalize_result(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) if value.eq_ignore_ascii_case("attempted") => "attempted".into(),
        Some(Value::String(value)) if value.eq_ignore_ascii_case("succeeded") => "succeeded".into(),
        Some(Value::Object(value)) => {
            let code = value
                .get("Failed")
                .or_else(|| value.get("failed"))
                .and_then(|failed| failed.get("error_code").or_else(|| failed.get("errorCode")))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_uppercase();
            if code.contains("AUTH") || code.contains("DENIED") || code.contains("FORBIDDEN") {
                "denied".into()
            } else {
                "failed".into()
            }
        }
        _ => "failed".into(),
    }
}
fn redact_secrets(value: &mut Value) {
    match value {
        Value::Object(m) => {
            for (k, v) in m.iter_mut() {
                let n = k.to_ascii_lowercase();
                if [
                    "password",
                    "passwordhash",
                    "token",
                    "secret",
                    "session",
                    "cookie",
                    "authorization",
                    "apikey",
                    "api_key",
                    "phone",
                    "email",
                    "address",
                    "customer",
                    "contract",
                    "document",
                    "identity",
                    "id_card",
                    "passport",
                ]
                .iter()
                .any(|s| n.contains(s))
                {
                    *v = Value::String("[REDACTED]".into())
                } else {
                    redact_secrets(v)
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(redact_secrets),
        _ => {}
    }
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|_| error("VAL_INPUT", "invalid command input", None))
}
fn bounded_limit(limit: Option<u32>) -> Result<u32, String> {
    let v = limit.unwrap_or(DEFAULT_LIMIT);
    if v == 0 || v > MAX_LIMIT {
        Err(error(
            "VAL_LIMIT",
            "limit must be between 1 and 200",
            Some("limit"),
        ))
    } else {
        Ok(v)
    }
}
fn validate_id(v: &str, field: &str) -> Result<(), String> {
    if v.trim().is_empty()
        || v.len() > 128
        || !v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.:".contains(&b))
    {
        Err(error("VAL_ID", "invalid identifier", Some(field)))
    } else {
        Ok(())
    }
}
fn validate_filter(v: &str, field: &str) -> Result<(), String> {
    if v.trim().is_empty() || v.len() > 100 {
        Err(error("VAL_FILTER", "invalid filter", Some(field)))
    } else {
        Ok(())
    }
}
fn validate_lifecycle_status(value: &str) -> Result<(), String> {
    if ["active", "suspended", "inactive", "deleted"].contains(&value) {
        Ok(())
    } else {
        Err(error(
            "VAL_STATUS",
            "invalid tenant lifecycle status",
            Some("status"),
        ))
    }
}
fn validate_audit_result(value: &str) -> Result<(), String> {
    if ["attempted", "succeeded", "failed", "denied"].contains(&value) {
        Ok(())
    } else {
        Err(error("VAL_RESULT", "invalid audit result", Some("result")))
    }
}
fn normalize_time_range(
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(Option<String>, Option<String>), String> {
    match (from, to) {
        (None, None) => Ok((None, None)),
        (Some(from), Some(to)) => {
            let from = chrono::DateTime::parse_from_rfc3339(from)
                .map_err(|_| error("VAL_TIME", "from must be ISO 8601", Some("from")))?;
            let to = chrono::DateTime::parse_from_rfc3339(to)
                .map_err(|_| error("VAL_TIME", "to must be ISO 8601", Some("to")))?;
            if to < from || to - from > chrono::Duration::days(31) {
                Err(error(
                    "VAL_TIME_RANGE",
                    "audit range must be ordered and no longer than 31 days",
                    Some("to"),
                ))
            } else {
                Ok((Some(canonical_shanghai(from)), Some(canonical_shanghai(to))))
            }
        }
        _ => Err(error(
            "VAL_TIME_RANGE",
            "from and to must be provided together",
            Some("from"),
        )),
    }
}

fn canonical_shanghai(value: chrono::DateTime<chrono::FixedOffset>) -> String {
    value
        .with_timezone(&chrono_tz::Asia::Shanghai)
        .format("%Y-%m-%dT%H:%M:%S%.3f+08:00")
        .to_string()
}
fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
fn validate_required_text(v: &str, field: &str, max: usize) -> Result<(), String> {
    let n = v.chars().count();
    if n == 0 {
        Err(error("VAL_REQUIRED", "field is required", Some(field)))
    } else if n > max {
        Err(error(
            "VAL_TOO_LONG",
            "field exceeds maximum length",
            Some(field),
        ))
    } else {
        Ok(())
    }
}
fn make_cursor(at: &str, id: &str) -> Result<String, String> {
    let canonical = if let Ok(value) = chrono::DateTime::parse_from_rfc3339(at) {
        canonical_shanghai(value)
    } else {
        let naive = NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M:%S")
            .map_err(|_| error("SYS_TIMESTAMP", "stored timestamp is invalid", None))?;
        let value = chrono_tz::Asia::Shanghai
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| error("SYS_TIMESTAMP", "stored timestamp is ambiguous", None))?;
        value.format("%Y-%m-%dT%H:%M:%S%.3f+08:00").to_string()
    };
    Ok(format!("{}|{}", canonical, id))
}
fn parse_cursor(v: Option<&str>) -> Result<Option<(String, String)>, String> {
    match v {
        None => Ok(None),
        Some(v) => {
            let Some((a, id)) = v.rsplit_once('|') else {
                return Err(error("VAL_CURSOR", "invalid cursor", Some("cursor")));
            };
            if a.is_empty() || a.len() > 64 {
                return Err(error("VAL_CURSOR", "invalid cursor", Some("cursor")));
            }
            chrono::DateTime::parse_from_rfc3339(a)
                .map_err(|_| error("VAL_CURSOR", "invalid cursor timestamp", Some("cursor")))?;
            validate_id(id, "cursor")?;
            Ok(Some((a.into(), id.into())))
        }
    }
}
fn shanghai_now() -> String {
    Utc::now()
        .with_timezone(&chrono_tz::Asia::Shanghai)
        .format("%Y-%m-%dT%H:%M:%S%.3f+08:00")
        .to_string()
}
fn storage_error(_: rusqlite::Error) -> String {
    error("SYS_STORAGE", "governance query failed", None)
}
fn not_found_or_storage(e: rusqlite::Error) -> String {
    if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
        error("NOT_FOUND", "resource not found", None)
    } else {
        storage_error(e)
    }
}
fn error(code: &str, message: &str, field: Option<&str>) -> String {
    serde_json::to_string(&ErrorPayload {
        category: if code.starts_with("AUTH") {
            "auth"
        } else if code.starts_with("VAL") {
            "val"
        } else {
            "sys"
        }
        .into(),
        code: code.into(),
        message: message.into(),
        field: field.map(str::to_owned),
        context: None,
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };
    fn pool() -> Pool<SqliteConnectionManager> {
        let p = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        p.get().unwrap().execute_batch("CREATE TABLE tenants(id TEXT PRIMARY KEY,name TEXT,slug TEXT,status TEXT,plan TEXT,settings TEXT,created_at TEXT,updated_at TEXT);CREATE TABLE audit_logs(id TEXT PRIMARY KEY,actorIdentityId TEXT,actorUsername TEXT,actionType TEXT,entityType TEXT,entityId TEXT,entityLabel TEXT,detailJson TEXT,ip TEXT,userAgent TEXT,createdAt TEXT,tenant_id TEXT);CREATE TABLE change_intents(id TEXT PRIMARY KEY,target_tenant_id TEXT,actor_id TEXT,reason TEXT,intended_outcome TEXT,impact TEXT,cost_minor INTEGER,currency TEXT,source TEXT,correlation_id TEXT,status TEXT,created_at TEXT);INSERT INTO tenants VALUES('tenant-a','A','a','active','pro','{\"secret\":\"hidden\"}','2026-07-18T10:00:00+08:00','2026-07-18T10:00:00+08:00');INSERT INTO tenants VALUES('tenant-b','B','b','suspended','free',NULL,'2026-07-17T10:00:00+08:00','2026-07-17T10:00:00+08:00');INSERT INTO audit_logs VALUES('e-a','u-a','A','login','user','u-a','','{\"result\":\"Succeeded\",\"payload_policy\":\"Full\",\"payload\":{\"passwordHash\":\"leak\",\"ok\":\"visible\"},\"note\":{\"bankCard\":\"also-leak\"}}','','','2026-07-18T11:00:00+08:00','tenant-a');INSERT INTO audit_logs VALUES('e-b','u-b','B','login','user','u-b','','{\"result\":\"Succeeded\",\"payload_policy\":\"ReferenceOnly\",\"payload\":{\"token\":\"leak\"}}','','','2026-07-18T12:00:00+08:00','tenant-b');").unwrap();
        p
    }
    fn ctx(role: &str, platform: bool) -> ExecutionContext {
        let scope = if platform {
            DataScope::platform(Revision::new("r1").unwrap())
        } else {
            DataScope::production(
                TenantId::new("tenant-a").unwrap(),
                Revision::new("r1").unwrap(),
            )
            .unwrap()
        };
        let actor = if platform && role == "platform_owner" {
            ActorIdentity::with_authority(
                "actor",
                system_core::AuthorityContext::Platform {
                    membership_id: system_core::PlatformMembershipId::new("platform-membership")
                        .unwrap(),
                    roles: vec![system_core::PlatformRole::Owner],
                },
            )
            .unwrap()
        } else {
            ActorIdentity::authenticated("actor", role).unwrap()
        };
        ExecutionContext::new(
            actor,
            if platform {
                TenantScope::platform()
            } else {
                TenantScope::tenant(TenantId::new("tenant-a").unwrap())
            },
            scope,
            ExecutionMode::Normal,
            RequestId::new("req-1").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }
    #[test]
    fn requires_platform_authority() {
        let f = FeatureTenantGovernance::with_pool(pool());
        assert!(
            f.execute("tenant.list", json!({}), &ctx("admin", true))
                .is_err()
        );
        assert!(
            f.execute("tenant.list", json!({}), &ctx("platform_owner", false))
                .is_err()
        );
    }
    #[test]
    fn tenant_views_are_allowlisted_and_cover_both_tenants() {
        let f = FeatureTenantGovernance::with_pool(pool());
        let v = f
            .execute("tenant.list", json!({}), &ctx("platform_owner", true))
            .unwrap();
        assert_eq!(v["tenants"].as_array().unwrap().len(), 2);
        assert!(!v.to_string().contains("hidden"));
        assert!(v["tenants"][0].get("settings").is_none());
        let filtered = f
            .execute(
                "tenant.list",
                json!({"search":"A","status":"active"}),
                &ctx("platform_owner", true),
            )
            .unwrap();
        assert_eq!(filtered["tenants"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn cross_tenant_audit_requires_platform_authority_and_redacts_secrets() {
        let f = FeatureTenantGovernance::with_pool(pool());
        let v = f
            .execute(
                "audit.query",
                json!({"from":"2026-07-01T00:00:00+08:00","to":"2026-07-31T23:59:59+08:00"}),
                &ctx("platform_owner", true),
            )
            .unwrap();
        assert_eq!(v["events"].as_array().unwrap().len(), 2);
        let text = v.to_string();
        assert!(!text.contains("leak"));
        assert!(!text.contains("visible"));
        assert!(!text.contains("bankCard"));
        assert_eq!(v["events"][0]["command"], "login");
        assert_eq!(v["events"][0]["result"], "succeeded");
        assert!(v["events"][0].get("occurredAt").is_some());
        assert!(
            f.execute(
                "audit.query",
                json!({"tenantId":"tenant-b"}),
                &ctx("admin", true)
            )
            .is_err()
        );
    }
    #[test]
    fn health_is_allowlisted_and_reports_state_with_server_timestamp() {
        let p = pool();
        p.get().unwrap().execute(
            "INSERT INTO audit_logs VALUES('malformed','u-a','A','legacy','user','u-a','','not-json','','',?1,'tenant-a')",
            params![shanghai_now()],
        ).unwrap();
        let f = FeatureTenantGovernance::with_pool(p);
        let v = f
            .execute(
                "tenant.health",
                json!({"id":"tenant-a"}),
                &ctx("platform_owner", true),
            )
            .unwrap();
        assert_eq!(v["tenantId"], "tenant-a");
        assert_eq!(v["status"], "healthy");
        assert_eq!(v["lifecycleStatus"], "active");
        assert_eq!(v["auditEvents24h"], 1);
        assert!(v["checkedAt"].as_str().unwrap().ends_with("+08:00"));
        assert!(v.get("settings").is_none());
    }
    #[test]
    fn every_governance_command_requires_platform_authority_and_simulation_is_blocked() {
        let f = FeatureTenantGovernance::with_pool(pool());
        let commands = f.commands();
        assert_eq!(commands.len(), 6);
        assert!(
            commands
                .iter()
                .all(|m| m.access == AccessRequirement::Platform)
        );
        assert!(
            commands
                .iter()
                .all(|m| m.simulation == SimulationSupport::Blocked)
        );
    }
    #[test]
    fn records_server_bound_validated_intent() {
        let p = pool();
        let f = FeatureTenantGovernance::with_pool(p.clone());
        let c = ctx("platform_owner", true);
        let out=f.execute("change_intent.record",json!({"targetTenantId":"tenant-b","reason":"capacity","intendedOutcome":"more devices","costMinor":50000,"currency":"cny"}),&c).unwrap();
        assert_eq!(out["actorId"], "actor");
        assert_eq!(out["source"], "platform_governance");
        // Execution audit is appended by ModuleRegistry's AuditSink boundary;
        // the feature crate must not write the legacy audit projection itself.
        let row: (String, String, String, String, String) = p
            .get()
            .unwrap()
            .query_row(
                "SELECT actor_id,target_tenant_id,correlation_id,source,status FROM change_intents",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                "actor".into(),
                "tenant-b".into(),
                "req-1".into(),
                "platform_governance".into(),
                "recorded".into()
            )
        );
        assert!(
            f.execute(
                "change_intent.record",
                json!({"targetTenantId":"tenant-b","reason":"","intendedOutcome":"x"}),
                &c
            )
            .is_err()
        );
    }
    #[test]
    fn cursor_and_limits_fail_closed() {
        let p = pool();
        p.get().unwrap().execute(
            "INSERT INTO tenants VALUES('tenant-legacy','Legacy','legacy','active','free',NULL,'2026-08-01 10:00:00','2026-08-01 10:00:00')",
            [],
        ).unwrap();
        let f = FeatureTenantGovernance::with_pool(p);
        let c = ctx("platform_owner", true);
        let first = f.execute("tenant.list", json!({"limit":1}), &c).unwrap();
        let cursor = first["nextCursor"].as_str().unwrap();
        assert!(cursor.contains("T10:00:00.000+08:00"));
        let second = f
            .execute("tenant.list", json!({"limit":1,"cursor":cursor}), &c)
            .unwrap();
        assert_ne!(first["tenants"][0]["id"], second["tenants"][0]["id"]);
        assert!(
            f.execute(
                "audit.query",
                json!({"tenantId":"tenant-a","limit":201}),
                &c
            )
            .is_err()
        );
        assert!(
            f.execute("tenant.list", json!({"cursor":"bad"}), &c)
                .is_err()
        );
        assert!(f.execute("audit.query", json!({}), &c).is_err());
        assert!(
            f.execute(
                "audit.query",
                json!({"tenantId":"tenant-a","cursor":"zzz|event-1"}),
                &c
            )
            .is_err()
        );
        assert!(
            f.execute(
                "audit.query",
                json!({"from":"2026-01-01T00:00:00+08:00","to":"2026-03-01T00:00:00+08:00"}),
                &c
            )
            .is_err()
        );
        assert_eq!(
            normalize_result(Some(&json!({"Failed":{"error_code":"AUTH_FORBIDDEN"}}))),
            "denied"
        );
        assert_eq!(
            normalize_result(Some(&json!({"Failed":{"error_code":"DB_FAILURE"}}))),
            "failed"
        );
        assert_eq!(normalize_result(Some(&json!("Attempted"))), "attempted");
    }
}

use chrono::{NaiveDateTime, TimeZone};
use feature_tenant_governance::FeatureTenantGovernance;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};
use uuid::Uuid;

use crate::repositories::{
    GovernanceAuditQuery, GovernanceAuditRecord, GovernanceChangeIntent, GovernanceMutationError,
    GovernanceTenantListQuery, RepositoryError, TenantGovernanceRepository,
};
use crate::utils::time::shanghai_now_iso;

const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;

#[derive(Clone)]
pub(crate) struct TenantGovernanceCompatibilityModule {
    repository: TenantGovernanceRepository,
}

impl TenantGovernanceCompatibilityModule {
    pub(crate) fn new(repository: TenantGovernanceRepository) -> Self {
        Self { repository }
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

        let mut tenants = self
            .repository
            .tenant_list(GovernanceTenantListQuery {
                search: input.search,
                status: input.status,
                cursor_at: cursor.as_ref().map(|value| value.0.clone()),
                cursor_id: cursor.as_ref().map(|value| value.1.clone()),
                limit_plus_one: i64::from(limit + 1),
            })
            .map_err(repository_error)?;
        let has_more = tenants.len() > limit as usize;
        tenants.truncate(limit as usize);
        let next_cursor = if has_more {
            tenants
                .last()
                .map(|tenant| make_cursor(&tenant.created_at, &tenant.id))
                .transpose()?
        } else {
            None
        };
        Ok(json!({"tenants": tenants, "nextCursor": next_cursor}))
    }

    fn tenant_get(&self, payload: Value) -> Result<Value, String> {
        let input: IdInput = decode(payload)?;
        validate_id(&input.id, "id")?;
        self.repository
            .tenant_get(&input.id)
            .map_err(repository_error)?
            .map(|tenant| json!(tenant))
            .ok_or_else(not_found)
    }

    fn tenant_health(&self, payload: Value) -> Result<Value, String> {
        let input: IdInput = decode(payload)?;
        validate_id(&input.id, "id")?;
        let now = shanghai_now_iso();
        let instant = chrono::DateTime::parse_from_rfc3339(&now)
            .map_err(|_| error("SYS_TIMESTAMP", "server timestamp is invalid", None))?;
        let since = canonical_shanghai(instant - chrono::Duration::hours(24));

        let health = self
            .repository
            .tenant_health(&input.id, &since)
            .map_err(repository_error)?
            .ok_or_else(not_found)?;

        let status = if health.lifecycle_status != "active" {
            "unavailable"
        } else if health.failed_command_count > 0 {
            "degraded"
        } else if health.audit_events_24h == 0 {
            "unknown"
        } else {
            "healthy"
        };
        let mut signals = Vec::new();
        if health.lifecycle_status != "active" {
            signals.push(HealthSignal::new(
                "tenant_not_active",
                "Tenant lifecycle",
                "unavailable",
                Some(format!("lifecycle status is {}", health.lifecycle_status)),
            ));
        }
        if health.failed_command_count > 0 {
            signals.push(HealthSignal::new(
                "failed_commands_24h",
                "Failed commands (24h)",
                "degraded",
                Some(format!(
                    "{} failed command(s) observed",
                    health.failed_command_count
                )),
            ));
        }
        if health.audit_events_24h == 0 {
            signals.push(HealthSignal::new(
                "no_recent_activity",
                "Recent activity",
                "unknown",
                Some("no audit activity observed in the last 24 hours".to_owned()),
            ));
        }

        Ok(json!(TenantHealth {
            tenant_id: health.tenant_id,
            status: status.to_owned(),
            lifecycle_status: health.lifecycle_status,
            last_activity_at: health.last_activity_at,
            audit_events_24h: health.audit_events_24h,
            failed_command_count: health.failed_command_count,
            checked_at: now,
            signals,
        }))
    }

    fn audit_query(&self, payload: Value) -> Result<Value, String> {
        let input: AuditQueryInput = decode(payload)?;
        let limit = bounded_limit(input.limit)?;
        if let Some(value) = &input.tenant_id {
            validate_id(value, "tenantId")?;
        }
        if let Some(value) = &input.actor {
            validate_filter(value, "actor")?;
        }
        if let Some(value) = &input.command {
            validate_filter(value, "command")?;
        }
        if let Some(value) = &input.result {
            validate_audit_result(value)?;
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

        let mut events = self
            .repository
            .audit_query(GovernanceAuditQuery {
                tenant_id: input.tenant_id,
                actor: input.actor,
                command: input.command,
                result: input.result,
                from,
                to,
                cursor_at: cursor.as_ref().map(|value| value.0.clone()),
                cursor_id: cursor.as_ref().map(|value| value.1.clone()),
                limit_plus_one: i64::from(limit + 1),
            })
            .map_err(repository_error)?
            .into_iter()
            .map(project_audit)
            .collect::<Vec<_>>();
        let has_more = events.len() > limit as usize;
        events.truncate(limit as usize);
        let next_cursor = if has_more {
            events
                .last()
                .map(|event| make_cursor(&event.occurred_at, &event.id))
                .transpose()?
        } else {
            None
        };
        Ok(json!({"events": events, "nextCursor": next_cursor}))
    }

    fn audit_get(&self, payload: Value) -> Result<Value, String> {
        let input: IdInput = decode(payload)?;
        validate_id(&input.id, "id")?;
        self.repository
            .audit_get(&input.id)
            .map_err(repository_error)?
            .map(project_audit)
            .map(|event| json!(event))
            .ok_or_else(not_found)
    }

    fn change_intent_record(
        &self,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let mut input: ChangeIntentInput = decode(payload)?;
        input.trim();
        input.validate()?;
        let actor_id = ctx
            .actor()
            .id()
            .ok_or_else(|| error("AUTH_FORBIDDEN", "authenticated actor required", None))?;
        let id = Uuid::new_v4().to_string();
        let created_at = shanghai_now_iso();
        let target_tenant_id = input.target_tenant_id.clone();
        let correlation_id = ctx.correlation_id().as_str().to_owned();

        self.repository
            .record_change_intent(GovernanceChangeIntent {
                id: id.clone(),
                target_tenant_id: target_tenant_id.clone(),
                actor_id: actor_id.to_owned(),
                reason: input.reason,
                intended_outcome: input.intended_outcome,
                impact: input.impact,
                cost_minor: input.cost_minor,
                currency: input.currency,
                correlation_id: correlation_id.clone(),
                created_at: created_at.clone(),
            })
            .map_err(mutation_error)?;

        Ok(json!({
            "id": id,
            "actorId": actor_id,
            "targetTenantId": target_tenant_id,
            "source": "platform_governance",
            "status": "recorded",
            "correlationId": correlation_id,
            "createdAt": created_at,
        }))
    }
}

impl SystemModule for TenantGovernanceCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureTenantGovernance::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureTenantGovernance::new().commands()
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
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
        FeatureTenantGovernance::new().schema()
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
        self.currency = self
            .currency
            .take()
            .map(|value| value.trim().to_ascii_uppercase());
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
        if let Some(currency) = &self.currency
            && (currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()))
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

fn project_audit(record: GovernanceAuditRecord) -> AuditView {
    let raw = record.detail_json.unwrap_or_else(|| "{}".into());
    let detail = redact_audit_detail(serde_json::from_str(&raw).unwrap_or(Value::Null));
    let command = detail
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or(record.action_type);
    let actor_id = detail
        .pointer("/actor/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or(record.actor_identity_id.unwrap_or_default());
    let occurred_at = detail
        .get("occurred_at")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or(record.created_at);
    let correlation_id = detail
        .get("correlation_id")
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

    AuditView {
        id: record.id,
        tenant_id: record.tenant_id,
        actor_id,
        command,
        result,
        correlation_id,
        source,
        change_intent_id,
        occurred_at,
        detail,
    }
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
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                let normalized = key.to_ascii_lowercase();
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
                .any(|candidate| normalized.contains(candidate))
                {
                    *value = Value::String("[REDACTED]".into());
                } else {
                    redact_secrets(value);
                }
            }
        }
        Value::Array(values) => values.iter_mut().for_each(redact_secrets),
        _ => {}
    }
}

fn decode<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|_| error("VAL_INPUT", "invalid command input", None))
}

fn bounded_limit(limit: Option<u32>) -> Result<u32, String> {
    let value = limit.unwrap_or(DEFAULT_LIMIT);
    if value == 0 || value > MAX_LIMIT {
        Err(error(
            "VAL_LIMIT",
            "limit must be between 1 and 200",
            Some("limit"),
        ))
    } else {
        Ok(value)
    }
}

fn validate_id(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.:".contains(&byte))
    {
        Err(error("VAL_ID", "invalid identifier", Some(field)))
    } else {
        Ok(())
    }
}

fn validate_filter(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 100 {
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

fn validate_required_text(value: &str, field: &str, max: usize) -> Result<(), String> {
    let count = value.chars().count();
    if count == 0 {
        Err(error("VAL_REQUIRED", "field is required", Some(field)))
    } else if count > max {
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
    Ok(format!("{canonical}|{id}"))
}

fn parse_cursor(value: Option<&str>) -> Result<Option<(String, String)>, String> {
    match value {
        None => Ok(None),
        Some(value) => {
            let Some((at, id)) = value.rsplit_once('|') else {
                return Err(error("VAL_CURSOR", "invalid cursor", Some("cursor")));
            };
            if at.is_empty() || at.len() > 64 {
                return Err(error("VAL_CURSOR", "invalid cursor", Some("cursor")));
            }
            chrono::DateTime::parse_from_rfc3339(at)
                .map_err(|_| error("VAL_CURSOR", "invalid cursor timestamp", Some("cursor")))?;
            validate_id(id, "cursor")?;
            Ok(Some((at.into(), id.into())))
        }
    }
}

fn repository_error(_error: RepositoryError) -> String {
    error("SYS_STORAGE", "governance query failed", None)
}

fn mutation_error(error_value: GovernanceMutationError) -> String {
    match error_value {
        GovernanceMutationError::TenantNotFound => not_found(),
        GovernanceMutationError::Storage(error_value) => repository_error(error_value),
    }
}

fn not_found() -> String {
    error("NOT_FOUND", "resource not found", None)
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
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        AccessRequirement, ActorIdentity, AuthorityContext, DataScope, ExecutionContext,
        ExecutionMode, NoopHttpClient, PlatformMembershipId, PlatformRole, RequestId, Revision,
        SimulationSupport, TenantId, TenantScope,
    };

    use super::*;

    fn pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        pool.get()
            .unwrap()
            .execute_batch(
                "CREATE TABLE tenants(
                    id TEXT PRIMARY KEY,name TEXT,slug TEXT,status TEXT,plan TEXT,settings TEXT,
                    created_at TEXT,updated_at TEXT
                );
                CREATE TABLE audit_logs(
                    id TEXT PRIMARY KEY,actorIdentityId TEXT,actorUsername TEXT,actionType TEXT,
                    entityType TEXT,entityId TEXT,entityLabel TEXT,detailJson TEXT,ip TEXT,
                    userAgent TEXT,createdAt TEXT,tenant_id TEXT
                );
                CREATE TABLE change_intents(
                    id TEXT PRIMARY KEY,target_tenant_id TEXT,actor_id TEXT,reason TEXT,
                    intended_outcome TEXT,impact TEXT,cost_minor INTEGER,currency TEXT,source TEXT,
                    correlation_id TEXT,status TEXT,created_at TEXT
                );
                CREATE TRIGGER trg_test_change_intents_no_update
                BEFORE UPDATE ON change_intents
                BEGIN SELECT RAISE(ABORT, 'change_intents are append-only'); END;
                CREATE TRIGGER trg_test_change_intents_no_delete
                BEFORE DELETE ON change_intents
                BEGIN SELECT RAISE(ABORT, 'change_intents are append-only'); END;
                INSERT INTO tenants VALUES
                    ('tenant-a','A','a','active','pro','{\"secret\":\"hidden\"}',
                     '2026-07-18T10:00:00+08:00','2026-07-18T10:00:00+08:00'),
                    ('tenant-b','B','b','suspended','free',NULL,
                     '2026-07-17T10:00:00+08:00','2026-07-17T10:00:00+08:00');
                INSERT INTO audit_logs VALUES
                    ('e-a','u-a','A','login','user','u-a','',
                     '{\"result\":\"Succeeded\",\"payload_policy\":\"Full\",\"payload\":{\"passwordHash\":\"leak\",\"ok\":\"visible\"},\"note\":{\"bankCard\":\"also-leak\"}}',
                     '','','2026-07-18T11:00:00+08:00','tenant-a'),
                    ('e-b','u-b','B','login','user','u-b','',
                     '{\"result\":\"Succeeded\",\"payload_policy\":\"ReferenceOnly\",\"payload\":{\"token\":\"leak\"}}',
                     '','','2026-07-18T12:00:00+08:00','tenant-b');",
            )
            .unwrap();
        pool
    }

    fn context(platform: bool) -> ExecutionContext {
        let scope = if platform {
            DataScope::platform(Revision::new("governance-revision").unwrap())
        } else {
            DataScope::production(
                TenantId::new("tenant-a").unwrap(),
                Revision::new("governance-revision").unwrap(),
            )
            .unwrap()
        };
        let actor = if platform {
            ActorIdentity::with_authority(
                "governance-actor",
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap()
        } else {
            ActorIdentity::authenticated("governance-actor", "admin").unwrap()
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
            RequestId::new("governance-request").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_tenant_governance_preserves_platform_scope_redaction_health_and_intents() {
        let pool = pool();
        let module =
            TenantGovernanceCompatibilityModule::new(TenantGovernanceRepository::new(pool.clone()));
        let platform = context(true);

        assert!(
            module
                .execute("tenant.list", json!({}), &context(false))
                .is_err()
        );

        let first = module
            .execute("tenant.list", json!({"limit": 1}), &platform)
            .unwrap();
        assert_eq!(first["tenants"].as_array().unwrap().len(), 1);
        assert!(first["tenants"][0].get("settings").is_none());
        let cursor = first["nextCursor"].as_str().unwrap();
        let second = module
            .execute(
                "tenant.list",
                json!({"limit": 1, "cursor": cursor}),
                &platform,
            )
            .unwrap();
        assert_ne!(first["tenants"][0]["id"], second["tenants"][0]["id"]);

        let audit = module
            .execute("audit.query", json!({"tenantId": "tenant-a"}), &platform)
            .unwrap();
        assert_eq!(audit["events"].as_array().unwrap().len(), 1);
        let audit_text = audit.to_string();
        assert!(!audit_text.contains("passwordHash"));
        assert!(!audit_text.contains("bankCard"));
        assert!(!audit_text.contains("leak"));

        let now = shanghai_now_iso();
        pool.get()
            .unwrap()
            .execute(
                "INSERT INTO audit_logs VALUES(
                    'e-now','u-a','A','health_probe','system','probe','',
                    '{\"result\":\"Succeeded\"}','','',?1,'tenant-a'
                )",
                [&now],
            )
            .unwrap();
        let health = module
            .execute("tenant.health", json!({"id": "tenant-a"}), &platform)
            .unwrap();
        assert_eq!(health["status"], "healthy");
        assert_eq!(health["lifecycleStatus"], "active");
        assert_eq!(health["auditEvents24h"], 1);
        assert!(health["checkedAt"].as_str().unwrap().ends_with("+08:00"));

        let intent = module
            .execute(
                "change_intent.record",
                json!({
                    "targetTenantId": "tenant-b",
                    "reason": " capacity ",
                    "intendedOutcome": " more devices ",
                    "costMinor": 50000,
                    "currency": "cny"
                }),
                &platform,
            )
            .unwrap();
        assert_eq!(intent["actorId"], "governance-actor");
        assert_eq!(intent["targetTenantId"], "tenant-b");
        assert_eq!(intent["source"], "platform_governance");
        assert_eq!(intent["status"], "recorded");

        let connection = pool.get().unwrap();
        let stored: (String, String, String, String, String) = connection
            .query_row(
                "SELECT actor_id,target_tenant_id,reason,currency,status FROM change_intents",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            stored,
            (
                "governance-actor".into(),
                "tenant-b".into(),
                "capacity".into(),
                "CNY".into(),
                "recorded".into(),
            )
        );
        assert!(
            connection
                .execute("UPDATE change_intents SET reason='rewritten'", [])
                .is_err()
        );

        let commands = module.commands();
        assert_eq!(commands.len(), 6);
        assert!(
            commands
                .iter()
                .all(|command| command.access == AccessRequirement::Platform)
        );
        assert!(
            commands
                .iter()
                .all(|command| command.simulation == SimulationSupport::Blocked)
        );
    }
}

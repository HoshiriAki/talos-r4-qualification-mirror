//! Read-only tenant preview plane.
//!
//! Preview sessions are durable control-plane aggregates. Business data is
//! always read through the request-scoped `ExecutionContext` supplied by the
//! host Registry; this module never impersonates a tenant user.

use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Asia::Shanghai;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ExecutionMode, ExecutionPlane, ModuleMetadata, ModuleSchema, PlatformCapability,
    SimulationSupport, SystemModule,
};

const MIN_TTL_MINUTES: i64 = 1;
const MAX_TTL_MINUTES: i64 = 60;

pub struct FeatureTenantPreview {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureTenantPreview {
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
        if std::env::var("DB_BACKEND")
            .map(|value| value.eq_ignore_ascii_case("postgres"))
            .unwrap_or(false)
        {
            return Err(error(
                "SYS_BACKEND_UNSUPPORTED",
                "tenant preview is not available on PostgreSQL yet",
            ));
        }
        self.pool
            .lock()
            .map_err(|_| error("SYS_LOCK", "preview pool lock failed"))?
            .clone()
            .ok_or_else(|| error("SYS_NO_DB", "preview database is not configured"))
    }

    fn require_platform_authority(ctx: &ExecutionContext) -> Result<(), String> {
        if !ctx.actor().has_platform_authority()
            || !ctx.data_scope().is_platform()
            || ctx.execution_mode() != &ExecutionMode::Normal
        {
            return Err(error(
                "AUTH_PLATFORM_REQUIRED",
                "platform authority context required",
            ));
        }
        Ok(())
    }

    fn create_session(
        &self,
        input: CreateSessionInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        Self::require_platform_authority(ctx)?;
        let tenant_id = non_blank(&input.tenant_id, "tenantId")?;
        let ttl = input.ttl_minutes.unwrap_or(15);
        if !(MIN_TTL_MINUTES..=MAX_TTL_MINUTES).contains(&ttl) {
            return Err(error("VAL_TTL", "ttlMinutes must be between 1 and 60"));
        }
        let actor_id = ctx
            .actor()
            .id()
            .ok_or_else(|| error("AUTH_ACTOR_REQUIRED", "actor id required"))?;
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|e| error("SYS_DB_CONN", &e.to_string()))?;
        let active: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tenants WHERE id = ?1 AND status = 'active')",
                [tenant_id],
                |row| row.get(0),
            )
            .map_err(|e| error("SYS_DB_QUERY", &e.to_string()))?;
        if !active {
            return Err(error("NOT_FOUND", "active tenant not found"));
        }
        let mode = input.mode.unwrap_or(WorkspaceMode::Preview);
        let simulation_id = match mode {
            WorkspaceMode::Simulation => {
                let simulation_id = non_blank(
                    input.simulation_id.as_deref().unwrap_or_default(),
                    "simulationId",
                )?;
                let exists: bool = conn
                    .query_row(
                        "SELECT EXISTS(
                           SELECT 1 FROM simulation_sessions
                           WHERE id = ?1 AND actor_id = ?2 AND target_tenant_id = ?3
                             AND status = 'active'
                         )",
                        params![simulation_id, actor_id, tenant_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| error("SYS_DB_QUERY", &e.to_string()))?;
                if !exists {
                    return Err(error(
                        "SIMULATION_NOT_FOUND",
                        "active actor-owned simulation was not found",
                    ));
                }
                Some(simulation_id.to_string())
            }
            _ => None,
        };
        let capabilities = match mode {
            WorkspaceMode::Preview => vec!["preview.read".into()],
            WorkspaceMode::Diagnostics => {
                vec!["preview.read".into(), "diagnostics.read".into()]
            }
            WorkspaceMode::Simulation => {
                vec!["simulation.read".into(), "simulation.execute".into()]
            }
        };

        let now = Utc::now().with_timezone(&Shanghai);
        let session = PreviewSession {
            id: uuid::Uuid::new_v4().to_string(),
            actor_id: actor_id.to_owned(),
            target_tenant_id: tenant_id.to_owned(),
            tenant_name: conn
                .query_row(
                    "SELECT name FROM tenants WHERE id = ?1",
                    [tenant_id],
                    |row| row.get(0),
                )
                .map_err(|e| error("SYS_DB_QUERY", &e.to_string()))?,
            tenant_slug: conn
                .query_row(
                    "SELECT slug FROM tenants WHERE id = ?1",
                    [tenant_id],
                    |row| row.get(0),
                )
                .map_err(|e| error("SYS_DB_QUERY", &e.to_string()))?,
            status: "active".into(),
            created_at: now.to_rfc3339(),
            expires_at: (now + Duration::minutes(ttl)).to_rfc3339(),
            ended_at: None,
            mode,
            simulation_id,
            capabilities,
        };
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| error("SYS_DB_WRITE", &e.to_string()))?;
        tx.execute(
            "INSERT INTO tenant_preview_sessions
             (id, actor_id, target_tenant_id, status, created_at, expires_at, ended_at)
             VALUES (?1, ?2, ?3, 'active', ?4, ?5, NULL)",
            params![
                session.id,
                session.actor_id,
                session.target_tenant_id,
                session.created_at,
                session.expires_at
            ],
        )
        .map_err(|e| error("SYS_DB_WRITE", &e.to_string()))?;
        tx.execute(
            "INSERT INTO tenant_workspace_sessions
             (id, actor_identity_id, tenant_id, mode, preview_session_id,
              simulation_id, capabilities_json, status, created_at, expires_at, ended_at)
             VALUES (?1, ?2, ?3, ?4, ?1, ?5, ?6, 'active', ?7, ?8, NULL)",
            params![
                session.id,
                session.actor_id,
                session.target_tenant_id,
                session.mode.as_str(),
                session.simulation_id,
                serde_json::to_string(&session.capabilities)
                    .map_err(|e| error("SYS_SERIALIZE", &e.to_string()))?,
                session.created_at,
                session.expires_at
            ],
        )
        .map_err(|e| error("SYS_DB_WRITE", &e.to_string()))?;
        tx.commit()
            .map_err(|e| error("SYS_DB_WRITE", &e.to_string()))?;
        serde_json::to_value(session).map_err(|e| error("SYS_SERIALIZE", &e.to_string()))
    }

    fn get_session(&self, input: SessionInput, ctx: &ExecutionContext) -> Result<Value, String> {
        Self::require_platform_authority(ctx)?;
        let actor = ctx
            .actor()
            .id()
            .ok_or_else(|| error("AUTH_ACTOR_REQUIRED", "actor id required"))?;
        let session = self.load_owned(&input.id, actor)?;
        if session.status == "active" {
            let expiry = DateTime::parse_from_rfc3339(&session.expires_at).map_err(|_| {
                error(
                    "PREVIEW_SESSION_INVALID",
                    "preview session expiry is invalid",
                )
            })?;
            if expiry <= Utc::now() {
                return Err(error("PREVIEW_SESSION_EXPIRED", "preview session expired"));
            }
            let pool = self.pool()?;
            let conn = pool
                .get()
                .map_err(|e| error("SYS_DB_CONN", &e.to_string()))?;
            let active: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM tenants WHERE id = ?1 AND status = 'active')",
                    [&session.target_tenant_id],
                    |row| row.get(0),
                )
                .map_err(|e| error("SYS_DB_QUERY", &e.to_string()))?;
            if !active {
                return Err(error(
                    "PREVIEW_TENANT_INACTIVE",
                    "preview tenant is not active",
                ));
            }
        }
        serde_json::to_value(session).map_err(|e| error("SYS_SERIALIZE", &e.to_string()))
    }

    fn end_session(&self, input: SessionInput, ctx: &ExecutionContext) -> Result<Value, String> {
        Self::require_platform_authority(ctx)?;
        let actor = ctx
            .actor()
            .id()
            .ok_or_else(|| error("AUTH_ACTOR_REQUIRED", "actor id required"))?;
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|e| error("SYS_DB_CONN", &e.to_string()))?;
        let owned: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tenant_preview_sessions WHERE id = ?1 AND actor_id = ?2)",
            params![input.id, actor], |row| row.get(0),
        ).map_err(|e| error("SYS_DB_QUERY", &e.to_string()))?;
        if !owned {
            return Err(error("NOT_FOUND", "preview session not found"));
        }
        let now = Utc::now().with_timezone(&Shanghai).to_rfc3339();
        conn.execute(
            "UPDATE tenant_preview_sessions SET status = 'ended', ended_at = COALESCE(ended_at, ?3)
             WHERE id = ?1 AND actor_id = ?2 AND status = 'active'",
            params![input.id, actor, now],
        )
        .map_err(|e| error("SYS_DB_WRITE", &e.to_string()))?;
        conn.execute(
            "UPDATE tenant_workspace_sessions
             SET status = 'ended', ended_at = COALESCE(ended_at, ?3)
             WHERE id = ?1 AND actor_identity_id = ?2 AND status = 'active'",
            params![input.id, actor, now],
        )
        .map_err(|e| error("SYS_DB_WRITE", &e.to_string()))?;
        let session = load_session(&conn, &input.id, actor)?;
        serde_json::to_value(session).map_err(|e| error("SYS_SERIALIZE", &e.to_string()))
    }

    fn resolve_session(
        &self,
        input: SessionInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        Self::require_platform_authority(ctx)?;
        let actor = ctx
            .actor()
            .id()
            .ok_or_else(|| error("AUTH_ACTOR_REQUIRED", "actor id required"))?;
        let session = self.load_owned(&input.id, actor)?;
        if session.status != "active" {
            return Err(error(
                "PREVIEW_SESSION_INACTIVE",
                "preview session is not active",
            ));
        }
        let expiry = DateTime::parse_from_rfc3339(&session.expires_at).map_err(|_| {
            error(
                "PREVIEW_SESSION_INVALID",
                "preview session expiry is invalid",
            )
        })?;
        if expiry <= Utc::now() {
            return Err(error("PREVIEW_SESSION_EXPIRED", "preview session expired"));
        }
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|e| error("SYS_DB_CONN", &e.to_string()))?;
        let active: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tenants WHERE id = ?1 AND status = 'active')",
                [&session.target_tenant_id],
                |row| row.get(0),
            )
            .map_err(|e| error("SYS_DB_QUERY", &e.to_string()))?;
        if !active {
            return Err(error(
                "PREVIEW_TENANT_INACTIVE",
                "preview tenant is not active",
            ));
        }
        Ok(json!({
            "sessionId": session.id,
            "tenantId": session.target_tenant_id,
            "expiresAt": session.expires_at,
            "mode": session.mode,
            "simulationId": session.simulation_id,
            "capabilities": session.capabilities,
        }))
    }

    fn load_owned(&self, id: &str, actor: &str) -> Result<PreviewSession, String> {
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|e| error("SYS_DB_CONN", &e.to_string()))?;
        load_session(&conn, non_blank(id, "id")?, actor)
    }

    fn dashboard_summary(&self, ctx: &ExecutionContext) -> Result<Value, String> {
        let preview = matches!(ctx.execution_mode(), ExecutionMode::ReadOnlyPreview(_));
        let diagnostics = matches!(ctx.execution_mode(), ExecutionMode::Normal)
            && ctx.plane() == ExecutionPlane::TenantBusiness
            && ctx
                .actor()
                .has_platform_capability(PlatformCapability::TenantDiagnosticsRead);
        if (!preview && !diagnostics) || !ctx.actor().has_platform_authority() {
            return Err(error(
                "AUTH_PREVIEW_REQUIRED",
                "read-only platform preview context required",
            ));
        }
        let pool = self.pool()?;
        let conn = pool
            .get()
            .map_err(|e| error("SYS_DB_CONN", &e.to_string()))?;
        let tenant = ctx.data_scope().tenant_id().as_str();
        let count = |sql: &str| -> Result<i64, String> {
            conn.query_row(sql, [tenant], |row| row.get(0))
                .map_err(|e| error("SYS_DB_QUERY", &e.to_string()))
        };
        Ok(json!({
            "orders": count("SELECT COUNT(*) FROM orders WHERE tenant_id = ?1")?,
            "activeOrders": count("SELECT COUNT(*) FROM orders WHERE tenant_id = ?1 AND status NOT IN ('completed','closed','cancelled')")?,
            "devices": count("SELECT COUNT(*) FROM devices WHERE tenant_id = ?1")?,
            "availableDevices": count("SELECT COUNT(*) FROM devices WHERE tenant_id = ?1 AND rentalStatus IN ('available','idle','已入库')")?,
        }))
    }
}

impl Default for FeatureTenantPreview {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureTenantPreview {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "feature-tenant-preview".into(),
            version: "0.1.0".into(),
            description: "Read-only tenant preview plane".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        if self
            .pool
            .lock()
            .map_err(|_| error("SYS_LOCK", "preview pool lock failed"))?
            .is_some()
        {
            return Ok(());
        }
        if let Some(path) = config.get("databaseUrl").and_then(Value::as_str) {
            let pool = Pool::builder()
                .max_size(5)
                .build(SqliteConnectionManager::file(path))
                .map_err(|e| error("SYS_DB_POOL", &e.to_string()))?;
            *self
                .pool
                .lock()
                .map_err(|_| error("SYS_LOCK", "preview pool lock failed"))? = Some(pool);
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
                "workspace.create_preview",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "workspace.create_diagnostics",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "workspace.create_simulation",
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
                "session.end",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "session.resolve",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "dashboard.summary",
                AccessRequirement::Platform,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
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
            "session.create"
            | "workspace.create_preview"
            | "workspace.create_diagnostics"
            | "workspace.create_simulation" => self.create_session(parse(payload)?, ctx),
            "session.get" => self.get_session(parse(payload)?, ctx),
            "session.end" => self.end_session(parse(payload)?, ctx),
            "session.resolve" => self.resolve_session(parse(payload)?, ctx),
            "dashboard.summary" => self.dashboard_summary(ctx),
            _ => Err(error(
                "SYS_UNKNOWN_COMMAND",
                "unknown tenant preview command",
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "feature-tenant-preview".into(),
            description: "Read-only tenant preview plane".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|m| CommandSchema {
                    name: m.name.into(),
                    description: "tenant preview command".into(),
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
struct CreateSessionInput {
    tenant_id: String,
    ttl_minutes: Option<i64>,
    mode: Option<WorkspaceMode>,
    simulation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMode {
    Preview,
    Diagnostics,
    Simulation,
}

impl WorkspaceMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Diagnostics => "diagnostics",
            Self::Simulation => "simulation",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionInput {
    id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSession {
    pub id: String,
    #[serde(skip_serializing)]
    pub actor_id: String,
    #[serde(rename = "tenantId")]
    pub target_tenant_id: String,
    pub tenant_name: String,
    pub tenant_slug: String,
    pub status: String,
    pub created_at: String,
    pub expires_at: String,
    pub ended_at: Option<String>,
    pub mode: WorkspaceMode,
    pub simulation_id: Option<String>,
    pub capabilities: Vec<String>,
}

fn load_session(
    conn: &rusqlite::Connection,
    id: &str,
    actor: &str,
) -> Result<PreviewSession, String> {
    conn.query_row(
        "SELECT s.id, s.actor_id, s.target_tenant_id, t.name, t.slug,
                s.status, s.created_at, s.expires_at, s.ended_at,
                w.mode, w.simulation_id, w.capabilities_json
         FROM tenant_preview_sessions s
         JOIN tenants t ON t.id = s.target_tenant_id
         JOIN tenant_workspace_sessions w ON w.preview_session_id = s.id
         WHERE s.id = ?1 AND s.actor_id = ?2",
        params![id, actor],
        |row| {
            Ok(PreviewSession {
                id: row.get(0)?,
                actor_id: row.get(1)?,
                target_tenant_id: row.get(2)?,
                tenant_name: row.get(3)?,
                tenant_slug: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                expires_at: row.get(7)?,
                ended_at: row.get(8)?,
                mode: match row.get::<_, String>(9)?.as_str() {
                    "preview" => WorkspaceMode::Preview,
                    "diagnostics" => WorkspaceMode::Diagnostics,
                    "simulation" => WorkspaceMode::Simulation,
                    _ => return Err(rusqlite::Error::InvalidQuery),
                },
                simulation_id: row.get(10)?,
                capabilities: serde_json::from_str::<Vec<String>>(&row.get::<_, String>(11)?)
                    .unwrap_or_default(),
            })
        },
    )
    .optional()
    .map_err(|e| error("SYS_DB_QUERY", &e.to_string()))?
    .ok_or_else(|| error("NOT_FOUND", "preview session not found"))
}

fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| error("VAL_INPUT", &e.to_string()))
}

fn non_blank<'a>(value: &'a str, field: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(error("VAL_REQUIRED", &format!("{field} is required")))
    } else {
        Ok(value)
    }
}

fn error(code: &str, message: &str) -> String {
    serde_json::to_string(&ErrorPayload {
        category: if code.starts_with("AUTH_") || code.starts_with("PREVIEW_") {
            "auth"
        } else if code.starts_with("VAL_") {
            "validation"
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
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use serde_json::{Value, json};
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, SystemModule, TenantId, TenantScope,
    };

    use super::FeatureTenantPreview;

    fn pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        pool.get().unwrap().execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE tenants (id TEXT PRIMARY KEY, name TEXT NOT NULL, slug TEXT NOT NULL, status TEXT NOT NULL);
             CREATE TABLE orders (id TEXT PRIMARY KEY, status TEXT NOT NULL, tenant_id TEXT NOT NULL);
             CREATE TABLE devices (id TEXT PRIMARY KEY, rentalStatus TEXT NOT NULL, tenant_id TEXT NOT NULL);
             CREATE TABLE tenant_preview_sessions (
                id TEXT PRIMARY KEY, actor_id TEXT NOT NULL, target_tenant_id TEXT NOT NULL,
                status TEXT NOT NULL, created_at TEXT NOT NULL, expires_at TEXT NOT NULL, ended_at TEXT
             );
             CREATE TABLE tenant_workspace_sessions (
                id TEXT PRIMARY KEY, actor_identity_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
                mode TEXT NOT NULL, preview_session_id TEXT, simulation_id TEXT,
                capabilities_json TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL, ended_at TEXT
             );
             CREATE TABLE simulation_sessions (
                id TEXT PRIMARY KEY, actor_id TEXT NOT NULL, target_tenant_id TEXT NOT NULL,
                status TEXT NOT NULL
             );
             INSERT INTO tenants VALUES ('tenant-a','Tenant A','tenant-a','active');
             INSERT INTO tenants VALUES ('tenant-b','Tenant B','tenant-b','active');
             INSERT INTO orders VALUES ('order-a','draft','tenant-a'), ('order-b','completed','tenant-b');
             INSERT INTO devices VALUES ('device-a','已入库','tenant-a'), ('device-b','租赁中','tenant-b');"
        ).unwrap();
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
            DataScope::platform(Revision::new("r1").unwrap()),
            ExecutionMode::Normal,
            RequestId::new(format!("req-{actor}")).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn preview(actor: &str, tenant: &str) -> ExecutionContext {
        let tenant = TenantId::new(tenant).unwrap();
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
            DataScope::production(tenant, Revision::new("r1").unwrap()).unwrap(),
            ExecutionMode::ReadOnlyPreview(
                system_core::PreviewSessionId::new("preview-1").unwrap(),
            ),
            RequestId::new(format!("req-preview-{actor}")).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn diagnostics(actor: &str, tenant: &str) -> ExecutionContext {
        let tenant = TenantId::new(tenant).unwrap();
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
            DataScope::production(tenant, Revision::new("diagnostics-r1").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(format!("req-diagnostics-{actor}")).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn create(module: &FeatureTenantPreview, actor: &str, tenant: &str) -> Value {
        module
            .execute(
                "session.create",
                json!({"tenantId": tenant, "ttlMinutes": 15}),
                &platform(actor),
            )
            .unwrap()
    }

    #[test]
    fn session_is_actor_bound_and_end_is_idempotent() {
        let module = FeatureTenantPreview::with_pool(pool());
        let created = create(&module, "actor-a", "tenant-a");
        let id = created["id"].as_str().unwrap();
        assert_eq!(created["tenantId"], "tenant-a");
        assert_eq!(created["tenantName"], "Tenant A");
        let resolved = module
            .execute("session.resolve", json!({"id": id}), &platform("actor-a"))
            .unwrap();
        assert_eq!(resolved["mode"], "preview");
        assert!(resolved["simulationId"].is_null());
        let wrong_actor = module
            .execute("session.resolve", json!({"id": id}), &platform("actor-b"))
            .unwrap_err();
        assert!(wrong_actor.contains("NOT_FOUND"));

        let ended = module
            .execute("session.end", json!({"id": id}), &platform("actor-a"))
            .unwrap();
        let ended_again = module
            .execute("session.end", json!({"id": id}), &platform("actor-a"))
            .unwrap();
        assert_eq!(ended["status"], "ended");
        assert_eq!(ended["endedAt"], ended_again["endedAt"]);
        let inactive = module
            .execute("session.resolve", json!({"id": id}), &platform("actor-a"))
            .unwrap_err();
        assert!(inactive.contains("PREVIEW_SESSION_INACTIVE"));
    }

    #[test]
    fn diagnostics_and_simulation_are_explicit_workspace_modes() {
        let pool = pool();
        pool.get()
            .unwrap()
            .execute(
                "INSERT INTO simulation_sessions VALUES ('simulation-a','actor-a','tenant-a','active')",
                [],
            )
            .unwrap();
        let module = FeatureTenantPreview::with_pool(pool);
        let diagnostics = module
            .execute(
                "workspace.create_diagnostics",
                json!({"tenantId":"tenant-a","mode":"diagnostics"}),
                &platform("actor-a"),
            )
            .unwrap();
        assert_eq!(diagnostics["mode"], "diagnostics");
        assert_eq!(
            diagnostics["capabilities"],
            json!(["preview.read", "diagnostics.read"])
        );

        let simulation = module
            .execute(
                "workspace.create_simulation",
                json!({
                    "tenantId":"tenant-a",
                    "mode":"simulation",
                    "simulationId":"simulation-a"
                }),
                &platform("actor-a"),
            )
            .unwrap();
        assert_eq!(simulation["mode"], "simulation");
        assert_eq!(simulation["simulationId"], "simulation-a");
        assert_eq!(
            simulation["capabilities"],
            json!(["simulation.read", "simulation.execute"])
        );

        let cross_actor = module
            .execute(
                "workspace.create_simulation",
                json!({
                    "tenantId":"tenant-a",
                    "mode":"simulation",
                    "simulationId":"simulation-a"
                }),
                &platform("actor-b"),
            )
            .unwrap_err();
        assert!(cross_actor.contains("SIMULATION_NOT_FOUND"));
    }

    #[test]
    fn expired_session_fails_closed_for_get_and_resolve() {
        let pool = pool();
        let module = FeatureTenantPreview::with_pool(pool.clone());
        let created = create(&module, "actor-a", "tenant-a");
        let id = created["id"].as_str().unwrap();
        pool.get().unwrap().execute(
            "UPDATE tenant_preview_sessions SET expires_at = '2000-01-01T00:00:00+08:00' WHERE id = ?1", [id],
        ).unwrap();
        for command in ["session.get", "session.resolve"] {
            let failure = module
                .execute(command, json!({"id": id}), &platform("actor-a"))
                .unwrap_err();
            assert!(failure.contains("PREVIEW_SESSION_EXPIRED"), "{failure}");
        }
    }

    #[test]
    fn dashboard_summary_is_tenant_isolated_and_workspace_read_only() {
        let module = FeatureTenantPreview::with_pool(pool());
        let a = module
            .execute(
                "dashboard.summary",
                json!({}),
                &preview("actor-a", "tenant-a"),
            )
            .unwrap();
        let b = module
            .execute(
                "dashboard.summary",
                json!({}),
                &preview("actor-a", "tenant-b"),
            )
            .unwrap();
        assert_eq!(
            a,
            json!({"orders": 1, "activeOrders": 1, "devices": 1, "availableDevices": 1})
        );
        assert_eq!(
            b,
            json!({"orders": 1, "activeOrders": 0, "devices": 1, "availableDevices": 0})
        );
        let diagnostic = module
            .execute(
                "dashboard.summary",
                json!({}),
                &diagnostics("actor-a", "tenant-a"),
            )
            .unwrap();
        assert_eq!(diagnostic, a);
        let normal = platform("actor-a");
        assert!(
            module
                .execute("dashboard.summary", json!({}), &normal)
                .unwrap_err()
                .contains("AUTH_PREVIEW_REQUIRED")
        );
    }

    #[test]
    fn inactive_tenant_fails_closed_on_every_resolution() {
        let pool = pool();
        let module = FeatureTenantPreview::with_pool(pool.clone());
        let created = create(&module, "actor-a", "tenant-a");
        let id = created["id"].as_str().unwrap();
        pool.get()
            .unwrap()
            .execute(
                "UPDATE tenants SET status = 'suspended' WHERE id = 'tenant-a'",
                [],
            )
            .unwrap();
        let failure = module
            .execute("session.resolve", json!({"id": id}), &platform("actor-a"))
            .unwrap_err();
        assert!(failure.contains("PREVIEW_TENANT_INACTIVE"));
    }
}

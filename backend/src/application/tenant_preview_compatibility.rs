use chrono::{DateTime, Duration, Utc};
use chrono_tz::Asia::Shanghai;
use feature_tenant_preview::FeatureTenantPreview;
use serde::Deserialize;
use serde_json::{Value, json};
use system_core::{
    ErrorPayload, ExecutionContext, ExecutionMode, ExecutionPlane, ModuleMetadata, ModuleSchema,
    PlatformCapability, SystemModule,
};

use crate::repositories::{
    PreviewMutationError, PreviewSessionCreate, PreviewSessionProjection, RepositoryError,
    TenantPreviewRepository,
};

const MIN_TTL_MINUTES: i64 = 1;
const MAX_TTL_MINUTES: i64 = 60;

#[derive(Clone)]
pub(crate) struct TenantPreviewCompatibilityModule {
    repository: TenantPreviewRepository,
}

impl TenantPreviewCompatibilityModule {
    pub(crate) fn new(repository: TenantPreviewRepository) -> Self {
        Self { repository }
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

        let mode = input.mode.unwrap_or(WorkspaceMode::Preview);
        let simulation_id = match mode {
            WorkspaceMode::Simulation => Some(
                non_blank(
                    input.simulation_id.as_deref().unwrap_or_default(),
                    "simulationId",
                )?
                .to_owned(),
            ),
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
        let command = PreviewSessionCreate {
            id: uuid::Uuid::new_v4().to_string(),
            actor_id: actor_id.to_owned(),
            target_tenant_id: tenant_id.to_owned(),
            mode: mode.as_str().into(),
            simulation_id,
            capabilities,
            created_at: now.to_rfc3339(),
            expires_at: (now + Duration::minutes(ttl)).to_rfc3339(),
        };
        let session = self
            .repository
            .create_session(command)
            .map_err(mutation_error)?;
        session_value(session)
    }

    fn get_session(&self, input: SessionInput, ctx: &ExecutionContext) -> Result<Value, String> {
        Self::require_platform_authority(ctx)?;
        let actor_id = ctx
            .actor()
            .id()
            .ok_or_else(|| error("AUTH_ACTOR_REQUIRED", "actor id required"))?;
        let session = self.load_owned(&input.id, actor_id)?;

        if session.status == "active" {
            ensure_not_expired(&session)?;
            if !self
                .repository
                .tenant_is_active(&session.target_tenant_id)
                .map_err(query_error)?
            {
                return Err(error(
                    "PREVIEW_TENANT_INACTIVE",
                    "preview tenant is not active",
                ));
            }
        }
        session_value(session)
    }

    fn end_session(&self, input: SessionInput, ctx: &ExecutionContext) -> Result<Value, String> {
        Self::require_platform_authority(ctx)?;
        let actor_id = ctx
            .actor()
            .id()
            .ok_or_else(|| error("AUTH_ACTOR_REQUIRED", "actor id required"))?;
        let id = non_blank(&input.id, "id")?;
        let ended_at = Utc::now().with_timezone(&Shanghai).to_rfc3339();
        let session = self
            .repository
            .end_session(id, actor_id, &ended_at)
            .map_err(mutation_error)?;
        session_value(session)
    }

    fn resolve_session(
        &self,
        input: SessionInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        Self::require_platform_authority(ctx)?;
        let actor_id = ctx
            .actor()
            .id()
            .ok_or_else(|| error("AUTH_ACTOR_REQUIRED", "actor id required"))?;
        let session = self.load_owned(&input.id, actor_id)?;
        if session.status != "active" {
            return Err(error(
                "PREVIEW_SESSION_INACTIVE",
                "preview session is not active",
            ));
        }
        ensure_not_expired(&session)?;
        if !self
            .repository
            .tenant_is_active(&session.target_tenant_id)
            .map_err(query_error)?
        {
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
        let summary = self
            .repository
            .dashboard_summary(ctx.data_scope().tenant_id().as_str())
            .map_err(query_error)?;
        serde_json::to_value(summary)
            .map_err(|_| error("SYS_SERIALIZE", "preview summary serialization failed"))
    }

    fn load_owned(&self, id: &str, actor_id: &str) -> Result<PreviewSessionProjection, String> {
        let id = non_blank(id, "id")?;
        self.repository
            .load_owned(id, actor_id)
            .map_err(query_error)?
            .ok_or_else(|| error("NOT_FOUND", "preview session not found"))
    }
}

impl SystemModule for TenantPreviewCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureTenantPreview::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureTenantPreview::new().commands()
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
        FeatureTenantPreview::new().schema()
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkspaceMode {
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

fn ensure_not_expired(session: &PreviewSessionProjection) -> Result<(), String> {
    let expiry = DateTime::parse_from_rfc3339(&session.expires_at).map_err(|_| {
        error(
            "PREVIEW_SESSION_INVALID",
            "preview session expiry is invalid",
        )
    })?;
    if expiry <= Utc::now() {
        return Err(error("PREVIEW_SESSION_EXPIRED", "preview session expired"));
    }
    Ok(())
}

fn session_value(session: PreviewSessionProjection) -> Result<Value, String> {
    serde_json::to_value(session)
        .map_err(|_| error("SYS_SERIALIZE", "preview session serialization failed"))
}

fn parse<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|_| error("VAL_INPUT", "invalid preview input"))
}

fn non_blank<'a>(value: &'a str, field: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(error("VAL_REQUIRED", &format!("{field} is required")))
    } else {
        Ok(value)
    }
}

fn mutation_error(error_value: PreviewMutationError) -> String {
    match error_value {
        PreviewMutationError::TenantNotFound => error("NOT_FOUND", "active tenant not found"),
        PreviewMutationError::SimulationNotFound => error(
            "SIMULATION_NOT_FOUND",
            "active actor-owned simulation was not found",
        ),
        PreviewMutationError::NotFound => error("NOT_FOUND", "preview session not found"),
        PreviewMutationError::Storage(error_value) => storage_error(error_value, true),
    }
}

fn query_error(error_value: RepositoryError) -> String {
    storage_error(error_value, false)
}

fn storage_error(error_value: RepositoryError, write: bool) -> String {
    let code = if write {
        "SYS_DB_WRITE"
    } else {
        "SYS_DB_QUERY"
    };
    error(
        code,
        if matches!(error_value, RepositoryError::AdapterUnavailable(_)) {
            "preview repository unavailable"
        } else {
            "preview persistence failed"
        },
    )
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
    .unwrap_or_else(|_| format!(r#"{{"category":"sys","code":"{code}"}}"#))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use serde_json::{Value, json};
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode,
        NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId, Revision,
        SystemModule, TenantId, TenantScope,
    };

    use crate::repositories::TenantPreviewRepository;

    use super::TenantPreviewCompatibilityModule;

    fn pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        pool.get()
            .unwrap()
            .execute_batch(
                "CREATE TABLE tenants (
                    id TEXT PRIMARY KEY,name TEXT NOT NULL,slug TEXT NOT NULL,status TEXT NOT NULL
                );
                CREATE TABLE orders (
                    id TEXT PRIMARY KEY,status TEXT NOT NULL,tenant_id TEXT NOT NULL
                );
                CREATE TABLE devices (
                    id TEXT PRIMARY KEY,rentalStatus TEXT NOT NULL,tenant_id TEXT NOT NULL
                );
                CREATE TABLE tenant_preview_sessions (
                    id TEXT PRIMARY KEY,actor_id TEXT NOT NULL,target_tenant_id TEXT NOT NULL,
                    status TEXT NOT NULL,created_at TEXT NOT NULL,expires_at TEXT NOT NULL,
                    ended_at TEXT
                );
                CREATE TABLE tenant_workspace_sessions (
                    id TEXT PRIMARY KEY,actor_identity_id TEXT NOT NULL,tenant_id TEXT NOT NULL,
                    mode TEXT NOT NULL,preview_session_id TEXT,simulation_id TEXT,
                    capabilities_json TEXT NOT NULL,status TEXT NOT NULL,created_at TEXT NOT NULL,
                    expires_at TEXT NOT NULL,ended_at TEXT
                );
                CREATE TABLE simulation_sessions (
                    id TEXT PRIMARY KEY,actor_id TEXT NOT NULL,target_tenant_id TEXT NOT NULL,
                    status TEXT NOT NULL
                );
                INSERT INTO tenants VALUES
                    ('tenant-a','Tenant A','tenant-a','active'),
                    ('tenant-b','Tenant B','tenant-b','active');
                INSERT INTO orders VALUES
                    ('order-a','draft','tenant-a'),
                    ('order-b','completed','tenant-b');
                INSERT INTO devices VALUES
                    ('device-a','已入库','tenant-a'),
                    ('device-b','租赁中','tenant-b');
                INSERT INTO simulation_sessions VALUES
                    ('simulation-a','actor-a','tenant-a','active');",
            )
            .unwrap();
        pool
    }

    fn platform(actor: &str) -> ExecutionContext {
        ExecutionContext::new(
            ActorIdentity::with_authority(
                actor,
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new(format!("membership-{actor}"))
                        .unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::platform(),
            DataScope::platform(Revision::new("preview-r1").unwrap()),
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
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new(format!("membership-{actor}"))
                        .unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("preview-r1").unwrap()).unwrap(),
            ExecutionMode::ReadOnlyPreview(PreviewSessionId::new("preview-read").unwrap()),
            RequestId::new(format!("req-preview-{actor}")).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn create(module: &TenantPreviewCompatibilityModule, actor: &str, tenant: &str) -> Value {
        module
            .execute(
                "workspace.create_preview",
                json!({"tenantId": tenant, "ttlMinutes": 15, "mode": "preview"}),
                &platform(actor),
            )
            .unwrap()
    }

    #[test]
    fn sqlite_tenant_preview_preserves_actor_binding_modes_expiry_and_tenant_reads() {
        let pool = pool();
        let module =
            TenantPreviewCompatibilityModule::new(TenantPreviewRepository::new(pool.clone()));

        let created = create(&module, "actor-a", "tenant-a");
        let id = created["id"].as_str().unwrap();
        assert_eq!(created["tenantId"], "tenant-a");
        assert_eq!(created["mode"], "preview");

        let resolved = module
            .execute("session.resolve", json!({"id": id}), &platform("actor-a"))
            .unwrap();
        assert_eq!(resolved["tenantId"], "tenant-a");
        assert_eq!(resolved["capabilities"], json!(["preview.read"]));

        let wrong_actor = module
            .execute("session.resolve", json!({"id": id}), &platform("actor-b"))
            .unwrap_err();
        assert!(wrong_actor.contains("NOT_FOUND"));

        let diagnostics = module
            .execute(
                "workspace.create_diagnostics",
                json!({"tenantId":"tenant-a","mode":"diagnostics"}),
                &platform("actor-a"),
            )
            .unwrap();
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
        assert_eq!(simulation["simulationId"], "simulation-a");

        let cross_actor_simulation = module
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
        assert!(cross_actor_simulation.contains("SIMULATION_NOT_FOUND"));

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
            json!({"orders":1,"activeOrders":1,"devices":1,"availableDevices":1})
        );
        assert_eq!(
            b,
            json!({"orders":1,"activeOrders":0,"devices":1,"availableDevices":0})
        );

        let ended = module
            .execute("session.end", json!({"id": id}), &platform("actor-a"))
            .unwrap();
        let ended_again = module
            .execute("session.end", json!({"id": id}), &platform("actor-a"))
            .unwrap();
        assert_eq!(ended["status"], "ended");
        assert_eq!(ended["endedAt"], ended_again["endedAt"]);

        {
            let connection = pool.get().unwrap();
            let workspace: (String, Option<String>) = connection
                .query_row(
                    "SELECT status,ended_at FROM tenant_workspace_sessions WHERE id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(workspace.0, "ended");
            assert_eq!(workspace.1.as_deref(), ended["endedAt"].as_str());
        }

        let expired = create(&module, "actor-a", "tenant-b");
        let expired_id = expired["id"].as_str().unwrap();
        {
            let connection = pool.get().unwrap();
            connection
                .execute(
                    "UPDATE tenant_preview_sessions
                     SET expires_at='2000-01-01T00:00:00+08:00' WHERE id=?1",
                    [expired_id],
                )
                .unwrap();
        }
        let expired_error = module
            .execute(
                "session.resolve",
                json!({"id": expired_id}),
                &platform("actor-a"),
            )
            .unwrap_err();
        assert!(expired_error.contains("PREVIEW_SESSION_EXPIRED"));

        let inactive = create(&module, "actor-a", "tenant-a");
        let inactive_id = inactive["id"].as_str().unwrap();
        {
            let connection = pool.get().unwrap();
            connection
                .execute(
                    "UPDATE tenants SET status='suspended' WHERE id='tenant-a'",
                    [],
                )
                .unwrap();
        }
        let inactive_error = module
            .execute(
                "session.resolve",
                json!({"id": inactive_id}),
                &platform("actor-a"),
            )
            .unwrap_err();
        assert!(inactive_error.contains("PREVIEW_TENANT_INACTIVE"));
    }
}

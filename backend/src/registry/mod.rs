//! ModuleRegistry — 模块注册表，集中管理已装配 SystemModule 的生命周期与调用。
//!
//! - assemble(): 构造 + init() + DI 注入（见 assembler.rs）
//! - execute():   路由 → 模块调度（含性能监控）
//! - all_schemas() / openai_tools():  /meta/modules 端点数据源

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use system_core::audit::{AuditEvent, AuditResult};
use system_core::transport::http_client::HttpClient;
use system_core::*;

use crate::auth_contract::AuthUserInfo;
use crate::observability::{MetricsSink, NoopMetrics, ResultClass, classify_registry_error};
use crate::utils::time::shanghai_now_iso;

pub mod assembler;
pub mod audit_sink;
#[cfg(feature = "postgres")]
pub(crate) mod audit_sink_postgres;
#[cfg(all(test, feature = "postgres"))]
mod audit_sink_postgres_qualification_tests;
pub mod descriptors;
pub mod factory;
pub mod gate;
pub mod provider_config;
pub mod validation;

pub struct ModuleRegistry {
    modules: HashMap<String, Arc<dyn SystemModule>>,
    audit_sink: Arc<dyn audit_sink::AuditSink>,
    metrics: Arc<dyn MetricsSink>,
}

impl ModuleRegistry {
    /// The bearer entry point cannot supply an actor, membership or mode.
    /// Project scopes narrow (never replace) the existing Registry gates.
    pub fn execute_machine(
        &self,
        pool: &r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
        token: &str,
        tenant: &str,
        version: &str,
        scope: &crate::services::machine_api::Scope,
        payload: Value,
        http_client: Arc<dyn HttpClient>,
        correlation: &str,
        idempotency_key: Option<String>,
    ) -> Result<Value, crate::error::AppError> {
        self.execute_machine_with_repository(
            &crate::repositories::MachineAuthorityRepository::new(pool.clone()),
            token,
            tenant,
            version,
            scope,
            payload,
            http_client,
            correlation,
            idempotency_key,
        )
    }

    pub(crate) fn execute_machine_with_repository(
        &self,
        repository: &crate::repositories::MachineAuthorityRepository,
        token: &str,
        tenant: &str,
        version: &str,
        scope: &crate::services::machine_api::Scope,
        payload: Value,
        http_client: Arc<dyn HttpClient>,
        correlation: &str,
        idempotency_key: Option<String>,
    ) -> Result<Value, crate::error::AppError> {
        use crate::error::AppError;
        if idempotency_key
            .as_ref()
            .is_some_and(|key| key.len() < 16 || key.len() > 128 || !key.is_ascii())
        {
            return Err(AppError::BadRequest("invalid Idempotency-Key".into()));
        }
        let auth = crate::services::machine_api::authorize_with_repository(
            repository,
            token,
            tenant,
            version,
            scope,
            correlation,
        )?;
        let ctx = ExecutionContext::new(
            ActorIdentity::with_authority(auth.identity_id, auth.authority)
                .map_err(AppError::Internal)?,
            TenantScope::tenant(auth.tenant_id.clone()),
            DataScope::production(
                auth.tenant_id,
                Revision::new("production-current").map_err(AppError::Internal)?,
            )
            .map_err(AppError::Internal)?,
            ExecutionMode::Normal,
            RequestId::new(correlation).map_err(AppError::Internal)?,
            idempotency_key,
            http_client,
        )
        .map_err(AppError::Internal)?;
        self.execute(&scope.module, &scope.command, payload, &ctx)
            .map_err(AppError::from_error_payload)
    }

    pub fn new(modules: HashMap<String, Arc<dyn SystemModule>>) -> Result<Self, String> {
        Self::new_with_audit_sink(modules, Arc::new(audit_sink::InMemoryAuditSink::default()))
    }

    /// Constructs a registry with its append-only execution audit boundary.
    pub fn new_with_audit_sink(
        modules: HashMap<String, Arc<dyn SystemModule>>,
        audit_sink: Arc<dyn audit_sink::AuditSink>,
    ) -> Result<Self, String> {
        Self::new_with_audit_sink_and_metrics(modules, audit_sink, Arc::new(NoopMetrics))
    }

    /// Constructs the Registry with an independently injectable operational
    /// metrics sink. Durable Audit remains the authority for command facts.
    pub(crate) fn new_with_audit_sink_and_metrics(
        modules: HashMap<String, Arc<dyn SystemModule>>,
        audit_sink: Arc<dyn audit_sink::AuditSink>,
        metrics: Arc<dyn MetricsSink>,
    ) -> Result<Self, String> {
        for (module_name, module) in &modules {
            crate::registry::validation::validate_command_metadata(
                module_name,
                &module.schema(),
                &module.commands(),
            )?;
        }
        Ok(Self {
            modules,
            audit_sink,
            metrics,
        })
    }

    /// 路由 → 模块调度。
    /// 从 HashMap 查找模块 → 调用 execute(command, payload, ctx)。
    /// 内置 tracing::debug 级性能监控。
    pub fn execute(
        &self,
        module_name: &str,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let start = std::time::Instant::now();
        self.metrics
            .registry_attempt(module_name, command, ctx.execution_mode());
        let attempt = match AuditEvent::from_execution(
            uuid::Uuid::new_v4().to_string(),
            ctx,
            format!("{module_name}.{command}"),
            AuditResult::Attempted,
            shanghai_now_iso(),
            AuditPayloadPolicy::ReferenceOnly,
            serde_json::json!({ "module": module_name, "command": command }),
        ) {
            Ok(attempt) => attempt,
            Err(error) => {
                self.metrics.registry_result(
                    module_name,
                    command,
                    ctx.execution_mode(),
                    ResultClass::SystemError,
                    start.elapsed(),
                );
                return Err(error);
            }
        };
        if let Err(error) = self.audit_sink.append(attempt) {
            let error = serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "EXEC_AUDIT_UNAVAILABLE".into(),
                message: format!(
                    "command was not executed because its attempt could not be audited: {error}"
                ),
                field: None,
                context: None,
            })
            .unwrap_or_default();
            self.metrics.registry_result(
                module_name,
                command,
                ctx.execution_mode(),
                ResultClass::SystemError,
                start.elapsed(),
            );
            return Err(error);
        }
        let result = (|| {
            let module_ref = self.modules.get(module_name).ok_or_else(|| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_MODULE_NOT_FOUND".into(),
                    message: format!("模块 {} 未注册", module_name),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

            if !ctx.has_resolved_data_scope() {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "auth".into(),
                    code: "EXEC_DATA_SCOPE_UNRESOLVED".into(),
                    message: "a resolved tenant data scope is required before command execution"
                        .into(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default());
            }

            let command_metadata = module_ref
                .commands()
                .into_iter()
                .find(|metadata| metadata.name == command)
                .ok_or_else(|| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_COMMAND_METADATA_MISSING".into(),
                        message: format!("command metadata missing for {module_name}.{command}"),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })?;

            crate::registry::gate::authorize_execution_mode_for(
                ctx.execution_mode(),
                &command_metadata,
                Some((module_name, command)),
            )
            .map_err(|error| {
                serde_json::to_string(&ErrorPayload {
                    category: "auth".into(),
                    code: error.code().into(),
                    message: "command is not permitted in the current execution mode".into(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

            crate::registry::gate::authorize_access(ctx, &command_metadata).map_err(|error| {
                serde_json::to_string(&ErrorPayload {
                    category: "auth".into(),
                    code: error.code().into(),
                    message: "actor does not satisfy the command access requirement".into(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;
            crate::registry::gate::authorize_platform_capability(
                ctx,
                &command_metadata,
                module_name,
                command,
            )
            .map_err(|error| {
                serde_json::to_string(&ErrorPayload {
                    category: "auth".into(),
                    code: error.code().into(),
                    message: "actor lacks the required platform capability".into(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

            module_ref.execute(command, payload, ctx)
        })();
        let elapsed = start.elapsed();

        let result_class = match &result {
            Ok(_) => ResultClass::Succeeded,
            Err(error) => classify_registry_error(error),
        };
        self.metrics.registry_result(
            module_name,
            command,
            ctx.execution_mode(),
            result_class,
            elapsed,
        );

        tracing::debug!(
            target: "registry",
            module = crate::observability::registry_metric_module(module_name),
            command = crate::observability::registry_metric_command(module_name, command),
            duration_ms = elapsed.as_millis(),
            "Registry execution completed",
        );

        let audit_result = match &result {
            Ok(_) => AuditResult::Succeeded,
            Err(error) => AuditResult::Failed {
                error_code: audit_error_code(error),
            },
        };
        let event = AuditEvent::from_execution(
            uuid::Uuid::new_v4().to_string(),
            ctx,
            format!("{module_name}.{command}"),
            audit_result,
            shanghai_now_iso(),
            AuditPayloadPolicy::ReferenceOnly,
            serde_json::json!({ "module": module_name, "command": command }),
        );
        match event.and_then(|event| self.audit_sink.append(event)) {
            Ok(()) => {}
            Err(_) => tracing::error!(
                target: "registry",
                module = crate::observability::registry_metric_module(module_name),
                command = crate::observability::registry_metric_command(module_name, command),
                error_class = "audit_append_failed",
                "execution audit append failed after command completion; preserving command outcome",
            ),
        }

        result
    }

    /// GET /meta/modules 的数据源
    pub fn all_schemas(&self) -> Vec<ModuleSchema> {
        self.modules.values().map(|m| m.schema()).collect()
    }

    /// GET /meta/modules/openai-tools 的数据源
    pub fn openai_tools(&self) -> Vec<Value> {
        self.modules
            .values()
            .map(|m| system_core::to_openai_function_schema(m.as_ref()))
            .collect()
    }

    /// 按名称查询模块引用
    pub fn get(&self, name: &str) -> Option<Arc<dyn SystemModule>> {
        self.modules.get(name).cloned()
    }
}

/// 从 AuthUserInfo + HttpClient 构建 ExecutionContext。
/// 用于路由层 handler 将认证用户信息传入模块执行上下文。
pub fn make_ctx(auth: &AuthUserInfo, http_client: Arc<dyn HttpClient>) -> ExecutionContext {
    let authenticated_tenant = auth
        .tenant_id()
        .expect("authenticated routes require a resolved tenant");
    let tenant_id = TenantId::new(authenticated_tenant).expect("authenticated tenant id is valid");
    let data_scope = DataScope::production(
        tenant_id,
        Revision::new("production-current").expect("static production revision is valid"),
    )
    .expect("production scope is valid");
    let tenant_scope = TenantScope::tenant(data_scope.tenant_id().clone());
    ExecutionContext::new(
        ActorIdentity::with_authority(auth.id.clone(), auth.authority.clone())
            .expect("authenticated user id is valid"),
        tenant_scope,
        data_scope,
        ExecutionMode::Normal,
        RequestId::new(uuid::Uuid::new_v4().to_string()).expect("UUID request id is valid"),
        None,
        http_client,
    )
    .expect("authenticated request context invariants are valid")
}

/// Build a fresh control-plane context for a server-authenticated platform_owner.
/// The platform scope cannot be selected by request payload or tenant headers.
pub fn make_platform_ctx(
    auth: &AuthUserInfo,
    http_client: Arc<dyn HttpClient>,
) -> Result<ExecutionContext, String> {
    if !matches!(auth.authority, AuthorityContext::Platform { .. }) {
        return Err("platform execution requires platform authority".into());
    }
    ExecutionContext::new(
        ActorIdentity::with_authority(auth.id.clone(), auth.authority.clone())?,
        TenantScope::platform(),
        DataScope::platform(Revision::new("control-plane-current")?),
        ExecutionMode::Normal,
        RequestId::new(uuid::Uuid::new_v4().to_string())?,
        None,
        http_client,
    )
}

fn audit_error_code(error: &str) -> String {
    serde_json::from_str::<ErrorPayload>(error)
        .map(|payload| payload.code)
        .unwrap_or_else(|_| "EXEC_COMMAND_FAILED".into())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::{Value, json};
    use system_core::{
        AccessRequirement, ActorIdentity, AuditPayloadPolicy, AuthorityContext, CommandMetadata,
        CommandSchema, DataScope, EffectClass, ExecutionContext, ExecutionMode, ModuleMetadata,
        ModuleSchema, Namespace, NoopHttpClient, PlatformMembershipId, PlatformRole,
        PreviewSessionId, RequestId, Revision, SimulationSupport, SystemModule, TenantId,
        TenantMembershipId, TenantRole, TenantScope,
    };

    use crate::observability::RuntimeMetrics;
    use crate::registry::audit_sink::{AuditSink, InMemoryAuditSink};
    use system_core::audit::{AuditEvent, AuditResult};

    use super::ModuleRegistry;
    use crate::auth_contract::AuthUserInfo;

    #[derive(Default)]
    struct ProbeModule {
        executions: Option<Arc<AtomicUsize>>,
    }

    struct RejectingAuditSink(AtomicUsize);

    impl AuditSink for RejectingAuditSink {
        fn append(&self, _event: AuditEvent) -> Result<(), String> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err("storage unavailable".into())
            }
        }
    }

    struct AlwaysRejectingAuditSink;

    impl AuditSink for AlwaysRejectingAuditSink {
        fn append(&self, _event: AuditEvent) -> Result<(), String> {
            Err("storage unavailable".into())
        }
    }

    impl SystemModule for ProbeModule {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                name: "probe".into(),
                version: "1.0.0".into(),
                description: "registry audit test module".into(),
                author: "test".into(),
                wasm_compatible: false,
                storage: None,
            }
        }

        fn init(&mut self, _config: Value) -> Result<(), String> {
            Ok(())
        }

        fn commands(&self) -> Vec<CommandMetadata> {
            vec![
                CommandMetadata::new(
                    "read",
                    AccessRequirement::Authenticated,
                    &[EffectClass::DatabaseRead],
                    SimulationSupport::Supported,
                ),
                CommandMetadata::new(
                    "write",
                    AccessRequirement::Authenticated,
                    &[EffectClass::DatabaseWrite],
                    SimulationSupport::Supported,
                ),
            ]
        }

        fn execute(
            &self,
            command: &str,
            _payload: Value,
            _ctx: &ExecutionContext,
        ) -> Result<Value, String> {
            if let Some(executions) = &self.executions {
                executions.fetch_add(1, Ordering::SeqCst);
            }
            match command {
                "read" | "write" => Ok(json!({ "handled": command })),
                _ => Err("unexpected command".into()),
            }
        }

        fn schema(&self) -> ModuleSchema {
            ModuleSchema {
                name: "probe".into(),
                description: "registry audit test module".into(),
                commands: ["read", "write"]
                    .into_iter()
                    .map(|name| CommandSchema {
                        name: name.into(),
                        description: "probe".into(),
                        version: "1.0.0".into(),
                        input_schema: None,
                        output_schema: None,
                    })
                    .collect(),
            }
        }
    }

    fn context(mode: ExecutionMode) -> ExecutionContext {
        let tenant_id = TenantId::new("tenant-audit").unwrap();
        let data_scope = DataScope::new(
            tenant_id.clone(),
            Namespace::production(),
            Revision::new("revision-audit").unwrap(),
        )
        .unwrap();
        let actor = if matches!(
            mode,
            ExecutionMode::ReadOnlyPreview(_) | ExecutionMode::Simulation(_)
        ) {
            ActorIdentity::with_authority(
                "actor-audit",
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-membership-audit").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap()
        } else {
            ActorIdentity::with_authority(
                "actor-audit",
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new("tenant-membership-audit").unwrap(),
                    tenant_id: tenant_id.clone(),
                    role: TenantRole::Admin,
                },
            )
            .unwrap()
        };
        ExecutionContext::new(
            actor,
            TenantScope::tenant(tenant_id),
            data_scope,
            mode,
            RequestId::new("request-audit").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn every_registry_attempt_is_audited_including_preview_gate_denials() {
        let sink = Arc::new(InMemoryAuditSink::default());
        let mut modules: HashMap<String, Arc<dyn SystemModule>> = HashMap::new();
        modules.insert("probe".into(), Arc::new(ProbeModule::default()));
        let registry = ModuleRegistry::new_with_audit_sink(modules, sink.clone()).unwrap();

        registry
            .execute(
                "probe",
                "read",
                json!({ "secret": "never recorded" }),
                &context(ExecutionMode::Normal),
            )
            .unwrap();
        let denied = registry.execute(
            "probe",
            "write",
            json!({}),
            &context(ExecutionMode::ReadOnlyPreview(
                PreviewSessionId::new("preview-1").unwrap(),
            )),
        );
        assert!(denied.unwrap_err().contains("EXEC_PREVIEW_WRITE_BLOCKED"));

        let events = sink.events().unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].command(), "probe.read");
        assert_eq!(events[0].result(), &AuditResult::Attempted);
        assert_eq!(
            events[0].payload_policy(),
            AuditPayloadPolicy::ReferenceOnly
        );
        assert_eq!(
            events[0].payload(),
            &json!({ "module": "probe", "command": "read" })
        );
        assert_eq!(events[1].result(), &AuditResult::Succeeded);
        assert_eq!(events[2].command(), "probe.write");
        assert_eq!(events[2].result(), &AuditResult::Attempted);
        assert_eq!(
            events[3].result(),
            &AuditResult::Failed {
                error_code: "EXEC_PREVIEW_WRITE_BLOCKED".into()
            },
        );
        assert_eq!(events[3].correlation_id().as_str(), "request-audit");
        assert!(events[3].occurred_at().ends_with("+08:00"));
    }

    #[test]
    fn registry_invocation_emits_bounded_attempt_result_and_latency_metrics() {
        let sink = Arc::new(InMemoryAuditSink::default());
        let metrics = Arc::new(RuntimeMetrics::default());
        let mut modules: HashMap<String, Arc<dyn SystemModule>> = HashMap::new();
        modules.insert("probe".into(), Arc::new(ProbeModule::default()));
        let registry =
            ModuleRegistry::new_with_audit_sink_and_metrics(modules, sink, metrics.clone())
                .unwrap();

        registry
            .execute(
                "probe",
                "read",
                json!({ "credential": "never-exported" }),
                &context(ExecutionMode::Normal),
            )
            .unwrap();

        let output = metrics.render();
        assert!(output.contains("talos_registry_commands_total"));
        assert!(output.contains("phase=\"attempt\""));
        assert!(output.contains("phase=\"result\""));
        assert!(output.contains("talos_registry_command_latency_milliseconds_count"));
        assert!(!output.contains("never-exported"));
    }

    #[test]
    fn audit_sink_failure_never_rewrites_a_completed_command_outcome() {
        let mut modules: HashMap<String, Arc<dyn SystemModule>> = HashMap::new();
        modules.insert("probe".into(), Arc::new(ProbeModule::default()));
        let registry = ModuleRegistry::new_with_audit_sink(
            modules,
            Arc::new(RejectingAuditSink(AtomicUsize::new(0))),
        )
        .unwrap();

        let output = registry.execute("probe", "write", json!({}), &context(ExecutionMode::Normal));
        assert_eq!(output.unwrap(), json!({ "handled": "write" }));
    }

    #[test]
    fn attempt_audit_failure_emits_system_result_without_executing_command() {
        let executions = Arc::new(AtomicUsize::new(0));
        let metrics = Arc::new(RuntimeMetrics::default());
        let mut modules: HashMap<String, Arc<dyn SystemModule>> = HashMap::new();
        modules.insert(
            "probe".into(),
            Arc::new(ProbeModule {
                executions: Some(executions.clone()),
            }),
        );
        let registry = ModuleRegistry::new_with_audit_sink_and_metrics(
            modules,
            Arc::new(AlwaysRejectingAuditSink),
            metrics.clone(),
        )
        .unwrap();

        let error = registry
            .execute("probe", "write", json!({}), &context(ExecutionMode::Normal))
            .unwrap_err();

        assert!(error.contains("EXEC_AUDIT_UNAVAILABLE"));
        assert_eq!(executions.load(Ordering::SeqCst), 0);
        let output = metrics.render();
        assert!(output.contains("phase=\"attempt\""));
        assert!(output.contains("phase=\"result\""));
        assert!(output.contains("result=\"system_error\""));
    }

    #[test]
    fn platform_context_factory_accepts_only_platform_authority() {
        let platform_user = AuthUserInfo {
            id: "platform-actor".into(),
            username: "platform".into(),
            session_id: "session-platform".into(),
            display_name: "Platform Actor".into(),
            email: String::new(),
            phone: String::new(),
            authority: AuthorityContext::Platform {
                membership_id: PlatformMembershipId::new("platform-membership").unwrap(),
                roles: vec![PlatformRole::Owner],
            },
        };
        let ctx = super::make_platform_ctx(&platform_user, Arc::new(NoopHttpClient))
            .expect("platform context");
        assert!(ctx.data_scope().is_platform());
        assert!(ctx.tenant_scope().is_platform());

        let tenant_user = AuthUserInfo {
            authority: AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new("tenant-membership").unwrap(),
                tenant_id: TenantId::new("tenant-a").unwrap(),
                role: TenantRole::Admin,
            },
            ..platform_user
        };
        assert!(super::make_platform_ctx(&tenant_user, Arc::new(NoopHttpClient)).is_err());
    }
}

use std::sync::Arc;

use serde_json::Value;
use system_core::ExecutionContext;

use crate::registry::ModuleRegistry;

/// Process-local application port for trusted module invocation.
pub trait ModuleClient: Send + Sync {
    fn execute(
        &self,
        module_name: &str,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String>;
}

pub struct RegistryModuleClient {
    registry: Arc<ModuleRegistry>,
}

impl RegistryModuleClient {
    pub fn new(registry: Arc<ModuleRegistry>) -> Self {
        Self { registry }
    }
}

impl ModuleClient for RegistryModuleClient {
    fn execute(
        &self,
        module_name: &str,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        self.registry.execute(module_name, command, payload, ctx)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use serde_json::{Value, json};
    use system_core::audit::AuditResult;
    use system_core::{
        AccessRequirement, ActorIdentity, AuthorityContext, CommandMetadata, CommandSchema,
        DataScope, EffectClass, ExecutionContext, ExecutionMode, ModuleMetadata, ModuleSchema,
        Namespace, NoopHttpClient, PlatformMembershipId, PlatformRole, PreviewSessionId, RequestId,
        Revision, SimulationSupport, SystemModule, TenantId, TenantMembershipId, TenantRole,
        TenantScope,
    };

    use crate::registry::ModuleRegistry;
    use crate::registry::audit_sink::InMemoryAuditSink;

    use super::{ModuleClient, RegistryModuleClient};

    #[derive(Default)]
    struct RecordingModule {
        calls: Arc<Mutex<Vec<(String, Value)>>>,
    }

    impl SystemModule for RecordingModule {
        fn metadata(&self) -> ModuleMetadata {
            ModuleMetadata {
                name: "probe".into(),
                version: "1.0.0".into(),
                description: "application ModuleClient probe".into(),
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
                CommandMetadata::new(
                    "admin",
                    AccessRequirement::TenantAdmin,
                    &[EffectClass::DatabaseRead],
                    SimulationSupport::Supported,
                ),
            ]
        }

        fn execute(
            &self,
            command: &str,
            payload: Value,
            _ctx: &ExecutionContext,
        ) -> Result<Value, String> {
            self.calls
                .lock()
                .map_err(|_| "probe lock poisoned".to_string())?
                .push((command.to_string(), payload.clone()));
            Ok(json!({ "command": command, "payload": payload }))
        }

        fn schema(&self) -> ModuleSchema {
            ModuleSchema {
                name: "probe".into(),
                description: "application ModuleClient probe".into(),
                commands: ["read", "write", "admin"]
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

    fn tenant_context_for_role(role: TenantRole) -> ExecutionContext {
        let tenant_id = TenantId::new("tenant-application").unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "tenant-user",
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new("membership-application").unwrap(),
                    tenant_id: tenant_id.clone(),
                    role,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::new(
                tenant_id,
                Namespace::production(),
                Revision::new("revision-application").unwrap(),
            )
            .unwrap(),
            ExecutionMode::Normal,
            RequestId::new("request-application").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn tenant_context(mode: ExecutionMode) -> ExecutionContext {
        if matches!(&mode, ExecutionMode::Normal) {
            return tenant_context_for_role(TenantRole::Admin);
        }

        let tenant_id = TenantId::new("tenant-application").unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "platform-owner",
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-application").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::new(
                tenant_id,
                Namespace::production(),
                Revision::new("revision-application").unwrap(),
            )
            .unwrap(),
            mode,
            RequestId::new("request-application").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn client_fixture() -> (
        RegistryModuleClient,
        Arc<Mutex<Vec<(String, Value)>>>,
        Arc<InMemoryAuditSink>,
    ) {
        let module = RecordingModule::default();
        let calls = module.calls.clone();
        let sink = Arc::new(InMemoryAuditSink::default());
        let mut modules: HashMap<String, Arc<dyn SystemModule>> = HashMap::new();
        modules.insert("probe".into(), Arc::new(module));
        let registry = ModuleRegistry::new_with_audit_sink(modules, sink.clone()).unwrap();
        (RegistryModuleClient::new(Arc::new(registry)), calls, sink)
    }

    #[test]
    fn delegates_module_command_payload_and_audit_to_registry() {
        let (client, calls, sink) = client_fixture();
        let payload = json!({ "nested": { "value": 7 } });

        let output = client
            .execute(
                "probe",
                "read",
                payload.clone(),
                &tenant_context(ExecutionMode::Normal),
            )
            .unwrap();

        assert_eq!(output, json!({ "command": "read", "payload": payload }));
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            &[("read".into(), payload)]
        );
        let events = sink.events().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].command(), "probe.read");
        assert_eq!(events[0].result(), &AuditResult::Attempted);
        assert_eq!(events[1].result(), &AuditResult::Succeeded);
    }

    #[test]
    fn preserves_unknown_module_failure_from_registry() {
        let (client, calls, sink) = client_fixture();

        let error = client
            .execute(
                "missing",
                "read",
                json!({}),
                &tenant_context(ExecutionMode::Normal),
            )
            .unwrap_err();

        assert!(error.contains("SYS_MODULE_NOT_FOUND"));
        assert!(calls.lock().unwrap().is_empty());
        let events = sink.events().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].result(), &AuditResult::Attempted);
        assert!(matches!(events[1].result(), AuditResult::Failed { .. }));
    }

    #[test]
    fn unresolved_data_scope_is_rejected_before_application_invocation() {
        let tenant_id = TenantId::new("tenant-application").unwrap();
        let scope = DataScope::new(
            tenant_id.clone(),
            Namespace::production(),
            Revision::new("revision-unresolved").unwrap(),
        )
        .unwrap();
        let mut encoded = serde_json::to_value(scope).unwrap();
        encoded["resolved"] = json!(false);
        let unresolved: DataScope = serde_json::from_value(encoded).unwrap();

        let context = ExecutionContext::new(
            ActorIdentity::authenticated("actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id),
            unresolved,
            ExecutionMode::Normal,
            RequestId::new("request-unresolved").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        );

        let error = context
            .err()
            .expect("unresolved DataScope must be rejected");
        assert!(error.contains("resolved data scope"));
    }

    #[test]
    fn tenant_admin_access_denial_is_not_bypassed() {
        let (client, calls, sink) = client_fixture();

        let error = client
            .execute(
                "probe",
                "admin",
                json!({ "forbidden": true }),
                &tenant_context_for_role(TenantRole::Staff),
            )
            .unwrap_err();

        assert!(error.contains("EXEC_ACCESS_DENIED"));
        assert!(calls.lock().unwrap().is_empty());
        let events = sink.events().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].result(), &AuditResult::Attempted);
        assert!(matches!(events[1].result(), AuditResult::Failed { .. }));
    }

    #[test]
    fn preview_write_denial_is_not_bypassed() {
        let (client, calls, sink) = client_fixture();

        let error = client
            .execute(
                "probe",
                "write",
                json!({ "forbidden": true }),
                &tenant_context(ExecutionMode::ReadOnlyPreview(
                    PreviewSessionId::new("preview-application").unwrap(),
                )),
            )
            .unwrap_err();

        assert!(error.contains("EXEC_PREVIEW_WRITE_BLOCKED"));
        assert!(calls.lock().unwrap().is_empty());
        let events = sink.events().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].result(), &AuditResult::Attempted);
        assert!(matches!(events[1].result(), AuditResult::Failed { .. }));
    }
}

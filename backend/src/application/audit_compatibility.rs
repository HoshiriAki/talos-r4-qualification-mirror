use serde_json::Value;
use system_admin::audit::{
    FeatureAudit, ListAuditLogsInput, WriteAuditLogInput, WriteAuditLogOutput,
    safe_audit_detail_json,
};
use system_core::{
    AccessRequirement, CommandMetadata, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule, Unvalidated,
};
use uuid::Uuid;

use crate::repositories::{AuditCompatibilityRepository, AuditCompatibilityWrite, RepositoryError};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct AuditCompatibilityModule {
    repository: AuditCompatibilityRepository,
}

impl AuditCompatibilityModule {
    pub(crate) fn new(repository: AuditCompatibilityRepository) -> Self {
        Self { repository }
    }

    fn storage_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "audit persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_else(|_| "SYS_AUDIT_PERSISTENCE".into())
    }
}

impl SystemModule for AuditCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "feature-audit".into(),
            version: "0.1.0".into(),
            description: "审计日志模块 — 异步缓冲写入 + 查询".into(),
            author: "Maxwell".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
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
            "write_audit_log" => {
                let unvalidated: Unvalidated<WriteAuditLogInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let id = Uuid::new_v4().to_string();
                let detail = serde_json::from_str(&input.detail_json)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                let entry = AuditCompatibilityWrite {
                    id: id.clone(),
                    tenant_id: ctx.data_scope().tenant_id().as_str().to_owned(),
                    actor_identity_id: ctx.user_id().unwrap_or("system").to_owned(),
                    actor_username: ctx.actor().id().unwrap_or("system").to_owned(),
                    action_type: input.action_type,
                    entity_type: input.entity_type,
                    entity_id: input.entity_id,
                    entity_label: input.entity_label,
                    detail_json: safe_audit_detail_json(&detail),
                    ip: String::new(),
                    user_agent: String::new(),
                    created_at: shanghai_now_iso(),
                };
                self.repository
                    .append_audit_log(&entry)
                    .map_err(Self::storage_error)?;
                serde_json::to_value(WriteAuditLogOutput { success: true, id })
                    .map_err(|error| error.to_string())
            }
            "list_audit_logs" => {
                let unvalidated: Unvalidated<ListAuditLogsInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let output = self
                    .repository
                    .list_audit_logs(ctx.data_scope().tenant_id().as_str(), &input)
                    .map_err(Self::storage_error)?;
                serde_json::to_value(output).map_err(|error| error.to_string())
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("unknown command: {command}"),
                field: Some("command".into()),
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "write_audit_log",
                AccessRequirement::System,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "list_audit_logs",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        FeatureAudit::new().schema()
    }
}

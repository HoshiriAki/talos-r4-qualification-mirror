use serde_json::Value;
use system_admin::deletion::{
    DeletionAdminInput, DeletionListInput, DeletionRequestInput, FeatureDeletion,
    require_self_service_actor,
};
use system_core::{
    AccessRequirement, CommandMetadata, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule, Unvalidated,
};

use crate::repositories::{
    DeletionCompatibilityError, DeletionCompatibilityRepository, DeletionRequestOutcome,
    RepositoryError,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct DeletionCompatibilityModule {
    repository: DeletionCompatibilityRepository,
}

impl DeletionCompatibilityModule {
    pub(crate) fn new(repository: DeletionCompatibilityRepository) -> Self {
        Self { repository }
    }

    fn compatibility_error(error: DeletionCompatibilityError) -> String {
        let payload = match error {
            DeletionCompatibilityError::LastOwner => ErrorPayload {
                category: "biz".into(),
                code: "TENANT_LAST_OWNER".into(),
                message: "必须先转移租户所有权，不能删除最后一位 Owner".into(),
                field: None,
                context: None,
            },
            DeletionCompatibilityError::MembershipNotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_NOT_FOUND".into(),
                message: "Tenant membership not found".into(),
                field: None,
                context: None,
            },
            DeletionCompatibilityError::Storage(error) => return Self::repository_error(error),
        };
        serde_json::to_string(&payload).unwrap_or_else(|_| "SYS_DELETION_PERSISTENCE".into())
    }

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "deletion persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_else(|_| "SYS_DELETION_PERSISTENCE".into())
    }
}

impl SystemModule for DeletionCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "deletion".into(),
            version: "0.1.0".into(),
            description: "数据删除/匿名化模块".into(),
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
        let tenant_id = ctx.data_scope().tenant_id().as_str();
        match command {
            "request" => {
                let unvalidated: Unvalidated<DeletionRequestInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                match self
                    .repository
                    .request(tenant_id, &input, &shanghai_now_iso())
                    .map_err(Self::compatibility_error)?
                {
                    DeletionRequestOutcome::Existing { id } => Ok(serde_json::json!({
                        "ok": true,
                        "id": id,
                        "message": "已有处理中的删除请求",
                    })),
                    DeletionRequestOutcome::Created { id, requested_at } => Ok(serde_json::json!({
                        "ok": true,
                        "id": id,
                        "status": "pending",
                        "requestedAt": requested_at,
                    })),
                }
            }
            "list" => {
                let unvalidated: Unvalidated<DeletionListInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let requests = self
                    .repository
                    .list(tenant_id, &input)
                    .map_err(Self::compatibility_error)?
                    .into_iter()
                    .map(|record| {
                        serde_json::json!({
                            "id": record.id,
                            "userId": record.user_id,
                            "requestType": record.request_type,
                            "status": record.status,
                            "reason": record.reason,
                            "requestedAt": record.requested_at,
                            "completedAt": record.completed_at,
                            "adminNotes": record.admin_notes,
                            "createdAt": record.created_at,
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(serde_json::json!({ "ok": true, "requests": requests }))
            }
            "process" => {
                let unvalidated: Unvalidated<DeletionAdminInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                self.repository
                    .process(tenant_id, &input)
                    .map_err(Self::compatibility_error)?;
                Ok(serde_json::json!({
                    "ok": true,
                    "id": input.request_id,
                    "status": "processing",
                }))
            }
            "complete" => {
                let unvalidated: Unvalidated<DeletionAdminInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let result = self
                    .repository
                    .complete(tenant_id, &input, &shanghai_now_iso())
                    .map_err(Self::compatibility_error)?;
                Ok(serde_json::json!({
                    "ok": true,
                    "id": input.request_id,
                    "status": "completed",
                    "completedAt": result.completed_at,
                    "identityId": result.identity_id,
                    "identityAnonymized": result.identity_anonymized,
                }))
            }
            "reject" => {
                let unvalidated: Unvalidated<DeletionAdminInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let rejected_at = shanghai_now_iso();
                self.repository
                    .reject(tenant_id, &input, &rejected_at)
                    .map_err(Self::compatibility_error)?;
                Ok(serde_json::json!({
                    "ok": true,
                    "id": input.request_id,
                    "status": "rejected",
                    "rejectedAt": rejected_at,
                }))
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "CMD_UNKNOWN".into(),
                message: format!("Unknown command: {command}"),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "request",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "list",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "process",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "complete",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "reject",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        FeatureDeletion::new().schema()
    }
}

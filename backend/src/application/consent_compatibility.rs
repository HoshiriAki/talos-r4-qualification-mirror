use serde_json::Value;
use system_admin::consent::{
    ConsentAuditInput, ConsentCheckInput, ConsentRecordInput, ConsentRevokeInput, FeatureConsent,
    require_self_service_actor,
};
use system_core::{
    AccessRequirement, CommandMetadata, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule, Unvalidated,
};

use crate::repositories::{ConsentCompatibilityRepository, RepositoryError};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct ConsentCompatibilityModule {
    repository: ConsentCompatibilityRepository,
}

impl ConsentCompatibilityModule {
    pub(crate) fn new(repository: ConsentCompatibilityRepository) -> Self {
        Self { repository }
    }

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "consent persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_else(|_| "SYS_CONSENT_PERSISTENCE".into())
    }
}

impl SystemModule for ConsentCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "consent".into(),
            version: "0.1.0".into(),
            description: "隐私同意书模块 — record/check/revoke/audit".into(),
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
            "record" => {
                let unvalidated: Unvalidated<ConsentRecordInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                let result = self
                    .repository
                    .record(
                        ctx.data_scope().tenant_id().as_str(),
                        &input,
                        &shanghai_now_iso(),
                    )
                    .map_err(Self::repository_error)?;
                Ok(serde_json::json!({
                    "ok": true,
                    "id": result.id,
                    "createdAt": result.created_at,
                }))
            }
            "check" => {
                let unvalidated: Unvalidated<ConsentCheckInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                let has_consented = self
                    .repository
                    .check(ctx.data_scope().tenant_id().as_str(), &input)
                    .map_err(Self::repository_error)?;
                Ok(serde_json::json!({
                    "ok": true,
                    "hasConsented": has_consented,
                }))
            }
            "revoke" => {
                let unvalidated: Unvalidated<ConsentRevokeInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                require_self_service_actor(ctx.actor().id(), &input.user_id)?;
                let result = self
                    .repository
                    .revoke(
                        ctx.data_scope().tenant_id().as_str(),
                        &input,
                        &shanghai_now_iso(),
                    )
                    .map_err(Self::repository_error)?;
                Ok(serde_json::json!({
                    "ok": true,
                    "affected": result.affected,
                    "revokedAt": result.revoked_at,
                }))
            }
            "audit" => {
                let unvalidated: Unvalidated<ConsentAuditInput> = payload.try_into()?;
                let validated = unvalidated.sanitize().validate()?;
                let input = validated.into_inner();
                let records = self
                    .repository
                    .audit(ctx.data_scope().tenant_id().as_str(), &input.user_id)
                    .map_err(Self::repository_error)?;
                let records = records
                    .into_iter()
                    .map(|record| {
                        serde_json::json!({
                            "id": record.id,
                            "userId": record.user_id,
                            "consentType": record.consent_type,
                            "version": record.version,
                            "consented": record.consented,
                            "ipAddress": record.ip_address,
                            "userAgent": record.user_agent,
                            "revokedAt": record.revoked_at,
                            "createdAt": record.created_at,
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(serde_json::json!({ "ok": true, "records": records }))
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
                "record",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "check",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "revoke",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "audit",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        FeatureConsent::new().schema()
    }
}

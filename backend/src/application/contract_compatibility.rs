use std::sync::Arc;

use serde_json::Value;
use system_admin::contract::{
    ContractGenerateInput, ContractGetInput, ContractListInput, ContractSignInput,
    ContractVerifyInput, FeatureContract, TemplateCreateInput, TemplateGetInput, TemplateListInput,
    TemplateUpdateInput,
};
use system_core::{
    ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule, Unvalidated,
};

use crate::repositories::{
    ContractMutationError, RepositoryError, RepositoryProvider, ScopedRepositories,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct ContractCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl ContractCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    fn scoped(&self, ctx: &ExecutionContext) -> Result<ScopedRepositories, String> {
        self.repository_provider
            .bind(ctx)
            .map_err(Self::repository_error)
    }

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "contract persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn error(category: &str, code: &str, message: String, field: Option<&str>) -> String {
        serde_json::to_string(&ErrorPayload {
            category: category.into(),
            code: code.into(),
            message,
            field: field.map(str::to_owned),
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: ContractMutationError, context: &'static str) -> String {
        match error {
            ContractMutationError::TemplateNotFound => {
                let message = if context == "generate" {
                    "合同模板不存在或已禁用"
                } else {
                    "模板不存在"
                };
                Self::error("biz", "BIZ_NOT_FOUND", message.into(), Some("templateId"))
            }
            ContractMutationError::OrderNotFound => Self::error(
                "biz",
                "BIZ_NOT_FOUND",
                "订单不属于当前租户".into(),
                Some("orderId"),
            ),
            ContractMutationError::ContractNotFound => {
                Self::error("biz", "BIZ_NOT_FOUND", "合同不存在".into(), Some("id"))
            }
            ContractMutationError::StatusInvalid(status) => Self::error(
                "biz",
                "BIZ_STATUS_INVALID",
                format!("当前状态 '{status}' 不可执行该操作"),
                Some("id"),
            ),
            ContractMutationError::NoSignature => Self::error(
                "biz",
                "BIZ_NO_SIGNATURE",
                "合同无有效签名记录".into(),
                Some("id"),
            ),
            ContractMutationError::Storage(error) => Self::repository_error(error),
        }
    }

    fn require_admin(ctx: &ExecutionContext) -> Result<(), String> {
        if ctx.has_tenant_admin_authority() {
            Ok(())
        } else {
            Err(Self::error(
                "auth",
                "AUTH_FORBIDDEN",
                "仅管理员可操作".into(),
                None,
            ))
        }
    }

    fn template_create(
        &self,
        ctx: &ExecutionContext,
        input: TemplateCreateInput,
    ) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .contracts()
            .template_create(&input.name, &input.content_json, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, "template"))?;
        Ok(serde_json::json!({
            "ok": true,
            "id": outcome.id,
            "createdAt": outcome.created_at,
        }))
    }

    fn template_update(
        &self,
        ctx: &ExecutionContext,
        input: TemplateUpdateInput,
    ) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .contracts()
            .template_update(
                input.id,
                input.name.as_deref(),
                input.content_json.as_ref(),
                input.is_active,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, "template"))?;
        Ok(serde_json::json!({
            "ok": true,
            "id": outcome.id,
            "updatedAt": outcome.updated_at,
        }))
    }

    fn template_list(
        &self,
        ctx: &ExecutionContext,
        input: TemplateListInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let result = scoped
            .contracts()
            .template_list(
                input.is_active,
                input.page.unwrap_or(1).max(1),
                input.page_size.unwrap_or(20).clamp(1, 100),
            )
            .map_err(Self::repository_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "items": result.items,
            "total": result.total,
            "page": result.page,
            "pageSize": result.page_size,
        }))
    }

    fn template_get(
        &self,
        ctx: &ExecutionContext,
        input: TemplateGetInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let record = scoped
            .contracts()
            .template_get(input.id)
            .map_err(Self::repository_error)?
            .ok_or_else(|| Self::error("biz", "BIZ_NOT_FOUND", "模板不存在".into(), Some("id")))?;
        Ok(serde_json::json!({
            "ok": true,
            "record": record,
        }))
    }

    fn generate(
        &self,
        ctx: &ExecutionContext,
        input: ContractGenerateInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .contracts()
            .generate(
                &input.order_id,
                input.template_id,
                &input.customer_name,
                &input.customer_phone,
                input.device_value,
                input.variables.as_ref(),
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, "generate"))?;
        Ok(serde_json::json!({
            "ok": true,
            "id": outcome.id,
            "status": outcome.status,
            "autoTriggered": outcome.auto_triggered,
            "createdAt": outcome.created_at,
        }))
    }

    fn sign(&self, ctx: &ExecutionContext, input: ContractSignInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .contracts()
            .sign(
                input.id,
                &input.signer_name,
                &input.signer_phone,
                &input.signature_data,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, "sign"))?;
        Ok(serde_json::json!({
            "ok": true,
            "id": outcome.id,
            "status": "signed",
            "signedAt": outcome.signed_at,
        }))
    }

    fn verify(&self, ctx: &ExecutionContext, input: ContractVerifyInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .contracts()
            .verify(input.id, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, "verify"))?;
        Ok(serde_json::json!({
            "ok": true,
            "id": outcome.id,
            "status": "verified",
            "verifiedAt": outcome.verified_at,
        }))
    }

    fn list(&self, ctx: &ExecutionContext, input: ContractListInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let result = scoped
            .contracts()
            .list(
                input.order_id.as_deref(),
                input.customer_phone.as_deref(),
                input.status.as_deref(),
                input.page.unwrap_or(1).max(1),
                input.page_size.unwrap_or(20).clamp(1, 100),
            )
            .map_err(Self::repository_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "items": result.items,
            "total": result.total,
            "page": result.page,
            "pageSize": result.page_size,
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: ContractGetInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let contract = scoped
            .contracts()
            .get(input.id)
            .map_err(Self::repository_error)?
            .ok_or_else(|| Self::error("biz", "BIZ_NOT_FOUND", "合同不存在".into(), Some("id")))?;

        let mut record = serde_json::to_value(&contract)
            .map_err(|error| Self::error("sys", "SYS_SERIALIZE", error.to_string(), None))?;
        let signatures = record
            .as_object_mut()
            .and_then(|object| object.remove("signatures"))
            .unwrap_or_else(|| serde_json::json!([]));

        Ok(serde_json::json!({
            "ok": true,
            "record": record,
            "signatures": signatures,
        }))
    }
}

impl SystemModule for ContractCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureContract::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureContract::new().commands()
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
            "template_create" => {
                let input: Unvalidated<TemplateCreateInput> = payload.try_into()?;
                self.template_create(ctx, input.sanitize().validate()?.into_inner())
            }
            "template_update" => {
                let input: Unvalidated<TemplateUpdateInput> = payload.try_into()?;
                self.template_update(ctx, input.sanitize().validate()?.into_inner())
            }
            "template_list" => {
                let input: Unvalidated<TemplateListInput> = payload.try_into()?;
                self.template_list(ctx, input.sanitize().validate()?.into_inner())
            }
            "template_get" => {
                let input: Unvalidated<TemplateGetInput> = payload.try_into()?;
                self.template_get(ctx, input.sanitize().validate()?.into_inner())
            }
            "contract_generate" => {
                let input: Unvalidated<ContractGenerateInput> = payload.try_into()?;
                self.generate(ctx, input.sanitize().validate()?.into_inner())
            }
            "contract_sign" => {
                let input: Unvalidated<ContractSignInput> = payload.try_into()?;
                self.sign(ctx, input.sanitize().validate()?.into_inner())
            }
            "contract_verify" => {
                let input: Unvalidated<ContractVerifyInput> = payload.try_into()?;
                self.verify(ctx, input.sanitize().validate()?.into_inner())
            }
            "contract_list" => {
                let input: Unvalidated<ContractListInput> = payload.try_into()?;
                self.list(ctx, input.sanitize().validate()?.into_inner())
            }
            "contract_get" => {
                let input: Unvalidated<ContractGetInput> = payload.try_into()?;
                self.get(ctx, input.sanitize().validate()?.into_inner())
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: contract.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureContract::new().schema()
    }
}

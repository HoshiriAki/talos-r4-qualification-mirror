use serde_json::Value;
use system_admin::staff::{
    CreateTenantMemberInput, ListTenantMembersInput, ResetPasswordInput, RevokeTenantMemberInput,
    ToggleTenantMemberInput, UpdateUsernameInput,
};
use system_core::{
    AccessRequirement, AuthorityContext, CommandMetadata, CommandSchema, DeserializeGuard,
    EffectClass, ErrorPayload, ExecutionContext, ExecutionMode, ModuleMetadata, ModuleSchema,
    SimulationSupport, SystemModule, Unvalidated,
};

use crate::repositories::{
    TenantMembershipActor, TenantMembershipAuthorityError, TenantMembershipAuthorityRepository,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct TenantMembershipCompatibilityModule {
    repository: TenantMembershipAuthorityRepository,
}

impl TenantMembershipCompatibilityModule {
    pub(crate) fn new(repository: TenantMembershipAuthorityRepository) -> Self {
        Self { repository }
    }

    fn actor(ctx: &ExecutionContext) -> Result<TenantMembershipActor, String> {
        if !matches!(ctx.execution_mode(), ExecutionMode::Normal) {
            return Err(error_payload(
                "sys",
                "SYS_TENANT_GOVERNANCE_MODE",
                "tenant membership governance is unavailable outside Normal execution",
                None,
            ));
        }
        let Some(AuthorityContext::Tenant {
            membership_id,
            tenant_id,
            role,
        }) = ctx.authority()
        else {
            return Err(error_payload(
                "val",
                "VAL_ROLE_ESCALATION",
                "当前成员角色不能管理目标角色",
                Some("role"),
            ));
        };
        if ctx.data_scope().tenant_id_opt() != Some(tenant_id) {
            return Err(error_payload(
                "sys",
                "SYS_TENANT_SCOPE_MISMATCH",
                "tenant membership authority scope mismatch",
                None,
            ));
        }
        let Some(identity_id) = ctx.user_id() else {
            return Err(error_payload(
                "sys",
                "SYS_TENANT_ACTOR_MISSING",
                "tenant membership actor identity unavailable",
                None,
            ));
        };
        Ok(TenantMembershipActor {
            tenant_id: tenant_id.as_str().to_owned(),
            membership_id: membership_id.as_str().to_owned(),
            identity_id: identity_id.to_owned(),
            role: *role,
        })
    }

    fn map_authority_error(error: TenantMembershipAuthorityError) -> String {
        match error {
            TenantMembershipAuthorityError::MembershipNotFound => {
                error_payload("val", "VAL_NOT_FOUND", "员工不存在", Some("id"))
            }
            TenantMembershipAuthorityError::DuplicateUsername => {
                error_payload("val", "VAL_DUPLICATE", "用户名已存在", Some("username"))
            }
            TenantMembershipAuthorityError::SelfReference => error_payload(
                "val",
                "VAL_SELF_REF",
                "不能删除当前登录管理员账号",
                Some("id"),
            ),
            TenantMembershipAuthorityError::SelfDisable => error_payload(
                "val",
                "VAL_SELF_DISABLE",
                "不能禁用当前登录管理员账号",
                Some("id"),
            ),
            TenantMembershipAuthorityError::LastHumanGovernance => error_payload(
                "val",
                "VAL_LAST_ADMIN",
                "不能移除最后一位人类治理成员",
                Some("role"),
            ),
            TenantMembershipAuthorityError::SharedIdentity => error_payload(
                "val",
                "VAL_SHARED_IDENTITY",
                "共享身份的账号名或凭证只能由身份管理流程修改",
                Some("id"),
            ),
            TenantMembershipAuthorityError::OwnerGovernanceRequired => error_payload(
                "val",
                "VAL_OWNER_GOVERNANCE_REQUIRED",
                "owner 只能通过租户所有权治理流程管理",
                Some("role"),
            ),
            TenantMembershipAuthorityError::RoleEscalation => error_payload(
                "val",
                "VAL_ROLE_ESCALATION",
                "当前成员角色不能管理目标角色",
                Some("role"),
            ),
            TenantMembershipAuthorityError::OwnershipTargetNotFound => {
                error_payload("val", "VAL_NOT_FOUND", "目标租户成员不存在", Some("id"))
            }
            TenantMembershipAuthorityError::MachineOwnerForbidden => error_payload(
                "auth",
                "MACHINE_OWNER_FORBIDDEN",
                "Machine identity cannot become tenant owner",
                Some("id"),
            ),
            TenantMembershipAuthorityError::InvalidOwnershipTarget => error_payload(
                "val",
                "VAL_OWNERSHIP_TARGET",
                "只能把所有权转移给有效的 admin 或 staff 成员",
                Some("id"),
            ),
            TenantMembershipAuthorityError::StaleOwner => error_payload(
                "biz",
                "BIZ_TENANT_OWNER_STALE",
                "当前 Owner 状态已变化，请刷新后重试",
                None,
            ),
            TenantMembershipAuthorityError::Storage(storage) => error_payload(
                "sys",
                storage.code(),
                "tenant membership persistence unavailable",
                None,
            ),
        }
    }

    fn serialize<T: serde::Serialize>(value: &T) -> Result<Value, String> {
        serde_json::to_value(value).map_err(|_| {
            error_payload(
                "sys",
                "SYS_SERIALIZE",
                "tenant membership response serialization failed",
                None,
            )
        })
    }
}

impl SystemModule for TenantMembershipCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "staff".into(),
            version: "0.1.0".into(),
            description: "员工管理模块 — backend-neutral tenant membership authority".into(),
            author: "TALOS".into(),
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
        let actor = Self::actor(ctx)?;
        let now = shanghai_now_iso();
        match command {
            "list_tenant_members" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ListTenantMembersInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let _validated = sanitized.validate()?;
                let result = self
                    .repository
                    .list(&actor.tenant_id)
                    .map_err(Self::map_authority_error)?;
                Self::serialize(&result)
            }
            "create_tenant_member" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<CreateTenantMemberInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let result = self
                    .repository
                    .create(&actor, &validated.into_inner(), &now)
                    .map_err(Self::map_authority_error)?;
                Self::serialize(&result)
            }
            "delete_tenant_member" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<RevokeTenantMemberInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let result = self
                    .repository
                    .delete(&actor, &validated.into_inner(), &now)
                    .map_err(Self::map_authority_error)?;
                Self::serialize(&result)
            }
            "toggle_tenant_member" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ToggleTenantMemberInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let result = self
                    .repository
                    .toggle(&actor, &validated.into_inner(), &now)
                    .map_err(Self::map_authority_error)?;
                Self::serialize(&result)
            }
            "reset_password" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<ResetPasswordInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let result = self
                    .repository
                    .reset_password(&actor, &validated.into_inner(), &now)
                    .map_err(Self::map_authority_error)?;
                Self::serialize(&result)
            }
            "update_username" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<UpdateUsernameInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let result = self
                    .repository
                    .update_username(&actor, &validated.into_inner(), &now)
                    .map_err(Self::map_authority_error)?;
                Self::serialize(&result)
            }
            "transfer_ownership" => {
                DeserializeGuard::default().check_raw(&payload)?;
                let target_membership_id = payload
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        error_payload("val", "VAL_REQUIRED", "目标租户成员不能为空", Some("id"))
                    })?;
                let result = self
                    .repository
                    .transfer_ownership(&actor, target_membership_id, &now)
                    .map_err(Self::map_authority_error)?;
                Ok(serde_json::json!({
                    "tenantId": result.tenant_id,
                    "previousOwnerMembershipId": result.previous_owner_membership_id,
                    "ownerMembershipId": result.owner_membership_id,
                }))
            }
            _ => Err(error_payload(
                "sys",
                "SYS_UNKNOWN_COMMAND",
                &format!("未知命令: {command}"),
                None,
            )),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "list_tenant_members",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "create_tenant_member",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "delete_tenant_member",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "toggle_tenant_member",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "reset_password",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "update_username",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "transfer_ownership",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "staff".into(),
            description: "员工管理模块 — backend-neutral tenant membership authority".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|command| CommandSchema {
                    name: command.name.into(),
                    description: "Tenant membership compatibility command".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                })
                .collect(),
        }
    }
}

fn error_payload(category: &str, code: &str, message: &str, field: Option<&str>) -> String {
    serde_json::to_string(&ErrorPayload {
        category: category.into(),
        code: code.into(),
        message: message.into(),
        field: field.map(str::to_owned),
        context: None,
    })
    .unwrap_or_else(|_| {
        r#"{"category":"sys","code":"SYS_SERIALIZE","message":"failed to serialize tenant membership error","field":null,"context":null}"#.into()
    })
}

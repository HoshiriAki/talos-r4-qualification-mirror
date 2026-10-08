use serde::Serialize;
use system_core::TenantRole;

#[derive(Clone)]
pub(crate) struct MachineScopeRecord {
    pub module: String,
    pub command: String,
}

#[derive(Clone)]
pub(crate) struct MachineAdminContext {
    pub identity_id: String,
    pub membership_id: String,
    pub tenant_id: String,
    pub role: TenantRole,
}

#[derive(Clone)]
pub(crate) struct MachineProvisionRecord {
    pub name: String,
    pub scopes: Vec<MachineScopeRecord>,
    pub rate_limit_rpm: u16,
    pub role: TenantRole,
}

#[derive(Clone, Serialize)]
pub(crate) struct MachineIssuedCredential {
    pub client_id: String,
    pub credential_id: String,
    pub secret: String,
    pub expires_at: String,
}

#[derive(Clone, Debug)]
pub(crate) struct MachineAuthorization {
    pub identity_id: String,
    pub membership_id: String,
    pub tenant_id: String,
    pub role: TenantRole,
}

#[derive(Clone, Serialize)]
pub(crate) struct MachineClientProjection {
    pub client_id: String,
    pub identity_id: String,
    pub name: String,
    pub status: String,
    pub api_version: String,
    pub rate_limit_rpm: u32,
    pub last_used_at: Option<String>,
    pub window_usage: u32,
}

pub(crate) fn machine_role_name(role: TenantRole) -> &'static str {
    match role {
        TenantRole::Staff => "staff",
        TenantRole::Admin | TenantRole::Owner => "admin",
    }
}

pub(crate) fn audit_role_name(role: TenantRole) -> &'static str {
    match role {
        TenantRole::Staff => "staff",
        TenantRole::Admin => "admin",
        TenantRole::Owner => "owner",
    }
}

pub(crate) fn machine_scope_allowed(scope: &MachineScopeRecord) -> bool {
    !matches!(
        scope.module.as_str(),
        "two_fa" | "consent" | "deletion" | "user_settings"
    )
}

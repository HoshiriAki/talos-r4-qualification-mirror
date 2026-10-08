use system_core::{AuthorityContext, PlatformCapability, PlatformRole, TenantRole};

#[derive(Debug, Clone)]
pub struct AuthUserInfo {
    pub id: String,
    pub username: String,
    pub session_id: String,
    pub display_name: String,
    pub email: String,
    pub phone: String,
    pub authority: AuthorityContext,
}

impl AuthUserInfo {
    pub fn has_platform_capability(&self, capability: PlatformCapability) -> bool {
        self.authority.allows(capability)
    }

    pub fn tenant_id(&self) -> Option<&str> {
        match &self.authority {
            AuthorityContext::Tenant { tenant_id, .. } => Some(tenant_id.as_str()),
            AuthorityContext::Platform { .. } => None,
        }
    }

    pub fn tenant_role(&self) -> Option<TenantRole> {
        match self.authority {
            AuthorityContext::Tenant { role, .. } => Some(role),
            AuthorityContext::Platform { .. } => None,
        }
    }

    pub fn platform_roles(&self) -> &[PlatformRole] {
        match &self.authority {
            AuthorityContext::Platform { roles, .. } => roles,
            AuthorityContext::Tenant { .. } => &[],
        }
    }

    pub fn authority_label(&self) -> String {
        match &self.authority {
            AuthorityContext::Tenant { role, .. } => format!("{role:?}").to_lowercase(),
            AuthorityContext::Platform { roles, .. } => roles
                .first()
                .map(|role| format!("{role:?}").to_lowercase())
                .unwrap_or_else(|| "platform".into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IdentityAccount {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub authority_role: String,
    pub is_enabled: bool,
    pub display_name: String,
    pub email: String,
    pub phone: String,
    pub tenant_scope_id: Option<String>,
}

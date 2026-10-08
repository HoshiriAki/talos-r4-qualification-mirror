use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::authority::PreviewSessionId;
use crate::transport::http_client::HttpClient;
use crate::{AuthorityContext, PlatformCapability, PlatformRole, TenantRole};

macro_rules! opaque_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(concat!(stringify!($name), " must not be blank").to_string());
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

opaque_id!(Revision);
opaque_id!(RequestId);
opaque_id!(SimulationId);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TenantId(String);

impl TenantId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err("TenantId must not be blank".to_string());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for TenantId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Namespace {
    Production,
    Simulation(SimulationId),
}

impl Namespace {
    pub fn production() -> Self {
        Self::Production
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ScopeTarget {
    Platform,
    Tenant(TenantId),
}

impl ScopeTarget {
    fn tenant_id(&self) -> Option<&TenantId> {
        match self {
            Self::Platform => None,
            Self::Tenant(tenant_id) => Some(tenant_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataScope {
    target: ScopeTarget,
    namespace: Namespace,
    base_revision: Revision,
    resolved: bool,
}

impl DataScope {
    pub fn new(
        tenant_id: TenantId,
        namespace: Namespace,
        base_revision: Revision,
    ) -> Result<Self, String> {
        Ok(Self {
            target: ScopeTarget::Tenant(tenant_id),
            namespace,
            base_revision,
            resolved: true,
        })
    }
    pub fn production(tenant_id: TenantId, revision: Revision) -> Result<Self, String> {
        Self::new(tenant_id, Namespace::Production, revision)
    }
    pub fn platform(revision: Revision) -> Self {
        Self {
            target: ScopeTarget::Platform,
            namespace: Namespace::Production,
            base_revision: revision,
            resolved: true,
        }
    }
    pub fn tenant_id(&self) -> &TenantId {
        self.tenant_id_opt()
            .expect("tenant_id is only available for tenant data scopes")
    }
    pub fn tenant_id_opt(&self) -> Option<&TenantId> {
        self.target.tenant_id()
    }
    pub fn namespace(&self) -> &Namespace {
        &self.namespace
    }
    pub fn base_revision(&self) -> &Revision {
        &self.base_revision
    }
    pub fn is_resolved(&self) -> bool {
        self.resolved
    }
    pub fn is_platform(&self) -> bool {
        matches!(self.target, ScopeTarget::Platform)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenantScope {
    target: ScopeTarget,
}
impl TenantScope {
    pub fn tenant(effective_tenant_id: TenantId) -> Self {
        Self {
            target: ScopeTarget::Tenant(effective_tenant_id),
        }
    }
    pub fn platform() -> Self {
        Self {
            target: ScopeTarget::Platform,
        }
    }
    pub fn effective_tenant_id(&self) -> &TenantId {
        self.effective_tenant_id_opt()
            .expect("effective_tenant_id is only available for tenant scopes")
    }
    pub fn effective_tenant_id_opt(&self) -> Option<&TenantId> {
        self.target.tenant_id()
    }
    pub fn is_platform(&self) -> bool {
        matches!(self.target, ScopeTarget::Platform)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorIdentity {
    id: Option<String>,
    role: Option<String>,
    authority: Option<AuthorityContext>,
}
impl ActorIdentity {
    pub fn authenticated(id: impl Into<String>, role: impl Into<String>) -> Result<Self, String> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err("actor id must not be blank".into());
        }
        Ok(Self {
            id: Some(id),
            role: Some(role.into()),
            authority: None,
        })
    }
    pub fn with_authority(
        id: impl Into<String>,
        authority: AuthorityContext,
    ) -> Result<Self, String> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err("actor id must not be blank".into());
        }
        let role = match &authority {
            AuthorityContext::Tenant { role, .. } => match role {
                TenantRole::Staff => "staff",
                TenantRole::Admin => "admin",
                TenantRole::Owner => "owner",
            },
            AuthorityContext::Platform { roles, .. } => match roles.first() {
                Some(PlatformRole::Owner) => "platform_owner",
                Some(PlatformRole::Admin) => "platform_admin",
                Some(PlatformRole::Operator) => "platform_operator",
                Some(PlatformRole::SupportEngineer) => "support_engineer",
                Some(PlatformRole::BusinessOperator) => "business_operator",
                Some(PlatformRole::SecurityAuditor) => "security_auditor",
                None => return Err("platform authority requires at least one role".into()),
            },
        };
        Ok(Self {
            id: Some(id),
            role: Some(role.into()),
            authority: Some(authority),
        })
    }
    pub fn system() -> Self {
        Self {
            id: None,
            role: None,
            authority: None,
        }
    }
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }
    pub fn authority(&self) -> Option<&AuthorityContext> {
        self.authority.as_ref()
    }
    pub fn has_platform_authority(&self) -> bool {
        matches!(self.authority, Some(AuthorityContext::Platform { .. }))
    }
    pub fn has_platform_capability(&self, capability: PlatformCapability) -> bool {
        self.authority
            .as_ref()
            .is_some_and(|authority| authority.allows(capability))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    Normal,
    ReadOnlyPreview(PreviewSessionId),
    Simulation(SimulationId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionPlane {
    PlatformControl,
    TenantBusiness,
    Simulation,
}

pub struct ExecutionContext {
    actor: ActorIdentity,
    authority: Option<AuthorityContext>,
    plane: ExecutionPlane,
    tenant_scope: TenantScope,
    data_scope: DataScope,
    execution_mode: ExecutionMode,
    correlation_id: RequestId,
    idempotency_key: Option<String>,
    http_client: Arc<dyn HttpClient>,
}

impl ExecutionContext {
    pub fn new(
        actor: ActorIdentity,
        tenant_scope: TenantScope,
        data_scope: DataScope,
        execution_mode: ExecutionMode,
        correlation_id: RequestId,
        idempotency_key: Option<String>,
        http_client: Arc<dyn HttpClient>,
    ) -> Result<Self, String> {
        if !data_scope.is_resolved() {
            return Err("execution context requires a resolved data scope".into());
        }
        if tenant_scope.effective_tenant_id_opt() != data_scope.tenant_id_opt() {
            return Err("tenant scope and data scope must identify the same tenant".into());
        }
        match (&execution_mode, data_scope.namespace()) {
            (ExecutionMode::Simulation(mode_id), Namespace::Simulation(scope_id))
                if mode_id == scope_id => {}
            (ExecutionMode::Simulation(_), _) => {
                return Err("simulation mode requires its matching simulation namespace".into());
            }
            (_, Namespace::Simulation(_)) => {
                return Err("simulation namespace requires matching simulation mode".into());
            }
            _ => {}
        }
        let authority = actor.authority().cloned();
        let plane = match execution_mode {
            ExecutionMode::Simulation(_) => ExecutionPlane::Simulation,
            _ if tenant_scope.is_platform() => ExecutionPlane::PlatformControl,
            _ => ExecutionPlane::TenantBusiness,
        };
        match (&execution_mode, &authority, plane) {
            (
                ExecutionMode::ReadOnlyPreview(_),
                Some(AuthorityContext::Platform { .. }),
                ExecutionPlane::TenantBusiness,
            ) => {}
            (ExecutionMode::ReadOnlyPreview(_), _, _) => {
                return Err(
                    "read-only preview requires platform authority over a tenant business scope"
                        .into(),
                );
            }
            (
                ExecutionMode::Simulation(_),
                Some(AuthorityContext::Platform { .. }),
                ExecutionPlane::Simulation,
            ) => {}
            (ExecutionMode::Simulation(_), _, _) => {
                return Err("simulation requires platform authority".into());
            }
            (
                ExecutionMode::Normal,
                Some(AuthorityContext::Tenant { .. }),
                ExecutionPlane::PlatformControl,
            ) => {
                return Err("tenant authority cannot enter the platform control plane".into());
            }
            _ => {}
        }
        Ok(Self {
            actor,
            authority,
            plane,
            tenant_scope,
            data_scope,
            execution_mode,
            correlation_id,
            idempotency_key,
            http_client,
        })
    }
    pub fn actor(&self) -> &ActorIdentity {
        &self.actor
    }
    pub fn authority(&self) -> Option<&AuthorityContext> {
        self.authority.as_ref()
    }
    pub fn plane(&self) -> ExecutionPlane {
        self.plane
    }
    pub fn tenant_scope(&self) -> &TenantScope {
        &self.tenant_scope
    }
    pub fn data_scope(&self) -> &DataScope {
        &self.data_scope
    }
    pub fn execution_mode(&self) -> &ExecutionMode {
        &self.execution_mode
    }
    pub fn correlation_id(&self) -> &RequestId {
        &self.correlation_id
    }
    pub fn idempotency_key(&self) -> Option<&str> {
        self.idempotency_key.as_deref()
    }
    pub fn http_client(&self) -> &Arc<dyn HttpClient> {
        &self.http_client
    }
    pub fn user_id(&self) -> Option<&str> {
        self.actor.id()
    }
    pub fn role(&self) -> Option<&str> {
        self.actor.role()
    }
    pub fn tenant_role(&self) -> Option<TenantRole> {
        match self.authority() {
            Some(AuthorityContext::Tenant { role, .. }) => Some(*role),
            _ => None,
        }
    }
    pub fn has_tenant_admin_authority(&self) -> bool {
        matches!(
            self.tenant_role(),
            Some(TenantRole::Admin | TenantRole::Owner)
        )
    }
    pub fn has_resolved_data_scope(&self) -> bool {
        self.data_scope.is_resolved()
    }
}

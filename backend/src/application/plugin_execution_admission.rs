//! Public R4-P6 plugin execution admission token.
//!
//! The inner permission authority is produced only by the persistence-bound
//! admission service. This wrapper additionally pins the sovereign facts from
//! the exact `ExecutionContext` that produced the admission and the exact
//! host-composed runtime instance that produced it. A token therefore cannot be
//! transferred to another actor/mode/DataScope lifecycle or to a second runtime
//! whose egress/storage/lane policy may differ.

use std::sync::Arc;

use system_core::security::plugin::{
    EffectivePluginPermissions, IsolationPolicy, PluginPermission,
};
use system_core::transport::interconnect::PluginExecutableRef;
use system_core::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, RequestId, TenantId, TenantScope,
};

use super::plugin_host::{PluginAdmission, PluginHostError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginExecutionAdmissionError {
    ContextMismatch,
    RuntimeMismatch,
    Host(PluginHostError),
}

impl From<PluginHostError> for PluginExecutionAdmissionError {
    fn from(value: PluginHostError) -> Self {
        Self::Host(value)
    }
}

/// Process-local, unforgeable binding shared only by facets created as part of
/// one `PluginRuntimeServices` composition. Pointer identity is intentional:
/// two independently constructed runtimes are never equivalent merely because
/// their policy values happen to compare equal.
#[derive(Debug, Clone)]
pub(crate) struct PluginExecutionRuntimeBinding(Arc<()>);

impl PluginExecutionRuntimeBinding {
    pub(crate) fn new() -> Self {
        Self(Arc::new(()))
    }

    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl PartialEq for PluginExecutionRuntimeBinding {
    fn eq(&self, other: &Self) -> bool {
        self.matches(other)
    }
}

impl Eq for PluginExecutionRuntimeBinding {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginExecutionContextBinding {
    actor: ActorIdentity,
    tenant_scope: TenantScope,
    data_scope: DataScope,
    execution_mode: ExecutionMode,
    correlation_id: RequestId,
    idempotency_key: Option<String>,
}

impl PluginExecutionContextBinding {
    fn from_context(context: &ExecutionContext) -> Self {
        Self {
            actor: context.actor().clone(),
            tenant_scope: context.tenant_scope().clone(),
            data_scope: context.data_scope().clone(),
            execution_mode: context.execution_mode().clone(),
            correlation_id: context.correlation_id().clone(),
            idempotency_key: context.idempotency_key().map(str::to_owned),
        }
    }

    fn matches(&self, context: &ExecutionContext) -> bool {
        &self.actor == context.actor()
            && &self.tenant_scope == context.tenant_scope()
            && &self.data_scope == context.data_scope()
            && &self.execution_mode == context.execution_mode()
            && &self.correlation_id == context.correlation_id()
            && self.idempotency_key.as_deref() == context.idempotency_key()
    }
}

/// Unforgeable public plugin execution token. No public constructor or mutable
/// field exists. The token can only be produced after exact persisted package /
/// installation resolution plus the six-way permission intersection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginExecutionAdmission {
    authority: PluginAdmission,
    context_binding: PluginExecutionContextBinding,
    runtime_binding: PluginExecutionRuntimeBinding,
}

impl PluginExecutionAdmission {
    pub(crate) fn new(
        authority: PluginAdmission,
        context: &ExecutionContext,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        Self {
            authority,
            context_binding: PluginExecutionContextBinding::from_context(context),
            runtime_binding,
        }
    }

    pub fn installation_id(&self) -> &str {
        &self.authority.installation_id
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.authority.tenant_id
    }

    pub fn executable(&self) -> &PluginExecutableRef {
        &self.authority.executable
    }

    pub fn isolation(&self) -> &IsolationPolicy {
        &self.authority.isolation
    }

    pub fn permissions(&self) -> &EffectivePluginPermissions {
        &self.authority.permissions
    }

    pub fn require_context(
        &self,
        context: &ExecutionContext,
    ) -> Result<(), PluginExecutionAdmissionError> {
        if self.context_binding.matches(context) {
            Ok(())
        } else {
            Err(PluginExecutionAdmissionError::ContextMismatch)
        }
    }

    pub(crate) fn require_runtime(
        &self,
        runtime_binding: &PluginExecutionRuntimeBinding,
    ) -> Result<(), PluginExecutionAdmissionError> {
        if self.runtime_binding.matches(runtime_binding) {
            Ok(())
        } else {
            Err(PluginExecutionAdmissionError::RuntimeMismatch)
        }
    }

    pub fn authorize(
        &self,
        context: &ExecutionContext,
        permission: &PluginPermission,
    ) -> Result<(), PluginExecutionAdmissionError> {
        self.require_context(context)?;
        self.authority.authorize(permission).map_err(Into::into)
    }

    pub(crate) fn authority(&self) -> &PluginAdmission {
        &self.authority
    }
}

#[cfg(test)]
mod tests {
    use system_core::security::plugin::{
        EffectivePluginPermissions, IsolationEnforcement, PermissionSet, PluginIsolationProfile,
    };
    use system_core::transport::interconnect::{ContractVersion, PluginId};
    use system_core::{AuthorityContext, NoopHttpClient, Revision, TenantMembershipId, TenantRole};

    use super::*;

    fn permission() -> PluginPermission {
        PluginPermission::CapabilityInvoke("orders.read".into())
    }

    fn context(actor: &str, correlation: &str) -> ExecutionContext {
        let tenant = TenantId::new("tenant-a").unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                actor,
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new(format!("membership-{actor}")).unwrap(),
                    tenant_id: tenant.clone(),
                    role: TenantRole::Staff,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(correlation).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn admission(
        context: &ExecutionContext,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> PluginExecutionAdmission {
        PluginExecutionAdmission::new(
            PluginAdmission {
                installation_id: "install-a".into(),
                tenant_id: TenantId::new("tenant-a").unwrap(),
                executable: PluginExecutableRef {
                    plugin_id: PluginId::new("official.fixture").unwrap(),
                    version: ContractVersion::new("1.0.0").unwrap(),
                    package_digest_sha256: "ab".repeat(32),
                    manifest_digest_sha256: "ef".repeat(32),
                    capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
                    provider_instance_id: None,
                    binding_revision: None,
                },
                isolation: IsolationPolicy {
                    profile: PluginIsolationProfile::FirstPartyNative,
                    enabled_in_r4: true,
                    enforcement: IsolationEnforcement::TrustedBuildBoundary,
                    requires_publisher_verification: true,
                    ambient_filesystem: false,
                    direct_core_db: false,
                    direct_network: false,
                    mediated_talos_data: true,
                    governed_egress_relay: true,
                    cloud_enabled: true,
                },
                permissions: EffectivePluginPermissions {
                    effective: PermissionSet::new([permission()]).unwrap(),
                    denied_requested: PermissionSet::empty(),
                },
            },
            context,
            runtime_binding,
        )
    }

    #[test]
    fn execution_admission_is_bound_to_actor_correlation_and_runtime() {
        let original = context("staff-a", "corr-a");
        let runtime = PluginExecutionRuntimeBinding::new();
        let admission = admission(&original, runtime.clone());
        assert_eq!(admission.authorize(&original, &permission()), Ok(()));
        assert_eq!(admission.require_runtime(&runtime), Ok(()));

        let other_actor = context("staff-b", "corr-a");
        assert_eq!(
            admission.authorize(&other_actor, &permission()),
            Err(PluginExecutionAdmissionError::ContextMismatch)
        );
        let other_correlation = context("staff-a", "corr-b");
        assert_eq!(
            admission.authorize(&other_correlation, &permission()),
            Err(PluginExecutionAdmissionError::ContextMismatch)
        );
        assert_eq!(
            admission.require_runtime(&PluginExecutionRuntimeBinding::new()),
            Err(PluginExecutionAdmissionError::RuntimeMismatch)
        );
    }
}

//! R4-P6 trusted plugin-host admission boundary.
//!
//! Persistence supplies package and tenant-installation facts. The host binds
//! those facts to the trusted ExecutionContext, verifies the exact P3 executable
//! pin, resolves the six required permission terms and returns an immutable
//! internal authority token. The public persistence-bound admission layer wraps
//! this token with the originating execution-context identity.

use system_core::security::plugin::{
    EffectivePluginPermissions, IsolationPolicy, PermissionResolutionInput, PermissionSet,
    PluginIsolationProfile, PluginPackageIdentity, PluginPackageRecord, PluginPermission,
    execution_plane_permission_policy, r4_isolation_policy, resolve_effective_permissions,
};
use system_core::security::plugin_compatibility::host_contract_is_compatible;
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef};
use system_core::{ExecutionContext, TenantId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginInstallationLifecycle {
    Staged,
    Active,
    Suspended,
    Revoked,
}

impl PluginInstallationLifecycle {
    fn is_executable(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInstallationRecord {
    pub installation_id: String,
    pub tenant_id: TenantId,
    pub package_identity: PluginPackageIdentity,
    pub isolation_profile: PluginIsolationProfile,
    pub installation_grant: PermissionSet,
    pub tenant_policy: PermissionSet,
    pub grant_revision: u64,
    pub tenant_policy_revision: u64,
    pub lifecycle: PluginInstallationLifecycle,
}

impl PluginInstallationRecord {
    pub fn validate(&self) -> Result<(), PluginHostError> {
        if self.installation_id.trim().is_empty()
            || self.grant_revision == 0
            || self.tenant_policy_revision == 0
        {
            return Err(PluginHostError::InvalidInstallation);
        }
        self.installation_grant
            .validate()
            .map_err(|_| PluginHostError::InvalidInstallation)?;
        self.tenant_policy
            .validate()
            .map_err(|_| PluginHostError::InvalidInstallation)?;
        Ok(())
    }
}

/// Trusted policy supplied by the host composition boundary, never by plugin
/// package/manifest/message data. `principal_authority` is invocation-local:
/// the public admission service clears any cached template value and resolves it
/// again from trusted identity governance for each admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPluginRuntimePolicy {
    pub host_contract_version: ContractVersion,
    pub principal_authority: PermissionSet,
    pub tenant_business_plane: PermissionSet,
    pub platform_control_plane: PermissionSet,
    pub simulation_plane: PermissionSet,
    pub runtime_policy: PermissionSet,
}

impl TrustedPluginRuntimePolicy {
    fn plane_policy(&self, context: &ExecutionContext) -> PermissionSet {
        execution_plane_permission_policy(
            context.plane(),
            self.tenant_business_plane.clone(),
            self.platform_control_plane.clone(),
            self.simulation_plane.clone(),
        )
    }

    fn validate(&self) -> Result<(), PluginHostError> {
        for set in [
            &self.principal_authority,
            &self.tenant_business_plane,
            &self.platform_control_plane,
            &self.simulation_plane,
            &self.runtime_policy,
        ] {
            set.validate()
                .map_err(|_| PluginHostError::InvalidTrustedPolicy)?;
        }
        Ok(())
    }
}

/// Internal authority token. Fields are crate-visible for tightly scoped host
/// adapters/tests, but callers outside the backend crate cannot construct or
/// mutate an admission. The public `PluginExecutionAdmission` is created only by
/// the persistence-bound admission service and adds exact ExecutionContext
/// binding before any externally callable plugin operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginAdmission {
    pub(crate) installation_id: String,
    pub(crate) tenant_id: TenantId,
    pub(crate) executable: PluginExecutableRef,
    pub(crate) isolation: IsolationPolicy,
    pub(crate) permissions: EffectivePluginPermissions,
}

impl PluginAdmission {
    pub(crate) fn authorize(&self, permission: &PluginPermission) -> Result<(), PluginHostError> {
        permission
            .validate()
            .map_err(|_| PluginHostError::PermissionDenied)?;
        if self.permissions.effective.contains(permission) {
            Ok(())
        } else {
            Err(PluginHostError::PermissionDenied)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginHostError {
    InvalidInstallation,
    InvalidTrustedPolicy,
    TenantScopeMismatch,
    PackageIdentityMismatch,
    ProviderBindingAuthorityUnavailable,
    HostContractIncompatible,
    PackageNotExecutable,
    InstallationNotExecutable,
    PermissionResolutionFailed,
    PermissionDenied,
}

pub(crate) struct PluginHost;

impl PluginHost {
    pub(crate) fn admit(
        context: &ExecutionContext,
        executable: &PluginExecutableRef,
        package: &PluginPackageRecord,
        installation: &PluginInstallationRecord,
        trusted_policy: &TrustedPluginRuntimePolicy,
    ) -> Result<PluginAdmission, PluginHostError> {
        executable
            .validate()
            .map_err(|_| PluginHostError::PackageIdentityMismatch)?;

        // P3 deliberately permits an immutable ProviderInstanceId / BindingRevision
        // pair to be carried by durable executable pins. P6 does not yet expose
        // an exact, host-owned Integration binding resolver at this admission
        // seam. Accepting the pair merely because the caller supplied two valid
        // tokens would turn immutable pinning into caller-selected authority.
        // Keep provider-bound plugin execution fail-closed until that resolver
        // exists; unbound fixture/native plugins remain executable under P6.
        if executable.provider_instance_id.is_some() {
            return Err(PluginHostError::ProviderBindingAuthorityUnavailable);
        }

        package
            .validate()
            .map_err(|_| PluginHostError::PackageNotExecutable)?;
        installation.validate()?;
        trusted_policy.validate()?;

        let context_tenant = context
            .data_scope()
            .tenant_id_opt()
            .ok_or(PluginHostError::TenantScopeMismatch)?;
        if context_tenant != &installation.tenant_id
            || context.tenant_scope().effective_tenant_id_opt() != Some(&installation.tenant_id)
        {
            return Err(PluginHostError::TenantScopeMismatch);
        }

        if package.identity != installation.package_identity
            || !package.identity.matches_executable(executable)
        {
            return Err(PluginHostError::PackageIdentityMismatch);
        }
        if !installation.lifecycle.is_executable() {
            return Err(PluginHostError::InstallationNotExecutable);
        }

        let compatible = host_contract_is_compatible(
            &package.compatibility_range,
            &trusted_policy.host_contract_version,
        )
        .map_err(|_| PluginHostError::HostContractIncompatible)?;
        if !compatible {
            return Err(PluginHostError::HostContractIncompatible);
        }

        package
            .executable_in_profile(installation.isolation_profile)
            .map_err(|_| PluginHostError::PackageNotExecutable)?;

        let permissions = resolve_effective_permissions(&PermissionResolutionInput {
            manifest_request: package.permission_request.clone(),
            installation_grant: installation.installation_grant.clone(),
            tenant_policy: installation.tenant_policy.clone(),
            principal_authority: trusted_policy.principal_authority.clone(),
            execution_plane_policy: trusted_policy.plane_policy(context),
            runtime_policy: trusted_policy.runtime_policy.clone(),
        })
        .map_err(|_| PluginHostError::PermissionResolutionFailed)?;

        Ok(PluginAdmission {
            installation_id: installation.installation_id.clone(),
            tenant_id: installation.tenant_id.clone(),
            executable: executable.clone(),
            isolation: r4_isolation_policy(installation.isolation_profile),
            permissions,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use system_core::security::plugin::{
        CompatibilityRange, PermissionSet, PluginLifecycle, PluginPackageIdentity,
        PluginPackageRecord, PublisherId, Sha256Digest, VerificationEvidence,
    };
    use system_core::transport::interconnect::{BindingRevisionRef, PluginId, ProviderInstanceRef};
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode,
        NoopHttpClient, RequestId, Revision, TenantMembershipId, TenantRole, TenantScope,
    };

    use super::*;

    fn permission() -> PluginPermission {
        PluginPermission::CapabilityInvoke("orders.read".into())
    }

    fn set(values: impl IntoIterator<Item = PluginPermission>) -> PermissionSet {
        PermissionSet::new(values).unwrap()
    }

    fn package(verification: VerificationEvidence) -> PluginPackageRecord {
        PluginPackageRecord {
            identity: PluginPackageIdentity {
                plugin_id: PluginId::new("official.fixture").unwrap(),
                publisher_id: PublisherId::new("talos.official").unwrap(),
                version: ContractVersion::new("1.0.0").unwrap(),
                package_digest_sha256: Sha256Digest::new("ab".repeat(32)).unwrap(),
                manifest_digest_sha256: Sha256Digest::new("ef".repeat(32)).unwrap(),
                capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
            },
            compatibility_range: CompatibilityRange::new(">=1.0.0,<2.0.0").unwrap(),
            declared_capabilities: BTreeSet::from(["orders.read".into()]),
            permission_request: set([permission()]),
            verification,
            lifecycle: PluginLifecycle::Active,
        }
    }

    fn executable() -> PluginExecutableRef {
        PluginExecutableRef {
            plugin_id: PluginId::new("official.fixture").unwrap(),
            version: ContractVersion::new("1.0.0").unwrap(),
            package_digest_sha256: "ab".repeat(32),
            manifest_digest_sha256: "ef".repeat(32),
            capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
            provider_instance_id: None,
            binding_revision: None,
        }
    }

    fn installation(profile: PluginIsolationProfile) -> PluginInstallationRecord {
        PluginInstallationRecord {
            installation_id: "install-a".into(),
            tenant_id: TenantId::new("tenant-a").unwrap(),
            package_identity: package(VerificationEvidence::DigestVerified).identity,
            isolation_profile: profile,
            installation_grant: set([permission()]),
            tenant_policy: set([permission()]),
            grant_revision: 1,
            tenant_policy_revision: 1,
            lifecycle: PluginInstallationLifecycle::Active,
        }
    }

    fn context(tenant: &str) -> ExecutionContext {
        let tenant = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "staff-a",
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new("membership-a").unwrap(),
                    tenant_id: tenant.clone(),
                    role: TenantRole::Staff,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new("corr-a").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn trusted() -> TrustedPluginRuntimePolicy {
        let allow = set([permission()]);
        TrustedPluginRuntimePolicy {
            host_contract_version: ContractVersion::new("1.0.0").unwrap(),
            principal_authority: allow.clone(),
            tenant_business_plane: allow.clone(),
            platform_control_plane: PermissionSet::empty(),
            simulation_plane: PermissionSet::empty(),
            runtime_policy: allow,
        }
    }

    fn verified_package() -> PluginPackageRecord {
        package(VerificationEvidence::PublisherVerified {
            publisher_key_id: "talos.release.root".into(),
            signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
        })
    }

    #[test]
    fn admission_binds_exact_package_tenant_profile_and_all_six_permission_terms() {
        let admitted = PluginHost::admit(
            &context("tenant-a"),
            &executable(),
            &verified_package(),
            &installation(PluginIsolationProfile::FirstPartyNative),
            &trusted(),
        )
        .unwrap();
        assert_eq!(admitted.authorize(&permission()), Ok(()));
        assert_eq!(admitted.tenant_id.as_str(), "tenant-a");
        assert_eq!(
            admitted.isolation.profile,
            PluginIsolationProfile::FirstPartyNative
        );

        let mut policy = trusted();
        policy.runtime_policy = PermissionSet::empty();
        let admitted = PluginHost::admit(
            &context("tenant-a"),
            &executable(),
            &verified_package(),
            &installation(PluginIsolationProfile::FirstPartyNative),
            &policy,
        )
        .unwrap();
        assert_eq!(
            admitted.authorize(&permission()),
            Err(PluginHostError::PermissionDenied)
        );
    }

    #[test]
    fn provider_bound_executable_fails_closed_until_host_owned_binding_authority_is_wired() {
        let mut provider_bound = executable();
        provider_bound.provider_instance_id = Some(ProviderInstanceRef::new("provider-a").unwrap());
        provider_bound.binding_revision = Some(BindingRevisionRef::new("binding-rev-1").unwrap());
        assert_eq!(
            PluginHost::admit(
                &context("tenant-a"),
                &provider_bound,
                &verified_package(),
                &installation(PluginIsolationProfile::FirstPartyNative),
                &trusted(),
            ),
            Err(PluginHostError::ProviderBindingAuthorityUnavailable)
        );
    }

    #[test]
    fn incompatible_or_unparseable_host_contract_fails_before_permission_use() {
        let mut policy = trusted();
        policy.host_contract_version = ContractVersion::new("2.0.0").unwrap();
        assert_eq!(
            PluginHost::admit(
                &context("tenant-a"),
                &executable(),
                &verified_package(),
                &installation(PluginIsolationProfile::FirstPartyNative),
                &policy,
            ),
            Err(PluginHostError::HostContractIncompatible)
        );

        let mut policy = trusted();
        policy.host_contract_version = ContractVersion::new("1.0.0-alpha").unwrap();
        assert_eq!(
            PluginHost::admit(
                &context("tenant-a"),
                &executable(),
                &verified_package(),
                &installation(PluginIsolationProfile::FirstPartyNative),
                &policy,
            ),
            Err(PluginHostError::HostContractIncompatible)
        );
    }

    #[test]
    fn cross_tenant_and_package_retargeting_fail_closed() {
        assert_eq!(
            PluginHost::admit(
                &context("tenant-b"),
                &executable(),
                &verified_package(),
                &installation(PluginIsolationProfile::FirstPartyNative),
                &trusted(),
            ),
            Err(PluginHostError::TenantScopeMismatch)
        );

        let mut wrong = executable();
        wrong.package_digest_sha256 = "11".repeat(32);
        assert_eq!(
            PluginHost::admit(
                &context("tenant-a"),
                &wrong,
                &verified_package(),
                &installation(PluginIsolationProfile::FirstPartyNative),
                &trusted(),
            ),
            Err(PluginHostError::PackageIdentityMismatch)
        );

        let mut wrong_contract = executable();
        wrong_contract.capability_contract_version = ContractVersion::new("2.0.0").unwrap();
        assert_eq!(
            PluginHost::admit(
                &context("tenant-a"),
                &wrong_contract,
                &verified_package(),
                &installation(PluginIsolationProfile::FirstPartyNative),
                &trusted(),
            ),
            Err(PluginHostError::PackageIdentityMismatch)
        );
    }

    #[test]
    fn disabled_isolation_and_unverified_native_package_do_not_execute() {
        assert_eq!(
            PluginHost::admit(
                &context("tenant-a"),
                &executable(),
                &verified_package(),
                &installation(PluginIsolationProfile::SingleFileWebApp),
                &trusted(),
            ),
            Err(PluginHostError::PackageNotExecutable)
        );
        assert_eq!(
            PluginHost::admit(
                &context("tenant-a"),
                &executable(),
                &package(VerificationEvidence::Unverified),
                &installation(PluginIsolationProfile::FirstPartyNative),
                &trusted(),
            ),
            Err(PluginHostError::PackageNotExecutable)
        );
    }

    #[test]
    fn staged_or_suspended_installation_fails_before_permission_use() {
        for lifecycle in [
            PluginInstallationLifecycle::Staged,
            PluginInstallationLifecycle::Suspended,
        ] {
            let mut install = installation(PluginIsolationProfile::FirstPartyNative);
            install.lifecycle = lifecycle;
            assert_eq!(
                PluginHost::admit(
                    &context("tenant-a"),
                    &executable(),
                    &verified_package(),
                    &install,
                    &trusted(),
                ),
                Err(PluginHostError::InstallationNotExecutable)
            );
        }
    }
}

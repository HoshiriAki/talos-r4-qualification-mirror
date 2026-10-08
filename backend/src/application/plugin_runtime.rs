//! R4-P6 production plugin-host composition boundary.
//!
//! `PluginHost` resolves package/install/context permission facts. This wrapper
//! adds production provenance and dangerous-combination policy. `FirstPartyNative`
//! is a trusted-build boundary: publisher identity and publisher key must both
//! be trusted by host composition, and individually granted permissions do not
//! implicitly authorize high-risk cross-products.

use std::collections::BTreeSet;

use system_core::ExecutionContext;
use system_core::security::plugin::{
    PermissionSet, PluginIsolationProfile, PluginPackageRecord, PluginPermission, PublisherId,
    VerificationEvidence,
};
use system_core::security::plugin_permission_enforcement::r4_permission_enforcement;
use system_core::transport::interconnect::PluginExecutableRef;

use super::plugin_host::{
    PluginAdmission, PluginHost, PluginHostError, PluginInstallationRecord,
    TrustedPluginRuntimePolicy,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TrustedPublisherKey {
    publisher_id: PublisherId,
    publisher_key_id: String,
}

/// Host-owned first-party publisher/key allowlist. The policy is intentionally
/// not accepted from package, manifest or plugin message data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedNativePublisherPolicy {
    keys: BTreeSet<TrustedPublisherKey>,
}

impl TrustedNativePublisherPolicy {
    pub fn new(
        bindings: impl IntoIterator<Item = (PublisherId, String)>,
    ) -> Result<Self, ProductionPluginHostError> {
        let mut keys = BTreeSet::new();
        for (publisher_id, publisher_key_id) in bindings {
            if !valid_policy_token(&publisher_key_id) {
                return Err(ProductionPluginHostError::InvalidNativePublisherPolicy);
            }
            keys.insert(TrustedPublisherKey {
                publisher_id,
                publisher_key_id,
            });
        }
        Ok(Self { keys })
    }

    pub fn deny_all() -> Self {
        Self::default()
    }

    fn allows_native_package(&self, package: &PluginPackageRecord) -> bool {
        let VerificationEvidence::PublisherVerified {
            publisher_key_id, ..
        } = &package.verification
        else {
            return false;
        };
        self.keys.contains(&TrustedPublisherKey {
            publisher_id: package.identity.publisher_id.clone(),
            publisher_key_id: publisher_key_id.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DangerousExecutableKey {
    plugin_id: String,
    version: String,
    package_digest_sha256: String,
    manifest_digest_sha256: String,
    capability_contract_version: String,
    provider_instance_id: Option<String>,
    binding_revision: Option<String>,
}

impl DangerousExecutableKey {
    fn from_executable(
        executable: &PluginExecutableRef,
    ) -> Result<Self, ProductionPluginHostError> {
        executable
            .validate()
            .map_err(|_| ProductionPluginHostError::InvalidDangerousCombinationPolicy)?;
        Ok(Self {
            plugin_id: executable.plugin_id.as_str().to_owned(),
            version: executable.version.as_str().to_owned(),
            package_digest_sha256: executable.package_digest_sha256.clone(),
            manifest_digest_sha256: executable.manifest_digest_sha256.clone(),
            capability_contract_version: executable.capability_contract_version.as_str().to_owned(),
            provider_instance_id: executable
                .provider_instance_id
                .as_ref()
                .map(|value| value.as_str().to_owned()),
            binding_revision: executable
                .binding_revision
                .as_ref()
                .map(|value| value.as_str().to_owned()),
        })
    }
}

/// Exact host-owned approval record for a dangerous permission cross-product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DangerousPluginCombinationApproval {
    pub installation_id: String,
    pub executable: PluginExecutableRef,
    pub background_job: String,
    pub secret_purpose: String,
    pub network_grant: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DangerousCombinationKey {
    installation_id: String,
    executable: DangerousExecutableKey,
    background_job: String,
    secret_purpose: String,
    network_grant: String,
}

/// Explicit host-owned approval for `background job + secret + network`.
///
/// The architecture security review classifies this triple as a dangerous
/// combination. Passing the six-way permission intersection for each dimension
/// independently is therefore insufficient: every effective cross-product must
/// be explicitly approved for the exact installation and exact executable pin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedDangerousPluginCombinationPolicy {
    approvals: BTreeSet<DangerousCombinationKey>,
}

impl TrustedDangerousPluginCombinationPolicy {
    pub fn new(
        approvals: impl IntoIterator<Item = DangerousPluginCombinationApproval>,
    ) -> Result<Self, ProductionPluginHostError> {
        let mut resolved = BTreeSet::new();
        for approval in approvals {
            if !valid_policy_token(&approval.installation_id)
                || PluginPermission::BackgroundJob(approval.background_job.clone())
                    .validate()
                    .is_err()
                || PluginPermission::SecretPurpose(approval.secret_purpose.clone())
                    .validate()
                    .is_err()
                || PluginPermission::NetworkGrant(approval.network_grant.clone())
                    .validate()
                    .is_err()
            {
                return Err(ProductionPluginHostError::InvalidDangerousCombinationPolicy);
            }
            let executable = DangerousExecutableKey::from_executable(&approval.executable)?;
            resolved.insert(DangerousCombinationKey {
                installation_id: approval.installation_id,
                executable,
                background_job: approval.background_job,
                secret_purpose: approval.secret_purpose,
                network_grant: approval.network_grant,
            });
        }
        Ok(Self {
            approvals: resolved,
        })
    }

    pub fn deny_all() -> Self {
        Self::default()
    }

    fn allows_effective_permissions(&self, admission: &PluginAdmission) -> bool {
        let Ok(executable) = DangerousExecutableKey::from_executable(&admission.executable) else {
            return false;
        };
        let mut jobs = Vec::new();
        let mut secrets = Vec::new();
        let mut networks = Vec::new();
        for permission in admission.permissions.effective.iter() {
            match permission {
                PluginPermission::BackgroundJob(value) => jobs.push(value.as_str()),
                PluginPermission::SecretPurpose(value) => secrets.push(value.as_str()),
                PluginPermission::NetworkGrant(value) => networks.push(value.as_str()),
                _ => {}
            }
        }

        for job in &jobs {
            for secret in &secrets {
                for network in &networks {
                    if !self.approvals.contains(&DangerousCombinationKey {
                        installation_id: admission.installation_id.clone(),
                        executable: executable.clone(),
                        background_job: (*job).to_owned(),
                        secret_purpose: (*secret).to_owned(),
                        network_grant: (*network).to_owned(),
                    }) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionPluginHostError {
    Host(PluginHostError),
    InvalidNativePublisherPolicy,
    InvalidDangerousCombinationPolicy,
    NativePublisherNotTrusted,
    UnsupportedPermissionInProfile,
    DangerousPermissionCombinationDenied,
}

impl From<PluginHostError> for ProductionPluginHostError {
    fn from(value: PluginHostError) -> Self {
        Self::Host(value)
    }
}

/// The production admission entry point for R4 plugin execution.
///
/// Low-level callers may still unit-test `PluginHost`, but runtime composition
/// uses this wrapper so provenance, enforcement availability and dangerous
/// permission combinations cannot be confused with the permission intersection.
pub struct ProductionPluginHost;

impl ProductionPluginHost {
    pub fn admit(
        context: &ExecutionContext,
        executable: &PluginExecutableRef,
        package: &PluginPackageRecord,
        installation: &PluginInstallationRecord,
        trusted_policy: &TrustedPluginRuntimePolicy,
        native_publishers: &TrustedNativePublisherPolicy,
        dangerous_combinations: &TrustedDangerousPluginCombinationPolicy,
    ) -> Result<PluginAdmission, ProductionPluginHostError> {
        let admission =
            PluginHost::admit(context, executable, package, installation, trusted_policy)?;

        if admission.isolation.profile == PluginIsolationProfile::FirstPartyNative
            && !native_publishers.allows_native_package(package)
        {
            return Err(ProductionPluginHostError::NativePublisherNotTrusted);
        }

        if admission.permissions.effective.iter().any(|permission| {
            r4_permission_enforcement(admission.isolation.profile, permission).is_none()
        }) {
            return Err(ProductionPluginHostError::UnsupportedPermissionInProfile);
        }

        if !dangerous_combinations.allows_effective_permissions(&admission) {
            return Err(ProductionPluginHostError::DangerousPermissionCombinationDenied);
        }

        Ok(admission)
    }
}

fn valid_policy_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 192
        && value.is_ascii()
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use system_core::security::plugin::{
        CompatibilityRange, PluginLifecycle, PluginPackageIdentity, Sha256Digest,
    };
    use system_core::transport::interconnect::{ContractVersion, PluginId};
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode,
        NoopHttpClient, RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
    };

    use super::*;
    use crate::application::plugin_host::PluginInstallationLifecycle;

    fn default_permission() -> PluginPermission {
        PluginPermission::CapabilityInvoke("orders.read".into())
    }

    fn set(values: impl IntoIterator<Item = PluginPermission>) -> PermissionSet {
        PermissionSet::new(values).unwrap()
    }

    fn package_with_permissions(
        publisher: &str,
        verification: VerificationEvidence,
        permissions: PermissionSet,
    ) -> PluginPackageRecord {
        PluginPackageRecord {
            identity: PluginPackageIdentity {
                plugin_id: PluginId::new("official.fixture").unwrap(),
                publisher_id: PublisherId::new(publisher).unwrap(),
                version: ContractVersion::new("1.0.0").unwrap(),
                package_digest_sha256: Sha256Digest::new("ab".repeat(32)).unwrap(),
                manifest_digest_sha256: Sha256Digest::new("ef".repeat(32)).unwrap(),
                capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
            },
            compatibility_range: CompatibilityRange::new(">=1.0.0,<2.0.0").unwrap(),
            declared_capabilities: BTreeSet::from(["orders.read".into()]),
            permission_request: permissions,
            verification,
            lifecycle: PluginLifecycle::Active,
        }
    }

    fn package_with_permission(
        publisher: &str,
        verification: VerificationEvidence,
        permission: PluginPermission,
    ) -> PluginPackageRecord {
        package_with_permissions(publisher, verification, set([permission]))
    }

    fn package(publisher: &str, verification: VerificationEvidence) -> PluginPackageRecord {
        package_with_permission(publisher, verification, default_permission())
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

    fn installation_with_permissions(
        package: &PluginPackageRecord,
        permissions: PermissionSet,
    ) -> PluginInstallationRecord {
        PluginInstallationRecord {
            installation_id: "install-a".into(),
            tenant_id: TenantId::new("tenant-a").unwrap(),
            package_identity: package.identity.clone(),
            isolation_profile: PluginIsolationProfile::FirstPartyNative,
            installation_grant: permissions.clone(),
            tenant_policy: permissions,
            grant_revision: 1,
            tenant_policy_revision: 1,
            lifecycle: PluginInstallationLifecycle::Active,
        }
    }

    fn installation_with_permission(
        package: &PluginPackageRecord,
        permission: PluginPermission,
    ) -> PluginInstallationRecord {
        installation_with_permissions(package, set([permission]))
    }

    fn installation(package: &PluginPackageRecord) -> PluginInstallationRecord {
        installation_with_permission(package, default_permission())
    }

    fn context() -> ExecutionContext {
        let tenant = TenantId::new("tenant-a").unwrap();
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

    fn trusted_runtime_with_permissions(permissions: PermissionSet) -> TrustedPluginRuntimePolicy {
        TrustedPluginRuntimePolicy {
            host_contract_version: ContractVersion::new("1.0.0").unwrap(),
            principal_authority: permissions.clone(),
            tenant_business_plane: permissions.clone(),
            platform_control_plane: PermissionSet::empty(),
            simulation_plane: PermissionSet::empty(),
            runtime_policy: permissions,
        }
    }

    fn trusted_runtime_with_permission(permission: PluginPermission) -> TrustedPluginRuntimePolicy {
        trusted_runtime_with_permissions(set([permission]))
    }

    fn trusted_runtime() -> TrustedPluginRuntimePolicy {
        trusted_runtime_with_permission(default_permission())
    }

    fn native_publishers() -> TrustedNativePublisherPolicy {
        TrustedNativePublisherPolicy::new([(
            PublisherId::new("talos.official").unwrap(),
            "talos.release.root".into(),
        )])
        .unwrap()
    }

    fn no_dangerous_combinations() -> TrustedDangerousPluginCombinationPolicy {
        TrustedDangerousPluginCombinationPolicy::deny_all()
    }

    fn publisher_verified() -> VerificationEvidence {
        VerificationEvidence::PublisherVerified {
            publisher_key_id: "talos.release.root".into(),
            signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
        }
    }

    #[test]
    fn approved_publisher_and_key_can_cross_first_party_native_trust_boundary() {
        let package = package("talos.official", publisher_verified());
        let admitted = ProductionPluginHost::admit(
            &context(),
            &executable(),
            &package,
            &installation(&package),
            &trusted_runtime(),
            &native_publishers(),
            &no_dangerous_combinations(),
        )
        .unwrap();
        assert_eq!(admitted.authorize(&default_permission()), Ok(()));
        assert_eq!(admitted.tenant_id.as_str(), "tenant-a");
    }

    #[test]
    fn attestation_is_not_publisher_verification_for_native_execution() {
        let package = package(
            "talos.official",
            VerificationEvidence::Attested {
                attestation_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
            },
        );
        assert_eq!(
            ProductionPluginHost::admit(
                &context(),
                &executable(),
                &package,
                &installation(&package),
                &trusted_runtime(),
                &native_publishers(),
                &no_dangerous_combinations(),
            ),
            Err(ProductionPluginHostError::Host(
                PluginHostError::PackageNotExecutable
            ))
        );
    }

    #[test]
    fn incompatible_host_contract_is_not_overridden_by_native_trust() {
        let package = package("talos.official", publisher_verified());
        let mut runtime = trusted_runtime();
        runtime.host_contract_version = ContractVersion::new("2.0.0").unwrap();
        assert_eq!(
            ProductionPluginHost::admit(
                &context(),
                &executable(),
                &package,
                &installation(&package),
                &runtime,
                &native_publishers(),
                &no_dangerous_combinations(),
            ),
            Err(ProductionPluginHostError::Host(
                PluginHostError::HostContractIncompatible
            ))
        );
    }

    #[test]
    fn verified_but_unapproved_publisher_or_key_cannot_become_native() {
        let unapproved_publisher = package(
            "vendor.example",
            VerificationEvidence::PublisherVerified {
                publisher_key_id: "vendor.release".into(),
                signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
            },
        );
        assert_eq!(
            ProductionPluginHost::admit(
                &context(),
                &executable(),
                &unapproved_publisher,
                &installation(&unapproved_publisher),
                &trusted_runtime(),
                &native_publishers(),
                &no_dangerous_combinations(),
            ),
            Err(ProductionPluginHostError::NativePublisherNotTrusted)
        );

        let unapproved_key = package(
            "talos.official",
            VerificationEvidence::PublisherVerified {
                publisher_key_id: "talos.untrusted.key".into(),
                signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
            },
        );
        assert_eq!(
            ProductionPluginHost::admit(
                &context(),
                &executable(),
                &unapproved_key,
                &installation(&unapproved_key),
                &trusted_runtime(),
                &native_publishers(),
                &no_dangerous_combinations(),
            ),
            Err(ProductionPluginHostError::NativePublisherNotTrusted)
        );
    }

    #[test]
    fn ui_and_unscoped_core_write_permissions_remain_disabled_even_if_all_six_terms_grant_them() {
        for permission in [
            PluginPermission::UiExtension("order-panel".into()),
            PluginPermission::DataWriteProjection("core:orders".into()),
        ] {
            let package =
                package_with_permission("talos.official", publisher_verified(), permission.clone());
            let installation = installation_with_permission(&package, permission.clone());
            assert_eq!(
                ProductionPluginHost::admit(
                    &context(),
                    &executable(),
                    &package,
                    &installation,
                    &trusted_runtime_with_permission(permission),
                    &native_publishers(),
                    &no_dangerous_combinations(),
                ),
                Err(ProductionPluginHostError::UnsupportedPermissionInProfile)
            );
        }
    }

    #[test]
    fn background_secret_network_requires_exact_executable_approval() {
        let permissions = set([
            PluginPermission::BackgroundJob("provider.reconcile".into()),
            PluginPermission::SecretPurpose("provider.sign".into()),
            PluginPermission::NetworkGrant("provider.api".into()),
        ]);
        let package =
            package_with_permissions("talos.official", publisher_verified(), permissions.clone());
        let installation = installation_with_permissions(&package, permissions.clone());
        let runtime = trusted_runtime_with_permissions(permissions);
        let exact_executable = executable();

        assert_eq!(
            ProductionPluginHost::admit(
                &context(),
                &exact_executable,
                &package,
                &installation,
                &runtime,
                &native_publishers(),
                &TrustedDangerousPluginCombinationPolicy::deny_all(),
            ),
            Err(ProductionPluginHostError::DangerousPermissionCombinationDenied)
        );

        let approved =
            TrustedDangerousPluginCombinationPolicy::new([DangerousPluginCombinationApproval {
                installation_id: "install-a".into(),
                executable: exact_executable.clone(),
                background_job: "provider.reconcile".into(),
                secret_purpose: "provider.sign".into(),
                network_grant: "provider.api".into(),
            }])
            .unwrap();
        assert!(
            ProductionPluginHost::admit(
                &context(),
                &exact_executable,
                &package,
                &installation,
                &runtime,
                &native_publishers(),
                &approved,
            )
            .is_ok()
        );

        let mut wrong_executable = exact_executable.clone();
        wrong_executable.package_digest_sha256 = "11".repeat(32);
        let wrong_approval =
            TrustedDangerousPluginCombinationPolicy::new([DangerousPluginCombinationApproval {
                installation_id: "install-a".into(),
                executable: wrong_executable,
                background_job: "provider.reconcile".into(),
                secret_purpose: "provider.sign".into(),
                network_grant: "provider.api".into(),
            }])
            .unwrap();
        assert_eq!(
            ProductionPluginHost::admit(
                &context(),
                &exact_executable,
                &package,
                &installation,
                &runtime,
                &native_publishers(),
                &wrong_approval,
            ),
            Err(ProductionPluginHostError::DangerousPermissionCombinationDenied)
        );
    }
}

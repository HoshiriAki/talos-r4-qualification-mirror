use std::collections::BTreeSet;
use std::sync::Arc;

use system_core::security::plugin::{
    CompatibilityRange, PermissionSet, PluginIsolationProfile, PluginLifecycle,
    PluginPackageIdentity, PluginPackageRecord, PluginPermission, PublisherId, Sha256Digest,
    VerificationEvidence,
};
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use super::plugin_host::{
    PluginHostError, PluginInstallationLifecycle, PluginInstallationRecord,
    TrustedPluginRuntimePolicy,
};
use super::plugin_runtime::{
    DangerousPluginCombinationApproval, ProductionPluginHost, ProductionPluginHostError,
    TrustedDangerousPluginCombinationPolicy, TrustedNativePublisherPolicy,
};

fn set(values: impl IntoIterator<Item = PluginPermission>) -> PermissionSet {
    PermissionSet::new(values).unwrap()
}

fn executable(plugin_id: &str, digest_seed: &str) -> PluginExecutableRef {
    PluginExecutableRef {
        plugin_id: PluginId::new(plugin_id).unwrap(),
        version: ContractVersion::new("1.0.0").unwrap(),
        package_digest_sha256: digest_seed.repeat(32),
        manifest_digest_sha256: "ef".repeat(32),
        capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
        provider_instance_id: None,
        binding_revision: None,
    }
}

fn package(
    plugin_id: &str,
    digest_seed: &str,
    permissions: PermissionSet,
    publisher_key_id: &str,
) -> PluginPackageRecord {
    PluginPackageRecord {
        identity: PluginPackageIdentity {
            plugin_id: PluginId::new(plugin_id).unwrap(),
            publisher_id: PublisherId::new("talos.official").unwrap(),
            version: ContractVersion::new("1.0.0").unwrap(),
            package_digest_sha256: Sha256Digest::new(digest_seed.repeat(32)).unwrap(),
            manifest_digest_sha256: Sha256Digest::new("ef".repeat(32)).unwrap(),
            capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
        },
        compatibility_range: CompatibilityRange::new(">=1.0.0,<2.0.0").unwrap(),
        declared_capabilities: BTreeSet::from([
            "provider.execute".into(),
            "provider.reconcile".into(),
        ]),
        permission_request: permissions,
        verification: VerificationEvidence::PublisherVerified {
            publisher_key_id: publisher_key_id.into(),
            signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
        },
        lifecycle: PluginLifecycle::Active,
    }
}

fn installation(
    tenant: &str,
    installation_id: &str,
    package: &PluginPackageRecord,
    permissions: PermissionSet,
) -> PluginInstallationRecord {
    PluginInstallationRecord {
        installation_id: installation_id.into(),
        tenant_id: TenantId::new(tenant).unwrap(),
        package_identity: package.identity.clone(),
        isolation_profile: PluginIsolationProfile::FirstPartyNative,
        installation_grant: permissions.clone(),
        tenant_policy: permissions,
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

fn runtime(permissions: PermissionSet) -> TrustedPluginRuntimePolicy {
    TrustedPluginRuntimePolicy {
        host_contract_version: ContractVersion::new("1.0.0").unwrap(),
        principal_authority: permissions.clone(),
        tenant_business_plane: permissions.clone(),
        platform_control_plane: PermissionSet::empty(),
        simulation_plane: PermissionSet::empty(),
        runtime_policy: permissions,
    }
}

fn official_publishers() -> TrustedNativePublisherPolicy {
    TrustedNativePublisherPolicy::new([(
        PublisherId::new("talos.official").unwrap(),
        "talos.release.root".into(),
    )])
    .unwrap()
}

#[test]
fn official_sf_fixture_obeys_exact_dangerous_combination_policy_like_any_plugin() {
    let permissions = set([
        PluginPermission::BackgroundJob("sf.reconcile".into()),
        PluginPermission::NetworkGrant("sf.api".into()),
        PluginPermission::SecretPurpose("sf.sign".into()),
    ]);
    let package = package(
        "official.sf-express.fixture",
        "ab",
        permissions.clone(),
        "talos.release.root",
    );
    let admitted_executable = executable("official.sf-express.fixture", "ab");
    let installation = installation("tenant-a", "install-sf", &package, permissions.clone());
    let runtime = runtime(permissions);

    assert_eq!(
        ProductionPluginHost::admit(
            &context("tenant-a"),
            &admitted_executable,
            &package,
            &installation,
            &runtime,
            &official_publishers(),
            &TrustedDangerousPluginCombinationPolicy::deny_all(),
        ),
        Err(ProductionPluginHostError::DangerousPermissionCombinationDenied)
    );

    let approved =
        TrustedDangerousPluginCombinationPolicy::new([DangerousPluginCombinationApproval {
            installation_id: "install-sf".into(),
            executable: admitted_executable.clone(),
            background_job: "sf.reconcile".into(),
            secret_purpose: "sf.sign".into(),
            network_grant: "sf.api".into(),
        }])
        .unwrap();
    assert!(
        ProductionPluginHost::admit(
            &context("tenant-a"),
            &admitted_executable,
            &package,
            &installation,
            &runtime,
            &official_publishers(),
            &approved,
        )
        .is_ok()
    );

    let wrong_executable = executable("official.sf-express.fixture", "12");
    let stale_approval =
        TrustedDangerousPluginCombinationPolicy::new([DangerousPluginCombinationApproval {
            installation_id: "install-sf".into(),
            executable: wrong_executable,
            background_job: "sf.reconcile".into(),
            secret_purpose: "sf.sign".into(),
            network_grant: "sf.api".into(),
        }])
        .unwrap();
    assert_eq!(
        ProductionPluginHost::admit(
            &context("tenant-a"),
            &admitted_executable,
            &package,
            &installation,
            &runtime,
            &official_publishers(),
            &stale_approval,
        ),
        Err(ProductionPluginHostError::DangerousPermissionCombinationDenied)
    );
}

#[test]
fn official_wechat_fixture_does_not_bypass_tenant_policy_or_principal_authority() {
    let requested = set([
        PluginPermission::CapabilityInvoke("payment.capture".into()),
        PluginPermission::NetworkGrant("wechat-pay.api".into()),
        PluginPermission::SecretPurpose("wechat-pay.sign".into()),
    ]);
    let package = package(
        "official.wechat-pay.fixture",
        "12",
        requested.clone(),
        "talos.release.root",
    );
    let executable = executable("official.wechat-pay.fixture", "12");
    let mut installation = installation("tenant-a", "install-wx", &package, requested.clone());
    installation.tenant_policy = set([
        PluginPermission::CapabilityInvoke("payment.capture".into()),
        PluginPermission::SecretPurpose("wechat-pay.sign".into()),
    ]);

    let admission = ProductionPluginHost::admit(
        &context("tenant-a"),
        &executable,
        &package,
        &installation,
        &runtime(requested),
        &official_publishers(),
        &TrustedDangerousPluginCombinationPolicy::deny_all(),
    )
    .unwrap();
    assert_eq!(
        admission.authorize(&PluginPermission::NetworkGrant("wechat-pay.api".into())),
        Err(PluginHostError::PermissionDenied)
    );
}

#[test]
fn official_name_and_publisher_id_do_not_bypass_untrusted_publisher_key() {
    let permissions = set([PluginPermission::CapabilityInvoke("payment.create".into())]);
    let package = package(
        "official.wechat-pay.fixture",
        "34",
        permissions.clone(),
        "attacker.key",
    );
    let executable = executable("official.wechat-pay.fixture", "34");
    let installation = installation("tenant-a", "install-wx", &package, permissions.clone());

    assert_eq!(
        ProductionPluginHost::admit(
            &context("tenant-a"),
            &executable,
            &package,
            &installation,
            &runtime(permissions),
            &official_publishers(),
            &TrustedDangerousPluginCombinationPolicy::deny_all(),
        ),
        Err(ProductionPluginHostError::NativePublisherNotTrusted)
    );
}

#[test]
fn official_fixture_cannot_cross_tenant_scope() {
    let permissions = set([PluginPermission::CapabilityInvoke("payment.create".into())]);
    let package = package(
        "official.wechat-pay.fixture",
        "56",
        permissions.clone(),
        "talos.release.root",
    );
    let executable = executable("official.wechat-pay.fixture", "56");
    let installation = installation("tenant-a", "install-wx", &package, permissions.clone());

    assert_eq!(
        ProductionPluginHost::admit(
            &context("tenant-b"),
            &executable,
            &package,
            &installation,
            &runtime(permissions),
            &official_publishers(),
            &TrustedDangerousPluginCombinationPolicy::deny_all(),
        ),
        Err(ProductionPluginHostError::Host(
            PluginHostError::TenantScopeMismatch
        ))
    );
}

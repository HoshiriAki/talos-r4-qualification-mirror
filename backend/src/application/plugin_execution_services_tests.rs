use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use system_core::security::plugin::{
    EffectivePluginPermissions, IsolationEnforcement, IsolationPolicy, PermissionSet,
    PluginIsolationProfile, PluginPermission,
};
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use super::plugin_egress::{
    PluginEgressBudgetRegistry, PluginEgressError, PluginEgressGrantPolicy,
    PluginSecretNetworkPolicy,
};
use super::plugin_execution_admission::{PluginExecutionAdmission, PluginExecutionRuntimeBinding};
use super::plugin_execution_services::{PluginExecutionEgressError, PluginExecutionEgressService};
use super::plugin_host::PluginAdmission;
use crate::integration::egress::{DestinationPolicy, NoopEgressEvidence};
use crate::integration::transport::{ExternalRequest, TransportTimeouts};

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
        DataScope::production(tenant, Revision::new("rev-a").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new("request-a").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
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

fn admission(
    context: &ExecutionContext,
    permissions: PermissionSet,
    runtime_binding: PluginExecutionRuntimeBinding,
) -> PluginExecutionAdmission {
    PluginExecutionAdmission::new(
        PluginAdmission {
            installation_id: "install-a".into(),
            tenant_id: TenantId::new("tenant-a").unwrap(),
            executable: executable(),
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
                effective: permissions,
                denied_requested: PermissionSet::empty(),
            },
        },
        context,
        runtime_binding,
    )
}

fn service(runtime_binding: PluginExecutionRuntimeBinding) -> PluginExecutionEgressService {
    PluginExecutionEgressService::new(
        Arc::new(PluginEgressBudgetRegistry::new()),
        Arc::new(PluginEgressGrantPolicy::deny_all()),
        Arc::new(PluginSecretNetworkPolicy::deny_all()),
        DestinationPolicy::public_only(),
        Arc::new(NoopEgressEvidence),
        runtime_binding,
    )
}

fn request() -> ExternalRequest {
    ExternalRequest {
        method: "POST".into(),
        url: "https://api.example.test/v1/test".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
        timeouts: TransportTimeouts {
            connect: Duration::from_millis(50),
            read: Duration::from_millis(50),
            overall: Duration::from_millis(100),
        },
        correlation_id: "corr-a".into(),
        cancellation: None,
    }
}

#[tokio::test]
async fn secret_capable_admission_cannot_relabel_egress_as_secretless() {
    let ctx = context();
    let runtime_binding = PluginExecutionRuntimeBinding::new();
    let admission = admission(
        &ctx,
        PermissionSet::new([
            PluginPermission::NetworkGrant("provider.fixture.api".into()),
            PluginPermission::SecretPurpose("fixture.sign".into()),
        ])
        .unwrap(),
        runtime_binding.clone(),
    );

    let result = service(runtime_binding)
        .send(&ctx, &admission, "provider.fixture.api", None, request())
        .await;
    assert!(matches!(
        result,
        Err(PluginExecutionEgressError::SecretPurposeRequired)
    ));
}

#[tokio::test]
async fn multiple_secret_purposes_fail_closed_before_network_dispatch() {
    let ctx = context();
    let runtime_binding = PluginExecutionRuntimeBinding::new();
    let admission = admission(
        &ctx,
        PermissionSet::new([
            PluginPermission::NetworkGrant("provider.fixture.api".into()),
            PluginPermission::SecretPurpose("fixture.sign.primary".into()),
            PluginPermission::SecretPurpose("fixture.sign.secondary".into()),
        ])
        .unwrap(),
        runtime_binding.clone(),
    );

    let result = service(runtime_binding)
        .send(
            &ctx,
            &admission,
            "provider.fixture.api",
            Some("fixture.sign.primary"),
            request(),
        )
        .await;
    assert!(matches!(
        result,
        Err(PluginExecutionEgressError::AmbiguousSecretPurpose)
    ));
}

#[tokio::test]
async fn cross_runtime_admission_is_rejected_before_egress_policy_resolution() {
    let ctx = context();
    let admission_binding = PluginExecutionRuntimeBinding::new();
    let admission = admission(
        &ctx,
        PermissionSet::new([PluginPermission::NetworkGrant(
            "provider.fixture.api".into(),
        )])
        .unwrap(),
        admission_binding,
    );

    let result = service(PluginExecutionRuntimeBinding::new())
        .send(&ctx, &admission, "provider.fixture.api", None, request())
        .await;
    assert!(matches!(
        result,
        Err(PluginExecutionEgressError::Admission(
            super::plugin_execution_admission::PluginExecutionAdmissionError::RuntimeMismatch
        ))
    ));
}

#[tokio::test]
async fn secretless_admission_continues_to_normal_egress_authority_resolution() {
    let ctx = context();
    let runtime_binding = PluginExecutionRuntimeBinding::new();
    let admission = admission(
        &ctx,
        PermissionSet::new([PluginPermission::NetworkGrant(
            "provider.fixture.api".into(),
        )])
        .unwrap(),
        runtime_binding.clone(),
    );

    let result = service(runtime_binding)
        .send(&ctx, &admission, "provider.fixture.api", None, request())
        .await;
    assert!(matches!(
        result,
        Err(PluginExecutionEgressError::Egress(
            PluginEgressError::UnknownNetworkGrant
        ))
    ));
}

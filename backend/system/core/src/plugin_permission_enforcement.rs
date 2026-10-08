//! R4-P6 enforcement mapping from semantic plugin permission to the only
//! permitted host mechanism. A permission token is never itself an executor.

use serde::{Deserialize, Serialize};

use super::plugin::{PluginIsolationProfile, PluginPermission};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermissionEnforcement {
    RegistryCapabilityInvoke,
    GovernedP5Egress,
    PurposeBoundSecretOperation,
    P3EventLane,
    P3WorkLane,
    TenantPluginScopedStorage,
}

/// Resolve a permission into its mandatory R4 enforcement mechanism.
///
/// `None` means the permission is not executable in the selected profile. This
/// is deliberately stronger than merely intersecting six permission sets: a
/// manifest/grant/policy combination cannot enable a capability for which R4
/// lacks a reviewed enforcement mechanism.
pub fn r4_permission_enforcement(
    profile: PluginIsolationProfile,
    permission: &PluginPermission,
) -> Option<PluginPermissionEnforcement> {
    if profile != PluginIsolationProfile::FirstPartyNative {
        return None;
    }

    match permission {
        PluginPermission::CapabilityInvoke(_) => {
            Some(PluginPermissionEnforcement::RegistryCapabilityInvoke)
        }
        // Declaring a capability in an immutable package is not equivalent to
        // safely registering/providing it at runtime. R4 has no separate
        // plugin-provider registration authority, so this remains fail-closed.
        PluginPermission::CapabilityProvide(_) => None,
        // No generic governed read-projection adapter is frozen yet. In
        // particular, the future SingleFileWebApp mediated-data direction is a
        // disabled isolation profile in R4 and must not backdoor native DB/read
        // authority through a token.
        PluginPermission::DataReadProjection(_) => None,
        // Plugin-owned state has its own PluginStorage dimension and adapter.
        // Import-staging/business projection writes remain disabled until a
        // separately reviewed bounded adapter exists.
        PluginPermission::DataWriteProjection(_) => None,
        PluginPermission::NetworkGrant(_) => Some(PluginPermissionEnforcement::GovernedP5Egress),
        PluginPermission::SecretPurpose(_) => {
            Some(PluginPermissionEnforcement::PurposeBoundSecretOperation)
        }
        PluginPermission::EventEmit(_) | PluginPermission::EventSubscribe(_) => {
            Some(PluginPermissionEnforcement::P3EventLane)
        }
        // R2 ExternalOperation currently pins provider binding/config revision
        // but not the exact P3 PluginExecutableRef. Enabling this permission
        // would allow a durable plugin-backed effect to outlive/rebind its
        // executable identity, violating the R4 security review. Keep the
        // semantic dimension stable but execution fail-closed until the durable
        // effect authority persists the exact executable pin.
        PluginPermission::ExternalEffect(_) => None,
        PluginPermission::BackgroundJob(_) => Some(PluginPermissionEnforcement::P3WorkLane),
        PluginPermission::PluginStorage(_) => {
            Some(PluginPermissionEnforcement::TenantPluginScopedStorage)
        }
        // R4 has no declarative/isolated-origin plugin UI runtime yet. A token in
        // a manifest must not turn into Core-origin DOM/session authority.
        PluginPermission::UiExtension(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::transport::interconnect::Subject;

    use super::*;

    #[test]
    fn only_permissions_with_real_r4_enforcement_are_enabled() {
        assert_eq!(
            r4_permission_enforcement(
                PluginIsolationProfile::FirstPartyNative,
                &PluginPermission::CapabilityInvoke("orders.read".into())
            ),
            Some(PluginPermissionEnforcement::RegistryCapabilityInvoke)
        );
        assert_eq!(
            r4_permission_enforcement(
                PluginIsolationProfile::FirstPartyNative,
                &PluginPermission::EventEmit(Subject::new("orders.changed").unwrap())
            ),
            Some(PluginPermissionEnforcement::P3EventLane)
        );
    }

    #[test]
    fn unsupported_semantic_dimensions_stay_disabled_without_reviewed_adapters() {
        for permission in [
            PluginPermission::CapabilityProvide("payment.create".into()),
            PluginPermission::DataReadProjection("orders".into()),
            PluginPermission::DataWriteProjection("plugin_storage:cache".into()),
            PluginPermission::DataWriteProjection("import_staging:orders".into()),
            PluginPermission::ExternalEffect("payment.capture".into()),
            PluginPermission::UiExtension("order-panel".into()),
        ] {
            assert_eq!(
                r4_permission_enforcement(PluginIsolationProfile::FirstPartyNative, &permission),
                None,
                "unexpected R4 execution authority for {permission:?}"
            );
        }
    }

    #[test]
    fn disabled_profiles_cannot_gain_authority_from_permission_tokens() {
        for profile in [
            PluginIsolationProfile::VerifiedSandboxed,
            PluginIsolationProfile::LocalUnverified,
            PluginIsolationProfile::SingleFileWebApp,
            PluginIsolationProfile::RemoteWorker,
        ] {
            assert_eq!(
                r4_permission_enforcement(
                    profile,
                    &PluginPermission::NetworkGrant("fixture.egress".into())
                ),
                None
            );
        }
    }
}

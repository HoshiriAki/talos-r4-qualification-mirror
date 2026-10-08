//! R4-P6 plugin isolation enforcement matrix.
//!
//! Permission policy and technical isolation are deliberately separate. In
//! particular, `FirstPartyNative` runs in the trusted process and therefore
//! cannot truthfully claim syscall/process isolation merely because callers are
//! expected to use capability-shaped host APIs.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::plugin::PluginIsolationProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmbientAuthorityClass {
    FilesystemSyscall,
    NetworkSyscall,
    ProcessMemory,
    CpuMemoryResourceUse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationMechanism {
    /// Trusted code in the TALOS process. Ambient authority is controlled by
    /// code review/dependency/composition rules, not by an OS sandbox.
    TrustedBuildBoundary,
    /// A real sandbox must technically enforce the boundary before enablement.
    SandboxRequired,
    /// An out-of-process/remote execution boundary and protocol enforcement are
    /// required before enablement.
    RemoteBoundaryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginIsolationEnforcementMatrix {
    pub profile: PluginIsolationProfile,
    pub enabled_in_r4: bool,
    pub mechanism: IsolationMechanism,
    /// Ambient authorities that are trusted rather than technically prevented
    /// by the current execution mechanism.
    pub trusted_not_technically_prevented: BTreeSet<AmbientAuthorityClass>,
    /// Even for trusted native code, normal TALOS plugin operation must use the
    /// governed P5 network path rather than treating process network access as
    /// a capability grant.
    pub governed_network_api_required: bool,
    /// Secret access is purpose-bound through a host-owned API/handle boundary.
    pub purpose_bound_secret_api_required: bool,
    /// Core business data remains mediated through scoped projections/commands;
    /// direct Core database access is never a plugin permission.
    pub governed_data_api_required: bool,
}

pub fn r4_plugin_isolation_enforcement(
    profile: PluginIsolationProfile,
) -> PluginIsolationEnforcementMatrix {
    match profile {
        PluginIsolationProfile::FirstPartyNative => PluginIsolationEnforcementMatrix {
            profile,
            enabled_in_r4: true,
            mechanism: IsolationMechanism::TrustedBuildBoundary,
            trusted_not_technically_prevented: BTreeSet::from([
                AmbientAuthorityClass::FilesystemSyscall,
                AmbientAuthorityClass::NetworkSyscall,
                AmbientAuthorityClass::ProcessMemory,
                AmbientAuthorityClass::CpuMemoryResourceUse,
            ]),
            governed_network_api_required: true,
            purpose_bound_secret_api_required: true,
            governed_data_api_required: true,
        },
        PluginIsolationProfile::VerifiedSandboxed
        | PluginIsolationProfile::LocalUnverified
        | PluginIsolationProfile::SingleFileWebApp => PluginIsolationEnforcementMatrix {
            profile,
            enabled_in_r4: false,
            mechanism: IsolationMechanism::SandboxRequired,
            trusted_not_technically_prevented: BTreeSet::new(),
            governed_network_api_required: true,
            purpose_bound_secret_api_required: true,
            governed_data_api_required: true,
        },
        PluginIsolationProfile::RemoteWorker => PluginIsolationEnforcementMatrix {
            profile,
            enabled_in_r4: false,
            mechanism: IsolationMechanism::RemoteBoundaryRequired,
            trusted_not_technically_prevented: BTreeSet::new(),
            governed_network_api_required: true,
            purpose_bound_secret_api_required: true,
            governed_data_api_required: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_party_native_never_claims_syscall_or_process_isolation() {
        let matrix = r4_plugin_isolation_enforcement(PluginIsolationProfile::FirstPartyNative);
        assert!(matrix.enabled_in_r4);
        assert_eq!(matrix.mechanism, IsolationMechanism::TrustedBuildBoundary);
        for authority in [
            AmbientAuthorityClass::FilesystemSyscall,
            AmbientAuthorityClass::NetworkSyscall,
            AmbientAuthorityClass::ProcessMemory,
            AmbientAuthorityClass::CpuMemoryResourceUse,
        ] {
            assert!(
                matrix
                    .trusted_not_technically_prevented
                    .contains(&authority)
            );
        }
        assert!(matrix.governed_network_api_required);
        assert!(matrix.purpose_bound_secret_api_required);
        assert!(matrix.governed_data_api_required);
    }

    #[test]
    fn untrusted_and_remote_profiles_stay_disabled_until_real_boundaries_exist() {
        for profile in [
            PluginIsolationProfile::VerifiedSandboxed,
            PluginIsolationProfile::LocalUnverified,
            PluginIsolationProfile::SingleFileWebApp,
            PluginIsolationProfile::RemoteWorker,
        ] {
            let matrix = r4_plugin_isolation_enforcement(profile);
            assert!(
                !matrix.enabled_in_r4,
                "{profile:?} must remain disabled in R4"
            );
            assert!(matrix.trusted_not_technically_prevented.is_empty());
            assert_ne!(matrix.mechanism, IsolationMechanism::TrustedBuildBoundary);
        }
    }
}

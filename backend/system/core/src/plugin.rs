//! R4-P6 plugin package, permission and isolation stable-core contracts.
//!
//! Core owns immutable package identity and capability semantics only. Package
//! storage, signature implementation, sandbox process technology and concrete
//! network/storage adapters remain backend/Profile concerns.

use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize};

use crate::ExecutionPlane;
use crate::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId, Subject};

const MAX_PLUGIN_TOKEN_BYTES: usize = 192;
const MAX_DECLARED_CAPABILITIES: usize = 256;
const MAX_PERMISSION_COUNT: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PublisherId(String);

impl PublisherId {
    pub fn new(value: impl Into<String>) -> Result<Self, PluginContractError> {
        let value = value.into();
        validate_token(&value, "publisher id")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PublisherId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

impl Sha256Digest {
    pub fn new(value: impl Into<String>) -> Result<Self, PluginContractError> {
        let value = value.into();
        if value.len() != 64
            || value
                .bytes()
                .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
        {
            return Err(PluginContractError::InvalidDigest);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct CompatibilityRange(String);

impl CompatibilityRange {
    pub fn new(value: impl Into<String>) -> Result<Self, PluginContractError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.is_ascii()
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err(PluginContractError::InvalidCompatibilityRange);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CompatibilityRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginLifecycle {
    Staged,
    Active,
    Suspended,
    Revoked { reason_code: String },
}

impl PluginLifecycle {
    pub fn is_executable(&self) -> bool {
        matches!(self, Self::Active)
    }

    pub fn validate(&self) -> Result<(), PluginContractError> {
        if let Self::Revoked { reason_code } = self {
            validate_token(reason_code, "revocation reason")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerificationEvidence {
    Unverified,
    DigestVerified,
    PublisherVerified {
        publisher_key_id: String,
        signature_digest_sha256: Sha256Digest,
    },
    Attested {
        attestation_digest_sha256: Sha256Digest,
    },
}

impl VerificationEvidence {
    pub fn validate(&self) -> Result<(), PluginContractError> {
        if let Self::PublisherVerified {
            publisher_key_id, ..
        } = self
        {
            validate_token(publisher_key_id, "publisher key id")?;
        }
        Ok(())
    }

    /// Publisher verification means a publisher-scoped signature identity was
    /// actually verified. Generic attestation remains valuable supply-chain
    /// evidence but does not prove publisher identity and cannot unlock a
    /// trusted native execution profile.
    pub fn publisher_verified(&self) -> bool {
        matches!(self, Self::PublisherVerified { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "dimension", content = "value", rename_all = "snake_case")]
pub enum PluginPermission {
    CapabilityInvoke(String),
    CapabilityProvide(String),
    DataReadProjection(String),
    DataWriteProjection(String),
    NetworkGrant(String),
    SecretPurpose(String),
    EventEmit(Subject),
    EventSubscribe(Subject),
    ExternalEffect(String),
    BackgroundJob(String),
    PluginStorage(String),
    UiExtension(String),
}

impl PluginPermission {
    pub fn validate(&self) -> Result<(), PluginContractError> {
        match self {
            Self::EventEmit(subject) | Self::EventSubscribe(subject) => {
                if subject.is_reserved() {
                    return Err(PluginContractError::ReservedEventSubject);
                }
                Ok(())
            }
            Self::BackgroundJob(value) => {
                validate_token(value, "plugin permission")?;
                let subject = Subject::new(value).map_err(|_| PluginContractError::InvalidToken)?;
                if subject.as_str() != value {
                    return Err(PluginContractError::InvalidToken);
                }
                if subject.is_reserved() {
                    return Err(PluginContractError::ReservedBackgroundJobSubject);
                }
                Ok(())
            }
            Self::CapabilityInvoke(value)
            | Self::CapabilityProvide(value)
            | Self::DataReadProjection(value)
            | Self::DataWriteProjection(value)
            | Self::NetworkGrant(value)
            | Self::SecretPurpose(value)
            | Self::ExternalEffect(value)
            | Self::PluginStorage(value)
            | Self::UiExtension(value) => validate_token(value, "plugin permission"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(transparent)]
pub struct PermissionSet(BTreeSet<PluginPermission>);

impl PermissionSet {
    pub fn new(
        values: impl IntoIterator<Item = PluginPermission>,
    ) -> Result<Self, PluginContractError> {
        let values: BTreeSet<_> = values.into_iter().collect();
        if values.len() > MAX_PERMISSION_COUNT {
            return Err(PluginContractError::TooManyPermissions);
        }
        for value in &values {
            value.validate()?;
        }
        Ok(Self(values))
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn contains(&self, permission: &PluginPermission) -> bool {
        self.0.contains(permission)
    }

    pub fn iter(&self) -> impl Iterator<Item = &PluginPermission> {
        self.0.iter()
    }

    pub fn validate(&self) -> Result<(), PluginContractError> {
        if self.0.len() > MAX_PERMISSION_COUNT {
            return Err(PluginContractError::TooManyPermissions);
        }
        for value in &self.0 {
            value.validate()?;
        }
        Ok(())
    }

    fn intersection(&self, other: &Self) -> Self {
        Self(self.0.intersection(&other.0).cloned().collect())
    }

    fn difference(&self, other: &Self) -> Self {
        Self(self.0.difference(&other.0).cloned().collect())
    }
}

impl<'de> Deserialize<'de> for PermissionSet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let values = BTreeSet::<PluginPermission>::deserialize(deserializer)?;
        Self::new(values).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionResolutionInput {
    pub manifest_request: PermissionSet,
    pub installation_grant: PermissionSet,
    pub tenant_policy: PermissionSet,
    pub principal_authority: PermissionSet,
    pub execution_plane_policy: PermissionSet,
    pub runtime_policy: PermissionSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectivePluginPermissions {
    pub effective: PermissionSet,
    pub denied_requested: PermissionSet,
}

pub fn resolve_effective_permissions(
    input: &PermissionResolutionInput,
) -> Result<EffectivePluginPermissions, PluginContractError> {
    for set in [
        &input.manifest_request,
        &input.installation_grant,
        &input.tenant_policy,
        &input.principal_authority,
        &input.execution_plane_policy,
        &input.runtime_policy,
    ] {
        set.validate()?;
    }

    let effective = input
        .manifest_request
        .intersection(&input.installation_grant)
        .intersection(&input.tenant_policy)
        .intersection(&input.principal_authority)
        .intersection(&input.execution_plane_policy)
        .intersection(&input.runtime_policy);
    let denied_requested = input.manifest_request.difference(&effective);
    Ok(EffectivePluginPermissions {
        effective,
        denied_requested,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginIsolationProfile {
    FirstPartyNative,
    VerifiedSandboxed,
    LocalUnverified,
    SingleFileWebApp,
    RemoteWorker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationEnforcement {
    TrustedBuildBoundary,
    SandboxRequired,
    RemoteBoundaryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolationPolicy {
    pub profile: PluginIsolationProfile,
    pub enabled_in_r4: bool,
    pub enforcement: IsolationEnforcement,
    pub requires_publisher_verification: bool,
    pub ambient_filesystem: bool,
    pub direct_core_db: bool,
    pub direct_network: bool,
    pub mediated_talos_data: bool,
    pub governed_egress_relay: bool,
    pub cloud_enabled: bool,
}

pub fn r4_isolation_policy(profile: PluginIsolationProfile) -> IsolationPolicy {
    match profile {
        PluginIsolationProfile::FirstPartyNative => IsolationPolicy {
            profile,
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
        PluginIsolationProfile::VerifiedSandboxed => IsolationPolicy {
            profile,
            enabled_in_r4: false,
            enforcement: IsolationEnforcement::SandboxRequired,
            requires_publisher_verification: true,
            ambient_filesystem: false,
            direct_core_db: false,
            direct_network: false,
            mediated_talos_data: true,
            governed_egress_relay: true,
            cloud_enabled: false,
        },
        PluginIsolationProfile::LocalUnverified => IsolationPolicy {
            profile,
            enabled_in_r4: false,
            enforcement: IsolationEnforcement::SandboxRequired,
            requires_publisher_verification: false,
            ambient_filesystem: false,
            direct_core_db: false,
            direct_network: false,
            mediated_talos_data: true,
            governed_egress_relay: true,
            cloud_enabled: false,
        },
        PluginIsolationProfile::SingleFileWebApp => IsolationPolicy {
            profile,
            enabled_in_r4: false,
            enforcement: IsolationEnforcement::SandboxRequired,
            requires_publisher_verification: false,
            ambient_filesystem: false,
            direct_core_db: false,
            direct_network: false,
            mediated_talos_data: true,
            governed_egress_relay: true,
            cloud_enabled: false,
        },
        PluginIsolationProfile::RemoteWorker => IsolationPolicy {
            profile,
            enabled_in_r4: false,
            enforcement: IsolationEnforcement::RemoteBoundaryRequired,
            requires_publisher_verification: true,
            ambient_filesystem: false,
            direct_core_db: false,
            direct_network: false,
            mediated_talos_data: false,
            governed_egress_relay: true,
            cloud_enabled: false,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPackageIdentity {
    pub plugin_id: PluginId,
    pub publisher_id: PublisherId,
    pub version: ContractVersion,
    pub package_digest_sha256: Sha256Digest,
    pub manifest_digest_sha256: Sha256Digest,
    /// Exact semantic host capability contract implemented by this immutable
    /// package/manifest identity. P3 durable work pins the same value.
    pub capability_contract_version: ContractVersion,
}

impl PluginPackageIdentity {
    pub fn matches_executable(&self, executable: &PluginExecutableRef) -> bool {
        self.plugin_id == executable.plugin_id
            && self.version == executable.version
            && self.package_digest_sha256.as_str() == executable.package_digest_sha256
            && self.manifest_digest_sha256.as_str() == executable.manifest_digest_sha256
            && self.capability_contract_version == executable.capability_contract_version
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPackageRecord {
    pub identity: PluginPackageIdentity,
    pub compatibility_range: CompatibilityRange,
    pub declared_capabilities: BTreeSet<String>,
    pub permission_request: PermissionSet,
    pub verification: VerificationEvidence,
    pub lifecycle: PluginLifecycle,
}

impl PluginPackageRecord {
    pub fn validate(&self) -> Result<(), PluginContractError> {
        if self.declared_capabilities.len() > MAX_DECLARED_CAPABILITIES {
            return Err(PluginContractError::TooManyDeclaredCapabilities);
        }
        for capability in &self.declared_capabilities {
            validate_token(capability, "declared capability")?;
        }
        self.permission_request.validate()?;
        self.verification.validate()?;
        self.lifecycle.validate()?;
        Ok(())
    }

    pub fn executable_in_profile(
        &self,
        profile: PluginIsolationProfile,
    ) -> Result<(), PluginContractError> {
        self.validate()?;
        if !self.lifecycle.is_executable() {
            return Err(PluginContractError::LifecycleNotExecutable);
        }
        let policy = r4_isolation_policy(profile);
        if !policy.enabled_in_r4 {
            return Err(PluginContractError::IsolationProfileDisabled);
        }
        if policy.requires_publisher_verification && !self.verification.publisher_verified() {
            return Err(PluginContractError::PublisherVerificationRequired);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginContractError {
    InvalidToken,
    InvalidDigest,
    InvalidCompatibilityRange,
    TooManyDeclaredCapabilities,
    TooManyPermissions,
    ReservedEventSubject,
    ReservedBackgroundJobSubject,
    LifecycleNotExecutable,
    IsolationProfileDisabled,
    PublisherVerificationRequired,
}

impl std::fmt::Display for PluginContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PluginContractError {}

fn validate_token(value: &str, _kind: &'static str) -> Result<(), PluginContractError> {
    if value.is_empty()
        || value.len() > MAX_PLUGIN_TOKEN_BYTES
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(PluginContractError::InvalidToken);
    }
    Ok(())
}

/// Trusted host code converts the current execution plane into a permission
/// policy. The plane itself is never accepted from a plugin manifest/message.
pub fn execution_plane_permission_policy(
    plane: ExecutionPlane,
    tenant_business: PermissionSet,
    platform_control: PermissionSet,
    simulation: PermissionSet,
) -> PermissionSet {
    match plane {
        ExecutionPlane::TenantBusiness => tenant_business,
        ExecutionPlane::PlatformControl => platform_control,
        ExecutionPlane::Simulation => simulation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::interconnect::{BindingRevisionRef, ProviderInstanceRef};

    fn p() -> PluginPermission {
        PluginPermission::CapabilityInvoke("orders.read".into())
    }

    fn set(values: impl IntoIterator<Item = PluginPermission>) -> PermissionSet {
        PermissionSet::new(values).unwrap()
    }

    fn all_terms(permission: PluginPermission) -> PermissionResolutionInput {
        let one = set([permission]);
        PermissionResolutionInput {
            manifest_request: one.clone(),
            installation_grant: one.clone(),
            tenant_policy: one.clone(),
            principal_authority: one.clone(),
            execution_plane_policy: one.clone(),
            runtime_policy: one,
        }
    }

    #[test]
    fn permission_resolution_is_exact_six_way_intersection() {
        let input = all_terms(p());
        let resolved = resolve_effective_permissions(&input).unwrap();
        assert!(resolved.effective.contains(&p()));
        assert!(resolved.denied_requested.iter().next().is_none());

        for term in 0..6 {
            let mut input = all_terms(p());
            match term {
                0 => input.manifest_request = PermissionSet::empty(),
                1 => input.installation_grant = PermissionSet::empty(),
                2 => input.tenant_policy = PermissionSet::empty(),
                3 => input.principal_authority = PermissionSet::empty(),
                4 => input.execution_plane_policy = PermissionSet::empty(),
                5 => input.runtime_policy = PermissionSet::empty(),
                _ => unreachable!(),
            }
            let resolved = resolve_effective_permissions(&input).unwrap();
            assert!(
                !resolved.effective.contains(&p()),
                "term {term} was bypassed"
            );
        }
    }

    #[test]
    fn reserved_event_subjects_are_never_manifest_permissions() {
        let permission =
            PluginPermission::EventEmit(Subject::new("system.security.audit").unwrap());
        assert_eq!(
            PermissionSet::new([permission]),
            Err(PluginContractError::ReservedEventSubject)
        );
    }

    #[test]
    fn reserved_or_noncanonical_background_job_subjects_are_never_permissions() {
        assert_eq!(
            PermissionSet::new([PluginPermission::BackgroundJob(
                "system.security.audit".into(),
            )]),
            Err(PluginContractError::ReservedBackgroundJobSubject)
        );
        assert_eq!(
            PermissionSet::new([PluginPermission::BackgroundJob("Orders.Reconcile".into())]),
            Err(PluginContractError::InvalidToken)
        );
    }

    #[test]
    fn single_file_webapp_is_defined_but_disabled_without_sandbox() {
        let policy = r4_isolation_policy(PluginIsolationProfile::SingleFileWebApp);
        assert!(!policy.enabled_in_r4);
        assert!(!policy.ambient_filesystem);
        assert!(!policy.direct_core_db);
        assert!(!policy.direct_network);
        assert!(policy.mediated_talos_data);
        assert!(policy.governed_egress_relay);
        assert!(!policy.cloud_enabled);
        assert_eq!(policy.enforcement, IsolationEnforcement::SandboxRequired);
    }

    #[test]
    fn unverified_or_attested_native_package_cannot_execute() {
        for verification in [
            VerificationEvidence::Unverified,
            VerificationEvidence::Attested {
                attestation_digest_sha256: Sha256Digest::new("12".repeat(32)).unwrap(),
            },
        ] {
            let record = fixture_record(verification);
            assert_eq!(
                record.executable_in_profile(PluginIsolationProfile::FirstPartyNative),
                Err(PluginContractError::PublisherVerificationRequired)
            );
        }
        let record = fixture_record(VerificationEvidence::Unverified);
        assert_eq!(
            record.executable_in_profile(PluginIsolationProfile::VerifiedSandboxed),
            Err(PluginContractError::IsolationProfileDisabled)
        );
    }

    #[test]
    fn package_identity_matches_p3_executable_pin_exactly() {
        let record = fixture_record(VerificationEvidence::PublisherVerified {
            publisher_key_id: "talos.release.root".into(),
            signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
        });
        let executable = PluginExecutableRef {
            plugin_id: PluginId::new("official.fixture").unwrap(),
            version: ContractVersion::new("1.0.0").unwrap(),
            package_digest_sha256: "ab".repeat(32),
            manifest_digest_sha256: "ef".repeat(32),
            capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
            provider_instance_id: Some(ProviderInstanceRef::new("provider-a").unwrap()),
            binding_revision: Some(BindingRevisionRef::new("binding-rev-1").unwrap()),
        };
        assert!(record.identity.matches_executable(&executable));

        let mut wrong_contract = executable.clone();
        wrong_contract.capability_contract_version = ContractVersion::new("2.0.0").unwrap();
        assert!(!record.identity.matches_executable(&wrong_contract));
    }

    fn fixture_record(verification: VerificationEvidence) -> PluginPackageRecord {
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
            permission_request: set([p()]),
            verification,
            lifecycle: PluginLifecycle::Active,
        }
    }
}

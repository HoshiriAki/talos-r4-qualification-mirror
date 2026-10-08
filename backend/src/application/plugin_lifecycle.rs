//! R4-P6 host-owned plugin supply-chain lifecycle contracts.
//!
//! Verification proof, durable package state and tenant installation state are
//! separate. Backends implement the same transition semantics without exposing a
//! raw SQL handle or caller-controlled provenance shortcut.

use std::collections::BTreeSet;

use serde::de::DeserializeOwned;
use system_core::TenantId;
use system_core::security::plugin::{
    CompatibilityRange, PermissionSet, PluginIsolationProfile, PluginLifecycle,
    PluginPackageIdentity, PluginPackageRecord, PluginPermission, PublisherId, Sha256Digest,
    VerificationEvidence, r4_isolation_policy,
};
use system_core::security::plugin_upgrade::PluginUpgradeReview;
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInstallationActivation {
    pub installation_id: String,
    pub tenant_id: TenantId,
    pub isolation_profile: PluginIsolationProfile,
    pub installation_grant: PermissionSet,
    pub tenant_policy: PermissionSet,
    pub grant_revision: u64,
    pub tenant_policy_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginUpgradeAuthorization {
    pub previous_executable: PluginExecutableRef,
    pub review: PluginUpgradeReview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginLifecycleMutationError {
    InvalidVerifiedPackage,
    InvalidActivation,
    PackageAlreadyRegistered,
    PackageNotFound,
    PackageNotActivatable,
    InstallationAlreadyExists,
    UpgradeReviewRequired,
    UnexpectedUpgradeReview,
    UpgradeSourceNotFound,
    UpgradeSourceNotExecutable,
    UpgradeReviewMismatch,
    GrantOutsideManifest,
    InvalidReason,
    InvalidTransition,
    Persistence,
    InvalidPersistedRecord,
}

impl std::fmt::Display for PluginLifecycleMutationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PluginLifecycleMutationError {}

#[cfg(feature = "sqlite")]
pub struct SqlitePluginLifecycleService {
    pub(crate) pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
}

#[cfg(feature = "sqlite")]
impl SqlitePluginLifecycleService {
    pub fn new(pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>) -> Self {
        Self { pool }
    }
}

#[cfg(feature = "postgres")]
pub struct PostgresPluginLifecycleService {
    pub(crate) pool: sqlx::PgPool,
}

#[cfg(feature = "postgres")]
impl PostgresPluginLifecycleService {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

pub(crate) fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 192
        && value.is_ascii()
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

pub(crate) fn validate_activation(
    package: &PluginPackageRecord,
    activation: &PluginInstallationActivation,
) -> Result<(), PluginLifecycleMutationError> {
    package
        .validate()
        .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?;
    if !valid_token(&activation.installation_id)
        || activation.grant_revision == 0
        || activation.tenant_policy_revision == 0
        || !r4_isolation_policy(activation.isolation_profile).enabled_in_r4
    {
        return Err(PluginLifecycleMutationError::InvalidActivation);
    }
    activation
        .installation_grant
        .validate()
        .map_err(|_| PluginLifecycleMutationError::InvalidActivation)?;
    activation
        .tenant_policy
        .validate()
        .map_err(|_| PluginLifecycleMutationError::InvalidActivation)?;
    if activation
        .installation_grant
        .iter()
        .any(|permission| !package.permission_request.contains(permission))
    {
        return Err(PluginLifecycleMutationError::GrantOutsideManifest);
    }
    Ok(())
}

pub(crate) fn isolation_profile_name(profile: PluginIsolationProfile) -> &'static str {
    match profile {
        PluginIsolationProfile::FirstPartyNative => "first_party_native",
        PluginIsolationProfile::VerifiedSandboxed => "verified_sandboxed",
        PluginIsolationProfile::LocalUnverified => "local_unverified",
        PluginIsolationProfile::SingleFileWebApp => "single_file_web_app",
        PluginIsolationProfile::RemoteWorker => "remote_worker",
    }
}

#[derive(Debug)]
pub(crate) struct PersistedPackage {
    pub publisher_id: String,
    pub compatibility_range: String,
    pub declared_capabilities_json: String,
    pub permission_request_json: String,
    pub verification_evidence_json: String,
    pub lifecycle_state: String,
    pub revoked_reason_code: Option<String>,
}

fn decode_json<T: DeserializeOwned>(value: &str) -> Result<T, PluginLifecycleMutationError> {
    serde_json::from_str(value).map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)
}

pub(crate) fn decode_package(
    persisted: PersistedPackage,
    identity: &PluginPackageIdentity,
) -> Result<PluginPackageRecord, PluginLifecycleMutationError> {
    let lifecycle = match persisted.lifecycle_state.as_str() {
        "staged" => PluginLifecycle::Staged,
        "active" => PluginLifecycle::Active,
        "suspended" => PluginLifecycle::Suspended,
        "revoked" => PluginLifecycle::Revoked {
            reason_code: persisted
                .revoked_reason_code
                .ok_or(PluginLifecycleMutationError::InvalidPersistedRecord)?,
        },
        _ => return Err(PluginLifecycleMutationError::InvalidPersistedRecord),
    };
    let record = PluginPackageRecord {
        identity: PluginPackageIdentity {
            plugin_id: PluginId::new(identity.plugin_id.as_str())
                .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
            publisher_id: PublisherId::new(persisted.publisher_id)
                .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
            version: ContractVersion::new(identity.version.as_str())
                .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
            package_digest_sha256: Sha256Digest::new(identity.package_digest_sha256.as_str())
                .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
            manifest_digest_sha256: Sha256Digest::new(identity.manifest_digest_sha256.as_str())
                .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
            capability_contract_version: ContractVersion::new(
                identity.capability_contract_version.as_str(),
            )
            .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
        },
        compatibility_range: CompatibilityRange::new(persisted.compatibility_range)
            .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
        declared_capabilities: decode_json::<BTreeSet<String>>(
            &persisted.declared_capabilities_json,
        )?,
        permission_request: decode_json::<PermissionSet>(&persisted.permission_request_json)?,
        verification: decode_json::<VerificationEvidence>(&persisted.verification_evidence_json)?,
        lifecycle,
    };
    if record.identity != *identity {
        return Err(PluginLifecycleMutationError::InvalidPersistedRecord);
    }
    record
        .validate()
        .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?;
    Ok(record)
}

pub(crate) fn package_identity_from_executable(
    executable: &PluginExecutableRef,
    publisher_id: PublisherId,
) -> Result<PluginPackageIdentity, PluginLifecycleMutationError> {
    executable
        .validate()
        .map_err(|_| PluginLifecycleMutationError::InvalidActivation)?;
    Ok(PluginPackageIdentity {
        plugin_id: PluginId::new(executable.plugin_id.as_str())
            .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
        publisher_id,
        version: ContractVersion::new(executable.version.as_str())
            .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
        package_digest_sha256: Sha256Digest::new(&executable.package_digest_sha256)
            .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
        manifest_digest_sha256: Sha256Digest::new(&executable.manifest_digest_sha256)
            .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
        capability_contract_version: ContractVersion::new(
            executable.capability_contract_version.as_str(),
        )
        .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
    })
}

pub(crate) fn validate_upgrade(
    previous: &PluginPackageRecord,
    candidate: &PluginPackageRecord,
    authorization: &PluginUpgradeAuthorization,
) -> Result<(), PluginLifecycleMutationError> {
    if !authorization.review.matches_transition(previous, candidate) {
        return Err(PluginLifecycleMutationError::UpgradeReviewMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_grant_cannot_bank_authority_outside_manifest_request() {
        let read = PluginPermission::CapabilityInvoke("orders.read".into());
        let package = PluginPackageRecord {
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
            permission_request: PermissionSet::new([read.clone()]).unwrap(),
            verification: VerificationEvidence::PublisherVerified {
                publisher_key_id: "talos.release.root".into(),
                signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
            },
            lifecycle: PluginLifecycle::Active,
        };
        let activation = PluginInstallationActivation {
            installation_id: "install-a".into(),
            tenant_id: TenantId::new("tenant-a").unwrap(),
            isolation_profile: PluginIsolationProfile::FirstPartyNative,
            installation_grant: PermissionSet::new([
                read,
                PluginPermission::NetworkGrant("provider.api".into()),
            ])
            .unwrap(),
            tenant_policy: PermissionSet::empty(),
            grant_revision: 1,
            tenant_policy_revision: 1,
        };
        assert_eq!(
            validate_activation(&package, &activation),
            Err(PluginLifecycleMutationError::GrantOutsideManifest)
        );
    }
}

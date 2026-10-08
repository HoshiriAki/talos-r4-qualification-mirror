//! R4-P6 package transition, permission-diff and migration authority.
//!
//! Package upgrades/rebinds must not silently inherit newly requested authority
//! or silently retarget durable work. The review result is exact and
//! content-derived; it is not a boolean supplied by the candidate package and
//! cannot be reused for another package transition.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::plugin::{PermissionSet, PluginPackageIdentity, PluginPackageRecord, PluginPermission};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPermissionDiff {
    pub added: PermissionSet,
    pub removed: PermissionSet,
    pub unchanged: PermissionSet,
}

impl PluginPermissionDiff {
    pub fn has_authority_increase(&self) -> bool {
        self.added.iter().next().is_some()
    }
}

/// R4's only reviewed upgrade migration strategy.
///
/// Existing durable operations keep their exact `PluginExecutableRef`; an
/// upgrade cannot rewrite queued work to the candidate package. The old
/// verified artifact/installation remains separately addressable until its
/// pinned work drains or an explicit future rebind mechanism is designed and
/// reviewed. Adding another strategy is therefore a Stable Core change rather
/// than an application-side flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginUpgradeMigrationStrategy {
    PreserveExactPins,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginUpgradeReview {
    previous_identity: PluginPackageIdentity,
    candidate_identity: PluginPackageIdentity,
    permission_diff: PluginPermissionDiff,
    /// Exact newly requested permissions approved for this transition. This is
    /// intentionally not carried forward as a reusable wildcard approval.
    approved_additions: PermissionSet,
    /// Migration semantics are explicit even though R4 supports only the
    /// conservative no-retarget strategy.
    migration_strategy: PluginUpgradeMigrationStrategy,
}

impl PluginUpgradeReview {
    pub fn previous_identity(&self) -> &PluginPackageIdentity {
        &self.previous_identity
    }

    pub fn candidate_identity(&self) -> &PluginPackageIdentity {
        &self.candidate_identity
    }

    pub fn permission_diff(&self) -> &PluginPermissionDiff {
        &self.permission_diff
    }

    pub fn approved_additions(&self) -> &PermissionSet {
        &self.approved_additions
    }

    pub fn migration_strategy(&self) -> PluginUpgradeMigrationStrategy {
        self.migration_strategy
    }

    pub fn matches_transition(
        &self,
        previous: &PluginPackageRecord,
        candidate: &PluginPackageRecord,
    ) -> bool {
        self.previous_identity == previous.identity
            && self.candidate_identity == candidate.identity
            && self.migration_strategy == PluginUpgradeMigrationStrategy::PreserveExactPins
            && permission_diff(&previous.permission_request, &candidate.permission_request)
                .is_ok_and(|diff| diff == self.permission_diff)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginUpgradeError {
    InvalidPackage,
    DifferentPlugin,
    PublisherChanged,
    AddedPermissionNotApproved,
    ApprovalOutsideCurrentDiff,
}

impl std::fmt::Display for PluginUpgradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PluginUpgradeError {}

pub fn permission_diff(
    previous: &PermissionSet,
    candidate: &PermissionSet,
) -> Result<PluginPermissionDiff, PluginUpgradeError> {
    previous
        .validate()
        .map_err(|_| PluginUpgradeError::InvalidPackage)?;
    candidate
        .validate()
        .map_err(|_| PluginUpgradeError::InvalidPackage)?;

    let added = collect(
        candidate
            .iter()
            .filter(|permission| !previous.contains(permission)),
    )?;
    let removed = collect(
        previous
            .iter()
            .filter(|permission| !candidate.contains(permission)),
    )?;
    let unchanged = collect(
        candidate
            .iter()
            .filter(|permission| previous.contains(permission)),
    )?;

    Ok(PluginPermissionDiff {
        added,
        removed,
        unchanged,
    })
}

/// Review a transition between two immutable package records.
///
/// Ordinary upgrades retain plugin and publisher identity. Publisher transfer
/// is a separate administrative/re-verification operation and cannot be hidden
/// in a package update. Every newly requested permission needs an exact approval
/// for this transition, and approvals that are not part of the current diff are
/// rejected rather than banked for future use. R4 records the conservative
/// `PreserveExactPins` migration strategy so no caller can reinterpret an
/// approved upgrade as permission to rewrite queued executable identities.
pub fn review_package_transition(
    previous: &PluginPackageRecord,
    candidate: &PluginPackageRecord,
    approved_additions: PermissionSet,
) -> Result<PluginUpgradeReview, PluginUpgradeError> {
    previous
        .validate()
        .map_err(|_| PluginUpgradeError::InvalidPackage)?;
    candidate
        .validate()
        .map_err(|_| PluginUpgradeError::InvalidPackage)?;
    approved_additions
        .validate()
        .map_err(|_| PluginUpgradeError::InvalidPackage)?;

    if previous.identity.plugin_id != candidate.identity.plugin_id {
        return Err(PluginUpgradeError::DifferentPlugin);
    }
    if previous.identity.publisher_id != candidate.identity.publisher_id {
        return Err(PluginUpgradeError::PublisherChanged);
    }

    let diff = permission_diff(&previous.permission_request, &candidate.permission_request)?;
    if diff
        .added
        .iter()
        .any(|permission| !approved_additions.contains(permission))
    {
        return Err(PluginUpgradeError::AddedPermissionNotApproved);
    }
    if approved_additions
        .iter()
        .any(|permission| !diff.added.contains(permission))
    {
        return Err(PluginUpgradeError::ApprovalOutsideCurrentDiff);
    }

    Ok(PluginUpgradeReview {
        previous_identity: previous.identity.clone(),
        candidate_identity: candidate.identity.clone(),
        permission_diff: diff,
        approved_additions,
        migration_strategy: PluginUpgradeMigrationStrategy::PreserveExactPins,
    })
}

fn collect<'a>(
    values: impl IntoIterator<Item = &'a PluginPermission>,
) -> Result<PermissionSet, PluginUpgradeError> {
    let values: BTreeSet<_> = values.into_iter().cloned().collect();
    PermissionSet::new(values).map_err(|_| PluginUpgradeError::InvalidPackage)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::security::plugin::{
        CompatibilityRange, PluginLifecycle, PluginPackageIdentity, PublisherId, Sha256Digest,
        VerificationEvidence,
    };
    use crate::transport::interconnect::{ContractVersion, PluginId};

    fn set(values: impl IntoIterator<Item = PluginPermission>) -> PermissionSet {
        PermissionSet::new(values).unwrap()
    }

    fn read() -> PluginPermission {
        PluginPermission::CapabilityInvoke("orders.read".into())
    }

    fn network() -> PluginPermission {
        PluginPermission::NetworkGrant("provider.fixture.api".into())
    }

    fn secret() -> PluginPermission {
        PluginPermission::SecretPurpose("fixture.secret".into())
    }

    fn package(
        plugin_id: &str,
        publisher_id: &str,
        version: &str,
        digest_seed: &str,
        permissions: PermissionSet,
    ) -> PluginPackageRecord {
        PluginPackageRecord {
            identity: PluginPackageIdentity {
                plugin_id: PluginId::new(plugin_id).unwrap(),
                publisher_id: PublisherId::new(publisher_id).unwrap(),
                version: ContractVersion::new(version).unwrap(),
                package_digest_sha256: Sha256Digest::new(digest_seed.repeat(32)).unwrap(),
                manifest_digest_sha256: Sha256Digest::new("ef".repeat(32)).unwrap(),
                capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
            },
            compatibility_range: CompatibilityRange::new(">=1.0.0,<2.0.0").unwrap(),
            declared_capabilities: BTreeSet::from(["orders.read".into()]),
            permission_request: permissions,
            verification: VerificationEvidence::PublisherVerified {
                publisher_key_id: "talos.release.root".into(),
                signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
            },
            lifecycle: PluginLifecycle::Active,
        }
    }

    #[test]
    fn exact_diff_classifies_added_removed_and_unchanged_permissions() {
        let previous = set([read(), network()]);
        let candidate = set([read(), secret()]);
        let diff = permission_diff(&previous, &candidate).unwrap();
        assert!(diff.added.contains(&secret()));
        assert!(diff.removed.contains(&network()));
        assert!(diff.unchanged.contains(&read()));
        assert!(diff.has_authority_increase());
    }

    #[test]
    fn added_permission_requires_exact_transition_approval() {
        let previous = package(
            "official.fixture",
            "talos.official",
            "1.0.0",
            "ab",
            set([read()]),
        );
        let candidate = package(
            "official.fixture",
            "talos.official",
            "1.1.0",
            "12",
            set([read(), network()]),
        );

        assert_eq!(
            review_package_transition(&previous, &candidate, PermissionSet::empty()),
            Err(PluginUpgradeError::AddedPermissionNotApproved)
        );
        let review = review_package_transition(&previous, &candidate, set([network()])).unwrap();
        assert!(review.permission_diff().added.contains(&network()));
        assert_eq!(review.previous_identity(), &previous.identity);
        assert_eq!(review.candidate_identity(), &candidate.identity);
        assert_eq!(
            review.migration_strategy(),
            PluginUpgradeMigrationStrategy::PreserveExactPins
        );
        assert!(review.matches_transition(&previous, &candidate));
    }

    #[test]
    fn review_cannot_be_reused_for_another_candidate_digest() {
        let previous = package(
            "official.fixture",
            "talos.official",
            "1.0.0",
            "ab",
            set([read()]),
        );
        let candidate = package(
            "official.fixture",
            "talos.official",
            "1.1.0",
            "12",
            set([read(), network()]),
        );
        let review = review_package_transition(&previous, &candidate, set([network()])).unwrap();
        let other_candidate = package(
            "official.fixture",
            "talos.official",
            "1.1.0",
            "34",
            set([read(), network()]),
        );
        assert!(!review.matches_transition(&previous, &other_candidate));
    }

    #[test]
    fn approval_cannot_be_banked_for_a_future_permission() {
        let previous = package(
            "official.fixture",
            "talos.official",
            "1.0.0",
            "ab",
            set([read()]),
        );
        let candidate = package(
            "official.fixture",
            "talos.official",
            "1.1.0",
            "12",
            set([read(), network()]),
        );
        assert_eq!(
            review_package_transition(&previous, &candidate, set([network(), secret()])),
            Err(PluginUpgradeError::ApprovalOutsideCurrentDiff)
        );
    }

    #[test]
    fn publisher_transfer_is_not_an_ordinary_package_upgrade() {
        let previous = package(
            "official.fixture",
            "talos.official",
            "1.0.0",
            "ab",
            set([read()]),
        );
        let candidate = package(
            "official.fixture",
            "vendor.example",
            "1.1.0",
            "12",
            set([read()]),
        );
        assert_eq!(
            review_package_transition(&previous, &candidate, PermissionSet::empty()),
            Err(PluginUpgradeError::PublisherChanged)
        );
    }
}

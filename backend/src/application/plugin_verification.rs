//! R4-P6 trusted plugin package verification boundary.
//!
//! Package metadata may request authority, but it cannot self-assert trusted
//! provenance. The host recomputes package/manifest content digests and delegates
//! publisher signature verification to a host-owned key/crypto Profile. Only a
//! successful verification produces an opaque `VerifiedPluginPackage` proof.

use sha2::{Digest, Sha256};
use system_core::security::plugin::{
    PluginLifecycle, PluginPackageRecord, PluginPermission, PublisherId, Sha256Digest,
    VerificationEvidence,
};

const MAX_PUBLISHER_KEY_ID_BYTES: usize = 192;
const MAX_SIGNATURE_BYTES: usize = 1024 * 1024;
const SIGNED_STATEMENT_DOMAIN: &[u8] = b"TALOS_PLUGIN_PACKAGE_V1";

pub trait PluginPublisherSignatureVerifier: Send + Sync {
    fn verify(
        &self,
        publisher_id: &PublisherId,
        publisher_key_id: &str,
        signed_statement: &[u8],
        signature: &[u8],
    ) -> Result<bool, String>;
}

/// Type-level proof that publisher verification was performed by the trusted
/// host boundary. The inner record is deliberately private and cannot be
/// constructed from deserialized/plugin-controlled `VerificationEvidence`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPluginPackage {
    record: PluginPackageRecord,
}

impl VerifiedPluginPackage {
    pub fn record(&self) -> &PluginPackageRecord {
        &self.record
    }

    pub(crate) fn into_record(self) -> PluginPackageRecord {
        self.record
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginPackageVerificationError {
    InvalidCandidate,
    CandidateClaimsVerifiedEvidence,
    PublisherKeyInvalid,
    SignatureTooLarge,
    PackageDigestMismatch,
    ManifestDigestMismatch,
    SignatureRejected,
    BackendFailure,
}

impl std::fmt::Display for PluginPackageVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for PluginPackageVerificationError {}

/// Verify immutable package/manifest material and return an opaque staged proof
/// whose provenance evidence was produced by the trusted host, not by plugin
/// input or a caller-constructed package record.
pub fn verify_publisher_package(
    candidate: &PluginPackageRecord,
    package_bytes: &[u8],
    manifest_bytes: &[u8],
    publisher_key_id: &str,
    signature: &[u8],
    verifier: &dyn PluginPublisherSignatureVerifier,
) -> Result<VerifiedPluginPackage, PluginPackageVerificationError> {
    candidate
        .validate()
        .map_err(|_| PluginPackageVerificationError::InvalidCandidate)?;
    if candidate.lifecycle != PluginLifecycle::Staged {
        return Err(PluginPackageVerificationError::InvalidCandidate);
    }
    if !matches!(
        candidate.verification,
        VerificationEvidence::Unverified | VerificationEvidence::DigestVerified
    ) {
        return Err(PluginPackageVerificationError::CandidateClaimsVerifiedEvidence);
    }
    if !valid_key_id(publisher_key_id) {
        return Err(PluginPackageVerificationError::PublisherKeyInvalid);
    }
    if signature.len() > MAX_SIGNATURE_BYTES {
        return Err(PluginPackageVerificationError::SignatureTooLarge);
    }

    let package_digest = sha256_hex(package_bytes);
    if package_digest != candidate.identity.package_digest_sha256.as_str() {
        return Err(PluginPackageVerificationError::PackageDigestMismatch);
    }
    let manifest_digest = sha256_hex(manifest_bytes);
    if manifest_digest != candidate.identity.manifest_digest_sha256.as_str() {
        return Err(PluginPackageVerificationError::ManifestDigestMismatch);
    }

    let statement = publisher_verification_statement(candidate)?;
    let verified = verifier
        .verify(
            &candidate.identity.publisher_id,
            publisher_key_id,
            &statement,
            signature,
        )
        .map_err(|_| PluginPackageVerificationError::BackendFailure)?;
    if !verified {
        return Err(PluginPackageVerificationError::SignatureRejected);
    }

    let mut record = candidate.clone();
    record.verification = VerificationEvidence::PublisherVerified {
        publisher_key_id: publisher_key_id.to_owned(),
        signature_digest_sha256: Sha256Digest::new(sha256_hex(signature))
            .map_err(|_| PluginPackageVerificationError::InvalidCandidate)?,
    };
    record
        .validate()
        .map_err(|_| PluginPackageVerificationError::InvalidCandidate)?;
    Ok(VerifiedPluginPackage { record })
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PUBLISHER_KEY_ID_BYTES
        && value.is_ascii()
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn append_field(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), PluginPackageVerificationError> {
    let len =
        u32::try_from(bytes.len()).map_err(|_| PluginPackageVerificationError::InvalidCandidate)?;
    output.extend_from_slice(&len.to_be_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

fn append_count(output: &mut Vec<u8>, count: usize) -> Result<(), PluginPackageVerificationError> {
    let count =
        u32::try_from(count).map_err(|_| PluginPackageVerificationError::InvalidCandidate)?;
    output.extend_from_slice(&count.to_be_bytes());
    Ok(())
}

/// Canonical permission representation used by the signature statement.
///
/// This deliberately does not depend on Rust enum discriminants or serde/JSON
/// object ordering. Cross-language implementations sort `(dimension, value)`
/// byte strings lexicographically and then emit the same length-prefixed fields.
fn canonical_permission(permission: &PluginPermission) -> (&'static str, &str) {
    match permission {
        PluginPermission::CapabilityInvoke(value) => ("capability_invoke", value),
        PluginPermission::CapabilityProvide(value) => ("capability_provide", value),
        PluginPermission::DataReadProjection(value) => ("data_read_projection", value),
        PluginPermission::DataWriteProjection(value) => ("data_write_projection", value),
        PluginPermission::NetworkGrant(value) => ("network_grant", value),
        PluginPermission::SecretPurpose(value) => ("secret_purpose", value),
        PluginPermission::EventEmit(subject) => ("event_emit", subject.as_str()),
        PluginPermission::EventSubscribe(subject) => ("event_subscribe", subject.as_str()),
        PluginPermission::ExternalEffect(value) => ("external_effect", value),
        PluginPermission::BackgroundJob(value) => ("background_job", value),
        PluginPermission::PluginStorage(value) => ("plugin_storage", value),
        PluginPermission::UiExtension(value) => ("ui_extension", value),
    }
}

/// Length-prefixed, versioned, language-neutral verification statement.
///
/// The content digests bind raw package/manifest bytes. The semantic fields are
/// also bound directly so a parser/record mismatch cannot pair a legitimate
/// signed manifest with altered compatibility, capability, or permission facts.
/// Collection ordering is explicitly canonical and therefore does not depend on
/// a particular serializer implementation.
fn publisher_verification_statement(
    candidate: &PluginPackageRecord,
) -> Result<Vec<u8>, PluginPackageVerificationError> {
    let mut output = SIGNED_STATEMENT_DOMAIN.to_vec();
    for value in [
        candidate.identity.plugin_id.as_str(),
        candidate.identity.publisher_id.as_str(),
        candidate.identity.version.as_str(),
        candidate.identity.package_digest_sha256.as_str(),
        candidate.identity.manifest_digest_sha256.as_str(),
        candidate.identity.capability_contract_version.as_str(),
        candidate.compatibility_range.as_str(),
    ] {
        append_field(&mut output, value.as_bytes())?;
    }

    append_field(&mut output, b"declared_capabilities")?;
    append_count(&mut output, candidate.declared_capabilities.len())?;
    for capability in &candidate.declared_capabilities {
        append_field(&mut output, capability.as_bytes())?;
    }

    append_field(&mut output, b"permission_request")?;
    let mut permissions: Vec<_> = candidate
        .permission_request
        .iter()
        .map(canonical_permission)
        .collect();
    permissions.sort_unstable();
    append_count(&mut output, permissions.len())?;
    for (dimension, value) in permissions {
        append_field(&mut output, dimension.as_bytes())?;
        append_field(&mut output, value.as_bytes())?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    use system_core::security::plugin::{
        CompatibilityRange, PermissionSet, PluginPackageIdentity, PluginPermission,
    };
    use system_core::transport::interconnect::{ContractVersion, PluginId};

    use super::*;

    struct FixtureVerifier {
        accept: bool,
        expected_statement: Option<Vec<u8>>,
        calls: Mutex<Vec<Vec<u8>>>,
    }

    impl PluginPublisherSignatureVerifier for FixtureVerifier {
        fn verify(
            &self,
            publisher_id: &PublisherId,
            publisher_key_id: &str,
            signed_statement: &[u8],
            _signature: &[u8],
        ) -> Result<bool, String> {
            assert_eq!(publisher_id.as_str(), "talos.official");
            assert_eq!(publisher_key_id, "talos.release.root");
            self.calls.lock().unwrap().push(signed_statement.to_vec());
            Ok(self.accept
                && self
                    .expected_statement
                    .as_ref()
                    .is_none_or(|expected| expected.as_slice() == signed_statement))
        }
    }

    fn verifier(accept: bool) -> FixtureVerifier {
        FixtureVerifier {
            accept,
            expected_statement: None,
            calls: Mutex::new(Vec::new()),
        }
    }

    fn candidate(package_bytes: &[u8], manifest_bytes: &[u8]) -> PluginPackageRecord {
        PluginPackageRecord {
            identity: PluginPackageIdentity {
                plugin_id: PluginId::new("official.fixture").unwrap(),
                publisher_id: PublisherId::new("talos.official").unwrap(),
                version: ContractVersion::new("1.0.0").unwrap(),
                package_digest_sha256: Sha256Digest::new(sha256_hex(package_bytes)).unwrap(),
                manifest_digest_sha256: Sha256Digest::new(sha256_hex(manifest_bytes)).unwrap(),
                capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
            },
            compatibility_range: CompatibilityRange::new(">=1.0.0,<2.0.0").unwrap(),
            declared_capabilities: BTreeSet::from(["orders.read".into()]),
            permission_request: PermissionSet::new([PluginPermission::CapabilityInvoke(
                "orders.read".into(),
            )])
            .unwrap(),
            verification: VerificationEvidence::Unverified,
            lifecycle: PluginLifecycle::Staged,
        }
    }

    #[test]
    fn host_recomputes_both_digests_before_publisher_verification() {
        let package_bytes = b"package-v1";
        let manifest_bytes = b"manifest-v1";
        let verifier = verifier(true);
        let verified = verify_publisher_package(
            &candidate(package_bytes, manifest_bytes),
            package_bytes,
            manifest_bytes,
            "talos.release.root",
            b"signature",
            &verifier,
        )
        .unwrap();
        assert!(matches!(
            verified.record().verification,
            VerificationEvidence::PublisherVerified { .. }
        ));
        assert_eq!(verified.record().lifecycle, PluginLifecycle::Staged);
        assert_eq!(verifier.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn content_substitution_is_rejected_before_signature_backend() {
        let verifier = verifier(true);
        assert_eq!(
            verify_publisher_package(
                &candidate(b"package-v1", b"manifest-v1"),
                b"package-v2",
                b"manifest-v1",
                "talos.release.root",
                b"signature",
                &verifier,
            ),
            Err(PluginPackageVerificationError::PackageDigestMismatch)
        );
        assert!(verifier.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn candidate_cannot_self_assert_publisher_verified_evidence() {
        let mut candidate = candidate(b"package-v1", b"manifest-v1");
        candidate.verification = VerificationEvidence::PublisherVerified {
            publisher_key_id: "talos.release.root".into(),
            signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
        };
        let verifier = verifier(true);
        assert_eq!(
            verify_publisher_package(
                &candidate,
                b"package-v1",
                b"manifest-v1",
                "talos.release.root",
                b"signature",
                &verifier,
            ),
            Err(PluginPackageVerificationError::CandidateClaimsVerifiedEvidence)
        );
        assert!(verifier.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn semantic_permission_tamper_changes_the_signed_statement() {
        let original = candidate(b"package-v1", b"manifest-v1");
        let original_statement = publisher_verification_statement(&original).unwrap();
        let mut tampered = original.clone();
        tampered.permission_request = PermissionSet::new([
            PluginPermission::CapabilityInvoke("orders.read".into()),
            PluginPermission::NetworkGrant("attacker.egress".into()),
        ])
        .unwrap();
        let verifier = FixtureVerifier {
            accept: true,
            expected_statement: Some(original_statement),
            calls: Mutex::new(Vec::new()),
        };
        assert_eq!(
            verify_publisher_package(
                &tampered,
                b"package-v1",
                b"manifest-v1",
                "talos.release.root",
                b"signature",
                &verifier,
            ),
            Err(PluginPackageVerificationError::SignatureRejected)
        );
    }

    #[test]
    fn canonical_permission_encoding_is_serializer_independent_and_sorted() {
        let mut record = candidate(b"package-v1", b"manifest-v1");
        record.permission_request = PermissionSet::new([
            PluginPermission::NetworkGrant("provider.api".into()),
            PluginPermission::CapabilityInvoke("orders.read".into()),
        ])
        .unwrap();
        let statement = publisher_verification_statement(&record).unwrap();
        assert!(
            statement
                .windows(b"capability_invoke".len())
                .any(|window| window == b"capability_invoke")
        );
        assert!(
            statement
                .windows(b"network_grant".len())
                .any(|window| window == b"network_grant")
        );
    }

    #[test]
    fn rejected_signature_never_produces_verified_evidence() {
        let verifier = verifier(false);
        assert_eq!(
            verify_publisher_package(
                &candidate(b"package-v1", b"manifest-v1"),
                b"package-v1",
                b"manifest-v1",
                "talos.release.root",
                b"signature",
                &verifier,
            ),
            Err(PluginPackageVerificationError::SignatureRejected)
        );
    }
}

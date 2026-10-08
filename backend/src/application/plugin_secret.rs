//! R4-P6 purpose-bound plugin secret operations.
//!
//! This adapter deliberately exposes no `get_secret`/`get_key` operation. The
//! plugin receives only operation results produced by a host-owned backend after
//! exact tenant/plugin/installation/executable/purpose authorization.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use system_core::TenantId;
use system_core::security::plugin::PluginPermission;
use system_core::security::plugin_secret::{PluginSecretHandleRef, PluginSecretOperationKind};
use system_core::transport::interconnect::{PluginExecutableRef, PluginId};

use super::plugin_host::{PluginAdmission, PluginHostError};

const MAX_SECRET_OPERATION_INPUT_BYTES: usize = 1024 * 1024;
const MAX_SECRET_OPERATION_OUTPUT_BYTES: usize = 1024 * 1024;

/// Host-owned secret backend. The complete revisioned handle is passed through
/// so KMS/HSM adapters can reject a stale handle revision instead of silently
/// operating on whatever key revision currently occupies the same identifier.
pub trait PluginSecretBackend: Send + Sync {
    fn sign(&self, handle: &PluginSecretHandleRef, payload: &[u8]) -> Result<Vec<u8>, String>;
    fn decrypt(
        &self,
        handle: &PluginSecretHandleRef,
        ciphertext: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSecretBinding {
    pub tenant_id: TenantId,
    pub plugin_id: PluginId,
    pub installation_id: String,
    /// Exact P3 executable identity admitted to use this binding. An install id
    /// is not sufficient by itself because durable work and upgrades are pinned
    /// to version/package/manifest/capability-contract facts.
    pub executable: PluginExecutableRef,
    pub purpose: String,
    pub handle: PluginSecretHandleRef,
    pub allowed_operations: BTreeSet<PluginSecretOperationKind>,
}

impl PluginSecretBinding {
    fn validate(&self) -> Result<(), PluginSecretError> {
        if self.installation_id.is_empty()
            || self.installation_id.len() > 192
            || !self.installation_id.is_ascii()
            || self
                .installation_id
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
            || self.plugin_id != self.executable.plugin_id
            || self.purpose != self.handle.purpose()
            || self.allowed_operations.is_empty()
        {
            return Err(PluginSecretError::InvalidPolicy);
        }
        self.executable
            .validate()
            .map_err(|_| PluginSecretError::InvalidPolicy)?;
        PluginPermission::SecretPurpose(self.purpose.clone())
            .validate()
            .map_err(|_| PluginSecretError::InvalidPolicy)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BindingKey {
    tenant_id: String,
    plugin_id: String,
    installation_id: String,
    purpose: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginSecretPolicy {
    bindings: BTreeMap<BindingKey, PluginSecretBinding>,
}

impl PluginSecretPolicy {
    pub fn new(
        bindings: impl IntoIterator<Item = PluginSecretBinding>,
    ) -> Result<Self, PluginSecretError> {
        let mut resolved = BTreeMap::new();
        for binding in bindings {
            binding.validate()?;
            let key = BindingKey {
                tenant_id: binding.tenant_id.as_str().to_owned(),
                plugin_id: binding.plugin_id.as_str().to_owned(),
                installation_id: binding.installation_id.clone(),
                purpose: binding.purpose.clone(),
            };
            if resolved.insert(key, binding).is_some() {
                return Err(PluginSecretError::InvalidPolicy);
            }
        }
        Ok(Self { bindings: resolved })
    }

    pub fn deny_all() -> Self {
        Self::default()
    }

    fn binding_for(
        &self,
        admission: &PluginAdmission,
        purpose: &str,
        operation: PluginSecretOperationKind,
    ) -> Result<&PluginSecretBinding, PluginSecretError> {
        admission
            .authorize(&PluginPermission::SecretPurpose(purpose.to_owned()))
            .map_err(PluginSecretError::Host)?;
        let key = BindingKey {
            tenant_id: admission.tenant_id.as_str().to_owned(),
            plugin_id: admission.executable.plugin_id.as_str().to_owned(),
            installation_id: admission.installation_id.clone(),
            purpose: purpose.to_owned(),
        };
        let binding = self
            .bindings
            .get(&key)
            .ok_or(PluginSecretError::SecretBindingDenied)?;
        if binding.executable != admission.executable {
            return Err(PluginSecretError::SecretBindingDenied);
        }
        if !binding.allowed_operations.contains(&operation) || binding.handle.purpose() != purpose {
            return Err(PluginSecretError::SecretOperationDenied);
        }
        Ok(binding)
    }
}

pub struct PluginSecretService {
    policy: Arc<PluginSecretPolicy>,
    backend: Arc<dyn PluginSecretBackend>,
}

impl PluginSecretService {
    pub fn new(policy: Arc<PluginSecretPolicy>, backend: Arc<dyn PluginSecretBackend>) -> Self {
        Self { policy, backend }
    }

    pub fn sign(
        &self,
        admission: &PluginAdmission,
        purpose: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, PluginSecretError> {
        validate_input(payload.len())?;
        let binding =
            self.policy
                .binding_for(admission, purpose, PluginSecretOperationKind::Sign)?;
        let output = self
            .backend
            .sign(&binding.handle, payload)
            .map_err(|_| PluginSecretError::BackendFailure)?;
        validate_output(output)
    }

    pub fn decrypt(
        &self,
        admission: &PluginAdmission,
        purpose: &str,
        ciphertext: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, PluginSecretError> {
        validate_input(ciphertext.len())?;
        validate_input(aad.len())?;
        let binding =
            self.policy
                .binding_for(admission, purpose, PluginSecretOperationKind::Decrypt)?;
        let output = self
            .backend
            .decrypt(&binding.handle, ciphertext, aad)
            .map_err(|_| PluginSecretError::BackendFailure)?;
        validate_output(output)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSecretError {
    Host(PluginHostError),
    InvalidPolicy,
    SecretBindingDenied,
    SecretOperationDenied,
    InputTooLarge,
    OutputTooLarge,
    BackendFailure,
}

fn validate_input(size: usize) -> Result<(), PluginSecretError> {
    if size > MAX_SECRET_OPERATION_INPUT_BYTES {
        return Err(PluginSecretError::InputTooLarge);
    }
    Ok(())
}

fn validate_output(output: Vec<u8>) -> Result<Vec<u8>, PluginSecretError> {
    if output.len() > MAX_SECRET_OPERATION_OUTPUT_BYTES {
        return Err(PluginSecretError::OutputTooLarge);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use system_core::security::plugin::{
        EffectivePluginPermissions, IsolationEnforcement, IsolationPolicy, PermissionSet,
        PluginIsolationProfile,
    };
    use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef};

    use super::*;

    struct FixtureBackend;

    impl PluginSecretBackend for FixtureBackend {
        fn sign(&self, handle: &PluginSecretHandleRef, payload: &[u8]) -> Result<Vec<u8>, String> {
            let mut result = format!("{}@{}:", handle.handle_id(), handle.revision()).into_bytes();
            result.extend_from_slice(payload);
            Ok(result)
        }

        fn decrypt(
            &self,
            handle: &PluginSecretHandleRef,
            ciphertext: &[u8],
            aad: &[u8],
        ) -> Result<Vec<u8>, String> {
            let mut result = format!("{}@{}:", handle.handle_id(), handle.revision()).into_bytes();
            result.extend_from_slice(ciphertext);
            result.extend_from_slice(aad);
            Ok(result)
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

    fn admission(purpose: &str) -> PluginAdmission {
        let permission = PluginPermission::SecretPurpose(purpose.into());
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
                effective: PermissionSet::new([permission]).unwrap(),
                denied_requested: PermissionSet::empty(),
            },
        }
    }

    fn binding() -> PluginSecretBinding {
        PluginSecretBinding {
            tenant_id: TenantId::new("tenant-a").unwrap(),
            plugin_id: PluginId::new("official.fixture").unwrap(),
            installation_id: "install-a".into(),
            executable: executable(),
            purpose: "fixture.sign".into(),
            handle: PluginSecretHandleRef::new("kms/fixture/key-a", "fixture.sign", 7).unwrap(),
            allowed_operations: BTreeSet::from([PluginSecretOperationKind::Sign]),
        }
    }

    #[test]
    fn sign_uses_exact_revisioned_binding_without_exporting_secret_material() {
        let service = PluginSecretService::new(
            Arc::new(PluginSecretPolicy::new([binding()]).unwrap()),
            Arc::new(FixtureBackend),
        );
        let result = service
            .sign(&admission("fixture.sign"), "fixture.sign", b"payload")
            .unwrap();
        assert!(result.starts_with(b"kms/fixture/key-a@7:"));
        assert!(result.ends_with(b"payload"));
    }

    #[test]
    fn knowing_handle_or_having_a_different_secret_permission_is_not_authority() {
        let service = PluginSecretService::new(
            Arc::new(PluginSecretPolicy::new([binding()]).unwrap()),
            Arc::new(FixtureBackend),
        );
        assert_eq!(
            service.sign(&admission("other.purpose"), "fixture.sign", b"payload"),
            Err(PluginSecretError::Host(PluginHostError::PermissionDenied))
        );
    }

    #[test]
    fn binding_is_exactly_executable_pinned() {
        let service = PluginSecretService::new(
            Arc::new(PluginSecretPolicy::new([binding()]).unwrap()),
            Arc::new(FixtureBackend),
        );
        let mut wrong = admission("fixture.sign");
        wrong.executable.package_digest_sha256 = "12".repeat(32);
        assert_eq!(
            service.sign(&wrong, "fixture.sign", b"payload"),
            Err(PluginSecretError::SecretBindingDenied)
        );
    }

    #[test]
    fn operation_kind_is_independently_constrained() {
        let service = PluginSecretService::new(
            Arc::new(PluginSecretPolicy::new([binding()]).unwrap()),
            Arc::new(FixtureBackend),
        );
        assert_eq!(
            service.decrypt(
                &admission("fixture.sign"),
                "fixture.sign",
                b"ciphertext",
                b"aad"
            ),
            Err(PluginSecretError::SecretOperationDenied)
        );
    }
}

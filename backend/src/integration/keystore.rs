use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};

use super::types::{CapabilityId, IntegrationError, ProviderInstanceId, SecretPurpose, SecretRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretEnvironment {
    Fixture,
    Deployment,
}

/// Registration proves that a tenant-local Provider instance may *reference*
/// a secret. Resolution is stricter: it additionally requires an explicit
/// capability and purpose grant. Equality/ordering intentionally use only the
/// tenant + instance identity so persistence can validate a reference without
/// inheriting its resolution grants.
#[derive(Debug, Clone)]
pub struct SecretRegistrationScope {
    pub tenant_id: String,
    pub provider_instance_id: ProviderInstanceId,
    pub allowed_capabilities: BTreeSet<CapabilityId>,
    pub allowed_purposes: Vec<SecretPurpose>,
}

impl PartialEq for SecretRegistrationScope {
    fn eq(&self, other: &Self) -> bool {
        self.tenant_id == other.tenant_id && self.provider_instance_id == other.provider_instance_id
    }
}

impl Eq for SecretRegistrationScope {}

impl PartialOrd for SecretRegistrationScope {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SecretRegistrationScope {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.tenant_id, &self.provider_instance_id)
            .cmp(&(&other.tenant_id, &other.provider_instance_id))
    }
}

impl SecretRegistrationScope {
    pub fn new(
        tenant_id: impl Into<String>,
        provider_instance_id: ProviderInstanceId,
    ) -> Result<Self, IntegrationError> {
        let tenant_id = tenant_id.into();
        if tenant_id.trim().is_empty() {
            return Err(IntegrationError::BlankValue { kind: "tenant id" });
        }
        Ok(Self {
            tenant_id,
            provider_instance_id,
            allowed_capabilities: BTreeSet::new(),
            allowed_purposes: Vec::new(),
        })
    }

    pub fn with_grants(
        mut self,
        capabilities: impl IntoIterator<Item = CapabilityId>,
        purposes: impl IntoIterator<Item = SecretPurpose>,
    ) -> Self {
        self.allowed_capabilities = capabilities.into_iter().collect();
        self.allowed_purposes.clear();
        for purpose in purposes {
            if !self.allowed_purposes.contains(&purpose) {
                self.allowed_purposes.push(purpose);
            }
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretAccessScope {
    pub tenant_id: String,
    pub provider_instance_id: ProviderInstanceId,
    pub capability: CapabilityId,
    pub purpose: SecretPurpose,
    pub environment: SecretEnvironment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretMetadata {
    pub secret_ref: SecretRef,
    pub environment: SecretEnvironment,
    pub version: u64,
    pub rotated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretAccessAudit {
    pub secret_ref: SecretRef,
    pub tenant_id: String,
    pub provider_instance_id: ProviderInstanceId,
    pub capability: CapabilityId,
    pub purpose: SecretPurpose,
    pub environment: SecretEnvironment,
    pub granted: bool,
    pub occurred_at: String,
}

/// Controlled secret resolution for provider operations. Implementations may
/// use a deployment environment adapter or an encrypted local adapter, but
/// callers only receive a `Secret` after tenant, instance, environment,
/// capability, and purpose all pass the ACL check.
pub trait KeyStore: Send + Sync {
    fn resolve(
        &self,
        secret_ref: &SecretRef,
        scope: &SecretAccessScope,
    ) -> Result<Secret<Vec<u8>>, IntegrationError>;
    fn metadata(&self, secret_ref: &SecretRef) -> Option<SecretMetadata>;
    /// Allows persistence to prove that a submitted opaque URI names a
    /// KeyStore-managed reference for the intended environment without ever
    /// resolving or serializing credential material. Registration alone does
    /// not grant a capability or purpose permission to resolve the value.
    fn accepts_reference(
        &self,
        _secret_ref: &SecretRef,
        _registration: &SecretRegistrationScope,
        _environment: SecretEnvironment,
    ) -> bool {
        false
    }
    fn access_audit(&self) -> Vec<SecretAccessAudit>;
}

#[derive(Clone)]
struct SecretAcl {
    environment: SecretEnvironment,
    allowed_scopes: BTreeSet<SecretRegistrationScope>,
    metadata: SecretMetadata,
}

impl SecretAcl {
    fn allows(&self, scope: &SecretAccessScope) -> bool {
        if self.environment != scope.environment {
            return false;
        }
        let lookup = SecretRegistrationScope {
            tenant_id: scope.tenant_id.clone(),
            provider_instance_id: scope.provider_instance_id.clone(),
            allowed_capabilities: BTreeSet::new(),
            allowed_purposes: Vec::new(),
        };
        self.allowed_scopes.get(&lookup).is_some_and(|grant| {
            grant.allowed_capabilities.contains(&scope.capability)
                && grant.allowed_purposes.contains(&scope.purpose)
        })
    }

    fn allows_registration(
        &self,
        registration: &SecretRegistrationScope,
        environment: SecretEnvironment,
    ) -> bool {
        self.environment == environment && self.allowed_scopes.contains(registration)
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn access_event(
    secret_ref: &SecretRef,
    scope: &SecretAccessScope,
    granted: bool,
) -> SecretAccessAudit {
    SecretAccessAudit {
        secret_ref: secret_ref.clone(),
        tenant_id: scope.tenant_id.clone(),
        provider_instance_id: scope.provider_instance_id.clone(),
        capability: scope.capability.clone(),
        purpose: scope.purpose.clone(),
        environment: scope.environment,
        granted,
        occurred_at: now(),
    }
}

/// Fixture-only in-memory store. It is intentionally explicit so fixture
/// credentials cannot be used by deployment-scoped provider instances.
#[derive(Default)]
pub struct InMemoryKeyStore {
    entries: Mutex<BTreeMap<SecretRef, (SecretAcl, Secret<Vec<u8>>)>>,
    audit: Mutex<Vec<SecretAccessAudit>>,
}

impl InMemoryKeyStore {
    pub fn insert(
        &self,
        secret_ref: SecretRef,
        environment: SecretEnvironment,
        allowed_scopes: BTreeSet<SecretRegistrationScope>,
        value: Secret<Vec<u8>>,
    ) {
        let metadata = SecretMetadata {
            secret_ref: secret_ref.clone(),
            environment,
            version: 1,
            rotated_at: now(),
        };
        self.entries.lock().expect("key store lock").insert(
            secret_ref,
            (
                SecretAcl {
                    environment,
                    allowed_scopes,
                    metadata,
                },
                value,
            ),
        );
    }
}

impl KeyStore for InMemoryKeyStore {
    fn resolve(
        &self,
        secret_ref: &SecretRef,
        scope: &SecretAccessScope,
    ) -> Result<Secret<Vec<u8>>, IntegrationError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| IntegrationError::Persistence)?;
        let Some((acl, value)) = entries.get(secret_ref) else {
            self.audit
                .lock()
                .map_err(|_| IntegrationError::Persistence)?
                .push(access_event(secret_ref, scope, false));
            return Err(IntegrationError::SecretUnavailable);
        };
        let granted = acl.allows(scope);
        self.audit
            .lock()
            .map_err(|_| IntegrationError::Persistence)?
            .push(access_event(secret_ref, scope, granted));
        granted
            .then(|| Secret::new(value.expose_secret().clone()))
            .ok_or(IntegrationError::SecretAccessDenied)
    }

    fn metadata(&self, secret_ref: &SecretRef) -> Option<SecretMetadata> {
        self.entries
            .lock()
            .ok()?
            .get(secret_ref)
            .map(|(acl, _)| acl.metadata.clone())
    }

    fn accepts_reference(
        &self,
        secret_ref: &SecretRef,
        registration: &SecretRegistrationScope,
        environment: SecretEnvironment,
    ) -> bool {
        self.entries
            .lock()
            .map(|entries| {
                entries
                    .get(secret_ref)
                    .is_some_and(|(acl, _)| acl.allows_registration(registration, environment))
            })
            .unwrap_or(false)
    }

    fn access_audit(&self) -> Vec<SecretAccessAudit> {
        self.audit
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

struct EncryptedSecret {
    nonce: [u8; 12],
    ciphertext: Vec<u8>,
}

/// Local encrypted adapter for environments that cannot yet use an external
/// secret manager. The encryption key itself is supplied by deployment and is
/// never persisted with the ciphertext.
pub struct LocalEncryptedKeyStore {
    cipher: Aes256Gcm,
    entries: Mutex<BTreeMap<SecretRef, (SecretAcl, EncryptedSecret)>>,
    audit: Mutex<Vec<SecretAccessAudit>>,
}

impl LocalEncryptedKeyStore {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            cipher: Aes256Gcm::new_from_slice(&key).expect("fixed key length"),
            entries: Mutex::new(BTreeMap::new()),
            audit: Mutex::new(Vec::new()),
        }
    }

    pub fn rotate(
        &self,
        secret_ref: SecretRef,
        environment: SecretEnvironment,
        allowed_scopes: BTreeSet<SecretRegistrationScope>,
        value: Secret<Vec<u8>>,
    ) -> Result<SecretMetadata, IntegrationError> {
        let mut nonce = [0_u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce), value.expose_secret().as_ref())
            .map_err(|_| IntegrationError::Persistence)?;
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| IntegrationError::Persistence)?;
        let version = entries
            .get(&secret_ref)
            .map(|(acl, _)| acl.metadata.version + 1)
            .unwrap_or(1);
        let metadata = SecretMetadata {
            secret_ref: secret_ref.clone(),
            environment,
            version,
            rotated_at: now(),
        };
        entries.insert(
            secret_ref,
            (
                SecretAcl {
                    environment,
                    allowed_scopes,
                    metadata: metadata.clone(),
                },
                EncryptedSecret { nonce, ciphertext },
            ),
        );
        Ok(metadata)
    }
}

impl KeyStore for LocalEncryptedKeyStore {
    fn resolve(
        &self,
        secret_ref: &SecretRef,
        scope: &SecretAccessScope,
    ) -> Result<Secret<Vec<u8>>, IntegrationError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| IntegrationError::Persistence)?;
        let Some((acl, encrypted)) = entries.get(secret_ref) else {
            self.audit
                .lock()
                .map_err(|_| IntegrationError::Persistence)?
                .push(access_event(secret_ref, scope, false));
            return Err(IntegrationError::SecretUnavailable);
        };
        let granted = acl.allows(scope);
        self.audit
            .lock()
            .map_err(|_| IntegrationError::Persistence)?
            .push(access_event(secret_ref, scope, granted));
        if !granted {
            return Err(IntegrationError::SecretAccessDenied);
        }
        let value = self
            .cipher
            .decrypt(
                Nonce::from_slice(&encrypted.nonce),
                encrypted.ciphertext.as_ref(),
            )
            .map_err(|_| IntegrationError::SecretUnavailable)?;
        Ok(Secret::new(value))
    }

    fn metadata(&self, secret_ref: &SecretRef) -> Option<SecretMetadata> {
        self.entries
            .lock()
            .ok()?
            .get(secret_ref)
            .map(|(acl, _)| acl.metadata.clone())
    }

    fn accepts_reference(
        &self,
        secret_ref: &SecretRef,
        registration: &SecretRegistrationScope,
        environment: SecretEnvironment,
    ) -> bool {
        self.entries
            .lock()
            .map(|entries| {
                entries
                    .get(secret_ref)
                    .is_some_and(|(acl, _)| acl.allows_registration(registration, environment))
            })
            .unwrap_or(false)
    }

    fn access_audit(&self) -> Vec<SecretAccessAudit> {
        self.audit
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

/// Deployment-environment adapter. The map itself contains only reference to
/// environment-variable names and ACL metadata, never values. It is the only
/// location in the new Fabric allowed to call `std::env::var` for a provider
/// secret.
pub struct EnvironmentKeyStore {
    entries: BTreeMap<SecretRef, (SecretAcl, String)>,
    audit: Mutex<Vec<SecretAccessAudit>>,
}

impl EnvironmentKeyStore {
    pub fn new(
        entries: BTreeMap<
            SecretRef,
            (SecretEnvironment, BTreeSet<SecretRegistrationScope>, String),
        >,
    ) -> Self {
        let entries = entries
            .into_iter()
            .map(|(secret_ref, (environment, allowed_scopes, variable))| {
                let metadata = SecretMetadata {
                    secret_ref: secret_ref.clone(),
                    environment,
                    version: 1,
                    rotated_at: now(),
                };
                (
                    secret_ref,
                    (
                        SecretAcl {
                            environment,
                            allowed_scopes,
                            metadata,
                        },
                        variable,
                    ),
                )
            })
            .collect();
        Self {
            entries,
            audit: Mutex::new(Vec::new()),
        }
    }
}

impl KeyStore for EnvironmentKeyStore {
    fn resolve(
        &self,
        secret_ref: &SecretRef,
        scope: &SecretAccessScope,
    ) -> Result<Secret<Vec<u8>>, IntegrationError> {
        let Some((acl, variable)) = self.entries.get(secret_ref) else {
            self.audit
                .lock()
                .map_err(|_| IntegrationError::Persistence)?
                .push(access_event(secret_ref, scope, false));
            return Err(IntegrationError::SecretUnavailable);
        };
        let granted = acl.allows(scope);
        self.audit
            .lock()
            .map_err(|_| IntegrationError::Persistence)?
            .push(access_event(secret_ref, scope, granted));
        if !granted {
            return Err(IntegrationError::SecretAccessDenied);
        }
        std::env::var(variable)
            .map(|value| Secret::new(value.into_bytes()))
            .map_err(|_| IntegrationError::SecretUnavailable)
    }

    fn metadata(&self, secret_ref: &SecretRef) -> Option<SecretMetadata> {
        self.entries
            .get(secret_ref)
            .map(|(acl, _)| acl.metadata.clone())
    }

    fn accepts_reference(
        &self,
        secret_ref: &SecretRef,
        registration: &SecretRegistrationScope,
        environment: SecretEnvironment,
    ) -> bool {
        self.entries
            .get(secret_ref)
            .is_some_and(|(acl, _)| acl.allows_registration(registration, environment))
    }

    fn access_audit(&self) -> Vec<SecretAccessAudit> {
        self.audit
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureSecretRegistration {
    secret_ref: SecretRef,
    tenant_id: String,
    provider_instance_id: ProviderInstanceId,
    variable: String,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    purposes: Vec<SecretPurpose>,
}

/// Builds the one shared application KeyStore from registration metadata only.
/// `capabilities` and `purposes` are optional for configuration compatibility,
/// but an omitted/empty grant is registration-only and cannot resolve a secret.
/// Secret values themselves never appear in this JSON; the environment adapter
/// resolves the named variable only after the full ACL passes.
pub fn configured_fixture_key_store() -> Result<Arc<dyn KeyStore>, IntegrationError> {
    let registrations =
        std::env::var("INTEGRATION_FIXTURE_KEYSTORE_REGISTRATIONS").unwrap_or_default();
    Ok(Arc::new(fixture_environment_key_store_from_json(
        &registrations,
    )?))
}

fn fixture_environment_key_store_from_json(
    registrations: &str,
) -> Result<EnvironmentKeyStore, IntegrationError> {
    if registrations.trim().is_empty() {
        return Ok(EnvironmentKeyStore::new(BTreeMap::new()));
    }
    let parsed =
        serde_json::from_str::<Vec<FixtureSecretRegistration>>(registrations).map_err(|_| {
            IntegrationError::InvalidManifest(
                "fixture KeyStore registrations must be a JSON array of reference metadata".into(),
            )
        })?;
    let mut entries =
        BTreeMap::<SecretRef, (SecretEnvironment, BTreeSet<SecretRegistrationScope>, String)>::new(
        );
    for registration in parsed {
        if !registration
            .secret_ref
            .as_str()
            .starts_with("keystore://fixture/")
            || !valid_environment_variable_name(&registration.variable)
        {
            return Err(IntegrationError::InvalidManifest(
                "fixture KeyStore registrations require a fixture URI and environment variable name"
                    .into(),
            ));
        }
        let capabilities = registration
            .capabilities
            .into_iter()
            .map(CapabilityId::new)
            .collect::<Result<Vec<_>, _>>()?;
        let scope = SecretRegistrationScope::new(
            registration.tenant_id,
            registration.provider_instance_id,
        )?
        .with_grants(capabilities, registration.purposes);
        match entries.get_mut(&registration.secret_ref) {
            Some((_, scopes, variable)) => {
                if variable != &registration.variable {
                    return Err(IntegrationError::InvalidManifest(
                        "one fixture secret reference cannot name multiple environment variables"
                            .into(),
                    ));
                }
                if !scopes.insert(scope) {
                    return Err(IntegrationError::InvalidManifest(
                        "duplicate fixture KeyStore registration".into(),
                    ));
                }
            }
            None => {
                entries.insert(
                    registration.secret_ref,
                    (
                        SecretEnvironment::Fixture,
                        BTreeSet::from([scope]),
                        registration.variable,
                    ),
                );
            }
        }
    }
    Ok(EnvironmentKeyStore::new(entries))
}

fn valid_environment_variable_name(value: &str) -> bool {
    let mut characters = value.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use secrecy::ExposeSecret;

    use super::{
        EnvironmentKeyStore, InMemoryKeyStore, KeyStore, LocalEncryptedKeyStore, SecretAccessAudit,
        SecretAccessScope, SecretEnvironment, SecretMetadata, SecretRegistrationScope,
        fixture_environment_key_store_from_json,
    };
    use crate::integration::types::{
        CapabilityId, IntegrationError, ProviderInstanceId, SecretPurpose, SecretRef,
    };

    struct MetadataOnlyKeyStore;

    impl KeyStore for MetadataOnlyKeyStore {
        fn resolve(
            &self,
            _secret_ref: &SecretRef,
            _scope: &SecretAccessScope,
        ) -> Result<secrecy::Secret<Vec<u8>>, IntegrationError> {
            Err(IntegrationError::SecretUnavailable)
        }

        fn metadata(&self, secret_ref: &SecretRef) -> Option<SecretMetadata> {
            Some(SecretMetadata {
                secret_ref: secret_ref.clone(),
                environment: SecretEnvironment::Fixture,
                version: 1,
                rotated_at: "2026-09-02T00:00:00Z".into(),
            })
        }

        fn access_audit(&self) -> Vec<SecretAccessAudit> {
            Vec::new()
        }
    }

    fn registration() -> SecretRegistrationScope {
        SecretRegistrationScope::new("tenant-a", ProviderInstanceId::new("instance-a").unwrap())
            .unwrap()
            .with_grants(
                [CapabilityId::new("fixture.payment.charge").unwrap()],
                [SecretPurpose::ProviderAuthentication],
            )
    }

    fn scope(
        tenant: &str,
        instance: &str,
        capability: &str,
        purpose: SecretPurpose,
        environment: SecretEnvironment,
    ) -> SecretAccessScope {
        SecretAccessScope {
            tenant_id: tenant.into(),
            provider_instance_id: ProviderInstanceId::new(instance).unwrap(),
            capability: CapabilityId::new(capability).unwrap(),
            purpose,
            environment,
        }
    }

    #[test]
    fn fixture_secret_is_capability_purpose_environment_scoped_and_audited_without_value() {
        let store = InMemoryKeyStore::default();
        let secret_ref = SecretRef::new("keystore://fixture/payment-a").unwrap();
        store.insert(
            secret_ref.clone(),
            SecretEnvironment::Fixture,
            BTreeSet::from([registration()]),
            secrecy::Secret::new(b"fixture-secret".to_vec()),
        );
        assert_eq!(
            store
                .resolve(
                    &secret_ref,
                    &scope(
                        "tenant-a",
                        "instance-a",
                        "fixture.payment.charge",
                        SecretPurpose::ProviderAuthentication,
                        SecretEnvironment::Fixture,
                    )
                )
                .unwrap()
                .expose_secret(),
            b"fixture-secret"
        );
        for denied in [
            scope(
                "tenant-a",
                "instance-a",
                "fixture.payment.refund",
                SecretPurpose::ProviderAuthentication,
                SecretEnvironment::Fixture,
            ),
            scope(
                "tenant-a",
                "instance-a",
                "fixture.payment.charge",
                SecretPurpose::WebhookVerification,
                SecretEnvironment::Fixture,
            ),
            scope(
                "tenant-a",
                "instance-a",
                "fixture.payment.charge",
                SecretPurpose::ProviderAuthentication,
                SecretEnvironment::Deployment,
            ),
        ] {
            assert!(store.resolve(&secret_ref, &denied).is_err());
        }
        let audit_json = serde_json::to_string(&store.access_audit()).unwrap();
        assert!(!audit_json.contains("fixture-secret"));
        assert!(audit_json.contains("provider_authentication"));
        assert_eq!(store.metadata(&secret_ref).unwrap().version, 1);
    }

    #[test]
    fn local_encrypted_store_rotates_without_persisting_plaintext() {
        let store = LocalEncryptedKeyStore::new([7_u8; 32]);
        let secret_ref = SecretRef::new("keystore://deployment/payment-a").unwrap();
        let allowed = BTreeSet::from([registration()]);
        assert_eq!(
            store
                .rotate(
                    secret_ref.clone(),
                    SecretEnvironment::Fixture,
                    allowed.clone(),
                    secrecy::Secret::new(b"one".to_vec()),
                )
                .unwrap()
                .version,
            1
        );
        assert_eq!(
            store
                .rotate(
                    secret_ref.clone(),
                    SecretEnvironment::Fixture,
                    allowed,
                    secrecy::Secret::new(b"two".to_vec()),
                )
                .unwrap()
                .version,
            2
        );
        assert_eq!(
            store
                .resolve(
                    &secret_ref,
                    &scope(
                        "tenant-a",
                        "instance-a",
                        "fixture.payment.charge",
                        SecretPurpose::ProviderAuthentication,
                        SecretEnvironment::Fixture,
                    )
                )
                .unwrap()
                .expose_secret(),
            b"two"
        );
    }

    #[test]
    fn fixture_reference_registration_is_tenant_and_instance_scoped_without_implicit_resolution_grant()
     {
        let store: EnvironmentKeyStore = fixture_environment_key_store_from_json(
            r#"[{"secretRef":"keystore://fixture/payment-a","tenantId":"tenant-a","providerInstanceId":"instance-a","variable":"FIXTURE_PAYMENT_A"}]"#,
        )
        .unwrap();
        let secret_ref = SecretRef::new("keystore://fixture/payment-a").unwrap();
        let tenant_a = SecretRegistrationScope::new(
            "tenant-a",
            ProviderInstanceId::new("instance-a").unwrap(),
        )
        .unwrap();
        let tenant_b = SecretRegistrationScope::new(
            "tenant-b",
            ProviderInstanceId::new("instance-a").unwrap(),
        )
        .unwrap();

        assert!(store.accepts_reference(&secret_ref, &tenant_a, SecretEnvironment::Fixture));
        assert!(!store.accepts_reference(&secret_ref, &tenant_b, SecretEnvironment::Fixture));
        assert!(matches!(
            store.resolve(
                &secret_ref,
                &scope(
                    "tenant-a",
                    "instance-a",
                    "fixture.payment.charge",
                    SecretPurpose::ProviderAuthentication,
                    SecretEnvironment::Fixture,
                )
            ),
            Err(IntegrationError::SecretAccessDenied)
        ));
    }

    #[test]
    fn metadata_only_keystore_cannot_bypass_registration_scope() {
        let store = MetadataOnlyKeyStore;
        let secret_ref = SecretRef::new("keystore://fixture/payment-a").unwrap();
        let registration = SecretRegistrationScope::new(
            "tenant-a",
            ProviderInstanceId::new("instance-a").unwrap(),
        )
        .unwrap();

        assert!(!store.accepts_reference(&secret_ref, &registration, SecretEnvironment::Fixture));
    }
}

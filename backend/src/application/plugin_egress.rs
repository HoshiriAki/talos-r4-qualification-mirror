//! P6 shared plugin-egress lifecycle layered over the P5 network gateway.
//!
//! P5 remains the destination/SSRF/timeout authority. P6 adds an exact
//! installation/executable + grant scoped authority/budget state that survives
//! individual P5 dispatches. Plugin code supplies only a grant ID;
//! destination/path/protocol/secret/budget authority is resolved from a
//! host-owned immutable grant policy.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use system_core::security::plugin::PluginPermission;
use system_core::transport::interconnect::PluginExecutableRef;
use system_core::transport::network::EgressGrant;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::integration::egress::{
    DestinationPolicy, DestinationResolver, EgressEvidenceSink, EgressIdentity,
};
use crate::integration::egress_dispatch::{GovernedDispatchError, GovernedEgressDispatcher};
use crate::integration::transport::{ExternalRequest, ExternalResponse, TransportFailure};

use super::plugin_host::{PluginAdmission, PluginHostError};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ExecutableAuthorityKey {
    plugin_id: String,
    version: String,
    package_digest_sha256: String,
    manifest_digest_sha256: String,
    capability_contract_version: String,
    provider_instance_id: Option<String>,
    binding_revision: Option<String>,
}

impl From<&PluginExecutableRef> for ExecutableAuthorityKey {
    fn from(value: &PluginExecutableRef) -> Self {
        Self {
            plugin_id: value.plugin_id.as_str().to_owned(),
            version: value.version.as_str().to_owned(),
            package_digest_sha256: value.package_digest_sha256.clone(),
            manifest_digest_sha256: value.manifest_digest_sha256.clone(),
            capability_contract_version: value.capability_contract_version.as_str().to_owned(),
            provider_instance_id: value
                .provider_instance_id
                .as_ref()
                .map(|item| item.as_str().to_owned()),
            binding_revision: value
                .binding_revision
                .as_ref()
                .map(|item| item.as_str().to_owned()),
        }
    }
}

/// Host-owned P5 authority registry. A plugin permission carries only
/// `NetworkGrant(grant_id)`; it never supplies the full destination authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginEgressGrantPolicy {
    grants: BTreeMap<String, EgressGrant>,
}

impl PluginEgressGrantPolicy {
    pub(crate) fn new(
        grants: impl IntoIterator<Item = EgressGrant>,
    ) -> Result<Self, PluginEgressError> {
        let mut resolved = BTreeMap::new();
        for grant in grants {
            grant
                .validate()
                .map_err(|_| PluginEgressError::InvalidGrantPolicy)?;
            let grant_id = grant.grant_id.clone();
            if resolved.insert(grant_id, grant).is_some() {
                return Err(PluginEgressError::InvalidGrantPolicy);
            }
        }
        Ok(Self { grants: resolved })
    }

    pub(crate) fn deny_all() -> Self {
        Self {
            grants: BTreeMap::new(),
        }
    }

    fn resolve(&self, grant_id: &str) -> Result<EgressGrant, PluginEgressError> {
        self.grants
            .get(grant_id)
            .cloned()
            .ok_or(PluginEgressError::UnknownNetworkGrant)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SecretNetworkBinding {
    installation_id: String,
    executable: ExecutableAuthorityKey,
    grant_id: String,
    secret_purpose: String,
}

/// Host-owned approval matrix for dangerous secret + network combinations.
/// Possessing the two individual permissions is deliberately insufficient:
/// the exact pair must also be approved for the exact installation/executable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginSecretNetworkPolicy {
    bindings: BTreeSet<SecretNetworkBinding>,
}

impl PluginSecretNetworkPolicy {
    pub fn new(
        bindings: impl IntoIterator<Item = (String, PluginExecutableRef, String, String)>,
    ) -> Result<Self, PluginEgressError> {
        let mut resolved = BTreeSet::new();
        for (installation_id, executable, grant_id, secret_purpose) in bindings {
            if installation_id.trim().is_empty()
                || executable.validate().is_err()
                || PluginPermission::NetworkGrant(grant_id.clone())
                    .validate()
                    .is_err()
                || PluginPermission::SecretPurpose(secret_purpose.clone())
                    .validate()
                    .is_err()
            {
                return Err(PluginEgressError::InvalidSecretNetworkPolicy);
            }
            resolved.insert(SecretNetworkBinding {
                installation_id,
                executable: ExecutableAuthorityKey::from(&executable),
                grant_id,
                secret_purpose,
            });
        }
        Ok(Self { bindings: resolved })
    }

    pub fn deny_all() -> Self {
        Self::default()
    }

    fn allows(&self, admission: &PluginAdmission, grant_id: &str, secret_purpose: &str) -> bool {
        self.bindings.contains(&SecretNetworkBinding {
            installation_id: admission.installation_id.clone(),
            executable: ExecutableAuthorityKey::from(&admission.executable),
            grant_id: grant_id.to_owned(),
            secret_purpose: secret_purpose.to_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PluginBudgetKey {
    installation_id: String,
    grant_id: String,
}

struct PluginBudgetState {
    executable: ExecutableAuthorityKey,
    /// The complete P5 grant is part of the authority identity. Reusing the same
    /// grant id with a different host/path/protocol/secret/budget is rejected
    /// rather than silently widening an installation's permission reference.
    grant: EgressGrant,
    concurrency: Arc<Semaphore>,
    rate_window: Mutex<VecDeque<Instant>>,
}

impl PluginBudgetState {
    fn new(admission: &PluginAdmission, grant: &EgressGrant) -> Self {
        Self {
            executable: ExecutableAuthorityKey::from(&admission.executable),
            grant: grant.clone(),
            concurrency: Arc::new(Semaphore::new(grant.max_concurrency as usize)),
            rate_window: Mutex::new(VecDeque::new()),
        }
    }

    fn acquire(&self) -> Result<OwnedSemaphorePermit, PluginEgressError> {
        let permit = self
            .concurrency
            .clone()
            .try_acquire_owned()
            .map_err(|_| PluginEgressError::ConcurrencyExhausted)?;
        let now = Instant::now();
        let mut window = self
            .rate_window
            .lock()
            .map_err(|_| PluginEgressError::BudgetStateFailure)?;
        while window
            .front()
            .is_some_and(|started| now.duration_since(*started) >= Duration::from_secs(60))
        {
            window.pop_front();
        }
        if window.len() >= self.grant.rate_limit_per_minute as usize {
            drop(permit);
            return Err(PluginEgressError::RateExhausted);
        }
        window.push_back(now);
        Ok(permit)
    }
}

/// Long-lived state owned by the trusted host composition. There is
/// intentionally no public constructor, `Default`, reset or invalidate API.
pub struct PluginEgressBudgetRegistry {
    states: Mutex<HashMap<PluginBudgetKey, Arc<PluginBudgetState>>>,
}

impl PluginEgressBudgetRegistry {
    pub(crate) fn new() -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
        }
    }

    fn state_for(
        &self,
        admission: &PluginAdmission,
        grant: &EgressGrant,
    ) -> Result<Arc<PluginBudgetState>, PluginEgressError> {
        let key = PluginBudgetKey {
            installation_id: admission.installation_id.clone(),
            grant_id: grant.grant_id.clone(),
        };
        let mut states = self
            .states
            .lock()
            .map_err(|_| PluginEgressError::BudgetStateFailure)?;
        if let Some(existing) = states.get(&key) {
            if existing.executable != ExecutableAuthorityKey::from(&admission.executable) {
                return Err(PluginEgressError::ExecutableAuthorityIdentityChanged);
            }
            if existing.grant != *grant {
                return Err(PluginEgressError::GrantAuthorityIdentityChanged);
            }
            return Ok(existing.clone());
        }
        let state = Arc::new(PluginBudgetState::new(admission, grant));
        states.insert(key, state.clone());
        Ok(state)
    }
}

pub struct PluginEgressExecutor {
    budgets: Arc<PluginEgressBudgetRegistry>,
    grant_policy: Arc<PluginEgressGrantPolicy>,
    secret_network_policy: Arc<PluginSecretNetworkPolicy>,
    destination_policy: DestinationPolicy,
    evidence: Arc<dyn EgressEvidenceSink>,
    dispatcher: GovernedEgressDispatcher,
}

impl PluginEgressExecutor {
    pub(crate) fn new(
        budgets: Arc<PluginEgressBudgetRegistry>,
        grant_policy: Arc<PluginEgressGrantPolicy>,
        secret_network_policy: Arc<PluginSecretNetworkPolicy>,
        destination_policy: DestinationPolicy,
        evidence: Arc<dyn EgressEvidenceSink>,
    ) -> Self {
        Self {
            budgets,
            grant_policy,
            secret_network_policy,
            destination_policy,
            evidence,
            dispatcher: GovernedEgressDispatcher::system(),
        }
    }

    #[cfg(test)]
    fn new_with_resolver(
        budgets: Arc<PluginEgressBudgetRegistry>,
        grant_policy: Arc<PluginEgressGrantPolicy>,
        secret_network_policy: Arc<PluginSecretNetworkPolicy>,
        destination_policy: DestinationPolicy,
        evidence: Arc<dyn EgressEvidenceSink>,
        resolver: Arc<dyn DestinationResolver>,
    ) -> Self {
        Self {
            budgets,
            grant_policy,
            secret_network_policy,
            destination_policy,
            evidence,
            dispatcher: GovernedEgressDispatcher::with_resolver(resolver),
        }
    }

    pub async fn send(
        &self,
        admission: &PluginAdmission,
        grant_id: &str,
        secret_purpose: Option<&str>,
        request: ExternalRequest,
    ) -> Result<ExternalResponse, PluginEgressError> {
        // Check semantic authority before even revealing whether a host-owned
        // grant ID exists. The plugin never supplies the full EgressGrant.
        admission
            .authorize(&PluginPermission::NetworkGrant(grant_id.to_owned()))
            .map_err(PluginEgressError::Host)?;
        let grant = self.grant_policy.resolve(grant_id)?;
        if let Some(purpose) = secret_purpose {
            admission
                .authorize(&PluginPermission::SecretPurpose(purpose.to_owned()))
                .map_err(PluginEgressError::Host)?;
            if !self
                .secret_network_policy
                .allows(admission, grant_id, purpose)
            {
                return Err(PluginEgressError::SecretNetworkBindingDenied);
            }
        }

        // Hold the shared permit for the complete P5 attempt, including DNS.
        let state = self.budgets.state_for(admission, &grant)?;
        let _shared_permit = state.acquire()?;

        let identity = EgressIdentity {
            actor_ref: format!("plugin-installation:{}", admission.installation_id),
            capability: format!("plugin-egress:{}", grant.grant_id),
            secret_purpose: secret_purpose.map(str::to_owned),
        };
        self.dispatcher
            .send(
                grant,
                identity,
                self.destination_policy.clone(),
                self.evidence.clone(),
                request,
            )
            .await
            .map_err(|error| match error {
                GovernedDispatchError::InvalidGrant => PluginEgressError::InvalidGrant,
                GovernedDispatchError::Transport(failure) => PluginEgressError::Transport(failure),
            })
    }
}

#[derive(Debug)]
pub enum PluginEgressError {
    Host(PluginHostError),
    InvalidGrant,
    InvalidGrantPolicy,
    UnknownNetworkGrant,
    InvalidSecretNetworkPolicy,
    SecretNetworkBindingDenied,
    ExecutableAuthorityIdentityChanged,
    GrantAuthorityIdentityChanged,
    ConcurrencyExhausted,
    RateExhausted,
    BudgetStateFailure,
    Transport(TransportFailure),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::IpAddr;

    use async_trait::async_trait;
    use system_core::TenantId;
    use system_core::security::plugin::{
        EffectivePluginPermissions, IsolationEnforcement, IsolationPolicy, PermissionSet,
        PluginIsolationProfile,
    };
    use system_core::transport::interconnect::{ContractVersion, PluginId};
    use system_core::transport::network::{
        EgressMethodClass, EgressProtocol, EgressRedirectPolicy,
    };

    use super::*;
    use crate::integration::egress::NoopEgressEvidence;
    use crate::integration::transport::TransportTimeouts;

    struct PublicResolver;

    #[async_trait]
    impl DestinationResolver for PublicResolver {
        async fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>, TransportFailure> {
            Ok(vec!["8.8.8.8".parse().unwrap()])
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

    fn grant(rate: u32, concurrency: u32) -> EgressGrant {
        EgressGrant {
            grant_id: "provider.fixture.api".into(),
            destination_host: "api.example.test".into(),
            ports: vec![443],
            protocols: vec![EgressProtocol::Https],
            method_classes: vec![EgressMethodClass::Mutation],
            path_prefixes: vec!["/v1".into()],
            redirect_policy: EgressRedirectPolicy::Deny,
            max_connect_timeout_ms: 100,
            max_read_timeout_ms: 100,
            max_overall_timeout_ms: 200,
            max_response_bytes: 64,
            max_concurrency: concurrency,
            rate_limit_per_minute: rate,
            secret_purposes: vec!["fixture.secret".into()],
        }
    }

    fn grant_policy(grant: EgressGrant) -> Arc<PluginEgressGrantPolicy> {
        Arc::new(PluginEgressGrantPolicy::new([grant]).unwrap())
    }

    fn admission(permissions: PermissionSet) -> PluginAdmission {
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
        }
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

    #[test]
    fn budget_state_is_shared_by_installation_and_grant_and_cannot_be_reset_by_lookup() {
        let permissions = PermissionSet::new([
            PluginPermission::NetworkGrant("provider.fixture.api".into()),
            PluginPermission::SecretPurpose("fixture.secret".into()),
        ])
        .unwrap();
        let admission = admission(permissions);
        let registry = PluginEgressBudgetRegistry::new();
        let first = registry.state_for(&admission, &grant(2, 1)).unwrap();
        let second = registry.state_for(&admission, &grant(2, 1)).unwrap();
        assert!(Arc::ptr_eq(&first, &second));

        let held = first.acquire().unwrap();
        assert!(matches!(
            second.acquire(),
            Err(PluginEgressError::ConcurrencyExhausted)
        ));
        drop(held);
        let second_permit = second.acquire().unwrap();
        drop(second_permit);
        assert!(matches!(
            first.acquire(),
            Err(PluginEgressError::RateExhausted)
        ));
    }

    #[test]
    fn same_installation_cannot_reset_budget_through_a_different_executable() {
        let permissions = PermissionSet::new([PluginPermission::NetworkGrant(
            "provider.fixture.api".into(),
        )])
        .unwrap();
        let admission = admission(permissions);
        let registry = PluginEgressBudgetRegistry::new();
        registry.state_for(&admission, &grant(2, 1)).unwrap();

        let mut rebound = admission.clone();
        rebound.executable.package_digest_sha256 = "12".repeat(32);
        assert!(matches!(
            registry.state_for(&rebound, &grant(2, 1)),
            Err(PluginEgressError::ExecutableAuthorityIdentityChanged)
        ));
    }

    #[test]
    fn same_grant_id_cannot_silently_change_authority_or_budget_identity() {
        let admission = admission(
            PermissionSet::new([PluginPermission::NetworkGrant(
                "provider.fixture.api".into(),
            )])
            .unwrap(),
        );
        let registry = PluginEgressBudgetRegistry::new();
        registry.state_for(&admission, &grant(2, 1)).unwrap();

        assert!(matches!(
            registry.state_for(&admission, &grant(3, 1)),
            Err(PluginEgressError::GrantAuthorityIdentityChanged)
        ));

        let mut different_host = grant(2, 1);
        different_host.destination_host = "other.example.test".into();
        assert!(matches!(
            registry.state_for(&admission, &different_host),
            Err(PluginEgressError::GrantAuthorityIdentityChanged)
        ));
    }

    #[test]
    fn secret_network_pair_is_exactly_executable_pinned() {
        let permissions = PermissionSet::new([
            PluginPermission::NetworkGrant("provider.fixture.api".into()),
            PluginPermission::SecretPurpose("fixture.secret".into()),
        ])
        .unwrap();
        let admission = admission(permissions);
        let policy = PluginSecretNetworkPolicy::new([(
            "install-a".into(),
            executable(),
            "provider.fixture.api".into(),
            "fixture.secret".into(),
        )])
        .unwrap();
        assert!(policy.allows(&admission, "provider.fixture.api", "fixture.secret"));
        let mut rebound = admission.clone();
        rebound.executable.manifest_digest_sha256 = "12".repeat(32);
        assert!(!policy.allows(&rebound, "provider.fixture.api", "fixture.secret"));
    }

    #[tokio::test]
    async fn caller_supplies_only_grant_identity_not_destination_authority() {
        let permissions = PermissionSet::new([PluginPermission::NetworkGrant(
            "provider.fixture.api".into(),
        )])
        .unwrap();
        let admission = admission(permissions);
        let executor = PluginEgressExecutor::new_with_resolver(
            Arc::new(PluginEgressBudgetRegistry::new()),
            grant_policy(grant(2, 1)),
            Arc::new(PluginSecretNetworkPolicy::deny_all()),
            DestinationPolicy::public_only(),
            Arc::new(NoopEgressEvidence),
            Arc::new(PublicResolver),
        );
        let mut wrong_destination = request();
        wrong_destination.url = "https://other.example.test/v1/test".into();
        assert!(matches!(
            executor
                .send(&admission, "provider.fixture.api", None, wrong_destination,)
                .await,
            Err(PluginEgressError::Transport(_))
        ));
    }

    #[tokio::test]
    async fn permission_denial_happens_before_host_grant_resolution() {
        let executor = PluginEgressExecutor::new_with_resolver(
            Arc::new(PluginEgressBudgetRegistry::new()),
            Arc::new(PluginEgressGrantPolicy::deny_all()),
            Arc::new(PluginSecretNetworkPolicy::deny_all()),
            DestinationPolicy::public_only(),
            Arc::new(NoopEgressEvidence),
            Arc::new(PublicResolver),
        );
        let denied = admission(PermissionSet::empty());
        assert!(matches!(
            executor
                .send(&denied, "provider.fixture.api", None, request())
                .await,
            Err(PluginEgressError::Host(PluginHostError::PermissionDenied))
        ));
    }

    #[tokio::test]
    async fn secret_and_network_require_an_exact_host_owned_pair_approval() {
        let permissions = PermissionSet::new([
            PluginPermission::NetworkGrant("provider.fixture.api".into()),
            PluginPermission::SecretPurpose("fixture.secret".into()),
        ])
        .unwrap();
        let admission = admission(permissions);

        let denied = PluginEgressExecutor::new_with_resolver(
            Arc::new(PluginEgressBudgetRegistry::new()),
            grant_policy(grant(2, 1)),
            Arc::new(PluginSecretNetworkPolicy::deny_all()),
            DestinationPolicy::public_only(),
            Arc::new(NoopEgressEvidence),
            Arc::new(PublicResolver),
        );
        assert!(matches!(
            denied
                .send(
                    &admission,
                    "provider.fixture.api",
                    Some("fixture.secret"),
                    request(),
                )
                .await,
            Err(PluginEgressError::SecretNetworkBindingDenied)
        ));

        let wrong_pair = PluginSecretNetworkPolicy::new([(
            "install-a".into(),
            executable(),
            "provider.other.api".into(),
            "fixture.secret".into(),
        )])
        .unwrap();
        let denied_wrong_pair = PluginEgressExecutor::new_with_resolver(
            Arc::new(PluginEgressBudgetRegistry::new()),
            grant_policy(grant(2, 1)),
            Arc::new(wrong_pair),
            DestinationPolicy::public_only(),
            Arc::new(NoopEgressEvidence),
            Arc::new(PublicResolver),
        );
        assert!(matches!(
            denied_wrong_pair
                .send(
                    &admission,
                    "provider.fixture.api",
                    Some("fixture.secret"),
                    request(),
                )
                .await,
            Err(PluginEgressError::SecretNetworkBindingDenied)
        ));
    }
}

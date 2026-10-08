//! R4-P6 host-owned plugin runtime composition.
//!
//! Admission, Interconnect operations, P5 egress, purpose-bound secret use and
//! plugin-owned storage must be selected as one trusted composition. A caller
//! must not be able to obtain an admitted token from one policy set and pair it
//! with a weaker independently constructed execution facade. Every complete
//! runtime also owns one process-local execution binding shared by all facets.

use std::sync::Arc;

use system_core::ExecutionContext;
use system_core::transport::interconnect::PluginExecutableRef;

use crate::integration::egress::{DestinationPolicy, EgressEvidenceSink};

use super::interconnect::InterconnectFabric;
#[cfg(feature = "postgres")]
use super::interconnect::postgres::PostgresDurableDriver;
use super::plugin_admission::{PluginAdmissionPolicies, PluginAdmissionServiceError};
#[cfg(feature = "postgres")]
use super::plugin_background_execution::PostgresPluginBackgroundExecutionGate;
use super::plugin_egress::{
    PluginEgressBudgetRegistry, PluginEgressGrantPolicy, PluginSecretNetworkPolicy,
};
use super::plugin_execution_admission::{PluginExecutionAdmission, PluginExecutionRuntimeBinding};
#[cfg(feature = "postgres")]
use super::plugin_execution_admission_service::PostgresPluginAdmissionService;
#[cfg(feature = "sqlite")]
use super::plugin_execution_admission_service::SqlitePluginAdmissionService;
#[cfg(feature = "postgres")]
use super::plugin_execution_services::PostgresPluginExecutionStorageService;
#[cfg(feature = "sqlite")]
use super::plugin_execution_services::SqlitePluginExecutionStorageService;
use super::plugin_execution_services::{
    PluginExecutionEgressService, PluginExecutionSecretService, PluginExecutionStorageError,
};
use super::plugin_host_operations::PluginHostOperations;
use super::plugin_secret::{PluginSecretBackend, PluginSecretPolicy};
use super::plugin_storage::{PluginStorageEntry, PluginStoragePolicy};

/// Complete security/configuration input owned by the trusted composition root.
/// There is intentionally no `Default`: enabling a runtime requires every
/// authority source to be selected explicitly. Runtime constructors create the
/// shared egress budget registry and runtime binding themselves so callers
/// cannot swap/reset either authority lifecycle per request.
pub(crate) struct PluginRuntimeSecurityProfile {
    admission: PluginAdmissionPolicies,
    egress_grants: Arc<PluginEgressGrantPolicy>,
    secret_network: Arc<PluginSecretNetworkPolicy>,
    destination_policy: DestinationPolicy,
    egress_evidence: Arc<dyn EgressEvidenceSink>,
    secrets: Arc<PluginSecretPolicy>,
    secret_backend: Arc<dyn PluginSecretBackend>,
    storage: Arc<PluginStoragePolicy>,
}

impl PluginRuntimeSecurityProfile {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        admission: PluginAdmissionPolicies,
        egress_grants: Arc<PluginEgressGrantPolicy>,
        secret_network: Arc<PluginSecretNetworkPolicy>,
        destination_policy: DestinationPolicy,
        egress_evidence: Arc<dyn EgressEvidenceSink>,
        secrets: Arc<PluginSecretPolicy>,
        secret_backend: Arc<dyn PluginSecretBackend>,
        storage: Arc<PluginStoragePolicy>,
    ) -> Self {
        Self {
            admission,
            egress_grants,
            secret_network,
            destination_policy,
            egress_evidence,
            secrets,
            secret_backend,
            storage,
        }
    }
}

struct CommonRuntimeServices {
    operations: Arc<PluginHostOperations>,
    egress: Arc<PluginExecutionEgressService>,
    secrets: Arc<PluginExecutionSecretService>,
}

#[cfg(feature = "sqlite")]
struct SqliteRuntimeServices {
    admission: SqlitePluginAdmissionService,
    storage: SqlitePluginExecutionStorageService,
}

#[cfg(feature = "postgres")]
struct PostgresRuntimeServices {
    admission: PostgresPluginAdmissionService,
    storage: PostgresPluginExecutionStorageService,
    background: Arc<PostgresPluginBackgroundExecutionGate>,
}

enum PersistenceRuntimeServices {
    #[cfg(feature = "sqlite")]
    Sqlite(SqliteRuntimeServices),
    #[cfg(feature = "postgres")]
    Postgres(PostgresRuntimeServices),
}

/// Single application-facing runtime handle. Production/business code can use
/// this object, but cannot separately replace its admission, egress, secret,
/// storage or durable-lane policy after composition.
pub struct PluginRuntimeServices {
    common: CommonRuntimeServices,
    persistence: PersistenceRuntimeServices,
}

impl PluginRuntimeServices {
    fn common(
        operations: Arc<PluginHostOperations>,
        profile: &PluginRuntimeSecurityProfile,
        runtime_binding: &PluginExecutionRuntimeBinding,
    ) -> CommonRuntimeServices {
        let budgets = Arc::new(PluginEgressBudgetRegistry::new());
        CommonRuntimeServices {
            operations,
            egress: Arc::new(PluginExecutionEgressService::new(
                budgets,
                profile.egress_grants.clone(),
                profile.secret_network.clone(),
                profile.destination_policy.clone(),
                profile.egress_evidence.clone(),
                runtime_binding.clone(),
            )),
            secrets: Arc::new(PluginExecutionSecretService::new(
                profile.secrets.clone(),
                profile.secret_backend.clone(),
                runtime_binding.clone(),
            )),
        }
    }

    #[cfg(feature = "sqlite")]
    pub(crate) fn sqlite(
        pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
        fabric: Arc<InterconnectFabric>,
        profile: PluginRuntimeSecurityProfile,
    ) -> Self {
        let runtime_binding = PluginExecutionRuntimeBinding::new();
        let operations = Arc::new(PluginHostOperations::development(
            fabric,
            runtime_binding.clone(),
        ));
        let common = Self::common(operations, &profile, &runtime_binding);
        Self {
            common,
            persistence: PersistenceRuntimeServices::Sqlite(SqliteRuntimeServices {
                admission: SqlitePluginAdmissionService::new(
                    pool.clone(),
                    profile.admission,
                    runtime_binding.clone(),
                ),
                storage: SqlitePluginExecutionStorageService::new(
                    pool,
                    profile.storage,
                    runtime_binding,
                ),
            }),
        }
    }

    #[cfg(feature = "postgres")]
    pub(crate) fn postgres(
        pool: sqlx::PgPool,
        fabric: Arc<InterconnectFabric>,
        profile: PluginRuntimeSecurityProfile,
    ) -> Self {
        let runtime_binding = PluginExecutionRuntimeBinding::new();
        let operations = Arc::new(PluginHostOperations::postgres(
            fabric,
            PostgresDurableDriver::new(pool.clone()),
            runtime_binding.clone(),
        ));
        let common = Self::common(operations, &profile, &runtime_binding);
        let background = Arc::new(PostgresPluginBackgroundExecutionGate::new_bound(
            pool.clone(),
            profile.admission.clone(),
            runtime_binding.clone(),
        ));
        Self {
            common,
            persistence: PersistenceRuntimeServices::Postgres(PostgresRuntimeServices {
                admission: PostgresPluginAdmissionService::new(
                    pool.clone(),
                    profile.admission,
                    runtime_binding.clone(),
                ),
                storage: PostgresPluginExecutionStorageService::new(
                    pool,
                    profile.storage,
                    runtime_binding,
                ),
                background,
            }),
        }
    }

    pub async fn admit(
        &self,
        context: &ExecutionContext,
        executable: &PluginExecutableRef,
    ) -> Result<PluginExecutionAdmission, PluginAdmissionServiceError> {
        match &self.persistence {
            #[cfg(feature = "sqlite")]
            PersistenceRuntimeServices::Sqlite(runtime) => {
                runtime.admission.admit(context, executable)
            }
            #[cfg(feature = "postgres")]
            PersistenceRuntimeServices::Postgres(runtime) => {
                runtime.admission.admit(context, executable).await
            }
        }
    }

    pub fn operations(&self) -> Arc<PluginHostOperations> {
        self.common.operations.clone()
    }

    pub fn egress(&self) -> Arc<PluginExecutionEgressService> {
        self.common.egress.clone()
    }

    pub fn secrets(&self) -> Arc<PluginExecutionSecretService> {
        self.common.secrets.clone()
    }

    pub async fn storage_put(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        namespace: &str,
        key: &str,
        value: &[u8],
    ) -> Result<PluginStorageEntry, PluginExecutionStorageError> {
        match &self.persistence {
            #[cfg(feature = "sqlite")]
            PersistenceRuntimeServices::Sqlite(runtime) => runtime
                .storage
                .put(context, admission, namespace, key, value),
            #[cfg(feature = "postgres")]
            PersistenceRuntimeServices::Postgres(runtime) => {
                runtime
                    .storage
                    .put(context, admission, namespace, key, value)
                    .await
            }
        }
    }

    pub async fn storage_get(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        namespace: &str,
        key: &str,
    ) -> Result<Option<PluginStorageEntry>, PluginExecutionStorageError> {
        match &self.persistence {
            #[cfg(feature = "sqlite")]
            PersistenceRuntimeServices::Sqlite(runtime) => {
                runtime.storage.get(context, admission, namespace, key)
            }
            #[cfg(feature = "postgres")]
            PersistenceRuntimeServices::Postgres(runtime) => {
                runtime
                    .storage
                    .get(context, admission, namespace, key)
                    .await
            }
        }
    }

    #[cfg(feature = "postgres")]
    pub fn postgres_background_gate(&self) -> Option<Arc<PostgresPluginBackgroundExecutionGate>> {
        match &self.persistence {
            PersistenceRuntimeServices::Postgres(runtime) => Some(runtime.background.clone()),
            #[cfg(feature = "sqlite")]
            PersistenceRuntimeServices::Sqlite(_) => None,
        }
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::collections::{BTreeSet, HashMap};

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use rusqlite::params;
    use system_core::security::plugin::{
        PermissionSet, PluginPermission, PublisherId, Sha256Digest, VerificationEvidence,
    };
    use system_core::transport::interconnect::{ContractVersion, PluginId};
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
    };

    use super::*;
    use crate::application::plugin_admission::PluginPrincipalAuthorityResolver;
    use crate::application::plugin_execution_admission::PluginExecutionAdmissionError;
    use crate::application::plugin_host::TrustedPluginRuntimePolicy;
    use crate::application::plugin_runtime::{
        TrustedDangerousPluginCombinationPolicy, TrustedNativePublisherPolicy,
    };
    use crate::integration::egress::NoopEgressEvidence;
    use crate::registry::ModuleRegistry;

    struct AllowInvoke;

    impl PluginPrincipalAuthorityResolver for AllowInvoke {
        fn resolve(&self, _: &ExecutionContext) -> Result<PermissionSet, String> {
            PermissionSet::new([PluginPermission::CapabilityInvoke("orders.read".into())])
                .map_err(|error| error.to_string())
        }
    }

    struct DenySecretBackend;

    impl PluginSecretBackend for DenySecretBackend {
        fn sign(
            &self,
            _: &system_core::security::plugin_secret::PluginSecretHandleRef,
            _: &[u8],
        ) -> Result<Vec<u8>, String> {
            Err("disabled fixture backend".into())
        }

        fn decrypt(
            &self,
            _: &system_core::security::plugin_secret::PluginSecretHandleRef,
            _: &[u8],
            _: &[u8],
        ) -> Result<Vec<u8>, String> {
            Err("disabled fixture backend".into())
        }
    }

    fn pool() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::run_all_sqlite_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) VALUES ('tenant-a','A','a','active','test','now','now')",
            [],
        )
        .unwrap();
        let permission =
            PermissionSet::new([PluginPermission::CapabilityInvoke("orders.read".into())]).unwrap();
        let permission_json = serde_json::to_string(&permission).unwrap();
        let verification_json = serde_json::to_string(&VerificationEvidence::PublisherVerified {
            publisher_key_id: "talos.release.root".into(),
            signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
        })
        .unwrap();
        conn.execute(
            "INSERT INTO plugin_packages (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,capability_contract_version,compatibility_range,declared_capabilities_json,permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) VALUES ('official.fixture','talos.official','1.0.0',?1,?2,'1.0.0','>=1.0.0,<2.0.0',?3,?4,?5,'active','now','now')",
            params![
                "ab".repeat(32),
                "ef".repeat(32),
                serde_json::to_string(&BTreeSet::from(["orders.read"])).unwrap(),
                permission_json,
                verification_json,
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO plugin_installations (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at) VALUES ('install-a','tenant-a','official.fixture','1.0.0',?1,?2,'first_party_native',?3,?3,1,1,'active','now','now')",
            params!["ab".repeat(32), "ef".repeat(32), permission_json],
        )
        .unwrap();
        drop(conn);
        pool
    }

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
            DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new("corr-runtime-a").unwrap(),
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

    fn security_profile() -> PluginRuntimeSecurityProfile {
        let allow =
            PermissionSet::new([PluginPermission::CapabilityInvoke("orders.read".into())]).unwrap();
        PluginRuntimeSecurityProfile::new(
            PluginAdmissionPolicies::new(
                TrustedPluginRuntimePolicy {
                    host_contract_version: ContractVersion::new("1.0.0").unwrap(),
                    principal_authority: PermissionSet::empty(),
                    tenant_business_plane: allow.clone(),
                    platform_control_plane: PermissionSet::empty(),
                    simulation_plane: PermissionSet::empty(),
                    runtime_policy: allow,
                },
                Arc::new(AllowInvoke),
                TrustedNativePublisherPolicy::new([(
                    PublisherId::new("talos.official").unwrap(),
                    "talos.release.root".into(),
                )])
                .unwrap(),
                TrustedDangerousPluginCombinationPolicy::deny_all(),
            ),
            Arc::new(PluginEgressGrantPolicy::deny_all()),
            Arc::new(PluginSecretNetworkPolicy::deny_all()),
            DestinationPolicy::public_only(),
            Arc::new(NoopEgressEvidence),
            Arc::new(PluginSecretPolicy::deny_all()),
            Arc::new(DenySecretBackend),
            Arc::new(PluginStoragePolicy::deny_all()),
        )
    }

    fn fabric() -> Arc<InterconnectFabric> {
        Arc::new(InterconnectFabric::new(Arc::new(
            ModuleRegistry::new(HashMap::new()).unwrap(),
        )))
    }

    #[tokio::test]
    async fn complete_sqlite_runtime_produces_context_and_runtime_bound_admission() {
        let pool = pool();
        let runtime = PluginRuntimeServices::sqlite(pool.clone(), fabric(), security_profile());
        let other_runtime = PluginRuntimeServices::sqlite(pool, fabric(), security_profile());
        let ctx = context();
        let executable = executable();
        let admission = runtime.admit(&ctx, &executable).await.unwrap();
        assert_eq!(admission.executable(), &executable);
        assert_eq!(admission.installation_id(), "install-a");
        let first = runtime.operations();
        let second = runtime.operations();
        assert!(Arc::ptr_eq(&first, &second));

        let cross_runtime = other_runtime
            .storage_get(&ctx, &admission, "fixture", "key")
            .await;
        assert!(matches!(
            cross_runtime,
            Err(PluginExecutionStorageError::Admission(
                PluginExecutionAdmissionError::RuntimeMismatch
            ))
        ));
    }
}

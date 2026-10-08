//! R4-P6 persistence-bound production plugin admission.
//!
//! The public execution seam accepts trusted `ExecutionContext` plus the
//! immutable P3 `PluginExecutableRef`. Package/install records come from durable
//! host-owned storage, while Principal Authority is re-resolved for every
//! admission from a host-owned resolver rather than cached in long-lived policy.

use std::sync::Arc;

use system_core::ExecutionContext;
use system_core::security::plugin::PermissionSet;
use system_core::transport::interconnect::PluginExecutableRef;

use super::plugin_host::{PluginAdmission, TrustedPluginRuntimePolicy};
use super::plugin_runtime::{
    ProductionPluginHost, ProductionPluginHostError, TrustedDangerousPluginCombinationPolicy,
    TrustedNativePublisherPolicy,
};
use super::plugin_store::PluginStoreError;

/// Identity/role governance adapter owned by trusted host composition. The
/// returned set is invocation-local authority and is never supplied by plugin
/// package, message, installation, or ordinary caller input.
pub trait PluginPrincipalAuthorityResolver: Send + Sync {
    fn resolve(&self, context: &ExecutionContext) -> Result<PermissionSet, String>;
}

#[derive(Clone)]
pub struct PluginAdmissionPolicies {
    runtime_template: TrustedPluginRuntimePolicy,
    principal_authority_resolver: Arc<dyn PluginPrincipalAuthorityResolver>,
    native_publishers: TrustedNativePublisherPolicy,
    dangerous_combinations: TrustedDangerousPluginCombinationPolicy,
}

impl PluginAdmissionPolicies {
    pub fn new(
        mut runtime_template: TrustedPluginRuntimePolicy,
        principal_authority_resolver: Arc<dyn PluginPrincipalAuthorityResolver>,
        native_publishers: TrustedNativePublisherPolicy,
        dangerous_combinations: TrustedDangerousPluginCombinationPolicy,
    ) -> Self {
        // Principal authority is never a long-lived runtime-template fact. Even
        // if a caller accidentally pre-populates it, discard it here and force
        // per-admission resolution from trusted identity governance.
        runtime_template.principal_authority = PermissionSet::empty();
        Self {
            runtime_template,
            principal_authority_resolver,
            native_publishers,
            dangerous_combinations,
        }
    }

    fn runtime_for(
        &self,
        context: &ExecutionContext,
    ) -> Result<TrustedPluginRuntimePolicy, PluginAdmissionServiceError> {
        let principal_authority = self
            .principal_authority_resolver
            .resolve(context)
            .map_err(|_| PluginAdmissionServiceError::PrincipalAuthorityResolution)?;
        principal_authority
            .validate()
            .map_err(|_| PluginAdmissionServiceError::PrincipalAuthorityResolution)?;
        let mut runtime = self.runtime_template.clone();
        runtime.principal_authority = principal_authority;
        Ok(runtime)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginAdmissionServiceError {
    TenantScopeMissing,
    PrincipalAuthorityResolution,
    Store(PluginStoreError),
    Host(ProductionPluginHostError),
}

impl From<PluginStoreError> for PluginAdmissionServiceError {
    fn from(value: PluginStoreError) -> Self {
        Self::Store(value)
    }
}

impl From<ProductionPluginHostError> for PluginAdmissionServiceError {
    fn from(value: ProductionPluginHostError) -> Self {
        Self::Host(value)
    }
}

#[cfg(feature = "sqlite")]
pub struct SqlitePluginAdmissionService {
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
    policies: PluginAdmissionPolicies,
}

#[cfg(feature = "sqlite")]
impl SqlitePluginAdmissionService {
    pub fn new(
        pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
        policies: PluginAdmissionPolicies,
    ) -> Self {
        Self { pool, policies }
    }

    pub fn admit(
        &self,
        context: &ExecutionContext,
        executable: &PluginExecutableRef,
    ) -> Result<PluginAdmission, PluginAdmissionServiceError> {
        let tenant_id = context
            .data_scope()
            .tenant_id_opt()
            .ok_or(PluginAdmissionServiceError::TenantScopeMissing)?;
        let runtime = self.policies.runtime_for(context)?;
        let (package, installation) =
            super::plugin_store::resolve_sqlite_plugin_facts(&self.pool, tenant_id, executable)?;
        ProductionPluginHost::admit(
            context,
            executable,
            &package,
            &installation,
            &runtime,
            &self.policies.native_publishers,
            &self.policies.dangerous_combinations,
        )
        .map_err(Into::into)
    }
}

#[cfg(feature = "postgres")]
pub struct PostgresPluginAdmissionService {
    pool: sqlx::PgPool,
    policies: PluginAdmissionPolicies,
}

#[cfg(feature = "postgres")]
impl PostgresPluginAdmissionService {
    pub fn new(pool: sqlx::PgPool, policies: PluginAdmissionPolicies) -> Self {
        Self { pool, policies }
    }

    pub async fn admit(
        &self,
        context: &ExecutionContext,
        executable: &PluginExecutableRef,
    ) -> Result<PluginAdmission, PluginAdmissionServiceError> {
        let tenant_id = context
            .data_scope()
            .tenant_id_opt()
            .ok_or(PluginAdmissionServiceError::TenantScopeMissing)?;
        let runtime = self.policies.runtime_for(context)?;
        let (package, installation) =
            super::plugin_store::resolve_postgres_plugin_facts(&self.pool, tenant_id, executable)
                .await?;
        ProductionPluginHost::admit(
            context,
            executable,
            &package,
            &installation,
            &runtime,
            &self.policies.native_publishers,
            &self.policies.dangerous_combinations,
        )
        .map_err(Into::into)
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

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

    fn permission() -> PluginPermission {
        PluginPermission::CapabilityInvoke("orders.read".into())
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

    fn context(tenant: &str) -> ExecutionContext {
        let tenant = TenantId::new(tenant).unwrap();
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
            RequestId::new("corr-a").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[derive(Clone)]
    struct FixturePrincipalAuthority {
        permissions: PermissionSet,
        fail: bool,
    }

    impl PluginPrincipalAuthorityResolver for FixturePrincipalAuthority {
        fn resolve(&self, _: &ExecutionContext) -> Result<PermissionSet, String> {
            if self.fail {
                Err("fixture principal authority unavailable".into())
            } else {
                Ok(self.permissions.clone())
            }
        }
    }

    fn policies_with_resolver(
        resolver: Arc<dyn PluginPrincipalAuthorityResolver>,
    ) -> PluginAdmissionPolicies {
        let allow = PermissionSet::new([permission()]).unwrap();
        let accidentally_cached =
            PermissionSet::new([PluginPermission::NetworkGrant("must.not.persist".into())])
                .unwrap();
        PluginAdmissionPolicies::new(
            TrustedPluginRuntimePolicy {
                host_contract_version: ContractVersion::new("1.0.0").unwrap(),
                principal_authority: accidentally_cached,
                tenant_business_plane: allow.clone(),
                platform_control_plane: PermissionSet::empty(),
                simulation_plane: PermissionSet::empty(),
                runtime_policy: allow,
            },
            resolver,
            TrustedNativePublisherPolicy::new([(
                PublisherId::new("talos.official").unwrap(),
                "talos.release.root".into(),
            )])
            .unwrap(),
            TrustedDangerousPluginCombinationPolicy::deny_all(),
        )
    }

    fn policies() -> PluginAdmissionPolicies {
        policies_with_resolver(Arc::new(FixturePrincipalAuthority {
            permissions: PermissionSet::new([permission()]).unwrap(),
            fail: false,
        }))
    }

    fn fixture() -> Pool<SqliteConnectionManager> {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::run_all_sqlite_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
             VALUES ('tenant-a','Tenant A','tenant-a','active','test','now','now')",
            [],
        )
        .unwrap();
        let permissions = PermissionSet::new([permission()]).unwrap();
        let permission_json = serde_json::to_string(&permissions).unwrap();
        let verification_json = serde_json::to_string(&VerificationEvidence::PublisherVerified {
            publisher_key_id: "talos.release.root".into(),
            signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
        })
        .unwrap();
        conn.execute(
            "INSERT INTO plugin_packages \
             (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,\
              capability_contract_version,compatibility_range,declared_capabilities_json,\
              permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) \
             VALUES ('official.fixture','talos.official','1.0.0',?1,?2,'1.0.0','>=1.0.0,<2.0.0',?3,?4,?5,'active','now','now')",
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
            "INSERT INTO plugin_installations \
             (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,\
              manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,\
              grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at) \
             VALUES ('install-a','tenant-a','official.fixture','1.0.0',?1,?2,\
                     'first_party_native',?3,?3,1,1,'active','now','now')",
            params!["ab".repeat(32), "ef".repeat(32), permission_json],
        )
        .unwrap();
        drop(conn);
        pool
    }

    #[test]
    fn public_admission_surface_resolves_package_installation_and_principal_per_call() {
        let service = SqlitePluginAdmissionService::new(fixture(), policies());
        let admission = service.admit(&context("tenant-a"), &executable()).unwrap();
        assert_eq!(admission.installation_id, "install-a");
        assert_eq!(admission.authorize(&permission()), Ok(()));
        assert_eq!(
            admission.authorize(&PluginPermission::NetworkGrant("must.not.persist".into())),
            Err(super::super::plugin_host::PluginHostError::PermissionDenied)
        );
    }

    #[test]
    fn principal_authority_resolution_failure_fails_closed_before_admission() {
        let service = SqlitePluginAdmissionService::new(
            fixture(),
            policies_with_resolver(Arc::new(FixturePrincipalAuthority {
                permissions: PermissionSet::empty(),
                fail: true,
            })),
        );
        assert_eq!(
            service.admit(&context("tenant-a"), &executable()),
            Err(PluginAdmissionServiceError::PrincipalAuthorityResolution)
        );
    }

    #[test]
    fn caller_cannot_retarget_persisted_admission_by_changing_executable_identity() {
        let service = SqlitePluginAdmissionService::new(fixture(), policies());
        let mut wrong = executable();
        wrong.package_digest_sha256 = "11".repeat(32);
        assert_eq!(
            service.admit(&context("tenant-a"), &wrong),
            Err(PluginAdmissionServiceError::Store(
                PluginStoreError::NotInstalled
            ))
        );
    }
}

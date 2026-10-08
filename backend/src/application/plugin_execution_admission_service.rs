//! Persistence-bound plugin execution admission services.
//!
//! The lower-level resolver/host service is deliberately kept inside the
//! application crate. Runtime composition supplies an unforgeable process-local
//! binding, and callers receive only `PluginExecutionAdmission` stamped with
//! both that runtime identity and the exact originating `ExecutionContext`.

use system_core::ExecutionContext;
use system_core::transport::interconnect::PluginExecutableRef;

use super::plugin_admission::{PluginAdmissionPolicies, PluginAdmissionServiceError};
use super::plugin_execution_admission::{PluginExecutionAdmission, PluginExecutionRuntimeBinding};

#[cfg(feature = "sqlite")]
pub(crate) struct SqlitePluginAdmissionService {
    inner: super::plugin_admission::SqlitePluginAdmissionService,
    runtime_binding: PluginExecutionRuntimeBinding,
}

#[cfg(feature = "sqlite")]
impl SqlitePluginAdmissionService {
    pub(crate) fn new(
        pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
        policies: PluginAdmissionPolicies,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        Self {
            inner: super::plugin_admission::SqlitePluginAdmissionService::new(pool, policies),
            runtime_binding,
        }
    }

    pub(crate) fn admit(
        &self,
        context: &ExecutionContext,
        executable: &PluginExecutableRef,
    ) -> Result<PluginExecutionAdmission, PluginAdmissionServiceError> {
        let authority = self.inner.admit(context, executable)?;
        Ok(PluginExecutionAdmission::new(
            authority,
            context,
            self.runtime_binding.clone(),
        ))
    }
}

#[cfg(feature = "postgres")]
pub(crate) struct PostgresPluginAdmissionService {
    inner: super::plugin_admission::PostgresPluginAdmissionService,
    runtime_binding: PluginExecutionRuntimeBinding,
}

#[cfg(feature = "postgres")]
impl PostgresPluginAdmissionService {
    pub(crate) fn new(
        pool: sqlx::PgPool,
        policies: PluginAdmissionPolicies,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        Self {
            inner: super::plugin_admission::PostgresPluginAdmissionService::new(pool, policies),
            runtime_binding,
        }
    }

    pub(crate) async fn admit(
        &self,
        context: &ExecutionContext,
        executable: &PluginExecutableRef,
    ) -> Result<PluginExecutionAdmission, PluginAdmissionServiceError> {
        let authority = self.inner.admit(context, executable).await?;
        Ok(PluginExecutionAdmission::new(
            authority,
            context,
            self.runtime_binding.clone(),
        ))
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
    use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode,
        NoopHttpClient, RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
    };

    use super::*;
    use crate::application::{
        PluginPrincipalAuthorityResolver, TrustedDangerousPluginCombinationPolicy,
        TrustedNativePublisherPolicy, TrustedPluginRuntimePolicy,
    };

    struct AllowRead;

    impl PluginPrincipalAuthorityResolver for AllowRead {
        fn resolve(&self, _: &ExecutionContext) -> Result<PermissionSet, String> {
            PermissionSet::new([PluginPermission::CapabilityInvoke("orders.read".into())])
                .map_err(|error| error.to_string())
        }
    }

    fn context(actor: &str, correlation: &str) -> ExecutionContext {
        let tenant = TenantId::new("tenant-a").unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                actor,
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new(format!("membership-{actor}")).unwrap(),
                    tenant_id: tenant.clone(),
                    role: TenantRole::Staff,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new(correlation).unwrap(),
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

    fn fixture() -> Pool<SqliteConnectionManager> {
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
        let permissions =
            PermissionSet::new([PluginPermission::CapabilityInvoke("orders.read".into())]).unwrap();
        let permission_json = serde_json::to_string(&permissions).unwrap();
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

    fn policies() -> PluginAdmissionPolicies {
        let allow =
            PermissionSet::new([PluginPermission::CapabilityInvoke("orders.read".into())]).unwrap();
        PluginAdmissionPolicies::new(
            TrustedPluginRuntimePolicy {
                host_contract_version: ContractVersion::new("1.0.0").unwrap(),
                principal_authority: PermissionSet::empty(),
                tenant_business_plane: allow.clone(),
                platform_control_plane: PermissionSet::empty(),
                simulation_plane: PermissionSet::empty(),
                runtime_policy: allow,
            },
            Arc::new(AllowRead),
            TrustedNativePublisherPolicy::new([(
                PublisherId::new("talos.official").unwrap(),
                "talos.release.root".into(),
            )])
            .unwrap(),
            TrustedDangerousPluginCombinationPolicy::deny_all(),
        )
    }

    #[test]
    fn service_returns_context_and_runtime_bound_token_not_raw_authority() {
        let runtime = PluginExecutionRuntimeBinding::new();
        let service = SqlitePluginAdmissionService::new(fixture(), policies(), runtime.clone());
        let original = context("staff-a", "corr-a");
        let admission = service.admit(&original, &executable()).unwrap();
        assert_eq!(admission.installation_id(), "install-a");
        assert!(admission.require_context(&original).is_ok());
        assert!(admission.require_runtime(&runtime).is_ok());
        assert!(
            admission
                .require_context(&context("staff-b", "corr-a"))
                .is_err()
        );
        assert!(
            admission
                .require_context(&context("staff-a", "corr-b"))
                .is_err()
        );
        assert!(
            admission
                .require_runtime(&PluginExecutionRuntimeBinding::new())
                .is_err()
        );
    }
}

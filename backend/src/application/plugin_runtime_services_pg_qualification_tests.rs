#![cfg(feature = "postgres")]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::security::plugin::{
    PermissionSet, PluginPermission, PublisherId, Sha256Digest, VerificationEvidence,
};
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};
use system_core::transport::network::{
    EgressGrant, EgressMethodClass, EgressProtocol, EgressRedirectPolicy,
};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use super::interconnect::InterconnectFabric;
use super::plugin_admission::{PluginAdmissionPolicies, PluginPrincipalAuthorityResolver};
use super::plugin_egress::{PluginEgressError, PluginEgressGrantPolicy, PluginSecretNetworkPolicy};
use super::plugin_execution_services::PluginExecutionEgressError;
use super::plugin_host::TrustedPluginRuntimePolicy;
use super::plugin_runtime::{
    TrustedDangerousPluginCombinationPolicy, TrustedNativePublisherPolicy,
};
use super::plugin_runtime_services::{PluginRuntimeSecurityProfile, PluginRuntimeServices};
use super::plugin_secret::{PluginSecretBackend, PluginSecretPolicy};
use super::plugin_storage::PluginStoragePolicy;
use crate::integration::egress::{DestinationPolicy, NoopEgressEvidence};
use crate::integration::transport::{ExternalRequest, TransportErrorClass, TransportTimeouts};
use crate::registry::ModuleRegistry;

const QUALIFICATION_PUBLISHER: &str = "p9.qualification.publisher";
const QUALIFICATION_KEY: &str = "p9.sp05.qualification.key";
const GRANT_ID: &str = "p9.sp05.loopback";

struct LiveFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("p9_sp05_plugin_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&database_url).await?;
        admin
            .execute(format!("CREATE SCHEMA {schema}").as_str())
            .await?;
        drop(admin);

        let schema_for_pool = schema.clone();
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |connection, _| {
                let schema = schema_for_pool.clone();
                Box::pin(async move {
                    connection
                        .execute(format!("SET search_path TO {schema}").as_str())
                        .await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await?;

        sqlx::query(
            "CREATE TABLE tenants (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                slug TEXT UNIQUE NOT NULL,
                status TEXT NOT NULL,
                plan TEXT NOT NULL,
                settings TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO tenants
             (id,name,slug,status,plan,created_at,updated_at)
             VALUES ('p9-sp05-tenant','P9 SP05','p9-sp05','active','qualification','now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::raw_sql(include_str!(
            "../db/migrations/postgres/068_r4_plugin_host_security.sql"
        ))
        .execute(&pool)
        .await?;

        Ok(Self {
            database_url,
            schema,
            pool,
        })
    }

    async fn seed(&self) -> anyhow::Result<()> {
        let permissions = PermissionSet::new([PluginPermission::NetworkGrant(GRANT_ID.into())])?;
        let permission_json = serde_json::to_string(&permissions)?;
        let verification_json = serde_json::to_string(&VerificationEvidence::PublisherVerified {
            publisher_key_id: QUALIFICATION_KEY.into(),
            signature_digest_sha256: Sha256Digest::new("cd".repeat(32))?,
        })?;
        let package_digest = "ab".repeat(32);
        let manifest_digest = "ef".repeat(32);

        sqlx::query(
            "INSERT INTO plugin_packages
             (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,
              capability_contract_version,compatibility_range,declared_capabilities_json,
              permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at)
             VALUES ('p9.sp05.fixture',$1,'1.0.0',$2,$3,'1.0.0','>=1.0.0,<2.0.0',$4,$5,$6,'active','now','now')",
        )
        .bind(QUALIFICATION_PUBLISHER)
        .bind(&package_digest)
        .bind(&manifest_digest)
        .bind(serde_json::to_string(&BTreeSet::from(["network.probe"]))?)
        .bind(&permission_json)
        .bind(&verification_json)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "INSERT INTO plugin_installations
             (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,
              manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,
              grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at)
             VALUES ('p9-sp05-install','p9-sp05-tenant','p9.sp05.fixture','1.0.0',$1,$2,
                     'first_party_native',$3,$3,1,1,'active','now','now')",
        )
        .bind(&package_digest)
        .bind(&manifest_digest)
        .bind(&permission_json)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn cleanup(self) -> anyhow::Result<()> {
        self.pool.close().await;
        let mut admin = PgConnection::connect(&self.database_url).await?;
        admin
            .execute(format!("DROP SCHEMA {} CASCADE", self.schema).as_str())
            .await?;
        Ok(())
    }
}

struct AllowQualificationNetwork;

impl PluginPrincipalAuthorityResolver for AllowQualificationNetwork {
    fn resolve(&self, _: &ExecutionContext) -> Result<PermissionSet, String> {
        PermissionSet::new([PluginPermission::NetworkGrant(GRANT_ID.into())])
            .map_err(|error| error.to_string())
    }
}

struct DisabledSecretBackend;

impl PluginSecretBackend for DisabledSecretBackend {
    fn sign(
        &self,
        _: &system_core::security::plugin_secret::PluginSecretHandleRef,
        _: &[u8],
    ) -> Result<Vec<u8>, String> {
        Err("qualification secret backend disabled".into())
    }

    fn decrypt(
        &self,
        _: &system_core::security::plugin_secret::PluginSecretHandleRef,
        _: &[u8],
        _: &[u8],
    ) -> Result<Vec<u8>, String> {
        Err("qualification secret backend disabled".into())
    }
}

fn context() -> ExecutionContext {
    let tenant = TenantId::new("p9-sp05-tenant").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "p9-sp05-actor",
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new("p9-sp05-membership").unwrap(),
                tenant_id: tenant.clone(),
                role: TenantRole::Staff,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant.clone()),
        DataScope::production(tenant, Revision::new("p9-sp05-rev").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new("p9-sp05-correlation").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn executable() -> PluginExecutableRef {
    PluginExecutableRef {
        plugin_id: PluginId::new("p9.sp05.fixture").unwrap(),
        version: ContractVersion::new("1.0.0").unwrap(),
        package_digest_sha256: "ab".repeat(32),
        manifest_digest_sha256: "ef".repeat(32),
        capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
        provider_instance_id: None,
        binding_revision: None,
    }
}

fn security_profile() -> PluginRuntimeSecurityProfile {
    let permissions =
        PermissionSet::new([PluginPermission::NetworkGrant(GRANT_ID.into())]).unwrap();
    let grant = EgressGrant {
        grant_id: GRANT_ID.into(),
        destination_host: "127.0.0.1".into(),
        ports: vec![443],
        protocols: vec![EgressProtocol::Https],
        method_classes: vec![EgressMethodClass::Mutation],
        path_prefixes: vec!["/probe".into()],
        redirect_policy: EgressRedirectPolicy::Deny,
        max_connect_timeout_ms: 100,
        max_read_timeout_ms: 100,
        max_overall_timeout_ms: 200,
        max_response_bytes: 1024,
        max_concurrency: 1,
        rate_limit_per_minute: 2,
        secret_purposes: Vec::new(),
    };

    PluginRuntimeSecurityProfile::new(
        PluginAdmissionPolicies::new(
            TrustedPluginRuntimePolicy {
                host_contract_version: ContractVersion::new("1.0.0").unwrap(),
                principal_authority: PermissionSet::empty(),
                tenant_business_plane: permissions.clone(),
                platform_control_plane: PermissionSet::empty(),
                simulation_plane: PermissionSet::empty(),
                runtime_policy: permissions,
            },
            Arc::new(AllowQualificationNetwork),
            TrustedNativePublisherPolicy::new([(
                PublisherId::new(QUALIFICATION_PUBLISHER).unwrap(),
                QUALIFICATION_KEY.into(),
            )])
            .unwrap(),
            TrustedDangerousPluginCombinationPolicy::deny_all(),
        ),
        Arc::new(PluginEgressGrantPolicy::new([grant]).unwrap()),
        Arc::new(PluginSecretNetworkPolicy::deny_all()),
        DestinationPolicy::public_only(),
        Arc::new(NoopEgressEvidence),
        Arc::new(PluginSecretPolicy::deny_all()),
        Arc::new(DisabledSecretBackend),
        Arc::new(PluginStoragePolicy::deny_all()),
    )
}

fn fabric() -> Arc<InterconnectFabric> {
    Arc::new(InterconnectFabric::new(Arc::new(
        ModuleRegistry::new(HashMap::new()).unwrap(),
    )))
}

fn loopback_request() -> ExternalRequest {
    ExternalRequest {
        method: "POST".into(),
        url: "https://127.0.0.1/probe".into(),
        headers: BTreeMap::new(),
        body: Vec::new(),
        timeouts: TransportTimeouts {
            connect: Duration::from_millis(50),
            read: Duration::from_millis(50),
            overall: Duration::from_millis(100),
        },
        correlation_id: "caller-selected-must-be-overwritten".into(),
        cancellation: None,
    }
}

#[tokio::test]
#[ignore = "requires TALOS_TEST_POSTGRES_URL"]
async fn p9_sp05_qualification_runtime_admits_fixture_and_rejects_loopback_egress()
-> anyhow::Result<()> {
    let fixture = LiveFixture::create().await?;
    let result = async {
        fixture.seed().await?;

        let runtime = PluginRuntimeServices::postgres(
            fixture.pool.clone(),
            fabric(),
            security_profile(),
        );
        let ctx = context();
        let executable = executable();
        let admission = runtime
            .admit(&ctx, &executable)
            .await
            .map_err(|error| anyhow::anyhow!("plugin admission failed: {error:?}"))?;

        anyhow::ensure!(admission.installation_id() == "p9-sp05-install");
        anyhow::ensure!(admission.executable() == &executable);

        let egress = runtime
            .egress()
            .send(&ctx, &admission, GRANT_ID, None, loopback_request())
            .await;
        match egress {
            Err(PluginExecutionEgressError::Egress(PluginEgressError::Transport(failure)))
                if failure.class == TransportErrorClass::EgressDenied
                    && !failure.may_have_dispatched => {}
            other => anyhow::bail!(
                "qualification loopback egress must fail before dispatch, got {other:?}"
            ),
        }

        println!(
            "P9_SP05_PLUGIN_FIXTURE admitted=true production_runtime_unchanged=true loopback_egress=fail_closed"
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;

    fixture.cleanup().await?;
    result
}

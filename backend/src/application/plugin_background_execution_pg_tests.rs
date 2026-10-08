#![cfg(feature = "postgres")]

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::security::plugin::{
    CompatibilityRange, PermissionSet, PluginIsolationProfile, PluginLifecycle,
    PluginPackageIdentity, PluginPackageRecord, PluginPermission, PublisherId, Sha256Digest,
    VerificationEvidence,
};
use system_core::transport::interconnect::{
    ContractBinding, ContractRef, ContractVersion, CorrelationId, Extensions, LeaseOwner,
    MessageEnvelope, MessageId, MessageKind, PayloadRef, PluginExecutableRef, PluginId, SchemaRef,
    Subject,
};
use system_core::{
    ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient,
    RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
};

use super::TrustedPluginRuntimePolicy;
use super::interconnect::postgres::PostgresDurableDriver;
use super::interconnect_plugin::{PluginWorkAdmission, enqueue_postgres_plugin_work};
use super::plugin_admission::{PluginAdmissionPolicies, PluginPrincipalAuthorityResolver};
use super::plugin_background_execution::{
    PluginBackgroundExecutionError, PostgresPluginBackgroundExecutionGate,
};
use super::plugin_lifecycle::{PluginInstallationActivation, PostgresPluginLifecycleService};
use super::plugin_runtime::{
    TrustedDangerousPluginCombinationPolicy, TrustedNativePublisherPolicy,
};
use super::plugin_verification::{PluginPublisherSignatureVerifier, verify_publisher_package};

struct LiveFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_plugin_background_{}", uuid::Uuid::new_v4().simple());
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
            "CREATE TABLE tenants (id TEXT PRIMARY KEY,name TEXT NOT NULL,slug TEXT UNIQUE NOT NULL,status TEXT NOT NULL,plan TEXT NOT NULL,settings TEXT,created_at TEXT NOT NULL,updated_at TEXT NOT NULL)",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) VALUES ('tenant-a','Tenant A','tenant-a','active','test','now','now')",
        )
        .execute(&pool)
        .await?;
        sqlx::raw_sql(include_str!(
            "../db/migrations/postgres/067_r4_interconnect_fabric.sql"
        ))
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

    async fn cleanup(self) -> anyhow::Result<()> {
        self.pool.close().await;
        let mut admin = PgConnection::connect(&self.database_url).await?;
        admin
            .execute(format!("DROP SCHEMA {} CASCADE", self.schema).as_str())
            .await?;
        Ok(())
    }
}

struct AcceptVerifier;

impl PluginPublisherSignatureVerifier for AcceptVerifier {
    fn verify(&self, _: &PublisherId, _: &str, _: &[u8], _: &[u8]) -> Result<bool, String> {
        Ok(true)
    }
}

struct AllowBackground;

impl PluginPrincipalAuthorityResolver for AllowBackground {
    fn resolve(&self, _: &ExecutionContext) -> Result<PermissionSet, String> {
        PermissionSet::new([background_permission()]).map_err(|error| error.to_string())
    }
}

fn background_permission() -> PluginPermission {
    PluginPermission::BackgroundJob("jobs.fixture".into())
}

fn context() -> ExecutionContext {
    let tenant = TenantId::new("tenant-a").unwrap();
    ExecutionContext::new(
        ActorIdentity::with_authority(
            "worker-a",
            AuthorityContext::Tenant {
                membership_id: TenantMembershipId::new("membership-worker-a").unwrap(),
                tenant_id: tenant.clone(),
                role: TenantRole::Staff,
            },
        )
        .unwrap(),
        TenantScope::tenant(tenant.clone()),
        DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
        ExecutionMode::Normal,
        RequestId::new("corr-background-a").unwrap(),
        None,
        Arc::new(NoopHttpClient),
    )
    .unwrap()
}

fn candidate() -> (PluginPackageRecord, Vec<u8>, Vec<u8>) {
    let package_bytes = b"background-package-v1".to_vec();
    let manifest_bytes = b"background-manifest-v1".to_vec();
    let record = PluginPackageRecord {
        identity: PluginPackageIdentity {
            plugin_id: PluginId::new("official.background.fixture").unwrap(),
            publisher_id: PublisherId::new("talos.official").unwrap(),
            version: ContractVersion::new("1.0.0").unwrap(),
            package_digest_sha256: Sha256Digest::new(hex::encode(Sha256::digest(&package_bytes)))
                .unwrap(),
            manifest_digest_sha256: Sha256Digest::new(hex::encode(Sha256::digest(&manifest_bytes)))
                .unwrap(),
            capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
        },
        compatibility_range: CompatibilityRange::new(">=1.0.0,<2.0.0").unwrap(),
        declared_capabilities: BTreeSet::new(),
        permission_request: PermissionSet::new([background_permission()]).unwrap(),
        verification: VerificationEvidence::Unverified,
        lifecycle: PluginLifecycle::Staged,
    };
    (record, package_bytes, manifest_bytes)
}

fn policies() -> PluginAdmissionPolicies {
    let allow = PermissionSet::new([background_permission()]).unwrap();
    PluginAdmissionPolicies::new(
        TrustedPluginRuntimePolicy {
            host_contract_version: ContractVersion::new("1.0.0").unwrap(),
            principal_authority: PermissionSet::empty(),
            tenant_business_plane: allow.clone(),
            platform_control_plane: PermissionSet::empty(),
            simulation_plane: PermissionSet::empty(),
            runtime_policy: allow,
        },
        Arc::new(AllowBackground),
        TrustedNativePublisherPolicy::new([(
            PublisherId::new("talos.official").unwrap(),
            "talos.release.root".into(),
        )])
        .unwrap(),
        TrustedDangerousPluginCombinationPolicy::deny_all(),
    )
}

fn executable(identity: &PluginPackageIdentity) -> PluginExecutableRef {
    PluginExecutableRef {
        plugin_id: identity.plugin_id.clone(),
        version: identity.version.clone(),
        package_digest_sha256: identity.package_digest_sha256.as_str().to_owned(),
        manifest_digest_sha256: identity.manifest_digest_sha256.as_str().to_owned(),
        capability_contract_version: identity.capability_contract_version.clone(),
        provider_instance_id: None,
        binding_revision: None,
    }
}

fn envelope(created_at_ms: u64) -> MessageEnvelope {
    MessageEnvelope {
        id: MessageId::new("plugin-background-work-a").unwrap(),
        kind: MessageKind::Work,
        subject: Subject::new("jobs.fixture").unwrap(),
        contract: ContractBinding {
            contract: ContractRef::new("jobs.fixture").unwrap(),
            version: ContractVersion::new("1.0.0").unwrap(),
            schema: SchemaRef::new("jobs.fixture.v1").unwrap(),
        },
        correlation_id: CorrelationId::new("corr-background-a").unwrap(),
        causation_id: None,
        created_at_ms,
        deadline_ms: None,
        ordering_key: None,
        idempotency_key: None,
        payload: PayloadRef::Inline(Value::Object(serde_json::Map::new())),
        extensions: Extensions::empty(),
    }
}

#[tokio::test]
#[ignore = "requires TALOS_TEST_POSTGRES_URL"]
async fn live_pg18_plugin_background_execution_reauthorizes_current_claim_and_blocks_revocation()
-> anyhow::Result<()> {
    let fixture = LiveFixture::create().await?;
    let lifecycle = PostgresPluginLifecycleService::new(fixture.pool.clone());
    let (candidate, package_bytes, manifest_bytes) = candidate();
    let verified = verify_publisher_package(
        &candidate,
        &package_bytes,
        &manifest_bytes,
        "talos.release.root",
        b"signature",
        &AcceptVerifier,
    )
    .map_err(|error| anyhow::anyhow!("plugin verification failed: {error:?}"))?;
    let identity = lifecycle
        .stage_verified_package(verified)
        .await
        .map_err(|error| anyhow::anyhow!("plugin stage failed: {error:?}"))?;
    let executable = executable(&identity);
    let permission = PermissionSet::new([background_permission()]).unwrap();
    lifecycle
        .activate_installation(
            &identity,
            &PluginInstallationActivation {
                installation_id: "install-background-a".into(),
                tenant_id: TenantId::new("tenant-a").unwrap(),
                isolation_profile: PluginIsolationProfile::FirstPartyNative,
                installation_grant: permission.clone(),
                tenant_policy: permission,
                grant_revision: 1,
                tenant_policy_revision: 1,
            },
            None,
        )
        .await
        .map_err(|error| anyhow::anyhow!("plugin activation failed: {error:?}"))?;

    let base_now = u64::try_from(chrono::Utc::now().timestamp_millis())?;
    let ctx = context();
    let driver = PostgresDurableDriver::new(fixture.pool.clone());
    let subject = Subject::new("jobs.fixture").unwrap();
    enqueue_postgres_plugin_work(
        &driver,
        &ctx,
        PluginWorkAdmission {
            envelope: envelope(base_now),
            executable: executable.clone(),
            retry_budget: 2,
            available_at_ms: base_now,
        },
    )
    .await?;

    let first = driver
        .claim_work(
            &ctx,
            &subject,
            LeaseOwner::new("worker-a").unwrap(),
            base_now,
            3_600_000,
        )
        .await?
        .expect("first plugin work claim");
    driver
        .nack_work(&ctx, &first, true, "retry", base_now)
        .await?;
    let current = driver
        .claim_work(
            &ctx,
            &subject,
            LeaseOwner::new("worker-a").unwrap(),
            base_now.saturating_add(1),
            3_600_000,
        )
        .await?
        .expect("current plugin work claim");

    let gate = PostgresPluginBackgroundExecutionGate::new(fixture.pool.clone(), policies());
    assert_eq!(
        gate.reauthorize_claim(&ctx, &first).await,
        Err(PluginBackgroundExecutionError::StaleExpiredOrUnpinnedClaim)
    );
    let admitted = gate
        .reauthorize_claim(&ctx, &current)
        .await
        .map_err(|error| anyhow::anyhow!("current plugin claim was not reauthorized: {error:?}"))?;
    assert_eq!(admitted.executable(), &executable);

    // Expiry is independently enforced even before another worker reclaims the
    // row. Mutate only the durable deadline, mirror that exact deadline in the
    // presented claim, and prove PostgreSQL server time rejects it.
    let expired_deadline = base_now.saturating_sub(1);
    let changed = sqlx::query(
        "UPDATE interconnect_work SET lease_deadline_ms=$1 \
         WHERE work_id=$2 AND claim_generation=$3 AND lease_owner=$4 AND status='leased'",
    )
    .bind(i64::try_from(expired_deadline)?)
    .bind(current.work_id.as_str())
    .bind(i64::try_from(current.claim_generation)?)
    .bind(current.lease_owner.as_str())
    .execute(&fixture.pool)
    .await?
    .rows_affected();
    assert_eq!(changed, 1);
    let mut expired = current.clone();
    expired.lease_deadline_ms = expired_deadline;
    assert_eq!(
        gate.reauthorize_claim(&ctx, &expired).await,
        Err(PluginBackgroundExecutionError::StaleExpiredOrUnpinnedClaim)
    );

    // Restore the exact current lease so revocation is tested independently of
    // expiry. The next denial must come from fresh persisted P6 admission.
    let restored = sqlx::query(
        "UPDATE interconnect_work SET lease_deadline_ms=$1 \
         WHERE work_id=$2 AND claim_generation=$3 AND lease_owner=$4 AND status='leased'",
    )
    .bind(i64::try_from(current.lease_deadline_ms)?)
    .bind(current.work_id.as_str())
    .bind(i64::try_from(current.claim_generation)?)
    .bind(current.lease_owner.as_str())
    .execute(&fixture.pool)
    .await?
    .rows_affected();
    assert_eq!(restored, 1);

    lifecycle
        .revoke_installation(
            &TenantId::new("tenant-a").unwrap(),
            "install-background-a",
            &executable,
            "security-revoked",
        )
        .await
        .map_err(|error| anyhow::anyhow!("plugin revocation failed: {error:?}"))?;
    assert!(matches!(
        gate.reauthorize_claim(&ctx, &current).await,
        Err(PluginBackgroundExecutionError::Admission(_))
    ));

    fixture.cleanup().await?;
    Ok(())
}

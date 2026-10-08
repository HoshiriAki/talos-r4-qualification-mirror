#![cfg(feature = "postgres")]

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::TenantId;
use system_core::security::plugin::{
    CompatibilityRange, PermissionSet, PluginLifecycle, PluginPackageIdentity, PluginPackageRecord,
    PluginPermission, PublisherId, Sha256Digest, VerificationEvidence,
};
use system_core::security::plugin_upgrade::{
    PluginUpgradeMigrationStrategy, review_package_transition,
};
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};

use super::plugin_lifecycle::{
    PluginInstallationActivation, PluginLifecycleMutationError, PluginUpgradeAuthorization,
    PostgresPluginLifecycleService,
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
        let schema = format!("r4_plugin_lifecycle_{}", uuid::Uuid::new_v4().simple());
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
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
             VALUES ('tenant-a','Tenant A','tenant-a','active','test','now','now')",
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

fn read_permission() -> PluginPermission {
    PluginPermission::CapabilityInvoke("orders.read".into())
}

fn candidate_with_permissions(
    seed: &str,
    permissions: PermissionSet,
) -> (PluginPackageRecord, Vec<u8>, Vec<u8>) {
    let package_bytes = format!("package-{seed}").into_bytes();
    let manifest_bytes = format!("manifest-{seed}").into_bytes();
    let record = PluginPackageRecord {
        identity: PluginPackageIdentity {
            plugin_id: PluginId::new("official.fixture").unwrap(),
            publisher_id: PublisherId::new("talos.official").unwrap(),
            version: ContractVersion::new(format!("1.0.{seed}")).unwrap(),
            package_digest_sha256: Sha256Digest::new(hex::encode(Sha256::digest(&package_bytes)))
                .unwrap(),
            manifest_digest_sha256: Sha256Digest::new(hex::encode(Sha256::digest(&manifest_bytes)))
                .unwrap(),
            capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
        },
        compatibility_range: CompatibilityRange::new(">=1.0.0,<2.0.0").unwrap(),
        declared_capabilities: BTreeSet::from(["orders.read".into()]),
        permission_request: permissions,
        verification: VerificationEvidence::Unverified,
        lifecycle: PluginLifecycle::Staged,
    };
    (record, package_bytes, manifest_bytes)
}

fn candidate(seed: &str) -> (PluginPackageRecord, Vec<u8>, Vec<u8>) {
    candidate_with_permissions(seed, PermissionSet::new([read_permission()]).unwrap())
}

fn activation_with_permissions(
    id: &str,
    permissions: PermissionSet,
) -> PluginInstallationActivation {
    PluginInstallationActivation {
        installation_id: id.into(),
        tenant_id: TenantId::new("tenant-a").unwrap(),
        isolation_profile: system_core::security::plugin::PluginIsolationProfile::FirstPartyNative,
        installation_grant: permissions.clone(),
        tenant_policy: permissions,
        grant_revision: 1,
        tenant_policy_revision: 1,
    }
}

fn activation(id: &str) -> PluginInstallationActivation {
    activation_with_permissions(id, PermissionSet::new([read_permission()]).unwrap())
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

fn active_for_review(record: &PluginPackageRecord) -> PluginPackageRecord {
    let mut active = record.clone();
    active.lifecycle = PluginLifecycle::Active;
    active.verification = VerificationEvidence::PublisherVerified {
        publisher_key_id: "talos.release.root".into(),
        signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
    };
    active
}

async fn stage(
    service: &PostgresPluginLifecycleService,
    record: &PluginPackageRecord,
    package_bytes: &[u8],
    manifest_bytes: &[u8],
    signature: &[u8],
) -> anyhow::Result<PluginPackageIdentity> {
    let proof = verify_publisher_package(
        record,
        package_bytes,
        manifest_bytes,
        "talos.release.root",
        signature,
        &AcceptVerifier,
    )?;
    Ok(service.stage_verified_package(proof).await?)
}

#[tokio::test]
#[ignore = "requires TALOS_TEST_POSTGRES_URL"]
async fn live_pg18_plugin_initial_install_write_skew_is_serialized_by_tenant_plugin_lock()
-> anyhow::Result<()> {
    let fixture = LiveFixture::create().await?;
    let service = PostgresPluginLifecycleService::new(fixture.pool.clone());

    let (first, first_package, first_manifest) = candidate("1");
    let (second, second_package, second_manifest) = candidate("2");
    let first_identity = stage(
        &service,
        &first,
        &first_package,
        &first_manifest,
        b"signature-a",
    )
    .await?;
    let second_identity = stage(
        &service,
        &second,
        &second_package,
        &second_manifest,
        b"signature-b",
    )
    .await?;

    let first_activation = activation("install-a");
    let second_activation = activation("install-b");
    let (first_result, second_result) = tokio::join!(
        service.activate_installation(&first_identity, &first_activation, None),
        service.activate_installation(&second_identity, &second_activation, None),
    );

    let outcomes = [first_result, second_result];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| {
                matches!(
                    result,
                    Err(PluginLifecycleMutationError::UpgradeReviewRequired)
                )
            })
            .count(),
        1
    );
    let installation_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM plugin_installations WHERE tenant_id='tenant-a' AND plugin_id='official.fixture'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(installation_count, 1);

    fixture.cleanup().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires TALOS_TEST_POSTGRES_URL"]
async fn live_pg18_plugin_upgrade_records_exact_preserve_pins_decision() -> anyhow::Result<()> {
    let fixture = LiveFixture::create().await?;
    let service = PostgresPluginLifecycleService::new(fixture.pool.clone());

    let first_permissions = PermissionSet::new([read_permission()]).unwrap();
    let network = PluginPermission::NetworkGrant("provider.fixture.api".into());
    let next_permissions = PermissionSet::new([read_permission(), network.clone()]).unwrap();
    let (first, first_package, first_manifest) =
        candidate_with_permissions("1", first_permissions.clone());
    let (next, next_package, next_manifest) =
        candidate_with_permissions("2", next_permissions.clone());

    let first_identity = stage(
        &service,
        &first,
        &first_package,
        &first_manifest,
        b"signature-a",
    )
    .await?;
    service
        .activate_installation(
            &first_identity,
            &activation_with_permissions("install-a", first_permissions),
            None,
        )
        .await?;

    let next_identity = stage(
        &service,
        &next,
        &next_package,
        &next_manifest,
        b"signature-b",
    )
    .await?;
    let review = review_package_transition(
        &active_for_review(&first),
        &active_for_review(&next),
        PermissionSet::new([network]).unwrap(),
    )?;
    assert_eq!(
        review.migration_strategy(),
        PluginUpgradeMigrationStrategy::PreserveExactPins
    );
    let previous_executable = executable(&first_identity);
    service
        .activate_installation(
            &next_identity,
            &activation_with_permissions("install-b", next_permissions),
            Some(&PluginUpgradeAuthorization {
                previous_executable: previous_executable.clone(),
                review,
            }),
        )
        .await?;

    let row = sqlx::query(
        "SELECT tenant_id,previous_executable_json,candidate_identity_json,permission_diff_json,\
                approved_additions_json,migration_strategy \
         FROM plugin_upgrade_transitions WHERE candidate_installation_id='install-b'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    use sqlx::Row;
    let tenant_id: String = row.try_get("tenant_id")?;
    let previous_json: String = row.try_get("previous_executable_json")?;
    let candidate_json: String = row.try_get("candidate_identity_json")?;
    let permission_diff_json: String = row.try_get("permission_diff_json")?;
    let approved_additions_json: String = row.try_get("approved_additions_json")?;
    let strategy: String = row.try_get("migration_strategy")?;
    assert_eq!(tenant_id, "tenant-a");
    assert_eq!(strategy, "preserve_exact_pins");
    assert_eq!(
        serde_json::from_str::<PluginExecutableRef>(&previous_json)?,
        previous_executable
    );
    assert_eq!(
        serde_json::from_str::<PluginPackageIdentity>(&candidate_json)?,
        next_identity
    );
    let diff: system_core::security::plugin_upgrade::PluginPermissionDiff =
        serde_json::from_str(&permission_diff_json)?;
    assert!(diff.added.contains(&PluginPermission::NetworkGrant(
        "provider.fixture.api".into()
    )));
    let approved: PermissionSet = serde_json::from_str(&approved_additions_json)?;
    assert!(approved.contains(&PluginPermission::NetworkGrant(
        "provider.fixture.api".into()
    )));

    let old_state: String = sqlx::query_scalar(
        "SELECT lifecycle_state FROM plugin_installations WHERE installation_id='install-a'",
    )
    .fetch_one(&fixture.pool)
    .await?;
    assert_eq!(old_state, "active");

    fixture.cleanup().await?;
    Ok(())
}

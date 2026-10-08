#![cfg(feature = "postgres")]

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::TenantId;
use system_core::security::plugin::{
    PermissionSet, PluginPermission, Sha256Digest, VerificationEvidence,
};
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};

use super::plugin_store::{PluginStoreError, resolve_postgres_plugin_facts};

struct LiveFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_plugin_host_{}", uuid::Uuid::new_v4().simple());
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
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) VALUES ('tenant-a','A','a','active','test','now','now')",
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

fn executable_with_version(version: &str, package: &str, manifest: &str) -> PluginExecutableRef {
    PluginExecutableRef {
        plugin_id: PluginId::new("official.fixture").unwrap(),
        version: ContractVersion::new(version).unwrap(),
        package_digest_sha256: package.to_owned(),
        manifest_digest_sha256: manifest.to_owned(),
        capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
        provider_instance_id: None,
        binding_revision: None,
    }
}

fn executable(package: &str, manifest: &str) -> PluginExecutableRef {
    executable_with_version("1.0.0", package, manifest)
}

async fn seed(fixture: &LiveFixture) -> anyhow::Result<()> {
    let permissions = PermissionSet::new([
        PluginPermission::CapabilityInvoke("orders.read".into()),
        PluginPermission::NetworkGrant("provider.fixture.api".into()),
    ])?;
    let permission_json = serde_json::to_string(&permissions)?;
    let verification_json = serde_json::to_string(&VerificationEvidence::PublisherVerified {
        publisher_key_id: "talos.release.root".into(),
        signature_digest_sha256: Sha256Digest::new("cd".repeat(32))?,
    })?;
    let package_digest = "ab".repeat(32);
    let manifest_digest = "ef".repeat(32);

    sqlx::query(
        "INSERT INTO plugin_packages \
         (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,\
          capability_contract_version,compatibility_range,declared_capabilities_json,\
          permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) \
         VALUES ($1,'talos.official','1.0.0',$2,$3,'1.0.0','>=1.0.0,<2.0.0',$4,$5,$6,'active','now','now')",
    )
    .bind("official.fixture")
    .bind(&package_digest)
    .bind(&manifest_digest)
    .bind(serde_json::to_string(&vec!["orders.read"])? )
    .bind(&permission_json)
    .bind(&verification_json)
    .execute(&fixture.pool)
    .await?;

    sqlx::query(
        "INSERT INTO plugin_installations \
         (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,\
          manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,\
          grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at) \
         VALUES ('install-a','tenant-a','official.fixture','1.0.0',$1,$2,\
                 'first_party_native',$3,$3,1,1,'active','now','now')",
    )
    .bind(&package_digest)
    .bind(&manifest_digest)
    .bind(&permission_json)
    .execute(&fixture.pool)
    .await?;

    let next_package_digest = "12".repeat(32);
    let next_manifest_digest = "34".repeat(32);
    let next_verification_json = serde_json::to_string(&VerificationEvidence::PublisherVerified {
        publisher_key_id: "talos.release.root".into(),
        signature_digest_sha256: Sha256Digest::new("56".repeat(32))?,
    })?;

    sqlx::query(
        "INSERT INTO plugin_packages \
         (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,\
          capability_contract_version,compatibility_range,declared_capabilities_json,\
          permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) \
         VALUES ($1,'talos.official','1.1.0',$2,$3,'1.0.0','>=1.0.0,<2.0.0',$4,$5,$6,'active','now','now')",
    )
    .bind("official.fixture")
    .bind(&next_package_digest)
    .bind(&next_manifest_digest)
    .bind(serde_json::to_string(&vec!["orders.read"])? )
    .bind(&permission_json)
    .bind(&next_verification_json)
    .execute(&fixture.pool)
    .await?;

    sqlx::query(
        "INSERT INTO plugin_installations \
         (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,\
          manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,\
          grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at) \
         VALUES ('install-b','tenant-a','official.fixture','1.1.0',$1,$2,\
                 'first_party_native',$3,$3,2,2,'active','now','now')",
    )
    .bind(&next_package_digest)
    .bind(&next_manifest_digest)
    .bind(&permission_json)
    .execute(&fixture.pool)
    .await?;
    Ok(())
}

fn store_error(error: PluginStoreError) -> anyhow::Error {
    anyhow::anyhow!("plugin store evidence failed: {error:?}")
}

#[tokio::test]
#[ignore = "requires TALOS_TEST_POSTGRES_URL"]
async fn live_pg18_plugin_installation_resolution_is_exactly_executable_pinned()
-> anyhow::Result<()> {
    let fixture = LiveFixture::create().await?;
    seed(&fixture).await?;

    let tenant = TenantId::new("tenant-a").map_err(anyhow::Error::msg)?;
    let exact = executable(&"ab".repeat(32), &"ef".repeat(32));
    let (package, installation) = resolve_postgres_plugin_facts(&fixture.pool, &tenant, &exact)
        .await
        .map_err(store_error)?;
    assert!(package.identity.matches_executable(&exact));
    assert_eq!(installation.installation_id, "install-a");

    let upgraded = executable_with_version("1.1.0", &"12".repeat(32), &"34".repeat(32));
    let (upgraded_package, upgraded_installation) =
        resolve_postgres_plugin_facts(&fixture.pool, &tenant, &upgraded)
            .await
            .map_err(store_error)?;
    assert!(upgraded_package.identity.matches_executable(&upgraded));
    assert_eq!(upgraded_installation.installation_id, "install-b");

    // A later exact installation must coexist with the earlier one. Durable P3
    // work pinned to the older executable is re-authorized against that exact
    // historical installation instead of silently retargeting to the upgrade.
    let (_, old_after_upgrade) = resolve_postgres_plugin_facts(&fixture.pool, &tenant, &exact)
        .await
        .map_err(store_error)?;
    assert_eq!(old_after_upgrade.installation_id, "install-a");

    let wrong_package = executable(&"11".repeat(32), &"ef".repeat(32));
    assert_eq!(
        resolve_postgres_plugin_facts(&fixture.pool, &tenant, &wrong_package).await,
        Err(PluginStoreError::NotInstalled)
    );
    let wrong_manifest = executable(&"ab".repeat(32), &"22".repeat(32));
    assert_eq!(
        resolve_postgres_plugin_facts(&fixture.pool, &tenant, &wrong_manifest).await,
        Err(PluginStoreError::NotInstalled)
    );
    let mut wrong_contract = exact.clone();
    wrong_contract.capability_contract_version = ContractVersion::new("2.0.0").unwrap();
    assert_eq!(
        resolve_postgres_plugin_facts(&fixture.pool, &tenant, &wrong_contract).await,
        Err(PluginStoreError::NotInstalled)
    );

    fixture.cleanup().await?;
    Ok(())
}

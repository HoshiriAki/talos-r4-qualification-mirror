#![cfg(feature = "postgres")]

use std::sync::Arc;

use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection, Executor};
use system_core::TenantId;
use system_core::security::plugin::{
    EffectivePluginPermissions, IsolationEnforcement, IsolationPolicy, PermissionSet,
    PluginIsolationProfile, PluginPermission,
};
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};

use super::plugin_host::PluginAdmission;
use super::plugin_storage::{
    PluginStorageError, PluginStoragePolicy, PluginStorageQuota, PostgresPluginStorageService,
};

struct LiveFixture {
    database_url: String,
    schema: String,
    pool: PgPool,
}

impl LiveFixture {
    async fn create() -> anyhow::Result<Self> {
        let database_url = std::env::var("TALOS_TEST_POSTGRES_URL")?;
        let schema = format!("r4_plugin_storage_{}", uuid::Uuid::new_v4().simple());
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
        for (id, name, slug) in [
            ("tenant-a", "Tenant A", "tenant-a"),
            ("tenant-b", "Tenant B", "tenant-b"),
        ] {
            sqlx::query(
                "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
                 VALUES ($1,$2,$3,'active','test','now','now')",
            )
            .bind(id)
            .bind(name)
            .bind(slug)
            .execute(&pool)
            .await?;
        }
        sqlx::raw_sql(include_str!(
            "../db/migrations/postgres/068_r4_plugin_host_security.sql"
        ))
        .execute(&pool)
        .await?;
        seed(&pool, "tenant-a", "install-a").await?;
        seed(&pool, "tenant-b", "install-b").await?;

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

async fn seed(pool: &PgPool, tenant: &str, installation: &str) -> anyhow::Result<()> {
    let package_digest = "ab".repeat(32);
    let manifest_digest = "ef".repeat(32);
    sqlx::query(
        "INSERT INTO plugin_packages \
         (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,\
          capability_contract_version,compatibility_range,declared_capabilities_json,\
          permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) \
         VALUES ('official.fixture','talos.official','1.0.0',$1,$2,'1.0.0','>=1.0.0,<2.0.0','[]','[]',\
                 '{\"kind\":\"digest_verified\"}','active','now','now') \
         ON CONFLICT DO NOTHING",
    )
    .bind(&package_digest)
    .bind(&manifest_digest)
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT INTO plugin_installations \
         (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,\
          manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,\
          grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at) \
         VALUES ($1,$2,'official.fixture','1.0.0',$3,$4,'first_party_native','[]','[]',1,1,'active','now','now')",
    )
    .bind(installation)
    .bind(tenant)
    .bind(&package_digest)
    .bind(&manifest_digest)
    .execute(pool)
    .await?;
    Ok(())
}

fn admission(tenant: &str, installation: &str) -> PluginAdmission {
    let permission = PluginPermission::PluginStorage("cache".into());
    PluginAdmission {
        installation_id: installation.into(),
        tenant_id: TenantId::new(tenant).unwrap(),
        executable: PluginExecutableRef {
            plugin_id: PluginId::new("official.fixture").unwrap(),
            version: ContractVersion::new("1.0.0").unwrap(),
            package_digest_sha256: "ab".repeat(32),
            manifest_digest_sha256: "ef".repeat(32),
            capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
            provider_instance_id: None,
            binding_revision: None,
        },
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

fn quota() -> PluginStorageQuota {
    PluginStorageQuota {
        max_entry_bytes: 16,
        max_total_bytes: 24,
        max_entries: 2,
    }
}

fn service(pool: &PgPool, quota: PluginStorageQuota) -> PostgresPluginStorageService {
    let policy = PluginStoragePolicy::new([
        (
            TenantId::new("tenant-a").unwrap(),
            PluginId::new("official.fixture").unwrap(),
            quota,
        ),
        (
            TenantId::new("tenant-b").unwrap(),
            PluginId::new("official.fixture").unwrap(),
            quota,
        ),
    ])
    .unwrap();
    PostgresPluginStorageService::new(pool.clone(), Arc::new(policy))
}

#[tokio::test]
#[ignore = "requires TALOS_TEST_POSTGRES_URL"]
async fn live_pg18_plugin_storage_is_scoped_quota_bounded_and_revocation_aware()
-> anyhow::Result<()> {
    let fixture = LiveFixture::create().await?;
    let storage = service(&fixture.pool, quota());
    let owner = admission("tenant-a", "install-a");
    let other_tenant = admission("tenant-b", "install-b");

    let first = storage.put(&owner, "cache", "item", b"alpha").await?;
    assert_eq!(first.revision, 1);
    storage.put(&other_tenant, "cache", "item", b"beta").await?;
    assert_eq!(
        storage.get(&owner, "cache", "item").await?.unwrap().value,
        b"alpha".to_vec()
    );
    assert_eq!(
        storage
            .get(&other_tenant, "cache", "item")
            .await?
            .unwrap()
            .value,
        b"beta".to_vec()
    );

    storage.put(&owner, "cache", "two", &[2; 12]).await?;
    assert_eq!(
        storage.put(&owner, "cache", "three", b"x").await,
        Err(PluginStorageError::EntryCountQuotaExceeded)
    );
    assert_eq!(
        storage.put(&owner, "cache", "item", &[3; 16]).await,
        Err(PluginStorageError::TotalQuotaExceeded)
    );

    // A second runtime may not silently widen the persisted owner quota.
    let wider = service(
        &fixture.pool,
        PluginStorageQuota {
            max_entry_bytes: 32,
            max_total_bytes: 64,
            max_entries: 8,
        },
    );
    assert_eq!(
        wider.get(&owner, "cache", "item").await,
        Err(PluginStorageError::QuotaAuthorityChanged)
    );

    sqlx::query(
        "UPDATE plugin_installations SET lifecycle_state='revoked',revoked_reason_code='test_revoke' \
         WHERE installation_id='install-a'",
    )
    .execute(&fixture.pool)
    .await?;
    assert_eq!(
        storage.get(&owner, "cache", "item").await,
        Err(PluginStorageError::InstallationInactiveOrMissing)
    );

    fixture.cleanup().await?;
    Ok(())
}

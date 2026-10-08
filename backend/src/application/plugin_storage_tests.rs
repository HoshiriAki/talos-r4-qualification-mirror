#![cfg(feature = "sqlite")]

use std::sync::Arc;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use system_core::TenantId;
use system_core::security::plugin::{
    EffectivePluginPermissions, IsolationEnforcement, IsolationPolicy, PermissionSet,
    PluginIsolationProfile, PluginPermission,
};
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};

use super::plugin_host::PluginAdmission;
use super::plugin_storage::{
    PluginStorageError, PluginStoragePolicy, PluginStorageQuota, SqlitePluginStorageService,
};

fn permissions(namespace: &str) -> PermissionSet {
    PermissionSet::new([PluginPermission::PluginStorage(namespace.into())]).unwrap()
}

fn admission(tenant: &str, plugin: &str, installation: &str, namespace: &str) -> PluginAdmission {
    PluginAdmission {
        installation_id: installation.into(),
        tenant_id: TenantId::new(tenant).unwrap(),
        executable: PluginExecutableRef {
            plugin_id: PluginId::new(plugin).unwrap(),
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
            effective: permissions(namespace),
            denied_requested: PermissionSet::empty(),
        },
    }
}

fn seed_package_and_installation(
    conn: &rusqlite::Connection,
    tenant: &str,
    plugin: &str,
    installation: &str,
) {
    let package_digest = "ab".repeat(32);
    let manifest_digest = "ef".repeat(32);
    conn.execute(
        "INSERT OR IGNORE INTO plugin_packages \
         (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,\
          capability_contract_version,compatibility_range,declared_capabilities_json,\
          permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) \
         VALUES (?1,'talos.official','1.0.0',?2,?3,'1.0.0','>=1.0.0,<2.0.0','[]','[]',\
                 '{\"kind\":\"digest_verified\"}','active','now','now')",
        params![plugin, package_digest, manifest_digest],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO plugin_installations \
         (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,\
          manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,\
          grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at) \
         VALUES (?1,?2,?3,'1.0.0',?4,?5,'first_party_native','[]','[]',1,1,'active','now','now')",
        params![
            installation,
            tenant,
            plugin,
            package_digest,
            manifest_digest
        ],
    )
    .unwrap();
}

fn fixture() -> Pool<SqliteConnectionManager> {
    let manager = SqliteConnectionManager::memory();
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    let conn = pool.get().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    crate::db::run_all_sqlite_migrations(&conn).unwrap();
    for (id, name, slug) in [
        ("tenant-a", "Tenant A", "tenant-a"),
        ("tenant-b", "Tenant B", "tenant-b"),
    ] {
        conn.execute(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
             VALUES (?1,?2,?3,'active','test','now','now')",
            params![id, name, slug],
        )
        .unwrap();
    }
    seed_package_and_installation(&conn, "tenant-a", "official.fixture", "install-a");
    seed_package_and_installation(&conn, "tenant-b", "official.fixture", "install-b");
    seed_package_and_installation(&conn, "tenant-a", "official.other", "install-other");
    drop(conn);
    pool
}

fn quota() -> PluginStorageQuota {
    PluginStorageQuota {
        max_entry_bytes: 16,
        max_total_bytes: 24,
        max_entries: 2,
    }
}

fn policy(entries: &[(&str, &str, PluginStorageQuota)]) -> Arc<PluginStoragePolicy> {
    Arc::new(
        PluginStoragePolicy::new(entries.iter().map(|(tenant, plugin, quota)| {
            (
                TenantId::new(*tenant).unwrap(),
                PluginId::new(*plugin).unwrap(),
                *quota,
            )
        }))
        .unwrap(),
    )
}

#[test]
fn sqlite_storage_is_exactly_tenant_and_plugin_scoped() {
    let pool = fixture();
    let service = SqlitePluginStorageService::new(
        pool.clone(),
        policy(&[
            ("tenant-a", "official.fixture", quota()),
            ("tenant-b", "official.fixture", quota()),
            ("tenant-a", "official.other", quota()),
        ]),
    );
    let owner = admission("tenant-a", "official.fixture", "install-a", "cache");
    let other_tenant = admission("tenant-b", "official.fixture", "install-b", "cache");
    let other_plugin = admission("tenant-a", "official.other", "install-other", "cache");

    let written = service.put(&owner, "cache", "item", b"alpha").unwrap();
    assert_eq!(written.revision, 1);
    assert_eq!(
        service.get(&owner, "cache", "item").unwrap().unwrap().value,
        b"alpha".to_vec()
    );
    assert_eq!(
        service.get(&other_tenant, "cache", "item").unwrap_err(),
        PluginStorageError::QuotaPolicyMissing
    );
    assert_eq!(
        service.get(&other_plugin, "cache", "item").unwrap_err(),
        PluginStorageError::QuotaPolicyMissing
    );
}

#[test]
fn sqlite_storage_namespace_and_quota_fail_closed() {
    let pool = fixture();
    let owner = admission("tenant-a", "official.fixture", "install-a", "cache");
    let service = SqlitePluginStorageService::new(
        pool,
        policy(&[(
            "tenant-a",
            "official.fixture",
            PluginStorageQuota {
                max_entry_bytes: 16,
                max_total_bytes: 32,
                max_entries: 2,
            },
        )]),
    );

    assert!(matches!(
        service.put(&owner, "other", "item", b"x"),
        Err(PluginStorageError::Host(_))
    ));
    assert_eq!(
        service.put(&owner, "cache", "large", &[0; 17]),
        Err(PluginStorageError::EntryTooLarge)
    );
    service.put(&owner, "cache", "one", &[1; 12]).unwrap();
    service.put(&owner, "cache", "two", &[2; 12]).unwrap();
    assert_eq!(
        service.put(&owner, "cache", "three", b"x"),
        Err(PluginStorageError::EntryCountQuotaExceeded)
    );
}

#[test]
fn sqlite_storage_quota_is_durable_authority_not_runtime_self_selection() {
    let pool = fixture();
    let owner = admission("tenant-a", "official.fixture", "install-a", "cache");
    let narrow = SqlitePluginStorageService::new(
        pool.clone(),
        policy(&[("tenant-a", "official.fixture", quota())]),
    );
    narrow.put(&owner, "cache", "one", b"alpha").unwrap();

    let widened = SqlitePluginStorageService::new(
        pool,
        policy(&[(
            "tenant-a",
            "official.fixture",
            PluginStorageQuota {
                max_entry_bytes: 64,
                max_total_bytes: 1024,
                max_entries: 100,
            },
        )]),
    );
    assert_eq!(
        widened.put(&owner, "cache", "two", b"beta"),
        Err(PluginStorageError::QuotaAuthorityChanged)
    );
}

#[test]
fn sqlite_storage_rechecks_exact_installation_and_package_lifecycle() {
    let pool = fixture();
    let owner = admission("tenant-a", "official.fixture", "install-a", "cache");
    let service = SqlitePluginStorageService::new(
        pool.clone(),
        policy(&[("tenant-a", "official.fixture", quota())]),
    );
    service.put(&owner, "cache", "item", b"alpha").unwrap();

    {
        let conn = pool.get().unwrap();
        conn.execute(
            "UPDATE plugin_installations SET lifecycle_state='revoked',revoked_reason_code='test_revoke' \
             WHERE installation_id='install-a'",
            [],
        )
        .unwrap();
    }
    assert_eq!(
        service.get(&owner, "cache", "item"),
        Err(PluginStorageError::InstallationInactiveOrMissing)
    );
}

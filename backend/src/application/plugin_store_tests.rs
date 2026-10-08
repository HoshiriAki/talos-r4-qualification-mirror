#![cfg(feature = "sqlite")]

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use system_core::TenantId;
use system_core::security::plugin::{
    PermissionSet, PluginPermission, Sha256Digest, VerificationEvidence,
};
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};

use super::plugin_store::{PluginStoreError, resolve_sqlite_plugin_facts};

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

#[test]
fn sqlite_plugin_resolution_is_tenant_and_exact_executable_pinned() {
    let manager = SqliteConnectionManager::memory();
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    {
        let conn = pool.get().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::run_all_sqlite_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
             VALUES ('tenant-a','Tenant A','tenant-a','active','test','now','now')",
            [],
        )
        .unwrap();

        let permissions = PermissionSet::new([
            PluginPermission::CapabilityInvoke("orders.read".into()),
            PluginPermission::NetworkGrant("provider.fixture.api".into()),
        ])
        .unwrap();
        let permission_json = serde_json::to_string(&permissions).unwrap();
        let verification_json = serde_json::to_string(&VerificationEvidence::PublisherVerified {
            publisher_key_id: "talos.release.root".into(),
            signature_digest_sha256: Sha256Digest::new("cd".repeat(32)).unwrap(),
        })
        .unwrap();
        let package_digest = "ab".repeat(32);
        let manifest_digest = "ef".repeat(32);

        conn.execute(
            "INSERT INTO plugin_packages \
             (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,\
              capability_contract_version,compatibility_range,declared_capabilities_json,\
              permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) \
             VALUES (?1,'talos.official','1.0.0',?2,?3,'1.0.0','>=1.0.0,<2.0.0',?4,?5,?6,'active','now','now')",
            params![
                "official.fixture",
                package_digest,
                manifest_digest,
                serde_json::to_string(&vec!["orders.read"]).unwrap(),
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
            params![package_digest, manifest_digest, permission_json],
        )
        .unwrap();

        let next_package_digest = "12".repeat(32);
        let next_manifest_digest = "34".repeat(32);
        conn.execute(
            "INSERT INTO plugin_packages \
             (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,\
              capability_contract_version,compatibility_range,declared_capabilities_json,\
              permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) \
             VALUES (?1,'talos.official','1.1.0',?2,?3,'1.0.0','>=1.0.0,<2.0.0',?4,?5,?6,'active','now','now')",
            params![
                "official.fixture",
                next_package_digest,
                next_manifest_digest,
                serde_json::to_string(&vec!["orders.read"]).unwrap(),
                serde_json::to_string(&permissions).unwrap(),
                serde_json::to_string(&VerificationEvidence::PublisherVerified {
                    publisher_key_id: "talos.release.root".into(),
                    signature_digest_sha256: Sha256Digest::new("56".repeat(32)).unwrap(),
                })
                .unwrap(),
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO plugin_installations \
             (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,\
              manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,\
              grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at) \
             VALUES ('install-b','tenant-a','official.fixture','1.1.0',?1,?2,\
                     'first_party_native',?3,?3,2,2,'active','now','now')",
            params![
                next_package_digest,
                next_manifest_digest,
                serde_json::to_string(&permissions).unwrap(),
            ],
        )
        .unwrap();
    }

    let tenant_a = TenantId::new("tenant-a").unwrap();
    let exact = executable(&"ab".repeat(32), &"ef".repeat(32));
    let (package, installation) = resolve_sqlite_plugin_facts(&pool, &tenant_a, &exact).unwrap();
    assert!(package.identity.matches_executable(&exact));
    assert_eq!(installation.installation_id, "install-a");
    assert_eq!(installation.tenant_id, tenant_a);

    let upgraded = executable_with_version("1.1.0", &"12".repeat(32), &"34".repeat(32));
    let (upgraded_package, upgraded_installation) =
        resolve_sqlite_plugin_facts(&pool, &tenant_a, &upgraded).unwrap();
    assert!(upgraded_package.identity.matches_executable(&upgraded));
    assert_eq!(upgraded_installation.installation_id, "install-b");

    // Installing the newer exact package must not make old queued work resolve
    // to the newer executable identity.
    let (_, old_after_upgrade) = resolve_sqlite_plugin_facts(&pool, &tenant_a, &exact).unwrap();
    assert_eq!(old_after_upgrade.installation_id, "install-a");

    let tenant_b = TenantId::new("tenant-b").unwrap();
    assert_eq!(
        resolve_sqlite_plugin_facts(&pool, &tenant_b, &exact),
        Err(PluginStoreError::NotInstalled)
    );

    let wrong_package = executable(&"11".repeat(32), &"ef".repeat(32));
    assert_eq!(
        resolve_sqlite_plugin_facts(&pool, &tenant_a, &wrong_package),
        Err(PluginStoreError::NotInstalled)
    );

    let wrong_manifest = executable(&"ab".repeat(32), &"22".repeat(32));
    assert_eq!(
        resolve_sqlite_plugin_facts(&pool, &tenant_a, &wrong_manifest),
        Err(PluginStoreError::NotInstalled)
    );

    let mut wrong_contract = exact.clone();
    wrong_contract.capability_contract_version = ContractVersion::new("2.0.0").unwrap();
    assert_eq!(
        resolve_sqlite_plugin_facts(&pool, &tenant_a, &wrong_contract),
        Err(PluginStoreError::NotInstalled)
    );
}

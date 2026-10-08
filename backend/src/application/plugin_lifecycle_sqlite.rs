#![cfg(feature = "sqlite")]

use rusqlite::{OptionalExtension, TransactionBehavior};
use system_core::TenantId;
use system_core::security::plugin::{
    PermissionSet, PluginLifecycle, PluginPackageIdentity, PluginPackageRecord, PublisherId,
    VerificationEvidence,
};
use system_core::transport::interconnect::PluginExecutableRef;

use super::plugin_lifecycle::{
    PersistedPackage, PluginInstallationActivation, PluginLifecycleMutationError,
    PluginUpgradeAuthorization, SqlitePluginLifecycleService, decode_package,
    isolation_profile_name, package_identity_from_executable, valid_token, validate_activation,
    validate_upgrade,
};
use super::plugin_verification::VerifiedPluginPackage;

impl SqlitePluginLifecycleService {
    pub fn stage_verified_package(
        &self,
        verified: VerifiedPluginPackage,
    ) -> Result<PluginPackageIdentity, PluginLifecycleMutationError> {
        let record = verified.into_record();
        if record.lifecycle != PluginLifecycle::Staged
            || !matches!(
                record.verification,
                VerificationEvidence::PublisherVerified { .. }
            )
        {
            return Err(PluginLifecycleMutationError::InvalidVerifiedPackage);
        }
        let identity = record.identity.clone();
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self
            .pool
            .get()
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        let inserted = conn
            .execute(
                "INSERT OR IGNORE INTO plugin_packages \
                 (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,\
                  capability_contract_version,compatibility_range,declared_capabilities_json,\
                  permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'staged',?11,?11)",
                rusqlite::params![
                    record.identity.plugin_id.as_str(),
                    record.identity.publisher_id.as_str(),
                    record.identity.version.as_str(),
                    record.identity.package_digest_sha256.as_str(),
                    record.identity.manifest_digest_sha256.as_str(),
                    record.identity.capability_contract_version.as_str(),
                    record.compatibility_range.as_str(),
                    serde_json::to_string(&record.declared_capabilities)
                        .map_err(|_| PluginLifecycleMutationError::Persistence)?,
                    serde_json::to_string(&record.permission_request)
                        .map_err(|_| PluginLifecycleMutationError::Persistence)?,
                    serde_json::to_string(&record.verification)
                        .map_err(|_| PluginLifecycleMutationError::Persistence)?,
                    now,
                ],
            )
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        if inserted != 1 {
            return Err(PluginLifecycleMutationError::PackageAlreadyRegistered);
        }
        Ok(identity)
    }

    pub fn activate_installation(
        &self,
        candidate_identity: &PluginPackageIdentity,
        activation: &PluginInstallationActivation,
        upgrade: Option<&PluginUpgradeAuthorization>,
    ) -> Result<(), PluginLifecycleMutationError> {
        let mut conn = self
            .pool
            .get()
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;

        let candidate = load_package(&tx, candidate_identity)?;
        if !matches!(
            candidate.lifecycle,
            PluginLifecycle::Staged | PluginLifecycle::Active
        ) || !matches!(
            candidate.verification,
            VerificationEvidence::PublisherVerified { .. }
        ) {
            return Err(PluginLifecycleMutationError::PackageNotActivatable);
        }
        validate_activation(&candidate, activation)?;

        let existing_count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM plugin_installations WHERE tenant_id=?1 AND plugin_id=?2",
                rusqlite::params![
                    activation.tenant_id.as_str(),
                    candidate.identity.plugin_id.as_str(),
                ],
                |row| row.get(0),
            )
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        if existing_count < 0 {
            return Err(PluginLifecycleMutationError::InvalidPersistedRecord);
        }
        match (existing_count == 0, upgrade) {
            (true, Some(_)) => return Err(PluginLifecycleMutationError::UnexpectedUpgradeReview),
            (false, None) => return Err(PluginLifecycleMutationError::UpgradeReviewRequired),
            (false, Some(authorization)) => {
                let previous = load_upgrade_source(
                    &tx,
                    &activation.tenant_id,
                    &authorization.previous_executable,
                )?;
                validate_upgrade(&previous, &candidate, authorization)?;
            }
            (true, None) => {}
        }

        let now = chrono::Utc::now().to_rfc3339();
        if candidate.lifecycle == PluginLifecycle::Staged {
            let changed = tx
                .execute(
                    "UPDATE plugin_packages SET lifecycle_state='active',updated_at=?1 \
                     WHERE plugin_id=?2 AND publisher_id=?3 AND plugin_version=?4 \
                       AND package_digest_sha256=?5 AND manifest_digest_sha256=?6 \
                       AND capability_contract_version=?7 AND lifecycle_state='staged'",
                    rusqlite::params![
                        now,
                        candidate.identity.plugin_id.as_str(),
                        candidate.identity.publisher_id.as_str(),
                        candidate.identity.version.as_str(),
                        candidate.identity.package_digest_sha256.as_str(),
                        candidate.identity.manifest_digest_sha256.as_str(),
                        candidate.identity.capability_contract_version.as_str(),
                    ],
                )
                .map_err(|_| PluginLifecycleMutationError::Persistence)?;
            if changed != 1 {
                return Err(PluginLifecycleMutationError::InvalidTransition);
            }
        }

        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO plugin_installations \
                 (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,\
                  manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,\
                  grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'active',?12,?12)",
                rusqlite::params![
                    activation.installation_id,
                    activation.tenant_id.as_str(),
                    candidate.identity.plugin_id.as_str(),
                    candidate.identity.version.as_str(),
                    candidate.identity.package_digest_sha256.as_str(),
                    candidate.identity.manifest_digest_sha256.as_str(),
                    isolation_profile_name(activation.isolation_profile),
                    serde_json::to_string(&activation.installation_grant)
                        .map_err(|_| PluginLifecycleMutationError::Persistence)?,
                    serde_json::to_string(&activation.tenant_policy)
                        .map_err(|_| PluginLifecycleMutationError::Persistence)?,
                    i64::try_from(activation.grant_revision)
                        .map_err(|_| PluginLifecycleMutationError::InvalidActivation)?,
                    i64::try_from(activation.tenant_policy_revision)
                        .map_err(|_| PluginLifecycleMutationError::InvalidActivation)?,
                    now,
                ],
            )
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        if inserted != 1 {
            return Err(PluginLifecycleMutationError::InstallationAlreadyExists);
        }

        if let Some(authorization) = upgrade {
            let recorded = tx
                .execute(
                    "INSERT INTO plugin_upgrade_transitions \
                     (candidate_installation_id,tenant_id,previous_executable_json,\
                      candidate_identity_json,permission_diff_json,approved_additions_json,\
                      migration_strategy,created_at) \
                     VALUES (?1,?2,?3,?4,?5,?6,'preserve_exact_pins',?7)",
                    rusqlite::params![
                        activation.installation_id,
                        activation.tenant_id.as_str(),
                        serde_json::to_string(&authorization.previous_executable)
                            .map_err(|_| PluginLifecycleMutationError::Persistence)?,
                        serde_json::to_string(&candidate.identity)
                            .map_err(|_| PluginLifecycleMutationError::Persistence)?,
                        serde_json::to_string(authorization.review.permission_diff())
                            .map_err(|_| PluginLifecycleMutationError::Persistence)?,
                        serde_json::to_string(authorization.review.approved_additions())
                            .map_err(|_| PluginLifecycleMutationError::Persistence)?,
                        now,
                    ],
                )
                .map_err(|_| PluginLifecycleMutationError::Persistence)?;
            if recorded != 1 {
                return Err(PluginLifecycleMutationError::InvalidTransition);
            }
        }

        tx.commit()
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        Ok(())
    }

    pub fn suspend_installation(
        &self,
        tenant_id: &TenantId,
        installation_id: &str,
        executable: &PluginExecutableRef,
    ) -> Result<(), PluginLifecycleMutationError> {
        validate_exact_installation_target(installation_id, executable)?;
        let conn = self
            .pool
            .get()
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        let now = chrono::Utc::now().to_rfc3339();
        let changed = conn
            .execute(
                "UPDATE plugin_installations AS i SET lifecycle_state='suspended',\
                 revoked_reason_code=NULL,updated_at=?1 \
                 WHERE i.installation_id=?2 AND i.tenant_id=?3 AND i.plugin_id=?4 \
                   AND i.plugin_version=?5 AND i.package_digest_sha256=?6 \
                   AND i.manifest_digest_sha256=?7 AND i.lifecycle_state='active' \
                   AND EXISTS (SELECT 1 FROM plugin_packages p WHERE p.plugin_id=i.plugin_id \
                     AND p.plugin_version=i.plugin_version \
                     AND p.package_digest_sha256=i.package_digest_sha256 \
                     AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
                     AND p.capability_contract_version=?8)",
                rusqlite::params![
                    now,
                    installation_id,
                    tenant_id.as_str(),
                    executable.plugin_id.as_str(),
                    executable.version.as_str(),
                    executable.package_digest_sha256,
                    executable.manifest_digest_sha256,
                    executable.capability_contract_version.as_str(),
                ],
            )
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        one_transition(changed)
    }

    pub fn revoke_installation(
        &self,
        tenant_id: &TenantId,
        installation_id: &str,
        executable: &PluginExecutableRef,
        reason_code: &str,
    ) -> Result<(), PluginLifecycleMutationError> {
        validate_exact_installation_target(installation_id, executable)?;
        if !valid_token(reason_code) {
            return Err(PluginLifecycleMutationError::InvalidReason);
        }
        let conn = self
            .pool
            .get()
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        let now = chrono::Utc::now().to_rfc3339();
        let changed = conn
            .execute(
                "UPDATE plugin_installations AS i SET lifecycle_state='revoked',\
                 revoked_reason_code=?1,updated_at=?2 \
                 WHERE i.installation_id=?3 AND i.tenant_id=?4 AND i.plugin_id=?5 \
                   AND i.plugin_version=?6 AND i.package_digest_sha256=?7 \
                   AND i.manifest_digest_sha256=?8 \
                   AND i.lifecycle_state IN ('active','suspended') \
                   AND EXISTS (SELECT 1 FROM plugin_packages p WHERE p.plugin_id=i.plugin_id \
                     AND p.plugin_version=i.plugin_version \
                     AND p.package_digest_sha256=i.package_digest_sha256 \
                     AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
                     AND p.capability_contract_version=?9)",
                rusqlite::params![
                    reason_code,
                    now,
                    installation_id,
                    tenant_id.as_str(),
                    executable.plugin_id.as_str(),
                    executable.version.as_str(),
                    executable.package_digest_sha256,
                    executable.manifest_digest_sha256,
                    executable.capability_contract_version.as_str(),
                ],
            )
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        one_transition(changed)
    }

    pub fn suspend_package(
        &self,
        identity: &PluginPackageIdentity,
    ) -> Result<(), PluginLifecycleMutationError> {
        self.transition_package(identity, false, None)
    }

    pub fn revoke_package(
        &self,
        identity: &PluginPackageIdentity,
        reason_code: &str,
    ) -> Result<(), PluginLifecycleMutationError> {
        if !valid_token(reason_code) {
            return Err(PluginLifecycleMutationError::InvalidReason);
        }
        self.transition_package(identity, true, Some(reason_code))
    }

    fn transition_package(
        &self,
        identity: &PluginPackageIdentity,
        revoke: bool,
        reason: Option<&str>,
    ) -> Result<(), PluginLifecycleMutationError> {
        let conn = self
            .pool
            .get()
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        let now = chrono::Utc::now().to_rfc3339();
        let changed = if revoke {
            conn.execute(
                "UPDATE plugin_packages SET lifecycle_state='revoked',revoked_reason_code=?1,updated_at=?2 \
                 WHERE plugin_id=?3 AND publisher_id=?4 AND plugin_version=?5 \
                   AND package_digest_sha256=?6 AND manifest_digest_sha256=?7 \
                   AND capability_contract_version=?8 \
                   AND lifecycle_state IN ('staged','active','suspended')",
                rusqlite::params![
                    reason,
                    now,
                    identity.plugin_id.as_str(),
                    identity.publisher_id.as_str(),
                    identity.version.as_str(),
                    identity.package_digest_sha256.as_str(),
                    identity.manifest_digest_sha256.as_str(),
                    identity.capability_contract_version.as_str(),
                ],
            )
        } else {
            conn.execute(
                "UPDATE plugin_packages SET lifecycle_state='suspended',revoked_reason_code=NULL,updated_at=?1 \
                 WHERE plugin_id=?2 AND publisher_id=?3 AND plugin_version=?4 \
                   AND package_digest_sha256=?5 AND manifest_digest_sha256=?6 \
                   AND capability_contract_version=?7 AND lifecycle_state='active'",
                rusqlite::params![
                    now,
                    identity.plugin_id.as_str(),
                    identity.publisher_id.as_str(),
                    identity.version.as_str(),
                    identity.package_digest_sha256.as_str(),
                    identity.manifest_digest_sha256.as_str(),
                    identity.capability_contract_version.as_str(),
                ],
            )
        }
        .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        one_transition(changed)
    }
}

fn validate_exact_installation_target(
    installation_id: &str,
    executable: &PluginExecutableRef,
) -> Result<(), PluginLifecycleMutationError> {
    if !valid_token(installation_id) || executable.validate().is_err() {
        return Err(PluginLifecycleMutationError::InvalidActivation);
    }
    Ok(())
}

fn one_transition(changed: usize) -> Result<(), PluginLifecycleMutationError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(PluginLifecycleMutationError::InvalidTransition)
    }
}

fn load_package(
    tx: &rusqlite::Transaction<'_>,
    identity: &PluginPackageIdentity,
) -> Result<PluginPackageRecord, PluginLifecycleMutationError> {
    let persisted = tx
        .query_row(
            "SELECT publisher_id,compatibility_range,declared_capabilities_json,\
                    permission_request_json,verification_evidence_json,lifecycle_state,revoked_reason_code \
             FROM plugin_packages WHERE plugin_id=?1 AND publisher_id=?2 AND plugin_version=?3 \
               AND package_digest_sha256=?4 AND manifest_digest_sha256=?5 \
               AND capability_contract_version=?6",
            rusqlite::params![
                identity.plugin_id.as_str(),
                identity.publisher_id.as_str(),
                identity.version.as_str(),
                identity.package_digest_sha256.as_str(),
                identity.manifest_digest_sha256.as_str(),
                identity.capability_contract_version.as_str(),
            ],
            |row| {
                Ok(PersistedPackage {
                    publisher_id: row.get(0)?,
                    compatibility_range: row.get(1)?,
                    declared_capabilities_json: row.get(2)?,
                    permission_request_json: row.get(3)?,
                    verification_evidence_json: row.get(4)?,
                    lifecycle_state: row.get(5)?,
                    revoked_reason_code: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|_| PluginLifecycleMutationError::Persistence)?
        .ok_or(PluginLifecycleMutationError::PackageNotFound)?;
    decode_package(persisted, identity)
}

fn load_upgrade_source(
    tx: &rusqlite::Transaction<'_>,
    tenant_id: &TenantId,
    executable: &PluginExecutableRef,
) -> Result<PluginPackageRecord, PluginLifecycleMutationError> {
    executable
        .validate()
        .map_err(|_| PluginLifecycleMutationError::InvalidActivation)?;
    let row: Option<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    )> = tx
        .query_row(
            "SELECT p.publisher_id,p.capability_contract_version,p.compatibility_range,\
                    p.declared_capabilities_json,p.permission_request_json,p.verification_evidence_json,\
                    p.lifecycle_state,p.revoked_reason_code,i.lifecycle_state \
             FROM plugin_installations i JOIN plugin_packages p \
               ON p.plugin_id=i.plugin_id AND p.plugin_version=i.plugin_version \
              AND p.package_digest_sha256=i.package_digest_sha256 \
              AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
             WHERE i.tenant_id=?1 AND i.plugin_id=?2 AND i.plugin_version=?3 \
               AND i.package_digest_sha256=?4 AND i.manifest_digest_sha256=?5 \
               AND p.capability_contract_version=?6",
            rusqlite::params![
                tenant_id.as_str(),
                executable.plugin_id.as_str(),
                executable.version.as_str(),
                executable.package_digest_sha256,
                executable.manifest_digest_sha256,
                executable.capability_contract_version.as_str(),
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .optional()
        .map_err(|_| PluginLifecycleMutationError::Persistence)?;
    let Some((
        publisher,
        capability_contract,
        compatibility,
        capabilities,
        permissions,
        verification,
        package_state,
        package_reason,
        installation_state,
    )) = row
    else {
        return Err(PluginLifecycleMutationError::UpgradeSourceNotFound);
    };
    if !matches!(installation_state.as_str(), "active" | "suspended")
        || !matches!(package_state.as_str(), "active" | "suspended")
    {
        return Err(PluginLifecycleMutationError::UpgradeSourceNotExecutable);
    }
    let identity = package_identity_from_executable(
        executable,
        PublisherId::new(publisher.clone())
            .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
    )?;
    let record = decode_package(
        PersistedPackage {
            publisher_id: publisher,
            compatibility_range: compatibility,
            declared_capabilities_json: capabilities,
            permission_request_json: permissions,
            verification_evidence_json: verification,
            lifecycle_state: package_state,
            revoked_reason_code: package_reason,
        },
        &identity,
    )?;
    if record.identity.capability_contract_version.as_str() != capability_contract
        || !record.identity.matches_executable(executable)
    {
        return Err(PluginLifecycleMutationError::InvalidPersistedRecord);
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use sha2::{Digest, Sha256};
    use system_core::security::plugin::{CompatibilityRange, PluginPermission, Sha256Digest};
    use system_core::security::plugin_upgrade::{
        PluginUpgradeMigrationStrategy, review_package_transition,
    };
    use system_core::transport::interconnect::{ContractVersion, PluginId};

    use super::*;
    use crate::application::plugin_verification::{
        PluginPublisherSignatureVerifier, verify_publisher_package,
    };

    struct AcceptVerifier;

    impl PluginPublisherSignatureVerifier for AcceptVerifier {
        fn verify(&self, _: &PublisherId, _: &str, _: &[u8], _: &[u8]) -> Result<bool, String> {
            Ok(true)
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
            "INSERT INTO tenants (id,name,slug,status,plan,created_at,updated_at) \
             VALUES ('tenant-a','A','a','active','test','now','now')",
            [],
        )
        .unwrap();
        drop(conn);
        pool
    }

    fn raw_candidate(
        version: &str,
        seed: &str,
        permissions: PermissionSet,
    ) -> (PluginPackageRecord, Vec<u8>, Vec<u8>) {
        let package_bytes = format!("package-{version}-{seed}").into_bytes();
        let manifest_bytes = format!("manifest-{version}-{seed}").into_bytes();
        let record = PluginPackageRecord {
            identity: PluginPackageIdentity {
                plugin_id: PluginId::new("official.fixture").unwrap(),
                publisher_id: PublisherId::new("talos.official").unwrap(),
                version: ContractVersion::new(version).unwrap(),
                package_digest_sha256: Sha256Digest::new(hex::encode(Sha256::digest(
                    &package_bytes,
                )))
                .unwrap(),
                manifest_digest_sha256: Sha256Digest::new(hex::encode(Sha256::digest(
                    &manifest_bytes,
                )))
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

    fn stage(
        service: &SqlitePluginLifecycleService,
        record: &PluginPackageRecord,
        package_bytes: &[u8],
        manifest_bytes: &[u8],
    ) -> PluginPackageIdentity {
        let proof = verify_publisher_package(
            record,
            package_bytes,
            manifest_bytes,
            "talos.release.root",
            b"signature",
            &AcceptVerifier,
        )
        .unwrap();
        service.stage_verified_package(proof).unwrap()
    }

    fn activation(id: &str, permissions: PermissionSet) -> PluginInstallationActivation {
        PluginInstallationActivation {
            installation_id: id.into(),
            tenant_id: TenantId::new("tenant-a").unwrap(),
            isolation_profile:
                system_core::security::plugin::PluginIsolationProfile::FirstPartyNative,
            installation_grant: permissions.clone(),
            tenant_policy: permissions,
            grant_revision: 1,
            tenant_policy_revision: 1,
        }
    }

    fn executable(record: &PluginPackageRecord) -> PluginExecutableRef {
        PluginExecutableRef {
            plugin_id: record.identity.plugin_id.clone(),
            version: record.identity.version.clone(),
            package_digest_sha256: record.identity.package_digest_sha256.as_str().to_owned(),
            manifest_digest_sha256: record.identity.manifest_digest_sha256.as_str().to_owned(),
            capability_contract_version: record.identity.capability_contract_version.clone(),
            provider_instance_id: None,
            binding_revision: None,
        }
    }

    fn active_for_review(record: &PluginPackageRecord) -> PluginPackageRecord {
        let mut record = record.clone();
        record.lifecycle = PluginLifecycle::Active;
        record.verification = VerificationEvidence::PublisherVerified {
            publisher_key_id: "talos.release.root".into(),
            signature_digest_sha256: Sha256Digest::new(hex::encode(Sha256::digest(b"signature")))
                .unwrap(),
        };
        record
    }

    #[test]
    fn verified_package_stages_then_initial_install_activates_exact_identity() {
        let permissions =
            PermissionSet::new([PluginPermission::CapabilityInvoke("orders.read".into())]).unwrap();
        let (record, package_bytes, manifest_bytes) =
            raw_candidate("1.0.0", "a", permissions.clone());
        let pool = pool();
        let service = SqlitePluginLifecycleService::new(pool.clone());
        let identity = stage(&service, &record, &package_bytes, &manifest_bytes);
        service
            .activate_installation(&identity, &activation("install-a", permissions), None)
            .unwrap();
        let package_state: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT lifecycle_state FROM plugin_packages WHERE plugin_id='official.fixture'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(package_state, "active");
    }

    #[test]
    fn upgrade_requires_exact_review_and_records_preserve_exact_pins_migration() {
        let read = PluginPermission::CapabilityInvoke("orders.read".into());
        let network = PluginPermission::NetworkGrant("provider.api".into());
        let first_permissions = PermissionSet::new([read.clone()]).unwrap();
        let next_permissions = PermissionSet::new([read, network.clone()]).unwrap();
        let (first, first_package, first_manifest) =
            raw_candidate("1.0.0", "a", first_permissions.clone());
        let (next, next_package, next_manifest) =
            raw_candidate("1.1.0", "b", next_permissions.clone());
        let pool = pool();
        let service = SqlitePluginLifecycleService::new(pool.clone());
        let first_identity = stage(&service, &first, &first_package, &first_manifest);
        service
            .activate_installation(
                &first_identity,
                &activation("install-a", first_permissions),
                None,
            )
            .unwrap();
        let next_identity = stage(&service, &next, &next_package, &next_manifest);
        assert_eq!(
            service.activate_installation(
                &next_identity,
                &activation("install-b", next_permissions.clone()),
                None,
            ),
            Err(PluginLifecycleMutationError::UpgradeReviewRequired)
        );

        let review = review_package_transition(
            &active_for_review(&first),
            &active_for_review(&next),
            PermissionSet::new([network]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            review.migration_strategy(),
            PluginUpgradeMigrationStrategy::PreserveExactPins
        );
        service
            .activate_installation(
                &next_identity,
                &activation("install-b", next_permissions),
                Some(&PluginUpgradeAuthorization {
                    previous_executable: executable(&first),
                    review,
                }),
            )
            .unwrap();

        let conn = pool.get().unwrap();
        let (strategy, previous_json): (String, String) = conn
            .query_row(
                "SELECT migration_strategy,previous_executable_json \
                 FROM plugin_upgrade_transitions WHERE candidate_installation_id='install-b'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(strategy, "preserve_exact_pins");
        let recorded_previous: PluginExecutableRef = serde_json::from_str(&previous_json).unwrap();
        assert_eq!(recorded_previous, executable(&first));

        let old_state: String = conn
            .query_row(
                "SELECT lifecycle_state FROM plugin_installations WHERE installation_id='install-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(old_state, "active");
    }

    #[test]
    fn package_revocation_blocks_future_exact_resolution_without_retargeting_installation() {
        let permissions =
            PermissionSet::new([PluginPermission::CapabilityInvoke("orders.read".into())]).unwrap();
        let (record, package_bytes, manifest_bytes) =
            raw_candidate("1.0.0", "a", permissions.clone());
        let pool = pool();
        let service = SqlitePluginLifecycleService::new(pool.clone());
        let identity = stage(&service, &record, &package_bytes, &manifest_bytes);
        service
            .activate_installation(&identity, &activation("install-a", permissions), None)
            .unwrap();
        service
            .revoke_package(&identity, "security_revoke")
            .unwrap();
        let state: String = pool
            .get()
            .unwrap()
            .query_row(
                "SELECT lifecycle_state FROM plugin_packages WHERE plugin_id='official.fixture'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "revoked");
    }
}

//! R4-P6 persistence adapter for exact plugin package/install facts.
//!
//! Resolution is intentionally keyed by the P3 immutable executable pin. It
//! never resolves "latest" plugin state and therefore cannot silently retarget
//! durable work after an upgrade.

use std::collections::BTreeSet;

#[cfg(feature = "sqlite")]
use r2d2::Pool;
#[cfg(feature = "sqlite")]
use r2d2_sqlite::SqliteConnectionManager;
#[cfg(feature = "sqlite")]
use rusqlite::{OptionalExtension, params};
use serde::de::DeserializeOwned;
use system_core::TenantId;
use system_core::security::plugin::{
    CompatibilityRange, PermissionSet, PluginIsolationProfile, PluginLifecycle,
    PluginPackageIdentity, PluginPackageRecord, PublisherId, Sha256Digest, VerificationEvidence,
};
use system_core::transport::interconnect::{ContractVersion, PluginExecutableRef, PluginId};

use super::plugin_host::{PluginInstallationLifecycle, PluginInstallationRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginStoreError {
    Persistence,
    NotInstalled,
    InvalidPersistedRecord,
}

#[derive(Debug, Clone)]
struct PersistedPluginFacts {
    publisher_id: String,
    capability_contract_version: String,
    compatibility_range: String,
    declared_capabilities_json: String,
    permission_request_json: String,
    verification_evidence_json: String,
    package_lifecycle: String,
    package_revoked_reason: Option<String>,
    installation_id: String,
    isolation_profile: String,
    installation_grant_json: String,
    tenant_policy_json: String,
    grant_revision: i64,
    tenant_policy_revision: i64,
    installation_lifecycle: String,
}

impl PersistedPluginFacts {
    fn decode(
        self,
        tenant_id: &TenantId,
        executable: &PluginExecutableRef,
    ) -> Result<(PluginPackageRecord, PluginInstallationRecord), PluginStoreError> {
        let package_identity = PluginPackageIdentity {
            plugin_id: PluginId::new(executable.plugin_id.as_str())
                .map_err(|_| PluginStoreError::InvalidPersistedRecord)?,
            publisher_id: PublisherId::new(self.publisher_id)
                .map_err(|_| PluginStoreError::InvalidPersistedRecord)?,
            version: ContractVersion::new(executable.version.as_str())
                .map_err(|_| PluginStoreError::InvalidPersistedRecord)?,
            package_digest_sha256: Sha256Digest::new(&executable.package_digest_sha256)
                .map_err(|_| PluginStoreError::InvalidPersistedRecord)?,
            manifest_digest_sha256: Sha256Digest::new(&executable.manifest_digest_sha256)
                .map_err(|_| PluginStoreError::InvalidPersistedRecord)?,
            capability_contract_version: ContractVersion::new(self.capability_contract_version)
                .map_err(|_| PluginStoreError::InvalidPersistedRecord)?,
        };
        if !package_identity.matches_executable(executable) {
            return Err(PluginStoreError::InvalidPersistedRecord);
        }

        let package = PluginPackageRecord {
            identity: package_identity.clone(),
            compatibility_range: CompatibilityRange::new(self.compatibility_range)
                .map_err(|_| PluginStoreError::InvalidPersistedRecord)?,
            declared_capabilities: decode_json::<BTreeSet<String>>(
                &self.declared_capabilities_json,
            )?,
            permission_request: decode_json::<PermissionSet>(&self.permission_request_json)?,
            verification: decode_json::<VerificationEvidence>(&self.verification_evidence_json)?,
            lifecycle: package_lifecycle(
                &self.package_lifecycle,
                self.package_revoked_reason.as_deref(),
            )?,
        };
        package
            .validate()
            .map_err(|_| PluginStoreError::InvalidPersistedRecord)?;

        let installation = PluginInstallationRecord {
            installation_id: self.installation_id,
            tenant_id: tenant_id.clone(),
            package_identity,
            isolation_profile: isolation_profile(&self.isolation_profile)?,
            installation_grant: decode_json::<PermissionSet>(&self.installation_grant_json)?,
            tenant_policy: decode_json::<PermissionSet>(&self.tenant_policy_json)?,
            grant_revision: u64::try_from(self.grant_revision)
                .map_err(|_| PluginStoreError::InvalidPersistedRecord)?,
            tenant_policy_revision: u64::try_from(self.tenant_policy_revision)
                .map_err(|_| PluginStoreError::InvalidPersistedRecord)?,
            lifecycle: installation_lifecycle(&self.installation_lifecycle)?,
        };
        installation
            .validate()
            .map_err(|_| PluginStoreError::InvalidPersistedRecord)?;
        Ok((package, installation))
    }
}

fn decode_json<T: DeserializeOwned>(value: &str) -> Result<T, PluginStoreError> {
    serde_json::from_str(value).map_err(|_| PluginStoreError::InvalidPersistedRecord)
}

fn package_lifecycle(
    value: &str,
    revoked_reason: Option<&str>,
) -> Result<PluginLifecycle, PluginStoreError> {
    match value {
        "staged" => Ok(PluginLifecycle::Staged),
        "active" => Ok(PluginLifecycle::Active),
        "suspended" => Ok(PluginLifecycle::Suspended),
        "revoked" => Ok(PluginLifecycle::Revoked {
            reason_code: revoked_reason
                .ok_or(PluginStoreError::InvalidPersistedRecord)?
                .to_owned(),
        }),
        _ => Err(PluginStoreError::InvalidPersistedRecord),
    }
}

fn installation_lifecycle(value: &str) -> Result<PluginInstallationLifecycle, PluginStoreError> {
    match value {
        "active" => Ok(PluginInstallationLifecycle::Active),
        "suspended" => Ok(PluginInstallationLifecycle::Suspended),
        "revoked" => Ok(PluginInstallationLifecycle::Revoked),
        _ => Err(PluginStoreError::InvalidPersistedRecord),
    }
}

fn isolation_profile(value: &str) -> Result<PluginIsolationProfile, PluginStoreError> {
    match value {
        "first_party_native" => Ok(PluginIsolationProfile::FirstPartyNative),
        "verified_sandboxed" => Ok(PluginIsolationProfile::VerifiedSandboxed),
        "local_unverified" => Ok(PluginIsolationProfile::LocalUnverified),
        "single_file_web_app" => Ok(PluginIsolationProfile::SingleFileWebApp),
        "remote_worker" => Ok(PluginIsolationProfile::RemoteWorker),
        _ => Err(PluginStoreError::InvalidPersistedRecord),
    }
}

#[cfg(feature = "sqlite")]
pub fn resolve_sqlite_plugin_facts(
    pool: &Pool<SqliteConnectionManager>,
    tenant_id: &TenantId,
    executable: &PluginExecutableRef,
) -> Result<(PluginPackageRecord, PluginInstallationRecord), PluginStoreError> {
    executable
        .validate()
        .map_err(|_| PluginStoreError::InvalidPersistedRecord)?;
    let conn = pool.get().map_err(|_| PluginStoreError::Persistence)?;
    let row = conn
        .query_row(
            "SELECT p.publisher_id,p.capability_contract_version,p.compatibility_range,\
                    p.declared_capabilities_json,p.permission_request_json,\
                    p.verification_evidence_json,p.lifecycle_state,p.revoked_reason_code,\
                    i.installation_id,i.isolation_profile,i.installation_grant_json,\
                    i.tenant_policy_json,i.grant_revision,i.tenant_policy_revision,i.lifecycle_state \
             FROM plugin_installations i \
             JOIN plugin_packages p \
               ON p.plugin_id=i.plugin_id \
              AND p.plugin_version=i.plugin_version \
              AND p.package_digest_sha256=i.package_digest_sha256 \
              AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
             WHERE i.tenant_id=?1 AND i.plugin_id=?2 AND i.plugin_version=?3 \
               AND i.package_digest_sha256=?4 AND i.manifest_digest_sha256=?5 \
               AND p.capability_contract_version=?6",
            params![
                tenant_id.as_str(),
                executable.plugin_id.as_str(),
                executable.version.as_str(),
                executable.package_digest_sha256,
                executable.manifest_digest_sha256,
                executable.capability_contract_version.as_str(),
            ],
            |row| {
                Ok(PersistedPluginFacts {
                    publisher_id: row.get(0)?,
                    capability_contract_version: row.get(1)?,
                    compatibility_range: row.get(2)?,
                    declared_capabilities_json: row.get(3)?,
                    permission_request_json: row.get(4)?,
                    verification_evidence_json: row.get(5)?,
                    package_lifecycle: row.get(6)?,
                    package_revoked_reason: row.get(7)?,
                    installation_id: row.get(8)?,
                    isolation_profile: row.get(9)?,
                    installation_grant_json: row.get(10)?,
                    tenant_policy_json: row.get(11)?,
                    grant_revision: row.get(12)?,
                    tenant_policy_revision: row.get(13)?,
                    installation_lifecycle: row.get(14)?,
                })
            },
        )
        .optional()
        .map_err(|_| PluginStoreError::Persistence)?
        .ok_or(PluginStoreError::NotInstalled)?;
    row.decode(tenant_id, executable)
}

#[cfg(feature = "postgres")]
pub async fn resolve_postgres_plugin_facts(
    pool: &sqlx::PgPool,
    tenant_id: &TenantId,
    executable: &PluginExecutableRef,
) -> Result<(PluginPackageRecord, PluginInstallationRecord), PluginStoreError> {
    use sqlx::Row;

    executable
        .validate()
        .map_err(|_| PluginStoreError::InvalidPersistedRecord)?;
    let row = sqlx::query(
        "SELECT p.publisher_id,p.capability_contract_version,p.compatibility_range,\
                p.declared_capabilities_json,p.permission_request_json,p.verification_evidence_json,\
                p.lifecycle_state,p.revoked_reason_code,i.installation_id,i.isolation_profile,\
                i.installation_grant_json,i.tenant_policy_json,i.grant_revision,\
                i.tenant_policy_revision,i.lifecycle_state AS installation_lifecycle \
         FROM plugin_installations i \
         JOIN plugin_packages p \
           ON p.plugin_id=i.plugin_id \
          AND p.plugin_version=i.plugin_version \
          AND p.package_digest_sha256=i.package_digest_sha256 \
          AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
         WHERE i.tenant_id=$1 AND i.plugin_id=$2 AND i.plugin_version=$3 \
           AND i.package_digest_sha256=$4 AND i.manifest_digest_sha256=$5 \
           AND p.capability_contract_version=$6",
    )
    .bind(tenant_id.as_str())
    .bind(executable.plugin_id.as_str())
    .bind(executable.version.as_str())
    .bind(&executable.package_digest_sha256)
    .bind(&executable.manifest_digest_sha256)
    .bind(executable.capability_contract_version.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|_| PluginStoreError::Persistence)?
    .ok_or(PluginStoreError::NotInstalled)?;

    PersistedPluginFacts {
        publisher_id: row
            .try_get("publisher_id")
            .map_err(|_| PluginStoreError::Persistence)?,
        capability_contract_version: row
            .try_get("capability_contract_version")
            .map_err(|_| PluginStoreError::Persistence)?,
        compatibility_range: row
            .try_get("compatibility_range")
            .map_err(|_| PluginStoreError::Persistence)?,
        declared_capabilities_json: row
            .try_get("declared_capabilities_json")
            .map_err(|_| PluginStoreError::Persistence)?,
        permission_request_json: row
            .try_get("permission_request_json")
            .map_err(|_| PluginStoreError::Persistence)?,
        verification_evidence_json: row
            .try_get("verification_evidence_json")
            .map_err(|_| PluginStoreError::Persistence)?,
        package_lifecycle: row
            .try_get("lifecycle_state")
            .map_err(|_| PluginStoreError::Persistence)?,
        package_revoked_reason: row
            .try_get("revoked_reason_code")
            .map_err(|_| PluginStoreError::Persistence)?,
        installation_id: row
            .try_get("installation_id")
            .map_err(|_| PluginStoreError::Persistence)?,
        isolation_profile: row
            .try_get("isolation_profile")
            .map_err(|_| PluginStoreError::Persistence)?,
        installation_grant_json: row
            .try_get("installation_grant_json")
            .map_err(|_| PluginStoreError::Persistence)?,
        tenant_policy_json: row
            .try_get("tenant_policy_json")
            .map_err(|_| PluginStoreError::Persistence)?,
        grant_revision: row
            .try_get("grant_revision")
            .map_err(|_| PluginStoreError::Persistence)?,
        tenant_policy_revision: row
            .try_get("tenant_policy_revision")
            .map_err(|_| PluginStoreError::Persistence)?,
        installation_lifecycle: row
            .try_get("installation_lifecycle")
            .map_err(|_| PluginStoreError::Persistence)?,
    }
    .decode(tenant_id, executable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use system_core::security::plugin::{PermissionSet, PluginPermission};
    use system_core::transport::interconnect::Subject;

    #[test]
    fn persisted_enums_fail_closed() {
        assert_eq!(
            isolation_profile("future_magic_runtime"),
            Err(PluginStoreError::InvalidPersistedRecord)
        );
        assert_eq!(
            package_lifecycle("revoked", None),
            Err(PluginStoreError::InvalidPersistedRecord)
        );
        assert_eq!(
            installation_lifecycle("staged"),
            Err(PluginStoreError::InvalidPersistedRecord)
        );
    }

    #[test]
    fn permission_json_is_revalidated_on_load() {
        let reserved = PermissionSet::new([PluginPermission::EventEmit(
            Subject::new("system.security.audit").unwrap(),
        )]);
        assert!(reserved.is_err());
    }
}

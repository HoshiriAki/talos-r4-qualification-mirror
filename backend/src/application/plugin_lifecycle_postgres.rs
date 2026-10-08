#![cfg(feature = "postgres")]

use sqlx::Row;
use system_core::TenantId;
use system_core::security::plugin::{
    PluginLifecycle, PluginPackageIdentity, PluginPackageRecord, PublisherId, VerificationEvidence,
};
use system_core::transport::interconnect::PluginExecutableRef;

use super::plugin_lifecycle::{
    PersistedPackage, PluginInstallationActivation, PluginLifecycleMutationError,
    PluginUpgradeAuthorization, PostgresPluginLifecycleService, decode_package,
    isolation_profile_name, package_identity_from_executable, valid_token, validate_activation,
    validate_upgrade,
};
use super::plugin_verification::VerifiedPluginPackage;

impl PostgresPluginLifecycleService {
    pub async fn stage_verified_package(
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
        let inserted = sqlx::query(
            "INSERT INTO plugin_packages \
             (plugin_id,publisher_id,plugin_version,package_digest_sha256,manifest_digest_sha256,\
              capability_contract_version,compatibility_range,declared_capabilities_json,\
              permission_request_json,verification_evidence_json,lifecycle_state,created_at,updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'staged',$11,$11) ON CONFLICT DO NOTHING",
        )
        .bind(record.identity.plugin_id.as_str())
        .bind(record.identity.publisher_id.as_str())
        .bind(record.identity.version.as_str())
        .bind(record.identity.package_digest_sha256.as_str())
        .bind(record.identity.manifest_digest_sha256.as_str())
        .bind(record.identity.capability_contract_version.as_str())
        .bind(record.compatibility_range.as_str())
        .bind(
            serde_json::to_string(&record.declared_capabilities)
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
        )
        .bind(
            serde_json::to_string(&record.permission_request)
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
        )
        .bind(
            serde_json::to_string(&record.verification)
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
        )
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|_| PluginLifecycleMutationError::Persistence)?
        .rows_affected();
        if inserted != 1 {
            return Err(PluginLifecycleMutationError::PackageAlreadyRegistered);
        }
        Ok(identity)
    }

    pub async fn activate_installation(
        &self,
        candidate_identity: &PluginPackageIdentity,
        activation: &PluginInstallationActivation,
        upgrade: Option<&PluginUpgradeAuthorization>,
    ) -> Result<(), PluginLifecycleMutationError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;

        // Serialize the tenant+PluginId decision before checking installation
        // history so concurrent first-install attempts cannot both observe zero.
        let lock_key = format!(
            "talos:r4:p6:plugin-install:{}:{}",
            activation.tenant_id.as_str(),
            candidate_identity.plugin_id.as_str()
        );
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut *tx)
            .await
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;

        let candidate = load_package(&mut tx, candidate_identity).await?;
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

        let row = sqlx::query(
            "SELECT COUNT(*)::BIGINT AS count FROM plugin_installations \
             WHERE tenant_id=$1 AND plugin_id=$2",
        )
        .bind(activation.tenant_id.as_str())
        .bind(candidate.identity.plugin_id.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        let existing_count: i64 = row
            .try_get("count")
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        if existing_count < 0 {
            return Err(PluginLifecycleMutationError::InvalidPersistedRecord);
        }
        match (existing_count == 0, upgrade) {
            (true, Some(_)) => return Err(PluginLifecycleMutationError::UnexpectedUpgradeReview),
            (false, None) => return Err(PluginLifecycleMutationError::UpgradeReviewRequired),
            (false, Some(authorization)) => {
                let previous = load_upgrade_source(
                    &mut tx,
                    &activation.tenant_id,
                    &authorization.previous_executable,
                )
                .await?;
                validate_upgrade(&previous, &candidate, authorization)?;
            }
            (true, None) => {}
        }

        let now = chrono::Utc::now().to_rfc3339();
        if candidate.lifecycle == PluginLifecycle::Staged {
            let changed = sqlx::query(
                "UPDATE plugin_packages SET lifecycle_state='active',updated_at=$1 \
                 WHERE plugin_id=$2 AND publisher_id=$3 AND plugin_version=$4 \
                   AND package_digest_sha256=$5 AND manifest_digest_sha256=$6 \
                   AND capability_contract_version=$7 AND lifecycle_state='staged'",
            )
            .bind(&now)
            .bind(candidate.identity.plugin_id.as_str())
            .bind(candidate.identity.publisher_id.as_str())
            .bind(candidate.identity.version.as_str())
            .bind(candidate.identity.package_digest_sha256.as_str())
            .bind(candidate.identity.manifest_digest_sha256.as_str())
            .bind(candidate.identity.capability_contract_version.as_str())
            .execute(&mut *tx)
            .await
            .map_err(|_| PluginLifecycleMutationError::Persistence)?
            .rows_affected();
            if changed != 1 {
                return Err(PluginLifecycleMutationError::InvalidTransition);
            }
        }

        let inserted = sqlx::query(
            "INSERT INTO plugin_installations \
             (installation_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,\
              manifest_digest_sha256,isolation_profile,installation_grant_json,tenant_policy_json,\
              grant_revision,tenant_policy_revision,lifecycle_state,created_at,updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'active',$12,$12) ON CONFLICT DO NOTHING",
        )
        .bind(&activation.installation_id)
        .bind(activation.tenant_id.as_str())
        .bind(candidate.identity.plugin_id.as_str())
        .bind(candidate.identity.version.as_str())
        .bind(candidate.identity.package_digest_sha256.as_str())
        .bind(candidate.identity.manifest_digest_sha256.as_str())
        .bind(isolation_profile_name(activation.isolation_profile))
        .bind(
            serde_json::to_string(&activation.installation_grant)
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
        )
        .bind(
            serde_json::to_string(&activation.tenant_policy)
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
        )
        .bind(
            i64::try_from(activation.grant_revision)
                .map_err(|_| PluginLifecycleMutationError::InvalidActivation)?,
        )
        .bind(
            i64::try_from(activation.tenant_policy_revision)
                .map_err(|_| PluginLifecycleMutationError::InvalidActivation)?,
        )
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|_| PluginLifecycleMutationError::Persistence)?
        .rows_affected();
        if inserted != 1 {
            return Err(PluginLifecycleMutationError::InstallationAlreadyExists);
        }

        if let Some(authorization) = upgrade {
            let recorded = sqlx::query(
                "INSERT INTO plugin_upgrade_transitions \
                 (candidate_installation_id,tenant_id,previous_executable_json,\
                  candidate_identity_json,permission_diff_json,approved_additions_json,\
                  migration_strategy,created_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,'preserve_exact_pins',$7)",
            )
            .bind(&activation.installation_id)
            .bind(activation.tenant_id.as_str())
            .bind(
                serde_json::to_string(&authorization.previous_executable)
                    .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            )
            .bind(
                serde_json::to_string(&candidate.identity)
                    .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            )
            .bind(
                serde_json::to_string(authorization.review.permission_diff())
                    .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            )
            .bind(
                serde_json::to_string(authorization.review.approved_additions())
                    .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            )
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(|_| PluginLifecycleMutationError::Persistence)?
            .rows_affected();
            if recorded != 1 {
                return Err(PluginLifecycleMutationError::InvalidTransition);
            }
        }

        tx.commit()
            .await
            .map_err(|_| PluginLifecycleMutationError::Persistence)?;
        Ok(())
    }

    pub async fn suspend_installation(
        &self,
        tenant_id: &TenantId,
        installation_id: &str,
        executable: &PluginExecutableRef,
    ) -> Result<(), PluginLifecycleMutationError> {
        validate_exact_installation_target(installation_id, executable)?;
        let now = chrono::Utc::now().to_rfc3339();
        let changed = sqlx::query(
            "UPDATE plugin_installations i SET lifecycle_state='suspended',\
             revoked_reason_code=NULL,updated_at=$1 \
             WHERE i.installation_id=$2 AND i.tenant_id=$3 AND i.plugin_id=$4 \
               AND i.plugin_version=$5 AND i.package_digest_sha256=$6 \
               AND i.manifest_digest_sha256=$7 AND i.lifecycle_state='active' \
               AND EXISTS (SELECT 1 FROM plugin_packages p WHERE p.plugin_id=i.plugin_id \
                 AND p.plugin_version=i.plugin_version \
                 AND p.package_digest_sha256=i.package_digest_sha256 \
                 AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
                 AND p.capability_contract_version=$8)",
        )
        .bind(&now)
        .bind(installation_id)
        .bind(tenant_id.as_str())
        .bind(executable.plugin_id.as_str())
        .bind(executable.version.as_str())
        .bind(&executable.package_digest_sha256)
        .bind(&executable.manifest_digest_sha256)
        .bind(executable.capability_contract_version.as_str())
        .execute(&self.pool)
        .await
        .map_err(|_| PluginLifecycleMutationError::Persistence)?
        .rows_affected();
        one_transition(changed)
    }

    pub async fn revoke_installation(
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
        let now = chrono::Utc::now().to_rfc3339();
        let changed = sqlx::query(
            "UPDATE plugin_installations i SET lifecycle_state='revoked',\
             revoked_reason_code=$1,updated_at=$2 \
             WHERE i.installation_id=$3 AND i.tenant_id=$4 AND i.plugin_id=$5 \
               AND i.plugin_version=$6 AND i.package_digest_sha256=$7 \
               AND i.manifest_digest_sha256=$8 \
               AND i.lifecycle_state IN ('active','suspended') \
               AND EXISTS (SELECT 1 FROM plugin_packages p WHERE p.plugin_id=i.plugin_id \
                 AND p.plugin_version=i.plugin_version \
                 AND p.package_digest_sha256=i.package_digest_sha256 \
                 AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
                 AND p.capability_contract_version=$9)",
        )
        .bind(reason_code)
        .bind(&now)
        .bind(installation_id)
        .bind(tenant_id.as_str())
        .bind(executable.plugin_id.as_str())
        .bind(executable.version.as_str())
        .bind(&executable.package_digest_sha256)
        .bind(&executable.manifest_digest_sha256)
        .bind(executable.capability_contract_version.as_str())
        .execute(&self.pool)
        .await
        .map_err(|_| PluginLifecycleMutationError::Persistence)?
        .rows_affected();
        one_transition(changed)
    }

    pub async fn suspend_package(
        &self,
        identity: &PluginPackageIdentity,
    ) -> Result<(), PluginLifecycleMutationError> {
        self.transition_package(identity, false, None).await
    }

    pub async fn revoke_package(
        &self,
        identity: &PluginPackageIdentity,
        reason_code: &str,
    ) -> Result<(), PluginLifecycleMutationError> {
        if !valid_token(reason_code) {
            return Err(PluginLifecycleMutationError::InvalidReason);
        }
        self.transition_package(identity, true, Some(reason_code))
            .await
    }

    async fn transition_package(
        &self,
        identity: &PluginPackageIdentity,
        revoke: bool,
        reason: Option<&str>,
    ) -> Result<(), PluginLifecycleMutationError> {
        let now = chrono::Utc::now().to_rfc3339();
        let changed = if revoke {
            sqlx::query(
                "UPDATE plugin_packages SET lifecycle_state='revoked',revoked_reason_code=$1,updated_at=$2 \
                 WHERE plugin_id=$3 AND publisher_id=$4 AND plugin_version=$5 \
                   AND package_digest_sha256=$6 AND manifest_digest_sha256=$7 \
                   AND capability_contract_version=$8 \
                   AND lifecycle_state IN ('staged','active','suspended')",
            )
            .bind(reason)
            .bind(&now)
            .bind(identity.plugin_id.as_str())
            .bind(identity.publisher_id.as_str())
            .bind(identity.version.as_str())
            .bind(identity.package_digest_sha256.as_str())
            .bind(identity.manifest_digest_sha256.as_str())
            .bind(identity.capability_contract_version.as_str())
            .execute(&self.pool)
            .await
        } else {
            sqlx::query(
                "UPDATE plugin_packages SET lifecycle_state='suspended',revoked_reason_code=NULL,updated_at=$1 \
                 WHERE plugin_id=$2 AND publisher_id=$3 AND plugin_version=$4 \
                   AND package_digest_sha256=$5 AND manifest_digest_sha256=$6 \
                   AND capability_contract_version=$7 AND lifecycle_state='active'",
            )
            .bind(&now)
            .bind(identity.plugin_id.as_str())
            .bind(identity.publisher_id.as_str())
            .bind(identity.version.as_str())
            .bind(identity.package_digest_sha256.as_str())
            .bind(identity.manifest_digest_sha256.as_str())
            .bind(identity.capability_contract_version.as_str())
            .execute(&self.pool)
            .await
        }
        .map_err(|_| PluginLifecycleMutationError::Persistence)?
        .rows_affected();
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

fn one_transition(changed: u64) -> Result<(), PluginLifecycleMutationError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(PluginLifecycleMutationError::InvalidTransition)
    }
}

async fn load_package(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    identity: &PluginPackageIdentity,
) -> Result<PluginPackageRecord, PluginLifecycleMutationError> {
    let row = sqlx::query(
        "SELECT publisher_id,compatibility_range,declared_capabilities_json,permission_request_json,\
                verification_evidence_json,lifecycle_state,revoked_reason_code \
         FROM plugin_packages WHERE plugin_id=$1 AND publisher_id=$2 AND plugin_version=$3 \
           AND package_digest_sha256=$4 AND manifest_digest_sha256=$5 \
           AND capability_contract_version=$6 FOR UPDATE",
    )
    .bind(identity.plugin_id.as_str())
    .bind(identity.publisher_id.as_str())
    .bind(identity.version.as_str())
    .bind(identity.package_digest_sha256.as_str())
    .bind(identity.manifest_digest_sha256.as_str())
    .bind(identity.capability_contract_version.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| PluginLifecycleMutationError::Persistence)?
    .ok_or(PluginLifecycleMutationError::PackageNotFound)?;
    decode_package(
        PersistedPackage {
            publisher_id: row
                .try_get("publisher_id")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            compatibility_range: row
                .try_get("compatibility_range")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            declared_capabilities_json: row
                .try_get("declared_capabilities_json")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            permission_request_json: row
                .try_get("permission_request_json")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            verification_evidence_json: row
                .try_get("verification_evidence_json")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            lifecycle_state: row
                .try_get("lifecycle_state")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            revoked_reason_code: row
                .try_get("revoked_reason_code")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
        },
        identity,
    )
}

async fn load_upgrade_source(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: &TenantId,
    executable: &PluginExecutableRef,
) -> Result<PluginPackageRecord, PluginLifecycleMutationError> {
    executable
        .validate()
        .map_err(|_| PluginLifecycleMutationError::InvalidActivation)?;
    let row = sqlx::query(
        "SELECT p.publisher_id,p.capability_contract_version,p.compatibility_range,\
                p.declared_capabilities_json,p.permission_request_json,p.verification_evidence_json,\
                p.lifecycle_state,p.revoked_reason_code,i.lifecycle_state AS installation_state \
         FROM plugin_installations i JOIN plugin_packages p \
           ON p.plugin_id=i.plugin_id AND p.plugin_version=i.plugin_version \
          AND p.package_digest_sha256=i.package_digest_sha256 \
          AND p.manifest_digest_sha256=i.manifest_digest_sha256 \
         WHERE i.tenant_id=$1 AND i.plugin_id=$2 AND i.plugin_version=$3 \
           AND i.package_digest_sha256=$4 AND i.manifest_digest_sha256=$5 \
           AND p.capability_contract_version=$6 FOR UPDATE OF i,p",
    )
    .bind(tenant_id.as_str())
    .bind(executable.plugin_id.as_str())
    .bind(executable.version.as_str())
    .bind(&executable.package_digest_sha256)
    .bind(&executable.manifest_digest_sha256)
    .bind(executable.capability_contract_version.as_str())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| PluginLifecycleMutationError::Persistence)?
    .ok_or(PluginLifecycleMutationError::UpgradeSourceNotFound)?;

    let installation_state: String = row
        .try_get("installation_state")
        .map_err(|_| PluginLifecycleMutationError::Persistence)?;
    let package_state: String = row
        .try_get("lifecycle_state")
        .map_err(|_| PluginLifecycleMutationError::Persistence)?;
    if !matches!(installation_state.as_str(), "active" | "suspended")
        || !matches!(package_state.as_str(), "active" | "suspended")
    {
        return Err(PluginLifecycleMutationError::UpgradeSourceNotExecutable);
    }
    let publisher: String = row
        .try_get("publisher_id")
        .map_err(|_| PluginLifecycleMutationError::Persistence)?;
    let capability_contract: String = row
        .try_get("capability_contract_version")
        .map_err(|_| PluginLifecycleMutationError::Persistence)?;
    let identity = package_identity_from_executable(
        executable,
        PublisherId::new(publisher.clone())
            .map_err(|_| PluginLifecycleMutationError::InvalidPersistedRecord)?,
    )?;
    if identity.capability_contract_version.as_str() != capability_contract {
        return Err(PluginLifecycleMutationError::InvalidPersistedRecord);
    }
    decode_package(
        PersistedPackage {
            publisher_id: publisher,
            compatibility_range: row
                .try_get("compatibility_range")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            declared_capabilities_json: row
                .try_get("declared_capabilities_json")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            permission_request_json: row
                .try_get("permission_request_json")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            verification_evidence_json: row
                .try_get("verification_evidence_json")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
            lifecycle_state: package_state,
            revoked_reason_code: row
                .try_get("revoked_reason_code")
                .map_err(|_| PluginLifecycleMutationError::Persistence)?,
        },
        &identity,
    )
}

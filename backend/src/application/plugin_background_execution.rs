#![cfg(feature = "postgres")]

//! R4-P6 re-authorization gate for claimed durable plugin background work.
//!
//! P3 owns claim/lease/fencing and immutable executable pins. P6 does not
//! duplicate that state machine: immediately before plugin code executes, this
//! gate proves that the supplied `WorkClaim` is still the current unexpired
//! leased generation, reloads the exact executable pin from durable storage,
//! performs a fresh persistence-bound plugin admission and re-authorizes the
//! Work subject. Revocation, policy changes, lease expiry, executable rebinding
//! or a cross-runtime token therefore stop execution before plugin code runs.

use sqlx::Row;
use system_core::ExecutionContext;
use system_core::security::plugin::PluginPermission;
use system_core::transport::interconnect::{
    BindingRevisionRef, ContractVersion, PluginExecutableRef, PluginId, ProviderInstanceRef,
    Subject, WorkClaim,
};

use super::plugin_admission::{PluginAdmissionPolicies, PluginAdmissionServiceError};
use super::plugin_execution_admission::{
    PluginExecutionAdmission, PluginExecutionAdmissionError, PluginExecutionRuntimeBinding,
};
use super::plugin_execution_admission_service::PostgresPluginAdmissionService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginBackgroundExecutionError {
    TenantScopeMissing,
    StaleExpiredOrUnpinnedClaim,
    InvalidPersistedPin,
    Admission(PluginAdmissionServiceError),
    Authorization(PluginExecutionAdmissionError),
    Persistence,
}

impl From<PluginAdmissionServiceError> for PluginBackgroundExecutionError {
    fn from(value: PluginAdmissionServiceError) -> Self {
        Self::Admission(value)
    }
}

impl From<PluginExecutionAdmissionError> for PluginBackgroundExecutionError {
    fn from(value: PluginExecutionAdmissionError) -> Self {
        Self::Authorization(value)
    }
}

/// Host-owned pre-execution gate for PostgreSQL-backed plugin Work. The
/// production constructor is crate-private and receives the runtime binding from
/// complete runtime composition. The caller supplies only the trusted
/// ExecutionContext and a P3 WorkClaim; executable identity, subject and lease
/// freshness are resolved from host-owned durable state. PostgreSQL server time
/// is the lease clock, so a caller cannot regain expired authority by supplying
/// a stale timestamp.
pub struct PostgresPluginBackgroundExecutionGate {
    pool: sqlx::PgPool,
    admission: PostgresPluginAdmissionService,
    runtime_binding: PluginExecutionRuntimeBinding,
}

impl PostgresPluginBackgroundExecutionGate {
    #[cfg(test)]
    pub(crate) fn new(pool: sqlx::PgPool, policies: PluginAdmissionPolicies) -> Self {
        Self::new_bound(pool, policies, PluginExecutionRuntimeBinding::new())
    }

    pub(crate) fn new_bound(
        pool: sqlx::PgPool,
        policies: PluginAdmissionPolicies,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        Self {
            admission: PostgresPluginAdmissionService::new(
                pool.clone(),
                policies,
                runtime_binding.clone(),
            ),
            pool,
            runtime_binding,
        }
    }

    pub async fn reauthorize_claim(
        &self,
        context: &ExecutionContext,
        claim: &WorkClaim,
    ) -> Result<PluginExecutionAdmission, PluginBackgroundExecutionError> {
        let tenant_id = context
            .data_scope()
            .tenant_id_opt()
            .ok_or(PluginBackgroundExecutionError::TenantScopeMissing)?;
        let generation = i64::try_from(claim.claim_generation)
            .map_err(|_| PluginBackgroundExecutionError::InvalidPersistedPin)?;
        let lease_deadline = i64::try_from(claim.lease_deadline_ms)
            .map_err(|_| PluginBackgroundExecutionError::InvalidPersistedPin)?;

        // The JOIN makes a generic/non-plugin Work row ineligible. Matching the
        // current generation, owner and exact lease deadline preserves P3
        // fencing. Lease freshness is evaluated by PostgreSQL `clock_timestamp`
        // in the same authority query, not by caller-selected application time.
        let row = sqlx::query(
            "SELECT w.subject, p.plugin_id, p.plugin_version, \
                    p.package_digest_sha256, p.manifest_digest_sha256, \
                    p.capability_contract_version, p.provider_instance_id, p.binding_revision \
             FROM interconnect_work w \
             INNER JOIN interconnect_plugin_work_pins p \
               ON p.work_id=w.work_id AND p.tenant_id=w.tenant_id \
             WHERE w.work_id=$1 AND w.tenant_id=$2 AND w.status='leased' \
               AND w.claim_generation=$3 AND w.lease_owner=$4 \
               AND w.lease_deadline_ms=$5 \
               AND w.lease_deadline_ms > \
                   (EXTRACT(EPOCH FROM clock_timestamp()) * 1000)::BIGINT",
        )
        .bind(claim.work_id.as_str())
        .bind(tenant_id.as_str())
        .bind(generation)
        .bind(claim.lease_owner.as_str())
        .bind(lease_deadline)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| PluginBackgroundExecutionError::Persistence)?
        .ok_or(PluginBackgroundExecutionError::StaleExpiredOrUnpinnedClaim)?;

        let subject = Subject::new(
            row.try_get::<String, _>("subject")
                .map_err(|_| PluginBackgroundExecutionError::Persistence)?,
        )
        .map_err(|_| PluginBackgroundExecutionError::InvalidPersistedPin)?;
        if subject.is_reserved() {
            return Err(PluginBackgroundExecutionError::InvalidPersistedPin);
        }

        let provider_instance_id: Option<String> = row
            .try_get("provider_instance_id")
            .map_err(|_| PluginBackgroundExecutionError::Persistence)?;
        let binding_revision: Option<String> = row
            .try_get("binding_revision")
            .map_err(|_| PluginBackgroundExecutionError::Persistence)?;
        let (provider_instance_id, binding_revision) =
            match (provider_instance_id, binding_revision) {
                (None, None) => (None, None),
                (Some(provider), Some(revision)) => (
                    Some(
                        ProviderInstanceRef::new(provider)
                            .map_err(|_| PluginBackgroundExecutionError::InvalidPersistedPin)?,
                    ),
                    Some(
                        BindingRevisionRef::new(revision)
                            .map_err(|_| PluginBackgroundExecutionError::InvalidPersistedPin)?,
                    ),
                ),
                _ => return Err(PluginBackgroundExecutionError::InvalidPersistedPin),
            };

        let executable = PluginExecutableRef {
            plugin_id: PluginId::new(
                row.try_get::<String, _>("plugin_id")
                    .map_err(|_| PluginBackgroundExecutionError::Persistence)?,
            )
            .map_err(|_| PluginBackgroundExecutionError::InvalidPersistedPin)?,
            version: ContractVersion::new(
                row.try_get::<String, _>("plugin_version")
                    .map_err(|_| PluginBackgroundExecutionError::Persistence)?,
            )
            .map_err(|_| PluginBackgroundExecutionError::InvalidPersistedPin)?,
            package_digest_sha256: row
                .try_get("package_digest_sha256")
                .map_err(|_| PluginBackgroundExecutionError::Persistence)?,
            manifest_digest_sha256: row
                .try_get("manifest_digest_sha256")
                .map_err(|_| PluginBackgroundExecutionError::Persistence)?,
            capability_contract_version: ContractVersion::new(
                row.try_get::<String, _>("capability_contract_version")
                    .map_err(|_| PluginBackgroundExecutionError::Persistence)?,
            )
            .map_err(|_| PluginBackgroundExecutionError::InvalidPersistedPin)?,
            provider_instance_id,
            binding_revision,
        };
        executable
            .validate()
            .map_err(|_| PluginBackgroundExecutionError::InvalidPersistedPin)?;

        let admission = self.admission.admit(context, &executable).await?;
        admission.require_runtime(&self.runtime_binding)?;
        admission.authorize(
            context,
            &PluginPermission::BackgroundJob(subject.as_str().to_owned()),
        )?;
        Ok(admission)
    }
}

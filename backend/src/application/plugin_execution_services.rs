//! Context- and runtime-bound public plugin operation facades for R4-P6.
//!
//! Low-level egress, secret and storage adapters continue to operate on the
//! crate-private host authority token. Public operations accept only
//! `PluginExecutionAdmission`; every call re-checks both the exact originating
//! `ExecutionContext` and the process-local runtime instance that minted it.

use std::sync::Arc;

use system_core::ExecutionContext;
use system_core::security::plugin::PluginPermission;

use crate::integration::egress::{DestinationPolicy, EgressEvidenceSink};
use crate::integration::transport::{ExternalRequest, ExternalResponse};

use super::plugin_egress::{
    PluginEgressBudgetRegistry, PluginEgressError, PluginEgressExecutor, PluginEgressGrantPolicy,
    PluginSecretNetworkPolicy,
};
use super::plugin_execution_admission::{
    PluginExecutionAdmission, PluginExecutionAdmissionError, PluginExecutionRuntimeBinding,
};
use super::plugin_secret::{
    PluginSecretBackend, PluginSecretError, PluginSecretPolicy, PluginSecretService,
};
use super::plugin_storage::{PluginStorageEntry, PluginStorageError, PluginStoragePolicy};

#[derive(Debug)]
pub enum PluginExecutionEgressError {
    Admission(PluginExecutionAdmissionError),
    SecretPurposeRequired,
    AmbiguousSecretPurpose,
    Egress(PluginEgressError),
}

impl From<PluginExecutionAdmissionError> for PluginExecutionEgressError {
    fn from(value: PluginExecutionAdmissionError) -> Self {
        Self::Admission(value)
    }
}

impl From<PluginEgressError> for PluginExecutionEgressError {
    fn from(value: PluginEgressError) -> Self {
        Self::Egress(value)
    }
}

/// Only public plugin egress surface. The plugin supplies only a grant ID; the
/// complete P5 destination/path/protocol/budget authority is resolved from the
/// host-owned immutable grant policy after runtime/context admission checks.
/// Request method/path/headers/body remain per-call protocol data and are
/// constrained by that P5 grant. Correlation identity is overwritten from the
/// trusted ExecutionContext before governed dispatch.
///
/// R4 deliberately has no byte-level taint/provenance tracker. If an admission
/// carries one SecretPurpose authority, every network operation must therefore
/// declare that purpose and cross the exact secret/network binding. If an
/// admission carries more than one SecretPurpose, network egress fails closed:
/// ordinary bytes returned by a signing operation cannot prove which purpose
/// produced them, so caller-selected relabelling would be ambiguous.
pub struct PluginExecutionEgressService {
    inner: PluginEgressExecutor,
    runtime_binding: PluginExecutionRuntimeBinding,
}

impl PluginExecutionEgressService {
    pub(crate) fn new(
        budgets: Arc<PluginEgressBudgetRegistry>,
        grant_policy: Arc<PluginEgressGrantPolicy>,
        secret_network_policy: Arc<PluginSecretNetworkPolicy>,
        destination_policy: DestinationPolicy,
        evidence: Arc<dyn EgressEvidenceSink>,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        Self {
            inner: PluginEgressExecutor::new(
                budgets,
                grant_policy,
                secret_network_policy,
                destination_policy,
                evidence,
            ),
            runtime_binding,
        }
    }

    pub async fn send(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        grant_id: &str,
        secret_purpose: Option<&str>,
        mut request: ExternalRequest,
    ) -> Result<ExternalResponse, PluginExecutionEgressError> {
        admission.require_runtime(&self.runtime_binding)?;
        admission.require_context(context)?;
        request.correlation_id = context.correlation_id().as_str().to_owned();
        let secret_purpose_count = admission
            .permissions()
            .effective
            .iter()
            .filter(|permission| matches!(permission, PluginPermission::SecretPurpose(_)))
            .count();
        if secret_purpose_count > 1 {
            return Err(PluginExecutionEgressError::AmbiguousSecretPurpose);
        }
        if secret_purpose_count == 1 && secret_purpose.is_none() {
            return Err(PluginExecutionEgressError::SecretPurposeRequired);
        }
        self.inner
            .send(admission.authority(), grant_id, secret_purpose, request)
            .await
            .map_err(Into::into)
    }
}

#[derive(Debug)]
pub enum PluginExecutionSecretError {
    Admission(PluginExecutionAdmissionError),
    Secret(PluginSecretError),
}

impl From<PluginExecutionAdmissionError> for PluginExecutionSecretError {
    fn from(value: PluginExecutionAdmissionError) -> Self {
        Self::Admission(value)
    }
}

impl From<PluginSecretError> for PluginExecutionSecretError {
    fn from(value: PluginSecretError) -> Self {
        Self::Secret(value)
    }
}

/// Purpose-bound non-exportable secret operations. R4 exposes signing only.
/// Decrypt remains an internal/future operation because returning plaintext to
/// plugin code would defeat secret+network anti-exfiltration pairing: once raw
/// plaintext leaves the host boundary, a later network call cannot reliably
/// prove that its bytes contain a secret.
pub struct PluginExecutionSecretService {
    inner: PluginSecretService,
    runtime_binding: PluginExecutionRuntimeBinding,
}

impl PluginExecutionSecretService {
    pub(crate) fn new(
        policy: Arc<PluginSecretPolicy>,
        backend: Arc<dyn PluginSecretBackend>,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        Self {
            inner: PluginSecretService::new(policy, backend),
            runtime_binding,
        }
    }

    pub fn sign(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        purpose: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, PluginExecutionSecretError> {
        admission.require_runtime(&self.runtime_binding)?;
        admission.require_context(context)?;
        self.inner
            .sign(admission.authority(), purpose, payload)
            .map_err(Into::into)
    }
}

#[derive(Debug)]
pub enum PluginExecutionStorageError {
    Admission(PluginExecutionAdmissionError),
    Storage(PluginStorageError),
}

impl From<PluginExecutionAdmissionError> for PluginExecutionStorageError {
    fn from(value: PluginExecutionAdmissionError) -> Self {
        Self::Admission(value)
    }
}

impl From<PluginStorageError> for PluginExecutionStorageError {
    fn from(value: PluginStorageError) -> Self {
        Self::Storage(value)
    }
}

#[cfg(feature = "sqlite")]
pub struct SqlitePluginExecutionStorageService {
    inner: super::plugin_storage::SqlitePluginStorageService,
    runtime_binding: PluginExecutionRuntimeBinding,
}

#[cfg(feature = "sqlite")]
impl SqlitePluginExecutionStorageService {
    pub(crate) fn new(
        pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
        policy: Arc<PluginStoragePolicy>,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        Self {
            inner: super::plugin_storage::SqlitePluginStorageService::new(pool, policy),
            runtime_binding,
        }
    }

    pub fn put(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        namespace: &str,
        key: &str,
        value: &[u8],
    ) -> Result<PluginStorageEntry, PluginExecutionStorageError> {
        admission.require_runtime(&self.runtime_binding)?;
        admission.require_context(context)?;
        self.inner
            .put(admission.authority(), namespace, key, value)
            .map_err(Into::into)
    }

    pub fn get(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        namespace: &str,
        key: &str,
    ) -> Result<Option<PluginStorageEntry>, PluginExecutionStorageError> {
        admission.require_runtime(&self.runtime_binding)?;
        admission.require_context(context)?;
        self.inner
            .get(admission.authority(), namespace, key)
            .map_err(Into::into)
    }
}

#[cfg(feature = "postgres")]
pub struct PostgresPluginExecutionStorageService {
    inner: super::plugin_storage::PostgresPluginStorageService,
    runtime_binding: PluginExecutionRuntimeBinding,
}

#[cfg(feature = "postgres")]
impl PostgresPluginExecutionStorageService {
    pub(crate) fn new(
        pool: sqlx::PgPool,
        policy: Arc<PluginStoragePolicy>,
        runtime_binding: PluginExecutionRuntimeBinding,
    ) -> Self {
        Self {
            inner: super::plugin_storage::PostgresPluginStorageService::new(pool, policy),
            runtime_binding,
        }
    }

    pub async fn put(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        namespace: &str,
        key: &str,
        value: &[u8],
    ) -> Result<PluginStorageEntry, PluginExecutionStorageError> {
        admission.require_runtime(&self.runtime_binding)?;
        admission.require_context(context)?;
        self.inner
            .put(admission.authority(), namespace, key, value)
            .await
            .map_err(Into::into)
    }

    pub async fn get(
        &self,
        context: &ExecutionContext,
        admission: &PluginExecutionAdmission,
        namespace: &str,
        key: &str,
    ) -> Result<Option<PluginStorageEntry>, PluginExecutionStorageError> {
        admission.require_runtime(&self.runtime_binding)?;
        admission.require_context(context)?;
        self.inner
            .get(admission.authority(), namespace, key)
            .await
            .map_err(Into::into)
    }
}

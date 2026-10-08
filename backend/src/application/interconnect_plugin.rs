//! Plugin-backed Work admission with immutable executable pinning.
//!
//! Generic Core work does not require a plugin executable. Any plugin runtime
//! path, however, must enter through this wrapper so queued work is bound to the
//! exact package/manifest/version/capability/provider-binding facts that were
//! admitted. A later plugin upgrade cannot silently retarget already-admitted
//! work.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use system_core::ExecutionContext;
use system_core::transport::interconnect::{
    InterconnectError, InterconnectErrorCode, MessageEnvelope, MessageId, PluginExecutableRef,
};

use super::interconnect::InProcessDriver;
#[cfg(feature = "postgres")]
use super::interconnect::postgres::PostgresDurableDriver;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginWorkAdmission {
    pub envelope: MessageEnvelope,
    pub executable: PluginExecutableRef,
    pub retry_budget: u32,
    pub available_at_ms: u64,
}

impl PluginWorkAdmission {
    pub fn validate(&self) -> Result<(), InterconnectError> {
        self.envelope.validate()?;
        self.executable.validate()?;
        if self.executable.provider_instance_id.is_some()
            != self.executable.binding_revision.is_some()
        {
            return Err(InterconnectError::new(
                InterconnectErrorCode::ContractIncompatible,
                "provider-backed plugin executable pin requires provider instance and binding revision together",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct InProcessPluginWorkQueue {
    driver: InProcessDriver,
    pins: Arc<Mutex<HashMap<String, PluginExecutableRef>>>,
}

impl InProcessPluginWorkQueue {
    pub fn new(driver: InProcessDriver) -> Self {
        Self {
            driver,
            pins: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn enqueue(
        &self,
        ctx: &ExecutionContext,
        admission: PluginWorkAdmission,
    ) -> Result<MessageId, InterconnectError> {
        admission.validate()?;
        let work_id = admission.envelope.id.as_str().to_owned();
        let inserted_pin = {
            let mut pins = self.pins.lock().map_err(|_| {
                InterconnectError::new(
                    InterconnectErrorCode::DriverFailure,
                    "plugin work pin map lock poisoned",
                )
            })?;
            if let Some(existing) = pins.get(&work_id) {
                if existing != &admission.executable {
                    return Err(pin_conflict());
                }
                false
            } else {
                pins.insert(work_id.clone(), admission.executable.clone());
                true
            }
        };
        match self
            .driver
            .enqueue_work(ctx, admission.envelope, admission.retry_budget)
        {
            Ok(id) => Ok(id),
            Err(error) => {
                if inserted_pin {
                    let mut pins = self.pins.lock().map_err(|_| {
                        InterconnectError::new(
                            InterconnectErrorCode::DriverFailure,
                            "plugin work pin map lock poisoned",
                        )
                    })?;
                    pins.remove(&work_id);
                }
                Err(error)
            }
        }
    }

    pub fn pinned_executable(
        &self,
        work_id: &MessageId,
    ) -> Result<Option<PluginExecutableRef>, InterconnectError> {
        Ok(self
            .pins
            .lock()
            .map_err(|_| {
                InterconnectError::new(
                    InterconnectErrorCode::DriverFailure,
                    "plugin work pin map lock poisoned",
                )
            })?
            .get(work_id.as_str())
            .cloned())
    }
}

#[cfg(feature = "postgres")]
pub async fn enqueue_postgres_plugin_work(
    driver: &PostgresDurableDriver,
    ctx: &ExecutionContext,
    admission: PluginWorkAdmission,
) -> Result<MessageId, InterconnectError> {
    use sqlx::Row;

    admission.validate()?;
    let tenant = ctx
        .data_scope()
        .tenant_id_opt()
        .map(|value| value.as_str())
        .ok_or_else(|| {
            InterconnectError::new(
                InterconnectErrorCode::PolicyDenied,
                "plugin work requires a trusted tenant DataScope",
            )
        })?;

    let mut tx = driver.pool().begin().await.map_err(pg_error)?;
    let (work_id, work_inserted) = driver
        .enqueue_work_in_transaction(
            &mut tx,
            ctx,
            &admission.envelope,
            admission.retry_budget,
            admission.available_at_ms,
        )
        .await?;

    if work_inserted {
        let orphan_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM interconnect_plugin_work_pins WHERE work_id=$1)",
        )
        .bind(work_id.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(pg_error)?;
        if orphan_exists {
            return Err(pin_conflict());
        }

        sqlx::query(
            "INSERT INTO interconnect_plugin_work_pins \
             (work_id,tenant_id,plugin_id,plugin_version,package_digest_sha256,manifest_digest_sha256,capability_contract_version,provider_instance_id,binding_revision) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(work_id.as_str())
        .bind(tenant)
        .bind(admission.executable.plugin_id.as_str())
        .bind(admission.executable.version.as_str())
        .bind(&admission.executable.package_digest_sha256)
        .bind(&admission.executable.manifest_digest_sha256)
        .bind(admission.executable.capability_contract_version.as_str())
        .bind(
            admission
                .executable
                .provider_instance_id
                .as_ref()
                .map(|value| value.as_str()),
        )
        .bind(
            admission
                .executable
                .binding_revision
                .as_ref()
                .map(|value| value.as_str()),
        )
        .execute(&mut *tx)
        .await
        .map_err(pg_error)?;
    } else {
        let row = sqlx::query(
            "SELECT tenant_id,plugin_id,plugin_version,package_digest_sha256,manifest_digest_sha256,capability_contract_version,provider_instance_id,binding_revision \
             FROM interconnect_plugin_work_pins WHERE work_id=$1 FOR UPDATE",
        )
        .bind(work_id.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(pg_error)?
        .ok_or_else(pin_conflict)?;

        let matches = row.try_get::<String, _>("tenant_id").map_err(pg_error)? == tenant
            && row.try_get::<String, _>("plugin_id").map_err(pg_error)?
                == admission.executable.plugin_id.as_str()
            && row
                .try_get::<String, _>("plugin_version")
                .map_err(pg_error)?
                == admission.executable.version.as_str()
            && row
                .try_get::<String, _>("package_digest_sha256")
                .map_err(pg_error)?
                == admission.executable.package_digest_sha256
            && row
                .try_get::<String, _>("manifest_digest_sha256")
                .map_err(pg_error)?
                == admission.executable.manifest_digest_sha256
            && row
                .try_get::<String, _>("capability_contract_version")
                .map_err(pg_error)?
                == admission.executable.capability_contract_version.as_str()
            && row
                .try_get::<Option<String>, _>("provider_instance_id")
                .map_err(pg_error)?
                .as_deref()
                == admission
                    .executable
                    .provider_instance_id
                    .as_ref()
                    .map(|value| value.as_str())
            && row
                .try_get::<Option<String>, _>("binding_revision")
                .map_err(pg_error)?
                .as_deref()
                == admission
                    .executable
                    .binding_revision
                    .as_ref()
                    .map(|value| value.as_str());
        if !matches {
            return Err(pin_conflict());
        }
    }

    tx.commit().await.map_err(pg_error)?;
    Ok(work_id)
}

fn pin_conflict() -> InterconnectError {
    InterconnectError::new(
        InterconnectErrorCode::ContractIncompatible,
        "plugin-backed work id is already pinned to a different executable/package identity or was admitted without a plugin pin",
    )
}

#[cfg(feature = "postgres")]
fn pg_error(error: impl std::fmt::Display) -> InterconnectError {
    InterconnectError::new(
        InterconnectErrorCode::DriverFailure,
        format!("PostgreSQL plugin executable pin failure: {error}"),
    )
    .retryable(true)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::Value;
    use system_core::transport::interconnect::{
        BindingRevisionRef, ContractBinding, ContractRef, ContractVersion, CorrelationId,
        Extensions, MessageKind, PayloadRef, PluginId, ProviderInstanceRef, SchemaRef, Subject,
    };
    use system_core::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode,
        NoopHttpClient, RequestId, Revision, TenantId, TenantMembershipId, TenantRole, TenantScope,
    };

    use super::*;

    fn ctx() -> ExecutionContext {
        let tenant = TenantId::new("tenant-a").unwrap();
        ExecutionContext::new(
            ActorIdentity::with_authority(
                "staff-a",
                AuthorityContext::Tenant {
                    membership_id: TenantMembershipId::new("membership-a").unwrap(),
                    tenant_id: tenant.clone(),
                    role: TenantRole::Staff,
                },
            )
            .unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("rev-1").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new("corr-a").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn admission(package_byte: &str) -> PluginWorkAdmission {
        PluginWorkAdmission {
            envelope: MessageEnvelope {
                id: MessageId::new("plugin-work-1").unwrap(),
                kind: MessageKind::Work,
                subject: Subject::new("logistics.shipment.query").unwrap(),
                contract: ContractBinding {
                    contract: ContractRef::new("logistics.shipment.query").unwrap(),
                    version: ContractVersion::new("1.0.0").unwrap(),
                    schema: SchemaRef::new("logistics.shipment.query.v1").unwrap(),
                },
                correlation_id: CorrelationId::new("corr-a").unwrap(),
                causation_id: None,
                created_at_ms: 10,
                deadline_ms: None,
                ordering_key: None,
                idempotency_key: None,
                payload: PayloadRef::Inline(Value::Null),
                extensions: Extensions::empty(),
            },
            executable: PluginExecutableRef {
                plugin_id: PluginId::new("official.sf-express").unwrap(),
                version: ContractVersion::new("1.0.0").unwrap(),
                package_digest_sha256: package_byte.repeat(64),
                manifest_digest_sha256: "b".repeat(64),
                capability_contract_version: ContractVersion::new("1.0.0").unwrap(),
                provider_instance_id: Some(ProviderInstanceRef::new("provider-a").unwrap()),
                binding_revision: Some(BindingRevisionRef::new("binding-rev-1").unwrap()),
            },
            retry_budget: 1,
            available_at_ms: 10,
        }
    }

    #[test]
    fn in_process_plugin_work_is_immutably_pinned() {
        let queue = InProcessPluginWorkQueue::new(InProcessDriver::default());
        let first = admission("a");
        let work_id = queue.enqueue(&ctx(), first.clone()).unwrap();
        assert_eq!(
            queue.pinned_executable(&work_id).unwrap(),
            Some(first.executable)
        );
        assert_eq!(
            queue.enqueue(&ctx(), admission("c")).unwrap_err().code,
            InterconnectErrorCode::ContractIncompatible
        );
    }

    #[test]
    fn provider_backed_plugin_pin_requires_instance_and_binding_revision_together() {
        let mut missing_binding = admission("a");
        missing_binding.executable.binding_revision = None;
        assert_eq!(
            missing_binding.validate().unwrap_err().code,
            InterconnectErrorCode::ContractIncompatible
        );

        let mut missing_instance = admission("a");
        missing_instance.executable.provider_instance_id = None;
        assert_eq!(
            missing_instance.validate().unwrap_err().code,
            InterconnectErrorCode::ContractIncompatible
        );

        let mut provider_neutral = admission("a");
        provider_neutral.executable.provider_instance_id = None;
        provider_neutral.executable.binding_revision = None;
        assert!(provider_neutral.validate().is_ok());
    }
}

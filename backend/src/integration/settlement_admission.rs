//! Narrow R3 settlement admission seam owned by the Integration Fabric.
//!
//! This is intentionally not a general-purpose external-operation constructor.
//! It derives a charge operation from already-authorized settlement facts while
//! holding the caller's SQLite transaction, so an R3 admission link can never
//! be committed without its R2 `ExternalOperation` identity.

use rusqlite::{OptionalExtension, Transaction, params};
#[cfg(feature = "postgres")]
use sqlx::Row;
use uuid::Uuid;

use super::operation::{EffectIntent, OperationState};

pub(crate) const SETTLEMENT_CHARGE_CAPABILITY: &str = "payment.charge";

#[derive(Debug, Clone)]
pub(crate) struct SettlementChargeAdmission<'a> {
    pub tenant_id: &'a str,
    pub settlement_case_id: &'a str,
    pub order_id: &'a str,
    pub amount_minor: i64,
    pub currency: &'a str,
    pub business_authorization_ref: &'a str,
    pub idempotency_key: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct CanonicalSettlementChargeAdmission {
    pub external_operation_id: String,
    pub request_hash: String,
    /// `true` only when this transaction inserted the canonical R2 operation.
    /// The caller must wait for its enclosing transaction to commit before
    /// emitting the one admission metric.
    pub created_new: bool,
}

/// Create (or return) the one canonical R2 charge operation for an accepted
/// R3 settlement authorization.  The provider binding is resolved from the
/// durable Fabric tables; no route or R3 caller can provide an operation ID,
/// binding revision, capability, or request hash.
pub(crate) fn admit_settlement_charge_tx(
    tx: &Transaction<'_>,
    admission: &SettlementChargeAdmission<'_>,
) -> Result<CanonicalSettlementChargeAdmission, String> {
    if admission.tenant_id.trim().is_empty()
        || admission.settlement_case_id.trim().is_empty()
        || admission.order_id.trim().is_empty()
        || admission.business_authorization_ref.trim().is_empty()
        || admission.idempotency_key.trim().is_empty()
        || admission.amount_minor <= 0
        || admission.currency.len() != 3
    {
        return Err("invalid R3 settlement charge admission facts".into());
    }

    // The R2 runtime only dispatches fixture-ready bindings in this release.
    // Keep this same readiness/binding-revision predicate at admission time so
    // a charge cannot be admitted against a stale or disabled routing record.
    let binding: Option<(String, String, String, String, String, String)> = tx
        .query_row(
            "SELECT i.id, b.id, b.config_revision, m.capabilities_json,
                    m.secret_schema_json, i.secret_refs_json
             FROM provider_bindings b
             JOIN provider_instances i
               ON i.tenant_id=b.tenant_id AND i.id=b.provider_instance_id
             JOIN provider_manifests m
               ON m.provider_id=i.provider_id AND m.version=i.manifest_version
             WHERE b.tenant_id=?1
               AND b.capability_id=?2
               AND b.enabled=1
               AND b.config_revision=i.config_revision
               AND i.lifecycle='active'
               AND i.health='ready'
               AND i.readiness='fixture'
               AND m.readiness='fixture'
             ORDER BY b.id
             LIMIT 1",
            params![admission.tenant_id, SETTLEMENT_CHARGE_CAPABILITY],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some((
        provider_instance_id,
        binding_id,
        binding_revision,
        capabilities_json,
        secret_schema_json,
        secret_refs_json,
    )) = binding
    else {
        return Err("R2 Integration Fabric has no active fixture-ready payment.charge binding for this tenant".into());
    };
    let capabilities: Vec<String> = serde_json::from_str(&capabilities_json)
        .map_err(|_| "R2 provider manifest capabilities are invalid".to_owned())?;
    if !capabilities
        .iter()
        .any(|capability| capability == SETTLEMENT_CHARGE_CAPABILITY)
    {
        return Err("R2 provider manifest does not authorize payment.charge".into());
    }
    assert_required_secret_references(&secret_schema_json, &secret_refs_json)?;

    let operation_idempotency_key = format!(
        "r3-settlement-charge:{}:{}",
        admission.settlement_case_id, admission.idempotency_key
    );
    let request_fingerprint = serde_json::to_string(&serde_json::json!({
        "kind": "r3_settlement_charge",
        "settlementCaseId": admission.settlement_case_id,
        "orderId": admission.order_id,
        "amountMinor": admission.amount_minor,
        "currency": admission.currency,
        "businessAuthorizationRef": admission.business_authorization_ref,
    }))
    .map_err(|_| "could not encode canonical R3 settlement facts".to_owned())?;
    let intent = EffectIntent {
        tenant_id: admission.tenant_id.to_owned(),
        provider_instance_id: provider_instance_id.clone(),
        binding_id: binding_id.clone(),
        binding_revision: binding_revision.clone(),
        capability: SETTLEMENT_CHARGE_CAPABILITY.to_owned(),
        operation_type: "charge".into(),
        idempotency_key: operation_idempotency_key.clone(),
        request_fingerprint,
    };
    let request_hash = intent
        .canonical_hash()
        .map_err(|_| "could not derive R2 EffectIntent hash".to_owned())?;

    let existing: Option<(String, String)> = tx
        .query_row(
            "SELECT id, request_hash FROM external_operations
             WHERE tenant_id=?1 AND binding_id=?2 AND idempotency_key=?3",
            params![admission.tenant_id, binding_id, operation_idempotency_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((external_operation_id, existing_hash)) = existing {
        if existing_hash != request_hash {
            return Err(
                "R2 ExternalOperation idempotency key was reused with different settlement facts"
                    .into(),
            );
        }
        return Ok(CanonicalSettlementChargeAdmission {
            external_operation_id,
            request_hash,
            created_new: false,
        });
    }

    let operation_id = Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO external_operations
         (id,tenant_id,provider_instance_id,binding_id,binding_revision,capability_id,
          operation_type,idempotency_key,request_hash,state,attempt_count,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,'charge',?7,?8,?9,0,?10,?10)",
        params![
            operation_id,
            admission.tenant_id,
            provider_instance_id,
            binding_id,
            binding_revision,
            SETTLEMENT_CHARGE_CAPABILITY,
            operation_idempotency_key,
            request_hash,
            state_value(&OperationState::Ready),
            timestamp,
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(CanonicalSettlementChargeAdmission {
        external_operation_id: operation_id,
        request_hash,
        created_new: true,
    })
}

#[cfg(feature = "postgres")]
pub(crate) async fn admit_settlement_charge_pg(
    connection: &mut sqlx::PgConnection,
    admission: &SettlementChargeAdmission<'_>,
) -> Result<CanonicalSettlementChargeAdmission, String> {
    if admission.tenant_id.trim().is_empty()
        || admission.settlement_case_id.trim().is_empty()
        || admission.order_id.trim().is_empty()
        || admission.business_authorization_ref.trim().is_empty()
        || admission.idempotency_key.trim().is_empty()
        || admission.amount_minor <= 0
        || admission.currency.len() != 3
    {
        return Err("invalid R3 settlement charge admission facts".into());
    }

    let binding = sqlx::query(
        "SELECT i.id AS provider_instance_id, b.id AS binding_id, b.config_revision, \
                m.capabilities_json, m.secret_schema_json, i.secret_refs_json \
         FROM provider_bindings b \
         JOIN provider_instances i \
           ON i.tenant_id=b.tenant_id AND i.id=b.provider_instance_id \
         JOIN provider_manifests m \
           ON m.provider_id=i.provider_id AND m.version=i.manifest_version \
         WHERE b.tenant_id=$1 \
           AND b.capability_id=$2 \
           AND b.enabled=TRUE \
           AND b.config_revision=i.config_revision \
           AND i.lifecycle='active' \
           AND i.health='ready' \
           AND i.readiness='fixture' \
           AND m.readiness='fixture' \
         ORDER BY b.id \
         LIMIT 1 \
         FOR SHARE OF b,i,m",
    )
    .bind(admission.tenant_id)
    .bind(SETTLEMENT_CHARGE_CAPABILITY)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| {
        "R2 Integration Fabric has no active fixture-ready payment.charge binding for this tenant"
            .to_owned()
    })?;

    let provider_instance_id: String = binding
        .try_get("provider_instance_id")
        .map_err(|error| error.to_string())?;
    let binding_id: String = binding
        .try_get("binding_id")
        .map_err(|error| error.to_string())?;
    let binding_revision: String = binding
        .try_get("config_revision")
        .map_err(|error| error.to_string())?;
    let capabilities_json: String = binding
        .try_get("capabilities_json")
        .map_err(|error| error.to_string())?;
    let secret_schema_json: String = binding
        .try_get("secret_schema_json")
        .map_err(|error| error.to_string())?;
    let secret_refs_json: String = binding
        .try_get("secret_refs_json")
        .map_err(|error| error.to_string())?;

    let capabilities: Vec<String> = serde_json::from_str(&capabilities_json)
        .map_err(|_| "R2 provider manifest capabilities are invalid".to_owned())?;
    if !capabilities
        .iter()
        .any(|capability| capability == SETTLEMENT_CHARGE_CAPABILITY)
    {
        return Err("R2 provider manifest does not authorize payment.charge".into());
    }
    assert_required_secret_references(&secret_schema_json, &secret_refs_json)?;

    let operation_idempotency_key = format!(
        "r3-settlement-charge:{}:{}",
        admission.settlement_case_id, admission.idempotency_key
    );
    let request_fingerprint = serde_json::to_string(&serde_json::json!({
        "kind": "r3_settlement_charge",
        "settlementCaseId": admission.settlement_case_id,
        "orderId": admission.order_id,
        "amountMinor": admission.amount_minor,
        "currency": admission.currency,
        "businessAuthorizationRef": admission.business_authorization_ref,
    }))
    .map_err(|_| "could not encode canonical R3 settlement facts".to_owned())?;
    let intent = EffectIntent {
        tenant_id: admission.tenant_id.to_owned(),
        provider_instance_id: provider_instance_id.clone(),
        binding_id: binding_id.clone(),
        binding_revision: binding_revision.clone(),
        capability: SETTLEMENT_CHARGE_CAPABILITY.to_owned(),
        operation_type: "charge".into(),
        idempotency_key: operation_idempotency_key.clone(),
        request_fingerprint,
    };
    let request_hash = intent
        .canonical_hash()
        .map_err(|_| "could not derive R2 EffectIntent hash".to_owned())?;

    let existing = sqlx::query(
        "SELECT id,request_hash FROM external_operations \
         WHERE tenant_id=$1 AND binding_id=$2 AND idempotency_key=$3 \
         FOR SHARE",
    )
    .bind(admission.tenant_id)
    .bind(&binding_id)
    .bind(&operation_idempotency_key)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| error.to_string())?;

    if let Some(existing) = existing {
        let external_operation_id: String =
            existing.try_get("id").map_err(|error| error.to_string())?;
        let existing_hash: String = existing
            .try_get("request_hash")
            .map_err(|error| error.to_string())?;
        if existing_hash != request_hash {
            return Err(
                "R2 ExternalOperation idempotency key was reused with different settlement facts"
                    .into(),
            );
        }
        return Ok(CanonicalSettlementChargeAdmission {
            external_operation_id,
            request_hash,
            created_new: false,
        });
    }

    let operation_id = Uuid::new_v4().to_string();
    let timestamp = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO external_operations \
         (id,tenant_id,provider_instance_id,binding_id,binding_revision,capability_id, \
          operation_type,idempotency_key,request_hash,state,attempt_count,created_at,updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,'charge',$7,$8,$9,0,$10,$10)",
    )
    .bind(&operation_id)
    .bind(admission.tenant_id)
    .bind(&provider_instance_id)
    .bind(&binding_id)
    .bind(&binding_revision)
    .bind(SETTLEMENT_CHARGE_CAPABILITY)
    .bind(&operation_idempotency_key)
    .bind(&request_hash)
    .bind(state_value(&OperationState::Ready))
    .bind(&timestamp)
    .execute(&mut *connection)
    .await
    .map_err(|error| error.to_string())?;

    Ok(CanonicalSettlementChargeAdmission {
        external_operation_id: operation_id,
        request_hash,
        created_new: true,
    })
}

/// Admission has no access to secret values.  It nevertheless enforces the
/// same manifest/instance prerequisite as the R2 catalog: every required
/// secret declaration must have a durable reference before a charge can exist.
fn assert_required_secret_references(
    secret_schema_json: &str,
    secret_refs_json: &str,
) -> Result<(), String> {
    let schema: Vec<serde_json::Value> = serde_json::from_str(secret_schema_json)
        .map_err(|_| "R2 provider manifest secret schema is invalid".to_owned())?;
    let refs = serde_json::from_str::<serde_json::Value>(secret_refs_json)
        .map_err(|_| "R2 provider instance secret references are invalid".to_owned())?;
    let refs = refs
        .as_object()
        .ok_or_else(|| "R2 provider instance secret references are invalid".to_owned())?;
    for requirement in schema {
        if requirement
            .get("required")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            let name = requirement
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| "R2 provider manifest secret schema is invalid".to_owned())?;
            if !refs.contains_key(name) {
                return Err("R2 provider instance is missing a required secret reference".into());
            }
        }
    }
    Ok(())
}

fn state_value(state: &OperationState) -> &'static str {
    match state {
        OperationState::Planned => "planned",
        OperationState::Ready => "ready",
        OperationState::Dispatching => "dispatching",
        OperationState::Succeeded => "succeeded",
        OperationState::Rejected => "rejected",
        OperationState::RetryableFailure => "retryable_failure",
        OperationState::NonRetryableFailure => "non_retryable_failure",
        OperationState::UnknownOutcome => "unknown_outcome",
        OperationState::Reconciling => "reconciling",
        OperationState::Resolved => "resolved",
        OperationState::ManualResolutionRequired => "manual_resolution_required",
    }
}

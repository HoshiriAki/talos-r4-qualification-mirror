#![cfg(feature = "postgres")]

use serde_json::Value;
use sqlx::Row;

use crate::repositories::contract::{
    ContractGenerateOutcome, ContractMutationError, ContractSignOutcome,
    ContractTemplateCreateOutcome, ContractTemplateUpdateOutcome, ContractVerifyOutcome, contract,
    map_mutation_error, render_template,
};
use crate::repositories::contract_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn template_create(
    session: &RepositorySession,
    name: &str,
    content_json: &Value,
    now: &str,
) -> Result<ContractTemplateCreateOutcome, ContractMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let name = name.to_owned();
    let content = serde_json::to_string(content_json).map_err(|error| {
        crate::repositories::RepositoryError::ContractViolation(error.to_string())
    })?;
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO contract_templates
                     (name,content_json,is_active,created_at,updated_at,tenant_id)
                     VALUES ($1,$2,1,$3,$3,$4)
                     RETURNING id",
                )
                .bind(&name)
                .bind(&content)
                .bind(&now)
                .bind(&tenant_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(ContractTemplateCreateOutcome {
                    id,
                    created_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn template_update(
    session: &RepositorySession,
    id: i64,
    name: Option<&str>,
    content_json: Option<&Value>,
    is_active: Option<bool>,
    now: &str,
) -> Result<ContractTemplateUpdateOutcome, ContractMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let name = name.map(str::to_owned);
    let content = content_json
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            crate::repositories::RepositoryError::ContractViolation(error.to_string())
        })?;
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let exists: Option<i64> = sqlx::query_scalar(
                    "SELECT id FROM contract_templates
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if exists.is_none() {
                    return Err(contract("contract-template-not-found".into()));
                }

                if let Some(name) = name {
                    sqlx::query(
                        "UPDATE contract_templates SET name=$1,updated_at=$2
                         WHERE tenant_id=$3 AND id=$4",
                    )
                    .bind(&name)
                    .bind(&now)
                    .bind(&tenant_id)
                    .bind(id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                }
                if let Some(content) = content {
                    sqlx::query(
                        "UPDATE contract_templates SET content_json=$1,updated_at=$2
                         WHERE tenant_id=$3 AND id=$4",
                    )
                    .bind(&content)
                    .bind(&now)
                    .bind(&tenant_id)
                    .bind(id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                }
                if let Some(is_active) = is_active {
                    sqlx::query(
                        "UPDATE contract_templates SET is_active=$1,updated_at=$2
                         WHERE tenant_id=$3 AND id=$4",
                    )
                    .bind(if is_active { 1_i32 } else { 0_i32 })
                    .bind(&now)
                    .bind(&tenant_id)
                    .bind(id)
                    .execute(&mut *connection)
                    .await
                    .map_err(pg_error)?;
                }
                sqlx::query(
                    "UPDATE contract_templates SET updated_at=$1
                     WHERE tenant_id=$2 AND id=$3",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(ContractTemplateUpdateOutcome {
                    id,
                    updated_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn generate(
    session: &RepositorySession,
    order_id: &str,
    template_id: Option<i64>,
    customer_name: &str,
    customer_phone: &str,
    device_value: f64,
    variables: Option<&Value>,
    now: &str,
) -> Result<ContractGenerateOutcome, ContractMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let order_id = order_id.to_owned();
    let customer_name = customer_name.to_owned();
    let customer_phone = customer_phone.to_owned();
    let variables = variables.cloned().unwrap_or_else(|| serde_json::json!({}));
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let order_exists: Option<String> = sqlx::query_scalar(
                    "SELECT id FROM orders
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR KEY SHARE",
                )
                .bind(&tenant_id)
                .bind(&order_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                if order_exists.is_none() {
                    return Err(contract("contract-order-not-found".into()));
                }

                let template = if let Some(template_id) = template_id {
                    sqlx::query(
                        "SELECT id,content_json FROM contract_templates
                         WHERE tenant_id=$1 AND id=$2 AND is_active=1
                         LIMIT 1
                         FOR SHARE",
                    )
                    .bind(&tenant_id)
                    .bind(template_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                } else {
                    sqlx::query(
                        "SELECT id,content_json FROM contract_templates
                         WHERE tenant_id=$1 AND is_active=1
                         ORDER BY id ASC LIMIT 1
                         FOR SHARE",
                    )
                    .bind(&tenant_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(pg_error)?
                }
                .ok_or_else(|| contract("contract-template-not-found".into()))?;

                let template_id = template.try_get::<i64, _>("id").map_err(pg_error)?;
                let template_content = template
                    .try_get::<String, _>("content_json")
                    .map_err(pg_error)?;
                let template_json: Value =
                    serde_json::from_str(&template_content).unwrap_or_default();
                let rendered_content = render_template(&template_json, &variables);
                let content_json = serde_json::json!({
                    "templateId": template_id,
                    "renderedContent": rendered_content,
                    "variables": variables,
                });
                let content = serde_json::to_string(&content_json)
                    .map_err(|error| contract(error.to_string()))?;

                let auto_triggered = device_value > 15_000.0;
                let status = if auto_triggered { "generated" } else { "draft" };
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO contracts
                     (order_id,template_id,customer_name,customer_phone,device_value,
                      content_json,status,created_at,updated_at,tenant_id)
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$8,$9)
                     RETURNING id",
                )
                .bind(&order_id)
                .bind(template_id)
                .bind(&customer_name)
                .bind(&customer_phone)
                .bind(device_value)
                .bind(&content)
                .bind(status)
                .bind(&now)
                .bind(&tenant_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;

                Ok(ContractGenerateOutcome {
                    id,
                    status: status.into(),
                    auto_triggered,
                    created_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn sign(
    session: &RepositorySession,
    id: i64,
    signer_name: &str,
    signer_phone: &str,
    signature_data: &str,
    now: &str,
) -> Result<ContractSignOutcome, ContractMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let signer_name = signer_name.to_owned();
    let signer_phone = signer_phone.to_owned();
    let signature_data = signature_data.to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let status: Option<String> = sqlx::query_scalar(
                    "SELECT status FROM contracts
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                let status = status.ok_or_else(|| contract("contract-not-found".into()))?;
                if status != "draft" && status != "generated" {
                    return Err(contract(format!("contract-status-invalid:{status}")));
                }

                sqlx::query(
                    "INSERT INTO e_signatures
                     (contract_id,signer_name,signer_phone,signature_data,signed_at,tenant_id)
                     VALUES ($1,$2,$3,$4,$5,$6)",
                )
                .bind(id)
                .bind(&signer_name)
                .bind(&signer_phone)
                .bind(&signature_data)
                .bind(&now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

                let updated = sqlx::query(
                    "UPDATE contracts
                     SET status='signed',signed_at=$1,updated_at=$1
                     WHERE tenant_id=$2 AND id=$3
                       AND status IN ('draft','generated')",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?
                .rows_affected();
                if updated != 1 {
                    return Err(contract("contract-status-invalid:changed".into()));
                }

                Ok(ContractSignOutcome { id, signed_at: now })
            })
        })
        .map_err(map_mutation_error)
}

pub(in crate::repositories) fn verify(
    session: &RepositorySession,
    id: i64,
    now: &str,
) -> Result<ContractVerifyOutcome, ContractMutationError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let now = now.to_owned();

    session
        .pg_write_serializable_repository(move |connection| {
            Box::pin(async move {
                let status: Option<String> = sqlx::query_scalar(
                    "SELECT status FROM contracts
                     WHERE tenant_id=$1 AND id=$2
                     LIMIT 1
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(pg_error)?;
                let status = status.ok_or_else(|| contract("contract-not-found".into()))?;
                if status != "signed" {
                    return Err(contract(format!("contract-status-invalid:{status}")));
                }

                let signature_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*)::bigint FROM e_signatures
                     WHERE tenant_id=$1 AND contract_id=$2",
                )
                .bind(&tenant_id)
                .bind(id)
                .fetch_one(&mut *connection)
                .await
                .map_err(pg_error)?;
                if signature_count == 0 {
                    return Err(contract("contract-no-signature".into()));
                }

                let updated = sqlx::query(
                    "UPDATE contracts SET status='verified',updated_at=$1
                     WHERE tenant_id=$2 AND id=$3 AND status='signed'",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?
                .rows_affected();
                if updated != 1 {
                    return Err(contract("contract-status-invalid:changed".into()));
                }

                Ok(ContractVerifyOutcome {
                    id,
                    verified_at: now,
                })
            })
        })
        .map_err(map_mutation_error)
}

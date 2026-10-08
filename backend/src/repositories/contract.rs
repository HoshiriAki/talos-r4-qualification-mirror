use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::Serialize;
use serde_json::Value;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

const TEMPLATE_NOT_FOUND: &str = "contract-template-not-found";
const ORDER_NOT_FOUND: &str = "contract-order-not-found";
const CONTRACT_NOT_FOUND: &str = "contract-not-found";
const STATUS_INVALID_PREFIX: &str = "contract-status-invalid:";
const NO_SIGNATURE: &str = "contract-no-signature";

#[derive(Debug, Clone, PartialEq)]
pub struct ContractTemplateCreateOutcome {
    pub id: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContractTemplateUpdateOutcome {
    pub id: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractTemplateProjection {
    pub id: i64,
    pub name: String,
    pub content_json: Value,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContractTemplateListProjection {
    pub items: Vec<ContractTemplateProjection>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContractGenerateOutcome {
    pub id: i64,
    pub status: String,
    pub auto_triggered: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContractSignOutcome {
    pub id: i64,
    pub signed_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContractVerifyOutcome {
    pub id: i64,
    pub verified_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractSignatureProjection {
    pub id: i64,
    pub signer_name: String,
    pub signer_phone: String,
    pub signature_data: String,
    pub signed_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractProjection {
    pub id: i64,
    pub order_id: String,
    pub template_id: Option<i64>,
    pub customer_name: String,
    pub customer_phone: String,
    pub device_value: f64,
    pub content_json: Value,
    pub status: String,
    pub signed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub signatures: Vec<ContractSignatureProjection>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractListItem {
    pub id: i64,
    pub order_id: String,
    pub template_id: Option<i64>,
    pub customer_name: String,
    pub customer_phone: String,
    pub device_value: f64,
    pub content_json: Value,
    pub status: String,
    pub signed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContractListProjection {
    pub items: Vec<ContractListItem>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum ContractMutationError {
    #[error("contract template not found")]
    TemplateNotFound,
    #[error("order not found")]
    OrderNotFound,
    #[error("contract not found")]
    ContractNotFound,
    #[error("contract status invalid: {0}")]
    StatusInvalid(String),
    #[error("contract has no signature")]
    NoSignature,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteContractRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteContractRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn template_create(
        &self,
        name: &str,
        content_json: &Value,
        now: &str,
    ) -> Result<ContractTemplateCreateOutcome, ContractMutationError> {
        let tenant_id = self.tenant_id();
        let name = name.to_owned();
        let content = serde_json::to_string(content_json)
            .map_err(|error| RepositoryError::ContractViolation(error.to_string()))?;
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                transaction
                    .execute(
                        "INSERT INTO contract_templates
                         (name,content_json,is_active,created_at,updated_at,tenant_id)
                         VALUES (?1,?2,1,?3,?3,?4)",
                        params![name, content, now, tenant_id],
                    )
                    .map_err(sqlite_error)?;
                Ok(ContractTemplateCreateOutcome {
                    id: transaction.last_insert_rowid(),
                    created_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn template_update(
        &self,
        id: i64,
        name: Option<&str>,
        content_json: Option<&Value>,
        is_active: Option<bool>,
        now: &str,
    ) -> Result<ContractTemplateUpdateOutcome, ContractMutationError> {
        let tenant_id = self.tenant_id();
        let name = name.map(str::to_owned);
        let content = content_json
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| RepositoryError::ContractViolation(error.to_string()))?;
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let exists = transaction
                    .query_row(
                        "SELECT 1 FROM contract_templates
                         WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if exists.is_none() {
                    return Err(contract(TEMPLATE_NOT_FOUND.into()));
                }

                if let Some(name) = name {
                    transaction
                        .execute(
                            "UPDATE contract_templates SET name=?1,updated_at=?2
                             WHERE tenant_id=?3 AND id=?4",
                            params![name, now, tenant_id, id],
                        )
                        .map_err(sqlite_error)?;
                }
                if let Some(content) = content {
                    transaction
                        .execute(
                            "UPDATE contract_templates SET content_json=?1,updated_at=?2
                             WHERE tenant_id=?3 AND id=?4",
                            params![content, now, tenant_id, id],
                        )
                        .map_err(sqlite_error)?;
                }
                if let Some(is_active) = is_active {
                    transaction
                        .execute(
                            "UPDATE contract_templates SET is_active=?1,updated_at=?2
                             WHERE tenant_id=?3 AND id=?4",
                            params![is_active as i32, now, tenant_id, id],
                        )
                        .map_err(sqlite_error)?;
                }
                transaction
                    .execute(
                        "UPDATE contract_templates SET updated_at=?1
                         WHERE tenant_id=?2 AND id=?3",
                        params![now, tenant_id, id],
                    )
                    .map_err(sqlite_error)?;

                Ok(ContractTemplateUpdateOutcome {
                    id,
                    updated_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn template_list(
        &self,
        is_active: Option<bool>,
        page: i64,
        page_size: i64,
    ) -> Result<ContractTemplateListProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let total: i64 = if let Some(active) = is_active {
                connection.query_row(
                    "SELECT COUNT(*) FROM contract_templates
                     WHERE tenant_id=?1 AND is_active=?2",
                    params![tenant_id, active as i32],
                    |row| row.get(0),
                )?
            } else {
                connection.query_row(
                    "SELECT COUNT(*) FROM contract_templates WHERE tenant_id=?1",
                    params![tenant_id],
                    |row| row.get(0),
                )?
            };

            let (sql, active_filter) = if is_active.is_some() {
                (
                    format!(
                        "SELECT id,name,content_json,is_active,created_at,updated_at
                         FROM contract_templates
                         WHERE tenant_id=?1 AND is_active=?2
                         ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
                    ),
                    true,
                )
            } else {
                (
                    format!(
                        "SELECT id,name,content_json,is_active,created_at,updated_at
                         FROM contract_templates
                         WHERE tenant_id=?1
                         ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
                    ),
                    false,
                )
            };
            let mut statement = connection.prepare(&sql)?;
            let items = if active_filter {
                statement
                    .query_map(
                        params![tenant_id, is_active.unwrap_or(false) as i32],
                        map_template,
                    )?
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                statement
                    .query_map(params![tenant_id], map_template)?
                    .collect::<Result<Vec<_>, _>>()?
            };

            Ok(ContractTemplateListProjection {
                items,
                page,
                page_size,
                total,
            })
        })
    }

    pub(in crate::repositories) fn template_get(
        &self,
        id: i64,
    ) -> Result<Option<ContractTemplateProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT id,name,content_json,is_active,created_at,updated_at
                     FROM contract_templates
                     WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                    params![tenant_id, id],
                    map_template,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn generate(
        &self,
        order_id: &str,
        template_id: Option<i64>,
        customer_name: &str,
        customer_phone: &str,
        device_value: f64,
        variables: Option<&Value>,
        now: &str,
    ) -> Result<ContractGenerateOutcome, ContractMutationError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id.to_owned();
        let customer_name = customer_name.to_owned();
        let customer_phone = customer_phone.to_owned();
        let variables = variables.cloned().unwrap_or_else(|| serde_json::json!({}));
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let order_exists = transaction
                    .query_row(
                        "SELECT 1 FROM orders WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, order_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?;
                if order_exists.is_none() {
                    return Err(contract(ORDER_NOT_FOUND.into()));
                }

                let template = if let Some(template_id) = template_id {
                    transaction
                        .query_row(
                            "SELECT id,content_json FROM contract_templates
                             WHERE tenant_id=?1 AND id=?2 AND is_active=1 LIMIT 1",
                            params![tenant_id, template_id],
                            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                        )
                        .optional()
                        .map_err(sqlite_error)?
                } else {
                    transaction
                        .query_row(
                            "SELECT id,content_json FROM contract_templates
                             WHERE tenant_id=?1 AND is_active=1
                             ORDER BY id ASC LIMIT 1",
                            params![tenant_id],
                            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                        )
                        .optional()
                        .map_err(sqlite_error)?
                }
                .ok_or_else(|| contract(TEMPLATE_NOT_FOUND.into()))?;
                let (template_id, template_content) = template;
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
                transaction
                    .execute(
                        "INSERT INTO contracts
                         (order_id,template_id,customer_name,customer_phone,device_value,
                          content_json,status,created_at,updated_at,tenant_id)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8,?9)",
                        params![
                            order_id,
                            template_id,
                            customer_name,
                            customer_phone,
                            device_value,
                            content,
                            status,
                            now,
                            tenant_id,
                        ],
                    )
                    .map_err(sqlite_error)?;

                Ok(ContractGenerateOutcome {
                    id: transaction.last_insert_rowid(),
                    status: status.into(),
                    auto_triggered,
                    created_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn sign(
        &self,
        id: i64,
        signer_name: &str,
        signer_phone: &str,
        signature_data: &str,
        now: &str,
    ) -> Result<ContractSignOutcome, ContractMutationError> {
        let tenant_id = self.tenant_id();
        let signer_name = signer_name.to_owned();
        let signer_phone = signer_phone.to_owned();
        let signature_data = signature_data.to_owned();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let status = transaction
                    .query_row(
                        "SELECT status FROM contracts
                         WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(CONTRACT_NOT_FOUND.into()))?;
                if status != "draft" && status != "generated" {
                    return Err(contract(format!("{STATUS_INVALID_PREFIX}{status}")));
                }

                transaction
                    .execute(
                        "INSERT INTO e_signatures
                         (contract_id,signer_name,signer_phone,signature_data,signed_at,tenant_id)
                         VALUES (?1,?2,?3,?4,?5,?6)",
                        params![
                            id,
                            signer_name,
                            signer_phone,
                            signature_data,
                            now,
                            tenant_id
                        ],
                    )
                    .map_err(sqlite_error)?;
                let updated = transaction
                    .execute(
                        "UPDATE contracts
                         SET status='signed',signed_at=?1,updated_at=?1
                         WHERE tenant_id=?2 AND id=?3
                           AND status IN ('draft','generated')",
                        params![now, tenant_id, id],
                    )
                    .map_err(sqlite_error)?;
                if updated != 1 {
                    return Err(contract(format!("{STATUS_INVALID_PREFIX}changed")));
                }

                Ok(ContractSignOutcome { id, signed_at: now })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn verify(
        &self,
        id: i64,
        now: &str,
    ) -> Result<ContractVerifyOutcome, ContractMutationError> {
        let tenant_id = self.tenant_id();
        let now = now.to_owned();

        self.session
            .write_immediate(move |transaction| {
                let status = transaction
                    .query_row(
                        "SELECT status FROM contracts
                         WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                        params![tenant_id, id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(sqlite_error)?
                    .ok_or_else(|| contract(CONTRACT_NOT_FOUND.into()))?;
                if status != "signed" {
                    return Err(contract(format!("{STATUS_INVALID_PREFIX}{status}")));
                }

                let signature_count: i64 = transaction
                    .query_row(
                        "SELECT COUNT(*) FROM e_signatures
                         WHERE tenant_id=?1 AND contract_id=?2",
                        params![tenant_id, id],
                        |row| row.get(0),
                    )
                    .map_err(sqlite_error)?;
                if signature_count == 0 {
                    return Err(contract(NO_SIGNATURE.into()));
                }

                let updated = transaction
                    .execute(
                        "UPDATE contracts SET status='verified',updated_at=?1
                         WHERE tenant_id=?2 AND id=?3 AND status='signed'",
                        params![now, tenant_id, id],
                    )
                    .map_err(sqlite_error)?;
                if updated != 1 {
                    return Err(contract(format!("{STATUS_INVALID_PREFIX}changed")));
                }

                Ok(ContractVerifyOutcome {
                    id,
                    verified_at: now,
                })
            })
            .map_err(map_mutation_error)
    }

    pub(in crate::repositories) fn list(
        &self,
        order_id: Option<&str>,
        customer_phone: Option<&str>,
        status: Option<&str>,
        page: i64,
        page_size: i64,
    ) -> Result<ContractListProjection, RepositoryError> {
        let tenant_id = self.tenant_id();
        let order_id = order_id
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let customer_phone = customer_phone
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let status = status.filter(|value| !value.is_empty()).map(str::to_owned);
        let page = page.max(1);
        let page_size = page_size.clamp(1, 100);
        let offset = (page - 1) * page_size;

        self.session.read(move |connection| {
            let mut conditions = vec!["tenant_id = ?".to_owned()];
            let mut values = vec![SqlValue::Text(tenant_id)];
            if let Some(order_id) = order_id {
                conditions.push("order_id = ?".into());
                values.push(SqlValue::Text(order_id));
            }
            if let Some(customer_phone) = customer_phone {
                conditions.push("customer_phone = ?".into());
                values.push(SqlValue::Text(customer_phone));
            }
            if let Some(status) = status {
                conditions.push("status = ?".into());
                values.push(SqlValue::Text(status));
            }
            let where_sql = conditions.join(" AND ");

            let total: i64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM contracts WHERE {where_sql}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;
            let sql = format!(
                "SELECT id,order_id,template_id,customer_name,customer_phone,device_value,
                        content_json,status,signed_at,created_at,updated_at
                 FROM contracts WHERE {where_sql}
                 ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}"
            );
            let mut statement = connection.prepare(&sql)?;
            let items = statement
                .query_map(params_from_iter(values.iter()), map_contract_list_item)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(ContractListProjection {
                items,
                page,
                page_size,
                total,
            })
        })
    }

    pub(in crate::repositories) fn get(
        &self,
        id: i64,
    ) -> Result<Option<ContractProjection>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            let base = connection
                .query_row(
                    "SELECT id,order_id,template_id,customer_name,customer_phone,device_value,
                            content_json,status,signed_at,created_at,updated_at
                     FROM contracts WHERE tenant_id=?1 AND id=?2 LIMIT 1",
                    params![tenant_id, id],
                    map_contract_base,
                )
                .optional()?;
            let Some(base) = base else {
                return Ok(None);
            };

            let mut statement = connection.prepare(
                "SELECT id,signer_name,signer_phone,signature_data,signed_at
                 FROM e_signatures
                 WHERE tenant_id=?1 AND contract_id=?2
                 ORDER BY signed_at ASC,id ASC",
            )?;
            let signatures = statement
                .query_map(params![tenant_id, id], map_signature)?
                .collect::<Result<Vec<_>, _>>()?;

            Ok(Some(ContractProjection {
                id: base.id,
                order_id: base.order_id,
                template_id: base.template_id,
                customer_name: base.customer_name,
                customer_phone: base.customer_phone,
                device_value: base.device_value,
                content_json: base.content_json,
                status: base.status,
                signed_at: base.signed_at,
                created_at: base.created_at,
                updated_at: base.updated_at,
                signatures,
            }))
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

#[derive(Debug)]
struct ContractBase {
    id: i64,
    order_id: String,
    template_id: Option<i64>,
    customer_name: String,
    customer_phone: String,
    device_value: f64,
    content_json: Value,
    status: String,
    signed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

pub(in crate::repositories) fn map_mutation_error(error: RepositoryError) -> ContractMutationError {
    if let RepositoryError::ContractViolation(message) = &error {
        if message == TEMPLATE_NOT_FOUND {
            return ContractMutationError::TemplateNotFound;
        }
        if message == ORDER_NOT_FOUND {
            return ContractMutationError::OrderNotFound;
        }
        if message == CONTRACT_NOT_FOUND {
            return ContractMutationError::ContractNotFound;
        }
        if message == NO_SIGNATURE {
            return ContractMutationError::NoSignature;
        }
        if let Some(status) = message.strip_prefix(STATUS_INVALID_PREFIX) {
            return ContractMutationError::StatusInvalid(status.to_owned());
        }
    }
    ContractMutationError::Storage(error)
}

pub(in crate::repositories) fn contract(message: String) -> RepositoryError {
    RepositoryError::ContractViolation(message)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

pub(in crate::repositories) fn render_template(template_json: &Value, variables: &Value) -> String {
    let mut rendered = serde_json::to_string(template_json).unwrap_or_default();
    if let Some(object) = variables.as_object() {
        for (key, value) in object {
            let placeholder = format!("{{{key}}}");
            let raw = match value {
                Value::String(value) => value.clone(),
                other => other.to_string(),
            };
            let escaped = raw
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&#x27;");
            rendered = rendered.replace(&placeholder, &escaped);
        }
    }
    rendered
}

fn map_template(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContractTemplateProjection> {
    let content: String = row.get(2)?;
    Ok(ContractTemplateProjection {
        id: row.get(0)?,
        name: row.get(1)?,
        content_json: serde_json::from_str(&content).unwrap_or_default(),
        is_active: row.get::<_, i32>(3)? == 1,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn map_contract_base(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContractBase> {
    let content: String = row.get(6)?;
    Ok(ContractBase {
        id: row.get(0)?,
        order_id: row.get(1)?,
        template_id: row.get(2)?,
        customer_name: row.get(3)?,
        customer_phone: row.get(4)?,
        device_value: row.get(5)?,
        content_json: serde_json::from_str(&content).unwrap_or_default(),
        status: row.get(7)?,
        signed_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn map_contract_list_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContractListItem> {
    let base = map_contract_base(row)?;
    Ok(ContractListItem {
        id: base.id,
        order_id: base.order_id,
        template_id: base.template_id,
        customer_name: base.customer_name,
        customer_phone: base.customer_phone,
        device_value: base.device_value,
        content_json: base.content_json,
        status: base.status,
        signed_at: base.signed_at,
        created_at: base.created_at,
        updated_at: base.updated_at,
    })
}

fn map_signature(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContractSignatureProjection> {
    Ok(ContractSignatureProjection {
        id: row.get(0)?,
        signer_name: row.get(1)?,
        signer_phone: row.get(2)?,
        signature_data: row.get(3)?,
        signed_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use crate::repositories::{
        ContractMutationError, RepositoryProvider, SqliteRepositoryProvider,
    };

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("contract-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("contract-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_contract_authority_preserves_scope_identity_and_signature_lifecycle() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON;
                 CREATE TABLE tenants (
                    id TEXT PRIMARY KEY
                 );
                 CREATE TABLE orders (
                    id TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    PRIMARY KEY(id,tenant_id)
                 );
                 CREATE TABLE contract_templates (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    content_json TEXT NOT NULL,
                    is_active INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    UNIQUE(id,tenant_id)
                 );
                 CREATE TABLE contracts (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    order_id TEXT NOT NULL,
                    template_id INTEGER,
                    customer_name TEXT NOT NULL,
                    customer_phone TEXT NOT NULL,
                    device_value REAL NOT NULL,
                    content_json TEXT NOT NULL,
                    status TEXT NOT NULL,
                    signed_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    UNIQUE(id,tenant_id),
                    FOREIGN KEY(order_id,tenant_id) REFERENCES orders(id,tenant_id),
                    FOREIGN KEY(template_id,tenant_id) REFERENCES contract_templates(id,tenant_id)
                 );
                 CREATE TABLE e_signatures (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    contract_id INTEGER NOT NULL,
                    signer_name TEXT NOT NULL,
                    signer_phone TEXT NOT NULL,
                    signature_data TEXT NOT NULL,
                    signed_at TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    FOREIGN KEY(contract_id,tenant_id) REFERENCES contracts(id,tenant_id)
                 );
                 INSERT INTO tenants VALUES ('tenant-a'),('tenant-b');
                 INSERT INTO orders VALUES ('order-alpha','tenant-a'),('order-beta','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let scoped_a = provider.bind(&context("tenant-a", "contract-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "contract-b")).unwrap();

        let template_a = scoped_a
            .contracts()
            .template_create(
                "Template A",
                &serde_json::json!({"body":"Hello {customer} <raw>"}),
                "2026-10-01T21:20:00+08:00",
            )
            .unwrap();
        let template_b = scoped_b
            .contracts()
            .template_create(
                "Template B",
                &serde_json::json!({"body":"Tenant B"}),
                "2026-10-01T21:20:01+08:00",
            )
            .unwrap();

        assert_eq!(
            scoped_a
                .contracts()
                .template_list(None, 1, 20)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            scoped_b
                .contracts()
                .template_list(None, 1, 20)
                .unwrap()
                .total,
            1
        );
        assert_ne!(template_a.id, template_b.id);

        let generated = scoped_a
            .contracts()
            .generate(
                "order-alpha",
                None,
                "Alice",
                "10086",
                20_000.0,
                Some(&serde_json::json!({"customer":"<Alice>"})),
                "2026-10-01T21:21:00+08:00",
            )
            .unwrap();
        assert_eq!(generated.status, "generated");
        assert!(generated.auto_triggered);

        let cross_tenant = scoped_b.contracts().get(generated.id).unwrap();
        assert!(cross_tenant.is_none());
        let cross_sign = scoped_b.contracts().sign(
            generated.id,
            "Bob",
            "10010",
            "sig-b",
            "2026-10-01T21:22:00+08:00",
        );
        assert!(matches!(
            cross_sign,
            Err(ContractMutationError::ContractNotFound)
        ));

        let invalid_order = scoped_a.contracts().generate(
            "order-beta",
            None,
            "Alice",
            "10086",
            100.0,
            None,
            "2026-10-01T21:22:10+08:00",
        );
        assert!(matches!(
            invalid_order,
            Err(ContractMutationError::OrderNotFound)
        ));

        let signed = scoped_a
            .contracts()
            .sign(
                generated.id,
                "Alice",
                "10086",
                "sig-a",
                "2026-10-01T21:23:00+08:00",
            )
            .unwrap();
        assert_eq!(signed.id, generated.id);

        let verified = scoped_a
            .contracts()
            .verify(generated.id, "2026-10-01T21:24:00+08:00")
            .unwrap();
        assert_eq!(verified.id, generated.id);

        let persisted = scoped_a.contracts().get(generated.id).unwrap().unwrap();
        assert_eq!(persisted.order_id, "order-alpha");
        assert_eq!(persisted.status, "verified");
        assert_eq!(persisted.signatures.len(), 1);
        assert!(persisted.content_json.to_string().contains("&lt;Alice&gt;"));

        assert_eq!(
            scoped_a
                .contracts()
                .list(Some("order-alpha"), None, Some("verified"), 1, 20)
                .unwrap()
                .total,
            1
        );
    }
}

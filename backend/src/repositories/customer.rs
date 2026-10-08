use chrono::{SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{
    ContactKind, CustomerId, CustomerRiskStatus, CustomerStatus, mask_contact_value,
};
use crate::repositories::sqlite::SqliteRepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone)]
pub struct NewCustomerContact {
    pub kind: ContactKind,
    pub raw_value: String,
    pub normalized_value: String,
    pub is_primary: bool,
}

#[derive(Debug, Clone)]
pub struct NewCustomerRecord {
    pub id: CustomerId,
    pub legal_name: String,
    pub display_name: String,
    pub status: CustomerStatus,
    pub risk_status: CustomerRiskStatus,
    pub contacts: Vec<NewCustomerContact>,
    pub actor_identity_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerContactProjection {
    pub id: String,
    pub kind: ContactKind,
    pub masked_value: String,
    pub is_primary: bool,
    pub classification: String,
    pub purpose: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerProjection {
    pub id: CustomerId,
    pub legal_name: String,
    pub display_name: String,
    pub status: CustomerStatus,
    pub risk_status: CustomerRiskStatus,
    pub version: i64,
    pub contacts: Vec<CustomerContactProjection>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCustomerCandidate {
    pub customer_id: CustomerId,
    pub display_name: String,
    pub matched_contact: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerMigrationExceptionProjection {
    pub id: String,
    pub source_table: String,
    pub source_row_id: String,
    pub reason: String,
    pub status: String,
    pub resolved_customer_id: Option<CustomerId>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

pub struct ScopedCustomerRepository<'a> {
    session: &'a SqliteRepositorySession,
}

impl<'a> ScopedCustomerRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub fn create(
        &self,
        record: &NewCustomerRecord,
    ) -> Result<CustomerProjection, RepositoryError> {
        if record.legal_name.trim().is_empty() || record.display_name.trim().is_empty() {
            return Err(RepositoryError::ContractViolation(
                "customer legal_name and display_name must not be blank".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write(|transaction| {
            transaction.execute(
                "INSERT INTO customers
                 (id, tenant_id, legal_name, display_name, status, risk_status, version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
                params![
                    record.id.as_str(),
                    tenant_id,
                    record.legal_name.trim(),
                    record.display_name.trim(),
                    record.status.as_str(),
                    record.risk_status.as_str(),
                    now,
                ],
            )?;

            for contact in &record.contacts {
                let contact_id = Uuid::new_v4().to_string();
                transaction.execute(
                    "INSERT INTO customer_contacts
                     (id, tenant_id, customer_id, kind, raw_value, normalized_value, is_primary,
                      classification, purpose, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pii', 'rental_contact', ?8, ?8)",
                    params![
                        contact_id,
                        tenant_id,
                        record.id.as_str(),
                        contact.kind.as_str(),
                        contact.raw_value.trim(),
                        contact.normalized_value,
                        if contact.is_primary { 1 } else { 0 },
                        now,
                    ],
                )?;
            }

            append_history(
                transaction,
                &tenant_id,
                &record.id,
                "customer_created",
                record.actor_identity_id.as_deref(),
                "{}",
                &now,
            )?;
            Ok(())
        })?;
        self.get(&record.id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("created customer could not be reloaded".into())
        })
    }

    pub fn list(&self, limit: u32) -> Result<Vec<CustomerProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let limit = limit.clamp(1, 200);
        self.session.read(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, legal_name, display_name, status, risk_status, version, created_at, updated_at
                 FROM customers WHERE tenant_id = ?1
                 ORDER BY updated_at DESC, id LIMIT ?2",
            )?;
            let rows = statement.query_map(params![tenant_id, i64::from(limit)], |row| {
                let id: String = row.get(0)?;
                map_customer_row(connection, &tenant_id, &id, row)
            })?;
            rows.collect()
        })
    }

    pub fn get(&self, id: &CustomerId) -> Result<Option<CustomerProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(|connection| {
            connection
                .query_row(
                    "SELECT id, legal_name, display_name, status, risk_status, version, created_at, updated_at
                     FROM customers WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, id.as_str()],
                    |row| {
                        let row_id: String = row.get(0)?;
                        map_customer_row(connection, &tenant_id, &row_id, row)
                    },
                )
                .optional()
        })
    }

    pub fn add_contact(
        &self,
        customer_id: &CustomerId,
        contact: &NewCustomerContact,
        actor_identity_id: Option<&str>,
    ) -> Result<CustomerProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write(|transaction| {
            ensure_customer(transaction, &tenant_id, customer_id)?;
            transaction.execute(
                "INSERT INTO customer_contacts
                 (id, tenant_id, customer_id, kind, raw_value, normalized_value, is_primary,
                  classification, purpose, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pii', 'rental_contact', ?8, ?8)",
                params![
                    Uuid::new_v4().to_string(),
                    tenant_id,
                    customer_id.as_str(),
                    contact.kind.as_str(),
                    contact.raw_value.trim(),
                    contact.normalized_value,
                    if contact.is_primary { 1 } else { 0 },
                    now,
                ],
            )?;
            transaction.execute(
                "UPDATE customers SET version = version + 1, updated_at = ?1
                 WHERE tenant_id = ?2 AND id = ?3",
                params![now, tenant_id, customer_id.as_str()],
            )?;
            append_history(
                transaction,
                &tenant_id,
                customer_id,
                "customer_contact_added",
                actor_identity_id,
                &serde_json::json!({"kind": contact.kind.as_str()}).to_string(),
                &now,
            )?;
            Ok(())
        })?;
        self.get(customer_id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("updated customer could not be reloaded".into())
        })
    }

    pub fn add_external_identity(
        &self,
        customer_id: &CustomerId,
        provider: &str,
        external_subject: &str,
        actor_identity_id: Option<&str>,
    ) -> Result<(), RepositoryError> {
        let provider = provider.trim().to_ascii_lowercase();
        let external_subject = external_subject.trim();
        if provider.is_empty() || external_subject.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "external identity provider and subject must not be blank".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write(|transaction| {
            ensure_customer(transaction, &tenant_id, customer_id)?;
            transaction.execute(
                "INSERT INTO customer_external_identities
                 (id, tenant_id, customer_id, provider, external_subject, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    Uuid::new_v4().to_string(),
                    tenant_id,
                    customer_id.as_str(),
                    provider,
                    external_subject,
                    now,
                ],
            )?;
            append_history(
                transaction,
                &tenant_id,
                customer_id,
                "customer_external_identity_added",
                actor_identity_id,
                &serde_json::json!({"provider": provider}).to_string(),
                &now,
            )?;
            Ok(())
        })
    }

    pub fn duplicate_candidates(
        &self,
        kind: ContactKind,
        normalized_value: &str,
    ) -> Result<Vec<DuplicateCustomerCandidate>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(|connection| {
            let mut statement = connection.prepare(
                "SELECT DISTINCT c.id, c.display_name
                 FROM customers c
                 JOIN customer_contacts cc
                   ON cc.tenant_id = c.tenant_id AND cc.customer_id = c.id
                 WHERE c.tenant_id = ?1 AND cc.kind = ?2 AND cc.normalized_value = ?3
                 ORDER BY c.display_name, c.id",
            )?;
            let masked = mask_contact_value(kind, normalized_value);
            let rows = statement.query_map(
                params![tenant_id, kind.as_str(), normalized_value],
                |row| {
                    let id: String = row.get(0)?;
                    Ok(DuplicateCustomerCandidate {
                        customer_id: CustomerId::parse(id).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                error.into(),
                            )
                        })?,
                        display_name: row.get(1)?,
                        matched_contact: masked.clone(),
                    })
                },
            )?;
            rows.collect()
        })
    }

    pub fn list_migration_exceptions(
        &self,
        limit: u32,
    ) -> Result<Vec<CustomerMigrationExceptionProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let limit = limit.clamp(1, 200);
        self.session.read(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, source_table, source_row_id, reason, status,
                        resolved_customer_id, created_at, resolved_at
                 FROM customer_migration_exceptions
                 WHERE tenant_id = ?1 AND status = 'pending'
                 ORDER BY created_at, id LIMIT ?2",
            )?;
            let rows = statement.query_map(params![tenant_id, i64::from(limit)], |row| {
                let resolved: Option<String> = row.get(5)?;
                Ok(CustomerMigrationExceptionProjection {
                    id: row.get(0)?,
                    source_table: row.get(1)?,
                    source_row_id: row.get(2)?,
                    reason: row.get(3)?,
                    status: row.get(4)?,
                    resolved_customer_id: resolved.map(CustomerId::parse).transpose().map_err(
                        |error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                5,
                                rusqlite::types::Type::Text,
                                error.into(),
                            )
                        },
                    )?,
                    created_at: row.get(6)?,
                    resolved_at: row.get(7)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn resolve_migration_exception(
        &self,
        exception_id: &str,
        customer_id: &CustomerId,
        actor_identity_id: Option<&str>,
    ) -> Result<(), RepositoryError> {
        let exception_id = exception_id.trim();
        if exception_id.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "migration exception id must not be blank".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session
            .write(|transaction| {
                ensure_customer(transaction, &tenant_id, customer_id)?;
                let source: Option<(String, String)> = transaction
                    .query_row(
                        "SELECT source_table, source_row_id
                     FROM customer_migration_exceptions
                     WHERE tenant_id = ?1 AND id = ?2 AND status = 'pending'",
                        params![tenant_id, exception_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?;
                let Some((source_table, source_row_id)) = source else {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                };

                let updated = match source_table.as_str() {
                    "orders" => transaction.execute(
                        "UPDATE orders SET customer_id = ?1
                     WHERE tenant_id = ?2 AND id = ?3",
                        params![customer_id.as_str(), tenant_id, source_row_id],
                    )?,
                    "blacklist" => transaction.execute(
                        "UPDATE blacklist SET customer_id = ?1
                     WHERE tenant_id = ?2 AND CAST(id AS TEXT) = ?3",
                        params![customer_id.as_str(), tenant_id, source_row_id],
                    )?,
                    "violations" => transaction.execute(
                        "UPDATE violations SET customer_id = ?1
                     WHERE tenant_id = ?2 AND CAST(id AS TEXT) = ?3",
                        params![customer_id.as_str(), tenant_id, source_row_id],
                    )?,
                    "credit_scores" => transaction.execute(
                        "UPDATE credit_scores SET customer_id = ?1
                     WHERE tenant_id = ?2 AND CAST(id AS TEXT) = ?3",
                        params![customer_id.as_str(), tenant_id, source_row_id],
                    )?,
                    "overdue_records" => transaction.execute(
                        "UPDATE overdue_records SET customer_id = ?1
                     WHERE tenant_id = ?2 AND CAST(id AS TEXT) = ?3",
                        params![customer_id.as_str(), tenant_id, source_row_id],
                    )?,
                    "contracts" => transaction.execute(
                        "UPDATE contracts SET customer_id = ?1
                     WHERE tenant_id = ?2 AND CAST(id AS TEXT) = ?3",
                        params![customer_id.as_str(), tenant_id, source_row_id],
                    )?,
                    _ => {
                        return Err(rusqlite::Error::InvalidParameterName(format!(
                            "unsupported customer migration source: {source_table}"
                        )));
                    }
                };
                if updated != 1 {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                }
                transaction.execute(
                    "UPDATE customer_migration_exceptions
                 SET status = 'resolved', resolved_customer_id = ?1, resolved_at = ?2
                 WHERE tenant_id = ?3 AND id = ?4 AND status = 'pending'",
                    params![customer_id.as_str(), now, tenant_id, exception_id],
                )?;
                append_history(
                    transaction,
                    &tenant_id,
                    customer_id,
                    "legacy_customer_association_resolved",
                    actor_identity_id,
                    &serde_json::json!({
                        "sourceTable": source_table,
                        "sourceRowId": source_row_id
                    })
                    .to_string(),
                    &now,
                )?;
                Ok(())
            })
            .map_err(|error| match error {
                RepositoryError::Sqlite(detail) if detail.contains("Query returned no rows") => {
                    RepositoryError::ContractViolation(
                        "migration exception or source row was not found in the bound tenant"
                            .into(),
                    )
                }
                other => other,
            })
    }

    pub fn anonymize(
        &self,
        customer_id: &CustomerId,
        actor_identity_id: Option<&str>,
    ) -> Result<CustomerProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write(|transaction| {
            ensure_customer(transaction, &tenant_id, customer_id)?;
            transaction.execute(
                "DELETE FROM customer_contacts WHERE tenant_id = ?1 AND customer_id = ?2",
                params![tenant_id, customer_id.as_str()],
            )?;
            transaction.execute(
                "DELETE FROM customer_external_identities
                 WHERE tenant_id = ?1 AND customer_id = ?2",
                params![tenant_id, customer_id.as_str()],
            )?;
            let anonymous_label = format!("ANON-{}", &customer_id.as_str()[..8]);
            transaction.execute(
                "UPDATE customers
                 SET legal_name = ?1, display_name = ?1, status = 'anonymized',
                     version = version + 1, updated_at = ?2
                 WHERE tenant_id = ?3 AND id = ?4",
                params![anonymous_label, now, tenant_id, customer_id.as_str()],
            )?;
            append_history(
                transaction,
                &tenant_id,
                customer_id,
                "customer_anonymized",
                actor_identity_id,
                "{}",
                &now,
            )?;
            Ok(())
        })?;
        self.get(customer_id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("anonymized customer could not be reloaded".into())
        })
    }
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn ensure_customer(
    transaction: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    customer_id: &CustomerId,
) -> Result<(), rusqlite::Error> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM customers WHERE tenant_id = ?1 AND id = ?2)",
        params![tenant_id, customer_id.as_str()],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(rusqlite::Error::QueryReturnedNoRows)
    }
}

fn append_history(
    transaction: &rusqlite::Transaction<'_>,
    tenant_id: &str,
    customer_id: &CustomerId,
    event_type: &str,
    actor_identity_id: Option<&str>,
    detail_json: &str,
    created_at: &str,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO customer_history
         (id, tenant_id, customer_id, event_type, detail_json, actor_identity_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            Uuid::new_v4().to_string(),
            tenant_id,
            customer_id.as_str(),
            event_type,
            detail_json,
            actor_identity_id,
            created_at,
        ],
    )?;
    Ok(())
}

fn map_customer_row(
    connection: &rusqlite::Connection,
    tenant_id: &str,
    id: &str,
    row: &rusqlite::Row<'_>,
) -> Result<CustomerProjection, rusqlite::Error> {
    let status: String = row.get(3)?;
    let risk_status: String = row.get(4)?;
    let mut contact_statement = connection.prepare(
        "SELECT id, kind, normalized_value, is_primary, classification, purpose
         FROM customer_contacts
         WHERE tenant_id = ?1 AND customer_id = ?2
         ORDER BY is_primary DESC, kind, id",
    )?;
    let contacts = contact_statement
        .query_map(params![tenant_id, id], |contact_row| {
            let kind_text: String = contact_row.get(1)?;
            let kind = parse_contact_kind(&kind_text)?;
            let normalized: String = contact_row.get(2)?;
            Ok(CustomerContactProjection {
                id: contact_row.get(0)?,
                kind,
                masked_value: mask_contact_value(kind, &normalized),
                is_primary: contact_row.get::<_, i64>(3)? != 0,
                classification: contact_row.get(4)?,
                purpose: contact_row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(CustomerProjection {
        id: CustomerId::parse(id.to_owned()).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
        })?,
        legal_name: row.get(1)?,
        display_name: row.get(2)?,
        status: parse_customer_status(&status)?,
        risk_status: parse_customer_risk_status(&risk_status)?,
        version: row.get(5)?,
        contacts,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn parse_contact_kind(value: &str) -> Result<ContactKind, rusqlite::Error> {
    match value {
        "phone" => Ok(ContactKind::Phone),
        "email" => Ok(ContactKind::Email),
        "other" => Ok(ContactKind::Other),
        _ => Err(rusqlite::Error::InvalidColumnType(
            0,
            "kind".into(),
            rusqlite::types::Type::Text,
        )),
    }
}

fn parse_customer_status(value: &str) -> Result<CustomerStatus, rusqlite::Error> {
    match value {
        "active" => Ok(CustomerStatus::Active),
        "inactive" => Ok(CustomerStatus::Inactive),
        "anonymized" => Ok(CustomerStatus::Anonymized),
        _ => Err(rusqlite::Error::InvalidColumnType(
            0,
            "status".into(),
            rusqlite::types::Type::Text,
        )),
    }
}

fn parse_customer_risk_status(value: &str) -> Result<CustomerRiskStatus, rusqlite::Error> {
    match value {
        "clear" => Ok(CustomerRiskStatus::Clear),
        "review_required" => Ok(CustomerRiskStatus::ReviewRequired),
        "blocked" => Ok(CustomerRiskStatus::Blocked),
        _ => Err(rusqlite::Error::InvalidColumnType(
            0,
            "risk_status".into(),
            rusqlite::types::Type::Text,
        )),
    }
}

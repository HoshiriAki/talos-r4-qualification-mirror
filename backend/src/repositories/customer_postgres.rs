#![cfg(feature = "postgres")]

use chrono::{SecondsFormat, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::domain::{
    ContactKind, CustomerId, CustomerRiskStatus, CustomerStatus, mask_contact_value,
};
use crate::repositories::RepositoryError;
use crate::repositories::customer::{
    CustomerContactProjection, CustomerMigrationExceptionProjection, CustomerProjection,
    DuplicateCustomerCandidate, NewCustomerContact, NewCustomerRecord,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresCustomerRepository<'a> {
    session: &'a RepositorySession,
}

struct RawCustomerContact {
    id: String,
    kind: String,
    normalized_value: String,
    is_primary: bool,
    classification: String,
    purpose: String,
}

struct RawCustomer {
    id: String,
    legal_name: String,
    display_name: String,
    status: String,
    risk_status: String,
    version: i64,
    contacts: Vec<RawCustomerContact>,
    created_at: String,
    updated_at: String,
}

enum ResolveOutcome {
    Resolved,
    NotFound,
    UnsupportedSource(String),
}

impl<'a> PostgresCustomerRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn create(
        &self,
        record: &NewCustomerRecord,
    ) -> Result<CustomerProjection, RepositoryError> {
        let legal_name = record.legal_name.trim().to_owned();
        let display_name = record.display_name.trim().to_owned();
        if legal_name.is_empty() || display_name.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "customer legal_name and display_name must not be blank".into(),
            ));
        }

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let customer_id = record.id.as_str().to_owned();
        let status = record.status.as_str().to_owned();
        let risk_status = record.risk_status.as_str().to_owned();
        let contacts = record.contacts.clone();
        let actor_identity_id = record.actor_identity_id.clone();
        let now = utc_now();

        self.session.pg_write(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "INSERT INTO customers \
                     (id, tenant_id, legal_name, display_name, status, risk_status, version, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, 1, $7, $7)",
                )
                .bind(&customer_id)
                .bind(&tenant_id)
                .bind(&legal_name)
                .bind(&display_name)
                .bind(&status)
                .bind(&risk_status)
                .bind(&now)
                .execute(&mut *connection)
                .await?;

                for contact in contacts {
                    sqlx::query(
                        "INSERT INTO customer_contacts \
                         (id, tenant_id, customer_id, kind, raw_value, normalized_value, is_primary, \
                          classification, purpose, created_at, updated_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, 'pii', 'rental_contact', $8, $8)",
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&tenant_id)
                    .bind(&customer_id)
                    .bind(contact.kind.as_str())
                    .bind(contact.raw_value.trim())
                    .bind(&contact.normalized_value)
                    .bind(contact.is_primary)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await?;
                }

                append_history_pg(
                    connection,
                    &tenant_id,
                    &customer_id,
                    "customer_created",
                    actor_identity_id.as_deref(),
                    "{}",
                    &now,
                )
                .await?;
                Ok(())
            })
        })?;

        self.get(&record.id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("created customer could not be reloaded".into())
        })
    }

    pub(in crate::repositories) fn list(
        &self,
        limit: u32,
    ) -> Result<Vec<CustomerProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let limit = i64::from(limit.clamp(1, 200));
        let raw = self.session.pg_read(move |connection| {
            Box::pin(async move {
                let rows = sqlx::query(
                    "SELECT id, legal_name, display_name, status, risk_status, version, created_at, updated_at \
                     FROM customers WHERE tenant_id = $1 \
                     ORDER BY updated_at DESC, id LIMIT $2",
                )
                .bind(&tenant_id)
                .bind(limit)
                .fetch_all(&mut *connection)
                .await?;

                let mut customers = Vec::with_capacity(rows.len());
                for row in rows {
                    let id: String = row.try_get("id")?;
                    let contacts = load_contacts_pg(connection, &tenant_id, &id).await?;
                    customers.push(RawCustomer {
                        id,
                        legal_name: row.try_get("legal_name")?,
                        display_name: row.try_get("display_name")?,
                        status: row.try_get("status")?,
                        risk_status: row.try_get("risk_status")?,
                        version: row.try_get("version")?,
                        contacts,
                        created_at: row.try_get("created_at")?,
                        updated_at: row.try_get("updated_at")?,
                    });
                }
                Ok(customers)
            })
        })?;

        raw.into_iter().map(project_customer).collect()
    }

    pub(in crate::repositories) fn get(
        &self,
        id: &CustomerId,
    ) -> Result<Option<CustomerProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.as_str().to_owned();
        let raw = self.session.pg_read(move |connection| {
            Box::pin(async move { load_customer_pg(connection, &tenant_id, &id).await })
        })?;
        raw.map(project_customer).transpose()
    }

    pub(in crate::repositories) fn add_contact(
        &self,
        customer_id: &CustomerId,
        contact: &NewCustomerContact,
        actor_identity_id: Option<&str>,
    ) -> Result<CustomerProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let customer_id_text = customer_id.as_str().to_owned();
        let contact = contact.clone();
        let actor_identity_id = actor_identity_id.map(str::to_owned);
        let now = utc_now();

        let updated = self.session.pg_write(move |connection| {
            Box::pin(async move {
                if !customer_exists_pg(connection, &tenant_id, &customer_id_text).await? {
                    return Ok(false);
                }
                sqlx::query(
                    "INSERT INTO customer_contacts \
                     (id, tenant_id, customer_id, kind, raw_value, normalized_value, is_primary, \
                      classification, purpose, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, 'pii', 'rental_contact', $8, $8)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&tenant_id)
                .bind(&customer_id_text)
                .bind(contact.kind.as_str())
                .bind(contact.raw_value.trim())
                .bind(&contact.normalized_value)
                .bind(contact.is_primary)
                .bind(&now)
                .execute(&mut *connection)
                .await?;
                sqlx::query(
                    "UPDATE customers SET version = version + 1, updated_at = $1 \
                     WHERE tenant_id = $2 AND id = $3",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(&customer_id_text)
                .execute(&mut *connection)
                .await?;
                let detail = serde_json::json!({"kind": contact.kind.as_str()}).to_string();
                append_history_pg(
                    connection,
                    &tenant_id,
                    &customer_id_text,
                    "customer_contact_added",
                    actor_identity_id.as_deref(),
                    &detail,
                    &now,
                )
                .await?;
                Ok(true)
            })
        })?;
        if !updated {
            return Err(RepositoryError::ContractViolation(
                "customer was not found in the bound tenant".into(),
            ));
        }
        self.get(customer_id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("updated customer could not be reloaded".into())
        })
    }

    pub(in crate::repositories) fn add_external_identity(
        &self,
        customer_id: &CustomerId,
        provider: &str,
        external_subject: &str,
        actor_identity_id: Option<&str>,
    ) -> Result<(), RepositoryError> {
        let provider = provider.trim().to_ascii_lowercase();
        let external_subject = external_subject.trim().to_owned();
        if provider.is_empty() || external_subject.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "external identity provider and subject must not be blank".into(),
            ));
        }

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let customer_id = customer_id.as_str().to_owned();
        let actor_identity_id = actor_identity_id.map(str::to_owned);
        let now = utc_now();
        let inserted = self.session.pg_write(move |connection| {
            Box::pin(async move {
                if !customer_exists_pg(connection, &tenant_id, &customer_id).await? {
                    return Ok(false);
                }
                sqlx::query(
                    "INSERT INTO customer_external_identities \
                     (id, tenant_id, customer_id, provider, external_subject, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $6)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&tenant_id)
                .bind(&customer_id)
                .bind(&provider)
                .bind(&external_subject)
                .bind(&now)
                .execute(&mut *connection)
                .await?;
                let detail = serde_json::json!({"provider": provider}).to_string();
                append_history_pg(
                    connection,
                    &tenant_id,
                    &customer_id,
                    "customer_external_identity_added",
                    actor_identity_id.as_deref(),
                    &detail,
                    &now,
                )
                .await?;
                Ok(true)
            })
        })?;
        if inserted {
            Ok(())
        } else {
            Err(RepositoryError::ContractViolation(
                "customer was not found in the bound tenant".into(),
            ))
        }
    }

    pub(in crate::repositories) fn duplicate_candidates(
        &self,
        kind: ContactKind,
        normalized_value: &str,
    ) -> Result<Vec<DuplicateCustomerCandidate>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let kind_text = kind.as_str().to_owned();
        let normalized_value = normalized_value.to_owned();
        let masked = mask_contact_value(kind, &normalized_value);
        let rows = self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT DISTINCT c.id, c.display_name \
                     FROM customers c \
                     JOIN customer_contacts cc \
                       ON cc.tenant_id = c.tenant_id AND cc.customer_id = c.id \
                     WHERE c.tenant_id = $1 AND cc.kind = $2 AND cc.normalized_value = $3 \
                     ORDER BY c.display_name, c.id",
                )
                .bind(&tenant_id)
                .bind(&kind_text)
                .bind(&normalized_value)
                .fetch_all(connection)
                .await
            })
        })?;

        rows.into_iter()
            .map(|row| {
                let id: String = row
                    .try_get("id")
                    .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
                Ok(DuplicateCustomerCandidate {
                    customer_id: CustomerId::parse(id).map_err(|_| {
                        RepositoryError::ContractViolation(
                            "persisted customer id is not a UUID".into(),
                        )
                    })?,
                    display_name: row
                        .try_get("display_name")
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?,
                    matched_contact: masked.clone(),
                })
            })
            .collect()
    }

    pub(in crate::repositories) fn list_migration_exceptions(
        &self,
        limit: u32,
    ) -> Result<Vec<CustomerMigrationExceptionProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let limit = i64::from(limit.clamp(1, 200));
        let rows = self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT id, source_table, source_row_id, reason, status, \
                            resolved_customer_id, created_at, resolved_at \
                     FROM customer_migration_exceptions \
                     WHERE tenant_id = $1 AND status = 'pending' \
                     ORDER BY created_at, id LIMIT $2",
                )
                .bind(&tenant_id)
                .bind(limit)
                .fetch_all(connection)
                .await
            })
        })?;

        rows.into_iter()
            .map(|row| {
                let resolved: Option<String> = row
                    .try_get("resolved_customer_id")
                    .map_err(|error| RepositoryError::Postgres(error.to_string()))?;
                Ok(CustomerMigrationExceptionProjection {
                    id: row
                        .try_get("id")
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?,
                    source_table: row
                        .try_get("source_table")
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?,
                    source_row_id: row
                        .try_get("source_row_id")
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?,
                    reason: row
                        .try_get("reason")
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?,
                    status: row
                        .try_get("status")
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?,
                    resolved_customer_id: resolved.map(CustomerId::parse).transpose().map_err(
                        |_| {
                            RepositoryError::ContractViolation(
                                "persisted resolved customer id is not a UUID".into(),
                            )
                        },
                    )?,
                    created_at: row
                        .try_get("created_at")
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?,
                    resolved_at: row
                        .try_get("resolved_at")
                        .map_err(|error| RepositoryError::Postgres(error.to_string()))?,
                })
            })
            .collect()
    }

    pub(in crate::repositories) fn resolve_migration_exception(
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
        let customer_id = customer_id.as_str().to_owned();
        let exception_id = exception_id.to_owned();
        let actor_identity_id = actor_identity_id.map(str::to_owned);
        let now = utc_now();

        let outcome = self.session.pg_write_serializable(move |connection| {
            Box::pin(async move {
                if !customer_exists_pg(connection, &tenant_id, &customer_id).await? {
                    return Ok(ResolveOutcome::NotFound);
                }
                let source = sqlx::query(
                    "SELECT source_table, source_row_id \
                     FROM customer_migration_exceptions \
                     WHERE tenant_id = $1 AND id = $2 AND status = 'pending'",
                )
                .bind(&tenant_id)
                .bind(&exception_id)
                .fetch_optional(&mut *connection)
                .await?;
                let Some(source) = source else {
                    return Ok(ResolveOutcome::NotFound);
                };
                let source_table: String = source.try_get("source_table")?;
                let source_row_id: String = source.try_get("source_row_id")?;

                let updated = match source_table.as_str() {
                    "orders" => {
                        sqlx::query(
                            "UPDATE orders SET customer_id = $1 \
                             WHERE tenant_id = $2 AND id = $3",
                        )
                        .bind(&customer_id)
                        .bind(&tenant_id)
                        .bind(&source_row_id)
                        .execute(&mut *connection)
                        .await?
                        .rows_affected()
                    }
                    "blacklist" => update_legacy_customer_pg(
                        connection,
                        "UPDATE blacklist SET customer_id = $1 WHERE tenant_id = $2 AND id::text = $3",
                        &customer_id,
                        &tenant_id,
                        &source_row_id,
                    )
                    .await?,
                    "violations" => update_legacy_customer_pg(
                        connection,
                        "UPDATE violations SET customer_id = $1 WHERE tenant_id = $2 AND id::text = $3",
                        &customer_id,
                        &tenant_id,
                        &source_row_id,
                    )
                    .await?,
                    "credit_scores" => update_legacy_customer_pg(
                        connection,
                        "UPDATE credit_scores SET customer_id = $1 WHERE tenant_id = $2 AND id::text = $3",
                        &customer_id,
                        &tenant_id,
                        &source_row_id,
                    )
                    .await?,
                    "overdue_records" => update_legacy_customer_pg(
                        connection,
                        "UPDATE overdue_records SET customer_id = $1 WHERE tenant_id = $2 AND id::text = $3",
                        &customer_id,
                        &tenant_id,
                        &source_row_id,
                    )
                    .await?,
                    "contracts" => update_legacy_customer_pg(
                        connection,
                        "UPDATE contracts SET customer_id = $1 WHERE tenant_id = $2 AND id::text = $3",
                        &customer_id,
                        &tenant_id,
                        &source_row_id,
                    )
                    .await?,
                    _ => return Ok(ResolveOutcome::UnsupportedSource(source_table)),
                };
                if updated != 1 {
                    return Ok(ResolveOutcome::NotFound);
                }

                let resolved = sqlx::query(
                    "UPDATE customer_migration_exceptions \
                     SET status = 'resolved', resolved_customer_id = $1, resolved_at = $2 \
                     WHERE tenant_id = $3 AND id = $4 AND status = 'pending'",
                )
                .bind(&customer_id)
                .bind(&now)
                .bind(&tenant_id)
                .bind(&exception_id)
                .execute(&mut *connection)
                .await?
                .rows_affected();
                if resolved != 1 {
                    return Ok(ResolveOutcome::NotFound);
                }

                let detail = serde_json::json!({
                    "sourceTable": source_table,
                    "sourceRowId": source_row_id,
                })
                .to_string();
                append_history_pg(
                    connection,
                    &tenant_id,
                    &customer_id,
                    "legacy_customer_association_resolved",
                    actor_identity_id.as_deref(),
                    &detail,
                    &now,
                )
                .await?;
                Ok(ResolveOutcome::Resolved)
            })
        })?;

        match outcome {
            ResolveOutcome::Resolved => Ok(()),
            ResolveOutcome::NotFound => Err(RepositoryError::ContractViolation(
                "migration exception or source row was not found in the bound tenant".into(),
            )),
            ResolveOutcome::UnsupportedSource(source) => Err(RepositoryError::ContractViolation(
                format!("unsupported customer migration source: {source}"),
            )),
        }
    }

    pub(in crate::repositories) fn anonymize(
        &self,
        customer_id: &CustomerId,
        actor_identity_id: Option<&str>,
    ) -> Result<CustomerProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let customer_id_text = customer_id.as_str().to_owned();
        let anonymous_label = format!("ANON-{}", &customer_id_text[..8]);
        let actor_identity_id = actor_identity_id.map(str::to_owned);
        let now = utc_now();

        let updated = self.session.pg_write(move |connection| {
            Box::pin(async move {
                if !customer_exists_pg(connection, &tenant_id, &customer_id_text).await? {
                    return Ok(false);
                }
                sqlx::query(
                    "DELETE FROM customer_contacts WHERE tenant_id = $1 AND customer_id = $2",
                )
                .bind(&tenant_id)
                .bind(&customer_id_text)
                .execute(&mut *connection)
                .await?;
                sqlx::query(
                    "DELETE FROM customer_external_identities \
                     WHERE tenant_id = $1 AND customer_id = $2",
                )
                .bind(&tenant_id)
                .bind(&customer_id_text)
                .execute(&mut *connection)
                .await?;
                sqlx::query(
                    "UPDATE customers \
                     SET legal_name = $1, display_name = $1, status = 'anonymized', \
                         version = version + 1, updated_at = $2 \
                     WHERE tenant_id = $3 AND id = $4",
                )
                .bind(&anonymous_label)
                .bind(&now)
                .bind(&tenant_id)
                .bind(&customer_id_text)
                .execute(&mut *connection)
                .await?;
                append_history_pg(
                    connection,
                    &tenant_id,
                    &customer_id_text,
                    "customer_anonymized",
                    actor_identity_id.as_deref(),
                    "{}",
                    &now,
                )
                .await?;
                Ok(true)
            })
        })?;
        if !updated {
            return Err(RepositoryError::ContractViolation(
                "customer was not found in the bound tenant".into(),
            ));
        }
        self.get(customer_id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("anonymized customer could not be reloaded".into())
        })
    }
}

async fn load_customer_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    id: &str,
) -> Result<Option<RawCustomer>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, legal_name, display_name, status, risk_status, version, created_at, updated_at \
         FROM customers WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: String = row.try_get("id")?;
    let contacts = load_contacts_pg(connection, tenant_id, &id).await?;
    Ok(Some(RawCustomer {
        id,
        legal_name: row.try_get("legal_name")?,
        display_name: row.try_get("display_name")?,
        status: row.try_get("status")?,
        risk_status: row.try_get("risk_status")?,
        version: row.try_get("version")?,
        contacts,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    }))
}

async fn load_contacts_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    customer_id: &str,
) -> Result<Vec<RawCustomerContact>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, kind, normalized_value, is_primary, classification, purpose \
         FROM customer_contacts \
         WHERE tenant_id = $1 AND customer_id = $2 \
         ORDER BY is_primary DESC, kind, id",
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_all(connection)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(RawCustomerContact {
                id: row.try_get("id")?,
                kind: row.try_get("kind")?,
                normalized_value: row.try_get("normalized_value")?,
                is_primary: row.try_get("is_primary")?,
                classification: row.try_get("classification")?,
                purpose: row.try_get("purpose")?,
            })
        })
        .collect()
}

async fn customer_exists_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    customer_id: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM customers WHERE tenant_id = $1 AND id = $2)",
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_one(connection)
    .await
}

async fn append_history_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    customer_id: &str,
    event_type: &str,
    actor_identity_id: Option<&str>,
    detail_json: &str,
    created_at: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO customer_history \
         (id, tenant_id, customer_id, event_type, detail_json, actor_identity_id, created_at) \
         VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(tenant_id)
    .bind(customer_id)
    .bind(event_type)
    .bind(detail_json)
    .bind(actor_identity_id)
    .bind(created_at)
    .execute(connection)
    .await?;
    Ok(())
}

async fn update_legacy_customer_pg(
    connection: &mut sqlx::PgConnection,
    query_text: &str,
    customer_id: &str,
    tenant_id: &str,
    source_row_id: &str,
) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(query_text)
        .bind(customer_id)
        .bind(tenant_id)
        .bind(source_row_id)
        .execute(connection)
        .await?
        .rows_affected())
}

fn project_customer(raw: RawCustomer) -> Result<CustomerProjection, RepositoryError> {
    let id = CustomerId::parse(raw.id).map_err(|_| {
        RepositoryError::ContractViolation("persisted customer id is not a UUID".into())
    })?;
    let status = parse_customer_status(&raw.status)?;
    let risk_status = parse_customer_risk_status(&raw.risk_status)?;
    let contacts = raw
        .contacts
        .into_iter()
        .map(|contact| {
            let kind = parse_contact_kind(&contact.kind)?;
            Ok(CustomerContactProjection {
                id: contact.id,
                kind,
                masked_value: mask_contact_value(kind, &contact.normalized_value),
                is_primary: contact.is_primary,
                classification: contact.classification,
                purpose: contact.purpose,
            })
        })
        .collect::<Result<Vec<_>, RepositoryError>>()?;

    Ok(CustomerProjection {
        id,
        legal_name: raw.legal_name,
        display_name: raw.display_name,
        status,
        risk_status,
        version: raw.version,
        contacts,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
    })
}

fn parse_contact_kind(value: &str) -> Result<ContactKind, RepositoryError> {
    match value {
        "phone" => Ok(ContactKind::Phone),
        "email" => Ok(ContactKind::Email),
        "other" => Ok(ContactKind::Other),
        _ => Err(RepositoryError::ContractViolation(
            "persisted customer contact kind is invalid".into(),
        )),
    }
}

fn parse_customer_status(value: &str) -> Result<CustomerStatus, RepositoryError> {
    match value {
        "active" => Ok(CustomerStatus::Active),
        "inactive" => Ok(CustomerStatus::Inactive),
        "anonymized" => Ok(CustomerStatus::Anonymized),
        _ => Err(RepositoryError::ContractViolation(
            "persisted customer status is invalid".into(),
        )),
    }
}

fn parse_customer_risk_status(value: &str) -> Result<CustomerRiskStatus, RepositoryError> {
    match value {
        "clear" => Ok(CustomerRiskStatus::Clear),
        "review_required" => Ok(CustomerRiskStatus::ReviewRequired),
        "blocked" => Ok(CustomerRiskStatus::Blocked),
        _ => Err(RepositoryError::ContractViolation(
            "persisted customer risk status is invalid".into(),
        )),
    }
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

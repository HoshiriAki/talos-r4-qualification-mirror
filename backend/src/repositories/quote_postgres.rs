#![cfg(feature = "postgres")]

use chrono::{SecondsFormat, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::domain::{CustomerId, Money, QuoteId, QuoteLineId, QuoteLineKind, QuoteStatus};
use crate::repositories::RepositoryError;
use crate::repositories::quote::{
    AccessoryProjection, NewQuote, OrderFromQuoteProjection, QuoteLineProjection, QuoteProjection,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresQuoteRepository<'a> {
    session: &'a RepositorySession,
}

struct RawQuoteLine {
    id: String,
    kind: String,
    reference_id: String,
    description: String,
    quantity: i64,
    unit_price_minor: i64,
    subtotal_minor: i64,
    currency: String,
    price_snapshot_json: String,
}

struct RawQuote {
    id: String,
    customer_id: String,
    status: String,
    start_date: String,
    end_date: String,
    region: String,
    currency: String,
    total_minor: i64,
    version: i64,
    expires_at: String,
    confirmed_at: Option<String>,
    converted_order_id: Option<String>,
    lines: Vec<RawQuoteLine>,
    created_at: String,
    updated_at: String,
}

struct QuoteConversion {
    order_id: String,
    order_no: String,
    quote_id: String,
    customer_id: String,
    total_minor: i64,
    currency: String,
}

impl<'a> PostgresQuoteRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn upsert_accessory(
        &self,
        id: &str,
        sku: &str,
        name: &str,
        unit_price: &Money,
        active: bool,
    ) -> Result<AccessoryProjection, RepositoryError> {
        let id = id.trim().to_owned();
        let sku = sku.trim().to_owned();
        let name = name.trim().to_owned();
        if id.is_empty() || sku.is_empty() || name.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "accessory id, sku and name must not be blank".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let currency = unit_price.currency.clone();
        let minor = unit_price.minor;
        let now = utc_now();
        let accessory_id = id.clone();
        self.session.pg_write(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "INSERT INTO accessory_catalog \
                     (id, tenant_id, sku, name, unit_price_minor, currency, active, version, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8, $8) \
                     ON CONFLICT(id) DO UPDATE SET \
                       sku = EXCLUDED.sku, \
                       name = EXCLUDED.name, \
                       unit_price_minor = EXCLUDED.unit_price_minor, \
                       currency = EXCLUDED.currency, \
                       active = EXCLUDED.active, \
                       version = accessory_catalog.version + 1, \
                       updated_at = EXCLUDED.updated_at \
                     WHERE accessory_catalog.tenant_id = EXCLUDED.tenant_id",
                )
                .bind(&accessory_id)
                .bind(&tenant_id)
                .bind(&sku)
                .bind(&name)
                .bind(minor)
                .bind(&currency)
                .bind(active)
                .bind(&now)
                .execute(connection)
                .await?;
                Ok(())
            })
        })?;
        self.get_accessory(&id)?.ok_or_else(|| {
            RepositoryError::ContractViolation(
                "accessory id already belongs to another tenant".into(),
            )
        })
    }

    pub(in crate::repositories) fn get_accessory(
        &self,
        id: &str,
    ) -> Result<Option<AccessoryProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.trim().to_owned();
        let row = self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT id, sku, name, unit_price_minor, currency, active, version \
                     FROM accessory_catalog WHERE tenant_id = $1 AND id = $2",
                )
                .bind(&tenant_id)
                .bind(&id)
                .fetch_optional(connection)
                .await
            })
        })?;
        row.map(|row| {
            let minor: i64 = pg_get(&row, "unit_price_minor")?;
            let currency: String = pg_get(&row, "currency")?;
            Ok(AccessoryProjection {
                id: pg_get(&row, "id")?,
                sku: pg_get(&row, "sku")?,
                name: pg_get(&row, "name")?,
                unit_price: Money::new(minor, currency)
                    .map_err(RepositoryError::ContractViolation)?,
                active: pg_get(&row, "active")?,
                version: pg_get(&row, "version")?,
            })
        })
        .transpose()
    }

    pub(in crate::repositories) fn create(
        &self,
        quote: &NewQuote,
    ) -> Result<QuoteProjection, RepositoryError> {
        if quote.lines.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "quote must contain at least one line".into(),
            ));
        }
        let currency = quote.currency.trim().to_ascii_uppercase();
        let mut total =
            Money::new(0, currency.clone()).map_err(RepositoryError::ContractViolation)?;
        for line in &quote.lines {
            if line.quantity == 0 {
                return Err(RepositoryError::ContractViolation(
                    "quote line quantity must be greater than zero".into(),
                ));
            }
            if line.unit_price.currency != currency || line.subtotal.currency != currency {
                return Err(RepositoryError::ContractViolation(
                    "all quote lines must use the quote currency".into(),
                ));
            }
            let expected = line
                .unit_price
                .checked_mul(line.quantity)
                .map_err(RepositoryError::ContractViolation)?;
            if expected != line.subtotal {
                return Err(RepositoryError::ContractViolation(
                    "quote line subtotal must be derived from server unit price".into(),
                ));
            }
            total = total
                .checked_add(&line.subtotal)
                .map_err(RepositoryError::ContractViolation)?;
        }

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let quote_id = quote.id.as_str().to_owned();
        let customer_id = quote.customer_id.as_str().to_owned();
        let start_date = quote.start_date.clone();
        let end_date = quote.end_date.clone();
        let region = quote.region.clone();
        let expires_at = quote.expires_at.clone();
        let lines = quote.lines.clone();
        let total_minor = total.minor;
        let now = utc_now();

        let created = self.session.pg_write(move |connection| {
            Box::pin(async move {
                let customer_exists = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM customers WHERE tenant_id = $1 AND id = $2)",
                )
                .bind(&tenant_id)
                .bind(&customer_id)
                .fetch_one(&mut *connection)
                .await?;
                if !customer_exists {
                    return Ok(false);
                }

                sqlx::query(
                    "INSERT INTO quotes \
                     (id, tenant_id, customer_id, status, start_date, end_date, region, \
                      currency, total_minor, version, expires_at, created_at, updated_at) \
                     VALUES ($1, $2, $3, 'draft', $4, $5, $6, $7, $8, 1, $9, $10, $10)",
                )
                .bind(&quote_id)
                .bind(&tenant_id)
                .bind(&customer_id)
                .bind(&start_date)
                .bind(&end_date)
                .bind(&region)
                .bind(&currency)
                .bind(total_minor)
                .bind(&expires_at)
                .bind(&now)
                .execute(&mut *connection)
                .await?;

                for line in lines {
                    sqlx::query(
                        "INSERT INTO quote_lines \
                         (id, tenant_id, quote_id, line_kind, reference_id, description, quantity, \
                          unit_price_minor, subtotal_minor, currency, price_snapshot_json, created_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb, $12)",
                    )
                    .bind(line.id.as_str())
                    .bind(&tenant_id)
                    .bind(&quote_id)
                    .bind(line.kind.as_str())
                    .bind(&line.reference_id)
                    .bind(&line.description)
                    .bind(i64::from(line.quantity))
                    .bind(line.unit_price.minor)
                    .bind(line.subtotal.minor)
                    .bind(&line.subtotal.currency)
                    .bind(&line.price_snapshot_json)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await?;
                }
                Ok(true)
            })
        })?;
        if !created {
            return Err(RepositoryError::ContractViolation(
                "quote customer was not found in the bound tenant".into(),
            ));
        }
        self.get(&quote.id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("created quote could not be reloaded".into())
        })
    }

    pub(in crate::repositories) fn list(
        &self,
        limit: u32,
    ) -> Result<Vec<QuoteProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let limit = i64::from(limit.clamp(1, 200));
        let ids = self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query_scalar::<_, String>(
                    "SELECT id FROM quotes WHERE tenant_id = $1 \
                     ORDER BY created_at DESC, id LIMIT $2",
                )
                .bind(&tenant_id)
                .bind(limit)
                .fetch_all(connection)
                .await
            })
        })?;
        ids.into_iter()
            .map(|id| {
                let id = QuoteId::parse(id).map_err(|_| {
                    RepositoryError::ContractViolation("persisted quote id is not a UUID".into())
                })?;
                self.get(&id)?.ok_or_else(|| {
                    RepositoryError::ContractViolation("listed quote disappeared".into())
                })
            })
            .collect()
    }

    pub(in crate::repositories) fn get(
        &self,
        id: &QuoteId,
    ) -> Result<Option<QuoteProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.as_str().to_owned();
        let raw = self.session.pg_read(move |connection| {
            Box::pin(async move { load_quote_pg(connection, &tenant_id, &id).await })
        })?;
        raw.map(project_quote).transpose()
    }

    pub(in crate::repositories) fn confirm(
        &self,
        id: &QuoteId,
    ) -> Result<QuoteProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id_text = id.as_str().to_owned();
        let now = utc_now();
        let changed = self.session.pg_write(move |connection| {
            Box::pin(async move {
                Ok(sqlx::query(
                    "UPDATE quotes SET status = 'confirmed', confirmed_at = $1, \
                                       version = version + 1, updated_at = $1 \
                     WHERE tenant_id = $2 AND id = $3 AND status = 'draft' AND expires_at > $1",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(&id_text)
                .execute(connection)
                .await?
                .rows_affected())
            })
        })?;
        if changed != 1 {
            return Err(RepositoryError::ContractViolation(
                "quote could not be confirmed in the bound tenant".into(),
            ));
        }
        self.get(id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("confirmed quote could not be reloaded".into())
        })
    }

    pub(in crate::repositories) fn expire(
        &self,
        id: &QuoteId,
    ) -> Result<QuoteProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id_text = id.as_str().to_owned();
        let now = utc_now();
        let changed = self.session.pg_write(move |connection| {
            Box::pin(async move {
                Ok(sqlx::query(
                    "UPDATE quotes SET status = 'expired', version = version + 1, updated_at = $1 \
                     WHERE tenant_id = $2 AND id = $3 AND status = 'draft' AND expires_at <= $1",
                )
                .bind(&now)
                .bind(&tenant_id)
                .bind(&id_text)
                .execute(connection)
                .await?
                .rows_affected())
            })
        })?;
        if changed != 1 {
            return Err(RepositoryError::ContractViolation(
                "quote could not be expired in the bound tenant".into(),
            ));
        }
        self.get(id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("expired quote could not be reloaded".into())
        })
    }

    pub(in crate::repositories) fn create_order_from_quote(
        &self,
        quote_id: &QuoteId,
        remark: &str,
    ) -> Result<OrderFromQuoteProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let quote_id_text = quote_id.as_str().to_owned();
        let remark = remark.to_owned();
        let now = utc_now();
        let order_id = Uuid::new_v4().to_string();
        let order_no = format!(
            "ORD-{}-{}",
            Utc::now().format("%Y%m%d%H%M%S"),
            &Uuid::new_v4().simple().to_string()[..8]
        );
        let order_id_for_tx = order_id.clone();
        let order_no_for_tx = order_no.clone();

        let conversion = self.session.pg_write_serializable(move |connection| {
            Box::pin(async move {
                let quote = sqlx::query(
                    "SELECT customer_id, start_date, end_date, region, total_minor, currency \
                     FROM quotes \
                     WHERE tenant_id = $1 AND id = $2 AND status = 'confirmed' \
                       AND converted_order_id IS NULL AND expires_at > $3 \
                     FOR UPDATE",
                )
                .bind(&tenant_id)
                .bind(&quote_id_text)
                .bind(&now)
                .fetch_optional(&mut *connection)
                .await?;
                let Some(quote) = quote else {
                    return Ok(None);
                };
                let customer_id: String = quote.try_get("customer_id")?;
                let start_date: String = quote.try_get("start_date")?;
                let end_date: String = quote.try_get("end_date")?;
                let region: String = quote.try_get("region")?;
                let total_minor: i64 = quote.try_get("total_minor")?;
                let currency: String = quote.try_get("currency")?;

                let line_rows = sqlx::query(
                    "SELECT id, line_kind, reference_id, description, quantity, unit_price_minor, \
                            subtotal_minor, currency, price_snapshot_json::text AS price_snapshot_json \
                     FROM quote_lines WHERE tenant_id = $1 AND quote_id = $2 \
                     ORDER BY created_at, id",
                )
                .bind(&tenant_id)
                .bind(&quote_id_text)
                .fetch_all(&mut *connection)
                .await?;

                sqlx::query(
                    "INSERT INTO orders \
                     (id, orderno, startdate, enddate, deliverydate, pickupmethods, address, notes, \
                      deviceserialno, createdat, tenant_id, customer_id, source_quote_id, \
                      total_minor, currency, province) \
                     VALUES ($1, $2, $3, $4, $3, '[]', '', $5, '', $6, $7, $8, $9, $10, $11, $12)",
                )
                .bind(&order_id_for_tx)
                .bind(&order_no_for_tx)
                .bind(&start_date)
                .bind(&end_date)
                .bind(&remark)
                .bind(&now)
                .bind(&tenant_id)
                .bind(&customer_id)
                .bind(&quote_id_text)
                .bind(total_minor)
                .bind(&currency)
                .bind(&region)
                .execute(&mut *connection)
                .await?;

                for line in line_rows {
                    let source_quote_line_id: String = line.try_get("id")?;
                    let line_kind: String = line.try_get("line_kind")?;
                    let reference_id: String = line.try_get("reference_id")?;
                    let description: String = line.try_get("description")?;
                    let quantity: i64 = line.try_get("quantity")?;
                    let unit_price_minor: i64 = line.try_get("unit_price_minor")?;
                    let subtotal_minor: i64 = line.try_get("subtotal_minor")?;
                    let line_currency: String = line.try_get("currency")?;
                    let snapshot: String = line.try_get("price_snapshot_json")?;
                    sqlx::query(
                        "INSERT INTO order_lines \
                         (id, tenant_id, order_id, source_quote_line_id, line_kind, reference_id, \
                          description, quantity, unit_price_minor, subtotal_minor, currency, \
                          price_snapshot_json, created_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::jsonb, $13)",
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&tenant_id)
                    .bind(&order_id_for_tx)
                    .bind(&source_quote_line_id)
                    .bind(&line_kind)
                    .bind(&reference_id)
                    .bind(&description)
                    .bind(quantity)
                    .bind(unit_price_minor)
                    .bind(subtotal_minor)
                    .bind(&line_currency)
                    .bind(&snapshot)
                    .bind(&now)
                    .execute(&mut *connection)
                    .await?;
                }

                let changed = sqlx::query(
                    "UPDATE quotes SET status = 'converted', converted_order_id = $1, \
                                       version = version + 1, updated_at = $2 \
                     WHERE tenant_id = $3 AND id = $4 AND status = 'confirmed' \
                       AND converted_order_id IS NULL",
                )
                .bind(&order_id_for_tx)
                .bind(&now)
                .bind(&tenant_id)
                .bind(&quote_id_text)
                .execute(&mut *connection)
                .await?
                .rows_affected();
                if changed != 1 {
                    return Ok(None);
                }

                Ok(Some(QuoteConversion {
                    order_id: order_id_for_tx,
                    order_no: order_no_for_tx,
                    quote_id: quote_id_text,
                    customer_id,
                    total_minor,
                    currency,
                }))
            })
        })?;
        let conversion = conversion.ok_or_else(|| {
            RepositoryError::ContractViolation(
                "confirmed quote could not be converted in the bound tenant".into(),
            )
        })?;
        Ok(OrderFromQuoteProjection {
            order_id: conversion.order_id,
            order_no: conversion.order_no,
            quote_id: QuoteId::parse(conversion.quote_id).map_err(|_| {
                RepositoryError::ContractViolation("persisted quote id is not a UUID".into())
            })?,
            customer_id: CustomerId::parse(conversion.customer_id).map_err(|_| {
                RepositoryError::ContractViolation("persisted customer id is not a UUID".into())
            })?,
            total: Money::new(conversion.total_minor, conversion.currency)
                .map_err(RepositoryError::ContractViolation)?,
        })
    }
}

async fn load_quote_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    id: &str,
) -> Result<Option<RawQuote>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, customer_id, status, start_date, end_date, region, currency, \
                total_minor, version, expires_at, confirmed_at, converted_order_id, \
                created_at, updated_at \
         FROM quotes WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: String = row.try_get("id")?;
    let customer_id: String = row.try_get("customer_id")?;
    let status: String = row.try_get("status")?;
    let start_date: String = row.try_get("start_date")?;
    let end_date: String = row.try_get("end_date")?;
    let region: String = row.try_get("region")?;
    let currency: String = row.try_get("currency")?;
    let total_minor: i64 = row.try_get("total_minor")?;
    let version: i64 = row.try_get("version")?;
    let expires_at: String = row.try_get("expires_at")?;
    let confirmed_at: Option<String> = row.try_get("confirmed_at")?;
    let converted_order_id: Option<String> = row.try_get("converted_order_id")?;
    let created_at: String = row.try_get("created_at")?;
    let updated_at: String = row.try_get("updated_at")?;

    let line_rows = sqlx::query(
        "SELECT id, line_kind, reference_id, description, quantity, unit_price_minor, \
                subtotal_minor, currency, price_snapshot_json::text AS price_snapshot_json \
         FROM quote_lines WHERE tenant_id = $1 AND quote_id = $2 ORDER BY created_at, id",
    )
    .bind(tenant_id)
    .bind(&id)
    .fetch_all(connection)
    .await?;
    let lines = line_rows
        .into_iter()
        .map(|line| {
            Ok(RawQuoteLine {
                id: line.try_get("id")?,
                kind: line.try_get("line_kind")?,
                reference_id: line.try_get("reference_id")?,
                description: line.try_get("description")?,
                quantity: line.try_get("quantity")?,
                unit_price_minor: line.try_get("unit_price_minor")?,
                subtotal_minor: line.try_get("subtotal_minor")?,
                currency: line.try_get("currency")?,
                price_snapshot_json: line.try_get("price_snapshot_json")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(Some(RawQuote {
        id,
        customer_id,
        status,
        start_date,
        end_date,
        region,
        currency,
        total_minor,
        version,
        expires_at,
        confirmed_at,
        converted_order_id,
        lines,
        created_at,
        updated_at,
    }))
}

fn project_quote(raw: RawQuote) -> Result<QuoteProjection, RepositoryError> {
    let id = QuoteId::parse(raw.id).map_err(|_| {
        RepositoryError::ContractViolation("persisted quote id is not a UUID".into())
    })?;
    let customer_id = CustomerId::parse(raw.customer_id).map_err(|_| {
        RepositoryError::ContractViolation("persisted customer id is not a UUID".into())
    })?;
    let status = parse_quote_status(&raw.status)?;
    let total = Money::new(raw.total_minor, raw.currency.clone())
        .map_err(RepositoryError::ContractViolation)?;
    let lines = raw
        .lines
        .into_iter()
        .map(|line| {
            let quantity = u32::try_from(line.quantity).map_err(|_| {
                RepositoryError::ContractViolation("persisted quote quantity is invalid".into())
            })?;
            let kind = parse_line_kind(&line.kind)?;
            let unit_price = Money::new(line.unit_price_minor, line.currency.clone())
                .map_err(RepositoryError::ContractViolation)?;
            let subtotal = Money::new(line.subtotal_minor, line.currency)
                .map_err(RepositoryError::ContractViolation)?;
            let price_snapshot = serde_json::from_str(&line.price_snapshot_json).map_err(|_| {
                RepositoryError::ContractViolation(
                    "persisted quote snapshot is invalid JSON".into(),
                )
            })?;
            Ok(QuoteLineProjection {
                id: QuoteLineId::parse(line.id).map_err(|_| {
                    RepositoryError::ContractViolation(
                        "persisted quote line id is not a UUID".into(),
                    )
                })?,
                kind,
                reference_id: line.reference_id,
                description: line.description,
                quantity,
                unit_price,
                subtotal,
                price_snapshot,
            })
        })
        .collect::<Result<Vec<_>, RepositoryError>>()?;

    Ok(QuoteProjection {
        id,
        customer_id,
        status,
        start_date: raw.start_date,
        end_date: raw.end_date,
        region: raw.region,
        total,
        version: raw.version,
        expires_at: raw.expires_at,
        confirmed_at: raw.confirmed_at,
        converted_order_id: raw.converted_order_id,
        lines,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
    })
}

fn parse_quote_status(value: &str) -> Result<QuoteStatus, RepositoryError> {
    match value {
        "draft" => Ok(QuoteStatus::Draft),
        "confirmed" => Ok(QuoteStatus::Confirmed),
        "expired" => Ok(QuoteStatus::Expired),
        "cancelled" => Ok(QuoteStatus::Cancelled),
        "converted" => Ok(QuoteStatus::Converted),
        _ => Err(RepositoryError::ContractViolation(
            "persisted quote status is invalid".into(),
        )),
    }
}

fn parse_line_kind(value: &str) -> Result<QuoteLineKind, RepositoryError> {
    match value {
        "model" => Ok(QuoteLineKind::Model),
        "accessory" => Ok(QuoteLineKind::Accessory),
        _ => Err(RepositoryError::ContractViolation(
            "persisted quote line kind is invalid".into(),
        )),
    }
}

fn pg_get<T>(row: &sqlx::postgres::PgRow, column: &str) -> Result<T, RepositoryError>
where
    T: for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(column)
        .map_err(|error| RepositoryError::Postgres(error.to_string()))
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

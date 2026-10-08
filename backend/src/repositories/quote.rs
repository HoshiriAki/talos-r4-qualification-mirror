use chrono::{Duration, SecondsFormat, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{CustomerId, Money, QuoteId, QuoteLineId, QuoteLineKind, QuoteStatus};
use crate::repositories::sqlite::SqliteRepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone)]
pub struct NewQuoteLine {
    pub id: QuoteLineId,
    pub kind: QuoteLineKind,
    pub reference_id: String,
    pub description: String,
    pub quantity: u32,
    pub unit_price: Money,
    pub subtotal: Money,
    pub price_snapshot_json: String,
}

#[derive(Debug, Clone)]
pub struct NewQuote {
    pub id: QuoteId,
    pub customer_id: CustomerId,
    pub start_date: String,
    pub end_date: String,
    pub region: String,
    pub currency: String,
    pub expires_at: String,
    pub lines: Vec<NewQuoteLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteLineProjection {
    pub id: QuoteLineId,
    pub kind: QuoteLineKind,
    pub reference_id: String,
    pub description: String,
    pub quantity: u32,
    pub unit_price: Money,
    pub subtotal: Money,
    pub price_snapshot: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteProjection {
    pub id: QuoteId,
    pub customer_id: CustomerId,
    pub status: QuoteStatus,
    pub start_date: String,
    pub end_date: String,
    pub region: String,
    pub total: Money,
    pub version: i64,
    pub expires_at: String,
    pub confirmed_at: Option<String>,
    pub converted_order_id: Option<String>,
    pub lines: Vec<QuoteLineProjection>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessoryProjection {
    pub id: String,
    pub sku: String,
    pub name: String,
    pub unit_price: Money,
    pub active: bool,
    pub version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderFromQuoteProjection {
    pub order_id: String,
    pub order_no: String,
    pub quote_id: QuoteId,
    pub customer_id: CustomerId,
    pub total: Money,
}

pub struct ScopedQuoteRepository<'a> {
    session: &'a SqliteRepositorySession,
}

impl<'a> ScopedQuoteRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub fn upsert_accessory(
        &self,
        id: &str,
        sku: &str,
        name: &str,
        unit_price: &Money,
        active: bool,
    ) -> Result<AccessoryProjection, RepositoryError> {
        let id = id.trim();
        let sku = sku.trim();
        let name = name.trim();
        if id.is_empty() || sku.is_empty() || name.is_empty() {
            return Err(RepositoryError::ContractViolation(
                "accessory id, sku and name must not be blank".into(),
            ));
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write(|transaction| {
            transaction.execute(
                "INSERT INTO accessory_catalog
                 (id, tenant_id, sku, name, unit_price_minor, currency, active, version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                   sku = excluded.sku,
                   name = excluded.name,
                   unit_price_minor = excluded.unit_price_minor,
                   currency = excluded.currency,
                   active = excluded.active,
                   version = accessory_catalog.version + 1,
                   updated_at = excluded.updated_at
                 WHERE accessory_catalog.tenant_id = excluded.tenant_id",
                params![
                    id,
                    tenant_id,
                    sku,
                    name,
                    unit_price.minor,
                    unit_price.currency,
                    if active { 1 } else { 0 },
                    now,
                ],
            )?;
            Ok(())
        })?;
        self.get_accessory(id)?.ok_or_else(|| {
            RepositoryError::ContractViolation(
                "accessory id already belongs to another tenant".into(),
            )
        })
    }

    pub fn get_accessory(&self, id: &str) -> Result<Option<AccessoryProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(|connection| {
            connection
                .query_row(
                    "SELECT id, sku, name, unit_price_minor, currency, active, version
                     FROM accessory_catalog WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, id],
                    |row| {
                        Ok(AccessoryProjection {
                            id: row.get(0)?,
                            sku: row.get(1)?,
                            name: row.get(2)?,
                            unit_price: Money::new(row.get(3)?, row.get::<_, String>(4)?)
                                .map_err(|error| sqlite_contract_error(3, error))?,
                            active: row.get::<_, i64>(5)? != 0,
                            version: row.get(6)?,
                        })
                    },
                )
                .optional()
        })
    }

    pub fn create(&self, quote: &NewQuote) -> Result<QuoteProjection, RepositoryError> {
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
        let now = utc_now();
        self.session.write(|transaction| {
            let customer_exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM customers WHERE tenant_id = ?1 AND id = ?2)",
                params![tenant_id, quote.customer_id.as_str()],
                |row| row.get(0),
            )?;
            if !customer_exists {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            transaction.execute(
                "INSERT INTO quotes
                 (id, tenant_id, customer_id, status, start_date, end_date, region,
                  currency, total_minor, version, expires_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'draft', ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10, ?10)",
                params![
                    quote.id.as_str(),
                    tenant_id,
                    quote.customer_id.as_str(),
                    quote.start_date,
                    quote.end_date,
                    quote.region,
                    currency,
                    total.minor,
                    quote.expires_at,
                    now,
                ],
            )?;
            for line in &quote.lines {
                transaction.execute(
                    "INSERT INTO quote_lines
                     (id, tenant_id, quote_id, line_kind, reference_id, description, quantity,
                      unit_price_minor, subtotal_minor, currency, price_snapshot_json, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        line.id.as_str(),
                        tenant_id,
                        quote.id.as_str(),
                        line.kind.as_str(),
                        line.reference_id,
                        line.description,
                        i64::from(line.quantity),
                        line.unit_price.minor,
                        line.subtotal.minor,
                        line.subtotal.currency,
                        line.price_snapshot_json,
                        now,
                    ],
                )?;
            }
            Ok(())
        })?;
        self.get(&quote.id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("created quote could not be reloaded".into())
        })
    }

    pub fn list(&self, limit: u32) -> Result<Vec<QuoteProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let limit = limit.clamp(1, 200);
        self.session.read(|connection| {
            let mut statement = connection.prepare(
                "SELECT id, customer_id, status, start_date, end_date, region, currency,
                        total_minor, version, expires_at, confirmed_at, converted_order_id,
                        created_at, updated_at
                 FROM quotes WHERE tenant_id = ?1 ORDER BY created_at DESC, id LIMIT ?2",
            )?;
            let ids = statement
                .query_map(params![tenant_id, i64::from(limit)], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| load_quote(connection, &tenant_id, &id))
                .collect()
        })
    }

    pub fn get(&self, id: &QuoteId) -> Result<Option<QuoteProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(|connection| {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM quotes WHERE tenant_id = ?1 AND id = ?2)",
                params![tenant_id, id.as_str()],
                |row| row.get(0),
            )?;
            if !exists {
                return Ok(None);
            }
            load_quote(connection, &tenant_id, id.as_str()).map(Some)
        })
    }

    pub fn confirm(&self, id: &QuoteId) -> Result<QuoteProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write(|transaction| {
            let changed = transaction.execute(
                "UPDATE quotes SET status = 'confirmed', confirmed_at = ?1,
                                   version = version + 1, updated_at = ?1
                 WHERE tenant_id = ?2 AND id = ?3 AND status = 'draft' AND expires_at > ?1",
                params![now, tenant_id, id.as_str()],
            )?;
            if changed != 1 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            Ok(())
        })?;
        self.get(id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("confirmed quote could not be reloaded".into())
        })
    }

    pub fn expire(&self, id: &QuoteId) -> Result<QuoteProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        self.session.write(|transaction| {
            let changed = transaction.execute(
                "UPDATE quotes SET status = 'expired', version = version + 1, updated_at = ?1
                 WHERE tenant_id = ?2 AND id = ?3 AND status = 'draft' AND expires_at <= ?1",
                params![now, tenant_id, id.as_str()],
            )?;
            if changed != 1 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            Ok(())
        })?;
        self.get(id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("expired quote could not be reloaded".into())
        })
    }

    pub fn create_order_from_quote(
        &self,
        quote_id: &QuoteId,
        remark: &str,
    ) -> Result<OrderFromQuoteProjection, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let now = utc_now();
        let order_id = Uuid::new_v4().to_string();
        let order_no = format!(
            "ORD-{}-{}",
            Utc::now().format("%Y%m%d%H%M%S"),
            &Uuid::new_v4().simple().to_string()[..8]
        );
        self.session.write(|transaction| {
            let quote: (String, String, String, String, i64, String, String) = transaction
                .query_row(
                    "SELECT customer_id, start_date, end_date, region, total_minor, currency, expires_at
                     FROM quotes
                     WHERE tenant_id = ?1 AND id = ?2 AND status = 'confirmed'
                       AND converted_order_id IS NULL",
                    params![tenant_id, quote_id.as_str()],
                    |row| {
                        Ok((
                            row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?,
                            row.get(5)?, row.get(6)?,
                        ))
                    },
                )?;
            if quote.6 <= now {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            let device_count: i64 = transaction.query_row(
                "SELECT COALESCE(SUM(quantity), 0) FROM quote_lines
                 WHERE tenant_id = ?1 AND quote_id = ?2 AND line_kind = 'model'",
                params![tenant_id, quote_id.as_str()],
                |row| row.get(0),
            )?;
            transaction.execute(
                "INSERT INTO orders
                 (id, orderNo, startDate, endDate, deliveryDate, pickupMethods, address, notes,
                  region, deviceCount, remark, tenant_id, customer_id, source_quote_id,
                  total_minor, currency, createdAt, updatedAt)
                 VALUES (?1, ?2, ?3, ?4, ?3, '[]', '', ?5, ?6, ?7, ?5, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
                params![
                    order_id,
                    order_no,
                    quote.1,
                    quote.2,
                    remark,
                    quote.3,
                    device_count,
                    tenant_id,
                    quote.0,
                    quote_id.as_str(),
                    quote.4,
                    quote.5,
                    now,
                ],
            )?;
            transaction.execute(
                "INSERT INTO order_lines
                 (id, tenant_id, order_id, source_quote_line_id, line_kind, reference_id,
                  description, quantity, unit_price_minor, subtotal_minor, currency,
                  price_snapshot_json, created_at)
                 SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' ||
                        substr(lower(hex(randomblob(2))),2) || '-' ||
                        substr('89ab',abs(random()) % 4 + 1,1) ||
                        substr(lower(hex(randomblob(2))),2) || '-' || lower(hex(randomblob(6))),
                        tenant_id, ?1, id, line_kind, reference_id, description, quantity,
                        unit_price_minor, subtotal_minor, currency, price_snapshot_json, ?2
                 FROM quote_lines WHERE tenant_id = ?3 AND quote_id = ?4",
                params![order_id, now, tenant_id, quote_id.as_str()],
            )?;
            let changed = transaction.execute(
                "UPDATE quotes SET status = 'converted', converted_order_id = ?1,
                                   version = version + 1, updated_at = ?2
                 WHERE tenant_id = ?3 AND id = ?4 AND status = 'confirmed'
                   AND converted_order_id IS NULL",
                params![order_id, now, tenant_id, quote_id.as_str()],
            )?;
            if changed != 1 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            Ok(())
        })?;
        let quote = self.get(quote_id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("converted quote could not be reloaded".into())
        })?;
        Ok(OrderFromQuoteProjection {
            order_id,
            order_no,
            quote_id: quote.id,
            customer_id: quote.customer_id,
            total: quote.total,
        })
    }

    pub fn default_expiry() -> String {
        (Utc::now() + Duration::days(7)).to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn load_quote(
    connection: &rusqlite::Connection,
    tenant_id: &str,
    id: &str,
) -> Result<QuoteProjection, rusqlite::Error> {
    let base = connection.query_row(
        "SELECT id, customer_id, status, start_date, end_date, region, currency,
                total_minor, version, expires_at, confirmed_at, converted_order_id,
                created_at, updated_at
         FROM quotes WHERE tenant_id = ?1 AND id = ?2",
        params![tenant_id, id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, String>(12)?,
                row.get::<_, String>(13)?,
            ))
        },
    )?;

    let mut line_statement = connection.prepare(
        "SELECT id, line_kind, reference_id, description, quantity, unit_price_minor,
                subtotal_minor, currency, price_snapshot_json
         FROM quote_lines WHERE tenant_id = ?1 AND quote_id = ?2 ORDER BY created_at, id",
    )?;
    let lines = line_statement
        .query_map(params![tenant_id, id], |row| {
            let kind_text: String = row.get(1)?;
            let currency: String = row.get(7)?;
            let snapshot_text: String = row.get(8)?;
            Ok(QuoteLineProjection {
                id: QuoteLineId::parse(row.get::<_, String>(0)?)
                    .map_err(|error| sqlite_contract_error(0, error.to_string()))?,
                kind: parse_line_kind(&kind_text)?,
                reference_id: row.get(2)?,
                description: row.get(3)?,
                quantity: u32::try_from(row.get::<_, i64>(4)?)
                    .map_err(|error| sqlite_contract_error(4, error.to_string()))?,
                unit_price: Money::new(row.get(5)?, currency.clone())
                    .map_err(|error| sqlite_contract_error(5, error))?,
                subtotal: Money::new(row.get(6)?, currency)
                    .map_err(|error| sqlite_contract_error(6, error))?,
                price_snapshot: serde_json::from_str(&snapshot_text)
                    .map_err(|error| sqlite_contract_error(8, error.to_string()))?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(QuoteProjection {
        id: QuoteId::parse(base.0).map_err(|error| sqlite_contract_error(0, error.to_string()))?,
        customer_id: CustomerId::parse(base.1)
            .map_err(|error| sqlite_contract_error(1, error.to_string()))?,
        status: parse_quote_status(&base.2)?,
        start_date: base.3,
        end_date: base.4,
        region: base.5,
        total: Money::new(base.7, base.6).map_err(|error| sqlite_contract_error(7, error))?,
        version: base.8,
        expires_at: base.9,
        confirmed_at: base.10,
        converted_order_id: base.11,
        lines,
        created_at: base.12,
        updated_at: base.13,
    })
}

fn parse_quote_status(value: &str) -> Result<QuoteStatus, rusqlite::Error> {
    match value {
        "draft" => Ok(QuoteStatus::Draft),
        "confirmed" => Ok(QuoteStatus::Confirmed),
        "expired" => Ok(QuoteStatus::Expired),
        "cancelled" => Ok(QuoteStatus::Cancelled),
        "converted" => Ok(QuoteStatus::Converted),
        _ => Err(sqlite_contract_error(2, "unknown quote status")),
    }
}

fn parse_line_kind(value: &str) -> Result<QuoteLineKind, rusqlite::Error> {
    match value {
        "model" => Ok(QuoteLineKind::Model),
        "accessory" => Ok(QuoteLineKind::Accessory),
        _ => Err(sqlite_contract_error(1, "unknown quote line kind")),
    }
}

fn sqlite_contract_error(index: usize, detail: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        std::io::Error::new(std::io::ErrorKind::InvalidData, detail.into()).into(),
    )
}

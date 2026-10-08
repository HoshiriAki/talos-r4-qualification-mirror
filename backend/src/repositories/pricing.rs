use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq)]
pub struct PricingConfigRecord {
    pub base_weekday_price: f64,
    pub base_weekend_price: f64,
    pub holiday_rules_json: String,
    pub receive_shipping_fees_json: String,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DynamicPriceRecord {
    pub date_key: String,
    pub price: f64,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelBasePriceRecord {
    pub weekday_price: f64,
    pub weekend_price: f64,
}

pub(in crate::repositories) struct SqlitePricingRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqlitePricingRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn get_or_create_config(
        &self,
        default_weekday: f64,
        default_weekend: f64,
        default_fees_json: &str,
        now: &str,
    ) -> Result<PricingConfigRecord, RepositoryError> {
        let tenant_id = self.tenant_id();
        let default_fees_json = default_fees_json.to_owned();
        let now = now.to_owned();

        self.session.write_immediate(move |transaction| {
            if let Some(record) = select_config_sqlite(transaction, &tenant_id)? {
                return Ok(record);
            }

            transaction
                .execute(
                    "INSERT INTO pricing_configs
                     (baseWeekdayPrice,baseWeekendPrice,holidayRulesJson,receiveShippingFeesJson,
                      updatedBy,createdAt,updatedAt,tenant_id)
                     VALUES (?1,?2,'[]',?3,'',?4,?4,?5)",
                    params![
                        default_weekday,
                        default_weekend,
                        default_fees_json,
                        now,
                        tenant_id
                    ],
                )
                .map_err(sqlite_error)?;

            select_config_sqlite(transaction, &tenant_id)?.ok_or_else(|| {
                RepositoryError::ContractViolation("pricing-config-create-missing".into())
            })
        })
    }

    pub(in crate::repositories) fn update_config(
        &self,
        base_weekday: f64,
        base_weekend: f64,
        holiday_rules_json: &str,
        updated_by: &str,
        default_fees_json: &str,
        now: &str,
    ) -> Result<PricingConfigRecord, RepositoryError> {
        let tenant_id = self.tenant_id();
        let holiday_rules_json = holiday_rules_json.to_owned();
        let updated_by = updated_by.to_owned();
        let default_fees_json = default_fees_json.to_owned();
        let now = now.to_owned();

        self.session.write_immediate(move |transaction| {
            ensure_config_sqlite(
                transaction,
                &tenant_id,
                base_weekday,
                base_weekend,
                &default_fees_json,
                &now,
            )?;
            transaction
                .execute(
                    "UPDATE pricing_configs
                     SET baseWeekdayPrice=?1,baseWeekendPrice=?2,holidayRulesJson=?3,
                         updatedBy=?4,updatedAt=?5
                     WHERE tenant_id=?6",
                    params![
                        base_weekday,
                        base_weekend,
                        holiday_rules_json,
                        updated_by,
                        now,
                        tenant_id
                    ],
                )
                .map_err(sqlite_error)?;

            select_config_sqlite(transaction, &tenant_id)?.ok_or_else(|| {
                RepositoryError::ContractViolation("pricing-config-update-missing".into())
            })
        })
    }

    pub(in crate::repositories) fn save_config(
        &self,
        base_weekday: f64,
        base_weekend: f64,
        holiday_rules_json: &str,
        receive_shipping_fees_json: &str,
        dynamic_prices: &HashMap<String, f64>,
        updated_by: &str,
        now: &str,
    ) -> Result<PricingConfigRecord, RepositoryError> {
        let tenant_id = self.tenant_id();
        let holiday_rules_json = holiday_rules_json.to_owned();
        let receive_shipping_fees_json = receive_shipping_fees_json.to_owned();
        let mut dynamic_prices = dynamic_prices
            .iter()
            .map(|(date, price)| (date.clone(), *price))
            .collect::<Vec<_>>();
        dynamic_prices.sort_by(|a, b| a.0.cmp(&b.0));
        let updated_by = updated_by.to_owned();
        let now = now.to_owned();

        self.session.write_immediate(move |transaction| {
            ensure_config_sqlite(
                transaction,
                &tenant_id,
                base_weekday,
                base_weekend,
                &receive_shipping_fees_json,
                &now,
            )?;
            transaction
                .execute(
                    "UPDATE pricing_configs
                     SET baseWeekdayPrice=?1,baseWeekendPrice=?2,holidayRulesJson=?3,
                         receiveShippingFeesJson=?4,updatedBy=?5,updatedAt=?6
                     WHERE tenant_id=?7",
                    params![
                        base_weekday,
                        base_weekend,
                        holiday_rules_json,
                        receive_shipping_fees_json,
                        updated_by,
                        now,
                        tenant_id
                    ],
                )
                .map_err(sqlite_error)?;

            transaction
                .execute(
                    "DELETE FROM dynamic_daily_prices WHERE tenant_id=?1",
                    params![tenant_id],
                )
                .map_err(sqlite_error)?;

            for (date_key, price) in dynamic_prices {
                transaction
                    .execute(
                        "INSERT INTO dynamic_daily_prices
                         (id,dateKey,price,updatedBy,createdAt,updatedAt,tenant_id)
                         VALUES (?1,?2,?3,?4,?5,?5,?6)",
                        params![
                            Uuid::new_v4().to_string(),
                            date_key,
                            price,
                            updated_by,
                            now,
                            tenant_id
                        ],
                    )
                    .map_err(sqlite_error)?;
            }

            select_config_sqlite(transaction, &tenant_id)?.ok_or_else(|| {
                RepositoryError::ContractViolation("pricing-config-save-missing".into())
            })
        })
    }

    pub(in crate::repositories) fn upsert_dynamic_price(
        &self,
        date_key: &str,
        price: f64,
        updated_by: &str,
        now: &str,
    ) -> Result<(), RepositoryError> {
        let tenant_id = self.tenant_id();
        let date_key = date_key.to_owned();
        let updated_by = updated_by.to_owned();
        let now = now.to_owned();

        self.session.write_immediate(move |transaction| {
            transaction
                .execute(
                    "DELETE FROM dynamic_daily_prices
                     WHERE tenant_id=?1 AND dateKey=?2",
                    params![tenant_id, date_key],
                )
                .map_err(sqlite_error)?;
            transaction
                .execute(
                    "INSERT INTO dynamic_daily_prices
                     (id,dateKey,price,updatedBy,createdAt,updatedAt,tenant_id)
                     VALUES (?1,?2,?3,?4,?5,?5,?6)",
                    params![
                        Uuid::new_v4().to_string(),
                        date_key,
                        price,
                        updated_by,
                        now,
                        tenant_id
                    ],
                )
                .map_err(sqlite_error)?;
            Ok(())
        })
    }

    pub(in crate::repositories) fn delete_dynamic_price(
        &self,
        date_key: &str,
    ) -> Result<(), RepositoryError> {
        let tenant_id = self.tenant_id();
        let date_key = date_key.to_owned();
        self.session.write_immediate(move |transaction| {
            transaction
                .execute(
                    "DELETE FROM dynamic_daily_prices
                     WHERE tenant_id=?1 AND dateKey=?2",
                    params![tenant_id, date_key],
                )
                .map_err(sqlite_error)?;
            Ok(())
        })
    }

    pub(in crate::repositories) fn list_dynamic_prices(
        &self,
    ) -> Result<Vec<DynamicPriceRecord>, RepositoryError> {
        let tenant_id = self.tenant_id();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT dateKey,price,updatedBy,createdAt,updatedAt
                 FROM dynamic_daily_prices
                 WHERE tenant_id=?1
                 ORDER BY dateKey ASC",
            )?;
            statement
                .query_map(params![tenant_id], |row| {
                    Ok(DynamicPriceRecord {
                        date_key: row.get(0)?,
                        price: row.get(1)?,
                        updated_by: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                })?
                .collect()
        })
    }

    pub(in crate::repositories) fn get_model_base_price(
        &self,
        model_id: &str,
    ) -> Result<Option<ModelBasePriceRecord>, RepositoryError> {
        let tenant_id = self.tenant_id();
        let model_id = model_id.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT weekdayPrice,weekendPrice
                     FROM model_base_prices
                     WHERE tenant_id=?1 AND modelId=?2
                     LIMIT 1",
                    params![tenant_id, model_id],
                    |row| {
                        Ok(ModelBasePriceRecord {
                            weekday_price: row.get(0)?,
                            weekend_price: row.get(1)?,
                        })
                    },
                )
                .optional()
        })
    }

    fn tenant_id(&self) -> String {
        self.session.binding().tenant_id().as_str().to_owned()
    }
}

fn ensure_config_sqlite(
    connection: &rusqlite::Connection,
    tenant_id: &str,
    default_weekday: f64,
    default_weekend: f64,
    default_fees_json: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    if select_config_sqlite(connection, tenant_id)?.is_some() {
        return Ok(());
    }
    connection
        .execute(
            "INSERT INTO pricing_configs
             (baseWeekdayPrice,baseWeekendPrice,holidayRulesJson,receiveShippingFeesJson,
              updatedBy,createdAt,updatedAt,tenant_id)
             VALUES (?1,?2,'[]',?3,'',?4,?4,?5)",
            params![
                default_weekday,
                default_weekend,
                default_fees_json,
                now,
                tenant_id
            ],
        )
        .map_err(sqlite_error)?;
    Ok(())
}

fn select_config_sqlite(
    connection: &rusqlite::Connection,
    tenant_id: &str,
) -> Result<Option<PricingConfigRecord>, RepositoryError> {
    connection
        .query_row(
            "SELECT baseWeekdayPrice,baseWeekendPrice,holidayRulesJson,
                    receiveShippingFeesJson,updatedBy,createdAt,updatedAt
             FROM pricing_configs
             WHERE tenant_id=?1
             LIMIT 1",
            params![tenant_id],
            |row| {
                Ok(PricingConfigRecord {
                    base_weekday_price: row.get(0)?,
                    base_weekend_price: row.get(1)?,
                    holiday_rules_json: row.get(2)?,
                    receive_shipping_fees_json: row.get(3)?,
                    updated_by: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(sqlite_error)
}

pub(in crate::repositories) fn sqlite_error(error: rusqlite::Error) -> RepositoryError {
    RepositoryError::Sqlite(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use crate::repositories::{RepositoryProvider, SqliteRepositoryProvider};

    fn context(tenant: &str, request: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("pricing-test-actor", "staff").unwrap(),
            TenantScope::tenant(tenant_id.clone()),
            DataScope::production(tenant_id, Revision::new("pricing-test-revision").unwrap())
                .unwrap(),
            ExecutionMode::Normal,
            RequestId::new(request).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn sqlite_pricing_authority_preserves_scope_config_dynamic_and_model_prices() {
        let pool = Pool::builder()
            .max_size(1)
            .build(SqliteConnectionManager::memory())
            .unwrap();
        let connection = pool.get().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE pricing_configs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    baseWeekdayPrice REAL NOT NULL,
                    baseWeekendPrice REAL NOT NULL,
                    holidayRulesJson TEXT NOT NULL DEFAULT '[]',
                    receiveShippingFeesJson TEXT NOT NULL DEFAULT '{}',
                    updatedBy TEXT DEFAULT '',
                    createdAt TEXT NOT NULL,
                    updatedAt TEXT NOT NULL,
                    tenant_id TEXT NOT NULL UNIQUE
                );
                CREATE TABLE dynamic_daily_prices (
                    id TEXT PRIMARY KEY,
                    dateKey TEXT NOT NULL,
                    price REAL NOT NULL,
                    updatedBy TEXT DEFAULT '',
                    createdAt TEXT NOT NULL,
                    updatedAt TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    UNIQUE(tenant_id, dateKey)
                );
                CREATE TABLE model_base_prices (
                    modelId TEXT NOT NULL,
                    weekdayPrice REAL NOT NULL,
                    weekendPrice REAL NOT NULL,
                    updatedBy TEXT DEFAULT '',
                    createdAt TEXT NOT NULL,
                    updatedAt TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    PRIMARY KEY (tenant_id, modelId)
                );
                INSERT INTO model_base_prices VALUES
                    ('shared-model',31,41,'','now','now','tenant-a'),
                    ('shared-model',32,42,'','now','now','tenant-b');",
            )
            .unwrap();
        drop(connection);

        let provider = SqliteRepositoryProvider::new(pool);
        let scoped_a = provider.bind(&context("tenant-a", "pricing-a")).unwrap();
        let scoped_b = provider.bind(&context("tenant-b", "pricing-b")).unwrap();
        let now = "2026-10-01T13:00:00+08:00";

        scoped_a
            .pricing()
            .update_config(11.0, 21.0, "[]", "a", "{}", now)
            .unwrap();
        scoped_b
            .pricing()
            .update_config(12.0, 22.0, "[]", "b", "{}", now)
            .unwrap();
        scoped_a
            .pricing()
            .upsert_dynamic_price("2026-10-05", 111.0, "a", now)
            .unwrap();
        scoped_b
            .pricing()
            .upsert_dynamic_price("2026-10-05", 222.0, "b", now)
            .unwrap();

        let a = scoped_a
            .pricing()
            .get_or_create_config(8.5, 14.0, "{}", now)
            .unwrap();
        let b = scoped_b
            .pricing()
            .get_or_create_config(8.5, 14.0, "{}", now)
            .unwrap();
        assert_eq!(a.base_weekday_price, 11.0);
        assert_eq!(b.base_weekday_price, 12.0);
        assert_eq!(
            scoped_a.pricing().list_dynamic_prices().unwrap()[0].price,
            111.0
        );
        assert_eq!(
            scoped_b.pricing().list_dynamic_prices().unwrap()[0].price,
            222.0
        );

        let replacement = HashMap::from([
            ("2026-10-06".to_owned(), 131.0),
            ("2026-10-07".to_owned(), 141.0),
        ]);
        scoped_a
            .pricing()
            .save_config(13.0, 23.0, "[]", "{}", &replacement, "a2", now)
            .unwrap();

        let a_prices = scoped_a.pricing().list_dynamic_prices().unwrap();
        let b_prices = scoped_b.pricing().list_dynamic_prices().unwrap();
        assert_eq!(a_prices.len(), 2);
        assert_eq!(b_prices.len(), 1);
        assert_eq!(b_prices[0].price, 222.0);

        let model_a = scoped_a
            .pricing()
            .get_model_base_price("shared-model")
            .unwrap()
            .unwrap();
        let model_b = scoped_b
            .pricing()
            .get_model_base_price("shared-model")
            .unwrap()
            .unwrap();
        assert_eq!(model_a.weekday_price, 31.0);
        assert_eq!(model_b.weekday_price, 32.0);
    }
}

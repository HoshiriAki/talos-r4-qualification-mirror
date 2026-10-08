#![cfg(feature = "postgres")]

use std::collections::HashMap;

use sqlx::Row;
use uuid::Uuid;

use crate::repositories::RepositoryError;
use crate::repositories::pricing::PricingConfigRecord;
use crate::repositories::pricing_postgres_common::pg_error;
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn get_or_create_config(
    session: &RepositorySession,
    default_weekday: f64,
    default_weekend: f64,
    default_fees_json: &str,
    now: &str,
) -> Result<PricingConfigRecord, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let default_fees_json = default_fees_json.to_owned();
    let now = now.to_owned();

    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            ensure_config_pg(
                connection,
                &tenant_id,
                default_weekday,
                default_weekend,
                &default_fees_json,
                &now,
            )
            .await?;
            select_config_pg(connection, &tenant_id)
                .await?
                .ok_or_else(|| {
                    RepositoryError::ContractViolation("pricing-config-create-missing".into())
                })
        })
    })
}

pub(in crate::repositories) fn update_config(
    session: &RepositorySession,
    base_weekday: f64,
    base_weekend: f64,
    holiday_rules_json: &str,
    updated_by: &str,
    default_fees_json: &str,
    now: &str,
) -> Result<PricingConfigRecord, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let holiday_rules_json = holiday_rules_json.to_owned();
    let updated_by = updated_by.to_owned();
    let default_fees_json = default_fees_json.to_owned();
    let now = now.to_owned();

    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            ensure_config_pg(
                connection,
                &tenant_id,
                base_weekday,
                base_weekend,
                &default_fees_json,
                &now,
            )
            .await?;

            sqlx::query(
                "UPDATE pricing_configs
                 SET baseweekdayprice=$1,baseweekendprice=$2,
                     holidayrulesjson=CAST($3 AS jsonb),updatedby=$4,updatedat=$5
                 WHERE tenant_id=$6",
            )
            .bind(base_weekday)
            .bind(base_weekend)
            .bind(&holiday_rules_json)
            .bind(&updated_by)
            .bind(&now)
            .bind(&tenant_id)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;

            select_config_pg(connection, &tenant_id)
                .await?
                .ok_or_else(|| {
                    RepositoryError::ContractViolation("pricing-config-update-missing".into())
                })
        })
    })
}

pub(in crate::repositories) fn save_config(
    session: &RepositorySession,
    base_weekday: f64,
    base_weekend: f64,
    holiday_rules_json: &str,
    receive_shipping_fees_json: &str,
    dynamic_prices: &HashMap<String, f64>,
    updated_by: &str,
    now: &str,
) -> Result<PricingConfigRecord, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let holiday_rules_json = holiday_rules_json.to_owned();
    let receive_shipping_fees_json = receive_shipping_fees_json.to_owned();
    let mut dynamic_prices = dynamic_prices
        .iter()
        .map(|(date, price)| (date.clone(), *price))
        .collect::<Vec<_>>();
    dynamic_prices.sort_by(|a, b| a.0.cmp(&b.0));
    let updated_by = updated_by.to_owned();
    let now = now.to_owned();

    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            ensure_config_pg(
                connection,
                &tenant_id,
                base_weekday,
                base_weekend,
                &receive_shipping_fees_json,
                &now,
            )
            .await?;

            sqlx::query(
                "UPDATE pricing_configs
                 SET baseweekdayprice=$1,baseweekendprice=$2,
                     holidayrulesjson=CAST($3 AS jsonb),
                     receiveshippingfeesjson=CAST($4 AS jsonb),
                     updatedby=$5,updatedat=$6
                 WHERE tenant_id=$7",
            )
            .bind(base_weekday)
            .bind(base_weekend)
            .bind(&holiday_rules_json)
            .bind(&receive_shipping_fees_json)
            .bind(&updated_by)
            .bind(&now)
            .bind(&tenant_id)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;

            sqlx::query("DELETE FROM dynamic_daily_prices WHERE tenant_id=$1")
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;

            for (date_key, price) in dynamic_prices {
                sqlx::query(
                    "INSERT INTO dynamic_daily_prices
                     (id,datekey,price,updatedby,createdat,updatedat,tenant_id)
                     VALUES ($1,$2,$3,$4,$5,$5,$6)",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(&date_key)
                .bind(price)
                .bind(&updated_by)
                .bind(&now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await
                .map_err(pg_error)?;
            }

            select_config_pg(connection, &tenant_id)
                .await?
                .ok_or_else(|| {
                    RepositoryError::ContractViolation("pricing-config-save-missing".into())
                })
        })
    })
}

pub(in crate::repositories) fn upsert_dynamic_price(
    session: &RepositorySession,
    date_key: &str,
    price: f64,
    updated_by: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let date_key = date_key.to_owned();
    let updated_by = updated_by.to_owned();
    let now = now.to_owned();

    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "DELETE FROM dynamic_daily_prices
                 WHERE tenant_id=$1 AND datekey=$2",
            )
            .bind(&tenant_id)
            .bind(&date_key)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;

            sqlx::query(
                "INSERT INTO dynamic_daily_prices
                 (id,datekey,price,updatedby,createdat,updatedat,tenant_id)
                 VALUES ($1,$2,$3,$4,$5,$5,$6)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&date_key)
            .bind(price)
            .bind(&updated_by)
            .bind(&now)
            .bind(&tenant_id)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;
            Ok(())
        })
    })
}

pub(in crate::repositories) fn delete_dynamic_price(
    session: &RepositorySession,
    date_key: &str,
) -> Result<(), RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let date_key = date_key.to_owned();

    session.pg_write_serializable_repository(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "DELETE FROM dynamic_daily_prices
                 WHERE tenant_id=$1 AND datekey=$2",
            )
            .bind(&tenant_id)
            .bind(&date_key)
            .execute(&mut *connection)
            .await
            .map_err(pg_error)?;
            Ok(())
        })
    })
}

async fn ensure_config_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
    default_weekday: f64,
    default_weekend: f64,
    default_fees_json: &str,
    now: &str,
) -> Result<(), RepositoryError> {
    if select_config_pg(connection, tenant_id).await?.is_some() {
        return Ok(());
    }

    sqlx::query(
        "INSERT INTO pricing_configs
         (baseweekdayprice,baseweekendprice,holidayrulesjson,receiveshippingfeesjson,
          updatedby,createdat,updatedat,tenant_id)
         VALUES ($1,$2,'[]'::jsonb,CAST($3 AS jsonb),'',$4,$4,$5)
         ON CONFLICT (tenant_id) DO NOTHING",
    )
    .bind(default_weekday)
    .bind(default_weekend)
    .bind(default_fees_json)
    .bind(now)
    .bind(tenant_id)
    .execute(&mut *connection)
    .await
    .map_err(pg_error)?;
    Ok(())
}

async fn select_config_pg(
    connection: &mut sqlx::PgConnection,
    tenant_id: &str,
) -> Result<Option<PricingConfigRecord>, RepositoryError> {
    let row = sqlx::query(
        "SELECT baseweekdayprice,baseweekendprice,
                holidayrulesjson::text AS holiday_rules_json,
                receiveshippingfeesjson::text AS receive_shipping_fees_json,
                COALESCE(updatedby,'') AS updated_by,createdat,updatedat
         FROM pricing_configs
         WHERE tenant_id=$1
         LIMIT 1",
    )
    .bind(tenant_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(pg_error)?;

    row.map(|row| {
        Ok(PricingConfigRecord {
            base_weekday_price: row.try_get("baseweekdayprice").map_err(pg_error)?,
            base_weekend_price: row.try_get("baseweekendprice").map_err(pg_error)?,
            holiday_rules_json: row.try_get("holiday_rules_json").map_err(pg_error)?,
            receive_shipping_fees_json: row
                .try_get("receive_shipping_fees_json")
                .map_err(pg_error)?,
            updated_by: row.try_get("updated_by").map_err(pg_error)?,
            created_at: row.try_get("createdat").map_err(pg_error)?,
            updated_at: row.try_get("updatedat").map_err(pg_error)?,
        })
    })
    .transpose()
}

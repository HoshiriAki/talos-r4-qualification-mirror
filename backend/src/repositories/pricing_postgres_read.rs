#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::pricing::{DynamicPriceRecord, ModelBasePriceRecord};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) fn list_dynamic_prices(
    session: &RepositorySession,
) -> Result<Vec<DynamicPriceRecord>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    session.pg_read(move |connection| {
        Box::pin(async move {
            let rows = sqlx::query(
                "SELECT datekey,price,COALESCE(updatedby,'') AS updated_by,createdat,updatedat
                 FROM dynamic_daily_prices
                 WHERE tenant_id=$1
                 ORDER BY datekey ASC",
            )
            .bind(&tenant_id)
            .fetch_all(&mut *connection)
            .await?;
            rows.iter()
                .map(|row| {
                    Ok(DynamicPriceRecord {
                        date_key: row.try_get("datekey")?,
                        price: row.try_get("price")?,
                        updated_by: row.try_get("updated_by")?,
                        created_at: row.try_get("createdat")?,
                        updated_at: row.try_get("updatedat")?,
                    })
                })
                .collect()
        })
    })
}

pub(in crate::repositories) fn get_model_base_price(
    session: &RepositorySession,
    model_id: &str,
) -> Result<Option<ModelBasePriceRecord>, RepositoryError> {
    let tenant_id = session.binding().tenant_id().as_str().to_owned();
    let model_id = model_id.to_owned();

    session.pg_read(move |connection| {
        Box::pin(async move {
            sqlx::query(
                "SELECT weekdayprice,weekendprice
                 FROM model_base_prices
                 WHERE tenant_id=$1 AND modelid=$2
                 LIMIT 1",
            )
            .bind(&tenant_id)
            .bind(&model_id)
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| {
                Ok(ModelBasePriceRecord {
                    weekday_price: row.try_get("weekdayprice")?,
                    weekend_price: row.try_get("weekendprice")?,
                })
            })
            .transpose()
        })
    })
}

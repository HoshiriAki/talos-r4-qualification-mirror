#![cfg(feature = "postgres")]

use sqlx::Row;

use crate::repositories::RepositoryError;
use crate::repositories::model::{
    ModelMutationError, ModelPatch, ModelPricingPatch, ModelProjection, NewModel,
};
use crate::repositories::session::RepositorySession;

pub(in crate::repositories) struct PostgresModelRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> PostgresModelRepository<'a> {
    pub(in crate::repositories) fn new(session: &'a RepositorySession) -> Self {
        Self { session }
    }

    pub(in crate::repositories) fn list(&self) -> Result<Vec<ModelProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "SELECT dm.id, dm.name, dm.category, dm.prefix, dm.enabled,
                            mbp.weekdayprice, mbp.weekendprice,
                            dm.createdat, dm.updatedat
                     FROM device_models dm
                     LEFT JOIN model_base_prices mbp
                       ON mbp.modelid = dm.id AND mbp.tenant_id = dm.tenant_id
                     WHERE dm.tenant_id = $1
                     ORDER BY dm.createdat ASC",
                )
                .bind(tenant_id)
                .fetch_all(&mut *connection)
                .await?
                .iter()
                .map(map_model)
                .collect()
            })
        })
    }

    pub(in crate::repositories) fn get(
        &self,
        id: &str,
    ) -> Result<Option<ModelProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT dm.id, dm.name, dm.category, dm.prefix, dm.enabled,
                            mbp.weekdayprice, mbp.weekendprice,
                            dm.createdat, dm.updatedat
                     FROM device_models dm
                     LEFT JOIN model_base_prices mbp
                       ON mbp.modelid = dm.id AND mbp.tenant_id = dm.tenant_id
                     WHERE dm.tenant_id = $1 AND dm.id = $2
                     LIMIT 1",
                )
                .bind(tenant_id)
                .bind(id)
                .fetch_optional(&mut *connection)
                .await?;
                row.as_ref().map(map_model).transpose()
            })
        })
    }

    pub(in crate::repositories) fn find_by_name(
        &self,
        name: &str,
    ) -> Result<Option<ModelProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let name = name.to_owned();
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                let row = sqlx::query(
                    "SELECT dm.id, dm.name, dm.category, dm.prefix, dm.enabled,
                            mbp.weekdayprice, mbp.weekendprice,
                            dm.createdat, dm.updatedat
                     FROM device_models dm
                     LEFT JOIN model_base_prices mbp
                       ON mbp.modelid = dm.id AND mbp.tenant_id = dm.tenant_id
                     WHERE dm.tenant_id = $1 AND dm.name = $2
                     LIMIT 1",
                )
                .bind(tenant_id)
                .bind(name)
                .fetch_optional(&mut *connection)
                .await?;
                row.as_ref().map(map_model).transpose()
            })
        })
    }

    pub(in crate::repositories) fn create(
        &self,
        input: &NewModel,
    ) -> Result<ModelProjection, ModelMutationError> {
        validate_optional_pricing(input.weekday_price, input.weekend_price)?;
        if self.name_exists(&input.name, None)? {
            return Err(ModelMutationError::DuplicateName);
        }

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let input = input.clone();
        let reload_id = input.id.clone();
        self.session.pg_write(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "INSERT INTO device_models
                     (id, name, category, prefix, enabled, createdat, updatedat, tenant_id)
                     VALUES ($1,$2,$3,$4,$5,$6,$6,$7)",
                )
                .bind(&input.id)
                .bind(&input.name)
                .bind(&input.category)
                .bind(&input.prefix)
                .bind(input.enabled)
                .bind(&input.now)
                .bind(&tenant_id)
                .execute(&mut *connection)
                .await?;

                if let (Some(weekday_price), Some(weekend_price)) =
                    (input.weekday_price, input.weekend_price)
                {
                    sqlx::query(
                        "INSERT INTO model_base_prices
                         (modelid, weekdayprice, weekendprice, updatedby, createdat, updatedat, tenant_id)
                         VALUES ($1,$2,$3,$4,$5,$5,$6)",
                    )
                    .bind(&input.id)
                    .bind(weekday_price)
                    .bind(weekend_price)
                    .bind(input.updated_by.unwrap_or_default())
                    .bind(&input.now)
                    .bind(&tenant_id)
                    .execute(&mut *connection)
                    .await?;
                }
                Ok(())
            })
        })?;

        self.get(&reload_id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("created model could not be reloaded".into()).into()
        })
    }

    pub(in crate::repositories) fn update(
        &self,
        id: &str,
        patch: &ModelPatch,
    ) -> Result<ModelProjection, ModelMutationError> {
        if self.get(id)?.is_none() {
            return Err(ModelMutationError::NotFound);
        }
        if let Some(name) = patch.name.as_deref()
            && self.name_exists(name, Some(id))?
        {
            return Err(ModelMutationError::DuplicateName);
        }
        validate_pricing_patch(&patch.pricing)?;

        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id_owned = id.to_owned();
        let patch = patch.clone();
        let has_model_fields = patch.name.is_some()
            || patch.category.is_some()
            || patch.prefix.is_some()
            || patch.enabled.is_some();

        self.session.pg_write(move |connection| {
            Box::pin(async move {
                if has_model_fields {
                    sqlx::query(
                        "UPDATE device_models
                         SET name = COALESCE($1, name),
                             category = COALESCE($2, category),
                             prefix = COALESCE($3, prefix),
                             enabled = COALESCE($4, enabled),
                             updatedat = $5
                         WHERE tenant_id = $6 AND id = $7",
                    )
                    .bind(patch.name.as_deref())
                    .bind(patch.category.as_deref())
                    .bind(patch.prefix.as_deref())
                    .bind(patch.enabled)
                    .bind(&patch.now)
                    .bind(&tenant_id)
                    .bind(&id_owned)
                    .execute(&mut *connection)
                    .await?;
                }

                match patch.pricing {
                    ModelPricingPatch::Unchanged => {}
                    ModelPricingPatch::Clear => {
                        sqlx::query(
                            "DELETE FROM model_base_prices
                             WHERE tenant_id = $1 AND modelid = $2",
                        )
                        .bind(&tenant_id)
                        .bind(&id_owned)
                        .execute(&mut *connection)
                        .await?;
                    }
                    ModelPricingPatch::Replace {
                        weekday_price,
                        weekend_price,
                        updated_by,
                    } => {
                        sqlx::query(
                            "INSERT INTO model_base_prices
                             (modelid, weekdayprice, weekendprice, updatedby, createdat, updatedat, tenant_id)
                             VALUES ($1,$2,$3,COALESCE($4,''),$5,$5,$6)
                             ON CONFLICT (tenant_id, modelid) DO UPDATE
                             SET weekdayprice = EXCLUDED.weekdayprice,
                                 weekendprice = EXCLUDED.weekendprice,
                                 updatedby = COALESCE($4, model_base_prices.updatedby),
                                 updatedat = EXCLUDED.updatedat",
                        )
                        .bind(&id_owned)
                        .bind(weekday_price)
                        .bind(weekend_price)
                        .bind(updated_by.as_deref())
                        .bind(&patch.now)
                        .bind(&tenant_id)
                        .execute(&mut *connection)
                        .await?;
                    }
                }
                Ok(())
            })
        })?;

        self.get(id)?.ok_or_else(|| {
            RepositoryError::ContractViolation("updated model could not be reloaded".into()).into()
        })
    }

    pub(in crate::repositories) fn delete(&self, id: &str) -> Result<(), ModelMutationError> {
        if self.get(id)?.is_none() {
            return Err(ModelMutationError::NotFound);
        }
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id_owned = id.to_owned();
        let device_count: i64 = self.session.pg_read({
            let tenant_id = tenant_id.clone();
            let id_owned = id_owned.clone();
            move |connection| {
                Box::pin(async move {
                    sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*)::bigint FROM devices
                         WHERE tenant_id = $1 AND modelid = $2",
                    )
                    .bind(tenant_id)
                    .bind(id_owned)
                    .fetch_one(&mut *connection)
                    .await
                })
            }
        })?;
        if device_count > 0 {
            return Err(ModelMutationError::Referenced(
                u64::try_from(device_count).unwrap_or(u64::MAX),
            ));
        }

        self.session.pg_write(move |connection| {
            Box::pin(async move {
                sqlx::query(
                    "DELETE FROM device_models
                     WHERE tenant_id = $1 AND id = $2",
                )
                .bind(tenant_id)
                .bind(id_owned)
                .execute(&mut *connection)
                .await?;
                Ok(())
            })
        })?;
        Ok(())
    }

    fn name_exists(&self, name: &str, exclude_id: Option<&str>) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let name = name.to_owned();
        let exclude_id = exclude_id.map(str::to_owned);
        self.session.pg_read(move |connection| {
            Box::pin(async move {
                match exclude_id {
                    Some(exclude_id) => {
                        sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(
                                 SELECT 1 FROM device_models
                                 WHERE tenant_id = $1 AND name = $2 AND id <> $3
                             )",
                        )
                        .bind(tenant_id)
                        .bind(name)
                        .bind(exclude_id)
                        .fetch_one(&mut *connection)
                        .await
                    }
                    None => {
                        sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(
                                 SELECT 1 FROM device_models
                                 WHERE tenant_id = $1 AND name = $2
                             )",
                        )
                        .bind(tenant_id)
                        .bind(name)
                        .fetch_one(&mut *connection)
                        .await
                    }
                }
            })
        })
    }
}

fn validate_optional_pricing(
    weekday_price: Option<f64>,
    weekend_price: Option<f64>,
) -> Result<(), ModelMutationError> {
    for value in [weekday_price, weekend_price].into_iter().flatten() {
        if !value.is_finite() || value <= 0.0 {
            return Err(ModelMutationError::InvalidPricing);
        }
    }
    Ok(())
}

fn validate_pricing_patch(pricing: &ModelPricingPatch) -> Result<(), ModelMutationError> {
    if let ModelPricingPatch::Replace {
        weekday_price,
        weekend_price,
        ..
    } = pricing
        && (!weekday_price.is_finite()
            || *weekday_price <= 0.0
            || !weekend_price.is_finite()
            || *weekend_price <= 0.0)
    {
        return Err(ModelMutationError::InvalidPricing);
    }
    Ok(())
}

fn map_model(row: &sqlx::postgres::PgRow) -> Result<ModelProjection, sqlx::Error> {
    Ok(ModelProjection {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        category: row.try_get("category")?,
        prefix: row.try_get("prefix")?,
        enabled: row.try_get("enabled")?,
        weekday_price: row.try_get("weekdayprice")?,
        weekend_price: row.try_get("weekendprice")?,
        created_at: row.try_get("createdat")?,
        updated_at: row.try_get("updatedat")?,
    })
}

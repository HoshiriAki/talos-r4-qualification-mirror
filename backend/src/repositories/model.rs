use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::repositories::session::RepositorySession;
use crate::repositories::{RepositoryError, ScopedRepositories};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProjection {
    pub id: String,
    pub name: String,
    pub category: String,
    pub prefix: String,
    pub enabled: bool,
    pub weekday_price: Option<f64>,
    pub weekend_price: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewModel {
    pub id: String,
    pub name: String,
    pub category: String,
    pub prefix: String,
    pub enabled: bool,
    pub weekday_price: Option<f64>,
    pub weekend_price: Option<f64>,
    pub updated_by: Option<String>,
    pub now: String,
}

#[derive(Debug, Clone, Default)]
pub enum ModelPricingPatch {
    #[default]
    Unchanged,
    Replace {
        weekday_price: f64,
        weekend_price: f64,
        updated_by: Option<String>,
    },
    Clear,
}

#[derive(Debug, Clone, Default)]
pub struct ModelPatch {
    pub name: Option<String>,
    pub category: Option<String>,
    pub prefix: Option<String>,
    pub enabled: Option<bool>,
    pub pricing: ModelPricingPatch,
    pub now: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ModelMutationError {
    #[error("model not found")]
    NotFound,
    #[error("model name already exists")]
    DuplicateName,
    #[error("model is referenced by {0} devices")]
    Referenced(u64),
    #[error("model pricing must contain finite positive weekday and weekend prices")]
    InvalidPricing,
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

pub(in crate::repositories) struct SqliteModelRepository<'a> {
    session: &'a RepositorySession,
}

impl<'a> SqliteModelRepository<'a> {
    pub(in crate::repositories) fn new(scoped: &'a ScopedRepositories) -> Self {
        Self {
            session: scoped.session(),
        }
    }

    pub(in crate::repositories) fn list(&self) -> Result<Vec<ModelProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        self.session.read(move |connection| {
            let mut statement = connection.prepare(
                "SELECT dm.id, dm.name, dm.category, dm.prefix, dm.enabled,
                        mbp.weekdayPrice, mbp.weekendPrice, dm.createdAt, dm.updatedAt
                 FROM device_models dm
                 LEFT JOIN model_base_prices mbp
                   ON mbp.modelId = dm.id AND mbp.tenant_id = dm.tenant_id
                 WHERE dm.tenant_id = ?1
                 ORDER BY dm.createdAt ASC",
            )?;
            statement
                .query_map([tenant_id], map_model)?
                .collect::<Result<Vec<_>, _>>()
        })
    }

    pub(in crate::repositories) fn get(
        &self,
        id: &str,
    ) -> Result<Option<ModelProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let id = id.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT dm.id, dm.name, dm.category, dm.prefix, dm.enabled,
                            mbp.weekdayPrice, mbp.weekendPrice, dm.createdAt, dm.updatedAt
                     FROM device_models dm
                     LEFT JOIN model_base_prices mbp
                       ON mbp.modelId = dm.id AND mbp.tenant_id = dm.tenant_id
                     WHERE dm.tenant_id = ?1 AND dm.id = ?2
                     LIMIT 1",
                    params![tenant_id, id],
                    map_model,
                )
                .optional()
        })
    }

    pub(in crate::repositories) fn find_by_name(
        &self,
        name: &str,
    ) -> Result<Option<ModelProjection>, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let name = name.to_owned();
        self.session.read(move |connection| {
            connection
                .query_row(
                    "SELECT dm.id, dm.name, dm.category, dm.prefix, dm.enabled,
                            mbp.weekdayPrice, mbp.weekendPrice, dm.createdAt, dm.updatedAt
                     FROM device_models dm
                     LEFT JOIN model_base_prices mbp
                       ON mbp.modelId = dm.id AND mbp.tenant_id = dm.tenant_id
                     WHERE dm.tenant_id = ?1 AND dm.name = ?2
                     LIMIT 1",
                    params![tenant_id, name],
                    map_model,
                )
                .optional()
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
        self.session.write(move |tx| {
            tx.execute(
                "INSERT INTO device_models
                 (id, name, category, prefix, enabled, createdAt, updatedAt, tenant_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)",
                params![
                    input.id,
                    input.name,
                    input.category,
                    input.prefix,
                    if input.enabled { 1 } else { 0 },
                    input.now,
                    tenant_id,
                ],
            )?;

            if let (Some(weekday_price), Some(weekend_price)) =
                (input.weekday_price, input.weekend_price)
            {
                tx.execute(
                    "INSERT INTO model_base_prices
                     (modelId, weekdayPrice, weekendPrice, updatedBy, createdAt, updatedAt, tenant_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
                    params![
                        input.id,
                        weekday_price,
                        weekend_price,
                        input.updated_by.unwrap_or_default(),
                        input.now,
                        tenant_id,
                    ],
                )?;
            }
            Ok(())
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
        self.session.write(move |tx| {
            let mut fields = Vec::new();
            let mut values = Vec::<SqlValue>::new();
            if let Some(value) = patch.name {
                fields.push("name = ?".to_owned());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = patch.category {
                fields.push("category = ?".to_owned());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = patch.prefix {
                fields.push("prefix = ?".to_owned());
                values.push(SqlValue::Text(value));
            }
            if let Some(value) = patch.enabled {
                fields.push("enabled = ?".to_owned());
                values.push(SqlValue::Integer(if value { 1 } else { 0 }));
            }
            if !fields.is_empty() {
                fields.push("updatedAt = ?".to_owned());
                values.push(SqlValue::Text(patch.now.clone()));
                values.push(SqlValue::Text(tenant_id.clone()));
                values.push(SqlValue::Text(id_owned.clone()));
                tx.execute(
                    &format!(
                        "UPDATE device_models SET {} WHERE tenant_id = ? AND id = ?",
                        fields.join(", ")
                    ),
                    params_from_iter(values.iter()),
                )?;
            }

            match patch.pricing {
                ModelPricingPatch::Unchanged => {}
                ModelPricingPatch::Clear => {
                    tx.execute(
                        "DELETE FROM model_base_prices WHERE tenant_id = ?1 AND modelId = ?2",
                        params![tenant_id, id_owned],
                    )?;
                }
                ModelPricingPatch::Replace {
                    weekday_price,
                    weekend_price,
                    updated_by,
                } => {
                    let exists: bool = tx.query_row(
                        "SELECT EXISTS(
                             SELECT 1 FROM model_base_prices
                             WHERE tenant_id = ?1 AND modelId = ?2
                         )",
                        params![tenant_id, id_owned],
                        |row| row.get(0),
                    )?;
                    if exists {
                        if let Some(updated_by) = updated_by {
                            tx.execute(
                                "UPDATE model_base_prices
                                 SET weekdayPrice = ?1, weekendPrice = ?2,
                                     updatedBy = ?3, updatedAt = ?4
                                 WHERE tenant_id = ?5 AND modelId = ?6",
                                params![
                                    weekday_price,
                                    weekend_price,
                                    updated_by,
                                    patch.now,
                                    tenant_id,
                                    id_owned,
                                ],
                            )?;
                        } else {
                            tx.execute(
                                "UPDATE model_base_prices
                                 SET weekdayPrice = ?1, weekendPrice = ?2, updatedAt = ?3
                                 WHERE tenant_id = ?4 AND modelId = ?5",
                                params![
                                    weekday_price,
                                    weekend_price,
                                    patch.now,
                                    tenant_id,
                                    id_owned,
                                ],
                            )?;
                        }
                    } else {
                        tx.execute(
                            "INSERT INTO model_base_prices
                             (modelId, weekdayPrice, weekendPrice, updatedBy, createdAt, updatedAt, tenant_id)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
                            params![
                                id_owned,
                                weekday_price,
                                weekend_price,
                                updated_by.unwrap_or_default(),
                                patch.now,
                                tenant_id,
                            ],
                        )?;
                    }
                }
            }
            Ok(())
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
        let device_count: i64 = self.session.read({
            let tenant_id = tenant_id.clone();
            let id_owned = id_owned.clone();
            move |connection| {
                connection.query_row(
                    "SELECT COUNT(*) FROM devices WHERE tenant_id = ?1 AND modelId = ?2",
                    params![tenant_id, id_owned],
                    |row| row.get(0),
                )
            }
        })?;
        if device_count > 0 {
            return Err(ModelMutationError::Referenced(
                u64::try_from(device_count).unwrap_or(u64::MAX),
            ));
        }

        self.session.write(move |tx| {
            tx.execute(
                "DELETE FROM device_models WHERE tenant_id = ?1 AND id = ?2",
                params![tenant_id, id_owned],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    fn name_exists(&self, name: &str, exclude_id: Option<&str>) -> Result<bool, RepositoryError> {
        let tenant_id = self.session.binding().tenant_id().as_str().to_owned();
        let name = name.to_owned();
        let exclude_id = exclude_id.map(str::to_owned);
        self.session.read(move |connection| match exclude_id {
            Some(exclude_id) => connection.query_row(
                "SELECT EXISTS(
                         SELECT 1 FROM device_models
                         WHERE tenant_id = ?1 AND name = ?2 AND id <> ?3
                     )",
                params![tenant_id, name, exclude_id],
                |row| row.get(0),
            ),
            None => connection.query_row(
                "SELECT EXISTS(
                         SELECT 1 FROM device_models
                         WHERE tenant_id = ?1 AND name = ?2
                     )",
                params![tenant_id, name],
                |row| row.get(0),
            ),
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

fn map_model(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelProjection> {
    Ok(ModelProjection {
        id: row.get(0)?,
        name: row.get(1)?,
        category: row.get(2)?,
        prefix: row.get(3)?,
        enabled: row.get::<_, i64>(4)? != 0,
        weekday_price: row.get(5)?,
        weekend_price: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

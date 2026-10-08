use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use system_core::ExecutionContext;

use crate::repositories::{
    ModelMutationError, ModelPatch, ModelPricingPatch, ModelProjection, RepositoryError,
    RepositoryProvider,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelAuthorityView {
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

impl From<ModelProjection> for ModelAuthorityView {
    fn from(value: ModelProjection) -> Self {
        Self {
            id: value.id,
            name: value.name,
            category: value.category,
            prefix: value.prefix,
            enabled: value.enabled,
            weekday_price: value.weekday_price,
            weekend_price: value.weekend_price,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug)]
pub enum ModelAuthorityError {
    InvalidInput(String),
    NotFound,
    DuplicateName,
    Referenced(u64),
    Persistence { code: &'static str },
}

impl From<RepositoryError> for ModelAuthorityError {
    fn from(value: RepositoryError) -> Self {
        Self::Persistence { code: value.code() }
    }
}

impl From<ModelMutationError> for ModelAuthorityError {
    fn from(value: ModelMutationError) -> Self {
        match value {
            ModelMutationError::NotFound => Self::NotFound,
            ModelMutationError::DuplicateName => Self::DuplicateName,
            ModelMutationError::Referenced(count) => Self::Referenced(count),
            ModelMutationError::InvalidPricing => {
                Self::InvalidInput("weekdayPrice 与 weekendPrice 必须大于 0".into())
            }
            ModelMutationError::Storage(error) => error.into(),
        }
    }
}

#[derive(Clone)]
pub struct ModelAuthorityService {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl ModelAuthorityService {
    pub fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    pub fn update_full(
        &self,
        ctx: &ExecutionContext,
        id: &str,
        body: &Value,
        updated_by: &str,
    ) -> Result<ModelAuthorityView, ModelAuthorityError> {
        let object = body
            .as_object()
            .ok_or_else(|| ModelAuthorityError::InvalidInput("型号更新必须是 JSON 对象".into()))?;
        let name = optional_trimmed_string(object.get("name"), "型号名称不能为空")?;
        let category = optional_string(object.get("category"))?;
        let prefix = optional_string(object.get("prefix"))?;
        let enabled = optional_bool(object.get("enabled"), "enabled 必须是布尔值")?;
        let pricing = pricing_patch(body, Some(updated_by.to_owned()), true)?;

        let scoped = self.repository_provider.bind(ctx)?;
        scoped
            .models()
            .update(
                id,
                &ModelPatch {
                    name,
                    category,
                    prefix,
                    enabled,
                    pricing,
                    now: shanghai_now_iso(),
                },
            )
            .map(ModelAuthorityView::from)
            .map_err(ModelAuthorityError::from)
    }

    pub fn update_pricing(
        &self,
        ctx: &ExecutionContext,
        id: &str,
        body: &Value,
        updated_by: &str,
    ) -> Result<ModelAuthorityView, ModelAuthorityError> {
        if !body.is_object() {
            return Err(ModelAuthorityError::InvalidInput(
                "型号定价更新必须是 JSON 对象".into(),
            ));
        }
        let scoped = self.repository_provider.bind(ctx)?;
        scoped
            .models()
            .update(
                id,
                &ModelPatch {
                    pricing: pricing_patch(body, Some(updated_by.to_owned()), false)?,
                    now: shanghai_now_iso(),
                    ..ModelPatch::default()
                },
            )
            .map(ModelAuthorityView::from)
            .map_err(ModelAuthorityError::from)
    }
}

fn optional_trimmed_string(
    value: Option<&Value>,
    empty_message: &str,
) -> Result<Option<String>, ModelAuthorityError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let raw = value
        .as_str()
        .ok_or_else(|| ModelAuthorityError::InvalidInput(empty_message.into()))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ModelAuthorityError::InvalidInput(empty_message.into()));
    }
    Ok(Some(trimmed.to_owned()))
}

fn optional_string(value: Option<&Value>) -> Result<Option<String>, ModelAuthorityError> {
    let Some(value) = value else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|value| Some(value.to_owned()))
        .ok_or_else(|| ModelAuthorityError::InvalidInput("型号字段必须是字符串".into()))
}

fn optional_bool(
    value: Option<&Value>,
    message: &str,
) -> Result<Option<bool>, ModelAuthorityError> {
    let Some(value) = value else {
        return Ok(None);
    };
    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| ModelAuthorityError::InvalidInput(message.into()))
}

fn pricing_patch(
    body: &Value,
    updated_by: Option<String>,
    allow_global_price: bool,
) -> Result<ModelPricingPatch, ModelAuthorityError> {
    if allow_global_price && body.get("useGlobalPrice").and_then(Value::as_bool) == Some(true) {
        return Ok(ModelPricingPatch::Clear);
    }

    let has_pricing = body.get("weekdayPrice").is_some() || body.get("weekendPrice").is_some();
    if !has_pricing {
        return if allow_global_price {
            Ok(ModelPricingPatch::Unchanged)
        } else {
            Err(ModelAuthorityError::InvalidInput(
                "weekdayPrice 与 weekendPrice 必须同时提供".into(),
            ))
        };
    }

    let weekday_price = body
        .get("weekdayPrice")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| ModelAuthorityError::InvalidInput("weekdayPrice 必须大于 0".into()))?;
    let weekend_price = body
        .get("weekendPrice")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| ModelAuthorityError::InvalidInput("weekendPrice 必须大于 0".into()))?;

    Ok(ModelPricingPatch::Replace {
        weekday_price,
        weekend_price,
        updated_by,
    })
}

#[cfg(test)]
mod tests {
    use super::{ModelAuthorityError, optional_trimmed_string, pricing_patch};
    use crate::repositories::ModelPricingPatch;

    #[test]
    fn full_update_rejects_blank_model_name() {
        let value = serde_json::json!("   ");
        assert!(matches!(
            optional_trimmed_string(Some(&value), "required"),
            Err(ModelAuthorityError::InvalidInput(_))
        ));
    }

    #[test]
    fn pricing_patch_preserves_global_and_pair_semantics() {
        assert!(matches!(
            pricing_patch(
                &serde_json::json!({"useGlobalPrice": true}),
                Some("admin".into()),
                true,
            ),
            Ok(ModelPricingPatch::Clear)
        ));
        assert!(matches!(
            pricing_patch(
                &serde_json::json!({"weekdayPrice": 10.0}),
                Some("admin".into()),
                false,
            ),
            Err(ModelAuthorityError::InvalidInput(_))
        ));
    }
}

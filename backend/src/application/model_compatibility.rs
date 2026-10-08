use std::sync::Arc;

use official_device::model::{
    CreateModelInput, DeleteModelInput, DeleteOutput, FeatureModel, GetModelInput, ListModelsInput,
    UpdateModelInput,
};
use serde_json::Value;
use system_core::{
    DeserializeGuard, ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, Sanitize,
    SystemModule, Unvalidated, Validate,
};
use uuid::Uuid;

use crate::repositories::{
    ModelMutationError, ModelPatch, ModelPricingPatch, NewModel, RepositoryError,
    RepositoryProvider,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct ModelCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl ModelCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    fn parse<T>(payload: Value) -> Result<T, String>
    where
        T: serde::de::DeserializeOwned + Sanitize + Validate,
    {
        DeserializeGuard::default().check_raw(&payload)?;
        let unvalidated: Unvalidated<T> = payload.try_into()?;
        Ok(unvalidated.sanitize().validate()?.into_inner())
    }

    fn scoped(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<crate::repositories::ScopedRepositories, String> {
        self.repository_provider
            .bind(ctx)
            .map_err(|error| Self::repository_error("SYS_DB_QUERY", error))
    }

    fn repository_error(code: &str, error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: format!("{code}: model persistence unavailable"),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: ModelMutationError) -> String {
        let payload = match error {
            ModelMutationError::NotFound => ErrorPayload {
                category: "val".into(),
                code: "VAL_NOT_FOUND".into(),
                message: "型号不存在".into(),
                field: Some("id".into()),
                context: None,
            },
            ModelMutationError::DuplicateName => ErrorPayload {
                category: "val".into(),
                code: "VAL_DUPLICATE".into(),
                message: "型号名称已存在".into(),
                field: Some("name".into()),
                context: None,
            },
            ModelMutationError::Referenced(count) => ErrorPayload {
                category: "val".into(),
                code: "VAL_REFERENCED".into(),
                message: format!("该型号已被 {count} 台设备使用，无法删除"),
                field: Some("id".into()),
                context: None,
            },
            ModelMutationError::InvalidPricing => ErrorPayload {
                category: "val".into(),
                code: "VAL_INVALID".into(),
                message: "weekdayPrice 与 weekendPrice 必须为大于 0 的有限数值".into(),
                field: None,
                context: None,
            },
            ModelMutationError::Storage(error) => ErrorPayload {
                category: "sys".into(),
                code: error.code().into(),
                message: "model persistence unavailable".into(),
                field: None,
                context: None,
            },
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn list(&self, _input: ListModelsInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let models = scoped
            .models()
            .list()
            .map_err(|error| Self::repository_error("list", error))?;
        serde_json::to_value(models).map_err(|error| {
            Self::repository_error(
                "serialize",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn get(&self, input: GetModelInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let model = scoped
            .models()
            .get(&input.id)
            .map_err(|error| Self::repository_error("get", error))?
            .ok_or_else(|| Self::mutation_error(ModelMutationError::NotFound))?;
        serde_json::to_value(model).map_err(|error| {
            Self::repository_error(
                "serialize",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn create(&self, input: CreateModelInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let model = scoped
            .models()
            .create(&NewModel {
                id: Uuid::new_v4().to_string(),
                name: input.name,
                category: input.category,
                prefix: input.prefix,
                enabled: input.enabled,
                weekday_price: input.weekday_price,
                weekend_price: input.weekend_price,
                updated_by: None,
                now: shanghai_now_iso(),
            })
            .map_err(Self::mutation_error)?;
        serde_json::to_value(model).map_err(|error| {
            Self::repository_error(
                "serialize",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn update(&self, input: UpdateModelInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let pricing = match (input.weekday_price, input.weekend_price) {
            (Some(weekday_price), Some(weekend_price)) => ModelPricingPatch::Replace {
                weekday_price,
                weekend_price,
                updated_by: None,
            },
            _ => ModelPricingPatch::Unchanged,
        };
        let model = scoped
            .models()
            .update(
                &input.id,
                &ModelPatch {
                    name: input.name,
                    category: input.category,
                    prefix: input.prefix,
                    enabled: input.enabled,
                    pricing,
                    now: shanghai_now_iso(),
                },
            )
            .map_err(Self::mutation_error)?;
        serde_json::to_value(model).map_err(|error| {
            Self::repository_error(
                "serialize",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }

    fn delete(&self, input: DeleteModelInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        scoped
            .models()
            .delete(&input.id)
            .map_err(Self::mutation_error)?;
        serde_json::to_value(DeleteOutput { success: true }).map_err(|error| {
            Self::repository_error(
                "serialize",
                RepositoryError::ContractViolation(error.to_string()),
            )
        })
    }
}

impl SystemModule for ModelCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureModel::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureModel::new().commands()
    }

    fn init(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "list_models" => self.list(Self::parse(payload)?, ctx),
            "get_model" => self.get(Self::parse(payload)?, ctx),
            "create_model" => self.create(Self::parse(payload)?, ctx),
            "update_model" => self.update(Self::parse(payload)?, ctx),
            "delete_model" => self.delete(Self::parse(payload)?, ctx),
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("unknown command: {command}"),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureModel::new().schema()
    }
}

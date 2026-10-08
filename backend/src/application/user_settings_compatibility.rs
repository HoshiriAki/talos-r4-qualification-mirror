use serde::Deserialize;
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::repositories::{RepositoryError, UserSettingsProfileRepository};
use crate::utils::time::shanghai_now_iso;

const MODULE_NAME: &str = "user_settings";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetSettingsInput {
    user_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveSettingsInput {
    user_id: String,
    settings: Value,
}

#[derive(Debug)]
enum UserSettingsError {
    InvalidInput(String),
    SelfOnly,
    Repository(RepositoryError),
    Serialize(String),
}

impl UserSettingsError {
    fn payload(&self) -> ErrorPayload {
        match self {
            Self::InvalidInput(message) => ErrorPayload {
                category: "val".into(),
                code: "VAL_USER_SETTINGS_INPUT".into(),
                message: message.clone(),
                field: None,
                context: None,
            },
            Self::SelfOnly => ErrorPayload {
                category: "auth".into(),
                code: "AUTH_USER_SETTINGS_SELF_ONLY".into(),
                message: "只能访问当前登录身份的用户设置".into(),
                field: Some("userId".into()),
                context: None,
            },
            Self::Repository(error) => ErrorPayload {
                category: "sys".into(),
                code: error.code().into(),
                message: "user settings persistence unavailable".into(),
                field: None,
                context: None,
            },
            Self::Serialize(message) => ErrorPayload {
                category: "sys".into(),
                code: "SYS_SERIALIZE".into(),
                message: message.clone(),
                field: None,
                context: None,
            },
        }
    }

    fn into_json(self) -> String {
        serde_json::to_string(&self.payload()).unwrap_or_else(|_| {
            r#"{"category":"sys","code":"SYS_SERIALIZE","message":"failed to serialize user settings error","field":null,"context":null}"#.into()
        })
    }
}

impl From<RepositoryError> for UserSettingsError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

#[derive(Clone)]
pub(crate) struct UserSettingsCompatibilityModule {
    repository: UserSettingsProfileRepository,
}

impl UserSettingsCompatibilityModule {
    pub(crate) fn new(repository: UserSettingsProfileRepository) -> Self {
        Self { repository }
    }

    fn parse<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload)
            .map_err(|error| UserSettingsError::InvalidInput(error.to_string()).into_json())
    }

    fn ensure_self<'a>(
        ctx: &'a ExecutionContext,
        requested_user_id: &'a str,
    ) -> Result<&'a str, String> {
        let requested = requested_user_id.trim();
        if requested.is_empty() {
            return Err(UserSettingsError::InvalidInput("用户ID不能为空".into()).into_json());
        }
        let actor = ctx
            .user_id()
            .ok_or_else(|| UserSettingsError::SelfOnly.into_json())?;
        if actor != requested {
            return Err(UserSettingsError::SelfOnly.into_json());
        }
        Ok(actor)
    }

    fn get_settings(
        &self,
        ctx: &ExecutionContext,
        input: GetSettingsInput,
    ) -> Result<Value, String> {
        let user_id = Self::ensure_self(ctx, &input.user_id)?;
        self.repository
            .get(user_id)
            .map_err(|error| UserSettingsError::from(error).into_json())
    }

    fn save_settings(
        &self,
        ctx: &ExecutionContext,
        input: SaveSettingsInput,
    ) -> Result<Value, String> {
        let user_id = Self::ensure_self(ctx, &input.user_id)?;
        if !input.settings.is_object() {
            return Err(
                UserSettingsError::InvalidInput("用户设置必须是 JSON 对象".into()).into_json(),
            );
        }
        self.repository
            .save(user_id, &input.settings, &shanghai_now_iso())
            .map_err(|error| UserSettingsError::from(error).into_json())?;
        Ok(serde_json::json!({ "success": true }))
    }
}

impl SystemModule for UserSettingsCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "0.1.0".into(),
            description: "Identity-global user UI profile compatibility".into(),
            author: "TALOS".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
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
            "get_settings" => self.get_settings(ctx, Self::parse(payload)?),
            "save_settings" => self.save_settings(ctx, Self::parse(payload)?),
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

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "get_settings",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "save_settings",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: MODULE_NAME.into(),
            description: "Identity-global user UI profile compatibility".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|command| CommandSchema {
                    name: command.name.into(),
                    description: "User settings compatibility command".into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use system_core::{
        ActorIdentity, DataScope, ExecutionContext, ExecutionMode, NoopHttpClient, RequestId,
        Revision, TenantId, TenantScope,
    };

    use crate::repositories::UserSettingsProfileRepository;

    use super::UserSettingsCompatibilityModule;

    fn context(user_id: &str) -> ExecutionContext {
        let tenant = TenantId::new("tenant-a").unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated(user_id, "staff").unwrap(),
            TenantScope::tenant(tenant.clone()),
            DataScope::production(tenant, Revision::new("test").unwrap()).unwrap(),
            ExecutionMode::Normal,
            RequestId::new("request-a").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn identity_profile_rejects_cross_identity_access_before_storage() {
        let pool = Pool::new(SqliteConnectionManager::memory()).unwrap();
        let module = UserSettingsCompatibilityModule::new(UserSettingsProfileRepository::new(pool));
        let error = module
            .get_settings(
                &context("identity-a"),
                super::GetSettingsInput {
                    user_id: "identity-b".into(),
                },
            )
            .unwrap_err();
        assert!(error.contains("AUTH_USER_SETTINGS_SELF_ONLY"));
    }
}

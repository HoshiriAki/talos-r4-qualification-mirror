use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use system_core::{
    AccessRequirement, CommandMetadata, CommandSchema, EffectClass, ErrorPayload, ExecutionContext,
    ModuleMetadata, ModuleSchema, SimulationSupport, SystemModule,
};

use crate::repositories::{
    RepositoryError, RepositoryProvider, WorkTaskListRequest, WorkTaskMutation, WorkTaskState,
};
use crate::utils::time::shanghai_now_iso;

const MODULE_NAME: &str = "work_task";
const ALLOWED_STATUSES: &[&str] = &["queued", "in_progress", "completed", "cancelled"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListTasksInput {
    status: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskActionInput {
    id: String,
    action: String,
    #[serde(default)]
    expected_version: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendToPcInput {
    id: String,
    expected_version: i64,
}

#[derive(Debug)]
enum WorkTaskError {
    InvalidInput(String),
    NotFound,
    Conflict,
    UnsupportedAction(String),
    Terminal,
    Forbidden,
    Repository(RepositoryError),
    Serialize(String),
}

impl WorkTaskError {
    fn payload(&self) -> ErrorPayload {
        match self {
            Self::InvalidInput(message) => ErrorPayload {
                category: "val".into(),
                code: "VAL_WORK_TASK_INPUT".into(),
                message: message.clone(),
                field: None,
                context: None,
            },
            Self::NotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_WORK_TASK_NOT_FOUND".into(),
                message: "任务不存在".into(),
                field: Some("id".into()),
                context: None,
            },
            Self::Conflict => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_WORK_TASK_CONFLICT".into(),
                message: "任务已在其他终端更新，请刷新重试".into(),
                field: Some("expectedVersion".into()),
                context: None,
            },
            Self::UnsupportedAction(action) => ErrorPayload {
                category: "val".into(),
                code: "VAL_WORK_TASK_ACTION".into(),
                message: format!("任务不支持操作: {action}"),
                field: Some("action".into()),
                context: None,
            },
            Self::Terminal => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_WORK_TASK_TERMINAL".into(),
                message: "此任务已结束，无法执行操作".into(),
                field: None,
                context: None,
            },
            Self::Forbidden => ErrorPayload {
                category: "auth".into(),
                code: "AUTH_WORK_TASK_FORBIDDEN".into(),
                message: "当前任务风险级别不允许该操作".into(),
                field: Some("action".into()),
                context: None,
            },
            Self::Repository(error) => ErrorPayload {
                category: "sys".into(),
                code: error.code().into(),
                message: "work task persistence unavailable".into(),
                field: None,
                context: None,
            },
            Self::Serialize(detail) => ErrorPayload {
                category: "sys".into(),
                code: "SYS_SERIALIZE".into(),
                message: detail.clone(),
                field: None,
                context: None,
            },
        }
    }

    fn into_json(self) -> String {
        serde_json::to_string(&self.payload()).unwrap_or_else(|_| {
            r#"{"category":"sys","code":"SYS_SERIALIZE","message":"failed to serialize work task error","field":null,"context":null}"#.into()
        })
    }
}

impl From<RepositoryError> for WorkTaskError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

#[derive(Clone)]
pub(crate) struct WorkTaskCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl WorkTaskCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    fn parse<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload)
            .map_err(|error| WorkTaskError::InvalidInput(error.to_string()).into_json())
    }

    fn serialize(value: impl serde::Serialize) -> Result<Value, String> {
        serde_json::to_value(value)
            .map_err(|error| WorkTaskError::Serialize(error.to_string()).into_json())
    }

    fn list_tasks(&self, ctx: &ExecutionContext, input: ListTasksInput) -> Result<Value, String> {
        let status_filter = input
            .status
            .unwrap_or_else(|| "queued,in_progress".to_owned());
        let statuses = status_filter
            .split(',')
            .map(str::trim)
            .filter(|status| !status.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if statuses.is_empty()
            || statuses.len() > ALLOWED_STATUSES.len()
            || statuses
                .iter()
                .any(|status| !ALLOWED_STATUSES.contains(&status.as_str()))
        {
            return Err(WorkTaskError::InvalidInput("无效的任务状态筛选".into()).into_json());
        }

        let scoped = self
            .repository_provider
            .bind(ctx)
            .map_err(|error| WorkTaskError::from(error).into_json())?;
        let tasks = scoped
            .work_tasks()
            .list(&WorkTaskListRequest {
                statuses,
                limit: u32::try_from(input.limit.unwrap_or(50).min(200)).unwrap_or(200),
            })
            .map_err(|error| WorkTaskError::from(error).into_json())?;
        Self::serialize(serde_json::json!({ "tasks": tasks }))
    }

    fn task_action(&self, ctx: &ExecutionContext, input: TaskActionInput) -> Result<Value, String> {
        let id = normalized_id(&input.id)?;
        let action = input.action.trim();
        if action.is_empty() {
            return Err(WorkTaskError::InvalidInput("任务操作不能为空".into()).into_json());
        }
        let expected_version = input.expected_version.ok_or_else(|| {
            WorkTaskError::InvalidInput("缺少 expectedVersion".into()).into_json()
        })?;
        let scoped = self
            .repository_provider
            .bind(ctx)
            .map_err(|error| WorkTaskError::from(error).into_json())?;
        let state = scoped
            .work_tasks()
            .get_state(id)
            .map_err(|error| WorkTaskError::from(error).into_json())?
            .ok_or_else(|| WorkTaskError::NotFound.into_json())?;
        validate_action_state(&state, action)?;

        let new_status = new_status_for(action, &state.risk);
        let updated = scoped
            .work_tasks()
            .update_status(id, expected_version, new_status, &shanghai_now_iso())
            .map_err(|error| WorkTaskError::from(error).into_json())?
            .ok_or_else(|| WorkTaskError::Conflict.into_json())?;
        action_response(new_status, updated)
    }

    fn send_to_pc(&self, ctx: &ExecutionContext, input: SendToPcInput) -> Result<Value, String> {
        let id = normalized_id(&input.id)?;
        let scoped = self
            .repository_provider
            .bind(ctx)
            .map_err(|error| WorkTaskError::from(error).into_json())?;
        let state = scoped
            .work_tasks()
            .get_state(id)
            .map_err(|error| WorkTaskError::from(error).into_json())?
            .ok_or_else(|| WorkTaskError::NotFound.into_json())?;
        validate_send_to_pc_state(&state)?;
        let updated = scoped
            .work_tasks()
            .send_to_pc(id, input.expected_version, &shanghai_now_iso())
            .map_err(|error| WorkTaskError::from(error).into_json())?
            .ok_or_else(|| WorkTaskError::Conflict.into_json())?;
        action_response("in_progress", updated)
    }
}

impl SystemModule for WorkTaskCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: MODULE_NAME.into(),
            version: "0.1.0".into(),
            description: "Tenant-scoped Work/HUD task queue compatibility".into(),
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
            "list_tasks" => self.list_tasks(ctx, Self::parse(payload)?),
            "task_action" => self.task_action(ctx, Self::parse(payload)?),
            "send_to_pc" => self.send_to_pc(ctx, Self::parse(payload)?),
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
                "list_tasks",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "task_action",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "send_to_pc",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: MODULE_NAME.into(),
            description: "Tenant-scoped Work/HUD task queue compatibility".into(),
            commands: self
                .commands()
                .into_iter()
                .map(|command| CommandSchema {
                    name: command.name.into(),
                    description: "Work task compatibility command".into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                })
                .collect(),
        }
    }
}

fn normalized_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err(WorkTaskError::InvalidInput("任务 id 不能为空".into()).into_json());
    }
    Ok(id)
}

fn validate_action_state(state: &WorkTaskState, action: &str) -> Result<(), String> {
    if !state
        .capabilities
        .iter()
        .any(|capability| capability == action)
    {
        return Err(WorkTaskError::UnsupportedAction(action.into()).into_json());
    }
    if state.status != "queued" && state.status != "in_progress" {
        return Err(WorkTaskError::Terminal.into_json());
    }
    if !huddable(action, &state.risk) {
        return Err(WorkTaskError::Forbidden.into_json());
    }
    Ok(())
}

fn validate_send_to_pc_state(state: &WorkTaskState) -> Result<(), String> {
    if state.status != "queued" && state.status != "in_progress" {
        return Err(WorkTaskError::Terminal.into_json());
    }
    if !state
        .capabilities
        .iter()
        .any(|capability| capability == "send_to_pc")
    {
        return Err(WorkTaskError::Forbidden.into_json());
    }
    if !huddable("send_to_pc", &state.risk) {
        return Err(WorkTaskError::Forbidden.into_json());
    }
    Ok(())
}

fn huddable(action: &str, risk: &str) -> bool {
    match risk {
        "low" => matches!(
            action,
            "acknowledge" | "confirm" | "scan" | "defer" | "open_work" | "send_to_pc"
        ),
        "medium" | "high" => matches!(action, "defer" | "open_work" | "send_to_pc"),
        _ => false,
    }
}

fn new_status_for(action: &str, _risk: &str) -> &'static str {
    match action {
        "acknowledge" | "confirm" | "scan" => "completed",
        "defer" => "queued",
        "open_work" | "send_to_pc" => "in_progress",
        _ => "queued",
    }
}

fn action_response(new_status: &str, task: WorkTaskMutation) -> Result<Value, String> {
    WorkTaskCompatibilityModule::serialize(serde_json::json!({
        "status": "ok",
        "newStatus": new_status,
        "task": task,
    }))
}

#[cfg(test)]
mod tests {
    use crate::repositories::WorkTaskState;

    use super::{huddable, new_status_for, validate_send_to_pc_state};

    #[test]
    fn low_risk_tasks_keep_existing_hud_capability_matrix() {
        assert!(huddable("defer", "low"));
        assert!(huddable("send_to_pc", "low"));
    }

    #[test]
    fn elevated_risk_tasks_keep_existing_hud_restrictions() {
        for risk in ["medium", "high"] {
            assert!(!huddable("confirm", risk));
            assert!(huddable("send_to_pc", risk));
        }
    }

    #[test]
    fn actions_keep_existing_canonical_task_states() {
        assert_eq!(new_status_for("acknowledge", "low"), "completed");
        assert_eq!(new_status_for("defer", "low"), "queued");
        assert_eq!(new_status_for("open_work", "high"), "in_progress");
    }

    #[test]
    fn send_to_pc_preserves_legacy_terminal_capability_and_risk_ordering() {
        let terminal = WorkTaskState {
            status: "completed".into(),
            risk: "low".into(),
            capabilities: vec![],
            version: 2,
        };
        assert!(
            validate_send_to_pc_state(&terminal)
                .unwrap_err()
                .contains("BIZ_WORK_TASK_TERMINAL")
        );

        let missing_capability = WorkTaskState {
            status: "queued".into(),
            risk: "low".into(),
            capabilities: vec![],
            version: 1,
        };
        assert!(
            validate_send_to_pc_state(&missing_capability)
                .unwrap_err()
                .contains("AUTH_WORK_TASK_FORBIDDEN")
        );

        let high_risk_allowed = WorkTaskState {
            status: "queued".into(),
            risk: "high".into(),
            capabilities: vec!["send_to_pc".into()],
            version: 1,
        };
        assert!(validate_send_to_pc_state(&high_risk_allowed).is_ok());
    }
}

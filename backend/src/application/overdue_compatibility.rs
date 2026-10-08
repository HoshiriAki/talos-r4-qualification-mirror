use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value, json};
use system_admin::overdue::FeatureOverdue;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{
    OverdueFeeConfigPatch, OverdueMutationError, RepositoryError, RepositoryProvider,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct OverdueCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum OrderIdInput {
    Text(String),
    Integer(i64),
}

impl OrderIdInput {
    fn normalized(self) -> Result<String, String> {
        let value = match self {
            Self::Text(value) => value.trim().to_owned(),
            Self::Integer(value) if value > 0 => value.to_string(),
            Self::Integer(_) => String::new(),
        };
        if value.is_empty() {
            Err(input_error("VAL_REQUIRED", "订单ID无效", Some("orderId")))
        } else {
            Ok(value)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CalcInput {
    order_id: OrderIdInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyInput {
    overdue_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WaiveInput {
    overdue_id: i64,
    waived_reason: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListInput {
    status: Option<String>,
    customer_phone: Option<String>,
    order_id: Option<OrderIdInput>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecordIdInput {
    overdue_id: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigUpsertInput {
    daily_rate: Option<f64>,
    max_days: Option<i64>,
    cap_multiplier: Option<f64>,
    grace_period_hours: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckBeforeOrderInput {
    customer_phone: String,
}

impl OverdueCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    fn scoped(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<crate::repositories::ScopedRepositories, String> {
        self.repository_provider.bind(ctx).map_err(repository_error)
    }

    fn parse<T: serde::de::DeserializeOwned>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload).map_err(|error| {
            input_error("VAL_DESERIALIZE", &format!("输入参数无效: {error}"), None)
        })
    }

    fn admin(ctx: &ExecutionContext) -> Result<(), String> {
        if ctx.has_tenant_admin_authority() {
            Ok(())
        } else {
            Err(error("auth", "AUTH_FORBIDDEN", "仅管理员可操作", None))
        }
    }

    fn now_and_today() -> (String, String) {
        let now = shanghai_now_iso();
        let today = now.get(..10).unwrap_or_default().to_owned();
        (now, today)
    }

    fn detect(&self, ctx: &ExecutionContext) -> Result<Value, String> {
        Self::admin(ctx)?;
        let scoped = self.scoped(ctx)?;
        let (now, today) = Self::now_and_today();
        scoped
            .overdues()
            .detect(&today, &now)
            .map(with_ok)
            .map_err(repository_error)
    }

    fn calc(&self, ctx: &ExecutionContext, input: CalcInput) -> Result<Value, String> {
        let order_id = input.order_id.normalized()?;
        let scoped = self.scoped(ctx)?;
        let (_, today) = Self::now_and_today();
        scoped
            .overdues()
            .calc(&order_id, &today)
            .map(with_ok)
            .map_err(repository_error)
    }

    fn apply(&self, ctx: &ExecutionContext, input: ApplyInput) -> Result<Value, String> {
        require_positive_id(input.overdue_id)?;
        let scoped = self.scoped(ctx)?;
        scoped
            .overdues()
            .apply_delegated(input.overdue_id)
            .map(|mut value| {
                if let Some(map) = value.as_object_mut() {
                    map.insert("ok".into(), Value::Bool(true));
                    map.insert(
                        "financialEffectNote".into(),
                        Value::String(
                            "Legacy Overdue no longer writes pricing snapshots or fabricates payment; R3 settlement owns additional-charge effects.".into(),
                        ),
                    );
                }
                value
            })
            .map_err(|error| mutation_error(error, input.overdue_id))
    }

    fn waive(&self, ctx: &ExecutionContext, mut input: WaiveInput) -> Result<Value, String> {
        Self::admin(ctx)?;
        require_positive_id(input.overdue_id)?;
        input.waived_reason = input.waived_reason.trim().to_owned();
        if input.waived_reason.is_empty() {
            return Err(input_error(
                "VAL_REQUIRED",
                "豁免原因不能为空",
                Some("waivedReason"),
            ));
        }
        let actor = ctx
            .user_id()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| error("auth", "AUTH_REQUIRED", "需要管理员身份", None))?;
        let scoped = self.scoped(ctx)?;
        scoped
            .overdues()
            .waive(
                input.overdue_id,
                &input.waived_reason,
                actor,
                &shanghai_now_iso(),
            )
            .map(with_ok)
            .map_err(|error| mutation_error(error, input.overdue_id))
    }

    fn list(&self, ctx: &ExecutionContext, mut input: ListInput) -> Result<Value, String> {
        let order_id = input
            .order_id
            .take()
            .map(OrderIdInput::normalized)
            .transpose()?;
        let status = trim_option(input.status);
        let phone = trim_option(input.customer_phone);
        let scoped = self.scoped(ctx)?;
        scoped
            .overdues()
            .list(
                status.as_deref(),
                phone.as_deref(),
                order_id.as_deref(),
                input.page.unwrap_or(1),
                input.page_size.unwrap_or(20),
            )
            .map(with_ok)
            .map_err(repository_error)
    }

    fn get(&self, ctx: &ExecutionContext, input: RecordIdInput) -> Result<Value, String> {
        require_positive_id(input.overdue_id)?;
        let scoped = self.scoped(ctx)?;
        let record = scoped
            .overdues()
            .get(input.overdue_id)
            .map_err(repository_error)?
            .ok_or_else(|| {
                error(
                    "biz",
                    "BIZ_OVERDUE_NOT_FOUND",
                    "逾期记录不存在",
                    Some("overdueId"),
                )
            })?;
        Ok(json!({ "ok": true, "record": record }))
    }

    fn config_get(&self, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let config = scoped.overdues().config_get().map_err(repository_error)?;
        Ok(json!({
            "ok": true,
            "config": config,
            "formula": "min(days * dailyRate, dailyRate * maxDays * capMultiplier)",
        }))
    }

    fn config_upsert(
        &self,
        ctx: &ExecutionContext,
        input: ConfigUpsertInput,
    ) -> Result<Value, String> {
        Self::admin(ctx)?;
        validate_config(&input)?;
        let scoped = self.scoped(ctx)?;
        let now = shanghai_now_iso();
        let config = scoped
            .overdues()
            .config_upsert(
                OverdueFeeConfigPatch {
                    daily_rate: input.daily_rate,
                    max_days: input.max_days,
                    cap_multiplier: input.cap_multiplier,
                    grace_period_hours: input.grace_period_hours,
                },
                &now,
            )
            .map_err(repository_error)?;
        Ok(json!({
            "ok": true,
            "config": config,
            "updatedAt": now,
        }))
    }

    fn escalate(&self, ctx: &ExecutionContext) -> Result<Value, String> {
        Self::admin(ctx)?;
        let scoped = self.scoped(ctx)?;
        scoped
            .overdues()
            .escalate(&shanghai_now_iso())
            .map(with_ok)
            .map_err(repository_error)
    }

    fn escalation_history(
        &self,
        ctx: &ExecutionContext,
        input: RecordIdInput,
    ) -> Result<Value, String> {
        require_positive_id(input.overdue_id)?;
        let scoped = self.scoped(ctx)?;
        let history = scoped
            .overdues()
            .escalation_history(input.overdue_id)
            .map_err(repository_error)?;
        Ok(json!({ "ok": true, "history": history }))
    }

    fn stats(&self, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        scoped
            .overdues()
            .stats()
            .map(with_ok)
            .map_err(repository_error)
    }

    fn check_before_order(
        &self,
        ctx: &ExecutionContext,
        mut input: CheckBeforeOrderInput,
    ) -> Result<Value, String> {
        input.customer_phone = input.customer_phone.trim().to_owned();
        if input.customer_phone.is_empty() {
            return Err(input_error(
                "VAL_REQUIRED",
                "客户电话不能为空",
                Some("customerPhone"),
            ));
        }
        let scoped = self.scoped(ctx)?;
        scoped
            .overdues()
            .check_before_order(&input.customer_phone)
            .map(with_ok)
            .map_err(repository_error)
    }
}

impl SystemModule for OverdueCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureOverdue::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureOverdue::new().commands()
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
            "overdue_detect" => self.detect(ctx),
            "overdue_calc" => self.calc(ctx, Self::parse(payload)?),
            "overdue_apply" => self.apply(ctx, Self::parse(payload)?),
            "overdue_waive" => self.waive(ctx, Self::parse(payload)?),
            "overdue_list" => self.list(ctx, Self::parse(payload)?),
            "overdue_get" => self.get(ctx, Self::parse(payload)?),
            "overdue_config_get" => self.config_get(ctx),
            "overdue_config_upsert" => self.config_upsert(ctx, Self::parse(payload)?),
            "overdue_escalate" => self.escalate(ctx),
            "overdue_escalation_history" => self.escalation_history(ctx, Self::parse(payload)?),
            "overdue_stats" => self.stats(ctx),
            "overdue_check_before_order" => self.check_before_order(ctx, Self::parse(payload)?),
            _ => Err(error(
                "sys",
                "SYS_UNKNOWN_COMMAND",
                &format!("未知 overdue 命令: {command}"),
                None,
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureOverdue::new().schema()
    }
}

fn validate_config(input: &ConfigUpsertInput) -> Result<(), String> {
    if let Some(value) = input.daily_rate
        && (!value.is_finite() || value < 0.0)
    {
        return Err(input_error(
            "VAL_INVALID",
            "日费率必须 >= 0",
            Some("dailyRate"),
        ));
    }
    if let Some(value) = input.max_days
        && !(1..=365).contains(&value)
    {
        return Err(input_error(
            "VAL_INVALID",
            "最大天数必须在 1-365 之间",
            Some("maxDays"),
        ));
    }
    if let Some(value) = input.cap_multiplier
        && (!value.is_finite() || !(1.0..=10.0).contains(&value))
    {
        return Err(input_error(
            "VAL_INVALID",
            "上限乘数必须在 1.0-10.0 之间",
            Some("capMultiplier"),
        ));
    }
    if let Some(value) = input.grace_period_hours
        && !(0..=168).contains(&value)
    {
        return Err(input_error(
            "VAL_INVALID",
            "宽限期必须在 0-168 小时之间",
            Some("gracePeriodHours"),
        ));
    }
    Ok(())
}

fn mutation_error(error_value: OverdueMutationError, overdue_id: i64) -> String {
    match error_value {
        OverdueMutationError::NotFound => error(
            "biz",
            "BIZ_OVERDUE_NOT_FOUND",
            &format!("逾期记录 {overdue_id} 不存在"),
            Some("overdueId"),
        ),
        OverdueMutationError::AlreadyWaived => error(
            "biz",
            "BIZ_ALREADY_WAIVED",
            "该逾期记录已被豁免",
            Some("overdueId"),
        ),
        OverdueMutationError::StatusInvalid(status) => error(
            "biz",
            "BIZ_STATUS_INVALID",
            &format!("当前状态 '{status}' 不可执行该操作"),
            Some("overdueId"),
        ),
        OverdueMutationError::Storage(error_value) => repository_error(error_value),
    }
}

fn require_positive_id(value: i64) -> Result<(), String> {
    if value > 0 {
        Ok(())
    } else {
        Err(input_error(
            "VAL_REQUIRED",
            "逾期记录ID无效",
            Some("overdueId"),
        ))
    }
}

fn trim_option(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn repository_error(error_value: RepositoryError) -> String {
    error(
        if matches!(error_value, RepositoryError::ContractViolation(_)) {
            "biz"
        } else {
            "sys"
        },
        error_value.code(),
        &error_value.to_string(),
        None,
    )
}

fn input_error(code: &str, message: &str, field: Option<&str>) -> String {
    error("val", code, message, field)
}

fn error(category: &str, code: &str, message: &str, field: Option<&str>) -> String {
    serde_json::to_string(&ErrorPayload {
        category: category.into(),
        code: code.into(),
        message: message.into(),
        field: field.map(str::to_owned),
        context: None,
    })
    .unwrap_or_default()
}

fn with_ok(value: Value) -> Value {
    match value {
        Value::Object(mut object) => {
            object.insert("ok".into(), Value::Bool(true));
            Value::Object(object)
        }
        other => {
            let mut object = Map::new();
            object.insert("ok".into(), Value::Bool(true));
            object.insert("data".into(), other);
            Value::Object(object)
        }
    }
}

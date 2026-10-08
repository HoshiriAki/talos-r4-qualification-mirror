use std::sync::Arc;

use serde_json::{Map, Value, json};
use system_admin::credit::{
    BlacklistAddInput, BlacklistCheckInput, BlacklistListInput, BlacklistRemoveInput,
    CheckBeforeOrderInput, CreditGetInput, CreditHistoryInput, CreditRecalculateInput,
    FeatureCredit, ViolationAppealInput, ViolationGetInput, ViolationListInput,
    ViolationRecordInput, ViolationReviewInput,
};
use system_core::{
    ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, Sanitize, SystemModule,
    Unvalidated, Validate,
};

use crate::repositories::{
    CreditMutationError, RepositoryError, RepositoryProvider, credit_score_label,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct CreditCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl CreditCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    fn scoped(
        &self,
        ctx: &ExecutionContext,
    ) -> Result<crate::repositories::ScopedRepositories, String> {
        self.repository_provider
            .bind(ctx)
            .map_err(Self::repository_error)
    }

    fn repository_error(error: RepositoryError) -> String {
        err("sys", error.code(), "credit persistence unavailable".into())
    }

    fn mutation_error(error: CreditMutationError) -> String {
        match error {
            CreditMutationError::DuplicateBlacklist => {
                err("biz", "BIZ_DUPLICATE", "该客户已在黑名单中".into())
            }
            CreditMutationError::BlacklistNotFound => {
                err("biz", "BIZ_NOT_FOUND", "黑名单记录不存在".into())
            }
            CreditMutationError::BlacklistAlreadyRemoved => {
                err("biz", "BIZ_ALREADY_REMOVED", "该黑名单记录已被移除".into())
            }
            CreditMutationError::ViolationNotFound => {
                err("db", "DB_QUERY", "违规记录不存在".into())
            }
            CreditMutationError::StatusInvalid(status) => err(
                "biz",
                "BIZ_STATUS_INVALID",
                format!("当前状态 '{status}' 不可执行该操作"),
            ),
            CreditMutationError::CreditNotFound => err("db", "DB_QUERY", "信用档案不存在".into()),
            CreditMutationError::Storage(error) => Self::repository_error(error),
        }
    }

    fn require_admin(ctx: &ExecutionContext) -> Result<(), String> {
        if !ctx.has_tenant_admin_authority() {
            return Err(err("auth", "AUTH_FORBIDDEN", "仅管理员可操作".into()));
        }
        Ok(())
    }

    fn actor(ctx: &ExecutionContext, message: &str) -> Result<String, String> {
        ctx.user_id()
            .map(str::to_owned)
            .ok_or_else(|| err("auth", "AUTH_REQUIRED", message.into()))
    }

    fn blacklist_add(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        let input: BlacklistAddInput = validated(payload)?;
        let actor = Self::actor(ctx, "需要管理员身份")?;
        let now = shanghai_now_iso();
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .credits()
            .blacklist_add(
                &input.customer_name,
                &input.customer_phone,
                input.id_number.as_deref(),
                &input.reason,
                &input.severity,
                &actor,
                &now,
            )
            .map_err(Self::mutation_error)?;
        Ok(json!({
            "ok": true,
            "id": outcome.id,
            "createdAt": outcome.created_at,
        }))
    }

    fn blacklist_remove(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        let input: BlacklistRemoveInput = validated(payload)?;
        let actor = Self::actor(ctx, "需要管理员身份")?;
        let now = shanghai_now_iso();
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .credits()
            .blacklist_remove(input.id, &input.removal_reason, &actor, &now)
            .map_err(Self::mutation_error)?;
        Ok(json!({
            "ok": true,
            "id": outcome.id,
            "removedAt": outcome.removed_at,
        }))
    }

    fn blacklist_check(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        let input: BlacklistCheckInput = validated(payload)?;
        let scoped = self.scoped(ctx)?;
        with_ok(
            scoped
                .credits()
                .blacklist_check(&input.customer_phone)
                .map_err(Self::repository_error)?,
        )
    }

    fn blacklist_list(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        let input: BlacklistListInput = validated(payload)?;
        let scoped = self.scoped(ctx)?;
        with_ok(
            scoped
                .credits()
                .blacklist_list(
                    input.is_active,
                    input.page.unwrap_or(1),
                    input.page_size.unwrap_or(20),
                )
                .map_err(Self::repository_error)?,
        )
    }

    fn violation_record(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        let input: ViolationRecordInput = validated(payload)?;
        let actor = Self::actor(ctx, "需要登录")?;
        let now = shanghai_now_iso();
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .credits()
            .violation_record(
                &input.customer_name,
                &input.customer_phone,
                input.order_id,
                &input.violation_type,
                &input.severity,
                &input.description,
                input.evidence.as_deref(),
                input.financial_penalty.unwrap_or(0.0),
                &actor,
                &now,
            )
            .map_err(Self::mutation_error)?;
        Ok(json!({
            "ok": true,
            "id": outcome.id,
            "penalty": outcome.penalty,
            "createdAt": outcome.created_at,
        }))
    }

    fn violation_appeal(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        let input: ViolationAppealInput = validated(payload)?;
        let now = shanghai_now_iso();
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .credits()
            .violation_appeal(input.id, &input.appeal_reason, &now)
            .map_err(Self::mutation_error)?;
        Ok(json!({
            "ok": true,
            "id": outcome.id,
            "status": outcome.status,
            "appealAt": outcome.at,
        }))
    }

    fn violation_review(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        let input: ViolationReviewInput = validated(payload)?;
        let actor = Self::actor(ctx, "需要管理员身份")?;
        let now = shanghai_now_iso();
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .credits()
            .violation_review(input.id, &input.status, &input.review_notes, &actor, &now)
            .map_err(Self::mutation_error)?;
        Ok(json!({
            "ok": true,
            "id": outcome.id,
            "status": outcome.status,
            "reviewedAt": outcome.at,
        }))
    }

    fn violation_list(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        let input: ViolationListInput = validated(payload)?;
        let scoped = self.scoped(ctx)?;
        with_ok(
            scoped
                .credits()
                .violation_list(
                    input.customer_phone.as_deref(),
                    input.status.as_deref(),
                    input.violation_type.as_deref(),
                    input.page.unwrap_or(1),
                    input.page_size.unwrap_or(20),
                )
                .map_err(Self::repository_error)?,
        )
    }

    fn violation_get(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        let input: ViolationGetInput = validated(payload)?;
        let scoped = self.scoped(ctx)?;
        let record = scoped
            .credits()
            .violation_get(input.id)
            .map_err(Self::repository_error)?
            .ok_or_else(|| err("db", "DB_QUERY", "违规记录不存在".into()))?;
        Ok(json!({
            "ok": true,
            "record": record,
        }))
    }

    fn credit_get(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        let input: CreditGetInput = validated(payload)?;
        let now = shanghai_now_iso();
        let scoped = self.scoped(ctx)?;
        let record = scoped
            .credits()
            .credit_get(&input.customer_phone, &now)
            .map_err(Self::mutation_error)?;
        Ok(json!({
            "ok": true,
            "record": record,
        }))
    }

    fn credit_history(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        let input: CreditHistoryInput = validated(payload)?;
        let scoped = self.scoped(ctx)?;
        with_ok(
            scoped
                .credits()
                .credit_history(&input.customer_phone)
                .map_err(Self::repository_error)?,
        )
    }

    fn credit_recalculate(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        Self::require_admin(ctx)?;
        let input: CreditRecalculateInput = validated(payload)?;
        let now = shanghai_now_iso();
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .credits()
            .credit_recalculate(&input.customer_phone, &now)
            .map_err(Self::mutation_error)?;
        Ok(json!({
            "ok": true,
            "customerPhone": outcome.customer_phone,
            "score": outcome.score,
            "label": credit_score_label(outcome.score),
            "formula": "100 + onTime*2 - late*5 - damage*10",
            "components": {
                "onTimeReturns": outcome.on_time_returns,
                "lateReturns": outcome.late_returns,
                "damageIncidents": outcome.damage_incidents,
            },
            "recalculatedAt": outcome.recalculated_at,
        }))
    }

    fn check_before_order(&self, ctx: &ExecutionContext, payload: Value) -> Result<Value, String> {
        let input: CheckBeforeOrderInput = validated(payload)?;
        let scoped = self.scoped(ctx)?;
        with_ok(
            scoped
                .credits()
                .check_before_order(&input.customer_phone)
                .map_err(Self::repository_error)?,
        )
    }
}

impl SystemModule for CreditCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureCredit::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureCredit::new().commands()
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
            "blacklist_add" => self.blacklist_add(ctx, payload),
            "blacklist_remove" => self.blacklist_remove(ctx, payload),
            "blacklist_check" => self.blacklist_check(ctx, payload),
            "blacklist_list" => self.blacklist_list(ctx, payload),
            "violation_record" => self.violation_record(ctx, payload),
            "violation_appeal" => self.violation_appeal(ctx, payload),
            "violation_review" => self.violation_review(ctx, payload),
            "violation_list" => self.violation_list(ctx, payload),
            "violation_get" => self.violation_get(ctx, payload),
            "credit_get" => self.credit_get(ctx, payload),
            "credit_history" => self.credit_history(ctx, payload),
            "credit_recalculate" => self.credit_recalculate(ctx, payload),
            "check_before_order" => self.check_before_order(ctx, payload),
            _ => Err(err(
                "sys",
                "CMD_UNKNOWN",
                format!("Unknown command: {command}"),
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureCredit::new().schema()
    }
}

fn validated<T>(payload: Value) -> Result<T, String>
where
    T: Sanitize + Validate,
    Unvalidated<T>: TryFrom<Value, Error = String>,
{
    let unvalidated: Unvalidated<T> = payload.try_into()?;
    Ok(unvalidated.sanitize().validate()?.into_inner())
}

fn with_ok(value: Value) -> Result<Value, String> {
    match value {
        Value::Object(mut object) => {
            object.insert("ok".into(), Value::Bool(true));
            Ok(Value::Object(object))
        }
        other => {
            let mut object = Map::new();
            object.insert("ok".into(), Value::Bool(true));
            object.insert("value".into(), other);
            Ok(Value::Object(object))
        }
    }
}

fn err(category: &str, code: &str, message: String) -> String {
    serde_json::to_string(&ErrorPayload {
        category: category.into(),
        code: code.into(),
        message,
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

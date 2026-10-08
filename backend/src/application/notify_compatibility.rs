use std::sync::Arc;

use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};
use system_notify::notify::{
    FeatureNotification, ListInput, MarkReadInput, SendInput, TriggerInput, UpsertTemplateInput,
};

use crate::repositories::{
    NotificationMutationError, RepositoryError, RepositoryProvider, ScopedRepositories,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct NotifyCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl NotifyCompatibilityModule {
    pub(crate) fn new(repository_provider: Arc<dyn RepositoryProvider>) -> Self {
        Self {
            repository_provider,
        }
    }

    fn scoped(&self, ctx: &ExecutionContext) -> Result<ScopedRepositories, String> {
        self.repository_provider
            .bind(ctx)
            .map_err(Self::repository_error)
    }

    fn parse<T: serde::de::DeserializeOwned>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload).map_err(|error| format!("VAL_DESERIALIZE: {error}"))
    }

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "notification persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn template_not_found(event_type: &str, channel: &str) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "biz".into(),
            code: "BIZ_TEMPLATE_NOT_FOUND".into(),
            message: format!("模板不存在: event_type={event_type}, channel={channel}"),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn render_template(template: &str, variables: &Value) -> String {
        let mut result = template.to_owned();
        if let Some(object) = variables.as_object() {
            for (key, value) in object {
                let replacement = match value {
                    Value::String(value) => value.clone(),
                    Value::Number(value) => value.to_string(),
                    _ => value.to_string(),
                };
                result = result.replace(&format!("{{{key}}}"), &replacement);
            }
        }
        result
    }

    fn send_internal(
        scoped: &ScopedRepositories,
        input: &SendInput,
        recipient: &str,
    ) -> Result<Value, String> {
        let channel = input.channel.clone().unwrap_or_else(|| "in_app".to_owned());
        let variables = input
            .variables
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        let template = scoped
            .notifications()
            .template(&input.event_type, &channel)
            .map_err(Self::repository_error)?
            .ok_or_else(|| Self::template_not_found(&input.event_type, &channel))?;

        let subject = Self::render_template(&template.subject_template, &variables);
        let body = Self::render_template(&template.body_template, &variables);

        match channel.as_str() {
            "email" => tracing::info!(
                target: "notify",
                event_type = %input.event_type,
                channel = "email",
                recipient = %recipient,
                subject = %subject,
                "Email notification (stub) — SMTP not configured"
            ),
            "sms" => tracing::info!(
                target: "notify",
                event_type = %input.event_type,
                channel = "sms",
                recipient = %recipient,
                body = %body,
                "SMS notification (stub) — SMS provider not configured"
            ),
            _ => {}
        }

        let persisted_channel = match channel.as_str() {
            "email" => "email",
            "sms" => "sms",
            _ => "in_app",
        };
        let outcome = scoped
            .notifications()
            .insert_log(
                &template.id,
                &input.event_type,
                persisted_channel,
                recipient,
                &subject,
                &body,
                &shanghai_now_iso(),
            )
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "messageId": outcome.message_id,
            "eventType": outcome.event_type,
            "channel": channel,
            "status": "sent",
        }))
    }

    fn send(&self, ctx: &ExecutionContext, input: SendInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let user_id = ctx.user_id().unwrap_or("system");
        let recipient = input
            .recipient
            .clone()
            .unwrap_or_else(|| user_id.to_owned());
        Self::send_internal(&scoped, &input, &recipient)
    }

    fn list(&self, ctx: &ExecutionContext, input: ListInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let user_id = ctx.user_id().unwrap_or("system");
        let result = scoped
            .notifications()
            .list_in_app(
                user_id,
                input.page.unwrap_or(1).max(1),
                input.page_size.unwrap_or(20).clamp(1, 100),
            )
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "messages": result.messages,
            "unreadCount": result.unread_count,
            "pagination": {
                "page": result.page,
                "pageSize": result.page_size,
                "total": result.total,
            },
        }))
    }

    fn mark_read(&self, ctx: &ExecutionContext, input: MarkReadInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let user_id = ctx.user_id().unwrap_or("system");
        let affected = scoped
            .notifications()
            .mark_read(input.message_id.as_deref(), user_id, &shanghai_now_iso())
            .map_err(Self::repository_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "affected": affected,
        }))
    }

    fn get_templates(&self, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let templates = scoped
            .notifications()
            .list_templates()
            .map_err(Self::repository_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "templates": templates,
        }))
    }

    fn upsert_template(
        &self,
        ctx: &ExecutionContext,
        input: UpsertTemplateInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .notifications()
            .upsert_template(
                input.id.as_deref(),
                &input.event_type,
                &input.channel,
                &input.subject_template,
                &input.body_template,
                input.is_enabled.unwrap_or(true),
                &shanghai_now_iso(),
            )
            .map_err(|error| match error {
                NotificationMutationError::TemplateNotFound => {
                    let id = input.id.as_deref().unwrap_or_default();
                    serde_json::to_string(&ErrorPayload {
                        category: "biz".into(),
                        code: "BIZ_TEMPLATE_NOT_FOUND".into(),
                        message: format!("模板 {id} 不存在"),
                        field: Some("id".into()),
                        context: None,
                    })
                    .unwrap_or_default()
                }
                NotificationMutationError::Storage(error) => Self::repository_error(error),
            })?;

        Ok(if outcome.created {
            serde_json::json!({
                "ok": true,
                "id": outcome.id,
                "created": true,
            })
        } else {
            serde_json::json!({
                "ok": true,
                "id": outcome.id,
                "updated": true,
            })
        })
    }

    fn trigger(&self, ctx: &ExecutionContext, input: TriggerInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let user_id = ctx.user_id().unwrap_or("system");
        let variables = input
            .variables
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        let channels = scoped
            .notifications()
            .enabled_channels(&input.event_type)
            .map_err(Self::repository_error)?;

        if channels.is_empty() {
            return Ok(serde_json::json!({
                "ok": true,
                "results": [],
                "message": "no enabled templates for this event",
            }));
        }

        let external_recipient = variables
            .get("customerName")
            .and_then(Value::as_str)
            .unwrap_or(user_id)
            .to_owned();

        let mut hydrated = variables;
        if let Some(order_id) = input.order_id.as_deref() {
            if let Ok(Some(order)) = scoped.notifications().order_context(order_id) {
                if let Some(object) = hydrated.as_object_mut() {
                    object.insert("orderNo".into(), Value::String(order.order_no));
                    object.insert(
                        "deviceSerialNo".into(),
                        Value::String(order.device_serial_no),
                    );
                    object.insert("customerName".into(), Value::String(order.customer_name));
                    object.insert("dueDate".into(), Value::String(order.due_date));
                }
            }
        }

        let mut results = Vec::new();
        for channel in channels {
            let recipient = if channel == "in_app" {
                user_id.to_owned()
            } else {
                external_recipient.clone()
            };
            let send_input = SendInput {
                event_type: input.event_type.clone(),
                channel: Some(channel.clone()),
                recipient: Some(recipient.clone()),
                variables: Some(hydrated.clone()),
            };
            match Self::send_internal(&scoped, &send_input, &recipient) {
                Ok(result) => results.push(result),
                Err(error) => {
                    tracing::warn!(
                        target: "notify",
                        channel = %channel,
                        error = %error,
                        "Trigger send failed for channel"
                    );
                    results.push(serde_json::json!({
                        "channel": channel,
                        "status": "failed",
                        "error": error,
                    }));
                }
            }
        }

        Ok(serde_json::json!({
            "ok": true,
            "results": results,
        }))
    }
}

impl SystemModule for NotifyCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureNotification::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureNotification::new().commands()
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
            "send" => self.send(ctx, Self::parse(payload)?),
            "list" => self.list(ctx, Self::parse(payload)?),
            "mark_read" => self.mark_read(ctx, Self::parse(payload)?),
            "get_templates" => self.get_templates(ctx),
            "upsert_template" => self.upsert_template(ctx, Self::parse(payload)?),
            "trigger" => self.trigger(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: notify.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureNotification::new().schema()
    }
}

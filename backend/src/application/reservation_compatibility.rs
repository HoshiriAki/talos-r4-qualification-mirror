use std::sync::Arc;

use serde_json::Value;
use system_admin::reservation::{
    CheckAvailabilityInput, FeatureReservation, ReservationGetInput, ReservationListInput,
    RuleGetInput, RuleUpdateInput,
};
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{
    LegacyReservationRule, LegacyReservationRulePatch, RepositoryError, RepositoryProvider,
};
use crate::utils::time::shanghai_now_iso;

#[derive(Clone)]
pub(crate) struct ReservationCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl ReservationCompatibilityModule {
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
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "legacy reservation compatibility persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn error(category: &str, code: &str, message: String) -> String {
        serde_json::to_string(&ErrorPayload {
            category: category.into(),
            code: code.into(),
            message,
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn retired_write(command: &str) -> String {
        Self::error(
            "biz",
            "BIZ_LEGACY_RESERVATION_WRITE_RETIRED",
            format!(
                "legacy reservation.{command} no longer owns Reservation writes; use reservation_v2"
            ),
        )
    }

    fn list(&self, ctx: &ExecutionContext, input: ReservationListInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
        let result = scoped
            .legacy_reservations()
            .list(
                input.status.as_deref(),
                input.device_serial_no.as_deref(),
                input.customer_phone.as_deref(),
                input.warehouse_id,
                page,
                page_size,
            )
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "items": result.items,
            "total": result.total,
            "page": result.page,
            "pageSize": result.page_size,
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: ReservationGetInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let record = scoped
            .legacy_reservations()
            .get(input.id)
            .map_err(Self::repository_error)?
            .ok_or_else(|| {
                Self::error(
                    "biz",
                    "BIZ_RESERVATION_NOT_FOUND",
                    format!("legacy reservation {} not found in active tenant", input.id),
                )
            })?;

        Ok(serde_json::json!({
            "ok": true,
            "record": record,
        }))
    }

    fn check_availability(
        &self,
        ctx: &ExecutionContext,
        input: CheckAvailabilityInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let conflicts = scoped
            .legacy_reservations()
            .conflicts(&input.device_serial_no, &input.start_date, &input.end_date)
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": input.device_serial_no,
            "startDate": input.start_date,
            "endDate": input.end_date,
            "available": conflicts.is_empty(),
            "conflicts": conflicts,
        }))
    }

    fn rule_get(&self, ctx: &ExecutionContext, _input: RuleGetInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let now = shanghai_now_iso();
        let rule = scoped
            .legacy_reservations()
            .get_rule()
            .map_err(Self::repository_error)?
            .unwrap_or(LegacyReservationRule {
                id: 0,
                rule_name: "default".into(),
                max_days_ahead: 90,
                max_concurrent_per_customer: 2,
                auto_release_minutes: 30,
                is_active: true,
                created_at: now.clone(),
                updated_at: now,
            });

        Ok(serde_json::json!({
            "ok": true,
            "rule": rule,
        }))
    }

    fn rule_update(&self, ctx: &ExecutionContext, input: RuleUpdateInput) -> Result<Value, String> {
        if !ctx.has_tenant_admin_authority() {
            return Err(Self::error(
                "auth",
                "AUTH_FORBIDDEN",
                "仅管理员可操作".into(),
            ));
        }

        let scoped = self.scoped(ctx)?;
        let rule = scoped
            .legacy_reservations()
            .update_rule(
                &LegacyReservationRulePatch {
                    max_days_ahead: input.max_days_ahead,
                    max_concurrent_per_customer: input.max_concurrent_per_customer,
                    auto_release_minutes: input.auto_release_minutes,
                    is_active: input.is_active,
                },
                &shanghai_now_iso(),
            )
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "rule": rule,
        }))
    }
}

impl SystemModule for ReservationCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureReservation::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureReservation::new().commands()
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
            "reserve" | "release" | "confirm" | "expire" => Err(Self::retired_write(command)),
            "list" => {
                let unvalidated: system_core::Unvalidated<ReservationListInput> =
                    payload.try_into()?;
                self.list(ctx, unvalidated.sanitize().validate()?.into_inner())
            }
            "get" => {
                let unvalidated: system_core::Unvalidated<ReservationGetInput> =
                    payload.try_into()?;
                self.get(ctx, unvalidated.sanitize().validate()?.into_inner())
            }
            "check_availability" => {
                let unvalidated: system_core::Unvalidated<CheckAvailabilityInput> =
                    payload.try_into()?;
                self.check_availability(ctx, unvalidated.sanitize().validate()?.into_inner())
            }
            "rule_get" => {
                let unvalidated: system_core::Unvalidated<RuleGetInput> = payload.try_into()?;
                self.rule_get(ctx, unvalidated.sanitize().validate()?.into_inner())
            }
            "rule_update" => {
                let unvalidated: system_core::Unvalidated<RuleUpdateInput> = payload.try_into()?;
                self.rule_update(ctx, unvalidated.sanitize().validate()?.into_inner())
            }
            _ => Err(Self::error(
                "sys",
                "CMD_UNKNOWN",
                format!("Unknown command: {command}"),
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureReservation::new().schema()
    }
}

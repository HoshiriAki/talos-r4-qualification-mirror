use std::sync::Arc;

use chrono::NaiveDate;
use serde_json::Value;
use system_admin::booking::{AvailabilityInput, DeviceSearchInput, EstimateInput, FeatureBooking};
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{BookingPriceProjection, RepositoryError, RepositoryProvider};

#[derive(Clone)]
pub(crate) struct BookingCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl BookingCompatibilityModule {
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
            message: "booking read persistence unavailable".into(),
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

    fn availability(
        &self,
        ctx: &ExecutionContext,
        input: AvailabilityInput,
    ) -> Result<Value, String> {
        let start = parse_date(&input.start_date, "开始日期")?;
        let end = parse_date(&input.end_date, "结束日期")?;
        let scoped = self.scoped(ctx)?;
        let result = scoped
            .booking_reads()
            .availability(input.device_serial_no.as_deref(), start, end)
            .map_err(Self::repository_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "dates": result.dates,
            "devices": result.devices,
        }))
    }

    fn estimate(&self, ctx: &ExecutionContext, input: EstimateInput) -> Result<Value, String> {
        let start = parse_date(&input.start_date, "开始日期")?;
        let end = parse_date(&input.end_date, "结束日期")?;
        let days = (end - start).num_days().max(1);

        let scoped = self.scoped(ctx)?;
        let price = scoped
            .booking_reads()
            .price(&input.device_serial_no)
            .map_err(Self::repository_error)?
            .unwrap_or(BookingPriceProjection {
                model_name: String::new(),
                weekday_price: 8.5,
                weekend_price: 14.0,
            });

        let mut total_rate = 0.0;
        let mut current = start;
        let mut weekday_count = 0_i64;
        let mut weekend_count = 0_i64;
        while current <= end {
            let day = current.format("%u").to_string().parse::<u32>().unwrap_or(1);
            if day >= 5 {
                total_rate += price.weekend_price;
                weekend_count += 1;
            } else {
                total_rate += price.weekday_price;
                weekday_count += 1;
            }
            current += chrono::Duration::days(1);
        }

        let daily_rate = total_rate / days as f64;
        let subtotal = total_rate;
        let deposit = subtotal;
        let total = subtotal + deposit;

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": input.device_serial_no,
            "deviceModel": price.model_name,
            "startDate": input.start_date,
            "endDate": input.end_date,
            "days": days,
            "weekdayCount": weekday_count,
            "weekendCount": weekend_count,
            "weekdayRate": price.weekday_price,
            "weekendRate": price.weekend_price,
            "dailyRate": round_money(daily_rate),
            "subtotal": round_money(subtotal),
            "deposit": round_money(deposit),
            "total": round_money(total),
        }))
    }

    fn device_search(
        &self,
        ctx: &ExecutionContext,
        input: DeviceSearchInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let mut items = scoped
            .booking_reads()
            .search(input.query.as_deref(), input.model.as_deref())
            .map_err(Self::repository_error)?;
        if let Some(min_price) = input.min_price {
            items.retain(|item| item.daily_rate >= min_price);
        }
        if let Some(max_price) = input.max_price {
            items.retain(|item| item.daily_rate <= max_price);
        }
        Ok(serde_json::json!({
            "ok": true,
            "total": items.len(),
            "items": items,
        }))
    }
}

impl SystemModule for BookingCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureBooking::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureBooking::new().commands()
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
            "availability" => {
                let unvalidated: system_core::Unvalidated<AvailabilityInput> =
                    payload.try_into()?;
                self.availability(ctx, unvalidated.sanitize().validate()?.into_inner())
            }
            "estimate" => {
                let unvalidated: system_core::Unvalidated<EstimateInput> = payload.try_into()?;
                self.estimate(ctx, unvalidated.sanitize().validate()?.into_inner())
            }
            "device_search" => {
                let unvalidated: system_core::Unvalidated<DeviceSearchInput> =
                    payload.try_into()?;
                self.device_search(ctx, unvalidated.sanitize().validate()?.into_inner())
            }
            "reserve" => Err(Self::error(
                "biz",
                "BIZ_LEGACY_BOOKING_WRITE_RETIRED",
                "Use reservation_v2.create_legacy_hold".into(),
            )),
            "confirm" => Err(Self::error(
                "biz",
                "BIZ_LEGACY_BOOKING_WRITE_RETIRED",
                "Use reservation_v2.confirm_reservation".into(),
            )),
            _ => Err(Self::error(
                "sys",
                "CMD_UNKNOWN",
                format!("Unknown command: {command}"),
            )),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureBooking::new().schema()
    }
}

fn parse_date(value: &str, label: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|error| {
        BookingCompatibilityModule::error("val", "VAL_INVALID", format!("{label}格式无效: {error}"))
    })
}

fn round_money(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

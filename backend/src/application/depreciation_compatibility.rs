use std::sync::Arc;

use official_device::depreciation::{
    CalculateDepreciationInput, FeatureDepreciation, GetDepreciationInput,
    RunMonthlyDepreciationInput,
};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{DepreciationMutationError, RepositoryError, RepositoryProvider};
use crate::utils::time::shanghai_now_iso;

const DEFAULT_USEFUL_LIFE_MONTHS: i32 = 36;

#[derive(Clone)]
pub(crate) struct DepreciationCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl DepreciationCompatibilityModule {
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

    fn parse<T: serde::de::DeserializeOwned>(payload: Value) -> Result<T, String> {
        serde_json::from_value(payload).map_err(|error| format!("VAL_DESERIALIZE: {error}"))
    }

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "depreciation persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: DepreciationMutationError, period: &str) -> String {
        let payload = match error {
            DepreciationMutationError::AlreadyRun(count) => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DEPRECIATION_ALREADY_RUN".into(),
                message: format!("{period} 月份折旧已计提 ({count} 条记录)"),
                field: Some("period".into()),
                context: None,
            },
            DepreciationMutationError::Storage(error) => ErrorPayload {
                category: "sys".into(),
                code: error.code().into(),
                message: "depreciation persistence unavailable".into(),
                field: None,
                context: None,
            },
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn asset_not_found(device_serial_no: &str) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "biz".into(),
            code: "BIZ_ASSET_NOT_FOUND".into(),
            message: format!("设备 {device_serial_no} 没有采购记录"),
            field: Some("deviceSerialNo".into()),
            context: None,
        })
        .unwrap_or_default()
    }

    fn calculate(
        &self,
        ctx: &ExecutionContext,
        input: CalculateDepreciationInput,
    ) -> Result<Value, String> {
        let useful_life_months = input
            .useful_life_months
            .unwrap_or(DEFAULT_USEFUL_LIFE_MONTHS);
        let useful_life = f64::from(useful_life_months);
        let scoped = self.scoped(ctx)?;
        let snapshot = scoped
            .depreciations()
            .snapshot(&input.device_serial_no)
            .map_err(Self::repository_error)?
            .ok_or_else(|| Self::asset_not_found(&input.device_serial_no))?;

        let net_book_value = (snapshot.purchase_price - snapshot.total_depreciation).max(0.0);
        let monthly_depreciation = snapshot.purchase_price / useful_life;

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": input.device_serial_no,
            "purchasePrice": snapshot.purchase_price,
            "totalDepreciation": snapshot.total_depreciation,
            "netBookValue": net_book_value,
            "monthlyDepreciation": monthly_depreciation,
            "usefulLifeMonths": useful_life_months,
            "depreciationMonths": snapshot.depreciation_months,
            "remainingMonths": (useful_life_months - i32::try_from(snapshot.depreciation_months).unwrap_or(i32::MAX)).max(0),
        }))
    }

    fn run_monthly(
        &self,
        ctx: &ExecutionContext,
        input: RunMonthlyDepreciationInput,
    ) -> Result<Value, String> {
        let useful_life_months = input
            .useful_life_months
            .unwrap_or(DEFAULT_USEFUL_LIFE_MONTHS);
        let period = current_period();
        let scoped = self.scoped(ctx)?;
        let result = scoped
            .depreciations()
            .run_monthly(&period, useful_life_months, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &period))?;

        Ok(serde_json::json!({
            "ok": true,
            "period": result.period,
            "devicesProcessed": result.devices_processed,
            "totalDevices": result.total_devices,
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: GetDepreciationInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let log = scoped
            .depreciations()
            .logs(&input.device_serial_no)
            .map_err(Self::repository_error)?;
        let total_depreciation = log
            .iter()
            .map(|entry| entry.depreciation_amount)
            .sum::<f64>();
        let months = log.len();

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": input.device_serial_no,
            "depreciationLog": log,
            "summary": {
                "totalDepreciation": total_depreciation,
                "months": months,
            },
        }))
    }
}

impl SystemModule for DepreciationCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureDepreciation::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureDepreciation::new().commands()
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
            "calculate" => self.calculate(ctx, Self::parse(payload)?),
            "run_monthly" => self.run_monthly(ctx, Self::parse(payload)?),
            "get" => self.get(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: depreciation.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureDepreciation::new().schema()
    }
}

fn current_period() -> String {
    use chrono::Datelike;

    let now = chrono::Utc::now().with_timezone(&chrono_tz::Asia::Shanghai);
    format!("{:04}-{:02}", now.year(), now.month())
}

#[cfg(test)]
mod tests {
    use super::DEFAULT_USEFUL_LIFE_MONTHS;

    #[test]
    fn default_useful_life_matches_legacy_contract() {
        assert_eq!(DEFAULT_USEFUL_LIFE_MONTHS, 36);
    }
}

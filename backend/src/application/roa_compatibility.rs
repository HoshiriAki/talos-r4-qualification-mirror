use std::sync::Arc;

use chrono::Datelike;
use official_device::roa::{CalculateRoaInput, FeatureRoa};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{RepositoryError, RepositoryProvider, RoaBasisProjection};

#[derive(Clone)]
pub(crate) struct RoaCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl RoaCompatibilityModule {
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
            message: "ROA persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn asset_not_found(device_serial_no: &str) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "biz".into(),
            code: "BIZ_ASSET_NOT_FOUND".into(),
            message: format!("设备 {device_serial_no} 没有采购记录，无法计算 ROA"),
            field: Some("deviceSerialNo".into()),
            context: None,
        })
        .unwrap_or_default()
    }

    fn zero_price(device_serial_no: &str) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "biz".into(),
            code: "BIZ_ASSET_ZERO_PRICE".into(),
            message: format!("设备 {device_serial_no} 采购价格为 0，无法计算 ROA"),
            field: Some("deviceSerialNo".into()),
            context: None,
        })
        .unwrap_or_default()
    }

    fn calculate(&self, ctx: &ExecutionContext, input: CalculateRoaInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let basis = scoped
            .roa()
            .get(&input.device_serial_no)
            .map_err(Self::repository_error)?
            .ok_or_else(|| Self::asset_not_found(&input.device_serial_no))?;

        if basis.purchase_price <= 0.0 {
            return Err(Self::zero_price(&input.device_serial_no));
        }

        let metrics = metrics(&basis);
        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": basis.device_serial_no,
            "purchasePrice": basis.purchase_price,
            "totalRevenue": basis.total_revenue,
            "annualRevenue": metrics.annual_revenue,
            "yearsInService": metrics.years_in_service,
            "roa": metrics.roa,
            "roaPercent": metrics.roa * 100.0,
            "assessment": assessment(metrics.roa * 100.0),
        }))
    }

    fn list(&self, ctx: &ExecutionContext) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let mut devices = scoped
            .roa()
            .list()
            .map_err(Self::repository_error)?
            .into_iter()
            .map(|basis| {
                let metrics = metrics(&basis);
                serde_json::json!({
                    "deviceSerialNo": basis.device_serial_no,
                    "purchasePrice": basis.purchase_price,
                    "totalRevenue": basis.total_revenue,
                    "annualRevenue": metrics.annual_revenue,
                    "yearsInService": metrics.years_in_service,
                    "roa": metrics.roa,
                    "roaPercent": metrics.roa * 100.0,
                })
            })
            .collect::<Vec<_>>();

        devices.sort_by(|left, right| {
            right["roa"]
                .as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&left["roa"].as_f64().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(serde_json::json!({
            "ok": true,
            "total": devices.len(),
            "devices": devices,
        }))
    }
}

impl SystemModule for RoaCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureRoa::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureRoa::new().commands()
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
            "list" => self.list(ctx),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: roa.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureRoa::new().schema()
    }
}

struct RoaMetrics {
    annual_revenue: f64,
    years_in_service: f64,
    roa: f64,
}

fn metrics(basis: &RoaBasisProjection) -> RoaMetrics {
    let years_in_service = years_in_service(basis.first_order_date.as_deref());
    let annual_revenue = if years_in_service > 0.0 {
        basis.total_revenue / years_in_service
    } else {
        basis.total_revenue
    };
    let roa = if basis.purchase_price > 0.0 {
        annual_revenue / basis.purchase_price
    } else {
        0.0
    };
    RoaMetrics {
        annual_revenue,
        years_in_service,
        roa,
    }
}

fn years_in_service(first_order_date: Option<&str>) -> f64 {
    let Some(date_str) = first_order_date else {
        return 0.0;
    };
    if date_str.len() < 10 {
        return 0.0;
    }

    let year: i32 = date_str[0..4].parse().unwrap_or(0);
    let month: u32 = date_str[5..7].parse().unwrap_or(1);
    let day: u32 = date_str[8..10].parse().unwrap_or(1);
    let offset = chrono::FixedOffset::east_opt(8 * 3600).expect("valid Shanghai offset");
    let today = chrono::Utc::now().with_timezone(&offset);
    let days = (today.year() - year) as f64 * 365.25
        + (today.month() as f64 - month as f64) * 30.44
        + (today.day() as f64 - day as f64);
    (days / 365.25).max(0.083)
}

fn assessment(roa_percent: f64) -> &'static str {
    if roa_percent >= 100.0 {
        "优秀 — 设备已回收全部成本并盈利"
    } else if roa_percent >= 50.0 {
        "良好 — 设备预计在 2 年内回收成本"
    } else if roa_percent >= 20.0 {
        "一般 — 设备预计在 5 年内回收成本"
    } else if roa_percent > 0.0 {
        "偏低 — 设备回本周期较长"
    } else {
        "无数据 — 设备尚未产生收入"
    }
}

#[cfg(test)]
mod tests {
    use super::{assessment, metrics};
    use crate::repositories::RoaBasisProjection;

    #[test]
    fn zero_revenue_preserves_legacy_no_data_assessment() {
        let basis = RoaBasisProjection {
            device_serial_no: "ROA-1".into(),
            purchase_price: 100.0,
            total_revenue: 0.0,
            first_order_date: None,
        };
        let metrics = metrics(&basis);
        assert_eq!(metrics.annual_revenue, 0.0);
        assert_eq!(metrics.roa, 0.0);
        assert_eq!(assessment(0.0), "无数据 — 设备尚未产生收入");
    }
}

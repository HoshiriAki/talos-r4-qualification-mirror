use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{Datelike, NaiveDate};
use official_order::pricing::{
    DYNAMIC_ROLLING_DAYS, DailyPriceEntry, DeleteDynamicPriceInput, DynamicPriceDeleteResult,
    DynamicPriceEntry, DynamicPriceUpsertResult, EstimatePricingInput, FeaturePricing, HolidayRule,
    PricingConfig, PricingEstimate, SavePricingConfigInput, SavePricingConfigResult,
    UpdatePricingConfigInput, UpsertDynamicPriceInput, WarehouseRoute,
};
use serde::Serialize;
use serde_json::Value;
use system_core::*;

use crate::repositories::{
    DynamicPriceRecord, PricingConfigRecord, RepositoryError, RepositoryProvider,
    ScopedRepositories,
};
use crate::utils::time::{shanghai_now_date_key, shanghai_now_iso};

const DEFAULT_BASE_WEEKDAY_PRICE: f64 = 8.5;
const DEFAULT_BASE_WEEKEND_PRICE: f64 = 14.0;
const DEFAULT_RECEIVE_SHIPPING_FEES_JSON: &str = r#"{"area1":7,"area4":18,"area2":7,"area3":7}"#;
const DEFAULT_REC_AREA_KEYS: &[&str] = &["area1", "area2", "area3", "area4"];
const OCCUPANCY_COEFF_SHIPPING: f64 = 0.2;
const OCCUPANCY_COEFF_NORMAL: f64 = 1.0;
const OCCUPANCY_COEFF_RETURN: f64 = 0.2;

#[derive(Clone)]
pub(crate) struct PricingCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
    logistics_module: Option<Arc<dyn SystemModule>>,
    warehouse_routing_module: Option<Arc<dyn SystemModule>>,
}

impl PricingCompatibilityModule {
    pub(crate) fn new(
        repository_provider: Arc<dyn RepositoryProvider>,
        logistics_module: Option<Arc<dyn SystemModule>>,
        warehouse_routing_module: Option<Arc<dyn SystemModule>>,
    ) -> Self {
        Self {
            repository_provider,
            logistics_module,
            warehouse_routing_module,
        }
    }

    fn scoped(&self, ctx: &ExecutionContext) -> Result<ScopedRepositories, String> {
        self.repository_provider
            .bind(ctx)
            .map_err(Self::repository_error)
    }

    fn repository_error(error: RepositoryError) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "sys".into(),
            code: error.code().into(),
            message: "pricing persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn call_logistics(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match &self.logistics_module {
            Some(module) => module.execute(command, payload, ctx),
            None => Ok(Value::Null),
        }
    }

    fn call_warehouse_routing(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match &self.warehouse_routing_module {
            Some(module) => module.execute(command, payload, ctx),
            None => Ok(Value::Null),
        }
    }

    fn load_config(&self, scoped: &ScopedRepositories) -> Result<PricingConfig, String> {
        let now = shanghai_now_iso();
        let record = scoped
            .pricing()
            .get_or_create_config(
                DEFAULT_BASE_WEEKDAY_PRICE,
                DEFAULT_BASE_WEEKEND_PRICE,
                DEFAULT_RECEIVE_SHIPPING_FEES_JSON,
                &now,
            )
            .map_err(Self::repository_error)?;
        let dynamic = scoped
            .pricing()
            .list_dynamic_prices()
            .map_err(Self::repository_error)?;
        Ok(config_from_records(record, dynamic))
    }

    fn get_pricing_config(&self, ctx: &ExecutionContext) -> Result<PricingConfig, String> {
        let scoped = self.scoped(ctx)?;
        self.load_config(&scoped)
    }

    fn update_pricing_config(
        &self,
        input: &UpdatePricingConfigInput,
        ctx: &ExecutionContext,
    ) -> Result<PricingConfig, String> {
        let scoped = self.scoped(ctx)?;
        let rules = normalized_rules(&input.holiday_rules);
        let rules_json = serde_json::to_string(&rules).map_err(serialize_error)?;
        let now = shanghai_now_iso();

        let record = scoped
            .pricing()
            .update_config(
                normalize_price(input.base_weekday_price),
                normalize_price(input.base_weekend_price),
                &rules_json,
                &input.updated_by,
                DEFAULT_RECEIVE_SHIPPING_FEES_JSON,
                &now,
            )
            .map_err(Self::repository_error)?;
        let dynamic = scoped
            .pricing()
            .list_dynamic_prices()
            .map_err(Self::repository_error)?;
        Ok(config_from_records(record, dynamic))
    }

    fn save_pricing_config(
        &self,
        input: &SavePricingConfigInput,
        ctx: &ExecutionContext,
    ) -> Result<SavePricingConfigResult, String> {
        let scoped = self.scoped(ctx)?;
        let rules = normalized_rules(&input.holiday_rules);
        let rules_json = serde_json::to_string(&rules).map_err(serialize_error)?;

        let defaults: HashMap<String, f64> =
            serde_json::from_str(DEFAULT_RECEIVE_SHIPPING_FEES_JSON).unwrap_or_default();
        let mut fees = HashMap::new();
        for area_key in DEFAULT_REC_AREA_KEYS {
            let value = input
                .receive_shipping_fees
                .as_ref()
                .and_then(|input_fees| input_fees.get(*area_key))
                .copied()
                .unwrap_or(*defaults.get(*area_key).unwrap_or(&0.0));
            fees.insert((*area_key).to_owned(), normalize_price(value));
        }
        let fees_json = serde_json::to_string(&fees).map_err(serialize_error)?;
        let now = shanghai_now_iso();

        let record = scoped
            .pricing()
            .save_config(
                normalize_price(input.base_weekday_price),
                normalize_price(input.base_weekend_price),
                &rules_json,
                &fees_json,
                &input.dynamic_price_map,
                &input.updated_by,
                &now,
            )
            .map_err(Self::repository_error)?;
        let dynamic = scoped
            .pricing()
            .list_dynamic_prices()
            .map_err(Self::repository_error)?;

        Ok(SavePricingConfigResult {
            success: true,
            config: config_from_records(record, dynamic),
        })
    }

    fn upsert_dynamic_price(
        &self,
        input: &UpsertDynamicPriceInput,
        ctx: &ExecutionContext,
    ) -> Result<DynamicPriceUpsertResult, String> {
        let scoped = self.scoped(ctx)?;
        scoped
            .pricing()
            .upsert_dynamic_price(
                &input.date_key,
                normalize_price(input.price),
                &input.updated_by,
                &shanghai_now_iso(),
            )
            .map_err(Self::repository_error)?;

        Ok(DynamicPriceUpsertResult {
            success: true,
            date_key: input.date_key.clone(),
            price: normalize_price(input.price),
        })
    }

    fn delete_dynamic_price(
        &self,
        input: &DeleteDynamicPriceInput,
        ctx: &ExecutionContext,
    ) -> Result<DynamicPriceDeleteResult, String> {
        let scoped = self.scoped(ctx)?;
        scoped
            .pricing()
            .delete_dynamic_price(&input.date_key)
            .map_err(Self::repository_error)?;

        Ok(DynamicPriceDeleteResult {
            success: true,
            date_key: input.date_key.clone(),
        })
    }

    fn get_dynamic_prices(&self, ctx: &ExecutionContext) -> Result<Vec<DynamicPriceEntry>, String> {
        let scoped = self.scoped(ctx)?;
        let entries = scoped
            .pricing()
            .list_dynamic_prices()
            .map_err(Self::repository_error)?;
        Ok(filter_dynamic_entries(entries))
    }

    fn estimate_pricing(
        &self,
        input: &EstimatePricingInput,
        ctx: &ExecutionContext,
    ) -> Result<PricingEstimate, String> {
        let scoped = self.scoped(ctx)?;
        let mut config = self.load_config(&scoped)?;

        if !input.model_id.is_empty()
            && let Some(model) = scoped
                .pricing()
                .get_model_base_price(&input.model_id)
                .map_err(Self::repository_error)?
        {
            config.base_weekday_price = model.weekday_price;
            config.base_weekend_price = model.weekend_price;
        }

        let mut route: Option<WarehouseRoute> = None;
        let mut shipping_days = 1_i64;
        let mut return_days = 1_i64;

        if !input.province.is_empty() {
            let route_result = self
                .call_warehouse_routing(
                    "resolve_warehouse",
                    serde_json::json!({"province": input.province}),
                    ctx,
                )
                .ok();

            if let Some(value) = route_result
                && !value.is_null()
                && let Ok(parsed) = serde_json::from_value::<WarehouseRoute>(value)
            {
                shipping_days = parsed.shipping_days.max(1);
                return_days = parsed.return_days.max(1);
                route = Some(parsed);
            }

            if route.is_none() {
                let logistics_result = self
                    .call_logistics(
                        "estimate_shipping",
                        serde_json::json!({
                            "province": input.province,
                            "warehouseName": "上海仓"
                        }),
                        ctx,
                    )
                    .ok();
                if let Some(value) = logistics_result
                    && !value.is_null()
                {
                    if let Some(days) = value.get("shippingDays").and_then(Value::as_i64) {
                        shipping_days = days.max(1);
                    }
                    if let Some(days) = value.get("returnDays").and_then(Value::as_i64) {
                        return_days = days.max(1);
                    }
                }
            }
        }

        let total_days = get_date_diff_in_days(&input.start_date, &input.end_date) + 1;
        let mut breakdown = Vec::with_capacity(total_days.max(0) as usize);
        let mut current = input.start_date.clone();
        let mut day_index = 0_i64;

        while normalize_date_str(&current).is_some() && day_index < total_days {
            let occupy_type = if day_index < shipping_days {
                "shipping"
            } else if day_index >= total_days - return_days {
                "return"
            } else {
                "normal"
            };
            let factor = match occupy_type {
                "shipping" => OCCUPANCY_COEFF_SHIPPING,
                "return" => OCCUPANCY_COEFF_RETURN,
                _ => OCCUPANCY_COEFF_NORMAL,
            };
            let (price, source) = resolve_daily_price(&current, &config);
            breakdown.push(DailyPriceEntry {
                date_key: current.clone(),
                final_daily_price: price,
                price_source: source,
                occupy_type: occupy_type.to_owned(),
                occupy_factor: factor,
                amount: normalize_price(price * factor),
            });

            current = match add_days_to_date_key(&current, 1) {
                Some(next) => next,
                None => break,
            };
            day_index += 1;
        }

        let total_price = normalize_price(breakdown.iter().map(|entry| entry.amount).sum());

        let shipping_estimate = if input.province.is_empty() {
            None
        } else {
            let warehouse_name = route
                .as_ref()
                .map(|value| value.send_warehouse_name.clone())
                .unwrap_or_else(|| "上海仓".to_owned());
            self.call_logistics(
                "estimate_shipping",
                serde_json::json!({
                    "province": input.province,
                    "warehouseName": warehouse_name
                }),
                ctx,
            )
            .ok()
        };

        let return_estimate = if input.province.is_empty() {
            None
        } else {
            let warehouse_name = route
                .as_ref()
                .map(|value| value.return_warehouse_name.clone())
                .unwrap_or_else(|| "上海仓".to_owned());
            self.call_logistics(
                "estimate_return",
                serde_json::json!({
                    "province": input.province,
                    "warehouseName": warehouse_name
                }),
                ctx,
            )
            .ok()
        };

        Ok(PricingEstimate {
            breakdown,
            total_price,
            route,
            shipping_estimate,
            return_estimate,
        })
    }

    fn get_model_pricing_config(
        &self,
        model_id: &str,
        ctx: &ExecutionContext,
    ) -> Result<PricingConfig, String> {
        let scoped = self.scoped(ctx)?;
        let mut config = self.load_config(&scoped)?;
        if !model_id.is_empty()
            && let Some(model) = scoped
                .pricing()
                .get_model_base_price(model_id)
                .map_err(Self::repository_error)?
        {
            config.base_weekday_price = model.weekday_price;
            config.base_weekend_price = model.weekend_price;
        }
        Ok(config)
    }
}

impl SystemModule for PricingCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeaturePricing::new().metadata()
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        FeaturePricing::new().commands()
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
        macro_rules! validated {
            ($ty:ty) => {{
                DeserializeGuard::default().check_raw(&payload)?;
                let unvalidated: Unvalidated<$ty> = payload.clone().try_into()?;
                unvalidated.sanitize().validate()?.into_inner()
            }};
        }

        match command {
            "get_pricing_config" => serialize(&self.get_pricing_config(ctx)?),
            "update_pricing_config" => {
                let input = validated!(UpdatePricingConfigInput);
                serialize(&self.update_pricing_config(&input, ctx)?)
            }
            "save_pricing_config" => {
                let input = validated!(SavePricingConfigInput);
                serialize(&self.save_pricing_config(&input, ctx)?)
            }
            "upsert_dynamic_price" => {
                let input = validated!(UpsertDynamicPriceInput);
                serialize(&self.upsert_dynamic_price(&input, ctx)?)
            }
            "delete_dynamic_price" => {
                let input = validated!(DeleteDynamicPriceInput);
                serialize(&self.delete_dynamic_price(&input, ctx)?)
            }
            "get_dynamic_prices" => serialize(&self.get_dynamic_prices(ctx)?),
            "estimate_pricing" => {
                let input = validated!(EstimatePricingInput);
                serialize(&self.estimate_pricing(&input, ctx)?)
            }
            "get_model_pricing_config" => {
                let model_id = payload.get("modelId").and_then(Value::as_str).unwrap_or("");
                serialize(&self.get_model_pricing_config(model_id, ctx)?)
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("未知命令: {command}"),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeaturePricing::new().schema()
    }
}

fn normalized_rules(input: &[official_order::pricing::HolidayRuleInput]) -> Vec<HolidayRule> {
    input
        .iter()
        .map(|rule| HolidayRule {
            name: rule.name.clone(),
            start_date: normalize_date_str(&rule.start_date).unwrap_or_default(),
            end_date: normalize_date_str(&rule.end_date).unwrap_or_default(),
            price: normalize_price(rule.price),
            include_previous_day: rule.include_previous_day,
        })
        .collect()
}

fn config_from_records(
    record: PricingConfigRecord,
    dynamic: Vec<DynamicPriceRecord>,
) -> PricingConfig {
    PricingConfig {
        base_weekday_price: record.base_weekday_price,
        base_weekend_price: record.base_weekend_price,
        holiday_rules: parse_holiday_rules_json(&record.holiday_rules_json),
        receive_shipping_fees: parse_receive_shipping_fees_json(&record.receive_shipping_fees_json),
        dynamic_price_map: filter_dynamic_entries(dynamic)
            .into_iter()
            .map(|entry| (entry.date_key, entry.price))
            .collect(),
        updated_by: record.updated_by,
        created_at: record.created_at,
        updated_at: record.updated_at,
    }
}

// TALOS-OPS-004: pricing rolling windows use the canonical application clock.
fn filter_dynamic_entries(entries: Vec<DynamicPriceRecord>) -> Vec<DynamicPriceEntry> {
    let rolling: HashSet<String> =
        get_rolling_date_keys(DYNAMIC_ROLLING_DAYS, &shanghai_now_date_key())
            .into_iter()
            .collect();

    entries
        .into_iter()
        .filter(|entry| rolling.contains(&entry.date_key) && is_positive_number(entry.price))
        .map(|entry| DynamicPriceEntry {
            date_key: entry.date_key,
            price: normalize_price(entry.price),
            updated_by: entry.updated_by,
            created_at: entry.created_at,
            updated_at: entry.updated_at,
        })
        .collect()
}

fn resolve_daily_price(date_key: &str, config: &PricingConfig) -> (f64, String) {
    if let Some(price) = config.dynamic_price_map.get(date_key) {
        return (*price, "dynamic".to_owned());
    }
    for rule in &config.holiday_rules {
        if rule.include_previous_day
            && let Some(previous) = add_days_to_date_key(&rule.start_date, -1)
            && date_key == previous
        {
            return (rule.price, "holiday".to_owned());
        }
        if rule.start_date.as_str() <= date_key && date_key <= rule.end_date.as_str() {
            return (rule.price, "holiday".to_owned());
        }
    }
    if is_weekend(date_key) {
        (config.base_weekend_price, "base_weekend".to_owned())
    } else {
        (config.base_weekday_price, "base_weekday".to_owned())
    }
}

fn normalize_price(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn is_positive_number(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn normalize_date_str(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.len() != 10 || &trimmed[4..5] != "-" || &trimmed[7..8] != "-" {
        return None;
    }
    let parts = trimmed.split('-').collect::<Vec<_>>();
    let year = parts.first()?.parse::<i32>().ok()?;
    let month = parts.get(1)?.parse::<u32>().ok()?;
    let day = parts.get(2)?.parse::<u32>().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)?;
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn add_days_to_date_key(date_key: &str, days: i64) -> Option<String> {
    let normalized = normalize_date_str(date_key)?;
    let date = NaiveDate::parse_from_str(&normalized, "%Y-%m-%d").ok()?;
    let shifted = date.checked_add_signed(chrono::Duration::days(days))?;
    Some(shifted.format("%Y-%m-%d").to_string())
}

fn get_date_diff_in_days(start: &str, end: &str) -> i64 {
    let start = NaiveDate::parse_from_str(start, "%Y-%m-%d").ok();
    let end = NaiveDate::parse_from_str(end, "%Y-%m-%d").ok();
    match (start, end) {
        (Some(start), Some(end)) => end.signed_duration_since(start).num_days(),
        _ => 0,
    }
}

fn is_weekend(date_key: &str) -> bool {
    NaiveDate::parse_from_str(date_key, "%Y-%m-%d")
        .map(|date| matches!(date.weekday().num_days_from_monday(), 4..=6))
        .unwrap_or(false)
}

fn get_rolling_date_keys(days: i64, from: &str) -> Vec<String> {
    (0..days.max(1))
        .filter_map(|offset| add_days_to_date_key(from, offset))
        .collect()
}

fn parse_holiday_rules_json(text: &str) -> Vec<HolidayRule> {
    if text.is_empty() || text == "[]" {
        return Vec::new();
    }
    serde_json::from_str(text).unwrap_or_default()
}

fn parse_receive_shipping_fees_json(text: &str) -> HashMap<String, f64> {
    if text.is_empty() {
        return serde_json::from_str(DEFAULT_RECEIVE_SHIPPING_FEES_JSON).unwrap_or_default();
    }
    serde_json::from_str(text).unwrap_or_else(|_| {
        serde_json::from_str(DEFAULT_RECEIVE_SHIPPING_FEES_JSON).unwrap_or_default()
    })
}

fn serialize<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(serialize_error)
}

fn serialize_error(error: serde_json::Error) -> String {
    serde_json::to_string(&ErrorPayload {
        category: "sys".into(),
        code: "SYS_SERIALIZE".into(),
        message: error.to_string(),
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_priority_remains_dynamic_then_holiday_then_base() {
        let mut config = PricingConfig {
            base_weekday_price: 10.0,
            base_weekend_price: 20.0,
            holiday_rules: vec![HolidayRule {
                name: "holiday".into(),
                start_date: "2026-10-05".into(),
                end_date: "2026-10-06".into(),
                price: 30.0,
                include_previous_day: true,
            }],
            receive_shipping_fees: HashMap::new(),
            dynamic_price_map: HashMap::from([("2026-10-05".into(), 40.0)]),
            updated_by: String::new(),
            created_at: "now".into(),
            updated_at: "now".into(),
        };

        assert_eq!(resolve_daily_price("2026-10-05", &config).0, 40.0);
        config.dynamic_price_map.clear();
        assert_eq!(resolve_daily_price("2026-10-05", &config).0, 30.0);
        assert_eq!(resolve_daily_price("2026-10-04", &config).0, 30.0);
        assert_eq!(resolve_daily_price("2026-10-07", &config).0, 10.0);
    }
}

use std::sync::Arc;

use chrono::Datelike;
use official_finance::settlement::{
    ConfirmSettlementInput, ExportSettlementInput, FeatureSettlement, GenerateSettlementInput,
    GetSettlementInput, ListSettlementsInput,
};
use serde_json::Value;
use system_core::{ErrorPayload, ExecutionContext, ModuleMetadata, ModuleSchema, SystemModule};

use crate::repositories::{RepositoryError, RepositoryProvider, SettlementMutationError};
use crate::utils::time::{shanghai_now_date_key, shanghai_now_iso};

#[derive(Clone)]
pub(crate) struct SettlementCompatibilityModule {
    repository_provider: Arc<dyn RepositoryProvider>,
}

impl SettlementCompatibilityModule {
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
            message: "settlement persistence unavailable".into(),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn mutation_error(error: SettlementMutationError, settlement_id: &str) -> String {
        let payload = match error {
            SettlementMutationError::NotFound => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_SETTLEMENT_NOT_FOUND".into(),
                message: format!("结算单 {settlement_id} 不存在"),
                field: Some("settlementId".into()),
                context: None,
            },
            SettlementMutationError::AlreadyConfirmed => ErrorPayload {
                category: "biz".into(),
                code: "BIZ_SETTLEMENT_ALREADY_CONFIRMED".into(),
                message: "结算单已确认，不可重复操作".into(),
                field: Some("settlementId".into()),
                context: None,
            },
            SettlementMutationError::Storage(error) => return Self::repository_error(error),
        };
        serde_json::to_string(&payload).unwrap_or_default()
    }

    fn generate(
        &self,
        ctx: &ExecutionContext,
        input: GenerateSettlementInput,
    ) -> Result<Value, String> {
        let period_key = resolve_period_key(&input.period_type, &input.period_key)?;
        let (date_start, date_end) = period_bounds(&period_key)?;
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .settlements()
            .generate(
                &input.period_type,
                &period_key,
                &date_start,
                &date_end,
                &shanghai_now_iso(),
            )
            .map_err(|error| Self::mutation_error(error, ""))?;

        if outcome.existing && outcome.confirmed {
            return Ok(serde_json::json!({
                "ok": true,
                "settlementId": outcome.settlement_id,
                "periodType": outcome.period_type,
                "periodKey": outcome.period_key,
                "existing": true,
                "confirmed": true,
            }));
        }

        Ok(serde_json::json!({
            "ok": true,
            "settlementId": outcome.settlement_id,
            "periodType": outcome.period_type,
            "periodKey": outcome.period_key,
            "totalRevenue": outcome.total_revenue.unwrap_or(0.0),
            "totalDeposits": outcome.total_deposits.unwrap_or(0.0),
            "totalRefunds": outcome.total_refunds.unwrap_or(0.0),
            "confirmed": false,
        }))
    }

    fn confirm(
        &self,
        ctx: &ExecutionContext,
        input: ConfirmSettlementInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let outcome = scoped
            .settlements()
            .confirm(&input.settlement_id, &shanghai_now_iso())
            .map_err(|error| Self::mutation_error(error, &input.settlement_id))?;

        Ok(serde_json::json!({
            "ok": true,
            "settlementId": outcome.settlement_id,
            "periodType": outcome.period_type,
            "periodKey": outcome.period_key,
            "confirmed": true,
        }))
    }

    fn get(&self, ctx: &ExecutionContext, input: GetSettlementInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let settlement = scoped
            .settlements()
            .get_by_period(&input.period_type, &input.period_key)
            .map_err(Self::repository_error)?;
        Ok(serde_json::json!({
            "ok": true,
            "settlement": settlement,
        }))
    }

    fn list(&self, ctx: &ExecutionContext, input: ListSettlementsInput) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let result = scoped
            .settlements()
            .list(
                input.period_type.as_deref(),
                input.confirmed,
                page,
                page_size,
            )
            .map_err(Self::repository_error)?;

        Ok(serde_json::json!({
            "ok": true,
            "settlements": result.settlements,
            "pagination": {
                "page": result.page,
                "pageSize": result.page_size,
                "total": result.total,
            },
        }))
    }

    fn export(
        &self,
        ctx: &ExecutionContext,
        input: ExportSettlementInput,
    ) -> Result<Value, String> {
        let scoped = self.scoped(ctx)?;
        let settlement = scoped
            .settlements()
            .get_by_id(&input.settlement_id)
            .map_err(Self::repository_error)?
            .ok_or_else(|| format!("BIZ_SETTLEMENT_NOT_FOUND: {}", input.settlement_id))?;
        let (date_start, date_end) = export_date_bounds(&settlement.period_key)?;
        let revenue_rows = scoped
            .settlements()
            .revenue_details(&date_start, &date_end)
            .map_err(Self::repository_error)?;

        let mut csv = String::new();
        csv.push_str("结算单ID,期间类型,期间键,总收入,押金合计,退款合计,已确认\n");
        let period_label = match settlement.period_type.as_str() {
            "daily" => "日结",
            "weekly" => "周结",
            "monthly" => "月结",
            _ => &settlement.period_type,
        };
        csv.push_str(&format!(
            "{},{},{},{:.2},{:.2},{:.2},{}\n",
            settlement.id,
            period_label,
            settlement.period_key,
            settlement.total_revenue,
            settlement.total_deposits,
            settlement.total_refunds,
            if settlement.confirmed { "是" } else { "否" },
        ));
        csv.push_str("\n收入明细\n");
        csv.push_str("订单ID,金额,确认日期,来源\n");
        for row in revenue_rows {
            csv.push_str(&format!(
                "{},{:.2},{},{}\n",
                row.order_id, row.amount, row.recognition_date, row.source
            ));
        }

        Ok(serde_json::json!({
            "ok": true,
            "csv": csv,
        }))
    }
}

impl SystemModule for SettlementCompatibilityModule {
    fn metadata(&self) -> ModuleMetadata {
        FeatureSettlement::new().metadata()
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        FeatureSettlement::new().commands()
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
            "generate" => self.generate(ctx, Self::parse(payload)?),
            "confirm" => self.confirm(ctx, Self::parse(payload)?),
            "get" => self.get(ctx, Self::parse(payload)?),
            "list" => self.list(ctx, Self::parse(payload)?),
            "export" => self.export(ctx, Self::parse(payload)?),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: settlement.{command}")),
        }
    }

    fn schema(&self) -> ModuleSchema {
        FeatureSettlement::new().schema()
    }
}

fn resolve_period_key(period_type: &str, input_key: &str) -> Result<String, String> {
    if !input_key.is_empty() {
        return Ok(input_key.to_owned());
    }
    let today = shanghai_now_date_key();
    match period_type {
        "daily" => Ok(today),
        "weekly" => iso_week_key(&today),
        "monthly" => Ok(today.get(..7).unwrap_or(&today).to_owned()),
        _ => Err(format!("VAL_INVALID_PERIOD_TYPE: {period_type}")),
    }
}

fn iso_week_key(date: &str) -> Result<String, String> {
    let date = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|error| format!("VAL_INVALID_DATE: {error}"))?;
    let iso = date.iso_week();
    Ok(format!("{}-W{:02}", iso.year(), iso.week()))
}

fn period_bounds(period_key: &str) -> Result<(String, String), String> {
    if period_key.len() == 10 {
        return Ok((
            format!("{period_key}T00:00:00"),
            format!("{period_key}T23:59:59"),
        ));
    }
    if period_key.contains("-W") {
        let (year, week) = parse_week(period_key)?;
        let monday = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)
            .ok_or_else(|| format!("VAL_INVALID_PERIOD_KEY: {period_key}"))?;
        let sunday = monday + chrono::Duration::days(6);
        return Ok((
            format!("{}T00:00:00", monday.format("%Y-%m-%d")),
            format!("{}T23:59:59", sunday.format("%Y-%m-%d")),
        ));
    }

    let first = chrono::NaiveDate::parse_from_str(&format!("{period_key}-01"), "%Y-%m-%d")
        .map_err(|error| format!("VAL_INVALID_PERIOD_KEY: {error}"))?;
    let last = first
        .checked_add_months(chrono::Months::new(1))
        .map(|next| next - chrono::Duration::days(1))
        .unwrap_or_else(|| first + chrono::Duration::days(30));
    Ok((
        format!("{}T00:00:00", first.format("%Y-%m-%d")),
        format!("{}T23:59:59", last.format("%Y-%m-%d")),
    ))
}

fn export_date_bounds(period_key: &str) -> Result<(String, String), String> {
    if period_key.len() == 10 {
        return Ok((period_key.to_owned(), period_key.to_owned()));
    }
    if period_key.contains("-W") {
        let (year, week) = parse_week(period_key)?;
        let monday = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)
            .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap());
        let sunday = monday + chrono::Duration::days(6);
        return Ok((
            monday.format("%Y-%m-%d").to_string(),
            sunday.format("%Y-%m-%d").to_string(),
        ));
    }
    Ok((format!("{period_key}-01"), format!("{period_key}-28")))
}

fn parse_week(period_key: &str) -> Result<(i32, u32), String> {
    let parts: Vec<&str> = period_key.split("-W").collect();
    if parts.len() != 2 {
        return Err(format!("VAL_INVALID_PERIOD_KEY: {period_key}"));
    }
    let year = parts[0]
        .parse()
        .map_err(|_| format!("VAL_INVALID_PERIOD_KEY: {period_key}"))?;
    let week = parts[1]
        .parse()
        .map_err(|_| format!("VAL_INVALID_PERIOD_KEY: {period_key}"))?;
    Ok((year, week))
}

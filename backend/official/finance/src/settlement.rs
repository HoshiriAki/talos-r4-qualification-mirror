//! feature-settlement — 结算单子模块
//!
//! 命令:
//! - generate   — 自动生成日/周/月结算单 (从 orders + deposits + refunds 聚合)
//! - confirm    — 确认结算单
//! - get        — 按 period 查询结算单
//! - list       — 分页查询结算列表
//! - export_csv — 导出结算数据为 CSV

use chrono::Datelike;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OptionalExtension;
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use uuid::Uuid;

use system_core::*;

// ═══════════════════════════════════════════════════════════════════
// 时间辅助
// ═══════════════════════════════════════════════════════════════════

fn shanghai_now_iso() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y-%m-%dT%H:%M:%S%.3f+08:00").to_string()
}

fn shanghai_today() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y-%m-%d").to_string()
}

fn iso_week_key(date: &str) -> Result<String, String> {
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| format!("VAL_INVALID_DATE: {}", e))?;
    let iso = d.iso_week();
    Ok(format!("{}-W{:02}", iso.year(), iso.week()))
}

fn month_key(date: &str) -> &str {
    // date is YYYY-MM-DD, return YYYY-MM
    if date.len() >= 7 { &date[..7] } else { date }
}

// ═══════════════════════════════════════════════════════════════════
// 输入类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateSettlementInput {
    pub period_type: String, // daily, weekly, monthly
    #[serde(default)]
    pub period_key: String, // YYYY-MM-DD | YYYY-WW | YYYY-MM — 不传则用今天
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmSettlementInput {
    pub settlement_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetSettlementInput {
    pub period_type: String,
    pub period_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListSettlementsInput {
    #[serde(default)]
    pub period_type: Option<String>,
    #[serde(default)]
    pub confirmed: Option<bool>,
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportSettlementInput {
    pub settlement_id: String,
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureSettlement {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureSettlement {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureSettlement {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: settlement pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    fn resolve_period_key(&self, period_type: &str, input_key: &str) -> Result<String, String> {
        if !input_key.is_empty() {
            return Ok(input_key.to_string());
        }
        let today = shanghai_today();
        match period_type {
            "daily" => Ok(today),
            "weekly" => iso_week_key(&today),
            "monthly" => Ok(month_key(&today).to_string()),
            _ => Err(format!("VAL_INVALID_PERIOD_TYPE: {}", period_type)),
        }
    }

    /// Aggregate from accounting_entries + deposits + revenue_records
    fn aggregate_for_period(
        &self,
        conn: &r2d2::PooledConnection<SqliteConnectionManager>,
        period_key: &str,
        scope: &DataScope,
    ) -> Result<(f64, f64, f64), String> {
        // Compute date range based on period_type
        let (date_start, date_end) = if period_key.len() == 10 {
            // daily: YYYY-MM-DD
            (
                format!("{}T00:00:00", period_key),
                format!("{}T23:59:59", period_key),
            )
        } else if period_key.contains("-W") {
            // weekly: YYYY-Www — approximate: parse and get Mon/Sun
            let parts: Vec<&str> = period_key.split("-W").collect();
            if parts.len() != 2 {
                return Err(format!("VAL_INVALID_PERIOD_KEY: {}", period_key));
            }
            let year: i32 = parts[0]
                .parse()
                .map_err(|_| format!("VAL_INVALID_PERIOD_KEY: {}", period_key))?;
            let week: u32 = parts[1]
                .parse()
                .map_err(|_| format!("VAL_INVALID_PERIOD_KEY: {}", period_key))?;
            let monday = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)
                .ok_or_else(|| format!("VAL_INVALID_PERIOD_KEY: {}", period_key))?;
            let sunday = monday + chrono::Duration::days(6);
            (
                format!("{}T00:00:00", monday.format("%Y-%m-%d")),
                format!("{}T23:59:59", sunday.format("%Y-%m-%d")),
            )
        } else {
            // monthly: YYYY-MM
            let date_str = format!("{}-01", period_key);
            let first = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                .map_err(|e| format!("VAL_INVALID_PERIOD_KEY: {}", e))?;
            let last = if let Some(next) = first.checked_add_months(chrono::Months::new(1)) {
                next - chrono::Duration::days(1)
            } else {
                first + chrono::Duration::days(30)
            };
            (
                format!("{}T00:00:00", first.format("%Y-%m-%d")),
                format!("{}T23:59:59", last.format("%Y-%m-%d")),
            )
        };

        // Total revenue from revenue_records in period
        let total_revenue: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM revenue_records WHERE recognition_date >= ?1 AND recognition_date <= ?2 AND tenant_id = ?3",
                params![&date_start[..10], &date_end[..10], scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        // Total deposits collected in period (from deposit_ledger)
        let total_deposits: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM deposit_ledger WHERE entry_type = 'collect' AND created_at >= ?1 AND created_at <= ?2 AND tenant_id = ?3",
                params![date_start, date_end, scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        // Total refunds in period
        let total_refunds: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM deposit_ledger WHERE entry_type IN ('release', 'refund') AND created_at >= ?1 AND created_at <= ?2 AND tenant_id = ?3",
                params![date_start, date_end, scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        Ok((total_revenue, total_deposits, total_refunds))
    }

    fn do_generate(
        &self,
        input: &GenerateSettlementInput,
        scope: &DataScope,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let period_key = self.resolve_period_key(&input.period_type, &input.period_key)?;

        // Check for existing
        let existing: Option<(String, i64)> = conn
            .query_row(
                "SELECT id, confirmed FROM settlements WHERE period_type = ?1 AND period_key = ?2 AND tenant_id = ?3",
                params![input.period_type, period_key, scope.tenant_id().as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        if let Some((sid, confirmed)) = existing {
            if confirmed != 0 {
                return Ok(serde_json::json!({
                    "ok": true,
                    "settlementId": sid,
                    "periodType": input.period_type,
                    "periodKey": period_key,
                    "existing": true,
                    "confirmed": true,
                }));
            }
            // Existing unconfirmed — re-aggregate
            let (rev, dep, refd) = self.aggregate_for_period(&conn, &period_key, scope)?;
            conn.execute(
                "UPDATE settlements SET total_revenue = ?1, total_deposits = ?2, total_refunds = ?3, created_at = ?4 WHERE id = ?5 AND tenant_id = ?6",
                params![rev, dep, refd, now, sid, scope.tenant_id().as_str()],
            ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

            return Ok(serde_json::json!({
                "ok": true,
                "settlementId": sid,
                "periodType": input.period_type,
                "periodKey": period_key,
                "totalRevenue": rev,
                "totalDeposits": dep,
                "totalRefunds": refd,
                "confirmed": false,
            }));
        }

        let (total_revenue, total_deposits, total_refunds) =
            self.aggregate_for_period(&conn, &period_key, scope)?;

        let settlement_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO settlements (id, period_type, period_key, total_revenue, total_deposits, total_refunds, confirmed, tenant_id, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)",
            params![settlement_id, input.period_type, period_key, total_revenue, total_deposits, total_refunds, scope.tenant_id().as_str(), now],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "settlementId": settlement_id,
            "periodType": input.period_type,
            "periodKey": period_key,
            "totalRevenue": total_revenue,
            "totalDeposits": total_deposits,
            "totalRefunds": total_refunds,
            "confirmed": false,
        }))
    }

    fn do_confirm(
        &self,
        input: &ConfirmSettlementInput,
        scope: &DataScope,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let (period_type, period_key, confirmed): (String, String, i64) = conn
            .query_row(
                "SELECT period_type, period_key, confirmed FROM settlements WHERE id = ?1 AND tenant_id = ?2",
                params![input.settlement_id, scope.tenant_id().as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|_| serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_SETTLEMENT_NOT_FOUND".into(),
                message: format!("结算单 {} 不存在", input.settlement_id),
                field: Some("settlementId".into()),
                context: None,
            }).unwrap_or_default())?;

        if confirmed != 0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_SETTLEMENT_ALREADY_CONFIRMED".into(),
                message: "结算单已确认，不可重复操作".into(),
                field: Some("settlementId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute(
            "UPDATE settlements SET confirmed = 1, confirmed_at = ?1 WHERE id = ?2 AND tenant_id = ?3",
            params![now, input.settlement_id, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "settlementId": input.settlement_id,
            "periodType": period_type,
            "periodKey": period_key,
            "confirmed": true,
        }))
    }

    fn do_get(&self, input: &GetSettlementInput, scope: &DataScope) -> Result<Value, String> {
        let conn = self.get_conn()?;

        let settlement: Option<Value> = conn
            .query_row(
                "SELECT id, period_type, period_key, total_revenue, total_deposits, total_refunds, confirmed, confirmed_at, tenant_id, created_at \
                 FROM settlements WHERE period_type = ?1 AND period_key = ?2 AND tenant_id = ?3",
                params![input.period_type, input.period_key, scope.tenant_id().as_str()],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "periodType": row.get::<_, String>(1)?,
                        "periodKey": row.get::<_, String>(2)?,
                        "totalRevenue": row.get::<_, f64>(3)?,
                        "totalDeposits": row.get::<_, f64>(4)?,
                        "totalRefunds": row.get::<_, f64>(5)?,
                        "confirmed": row.get::<_, i64>(6)? != 0,
                        "confirmedAt": row.get::<_, Option<String>>(7)?,
                        "tenantId": row.get::<_, String>(8)?,
                        "createdAt": row.get::<_, String>(9)?,
                    }))
                },
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        Ok(serde_json::json!({ "ok": true, "settlement": settlement }))
    }

    fn do_list(&self, input: &ListSettlementsInput, scope: &DataScope) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let offset = (page - 1) * page_size;

        let mut conditions: Vec<String> = vec!["tenant_id = ?1".into()];
        let mut param_values: Vec<String> = vec![scope.tenant_id().as_str().to_string()];

        if let Some(ref pt) = input.period_type
            && !pt.is_empty()
        {
            param_values.push(pt.clone());
            conditions.push(format!("period_type = ?{}", param_values.len()));
        }
        if let Some(confirmed) = input.confirmed {
            param_values.push(if confirmed {
                "1".to_string()
            } else {
                "0".to_string()
            });
            conditions.push(format!("confirmed = ?{}", param_values.len()));
        }

        let where_clause = conditions.join(" AND ");

        let count_sql = format!("SELECT COUNT(*) FROM settlements WHERE {}", where_clause);
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();

        let total: i64 = conn
            .query_row(&count_sql, params_ref.as_slice(), |row| row.get(0))
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let data_sql = format!(
            "SELECT id, period_type, period_key, total_revenue, total_deposits, total_refunds, confirmed, confirmed_at, tenant_id, created_at \
             FROM settlements WHERE {} ORDER BY period_key DESC LIMIT {} OFFSET {}",
            where_clause, page_size, offset,
        );
        let data_params: Vec<&dyn rusqlite::types::ToSql> = param_values
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();

        let mut stmt = conn
            .prepare(&data_sql)
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;
        let rows: Vec<Value> = stmt
            .query_map(data_params.as_slice(), |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "periodType": row.get::<_, String>(1)?,
                    "periodKey": row.get::<_, String>(2)?,
                    "totalRevenue": row.get::<_, f64>(3)?,
                    "totalDeposits": row.get::<_, f64>(4)?,
                    "totalRefunds": row.get::<_, f64>(5)?,
                    "confirmed": row.get::<_, i64>(6)? != 0,
                    "confirmedAt": row.get::<_, Option<String>>(7)?,
                    "tenantId": row.get::<_, String>(8)?,
                    "createdAt": row.get::<_, String>(9)?,
                }))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!({
            "ok": true,
            "settlements": rows,
            "pagination": {
                "page": page,
                "pageSize": page_size,
                "total": total,
            },
        }))
    }

    fn do_export_csv(
        &self,
        input: &ExportSettlementInput,
        scope: &DataScope,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;

        let (period_type, period_key, total_revenue, total_deposits, total_refunds, confirmed): (String, String, f64, f64, f64, i64) = conn
            .query_row(
                "SELECT period_type, period_key, total_revenue, total_deposits, total_refunds, confirmed FROM settlements WHERE id = ?1 AND tenant_id = ?2",
                params![input.settlement_id, scope.tenant_id().as_str()],
                |row| Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?,
                )),
            )
            .map_err(|_| format!("BIZ_SETTLEMENT_NOT_FOUND: {}", input.settlement_id))?;

        let mut csv = String::new();
        csv.push_str("结算单ID,期间类型,期间键,总收入,押金合计,退款合计,已确认\n");
        csv.push_str(&format!(
            "{},{},{},{:.2},{:.2},{:.2},{}\n",
            input.settlement_id,
            match period_type.as_str() {
                "daily" => "日结",
                "weekly" => "周结",
                "monthly" => "月结",
                _ => &period_type,
            },
            period_key,
            total_revenue,
            total_deposits,
            total_refunds,
            if confirmed != 0 { "是" } else { "否" },
        ));

        // Revenue detail
        csv.push_str("\n收入明细\n");
        csv.push_str("订单ID,金额,确认日期,来源\n");

        let (date_start, date_end) = if period_key.len() == 10 {
            (period_key.to_string(), period_key.to_string())
        } else if period_key.contains("-W") {
            // Weekly: approximate
            let parts: Vec<&str> = period_key.split("-W").collect();
            let year: i32 = parts[0].parse().unwrap_or(2024);
            let week: u32 = parts[1].parse().unwrap_or(1);
            let monday = chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon)
                .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap());
            let sunday = monday + chrono::Duration::days(6);
            (
                monday.format("%Y-%m-%d").to_string(),
                sunday.format("%Y-%m-%d").to_string(),
            )
        } else {
            (format!("{}-01", period_key), format!("{}-28", period_key))
        };

        let mut stmt = conn.prepare(
            "SELECT order_id, amount, recognition_date, source FROM revenue_records WHERE recognition_date >= ?1 AND recognition_date <= ?2 AND tenant_id = ?3 ORDER BY recognition_date ASC"
        ).map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let revenue_rows: Vec<(String, f64, String, String)> = stmt
            .query_map(
                params![date_start, date_end, scope.tenant_id().as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        for (oid, amount, date, source) in &revenue_rows {
            csv.push_str(&format!("{},{:.2},{},{}\n", oid, amount, date, source));
        }

        Ok(serde_json::json!({
            "ok": true,
            "csv": csv,
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════
// SystemModule trait
// ═══════════════════════════════════════════════════════════════════

impl SystemModule for FeatureSettlement {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "settlement".into(),
            version: "0.1.0".into(),
            description: "结算管理 — 自动生成日/周/月结算单 + 收入确认(权责发生制)".into(),
            author: "talos".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "generate",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "confirm",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "list",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "export",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
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
            "generate" => {
                let input: GenerateSettlementInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_generate(&input, ctx.data_scope())
            }
            "confirm" => {
                let input: ConfirmSettlementInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_confirm(&input, ctx.data_scope())
            }
            "get" => {
                let input: GetSettlementInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_get(&input, ctx.data_scope())
            }
            "list" => {
                let input: ListSettlementsInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_list(&input, ctx.data_scope())
            }
            "export" => {
                let input: ExportSettlementInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_export_csv(&input, ctx.data_scope())
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: settlement.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "settlement".into(),
            description: "结算管理 — 自动生成日/周/月结算单 + 收入确认(权责发生制)".into(),
            commands: vec![
                CommandSchema {
                    name: "generate".into(),
                    description: "自动生成结算单 (从 orders/deposits/refunds 聚合)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        GenerateSettlementInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "confirm".into(),
                    description: "确认结算单".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        ConfirmSettlementInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get".into(),
                    description: "按 period 查询结算单".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetSettlementInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "list".into(),
                    description: "分页查询结算列表".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ListSettlementsInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "export".into(),
                    description: "导出结算数据为 CSV".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        ExportSettlementInput
                    ))
                    .ok(),
                    output_schema: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregation_is_limited_to_the_data_scope_tenant() {
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::builder()
            .max_size(1)
            .build(manager)
            .expect("memory pool");
        let feature = FeatureSettlement {
            pool: Mutex::new(Some(pool)),
        };
        let conn = feature.get_conn().expect("connection");
        conn.execute_batch(
            "CREATE TABLE revenue_records (amount REAL NOT NULL, recognition_date TEXT NOT NULL, tenant_id TEXT NOT NULL);\
             CREATE TABLE deposit_ledger (amount REAL NOT NULL, entry_type TEXT NOT NULL, created_at TEXT NOT NULL, tenant_id TEXT NOT NULL);\
             INSERT INTO revenue_records VALUES (100.0, '2026-07-16', 'tenant-a');\
             INSERT INTO revenue_records VALUES (900.0, '2026-07-16', 'tenant-b');",
        ).expect("finance tables");

        let scope = DataScope::production(
            TenantId::new("tenant-a").expect("tenant"),
            Revision::new("test-revision").expect("revision"),
        )
        .expect("scope");

        let (revenue, _, _) = feature
            .aggregate_for_period(&conn, "2026-07-16", &scope)
            .expect("aggregate");
        assert_eq!(revenue, 100.0);
    }
}

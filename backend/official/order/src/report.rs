//! 营收报表模块 (storage-bound — 连接池集成)
//!
//! 提供营收报表的聚合查询和 Excel 导出。
//! 命令:
//! - get_revenue_data: 按日期范围查询营收汇总、省份分布、月度趋势
//! - export_excel: 生成 3-sheet Excel workbook
//!
//! 使用 r2d2 连接池 (SQLite)。独立运行时通过 init() 传入 {"databaseUrl": ":memory:"} 等配置。

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use rust_xlsxwriter::{Format, Workbook};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use system_core::*;

// ── 输入类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetRevenueDataInput {
    #[serde(default)]
    pub start_date: String,
    #[serde(default)]
    pub end_date: String,
}

impl Validate for GetRevenueDataInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        // start_date 和 end_date 为空时视为无限制，不报错
        if !self.start_date.is_empty() && self.start_date.len() != 10 {
            errors.push(FieldError {
                field: "startDate".into(),
                message: "开始日期格式无效，应为 YYYY-MM-DD".into(),
                code: "VAL_DATE_FORMAT".into(),
            });
        }
        if !self.end_date.is_empty() && self.end_date.len() != 10 {
            errors.push(FieldError {
                field: "endDate".into(),
                message: "结束日期格式无效，应为 YYYY-MM-DD".into(),
                code: "VAL_DATE_FORMAT".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for GetRevenueDataInput {
    fn sanitize(&mut self) {
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportExcelInput {
    #[serde(default)]
    pub start_date: String,
    #[serde(default)]
    pub end_date: String,
    #[serde(default = "default_label")]
    pub label: String,
}

fn default_label() -> String {
    "营收报表".into()
}

impl Validate for ExportExcelInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if !self.start_date.is_empty() && self.start_date.len() != 10 {
            errors.push(FieldError {
                field: "startDate".into(),
                message: "开始日期格式无效，应为 YYYY-MM-DD".into(),
                code: "VAL_DATE_FORMAT".into(),
            });
        }
        if !self.end_date.is_empty() && self.end_date.len() != 10 {
            errors.push(FieldError {
                field: "endDate".into(),
                message: "结束日期格式无效，应为 YYYY-MM-DD".into(),
                code: "VAL_DATE_FORMAT".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for ExportExcelInput {
    fn sanitize(&mut self) {
        self.start_date = self.start_date.trim().to_string();
        self.end_date = self.end_date.trim().to_string();
        if self.label.trim().is_empty() {
            self.label = default_label();
        } else {
            self.label = self.label.trim().to_string();
        }
    }
}

// ── 领域/输出类型 ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevenueSummary {
    pub total_orders: i64,
    pub total_revenue: f64,
    pub avg_order_value: f64,
    pub devices_rented: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProvinceRevenue {
    pub province: String,
    pub order_count: i64,
    pub revenue: f64,
    pub avg_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonthlyRevenue {
    pub month: String,
    pub order_count: i64,
    pub revenue: f64,
    pub avg_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevenueData {
    pub summary: RevenueSummary,
    pub by_province: Vec<ProvinceRevenue>,
    pub by_month: Vec<MonthlyRevenue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportExcelOutput {
    pub file_name: String,
    /// Base64 编码的 xlsx 内容
    pub content: String,
    pub mime_type: String,
}

// ── 模块主体 ──

/// 营收报表模块
///
/// 通过 r2d2 连接池访问 SQLite 数据库。
/// init() 接受 `{"databaseUrl": ":memory:"}` 或实际文件路径。
pub struct FeatureReport {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl FeatureReport {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    /// 从连接池获取连接
    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_LOCK".into(),
                message: format!("连接池锁错误: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        guard
            .as_ref()
            .ok_or_else(|| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_NOT_INIT".into(),
                    message: "数据库未初始化".into(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?
            .get()
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_POOL".into(),
                    message: format!("连接池获取失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })
    }

    fn build_where_clause(
        scope: &DataScope,
        start_date: &str,
        end_date: &str,
    ) -> (String, Vec<String>) {
        let mut clauses: Vec<&str> = vec!["o.tenant_id = ?"];
        let mut values: Vec<String> = vec![scope.tenant_id().as_str().to_owned()];

        if !start_date.is_empty() {
            clauses.push("o.startDate >= ?");
            values.push(start_date.to_string());
        }
        if !end_date.is_empty() {
            clauses.push("o.startDate <= ?");
            values.push(end_date.to_string());
        }

        let where_sql = format!("WHERE {}", clauses.join(" AND "));

        (where_sql, values)
    }

    fn params_from(values: &[String]) -> Vec<&dyn rusqlite::types::ToSql> {
        values
            .iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect()
    }

    fn do_get_revenue_data(
        &self,
        conn: &Connection,
        scope: &DataScope,
        start_date: &str,
        end_date: &str,
    ) -> Result<RevenueData, String> {
        let start = start_date.trim();
        let end = end_date.trim();
        let (where_clause, values) = Self::build_where_clause(scope, start, end);
        let param_refs = Self::params_from(&values);

        // Summary
        let summary_sql = format!(
            "SELECT COUNT(*), COALESCE(SUM(o.totalPrice), 0), COALESCE(AVG(o.totalPrice), 0) FROM orders o {}",
            where_clause
        );
        let param_refs_slice: Vec<&dyn rusqlite::types::ToSql> = param_refs.to_vec();
        let (total_orders, total_revenue, avg_order_value): (i64, f64, f64) = conn
            .query_row(&summary_sql, param_refs_slice.as_slice(), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("汇总查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

        // Device count
        let device_sql = format!(
            "SELECT COUNT(DISTINCT od.serialNo) FROM order_devices od JOIN orders o ON o.id = od.orderId AND o.tenant_id = od.tenant_id {}",
            where_clause
        );
        let param_refs2 = Self::params_from(&values);
        let param_refs2_slice: Vec<&dyn rusqlite::types::ToSql> = param_refs2.to_vec();
        let devices_rented: i64 = conn
            .query_row(&device_sql, param_refs2_slice.as_slice(), |row| row.get(0))
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("设备数量查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;

        let summary = RevenueSummary {
            total_orders,
            total_revenue,
            avg_order_value,
            devices_rented,
        };

        // By province
        let province_sql = format!(
            "SELECT COALESCE(o.province, '未知'), COUNT(*), COALESCE(SUM(o.totalPrice), 0), COALESCE(AVG(o.totalPrice), 0) \
             FROM orders o {} GROUP BY o.province ORDER BY SUM(o.totalPrice) DESC",
            where_clause
        );
        let param_refs3 = Self::params_from(&values);
        let param_refs3_slice: Vec<&dyn rusqlite::types::ToSql> = param_refs3.to_vec();
        let mut stmt = conn.prepare(&province_sql).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_PREPARE".into(),
                message: format!("省份查询准备失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        let by_province: Vec<ProvinceRevenue> = stmt
            .query_map(param_refs3_slice.as_slice(), |row| {
                Ok(ProvinceRevenue {
                    province: row.get(0)?,
                    order_count: row.get(1)?,
                    revenue: row.get(2)?,
                    avg_value: row.get(3)?,
                })
            })
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("省份查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?
            .filter_map(|r| r.ok())
            .collect();

        // By month
        let month_sql = format!(
            "SELECT substr(o.startDate, 1, 7), COUNT(*), COALESCE(SUM(o.totalPrice), 0), COALESCE(AVG(o.totalPrice), 0) \
             FROM orders o {} GROUP BY substr(o.startDate, 1, 7) ORDER BY substr(o.startDate, 1, 7) ASC",
            where_clause
        );
        let param_refs4 = Self::params_from(&values);
        let param_refs4_slice: Vec<&dyn rusqlite::types::ToSql> = param_refs4.to_vec();
        let mut stmt = conn.prepare(&month_sql).map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_DB_PREPARE".into(),
                message: format!("月度查询准备失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        let by_month: Vec<MonthlyRevenue> = stmt
            .query_map(param_refs4_slice.as_slice(), |row| {
                Ok(MonthlyRevenue {
                    month: row.get(0)?,
                    order_count: row.get(1)?,
                    revenue: row.get(2)?,
                    avg_value: row.get(3)?,
                })
            })
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_QUERY".into(),
                    message: format!("月度查询失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(RevenueData {
            summary,
            by_province,
            by_month,
        })
    }

    fn format_report_file_name(label: &str) -> String {
        let ts = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
        format!("{}-{}.xlsx", label, ts)
    }

    pub fn build_excel_bytes(data: &RevenueData) -> Result<Vec<u8>, String> {
        let mut workbook = Workbook::new();
        let header_fmt = Format::new().set_bold();

        // Sheet 1: Summary
        let ws1 = workbook.add_worksheet();
        ws1.set_name("营收概览").map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_XLSX".into(),
                message: format!("设置工作表名称失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        ws1.write_string_with_format(0, 0, "指标", &header_fmt).ok();
        ws1.write_string_with_format(0, 1, "数值", &header_fmt).ok();
        ws1.write_string(1, 0, "总订单数").ok();
        ws1.write_number(1, 1, data.summary.total_orders as f64)
            .ok();
        ws1.write_string(2, 0, "总营收 (元)").ok();
        ws1.write_number(2, 1, data.summary.total_revenue).ok();
        ws1.write_string(3, 0, "平均订单金额 (元)").ok();
        ws1.write_number(3, 1, data.summary.avg_order_value).ok();
        ws1.write_string(4, 0, "租出设备数").ok();
        ws1.write_number(4, 1, data.summary.devices_rented as f64)
            .ok();
        ws1.set_column_width(0, 20).ok();
        ws1.set_column_width(1, 15).ok();

        // Sheet 2: By Province
        let ws2 = workbook.add_worksheet();
        ws2.set_name("省份分布").map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_XLSX".into(),
                message: format!("设置工作表名称失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        ws2.write_string_with_format(0, 0, "省份", &header_fmt).ok();
        ws2.write_string_with_format(0, 1, "订单数", &header_fmt)
            .ok();
        ws2.write_string_with_format(0, 2, "营收 (元)", &header_fmt)
            .ok();
        ws2.write_string_with_format(0, 3, "平均订单金额 (元)", &header_fmt)
            .ok();
        for (i, p) in data.by_province.iter().enumerate() {
            let r = (i + 1) as u32;
            ws2.write_string(r, 0, &p.province).ok();
            ws2.write_number(r, 1, p.order_count as f64).ok();
            ws2.write_number(r, 2, p.revenue).ok();
            ws2.write_number(r, 3, p.avg_value).ok();
        }
        ws2.set_column_width(0, 16).ok();
        ws2.set_column_width(1, 12).ok();
        ws2.set_column_width(2, 14).ok();
        ws2.set_column_width(3, 18).ok();

        // Sheet 3: By Month
        let ws3 = workbook.add_worksheet();
        ws3.set_name("月度趋势").map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_XLSX".into(),
                message: format!("设置工作表名称失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;
        ws3.write_string_with_format(0, 0, "月份", &header_fmt).ok();
        ws3.write_string_with_format(0, 1, "订单数", &header_fmt)
            .ok();
        ws3.write_string_with_format(0, 2, "营收 (元)", &header_fmt)
            .ok();
        ws3.write_string_with_format(0, 3, "平均订单金额 (元)", &header_fmt)
            .ok();
        for (i, m) in data.by_month.iter().enumerate() {
            let r = (i + 1) as u32;
            ws3.write_string(r, 0, &m.month).ok();
            ws3.write_number(r, 1, m.order_count as f64).ok();
            ws3.write_number(r, 2, m.revenue).ok();
            ws3.write_number(r, 3, m.avg_value).ok();
        }
        ws3.set_column_width(0, 12).ok();
        ws3.set_column_width(1, 12).ok();
        ws3.set_column_width(2, 14).ok();
        ws3.set_column_width(3, 18).ok();

        let buffer = workbook.save_to_buffer().map_err(|e| {
            serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_XLSX".into(),
                message: format!("保存 workbook 失败: {}", e),
                field: None,
                context: None,
            })
            .unwrap_or_default()
        })?;

        Ok(buffer)
    }

    pub fn build_excel(data: &RevenueData, label: &str) -> Result<ExportExcelOutput, String> {
        let buffer = Self::build_excel_bytes(data)?;
        let file_name = Self::format_report_file_name(label);
        let content = base64_encode(&buffer);

        Ok(ExportExcelOutput {
            file_name,
            content,
            mime_type: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
        })
    }
}

impl Default for FeatureReport {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemModule for FeatureReport {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "report".into(),
            version: "0.1.0".into(),
            description: "营收报表模块 — 按省份/月度聚合查询 + Excel 导出".into(),
            author: "hoshi".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "get_revenue_data",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "export_excel",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        if let Some(db_url) = config.get("databaseUrl").and_then(|v| v.as_str()) {
            let manager = SqliteConnectionManager::file(db_url);
            let pool = Pool::builder().max_size(5).build(manager).map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_DB_POOL".into(),
                    message: format!("创建连接池失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;
            let mut guard = self.pool.lock().map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "sys".into(),
                    code: "SYS_LOCK".into(),
                    message: format!("锁获取失败: {}", e),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })?;
            *guard = Some(pool);
        }
        Ok(())
    }

    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        match command {
            "get_revenue_data" => {
                DeserializeGuard::default().check_raw(&payload)?;

                let unvalidated: Unvalidated<GetRevenueDataInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                // 从 config 中获取 databaseUrl（在独立运行时传入）
                // 由于 init() 已经设置 pool，这里从 pool 获取连接
                let conn = self.get_conn()?;
                let result = self.do_get_revenue_data(
                    &conn,
                    ctx.data_scope(),
                    &input.start_date,
                    &input.end_date,
                )?;

                serde_json::to_value(result).map_err(|e| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })
            }
            "export_excel" => {
                DeserializeGuard::default().check_raw(&payload)?;

                let unvalidated: Unvalidated<ExportExcelInput> = payload.try_into()?;
                let sanitized = unvalidated.sanitize();
                let validated = sanitized.validate()?;
                let input = validated.into_inner();

                let conn = self.get_conn()?;
                let data = self.do_get_revenue_data(
                    &conn,
                    ctx.data_scope(),
                    &input.start_date,
                    &input.end_date,
                )?;
                let result = Self::build_excel(&data, &input.label)?;

                serde_json::to_value(result).map_err(|e| {
                    serde_json::to_string(&ErrorPayload {
                        category: "sys".into(),
                        code: "SYS_SERIALIZE".into(),
                        message: e.to_string(),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default()
                })
            }
            _ => Err(serde_json::to_string(&ErrorPayload {
                category: "sys".into(),
                code: "SYS_UNKNOWN_COMMAND".into(),
                message: format!("未知命令: {}", command),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "report".into(),
            description: "营收报表模块 — 按省份/月度聚合查询 + Excel 导出".into(),
            commands: vec![
                CommandSchema {
                    name: "get_revenue_data".into(),
                    description: "按日期范围查询营收汇总、省份分布、月度趋势".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "export_excel".into(),
                    description: "生成 3-sheet Excel workbook (营收概览/省份分布/月度趋势)".into(),
                    version: "1.0.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

// ── 辅助函数 ──

fn base64_encode(bytes: &[u8]) -> String {
    // 使用标准 base64 编码
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;

    #[test]
    fn revenue_report_is_limited_to_data_scope() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE orders (id TEXT, startDate TEXT, totalPrice REAL, province TEXT, tenant_id TEXT);
             CREATE TABLE order_devices (orderId TEXT, serialNo TEXT, tenant_id TEXT);
             INSERT INTO orders VALUES ('a', '2026-07-01', 100, 'Shanghai', 'tenant-a');
             INSERT INTO orders VALUES ('b', '2026-07-01', 900, 'Beijing', 'tenant-b');
             INSERT INTO order_devices VALUES ('a', 'A-1', 'tenant-a');
             INSERT INTO order_devices VALUES ('b', 'B-1', 'tenant-b');",
        ).unwrap();
        let scope = DataScope::production(
            TenantId::new("tenant-a").unwrap(),
            Revision::new("report-test").unwrap(),
        )
        .unwrap();

        let result = FeatureReport::new()
            .do_get_revenue_data(&conn, &scope, "", "")
            .unwrap();

        assert_eq!(result.summary.total_orders, 1);
        assert_eq!(result.summary.total_revenue, 100.0);
        assert_eq!(result.summary.devices_rented, 1);
        assert_eq!(result.by_province.len(), 1);
        assert_eq!(result.by_province[0].province, "Shanghai");
    }
}

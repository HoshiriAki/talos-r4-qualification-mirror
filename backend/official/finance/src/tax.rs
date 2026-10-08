//! feature-tax — 税务子模块
//!
//! 命令:
//! - get_config    — 查询活跃税率配置
//! - upsert_config — 创建/更新税率配置
//! - calculate     — 根据金额计算销项税额
//! - export_csv    — 导出税务申报辅助 CSV

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

// ═══════════════════════════════════════════════════════════════════
// 输入类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetTaxConfigInput {
    #[serde(default)]
    pub tax_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertTaxConfigInput {
    #[serde(default = "default_tax_type")]
    pub tax_type: String,
    pub rate: f64,
    #[serde(default)]
    pub effective_from: String,
}

fn default_tax_type() -> String {
    "vat".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalculateTaxInput {
    pub amount: f64, // 含税总额
    #[serde(default)]
    pub tax_rate: Option<f64>, // 不传则查活动税率
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportTaxCsvInput {
    #[serde(default)]
    pub period_key: String, // YYYY-MM or YYYY-MM-DD
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureTax {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureTax {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureTax {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: tax pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    fn do_get_config(&self, scope: &DataScope, input: &GetTaxConfigInput) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let tax_type = input.tax_type.as_deref().unwrap_or("vat");
        let tenant_id = scope.tenant_id().as_str();

        let configs: Vec<Value> = {
            let mut stmt = conn.prepare(
                "SELECT id, tax_type, rate, effective_from, is_active, created_at \
                 FROM tax_config WHERE tax_type = ?1 AND tenant_id = ?2 ORDER BY effective_from DESC"
            ).map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

            stmt.query_map(params![tax_type, tenant_id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "taxType": row.get::<_, String>(1)?,
                    "rate": row.get::<_, f64>(2)?,
                    "effectiveFrom": row.get::<_, String>(3)?,
                    "isActive": row.get::<_, i64>(4)? != 0,
                    "createdAt": row.get::<_, String>(5)?,
                }))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect()
        };

        Ok(serde_json::json!({
            "ok": true,
            "configs": configs,
        }))
    }

    fn do_upsert_config(
        &self,
        scope: &DataScope,
        input: &UpsertTaxConfigInput,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        if input.rate <= 0.0 || input.rate > 1.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_INVALID_TAX_RATE".into(),
                message: "税率必须在 0.01 ~ 1.00 之间".into(),
                field: Some("rate".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let effective_from = if input.effective_from.is_empty() {
            &now[..10]
        } else {
            &input.effective_from
        };

        // Deactivate all existing for this tax_type
        conn.execute(
            "UPDATE tax_config SET is_active = 0 WHERE tax_type = ?1 AND tenant_id = ?2",
            params![input.tax_type, tenant_id],
        )
        .map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        // Create new active config
        let config_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO tax_config (id, tax_type, rate, effective_from, is_active, created_at, tenant_id) \
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)",
            params![config_id, input.tax_type, input.rate, effective_from, now, tenant_id],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "configId": config_id,
            "taxType": input.tax_type,
            "rate": input.rate,
            "effectiveFrom": effective_from,
            "isActive": true,
        }))
    }

    fn do_calculate(&self, scope: &DataScope, input: &CalculateTaxInput) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let tenant_id = scope.tenant_id().as_str();

        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let tax_rate: f64 = if let Some(tr) = input.tax_rate {
            tr
        } else {
            conn.query_row(
                "SELECT rate FROM tax_config WHERE tax_type = 'vat' AND is_active = 1 AND tenant_id = ?1 ORDER BY effective_from DESC LIMIT 1",
                params![tenant_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .unwrap_or(0.13)
        };

        // 含税金额拆分为不含税 + 税额
        let pre_tax = (input.amount / (1.0 + tax_rate) * 100.0).round() / 100.0;
        let tax_amount = (input.amount - pre_tax).max(0.0);

        Ok(serde_json::json!({
            "ok": true,
            "totalAmount": input.amount,     // 含税总额
            "preTaxAmount": pre_tax,         // 不含税金额
            "taxAmount": tax_amount,         // 税额
            "taxRate": tax_rate,             // 适用税率
        }))
    }

    fn do_export_csv(&self, scope: &DataScope, input: &ExportTaxCsvInput) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let tenant_id = scope.tenant_id().as_str();

        let period = if input.period_key.is_empty() {
            let shanghai = chrono_tz::Asia::Shanghai;
            let now = chrono::Utc::now().with_timezone(&shanghai);
            now.format("%Y-%m").to_string()
        } else {
            input.period_key.clone()
        };

        let date_pattern = format!("{}%", period);

        let mut csv = String::new();
        csv.push_str("销项税额申报辅助表\n");
        csv.push_str(&format!("期间: {}\n\n", period));

        csv.push_str("发票号码,订单ID,票种,金额(含税),税率,税额,状态,开票日期\n");

        let mut stmt = conn.prepare(
            "SELECT invoice_no, order_id, type, amount, tax_rate, tax_amount, status, issued_at \
             FROM invoices WHERE issued_at LIKE ?1 AND tenant_id = ?2 ORDER BY issued_at ASC"
        ).map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, String, String, f64, f64, f64, String, String)> = stmt
            .query_map(params![date_pattern, tenant_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, f64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let mut total_tax = 0.0_f64;
        let mut total_amount = 0.0_f64;

        for (inv_no, oid, inv_type, amount, rate, tax, status, issued) in &rows {
            csv.push_str(&format!(
                "{},{},{},{:.2},{:.2}%,{:.2},{},{}\n",
                inv_no,
                oid,
                inv_type,
                amount,
                (rate * 100.0),
                tax,
                status,
                issued
            ));
            total_amount += amount;
            total_tax += tax;
        }

        csv.push('\n');
        csv.push_str(&format!(
            "合计,,,{:.2},,,{:.2},,\n",
            total_amount, total_tax
        ));

        // Active tax config summary
        csv.push_str("\n当前活跃税率配置\n");
        csv.push_str("税种,税率,生效日期\n");

        let mut stmt2 = conn.prepare(
            "SELECT tax_type, rate, effective_from FROM tax_config WHERE is_active = 1 AND tenant_id = ?1 ORDER BY tax_type ASC"
        ).map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let configs: Vec<(String, f64, String)> = stmt2
            .query_map(params![tenant_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        for (tax_type, rate, effective) in &configs {
            csv.push_str(&format!(
                "{},{:.2}%,{}\n",
                tax_type,
                rate * 100.0,
                effective
            ));
        }

        Ok(serde_json::json!({
            "ok": true,
            "csv": csv,
            "period": period,
            "invoiceCount": rows.len(),
            "totalTax": total_tax,
            "totalAmount": total_amount,
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════
// SystemModule trait
// ═══════════════════════════════════════════════════════════════════

impl SystemModule for FeatureTax {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "tax".into(),
            version: "0.1.0".into(),
            description: "税务管理 — 税率配置 + 销项税额计算 + 申报辅助 CSV 导出".into(),
            author: "talos".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "get_config",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "upsert_config",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "calculate",
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
            "get_config" => {
                let input: GetTaxConfigInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_get_config(ctx.data_scope(), &input)
            }
            "upsert_config" => {
                let input: UpsertTaxConfigInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_upsert_config(ctx.data_scope(), &input)
            }
            "calculate" => {
                let input: CalculateTaxInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_calculate(ctx.data_scope(), &input)
            }
            "export" => {
                let input: ExportTaxCsvInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_export_csv(ctx.data_scope(), &input)
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: tax.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "tax".into(),
            description: "税务管理 — 税率配置 + 销项税额计算 + 申报辅助 CSV 导出".into(),
            commands: vec![
                CommandSchema {
                    name: "get_config".into(),
                    description: "查询活跃税率配置".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetTaxConfigInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "upsert_config".into(),
                    description: "创建/更新税率配置 (自动停用旧税率)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(UpsertTaxConfigInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "calculate".into(),
                    description: "根据含税金额计算不含税金额 + 销项税额".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(CalculateTaxInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "export".into(),
                    description: "导出税务申报辅助 CSV (按期间汇总发票)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ExportTaxCsvInput))
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
    use std::sync::Arc;

    fn context(tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).unwrap();
        let scope = DataScope::production(tenant_id.clone(), Revision::new("r1").unwrap()).unwrap();
        ExecutionContext::new(
            ActorIdentity::authenticated("admin", "admin").unwrap(),
            TenantScope::tenant(tenant_id),
            scope,
            ExecutionMode::Normal,
            RequestId::new(format!("req-{tenant}")).unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn tax_configs_are_isolated_by_execution_context_data_scope() {
        let db_path = std::env::temp_dir().join(format!("talos-tax-{}.db", Uuid::new_v4()));
        let manager = SqliteConnectionManager::file(&db_path);
        let pool = Pool::new(manager).unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "CREATE TABLE tax_config (\
                 id TEXT PRIMARY KEY, tax_type TEXT NOT NULL, rate REAL NOT NULL, \
                 effective_from TEXT NOT NULL, is_active INTEGER NOT NULL, \
                 created_at TEXT NOT NULL, tenant_id TEXT NOT NULL\
             );",
        )
        .unwrap();
        drop(conn);

        let feature = FeatureTax {
            pool: Mutex::new(Some(pool)),
        };
        feature
            .execute(
                "upsert_config",
                serde_json::json!({"taxType":"vat","rate":0.06,"effectiveFrom":"2026-01-01"}),
                &context("tenant-a"),
            )
            .unwrap();
        feature
            .execute(
                "upsert_config",
                serde_json::json!({"taxType":"vat","rate":0.13,"effectiveFrom":"2026-01-01"}),
                &context("tenant-b"),
            )
            .unwrap();

        let tenant_a = feature
            .execute("get_config", serde_json::json!({}), &context("tenant-a"))
            .unwrap();
        let tenant_b_calculation = feature
            .execute(
                "calculate",
                serde_json::json!({"amount":113.0}),
                &context("tenant-b"),
            )
            .unwrap();

        assert_eq!(tenant_a["configs"].as_array().unwrap().len(), 1);
        assert_eq!(tenant_a["configs"][0]["rate"], 0.06);
        assert_eq!(tenant_b_calculation["taxRate"], 0.13);

        let _ = std::fs::remove_file(db_path);
    }
}

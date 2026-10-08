//! feature-depreciation — 设备折旧追踪子模块 (official/device)
//!
//! 命令:
//! - calculate    — 计算设备当前账面净值 (原值 - 累计折旧)
//! - run_monthly  — 对全部有采购记录的设备，自动计提当月折旧 (直线法)
//! - get          — 查询设备折旧日志

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use uuid::Uuid;

use system_core::*;

fn shanghai_now_iso() -> String {
    let offset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    chrono::Utc::now()
        .with_timezone(&offset)
        .format("%Y-%m-%dT%H:%M:%S%.3f+08:00")
        .to_string()
}

fn current_period() -> String {
    let offset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    chrono::Utc::now()
        .with_timezone(&offset)
        .format("%Y-%m")
        .to_string()
}

/// 默认使用年限：36 个月 (Pocket 3 相机)
const DEFAULT_USEFUL_LIFE_MONTHS: i32 = 36;

// ═══════════════════════════════════════════════════════════════════
// 输入类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalculateDepreciationInput {
    pub device_serial_no: String,
    #[serde(default)]
    pub useful_life_months: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunMonthlyDepreciationInput {
    #[serde(default)]
    pub useful_life_months: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDepreciationInput {
    pub device_serial_no: String,
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureDepreciation {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureDepreciation {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureDepreciation {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: depreciation pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    fn do_calculate(
        &self,
        scope: &DataScope,
        input: &CalculateDepreciationInput,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let useful_life = input
            .useful_life_months
            .unwrap_or(DEFAULT_USEFUL_LIFE_MONTHS) as f64;

        // Get purchase info
        let purchase: (f64,) = conn
            .query_row(
                "SELECT purchase_price FROM asset_purchases WHERE tenant_id = ?1 AND device_serial_no = ?2",
                params![scope.tenant_id().as_str(), input.device_serial_no],
                |row| Ok((row.get(0)?,)),
            )
            .map_err(|_| serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ASSET_NOT_FOUND".into(),
                message: format!("设备 {} 没有采购记录", input.device_serial_no),
                field: Some("deviceSerialNo".into()),
                context: None,
            }).unwrap_or_default())?;

        let purchase_price = purchase.0;

        // Sum accumulated depreciation
        let total_depreciation: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(depreciation_amount), 0) FROM depreciation_log WHERE tenant_id = ?1 AND device_serial_no = ?2",
                params![scope.tenant_id().as_str(), input.device_serial_no],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        let net_book_value = (purchase_price - total_depreciation).max(0.0);
        let monthly_depreciation = purchase_price / useful_life;
        let depreciation_months_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM depreciation_log WHERE tenant_id = ?1 AND device_serial_no = ?2",
                params![scope.tenant_id().as_str(), input.device_serial_no],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": input.device_serial_no,
            "purchasePrice": purchase_price,
            "totalDepreciation": total_depreciation,
            "netBookValue": net_book_value,
            "monthlyDepreciation": monthly_depreciation,
            "usefulLifeMonths": useful_life as i32,
            "depreciationMonths": depreciation_months_count,
            "remainingMonths": (useful_life as i32 - depreciation_months_count as i32).max(0),
        }))
    }

    fn do_run_monthly(
        &self,
        scope: &DataScope,
        input: &RunMonthlyDepreciationInput,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let period = current_period();
        let useful_life = input
            .useful_life_months
            .unwrap_or(DEFAULT_USEFUL_LIFE_MONTHS) as f64;

        // Check if this period has already been processed for all devices
        // We check by looking for any records in the current period
        let existing_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM depreciation_log WHERE tenant_id = ?1 AND period = ?2",
                params![scope.tenant_id().as_str(), period],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if existing_count > 0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DEPRECIATION_ALREADY_RUN".into(),
                message: format!("{} 月份折旧已计提 ({} 条记录)", period, existing_count),
                field: Some("period".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        // Get all devices with purchase records
        let mut stmt = conn
            .prepare(
                "SELECT device_serial_no, purchase_price FROM asset_purchases WHERE tenant_id = ?1",
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let devices: Vec<(String, f64)> = stmt
            .query_map(params![scope.tenant_id().as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let mut processed = 0;
        for (device_serial_no, purchase_price) in &devices {
            // Get current accumulated depreciation
            let total_dep: f64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(depreciation_amount), 0) FROM depreciation_log WHERE tenant_id = ?1 AND device_serial_no = ?2",
                    params![scope.tenant_id().as_str(), device_serial_no],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            let opening_value = (*purchase_price - total_dep).max(0.0);
            if opening_value <= 0.0 {
                continue; // Fully depreciated
            }

            let monthly_dep = *purchase_price / useful_life;
            let closing_value = (opening_value - monthly_dep).max(0.0);

            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO depreciation_log (id, device_serial_no, period, opening_value, \
                 depreciation_amount, closing_value, method, created_at, tenant_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'straight_line', ?7, ?8)",
                params![
                    id,
                    device_serial_no,
                    period,
                    opening_value,
                    monthly_dep,
                    closing_value,
                    now,
                    scope.tenant_id().as_str()
                ],
            )
            .map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

            processed += 1;
        }

        Ok(serde_json::json!({
            "ok": true,
            "period": period,
            "devicesProcessed": processed,
            "totalDevices": devices.len(),
        }))
    }

    fn do_get(&self, scope: &DataScope, input: &GetDepreciationInput) -> Result<Value, String> {
        let conn = self.get_conn()?;

        let mut stmt = conn.prepare(
            "SELECT id, device_serial_no, period, opening_value, depreciation_amount, \
                    closing_value, method, created_at \
             FROM depreciation_log WHERE tenant_id = ?1 AND device_serial_no = ?2 ORDER BY period DESC"
        ).map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let log: Vec<Value> = stmt
            .query_map(
                params![scope.tenant_id().as_str(), input.device_serial_no],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "deviceSerialNo": row.get::<_, String>(1)?,
                        "period": row.get::<_, String>(2)?,
                        "openingValue": row.get::<_, f64>(3)?,
                        "depreciationAmount": row.get::<_, f64>(4)?,
                        "closingValue": row.get::<_, f64>(5)?,
                        "method": row.get::<_, String>(6)?,
                        "createdAt": row.get::<_, String>(7)?,
                    }))
                },
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        // Also include summary
        let total_depreciation: f64 = log
            .iter()
            .map(|v| v["depreciationAmount"].as_f64().unwrap_or(0.0))
            .sum();
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

impl SystemModule for FeatureDepreciation {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "depreciation".into(),
            version: "0.1.0".into(),
            description: "设备折旧追踪 — 直线法月结计提".into(),
            author: "talos".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "calculate",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "run_monthly",
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
        let scope = ctx.data_scope();

        match command {
            "calculate" => {
                let input: CalculateDepreciationInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_calculate(scope, &input)
            }
            "run_monthly" => {
                let input: RunMonthlyDepreciationInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_run_monthly(scope, &input)
            }
            "get" => {
                let input: GetDepreciationInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_get(scope, &input)
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: depreciation.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "depreciation".into(),
            description: "设备折旧追踪 — 直线法月结计提".into(),
            commands: vec![
                CommandSchema {
                    name: "calculate".into(),
                    description: "计算设备当前净值".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        CalculateDepreciationInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "run_monthly".into(),
                    description: "月结批量计提折旧".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        RunMonthlyDepreciationInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get".into(),
                    description: "查询设备折旧日志".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetDepreciationInput))
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

    fn initialized_module() -> FeatureDepreciation {
        let module = FeatureDepreciation::new();
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::new(manager).expect("in-memory pool");
        pool.get()
            .expect("connection")
            .execute_batch(
                "CREATE TABLE asset_purchases (
                id TEXT PRIMARY KEY, device_serial_no TEXT NOT NULL, purchase_price REAL NOT NULL,
                purchase_date TEXT NOT NULL, vendor TEXT NOT NULL, invoice_no TEXT NOT NULL,
                replacement_value REAL NOT NULL, notes TEXT NOT NULL, created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL
            );
            CREATE TABLE depreciation_log (
                id TEXT PRIMARY KEY, device_serial_no TEXT NOT NULL, period TEXT NOT NULL,
                opening_value REAL NOT NULL, depreciation_amount REAL NOT NULL,
                closing_value REAL NOT NULL, method TEXT NOT NULL, created_at TEXT NOT NULL,
                tenant_id TEXT NOT NULL
            );
            INSERT INTO asset_purchases VALUES
                ('a', 'OWN', 360, '', '', '', 360, '', '', 'test-tenant'),
                ('b', 'OTHER', 720, '', '', '', 720, '', '', 'other-tenant');",
            )
            .expect("fixtures");
        pool.get()
            .expect("connection")
            .execute(
                "INSERT INTO depreciation_log VALUES
             ('foreign-log', 'OTHER', ?1, 720, 20, 700, 'straight_line', '', 'other-tenant')",
                params![current_period()],
            )
            .expect("foreign current-period fixture");
        *module.pool.lock().expect("pool lock") = Some(pool);
        module
    }

    #[test]
    fn depreciation_reads_and_monthly_run_are_limited_to_data_scope() {
        let module = initialized_module();
        let ctx = crate::test_context();

        let hidden = module
            .do_get(
                ctx.data_scope(),
                &GetDepreciationInput {
                    device_serial_no: "OTHER".into(),
                },
            )
            .expect("query succeeds");
        assert_eq!(hidden["summary"]["months"], 0);

        let result = module
            .do_run_monthly(
                ctx.data_scope(),
                &RunMonthlyDepreciationInput {
                    useful_life_months: Some(36),
                },
            )
            .expect("tenant monthly run succeeds");
        assert_eq!(result["devicesProcessed"], 1);

        let conn = module.get_conn().expect("connection");
        let own_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM depreciation_log WHERE tenant_id = 'test-tenant'",
                [],
                |row| row.get(0),
            )
            .expect("own logs");
        let foreign_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM depreciation_log WHERE tenant_id = 'other-tenant'",
                [],
                |row| row.get(0),
            )
            .expect("foreign logs");
        assert_eq!(own_count, 1);
        assert_eq!(foreign_count, 1);
    }
}

//! feature-roa — 设备投资回报率 (ROA) 子模块 (official/device)
//!
//! 命令:
//! - calculate — 单设备 ROA 计算 (年收入 / 资产原值)
//! - list     — 全部设备 ROA 排行

use chrono::Datelike;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;

use system_core::*;

// ═══════════════════════════════════════════════════════════════════
// 输入类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalculateRoaInput {
    pub device_serial_no: String,
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureRoa {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureRoa {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureRoa {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: roa pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    fn do_calculate(&self, scope: &DataScope, input: &CalculateRoaInput) -> Result<Value, String> {
        let conn = self.get_conn()?;

        // Get purchase price from asset_purchases
        let purchase_price: f64 = conn
            .query_row(
                "SELECT purchase_price FROM asset_purchases WHERE device_serial_no = ?1 AND tenant_id = ?2",
                params![input.device_serial_no, scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .map_err(|_| {
                serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_ASSET_NOT_FOUND".into(),
                    message: format!("设备 {} 没有采购记录，无法计算 ROA", input.device_serial_no),
                    field: Some("deviceSerialNo".into()),
                    context: None,
                })
                .unwrap_or_default()
            })?;

        if purchase_price <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ASSET_ZERO_PRICE".into(),
                message: format!("设备 {} 采购价格为 0，无法计算 ROA", input.device_serial_no),
                field: Some("deviceSerialNo".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        // Calculate total revenue: sum of (price * occupancy_coefficient) from order_price_details
        // joined via order_devices for this specific device
        // The coefficient varies: 0.2 for shipping/return days, 1.0 for normal
        // We approximate: sum all price_detail rows where order is completed + device matches
        let total_revenue: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(opd.price * opd.occupancy_coefficient), 0) \
                 FROM order_price_details opd \
                 JOIN order_devices od ON opd.order_id = od.order_id \
                 JOIN orders o ON opd.order_id = o.id \
                 WHERE od.device_serial_no = ?1 \
                 AND od.tenant_id = ?2 AND o.tenant_id = ?2 \
                 AND o.status IN ('completed', 'returned', 'inspected', 'in_use', 'paid', 'shipped')",
                params![input.device_serial_no, scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        // Calculate first order date to determine operational lifespan in years
        let first_order_date: Option<String> = conn
            .query_row(
                "SELECT MIN(opd.date) FROM order_price_details opd \
                 JOIN order_devices od ON opd.order_id = od.order_id \
                 JOIN orders o ON opd.order_id = o.id \
                 WHERE od.device_serial_no = ?1 AND od.tenant_id = ?2 AND o.tenant_id = ?2",
                params![input.device_serial_no, scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .ok()
            .filter(|s: &Option<String>| s.is_some())
            .flatten();

        // Calculate years in service (approximate)
        let years_in_service: f64 = if let Some(ref date_str) = first_order_date {
            if date_str.len() >= 10 {
                let yr: i32 = date_str[0..4].parse().unwrap_or(0);
                let mo: u32 = date_str[5..7].parse().unwrap_or(1);
                let dy: u32 = date_str[8..10].parse().unwrap_or(1);
                let offset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
                let today = chrono::Utc::now().with_timezone(&offset);
                let days = (today.year() - yr) as f64 * 365.25
                    + (today.month() as f64 - mo as f64) * 30.44
                    + (today.day() as f64 - dy as f64);
                (days / 365.25).max(0.083) // at least 1 month
            } else {
                0.0
            }
        } else {
            0.0
        };

        let annual_revenue = if years_in_service > 0.0 {
            total_revenue / years_in_service
        } else {
            total_revenue
        };

        let roa = if purchase_price > 0.0 {
            annual_revenue / purchase_price
        } else {
            0.0
        };

        let roa_pct = roa * 100.0;

        let assessment = if roa_pct >= 100.0 {
            "优秀 — 设备已回收全部成本并盈利"
        } else if roa_pct >= 50.0 {
            "良好 — 设备预计在 2 年内回收成本"
        } else if roa_pct >= 20.0 {
            "一般 — 设备预计在 5 年内回收成本"
        } else if roa_pct > 0.0 {
            "偏低 — 设备回本周期较长"
        } else {
            "无数据 — 设备尚未产生收入"
        };

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": input.device_serial_no,
            "purchasePrice": purchase_price,
            "totalRevenue": total_revenue,
            "annualRevenue": annual_revenue,
            "yearsInService": years_in_service,
            "roa": roa,
            "roaPercent": roa_pct,
            "assessment": assessment,
        }))
    }

    fn do_list(&self, scope: &DataScope) -> Result<Value, String> {
        let conn = self.get_conn()?;

        // Get all devices with purchase records
        let mut stmt = conn
            .prepare(
                "SELECT ap.device_serial_no, ap.purchase_price \
             FROM asset_purchases ap WHERE ap.tenant_id = ?1",
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let devices: Vec<(String, f64)> = stmt
            .query_map(params![scope.tenant_id().as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        let mut results: Vec<Value> = vec![];

        for (serial_no, purchase_price) in &devices {
            let total_revenue: f64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(opd.price * opd.occupancy_coefficient), 0) \
                     FROM order_price_details opd \
                     JOIN order_devices od ON opd.order_id = od.order_id \
                     JOIN orders o ON opd.order_id = o.id \
                     WHERE od.device_serial_no = ?1 \
                     AND od.tenant_id = ?2 AND o.tenant_id = ?2 \
                     AND o.status IN ('completed', 'returned', 'inspected', 'in_use', 'paid', 'shipped')",
                    params![serial_no, scope.tenant_id().as_str()],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            let first_order_date: Option<String> = conn
                .query_row(
                    "SELECT MIN(opd.date) FROM order_price_details opd \
                     JOIN order_devices od ON opd.order_id = od.order_id \
                     JOIN orders o ON opd.order_id = o.id \
                     WHERE od.device_serial_no = ?1 AND od.tenant_id = ?2 AND o.tenant_id = ?2",
                    params![serial_no, scope.tenant_id().as_str()],
                    |row| row.get(0),
                )
                .ok()
                .filter(|s: &Option<String>| s.is_some())
                .flatten();

            let years_in_service: f64 = if let Some(ref date_str) = first_order_date {
                if date_str.len() >= 10 {
                    let yr: i32 = date_str[0..4].parse().unwrap_or(0);
                    let mo: u32 = date_str[5..7].parse().unwrap_or(1);
                    let dy: u32 = date_str[8..10].parse().unwrap_or(1);
                    let offset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
                    let today = chrono::Utc::now().with_timezone(&offset);
                    let days = (today.year() - yr) as f64 * 365.25
                        + (today.month() as f64 - mo as f64) * 30.44
                        + (today.day() as f64 - dy as f64);
                    (days / 365.25).max(0.083)
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let annual_revenue = if years_in_service > 0.0 {
                total_revenue / years_in_service
            } else {
                total_revenue
            };
            let roa = if *purchase_price > 0.0 {
                annual_revenue / purchase_price
            } else {
                0.0
            };

            results.push(serde_json::json!({
                "deviceSerialNo": serial_no,
                "purchasePrice": purchase_price,
                "totalRevenue": total_revenue,
                "annualRevenue": annual_revenue,
                "yearsInService": years_in_service,
                "roa": roa,
                "roaPercent": roa * 100.0,
            }));
        }

        // Sort by ROA descending
        results.sort_by(|a, b| {
            b["roa"]
                .as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&a["roa"].as_f64().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(serde_json::json!({
            "ok": true,
            "devices": results,
            "total": results.len(),
        }))
    }
}

impl SystemModule for FeatureRoa {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "roa".into(),
            version: "0.1.0".into(),
            description: "设备投资回报率 — 年收入/资产原值比率计算".into(),
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
                "list",
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
            "calculate" => {
                let input: CalculateRoaInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_calculate(ctx.data_scope(), &input)
            }
            "list" => self.do_list(ctx.data_scope()),
            _ => Err(format!("MOD_UNKNOWN_COMMAND: roa.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "roa".into(),
            description: "设备投资回报率 — 年收入/资产原值比率计算".into(),
            commands: vec![
                CommandSchema {
                    name: "calculate".into(),
                    description: "单设备 ROA 计算".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(CalculateRoaInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "list".into(),
                    description: "全部设备 ROA 排行".into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tenant_scope_tests {
    use super::*;

    #[test]
    fn roa_assets_and_revenue_are_limited_to_data_scope() {
        let module = FeatureRoa::new();
        let pool = Pool::new(SqliteConnectionManager::memory()).expect("in-memory pool");
        pool.get()
            .expect("connection")
            .execute_batch(
                "CREATE TABLE asset_purchases (
                    device_serial_no TEXT, purchase_price REAL, tenant_id TEXT
                 );
                 CREATE TABLE orders (id TEXT, status TEXT, tenant_id TEXT);
                 CREATE TABLE order_devices (order_id TEXT, device_serial_no TEXT, tenant_id TEXT);
                 CREATE TABLE order_price_details (
                    order_id TEXT, price REAL, occupancy_coefficient REAL, date TEXT
                 );
                 INSERT INTO asset_purchases VALUES
                    ('SHARED', 100, 'test-tenant'),
                    ('FOREIGN', 200, 'other-tenant');
                 INSERT INTO orders VALUES
                    ('local-order', 'completed', 'test-tenant'),
                    ('foreign-order', 'completed', 'other-tenant');
                 INSERT INTO order_devices VALUES
                    ('local-order', 'SHARED', 'test-tenant'),
                    ('foreign-order', 'SHARED', 'other-tenant');
                 INSERT INTO order_price_details VALUES
                    ('local-order', 10, 1, '2026-01-01'),
                    ('foreign-order', 900, 1, '2026-01-01');",
            )
            .expect("fixtures");
        *module.pool.lock().expect("pool lock") = Some(pool);
        let ctx = crate::test_context();

        let listed = module.do_list(ctx.data_scope()).expect("list");
        assert_eq!(listed["total"], 1);
        assert_eq!(listed["devices"][0]["deviceSerialNo"], "SHARED");
        assert_eq!(listed["devices"][0]["totalRevenue"], 10.0);

        let hidden = module.do_calculate(
            ctx.data_scope(),
            &CalculateRoaInput {
                device_serial_no: "FOREIGN".into(),
            },
        );
        assert!(hidden.is_err());
    }
}

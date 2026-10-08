//! feature-damage — 设备损坏报告子模块 (official/device)
//!
//! 命令:
//! - report    — 归还检查时登记损坏 (reported)
//! - assess    — 定损 (reported→assessed, 含 estimated_damage_amount + liability)
//! - adjudicate — 责任认定完成 (assessed→adjudicated)
//! - get       — 按 order_id 或 device_serial_no 查询损坏报告
//! - list      — 分页查询损坏报告

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

fn shanghai_now_iso() -> String {
    let offset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    chrono::Utc::now()
        .with_timezone(&offset)
        .format("%Y-%m-%dT%H:%M:%S%.3f+08:00")
        .to_string()
}

// ═══════════════════════════════════════════════════════════════════
// 输入类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportDamageInput {
    pub order_id: String,
    pub device_serial_no: String,
    pub appearance_ok: bool,
    pub accessories_ok: bool,
    pub function_ok: bool,
    #[serde(default)]
    pub damage_description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssessDamageInput {
    pub damage_id: String,
    pub estimated_damage_amount: f64,
    pub liability: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdjudicateDamageInput {
    pub damage_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDamageInput {
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub device_serial_no: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListDamageInput {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub page_size: Option<i64>,
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureDamage {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureDamage {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureDamage {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: damage pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    fn do_report(
        &self,
        input: &ReportDamageInput,
        operator: &str,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        if input.appearance_ok && input.accessories_ok && input.function_ok {
            return Ok(serde_json::json!({ "ok": true, "damageId": null, "noDamage": true }));
        }

        let resource_exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM orders o JOIN devices d ON d.serialNo = ?1 \
                 WHERE o.tenant_id = ?2 AND d.tenant_id = ?2 AND o.id = ?3",
                params![
                    input.device_serial_no,
                    ctx.data_scope().tenant_id().as_str(),
                    input.order_id
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;
        if resource_exists.is_none() {
            return Err(
                "BIZ_DAMAGE_RESOURCE_NOT_FOUND: order or device is outside data scope".into(),
            );
        }

        let damage_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO damage_reports (id, order_id, device_serial_no, appearance_ok, accessories_ok, function_ok, \
             damage_description, status, reported_by, reported_at, created_at, updated_at, tenant_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'reported', ?8, ?9, ?10, ?11, ?12)",
            params![
                damage_id, input.order_id, input.device_serial_no,
                input.appearance_ok as i32, input.accessories_ok as i32, input.function_ok as i32,
                input.damage_description, operator, now, now, now, ctx.data_scope().tenant_id().as_str(),
            ],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "damageId": damage_id,
            "orderId": input.order_id,
            "deviceSerialNo": input.device_serial_no,
            "status": "reported",
            "noDamage": false,
        }))
    }

    fn do_assess(
        &self,
        input: &AssessDamageInput,
        operator: &str,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let valid_liabilities = ["customer", "logistics", "warehouse", "unknown"];
        if !valid_liabilities.contains(&input.liability.as_str()) {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_INVALID_LIABILITY".into(),
                message: format!("责任类型无效: {}", input.liability),
                field: Some("liability".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let (status, order_id, device_serial_no): (String, String, String) = conn
            .query_row(
                "SELECT status, order_id, device_serial_no FROM damage_reports WHERE tenant_id = ?1 AND id = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.damage_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|_| serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_FOUND".into(),
                message: format!("损坏报告 {} 不存在", input.damage_id),
                field: Some("damageId".into()),
                context: None,
            }).unwrap_or_default())?;

        if status != "reported" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_REPORTED".into(),
                message: format!("损坏报告状态为 {}，不可定损", status),
                field: Some("damageId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute(
            "UPDATE damage_reports SET estimated_damage_amount = ?1, liability = ?2, \
             damage_description = damage_description || ?3, \
             status = 'assessed', assessed_by = ?4, assessed_at = ?5, updated_at = ?6 WHERE tenant_id = ?7 AND id = ?8",
            params![input.estimated_damage_amount, input.liability,
                    if input.notes.is_empty() { "".to_string() } else { format!(" | 定损备注: {}", input.notes) },
                    operator, now, now, ctx.data_scope().tenant_id().as_str(), input.damage_id],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "damageId": input.damage_id,
            "orderId": order_id,
            "deviceSerialNo": device_serial_no,
            "estimatedAmount": input.estimated_damage_amount,
            "liability": input.liability,
            "status": "assessed",
        }))
    }

    fn do_adjudicate(
        &self,
        input: &AdjudicateDamageInput,
        operator: &str,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let (status, order_id, device_serial_no): (String, String, String) = conn
            .query_row(
                "SELECT status, order_id, device_serial_no FROM damage_reports WHERE tenant_id = ?1 AND id = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.damage_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|_| serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_FOUND".into(),
                message: format!("损坏报告 {} 不存在", input.damage_id),
                field: Some("damageId".into()),
                context: None,
            }).unwrap_or_default())?;

        if status != "assessed" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_ASSESSED".into(),
                message: format!("损坏报告状态为 {}，不可认定", status),
                field: Some("damageId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute(
            "UPDATE damage_reports SET status = 'adjudicated', adjudicated_by = ?1, adjudicated_at = ?2, updated_at = ?3 WHERE tenant_id = ?4 AND id = ?5",
            params![operator, now, now, ctx.data_scope().tenant_id().as_str(), input.damage_id],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "damageId": input.damage_id,
            "orderId": order_id,
            "deviceSerialNo": device_serial_no,
            "status": "adjudicated",
        }))
    }

    fn do_get(&self, input: &GetDamageInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let conn = self.get_conn()?;

        let (where_clause, param_val): (String, String) = if let Some(ref oid) = input.order_id {
            ("order_id = ?2".to_string(), oid.clone())
        } else if let Some(ref sn) = input.device_serial_no {
            ("device_serial_no = ?2".to_string(), sn.clone())
        } else {
            return Err("VAL_MISSING_FILTER: orderId or deviceSerialNo required".to_string());
        };

        let sql = format!(
            "SELECT id, order_id, device_serial_no, appearance_ok, accessories_ok, function_ok, \
                    damage_description, estimated_damage_amount, liability, status, \
                    reported_by, assessed_by, adjudicated_by, \
                    reported_at, assessed_at, adjudicated_at, created_at, updated_at \
             FROM damage_reports WHERE tenant_id = ?1 AND {} ORDER BY created_at DESC",
            where_clause,
        );

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;
        let reports: Vec<Value> = stmt
            .query_map(
                params![ctx.data_scope().tenant_id().as_str(), param_val],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "orderId": row.get::<_, String>(1)?,
                        "deviceSerialNo": row.get::<_, String>(2)?,
                        "appearanceOk": row.get::<_, bool>(3)?,
                        "accessoriesOk": row.get::<_, bool>(4)?,
                        "functionOk": row.get::<_, bool>(5)?,
                        "damageDescription": row.get::<_, String>(6)?,
                        "estimatedDamageAmount": row.get::<_, f64>(7)?,
                        "liability": row.get::<_, String>(8)?,
                        "status": row.get::<_, String>(9)?,
                        "reportedBy": row.get::<_, String>(10)?,
                        "assessedBy": row.get::<_, Option<String>>(11)?,
                        "adjudicatedBy": row.get::<_, Option<String>>(12)?,
                        "reportedAt": row.get::<_, String>(13)?,
                        "assessedAt": row.get::<_, Option<String>>(14)?,
                        "adjudicatedAt": row.get::<_, Option<String>>(15)?,
                        "createdAt": row.get::<_, String>(16)?,
                        "updatedAt": row.get::<_, String>(17)?,
                    }))
                },
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!({ "ok": true, "damageReports": reports }))
    }

    fn do_list(&self, input: &ListDamageInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let offset = (page - 1) * page_size;

        let (count_sql, data_sql, params_str): (String, String, Option<String>) = if let Some(
            ref s,
        ) =
            input.status
        {
            if !s.is_empty() {
                (
                    "SELECT COUNT(*) FROM damage_reports WHERE tenant_id = ?1 AND status = ?2"
                        .into(),
                    format!(
                        "SELECT id, order_id, device_serial_no, estimated_damage_amount, liability, status, \
                                reported_at, assessed_at, adjudicated_at \
                                FROM damage_reports WHERE tenant_id = ?1 AND status = ?2 ORDER BY created_at DESC LIMIT {} OFFSET {}",
                        page_size, offset
                    ),
                    Some(s.clone()),
                )
            } else {
                (
                    "SELECT COUNT(*) FROM damage_reports WHERE tenant_id = ?1".into(),
                    format!(
                        "SELECT id, order_id, device_serial_no, estimated_damage_amount, liability, status, \
                                reported_at, assessed_at, adjudicated_at \
                                FROM damage_reports WHERE tenant_id = ?1 ORDER BY created_at DESC LIMIT {} OFFSET {}",
                        page_size, offset
                    ),
                    None,
                )
            }
        } else {
            (
                "SELECT COUNT(*) FROM damage_reports WHERE tenant_id = ?1".into(),
                format!(
                    "SELECT id, order_id, device_serial_no, estimated_damage_amount, liability, status, \
                            reported_at, assessed_at, adjudicated_at \
                            FROM damage_reports WHERE tenant_id = ?1 ORDER BY created_at DESC LIMIT {} OFFSET {}",
                    page_size, offset
                ),
                None,
            )
        };

        let total: i64 = if let Some(ref s) = params_str {
            conn.query_row(
                &count_sql,
                params![ctx.data_scope().tenant_id().as_str(), s],
                |row| row.get(0),
            )
        } else {
            conn.query_row(
                &count_sql,
                params![ctx.data_scope().tenant_id().as_str()],
                |row| row.get(0),
            )
        }
        .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let mut stmt = conn
            .prepare(&data_sql)
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;
        let rows: Vec<Value> = if let Some(ref s) = params_str {
            stmt.query_map(params![ctx.data_scope().tenant_id().as_str(), s], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "orderId": row.get::<_, String>(1)?,
                    "deviceSerialNo": row.get::<_, String>(2)?,
                    "estimatedDamageAmount": row.get::<_, f64>(3)?,
                    "liability": row.get::<_, String>(4)?,
                    "status": row.get::<_, String>(5)?,
                    "reportedAt": row.get::<_, String>(6)?,
                    "assessedAt": row.get::<_, Option<String>>(7)?,
                    "adjudicatedAt": row.get::<_, Option<String>>(8)?,
                }))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect()
        } else {
            stmt.query_map(params![ctx.data_scope().tenant_id().as_str()], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "orderId": row.get::<_, String>(1)?,
                    "deviceSerialNo": row.get::<_, String>(2)?,
                    "estimatedDamageAmount": row.get::<_, f64>(3)?,
                    "liability": row.get::<_, String>(4)?,
                    "status": row.get::<_, String>(5)?,
                    "reportedAt": row.get::<_, String>(6)?,
                    "assessedAt": row.get::<_, Option<String>>(7)?,
                    "adjudicatedAt": row.get::<_, Option<String>>(8)?,
                }))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect()
        };

        Ok(serde_json::json!({
            "ok": true,
            "damageReports": rows,
            "pagination": { "page": page, "pageSize": page_size, "total": total },
        }))
    }
}

impl SystemModule for FeatureDamage {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "damage".into(),
            version: "0.1.0".into(),
            description: "设备损坏报告 — report/assess/adjudicate".into(),
            author: "talos".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "report",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "assess",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "adjudicate",
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
        let operator = ctx.user_id().unwrap_or("system");

        match command {
            "report" => {
                let input: ReportDamageInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_report(&input, operator, ctx)
            }
            "assess" => {
                let input: AssessDamageInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_assess(&input, operator, ctx)
            }
            "adjudicate" => {
                let input: AdjudicateDamageInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_adjudicate(&input, operator, ctx)
            }
            "get" => {
                let input: GetDamageInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_get(&input, ctx)
            }
            "list" => {
                let input: ListDamageInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_list(&input, ctx)
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: damage.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "damage".into(),
            description: "设备损坏报告 — report/assess/adjudicate".into(),
            commands: vec![
                CommandSchema {
                    name: "report".into(),
                    description: "登记设备损坏".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ReportDamageInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "assess".into(),
                    description: "定损 + 责任认定".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(AssessDamageInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "adjudicate".into(),
                    description: "责任认定完成".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        AdjudicateDamageInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get".into(),
                    description: "按订单号或设备SN查询损坏报告".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetDamageInput)).ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "list".into(),
                    description: "分页查询损坏报告".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ListDamageInput)).ok(),
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
    fn damage_reads_are_limited_to_execution_context_data_scope() {
        let module = FeatureDamage::new();
        let pool = Pool::new(SqliteConnectionManager::memory()).expect("in-memory pool");
        pool.get().expect("connection").execute_batch(
            "CREATE TABLE damage_reports (
                id TEXT PRIMARY KEY, order_id TEXT NOT NULL, device_serial_no TEXT NOT NULL,
                appearance_ok INTEGER NOT NULL, accessories_ok INTEGER NOT NULL, function_ok INTEGER NOT NULL,
                damage_description TEXT NOT NULL, estimated_damage_amount REAL NOT NULL,
                liability TEXT NOT NULL, status TEXT NOT NULL, reported_by TEXT NOT NULL,
                assessed_by TEXT, adjudicated_by TEXT, reported_at TEXT NOT NULL,
                assessed_at TEXT, adjudicated_at TEXT, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, tenant_id TEXT NOT NULL
            );
            INSERT INTO damage_reports VALUES
                ('mine', 'ORDER', 'SHARED', 0, 1, 1, '', 0, 'unknown', 'reported', '', NULL, NULL, '', NULL, NULL, '2', '2', 'test-tenant'),
                ('hidden', 'ORDER', 'SHARED', 0, 1, 1, '', 0, 'unknown', 'reported', '', NULL, NULL, '', NULL, NULL, '1', '1', 'other-tenant');"
        ).expect("fixtures");
        *module.pool.lock().expect("pool lock") = Some(pool);
        let ctx = crate::test_context();

        let result = module
            .do_get(
                &GetDamageInput {
                    order_id: None,
                    device_serial_no: Some("SHARED".into()),
                },
                &ctx,
            )
            .expect("scoped query");
        let reports = result["damageReports"].as_array().expect("reports");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0]["id"], "mine");

        let mutation = module.do_assess(
            &AssessDamageInput {
                damage_id: "hidden".into(),
                estimated_damage_amount: 1.0,
                liability: "unknown".into(),
                notes: String::new(),
            },
            "tester",
            &ctx,
        );
        assert!(mutation.is_err());
    }
}

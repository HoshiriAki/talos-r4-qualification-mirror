//! feature-repair — 维修工单子模块 (official/device)
//!
//! 命令:
//! - create_repair_order — 根据损坏报告创建维修工单 (pending)
//! - start_repair        — 开始维修 (pending→in_progress)
//! - complete_repair     — 维修完成 (in_progress→completed)
//! - return_to_stock     — 设备返库 (completed→returned, 更新设备状态为 available)
//! - get                 — 查询维修工单详情
//! - list                — 分页查询 (可按 status 过滤)
//! - get_device_stats    — 设备维修历史聚合 + 75%替换阈值告警

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::OptionalExtension;
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};
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
pub struct CreateRepairOrderInput {
    pub damage_report_id: String,
    #[serde(default)]
    pub repair_description: String,
    #[serde(default)]
    pub vendor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRepairStatusInput {
    pub repair_id: String,
    #[serde(default)]
    pub repair_cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetRepairInput {
    pub repair_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListRepairInput {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub device_serial_no: Option<String>,
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDeviceStatsInput {
    pub device_serial_no: String,
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureRepair {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
    /// Legacy composition handle retained for compatibility only. Financial effects
    /// are owned by the R3 settlement authority and must not be emitted from repair.
    pub deposit_module: Mutex<Option<Arc<dyn SystemModule>>>,
    /// Legacy composition handle retained for compatibility only. Return-to-stock
    /// inventory state is updated atomically on the repair connection below.
    pub device_module: Mutex<Option<Arc<dyn SystemModule>>>,
}

impl Default for FeatureRepair {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureRepair {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
            deposit_module: Mutex::new(None),
            device_module: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: repair pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    fn do_create_repair_order(
        &self,
        input: &CreateRepairOrderInput,
        operator: &str,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let (device_serial_no, damage_status): (String, String) = conn
            .query_row(
                "SELECT device_serial_no, status FROM damage_reports WHERE tenant_id = ?1 AND id = ?2",
                params![
                    ctx.data_scope().tenant_id().as_str(),
                    input.damage_report_id
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| {
                serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_DAMAGE_NOT_FOUND".into(),
                    message: format!("损坏报告 {} 不存在", input.damage_report_id),
                    field: Some("damageReportId".into()),
                    context: None,
                })
                .unwrap_or_default()
            })?;

        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM repair_orders WHERE tenant_id = ?1 AND damage_report_id = ?2",
                params![
                    ctx.data_scope().tenant_id().as_str(),
                    input.damage_report_id
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        if let Some(existing_id) = existing {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REPAIR_ALREADY_EXISTS".into(),
                message: format!("损坏报告已有维修工单 {}", existing_id),
                field: Some("damageReportId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        if damage_status != "adjudicated" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_DAMAGE_NOT_ADJUDICATED".into(),
                message: format!("损坏报告尚未责任认定 (当前: {})", damage_status),
                field: Some("damageReportId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| format!("SYS_DB_TXN: {}", e))?;

        let result = (|| -> Result<Value, String> {
            let repair_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO repair_orders (id, damage_report_id, device_serial_no, repair_description, \
                 vendor, created_by, created_at, updated_at, tenant_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![repair_id, input.damage_report_id, device_serial_no,
                        input.repair_description, input.vendor, operator, now, now,
                        ctx.data_scope().tenant_id().as_str()],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

            Ok(serde_json::json!({
                "ok": true,
                "repairId": repair_id,
                "damageReportId": input.damage_report_id,
                "deviceSerialNo": device_serial_no,
                "status": "pending",
                "depositForfeited": false,
                "settlementAuthority": "r3_settlement",
            }))
        })();

        match &result {
            Ok(_) => conn
                .execute_batch("COMMIT")
                .map_err(|e| format!("SYS_DB_TXN_COMMIT: {}", e))?,
            Err(_) => {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        result
    }

    fn do_start_repair(
        &self,
        input: &UpdateRepairStatusInput,
        _operator: &str,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let status: String = conn
            .query_row(
                "SELECT status FROM repair_orders WHERE tenant_id = ?1 AND id = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.repair_id],
                |row| row.get(0),
            )
            .map_err(|_| {
                serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_REPAIR_NOT_FOUND".into(),
                    message: format!("维修工单 {} 不存在", input.repair_id),
                    field: Some("repairId".into()),
                    context: None,
                })
                .unwrap_or_default()
            })?;

        if status != "pending" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REPAIR_NOT_PENDING".into(),
                message: format!("工单状态为 {}，不可开始维修", status),
                field: Some("repairId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute(
            "UPDATE repair_orders SET status = 'in_progress', started_at = ?1, updated_at = ?2 WHERE tenant_id = ?3 AND id = ?4",
            params![now, now, ctx.data_scope().tenant_id().as_str(), input.repair_id],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        Ok(serde_json::json!({ "ok": true, "repairId": input.repair_id, "status": "in_progress" }))
    }

    fn do_complete_repair(
        &self,
        input: &UpdateRepairStatusInput,
        operator: &str,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let status: String = conn
            .query_row(
                "SELECT status FROM repair_orders WHERE tenant_id = ?1 AND id = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.repair_id],
                |row| row.get(0),
            )
            .map_err(|_| {
                serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_REPAIR_NOT_FOUND".into(),
                    message: format!("维修工单 {} 不存在", input.repair_id),
                    field: Some("repairId".into()),
                    context: None,
                })
                .unwrap_or_default()
            })?;

        if status != "in_progress" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REPAIR_NOT_IN_PROGRESS".into(),
                message: format!("工单状态为 {}，不可完成", status),
                field: Some("repairId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let cost = input.repair_cost.unwrap_or(0.0);
        conn.execute(
            "UPDATE repair_orders SET status = 'completed', repair_cost = ?1, completed_by = ?2, completed_at = ?3, updated_at = ?4 WHERE tenant_id = ?5 AND id = ?6",
            params![cost, operator, now, now, ctx.data_scope().tenant_id().as_str(), input.repair_id],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        Ok(
            serde_json::json!({ "ok": true, "repairId": input.repair_id, "status": "completed", "repairCost": cost }),
        )
    }

    fn do_return_to_stock(
        &self,
        input: &UpdateRepairStatusInput,
        operator: &str,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| format!("SYS_DB_TXN: {}", e))?;

        let result = (|| -> Result<Value, String> {
            let (status, device_serial_no): (String, String) = conn
                .query_row(
                    "SELECT status, device_serial_no FROM repair_orders WHERE tenant_id = ?1 AND id = ?2",
                    params![tenant_id, input.repair_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| {
                    serde_json::to_string(&ErrorPayload {
                        category: "biz".into(),
                        code: "BIZ_REPAIR_NOT_FOUND".into(),
                        message: format!("维修工单 {} 不存在", input.repair_id),
                        field: Some("repairId".into()),
                        context: None,
                    })
                    .unwrap_or_default()
                })?;

            if status != "completed" {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_REPAIR_NOT_COMPLETED".into(),
                    message: format!("工单状态为 {}，不可返库", status),
                    field: Some("repairId".into()),
                    context: None,
                })
                .unwrap_or_default());
            }

            let device_updated = conn
                .execute(
                    "UPDATE devices SET rentalStatus = 'available' WHERE tenant_id = ?1 AND serialNo = ?2",
                    params![tenant_id, device_serial_no],
                )
                .map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;
            if device_updated != 1 {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_DEVICE_NOT_FOUND".into(),
                    message: format!("返库设备 {} 不存在或不属于当前租户", device_serial_no),
                    field: Some("deviceSerialNo".into()),
                    context: None,
                })
                .unwrap_or_default());
            }

            let repair_updated = conn
                .execute(
                    "UPDATE repair_orders SET status = 'returned', returned_by = ?1, returned_at = ?2, updated_at = ?3 WHERE tenant_id = ?4 AND id = ?5 AND status = 'completed'",
                    params![operator, now, now, tenant_id, input.repair_id],
                )
                .map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;
            if repair_updated != 1 {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_REPAIR_NOT_COMPLETED".into(),
                    message: "维修状态已变化，返库事务已取消".into(),
                    field: Some("repairId".into()),
                    context: None,
                })
                .unwrap_or_default());
            }

            Ok(serde_json::json!({
                "ok": true,
                "repairId": input.repair_id,
                "deviceSerialNo": device_serial_no,
                "status": "returned",
            }))
        })();

        match &result {
            Ok(_) => conn
                .execute_batch("COMMIT")
                .map_err(|e| format!("SYS_DB_TXN_COMMIT: {}", e))?,
            Err(_) => {
                let _ = conn.execute_batch("ROLLBACK");
            }
        }
        result
    }

    fn do_get(&self, input: &GetRepairInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let r: Value = conn
            .query_row(
                "SELECT id, damage_report_id, device_serial_no, status, repair_description, repair_cost, vendor, \
                        created_by, completed_by, returned_by, started_at, completed_at, returned_at, created_at, updated_at \
                 FROM repair_orders WHERE tenant_id = ?1 AND id = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.repair_id],
                |row| Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "damageReportId": row.get::<_, String>(1)?,
                    "deviceSerialNo": row.get::<_, String>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "repairDescription": row.get::<_, String>(4)?,
                    "repairCost": row.get::<_, f64>(5)?,
                    "vendor": row.get::<_, String>(6)?,
                    "createdBy": row.get::<_, String>(7)?,
                    "completedBy": row.get::<_, Option<String>>(8)?,
                    "returnedBy": row.get::<_, Option<String>>(9)?,
                    "startedAt": row.get::<_, Option<String>>(10)?,
                    "completedAt": row.get::<_, Option<String>>(11)?,
                    "returnedAt": row.get::<_, Option<String>>(12)?,
                    "createdAt": row.get::<_, String>(13)?,
                    "updatedAt": row.get::<_, String>(14)?,
                })),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .unwrap_or(serde_json::json!(null));

        Ok(serde_json::json!({ "ok": true, "repairOrder": r }))
    }

    fn do_list(&self, input: &ListRepairInput, ctx: &ExecutionContext) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let offset = (page - 1) * page_size;

        let mut conditions: Vec<String> = vec!["tenant_id = ?1".into()];
        let mut params_vec: Vec<String> = vec![ctx.data_scope().tenant_id().as_str().to_owned()];

        if let Some(ref s) = input.status
            && !s.is_empty()
        {
            params_vec.push(s.clone());
            conditions.push(format!("status = ?{}", params_vec.len()));
        }
        if let Some(ref sn) = input.device_serial_no
            && !sn.is_empty()
        {
            params_vec.push(sn.clone());
            conditions.push(format!("device_serial_no = ?{}", params_vec.len()));
        }

        let where_clause = conditions.join(" AND ");
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = params_vec
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();

        let total: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM repair_orders WHERE {}", where_clause),
                params_ref.as_slice(),
                |row| row.get(0),
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let data_sql = format!(
            "SELECT id, damage_report_id, device_serial_no, status, repair_cost, vendor, started_at, completed_at, returned_at, created_at \
             FROM repair_orders WHERE {} ORDER BY created_at DESC LIMIT {} OFFSET {}",
            where_clause, page_size, offset,
        );

        let mut stmt = conn
            .prepare(&data_sql)
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;
        let rows: Vec<Value> = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "damageReportId": row.get::<_, String>(1)?,
                    "deviceSerialNo": row.get::<_, String>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "repairCost": row.get::<_, f64>(4)?,
                    "vendor": row.get::<_, String>(5)?,
                    "startedAt": row.get::<_, Option<String>>(6)?,
                    "completedAt": row.get::<_, Option<String>>(7)?,
                    "returnedAt": row.get::<_, Option<String>>(8)?,
                    "createdAt": row.get::<_, String>(9)?,
                }))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!({
            "ok": true, "repairOrders": rows,
            "pagination": { "page": page, "pageSize": page_size, "total": total },
        }))
    }

    fn do_get_device_stats(
        &self,
        input: &GetDeviceStatsInput,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;

        let (total_repairs, total_repair_cost): (i64, f64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(repair_cost), 0) FROM repair_orders WHERE tenant_id = ?1 AND device_serial_no = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.device_serial_no],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let replacement_value: f64 = conn
            .query_row(
                "SELECT replacement_value FROM asset_purchases WHERE tenant_id = ?1 AND device_serial_no = ?2",
                params![ctx.data_scope().tenant_id().as_str(), input.device_serial_no],
                |row| row.get(0),
            )
            .unwrap_or(0.0);

        let exceeds_threshold =
            replacement_value > 0.0 && total_repair_cost > replacement_value * 0.75;
        let threshold_ratio = if replacement_value > 0.0 {
            total_repair_cost / replacement_value
        } else {
            0.0
        };

        let mut stmt = conn.prepare(
            "SELECT id, status, repair_cost, vendor, created_at, completed_at \
             FROM repair_orders WHERE tenant_id = ?1 AND device_serial_no = ?2 ORDER BY created_at DESC LIMIT 10"
        ).map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let recent_repairs: Vec<Value> = stmt
            .query_map(
                params![
                    ctx.data_scope().tenant_id().as_str(),
                    input.device_serial_no
                ],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "status": row.get::<_, String>(1)?,
                        "repairCost": row.get::<_, f64>(2)?,
                        "vendor": row.get::<_, String>(3)?,
                        "createdAt": row.get::<_, String>(4)?,
                        "completedAt": row.get::<_, Option<String>>(5)?,
                    }))
                },
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": input.device_serial_no,
            "totalRepairs": total_repairs,
            "totalRepairCost": total_repair_cost,
            "replacementValue": replacement_value,
            "thresholdRatio": threshold_ratio,
            "exceedsReplacementThreshold": exceeds_threshold,
            "warning": if exceeds_threshold { "累计维修费用超过重置价值的75%，建议替换设备" } else { "" },
            "recentRepairs": recent_repairs,
        }))
    }
}

impl SystemModule for FeatureRepair {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "repair".into(),
            version: "0.1.0".into(),
            description: "维修工单 — 送修/维修中/已返库".into(),
            author: "talos".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "create_repair_order",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "start_repair",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "complete_repair",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "return_to_stock",
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
                "get_device_stats",
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
            "create_repair_order" => {
                let input: CreateRepairOrderInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_create_repair_order(&input, operator, ctx)
            }
            "start_repair" => {
                let input: UpdateRepairStatusInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_start_repair(&input, operator, ctx)
            }
            "complete_repair" => {
                let input: UpdateRepairStatusInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_complete_repair(&input, operator, ctx)
            }
            "return_to_stock" => {
                let input: UpdateRepairStatusInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_return_to_stock(&input, operator, ctx)
            }
            "get" => {
                let input: GetRepairInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_get(&input, ctx)
            }
            "list" => {
                let input: ListRepairInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_list(&input, ctx)
            }
            "get_device_stats" => {
                let input: GetDeviceStatsInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_get_device_stats(&input, ctx)
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: repair.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "repair".into(),
            description: "维修工单 — 送修/维修中/已返库".into(),
            commands: vec![
                CommandSchema {
                    name: "create_repair_order".into(),
                    description: "创建维修工单；资金处理由 R3 settlement authority 完成".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        CreateRepairOrderInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "start_repair".into(),
                    description: "开始维修".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        UpdateRepairStatusInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "complete_repair".into(),
                    description: "维修完成 (含维修费用)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        UpdateRepairStatusInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "return_to_stock".into(),
                    description: "设备返库 + 恢复可租状态".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        UpdateRepairStatusInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get".into(),
                    description: "查询维修工单详情".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetRepairInput)).ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "list".into(),
                    description: "分页查询维修工单".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ListRepairInput)).ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_device_stats".into(),
                    description: "设备维修历史聚合 + 75%替换阈值告警".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetDeviceStatsInput))
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
    fn repair_reads_are_limited_to_execution_context_data_scope() {
        let module = FeatureRepair::new();
        let pool = Pool::new(SqliteConnectionManager::memory()).expect("in-memory pool");
        pool.get().expect("connection").execute_batch(
            "CREATE TABLE repair_orders (
                id TEXT PRIMARY KEY, damage_report_id TEXT NOT NULL, device_serial_no TEXT NOT NULL,
                status TEXT NOT NULL, repair_description TEXT NOT NULL, repair_cost REAL NOT NULL,
                vendor TEXT NOT NULL, created_by TEXT NOT NULL, completed_by TEXT, returned_by TEXT,
                started_at TEXT, completed_at TEXT, returned_at TEXT, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, tenant_id TEXT NOT NULL
            );
            INSERT INTO repair_orders VALUES
                ('mine', 'D1', 'SHARED', 'pending', '', 0, '', '', NULL, NULL, NULL, NULL, NULL, '2', '2', 'test-tenant'),
                ('hidden', 'D2', 'SHARED', 'pending', '', 0, '', '', NULL, NULL, NULL, NULL, NULL, '1', '1', 'other-tenant');"
        ).expect("fixtures");
        *module.pool.lock().expect("pool lock") = Some(pool);
        let ctx = crate::test_context();

        let hidden = module
            .do_get(
                &GetRepairInput {
                    repair_id: "hidden".into(),
                },
                &ctx,
            )
            .expect("scoped get");
        assert!(hidden["repairOrder"].is_null());

        let listed = module
            .do_list(
                &ListRepairInput {
                    status: None,
                    device_serial_no: Some("SHARED".into()),
                    page: None,
                    page_size: None,
                },
                &ctx,
            )
            .expect("scoped list");
        let repairs = listed["repairOrders"].as_array().expect("repair orders");
        assert_eq!(repairs.len(), 1);
        assert_eq!(repairs[0]["id"], "mine");

        let mutation = module.do_start_repair(
            &UpdateRepairStatusInput {
                repair_id: "hidden".into(),
                repair_cost: None,
            },
            "tester",
            &ctx,
        );
        assert!(mutation.is_err());
    }

    #[test]
    fn return_to_stock_rolls_back_until_device_inventory_update_can_commit() {
        let module = FeatureRepair::new();
        let pool = Pool::new(SqliteConnectionManager::memory()).expect("in-memory pool");
        let conn = pool.get().expect("connection");
        conn.execute_batch(
            "CREATE TABLE repair_orders (
                id TEXT PRIMARY KEY, damage_report_id TEXT NOT NULL, device_serial_no TEXT NOT NULL,
                status TEXT NOT NULL, repair_description TEXT NOT NULL, repair_cost REAL NOT NULL,
                vendor TEXT NOT NULL, created_by TEXT NOT NULL, completed_by TEXT, returned_by TEXT,
                started_at TEXT, completed_at TEXT, returned_at TEXT, created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL, tenant_id TEXT NOT NULL
            );
            CREATE TABLE devices (
                serialNo TEXT NOT NULL, rentalStatus TEXT NOT NULL, tenant_id TEXT NOT NULL,
                PRIMARY KEY (tenant_id, serialNo)
            );
            INSERT INTO repair_orders VALUES
                ('repair-1', 'D1', 'DEVICE1', 'completed', '', 100, '', 'admin', 'admin', NULL,
                 '1', '2', NULL, '1', '2', 'test-tenant');",
        )
        .expect("fixtures");
        drop(conn);
        *module.pool.lock().expect("pool lock") = Some(pool.clone());
        let ctx = crate::test_context();
        let input = UpdateRepairStatusInput {
            repair_id: "repair-1".into(),
            repair_cost: None,
        };

        let failed = module.do_return_to_stock(&input, "tester", &ctx);
        assert!(failed.is_err());
        assert!(failed.unwrap_err().contains("BIZ_DEVICE_NOT_FOUND"));

        let conn = pool.get().expect("connection");
        let repair_status: String = conn
            .query_row(
                "SELECT status FROM repair_orders WHERE tenant_id = 'test-tenant' AND id = 'repair-1'",
                [],
                |row| row.get(0),
            )
            .expect("repair status");
        assert_eq!(repair_status, "completed");
        conn.execute(
            "INSERT INTO devices (serialNo, rentalStatus, tenant_id) VALUES ('DEVICE1', 'repairing', 'test-tenant')",
            [],
        )
        .expect("device fixture");
        drop(conn);

        let returned = module
            .do_return_to_stock(&input, "tester", &ctx)
            .expect("atomic return-to-stock");
        assert_eq!(returned["status"], "returned");

        let conn = pool.get().expect("connection");
        let (repair_status, rental_status): (String, String) = conn
            .query_row(
                "SELECT r.status, d.rentalStatus
                 FROM repair_orders r
                 JOIN devices d ON d.tenant_id = r.tenant_id AND d.serialNo = r.device_serial_no
                 WHERE r.tenant_id = 'test-tenant' AND r.id = 'repair-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("committed states");
        assert_eq!(repair_status, "returned");
        assert_eq!(rental_status, "available");
    }
}

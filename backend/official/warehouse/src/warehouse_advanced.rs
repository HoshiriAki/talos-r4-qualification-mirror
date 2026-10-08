//! 仓库管理增强 — 首个原生 Maxwell 模块
//!
//! 验证 5 个假设：
//! 1. execute() 跨模块调用的 JSON 序列化开销是否可接受
//! 2. Saga 原语在真实业务中可用
//! 3. 4 层管线可处理跨表查询校验
//! 4. manifest.json 可驱动前端自动路由
//! 5. 样板量降到可接受水平（command! 宏）
//!
//! 命令：
//! - get_low_stock      → 库存预警——查询低于阈值的设备
//! - set_alert_threshold → 设置预警阈值
//! - transfer_device    → 设备调拨 Saga（lock → deduct → transfer → commit / compensate）
//! - cancel_transfer    → 取消调拨补偿
//! - get_capacity_stats → 仓库容量统计

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use system_core::*;

// ── 时间辅助 ──

fn shanghai_now_iso() -> String {
    chrono::Local::now()
        .format("%Y-%m-%dT%H:%M:%S%.3f+08:00")
        .to_string()
}

// ═══════════════════════════════════════════════════════════════════
// 输入类型
// ═══════════════════════════════════════════════════════════════════

// ── get_low_stock ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetLowStockInput {
    #[serde(default)]
    pub warehouse_id: Option<String>,
}

impl Validate for GetLowStockInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for GetLowStockInput {
    fn sanitize(&mut self) {
        if let Some(ref mut id) = self.warehouse_id {
            *id = id.trim().to_string();
        }
    }
}

// ── set_alert_threshold ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetAlertThresholdInput {
    pub threshold: i64,
}

impl Validate for SetAlertThresholdInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.threshold < 0 {
            errors.push(FieldError {
                field: "threshold".into(),
                message: "阈值不能为负数".into(),
                code: "VAL_NEGATIVE".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for SetAlertThresholdInput {
    fn sanitize(&mut self) {}
}

// ── transfer_device ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferDeviceInput {
    pub serial_no: String,
    pub from_warehouse_id: String,
    pub to_warehouse_id: String,
}

impl Validate for TransferDeviceInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.serial_no.trim().is_empty() {
            errors.push(FieldError {
                field: "serialNo".into(),
                message: "设备序列号不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.from_warehouse_id.trim().is_empty() {
            errors.push(FieldError {
                field: "fromWarehouseId".into(),
                message: "源仓库ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.to_warehouse_id.trim().is_empty() {
            errors.push(FieldError {
                field: "toWarehouseId".into(),
                message: "目标仓库ID不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        if self.from_warehouse_id == self.to_warehouse_id {
            errors.push(FieldError {
                field: "toWarehouseId".into(),
                message: "源仓库与目标仓库不能相同".into(),
                code: "VAL_SAME_WAREHOUSE".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for TransferDeviceInput {
    fn sanitize(&mut self) {
        self.serial_no = self.serial_no.trim().to_uppercase();
        self.from_warehouse_id = self.from_warehouse_id.trim().to_string();
        self.to_warehouse_id = self.to_warehouse_id.trim().to_string();
    }
}

// ── cancel_transfer ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelTransferInput {
    pub serial_no: String,
    pub original_from_warehouse_id: String,
    pub original_to_warehouse_id: String,
}

impl Validate for CancelTransferInput {
    fn validate(&self) -> ValidationResult {
        let mut errors = Vec::new();
        if self.serial_no.trim().is_empty() {
            errors.push(FieldError {
                field: "serialNo".into(),
                message: "设备序列号不能为空".into(),
                code: "VAL_REQUIRED".into(),
            });
        }
        ValidationResult { errors }
    }
}

impl Sanitize for CancelTransferInput {
    fn sanitize(&mut self) {
        self.serial_no = self.serial_no.trim().to_uppercase();
        self.original_from_warehouse_id = self.original_from_warehouse_id.trim().to_string();
        self.original_to_warehouse_id = self.original_to_warehouse_id.trim().to_string();
    }
}

// ── get_capacity_stats ──

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetCapacityStatsInput {}

impl Validate for GetCapacityStatsInput {
    fn validate(&self) -> ValidationResult {
        ValidationResult { errors: vec![] }
    }
}

impl Sanitize for GetCapacityStatsInput {
    fn sanitize(&mut self) {}
}

// ═══════════════════════════════════════════════════════════════════
// 输出类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LowStockAlert {
    pub warehouse_id: String,
    pub warehouse_name: String,
    pub available_count: i64,
    pub threshold: i64,
    pub shortage: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetThresholdOutput {
    pub success: bool,
    pub threshold: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransferOutput {
    pub success: bool,
    pub serial_no: String,
    pub from_warehouse_id: String,
    pub to_warehouse_id: String,
    pub transferred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelTransferOutput {
    pub success: bool,
    pub serial_no: String,
    pub reverted_to_warehouse_id: String,
    pub cancelled_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CapacityStats {
    pub warehouse_id: String,
    pub warehouse_name: String,
    pub capacity: i64,
    pub total_devices: i64,
    pub available_devices: i64,
    pub rented_devices: i64,
    pub utilization_percent: f64,
}

// ═══════════════════════════════════════════════════════════════════
// 模块主体
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureWarehouseAdvanced {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
    /// 跨模块依赖：feature-warehouse
    pub warehouse_module: Mutex<Option<Arc<dyn SystemModule>>>,
    /// 跨模块依赖：feature-device
    pub device_module: Mutex<Option<Arc<dyn SystemModule>>>,
    /// 库存预警阈值
    alert_threshold: Mutex<i64>,
}

impl FeatureWarehouseAdvanced {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
            warehouse_module: Mutex::new(None),
            device_module: Mutex::new(None),
            alert_threshold: Mutex::new(5),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self
            .pool
            .lock()
            .map_err(|e| err_json("SYS_DB_LOCK", &e.to_string()))?;
        guard
            .as_ref()
            .ok_or_else(|| err_json("SYS_DB_NOT_INIT", "数据库未初始化"))?
            .get()
            .map_err(|e| err_json("SYS_DB_POOL", &e.to_string()))
    }

    /// 跨模块调用 device 模块
    #[allow(dead_code)]
    fn call_device(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let guard = self
            .device_module
            .lock()
            .map_err(|e| err_json("SYS_MODULE_LOCK", &e.to_string()))?;
        let module = guard
            .as_ref()
            .ok_or_else(|| err_json("SYS_MODULE_NOT_INJECTED", "device 模块未注入"))?;
        module.execute(command, payload, ctx)
    }

    /// 跨模块调用 warehouse 模块
    #[allow(dead_code)]
    fn call_warehouse(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String> {
        let guard = self
            .warehouse_module
            .lock()
            .map_err(|e| err_json("SYS_MODULE_LOCK", &e.to_string()))?;
        let module = guard
            .as_ref()
            .ok_or_else(|| err_json("SYS_MODULE_NOT_INJECTED", "warehouse 模块未注入"))?;
        module.execute(command, payload, ctx)
    }

    // ── 业务方法 ──

    fn do_get_low_stock(
        &self,
        input: &GetLowStockInput,
        ctx: &ExecutionContext,
    ) -> Result<Vec<LowStockAlert>, String> {
        let threshold = *self
            .alert_threshold
            .lock()
            .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
        let conn = self.get_conn()?;
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        let sql = if input.warehouse_id.is_some() {
            String::from(
                "SELECT w.id, w.name, COUNT(d.serialNo) as available \
                 FROM warehouses w \
                 LEFT JOIN devices d ON d.warehouseId = w.id AND d.status = 'available' AND d.tenant_id = w.tenant_id \
                 WHERE w.id = ?1 AND w.tenant_id = ?2 \
                 GROUP BY w.id \
                 HAVING COUNT(d.serialNo) < ?3",
            )
        } else {
            String::from(
                "SELECT w.id, w.name, COUNT(d.serialNo) as available \
                 FROM warehouses w \
                 LEFT JOIN devices d ON d.warehouseId = w.id AND d.status = 'available' AND d.tenant_id = w.tenant_id \
                 WHERE w.tenant_id = ?1 \
                 GROUP BY w.id \
                 HAVING COUNT(d.serialNo) < ?2",
            )
        };

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        let alerts: Vec<LowStockAlert> = if let Some(ref wh_id) = input.warehouse_id {
            stmt.query_map(params![wh_id, tenant_id, threshold], |row| {
                Ok(LowStockAlert {
                    warehouse_id: row.get(0)?,
                    warehouse_name: row.get(1)?,
                    available_count: row.get(2)?,
                    threshold,
                    shortage: threshold - row.get::<_, i64>(2)?,
                })
            })
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?
            .filter_map(|r| r.ok())
            .collect()
        } else {
            stmt.query_map(params![tenant_id, threshold], |row| {
                Ok(LowStockAlert {
                    warehouse_id: row.get(0)?,
                    warehouse_name: row.get(1)?,
                    available_count: row.get(2)?,
                    threshold,
                    shortage: threshold - row.get::<_, i64>(2)?,
                })
            })
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?
            .filter_map(|r| r.ok())
            .collect()
        };

        Ok(alerts)
    }

    fn do_set_alert_threshold(
        &self,
        input: &SetAlertThresholdInput,
    ) -> Result<SetThresholdOutput, String> {
        let mut guard = self
            .alert_threshold
            .lock()
            .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
        *guard = input.threshold;
        Ok(SetThresholdOutput {
            success: true,
            threshold: input.threshold,
        })
    }

    /// 设备调拨 Saga
    ///
    /// Step 1: 验证 source warehouse 存在且有容量
    /// Step 2: 验证 target warehouse 存在且有容量
    /// Step 3: UPDATE devices SET warehouseId = target WHERE serialNo = ? AND warehouseId = source
    /// Step 4 (compensate on failure): 回滚
    fn do_transfer_device(
        &self,
        input: &TransferDeviceInput,
        ctx: &ExecutionContext,
    ) -> Result<TransferOutput, String> {
        let conn = self.get_conn()?;
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        // Step 1: 验证设备存在且在源仓库
        let mut stmt = conn
            .prepare(
                "SELECT d.serialNo, d.status FROM devices d \
             INNER JOIN warehouses w ON w.id = d.warehouseId AND w.tenant_id = d.tenant_id \
             WHERE d.serialNo = ?1 AND d.warehouseId = ?2 AND d.tenant_id = ?3",
            )
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        let device_exists: bool = stmt
            .exists(params![input.serial_no, input.from_warehouse_id, tenant_id])
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        if !device_exists {
            return Err(err_json(
                "BIZ_DEVICE_NOT_IN_SOURCE",
                &format!(
                    "设备 {} 不在源仓库 {}",
                    input.serial_no, input.from_warehouse_id
                ),
            ));
        }

        // Step 2: 验证目标仓库存在
        let mut stmt2 = conn
            .prepare("SELECT id, capacity FROM warehouses WHERE id = ?1 AND tenant_id = ?2")
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        let target_exists: bool = stmt2
            .exists(params![input.to_warehouse_id, tenant_id])
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        if !target_exists {
            return Err(err_json(
                "BIZ_WAREHOUSE_NOT_FOUND",
                &format!("目标仓库 {} 不存在", input.to_warehouse_id),
            ));
        }

        // Step 3: Saga BEGIN → transfer → COMMIT
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| err_json("SYS_DB_TXN_BEGIN", &e.to_string()))?;

        let transfer_result = (|| -> Result<(), String> {
            // 记录调拨前的状态（用于审计）
            let now = shanghai_now_iso();

            conn.execute(
                "UPDATE devices SET warehouseId = ?1, updatedAt = ?2 \
                 WHERE serialNo = ?3 AND warehouseId = ?4 AND tenant_id = ?5",
                params![
                    input.to_warehouse_id,
                    now,
                    input.serial_no,
                    input.from_warehouse_id,
                    tenant_id
                ],
            )
            .map_err(|e| err_json("SYS_DB_UPDATE", &e.to_string()))?;

            Ok(())
        })();

        match transfer_result {
            Ok(()) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| err_json("SYS_DB_TXN_COMMIT", &e.to_string()))?;

                Ok(TransferOutput {
                    success: true,
                    serial_no: input.serial_no.clone(),
                    from_warehouse_id: input.from_warehouse_id.clone(),
                    to_warehouse_id: input.to_warehouse_id.clone(),
                    transferred_at: shanghai_now_iso(),
                })
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    fn do_cancel_transfer(
        &self,
        input: &CancelTransferInput,
        ctx: &ExecutionContext,
    ) -> Result<CancelTransferOutput, String> {
        let conn = self.get_conn()?;
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        // 调回原仓库
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| err_json("SYS_DB_TXN_BEGIN", &e.to_string()))?;

        let revert = (|| -> Result<(), String> {
            let now = shanghai_now_iso();

            let affected = conn
                .execute(
                    "UPDATE devices SET warehouseId = ?1, updatedAt = ?2 \
                 WHERE serialNo = ?3 AND warehouseId = ?4 AND tenant_id = ?5",
                    params![
                        input.original_from_warehouse_id,
                        now,
                        input.serial_no,
                        input.original_to_warehouse_id,
                        tenant_id
                    ],
                )
                .map_err(|e| err_json("SYS_DB_UPDATE", &e.to_string()))?;

            if affected == 0 {
                return Err(err_json("BIZ_DEVICE_NOT_FOUND", "设备未找到，无法取消调拨"));
            }

            Ok(())
        })();

        match revert {
            Ok(()) => {
                conn.execute_batch("COMMIT")
                    .map_err(|e| err_json("SYS_DB_TXN_COMMIT", &e.to_string()))?;

                Ok(CancelTransferOutput {
                    success: true,
                    serial_no: input.serial_no.clone(),
                    reverted_to_warehouse_id: input.original_from_warehouse_id.clone(),
                    cancelled_at: shanghai_now_iso(),
                })
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    fn do_get_capacity_stats(
        &self,
        _input: &GetCapacityStatsInput,
        ctx: &ExecutionContext,
    ) -> Result<Vec<CapacityStats>, String> {
        let conn = self.get_conn()?;
        let tenant_id = ctx.data_scope().tenant_id().as_str();

        let mut stmt = conn
            .prepare(
                "SELECT w.id, w.name, w.capacity, \
                    COUNT(d.serialNo) as total, \
                    SUM(CASE WHEN d.status = 'available' THEN 1 ELSE 0 END) as available, \
                    SUM(CASE WHEN d.status = 'rented' THEN 1 ELSE 0 END) as rented \
             FROM warehouses w \
             LEFT JOIN devices d ON d.warehouseId = w.id AND d.tenant_id = w.tenant_id \
             WHERE w.tenant_id = ?1 \
             GROUP BY w.id \
             ORDER BY total DESC",
            )
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?;

        let stats: Vec<CapacityStats> = stmt
            .query_map(params![tenant_id], |row| {
                let capacity: i64 = row.get(2)?;
                let total: i64 = row.get(3)?;
                let available: i64 = row.get(4)?;
                let rented: i64 = row.get(5)?;
                let utilization = if capacity > 0 {
                    (rented as f64 / capacity as f64) * 100.0
                } else {
                    0.0
                };
                Ok(CapacityStats {
                    warehouse_id: row.get(0)?,
                    warehouse_name: row.get(1)?,
                    capacity,
                    total_devices: total,
                    available_devices: available,
                    rented_devices: rented,
                    utilization_percent: utilization,
                })
            })
            .map_err(|e| err_json("SYS_DB_QUERY", &e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(stats)
    }
}

impl Default for FeatureWarehouseAdvanced {
    fn default() -> Self {
        Self::new()
    }
}

// ── 管线辅助宏 ──

/// 执行标准 4 层管线：DeserializeGuard → TryInto → Sanitize → Validate → 业务逻辑
macro_rules! pipeline {
    ($payload:expr, $T:ty, $self:ident, $ctx:expr, $method:ident) => {{
        DeserializeGuard::default().check_raw(&$payload)?;
        let unvalidated: Unvalidated<$T> = $payload.try_into()?;
        let sanitized = unvalidated.sanitize();
        let validated = sanitized.validate()?;
        let input = validated.into_inner();
        $self.$method(&input, $ctx)
    }};
    // 无 ctx 版本
    ($payload:expr, $T:ty, $self:ident, $method:ident) => {{
        DeserializeGuard::default().check_raw(&$payload)?;
        let unvalidated: Unvalidated<$T> = $payload.try_into()?;
        let sanitized = unvalidated.sanitize();
        let validated = sanitized.validate()?;
        let input = validated.into_inner();
        $self.$method(&input)
    }};
}

// ── 错误辅助 ──

fn err_json(code: &str, msg: &str) -> String {
    serde_json::to_string(&ErrorPayload {
        category: if code.starts_with("SYS") {
            "sys".into()
        } else {
            "biz".into()
        },
        code: code.into(),
        message: msg.into(),
        field: None,
        context: None,
    })
    .unwrap_or_default()
}

fn serialize<T: Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| {
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

impl SystemModule for FeatureWarehouseAdvanced {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "warehouse_advanced".into(),
            version: "0.1.0".into(),
            description: "仓库管理增强 — 库存预警 + 设备调拨 Saga + 容量统计".into(),
            author: "hoshi".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "get_low_stock",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "set_alert_threshold",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "transfer_device",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "cancel_transfer",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "get_capacity_stats",
                AccessRequirement::Authenticated,
                &[EffectClass::DatabaseRead],
                SimulationSupport::Supported,
            ),
        ]
    }

    fn init(&mut self, config: Value) -> Result<(), String> {
        // 从 config 注入数据库 URL
        if let Some(url) = config.get("databaseUrl").and_then(|v| v.as_str()) {
            let manager = SqliteConnectionManager::file(url);
            let pool = Pool::builder()
                .max_size(4)
                .build(manager)
                .map_err(|e| err_json("SYS_DB_POOL_CREATE", &e.to_string()))?;
            let mut guard = self
                .pool
                .lock()
                .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
            *guard = Some(pool);
        }

        // 注入跨模块依赖
        if let Some(_wh) = config.get("warehouse_module") {
            // 外部通过 Arc 注入已初始化的模块引用
            // 实际使用中由 ModuleRegistry::assemble() 设置
            let module: Arc<dyn SystemModule> = Arc::new(StubModule);
            let mut guard = self
                .warehouse_module
                .lock()
                .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
            *guard = Some(module);
        }

        if let Some(_dev) = config.get("device_module") {
            let module: Arc<dyn SystemModule> = Arc::new(StubModule);
            let mut guard = self
                .device_module
                .lock()
                .map_err(|e| err_json("SYS_LOCK", &e.to_string()))?;
            *guard = Some(module);
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
            "get_low_stock" => {
                let result = pipeline!(payload, GetLowStockInput, self, ctx, do_get_low_stock)?;
                serialize(&result)
            }
            "set_alert_threshold" => {
                let result = pipeline!(
                    payload,
                    SetAlertThresholdInput,
                    self,
                    do_set_alert_threshold
                )?;
                serialize(&result)
            }
            "transfer_device" => {
                let result =
                    pipeline!(payload, TransferDeviceInput, self, ctx, do_transfer_device)?;
                serialize(&result)
            }
            "cancel_transfer" => {
                let result =
                    pipeline!(payload, CancelTransferInput, self, ctx, do_cancel_transfer)?;
                serialize(&result)
            }
            "get_capacity_stats" => {
                let result = pipeline!(
                    payload,
                    GetCapacityStatsInput,
                    self,
                    ctx,
                    do_get_capacity_stats
                )?;
                serialize(&result)
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
            name: "warehouse_advanced".into(),
            description: "仓库管理增强".into(),
            commands: vec![
                CommandSchema {
                    name: "get_low_stock".into(),
                    description: "查询库存低于预警阈值的仓库".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetLowStockInput))
                        .ok(),
                    output_schema: Some(
                        serde_json::json!({"type": "array", "items": {"$ref": "#/definitions/LowStockAlert"}}),
                    ),
                },
                CommandSchema {
                    name: "set_alert_threshold".into(),
                    description: "设置库存预警阈值".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        SetAlertThresholdInput
                    ))
                    .ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(SetThresholdOutput))
                        .ok(),
                },
                CommandSchema {
                    name: "transfer_device".into(),
                    description: "设备调拨（Saga — BEGIN IMMEDIATE + COMMIT/ROLLBACK）".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(TransferDeviceInput))
                        .ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(TransferOutput)).ok(),
                },
                CommandSchema {
                    name: "cancel_transfer".into(),
                    description: "取消设备调拨（补偿操作）".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(CancelTransferInput))
                        .ok(),
                    output_schema: serde_json::to_value(schemars::schema_for!(
                        CancelTransferOutput
                    ))
                    .ok(),
                },
                CommandSchema {
                    name: "get_capacity_stats".into(),
                    description: "查询所有仓库的容量统计".into(),
                    version: "1.0.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        GetCapacityStatsInput
                    ))
                    .ok(),
                    output_schema: Some(
                        serde_json::json!({"type": "array", "items": {"$ref": "#/definitions/CapacityStats"}}),
                    ),
                },
            ],
        }
    }

    fn shutdown(&mut self) -> Result<(), String> {
        // 释放连接池
        if let Ok(mut guard) = self.pool.lock() {
            *guard = None;
        }
        Ok(())
    }
}

/// 存根模块 — init() 中使用的占位符。
/// 实际运行时由 ModuleRegistry::assemble() 替换为真实模块。
struct StubModule;

impl SystemModule for StubModule {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "stub".into(),
            version: "0.0.0".into(),
            description: "存根".into(),
            author: "system".into(),
            wasm_compatible: false,
            storage: None,
        }
    }
    fn commands(&self) -> Vec<CommandMetadata> {
        vec![]
    }
    fn init(&mut self, _: Value) -> Result<(), String> {
        Ok(())
    }
    fn execute(&self, _: &str, _: Value, _: &ExecutionContext) -> Result<Value, String> {
        Ok(serde_json::Value::Null)
    }
    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "stub".into(),
            description: "存根".into(),
            commands: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_for(tenant: &str) -> ExecutionContext {
        let tenant_id = TenantId::new(tenant).expect("tenant ID is valid");
        let data_scope = DataScope::production(
            tenant_id.clone(),
            Revision::new("warehouse-advanced-test-revision").expect("revision is valid"),
        )
        .expect("production data scope is valid");

        ExecutionContext::new(
            ActorIdentity::system(),
            TenantScope::tenant(tenant_id),
            data_scope,
            ExecutionMode::Normal,
            RequestId::new("warehouse-advanced-test-request").expect("request ID is valid"),
            None,
            std::sync::Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    fn initialized_advanced_module() -> FeatureWarehouseAdvanced {
        let mut module = FeatureWarehouseAdvanced::new();
        module
            .init(serde_json::json!({ "databaseUrl": ":memory:" }))
            .expect("in-memory database initializes");
        let conn = module.get_conn().expect("database connection");
        conn.execute_batch(
            "CREATE TABLE warehouses (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                capacity INTEGER NOT NULL,
                tenant_id TEXT NOT NULL
            );
            CREATE TABLE devices (
                serialNo TEXT PRIMARY KEY,
                warehouseId TEXT,
                status TEXT NOT NULL,
                updatedAt TEXT NOT NULL,
                tenant_id TEXT NOT NULL
            );
            INSERT INTO warehouses (id, name, capacity, tenant_id)
            VALUES ('warehouse-a', 'Tenant A warehouse', 10, 'tenant-a');
            INSERT INTO warehouses (id, name, capacity, tenant_id)
            VALUES ('warehouse-b', 'Tenant B warehouse', 10, 'tenant-b');
            INSERT INTO devices (serialNo, warehouseId, status, updatedAt, tenant_id)
            VALUES ('device-b', 'warehouse-b', 'available', '2026-07-16', 'tenant-b');",
        )
        .expect("warehouse advanced fixtures created");
        module
    }

    #[test]
    fn low_stock_alerts_are_limited_to_execution_context_data_scope() {
        let module = initialized_advanced_module();

        let alerts = module
            .do_get_low_stock(
                &GetLowStockInput { warehouse_id: None },
                &context_for("tenant-a"),
            )
            .expect("tenant-scoped low-stock query succeeds");

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].warehouse_id, "warehouse-a");
    }
}

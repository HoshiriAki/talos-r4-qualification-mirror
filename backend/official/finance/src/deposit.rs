//! feature-deposit — 押金子模块
//!
//! 命令:
//! - calculate — 根据订单设备数量计算建议押金
//! - collect   — 记录押金收款 (pending→paid)
//! - release   — 释放押金 (paid→released, 扣除已罚没部分)
//! - forfeit   — 罚没押金 (全额或部分, 含原因)
//! - get       — 按 order_id 查询押金明细

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
pub struct CalculateDepositInput {
    pub order_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollectDepositInput {
    pub order_id: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseDepositInput {
    pub order_id: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForfeitDepositInput {
    pub order_id: String,
    pub amount: f64,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetDepositInput {
    pub order_id: String,
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureDeposit {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureDeposit {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureDeposit {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: deposit pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    /// 默认每台设备的押金额 (元) — 后续可迁移为 device_models 字段
    const DEFAULT_DEPOSIT_PER_DEVICE: f64 = 2000.0;

    // ── 业务方法 ─────────────────────────────────────────────────

    fn do_calculate(
        &self,
        scope: &DataScope,
        input: &CalculateDepositInput,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        // 1. 确认订单存在
        let order_exists: bool = conn
            .query_row(
                "SELECT 1 FROM orders WHERE id = ?1 AND tenant_id = ?2",
                params![input.order_id, tenant_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .is_some();

        if !order_exists {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ORDER_NOT_FOUND".into(),
                message: format!("订单 {} 不存在", input.order_id),
                field: Some("orderId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        // 2. 是否已有押金记录
        let existing: Option<(String, f64, String)> = conn
            .query_row(
                "SELECT id, amount, status FROM deposits WHERE order_id = ?1 AND tenant_id = ?2",
                params![input.order_id, tenant_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        if let Some((deposit_id, amount, status)) = existing {
            return Ok(serde_json::json!({
                "ok": true,
                "depositId": deposit_id,
                "orderId": input.order_id,
                "amount": amount,
                "status": status,
                "existing": true,
            }));
        }

        // 3. 统计设备数
        let device_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM order_devices od JOIN devices d ON d.deviceSerialNo = od.deviceSerialNo WHERE od.orderId = ?1 AND d.tenant_id = ?2",
                params![input.order_id, tenant_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let amount = if device_count > 0 {
            device_count as f64 * Self::DEFAULT_DEPOSIT_PER_DEVICE
        } else {
            Self::DEFAULT_DEPOSIT_PER_DEVICE // 最小值: 一台设备
        };

        // 4. 创建押金记录 (pending)
        let deposit_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO deposits (id, order_id, amount, status, created_at, updated_at, tenant_id) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6)",
            params![deposit_id, input.order_id, amount, now, now, tenant_id],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "depositId": deposit_id,
            "orderId": input.order_id,
            "amount": amount,
            "status": "pending",
            "deviceCount": device_count,
            "existing": false,
        }))
    }

    fn do_collect(
        &self,
        scope: &DataScope,
        input: &CollectDepositInput,
        operator: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "押金金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let order_exists = conn
            .query_row(
                "SELECT 1 FROM orders WHERE id = ?1 AND tenant_id = ?2",
                params![input.order_id, tenant_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .is_some();
        if !order_exists {
            return Err("BIZ_ORDER_NOT_FOUND: order is outside the execution data scope".into());
        }

        // BEGIN IMMEDIATE — 防 TOCTOU
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| format!("SYS_DB_TXN: {}", e))?;

        let result = (|| -> Result<Value, String> {
            // 查找或创建 deposit
            let (deposit_id, current_status, current_amount): (String, String, f64) = conn
                .query_row(
                    "SELECT id, status, amount FROM deposits WHERE order_id = ?1 AND tenant_id = ?2",
                    params![input.order_id, tenant_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
                .unwrap_or_else(|| {
                    let id = Uuid::new_v4().to_string();
                    conn.execute(
                        "INSERT INTO deposits (id, order_id, amount, status, created_at, updated_at, tenant_id) VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6)",
                        params![id, input.order_id, 0.0_f64, &now, &now, tenant_id],
                    ).ok();
                    (id, "pending".to_string(), 0.0_f64)
                });

            if current_status == "paid" || current_status == "released" {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_DEPOSIT_ALREADY_PAID".into(),
                    message: "押金已缴纳或已释放，不可重复收款".into(),
                    field: Some("orderId".into()),
                    context: None,
                })
                .unwrap_or_default());
            }

            let new_amount = if current_amount > 0.0 {
                current_amount
            } else {
                input.amount
            };

            conn.execute(
                "UPDATE deposits SET amount = ?1, status = 'paid', paid_at = ?2, updated_at = ?3 WHERE id = ?4 AND tenant_id = ?5",
                params![new_amount, now, now, deposit_id, tenant_id],
            ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

            conn.execute(
                "INSERT INTO deposit_ledger (id, deposit_id, order_id, entry_type, amount, balance_after, description, operator, created_at, tenant_id) \
                 VALUES (?1, ?2, ?3, 'collect', ?4, ?5, ?6, ?7, ?8, ?9)",
                params![Uuid::new_v4().to_string(), deposit_id, input.order_id, new_amount, new_amount,
                        format!("收取押金 ¥{:.2}", new_amount), operator, now, tenant_id],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

            // Accounting entries: collect → debit:cash, credit:deposit_liability
            let ae1 = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
                 VALUES (?1, ?2, 'debit', 'cash', ?3, ?4, ?5, ?6)",
                params![ae1, input.order_id, new_amount, format!("押金收款 ¥{:.2}", new_amount), now, tenant_id],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;
            let ae2 = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
                 VALUES (?1, ?2, 'credit', 'deposit', ?3, ?4, ?5, ?6)",
                params![ae2, input.order_id, new_amount, format!("押金负债 ¥{:.2}", new_amount), now, tenant_id],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

            Ok(serde_json::json!({
                "ok": true,
                "depositId": deposit_id,
                "orderId": input.order_id,
                "amount": new_amount,
                "status": "paid",
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

    fn do_release(
        &self,
        scope: &DataScope,
        input: &ReleaseDepositInput,
        operator: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| format!("SYS_DB_TXN: {}", e))?;

        let result = (|| -> Result<Value, String> {
            let (deposit_id, status, total_amount, released_at): (String, String, f64, Option<String>) = conn
                .query_row(
                    "SELECT id, status, amount, released_at FROM deposits WHERE order_id = ?1 AND tenant_id = ?2",
                    params![input.order_id, tenant_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .map_err(|_| serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_DEPOSIT_NOT_FOUND".into(),
                    message: format!("订单 {} 没有押金记录", input.order_id),
                    field: Some("orderId".into()),
                    context: None,
                }).unwrap_or_default())?;

            // 防双重释放
            if released_at.is_some() {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_DEPOSIT_ALREADY_RELEASED".into(),
                    message: "押金已释放，不可重复操作".into(),
                    field: Some("orderId".into()),
                    context: None,
                })
                .unwrap_or_default());
            }

            if status != "paid" && status != "partially_forfeited" {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_DEPOSIT_NOT_PAID".into(),
                    message: format!("押金状态为 {}，不可释放", status),
                    field: Some("orderId".into()),
                    context: None,
                })
                .unwrap_or_default());
            }

            let forfeited_total: f64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(amount), 0) FROM deposit_ledger WHERE deposit_id = ?1 AND tenant_id = ?2 AND entry_type = 'forfeit'",
                    params![deposit_id, tenant_id],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            let release_amount = total_amount - forfeited_total;

            // 始终设为 released（罚没信息已在 ledger 中）
            conn.execute(
                "UPDATE deposits SET status = 'released', released_at = ?1, updated_at = ?2 WHERE id = ?3 AND tenant_id = ?4",
                params![now, now, deposit_id, tenant_id],
            ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

            let desc = if !input.reason.is_empty() {
                format!("释放押金 ¥{:.2} — {}", release_amount, input.reason)
            } else {
                format!("释放押金 ¥{:.2}", release_amount)
            };
            conn.execute(
                "INSERT INTO deposit_ledger (id, deposit_id, order_id, entry_type, amount, balance_after, description, operator, created_at, tenant_id) \
                 VALUES (?1, ?2, ?3, 'release', ?4, ?5, ?6, ?7, ?8, ?9)",
                params![Uuid::new_v4().to_string(), deposit_id, input.order_id,
                        release_amount, 0.0_f64, desc, operator, now, tenant_id],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

            // Accounting entries: release → debit:deposit, credit:cash
            let ae1 = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
                 VALUES (?1, ?2, 'debit', 'deposit', ?3, ?4, ?5, ?6)",
                params![ae1, input.order_id, release_amount, format!("押金释放 ¥{:.2}", release_amount), now, tenant_id],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;
            let ae2 = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
                 VALUES (?1, ?2, 'credit', 'cash', ?3, ?4, ?5, ?6)",
                params![ae2, input.order_id, release_amount, format!("押金退回 ¥{:.2}", release_amount), now, tenant_id],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

            Ok(serde_json::json!({
                "ok": true,
                "depositId": deposit_id,
                "orderId": input.order_id,
                "releasedAmount": release_amount,
                "forfeitedAmount": forfeited_total,
                "status": "released",
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

    fn do_forfeit(
        &self,
        scope: &DataScope,
        input: &ForfeitDepositInput,
        operator: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "罚没金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| format!("SYS_DB_TXN: {}", e))?;

        let result = (|| -> Result<Value, String> {
            let (deposit_id, status, total_amount): (String, String, f64) = conn
                .query_row(
                    "SELECT id, status, amount FROM deposits WHERE order_id = ?1 AND tenant_id = ?2",
                    params![input.order_id, tenant_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|_| serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_DEPOSIT_NOT_FOUND".into(),
                    message: format!("订单 {} 没有押金记录", input.order_id),
                    field: Some("orderId".into()),
                    context: None,
                }).unwrap_or_default())?;

            if status != "paid" && status != "partially_forfeited" {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_DEPOSIT_NOT_PAID".into(),
                    message: format!("押金状态为 {}，不可罚没", status),
                    field: Some("orderId".into()),
                    context: None,
                })
                .unwrap_or_default());
            }

            let already_forfeited: f64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(amount), 0) FROM deposit_ledger WHERE deposit_id = ?1 AND tenant_id = ?2 AND entry_type = 'forfeit'",
                    params![deposit_id, tenant_id],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            let available = total_amount - already_forfeited;
            if input.amount > available {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_EXCEEDS_AVAILABLE".into(),
                    message: format!(
                        "罚没金额 ¥{:.2} 超过可用余额 ¥{:.2}",
                        input.amount, available
                    ),
                    field: Some("amount".into()),
                    context: None,
                })
                .unwrap_or_default());
            }

            let new_total_forfeited = already_forfeited + input.amount;
            let new_balance = total_amount - new_total_forfeited;
            let new_status = if new_balance <= 0.01 {
                "forfeited"
            } else {
                "partially_forfeited"
            };

            conn.execute(
                "UPDATE deposits SET status = ?1, forfeited_at = ?2, updated_at = ?3 WHERE id = ?4 AND tenant_id = ?5",
                params![new_status, now, now, deposit_id, tenant_id],
            ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

            let desc = if !input.reason.is_empty() {
                format!("罚没押金 ¥{:.2} — {}", input.amount, input.reason)
            } else {
                format!("罚没押金 ¥{:.2}", input.amount)
            };
            conn.execute(
                "INSERT INTO deposit_ledger (id, deposit_id, order_id, entry_type, amount, balance_after, description, operator, created_at, tenant_id) \
                 VALUES (?1, ?2, ?3, 'forfeit', ?4, ?5, ?6, ?7, ?8, ?9)",
                params![Uuid::new_v4().to_string(), deposit_id, input.order_id,
                        input.amount, new_balance, desc, operator, now, tenant_id],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

            // Accounting entries: forfeit → debit:deposit, credit:revenue
            let ae1 = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
                 VALUES (?1, ?2, 'debit', 'deposit', ?3, ?4, ?5, ?6)",
                params![ae1, input.order_id, input.amount, format!("押金罚没负债减少 ¥{:.2}", input.amount), now, tenant_id],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;
            let ae2 = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
                 VALUES (?1, ?2, 'credit', 'revenue', ?3, ?4, ?5, ?6)",
                params![ae2, input.order_id, input.amount, format!("押金罚没收入 ¥{:.2}", input.amount), now, tenant_id],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

            Ok(serde_json::json!({
                "ok": true,
                "depositId": deposit_id,
                "orderId": input.order_id,
                "forfeitedAmount": input.amount,
                "totalForfeited": new_total_forfeited,
                "remainingBalance": new_balance,
                "status": new_status,
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

    fn do_get(&self, scope: &DataScope, input: &GetDepositInput) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let tenant_id = scope.tenant_id().as_str();

        let deposit: Option<Value> = conn
            .query_row(
                "SELECT id, order_id, amount, status, paid_at, released_at, forfeited_at, created_at, updated_at \
                 FROM deposits WHERE order_id = ?1 AND tenant_id = ?2",
                params![input.order_id, tenant_id],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "orderId": row.get::<_, String>(1)?,
                        "amount": row.get::<_, f64>(2)?,
                        "status": row.get::<_, String>(3)?,
                        "paidAt": row.get::<_, Option<String>>(4)?,
                        "releasedAt": row.get::<_, Option<String>>(5)?,
                        "forfeitedAt": row.get::<_, Option<String>>(6)?,
                        "createdAt": row.get::<_, String>(7)?,
                        "updatedAt": row.get::<_, String>(8)?,
                    }))
                },
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        match deposit {
            Some(d) => {
                let deposit_id = d["id"].as_str().unwrap_or("");
                // Ledger entries
                let mut stmt = conn.prepare(
                    "SELECT entry_type, amount, balance_after, description, operator, created_at \
                     FROM deposit_ledger WHERE deposit_id = ?1 AND tenant_id = ?2 ORDER BY created_at ASC"
                ).map_err(|e| format!("SYS_DB_QUERY: {}", e))?;
                let ledger: Vec<Value> = stmt
                    .query_map(params![deposit_id, tenant_id], |row| {
                        Ok(serde_json::json!({
                            "entryType": row.get::<_, String>(0)?,
                            "amount": row.get::<_, f64>(1)?,
                            "balanceAfter": row.get::<_, f64>(2)?,
                            "description": row.get::<_, String>(3)?,
                            "operator": row.get::<_, String>(4)?,
                            "createdAt": row.get::<_, String>(5)?,
                        }))
                    })
                    .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
                    .filter_map(|r| r.ok())
                    .collect();

                Ok(serde_json::json!({ "ok": true, "deposit": d, "ledger": ledger }))
            }
            None => Ok(serde_json::json!({ "ok": true, "deposit": null, "ledger": [] })),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// SystemModule trait
// ═══════════════════════════════════════════════════════════════════

impl SystemModule for FeatureDeposit {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "deposit".into(),
            version: "0.1.0".into(),
            description: "押金管理 — 计算/收取/释放/罚没".into(),
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
                &[EffectClass::DatabaseRead, EffectClass::DatabaseWrite],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "collect",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite, EffectClass::Payment],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "release",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite, EffectClass::Payment],
                SimulationSupport::Blocked,
            ),
            CommandMetadata::new(
                "forfeit",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite, EffectClass::Payment],
                SimulationSupport::Blocked,
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
        let operator = ctx.user_id().unwrap_or("system");

        match command {
            "calculate" => {
                let input: CalculateDepositInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_calculate(ctx.data_scope(), &input)
            }
            "collect" => {
                let input: CollectDepositInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_collect(ctx.data_scope(), &input, operator)
            }
            "release" => {
                let input: ReleaseDepositInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_release(ctx.data_scope(), &input, operator)
            }
            "forfeit" => {
                let input: ForfeitDepositInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_forfeit(ctx.data_scope(), &input, operator)
            }
            "get" => {
                let input: GetDepositInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_get(ctx.data_scope(), &input)
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: deposit.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "deposit".into(),
            description: "押金管理 — 计算/收取/释放/罚没".into(),
            commands: vec![
                CommandSchema {
                    name: "calculate".into(),
                    description: "计算订单的建议押金金额".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        CalculateDepositInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "collect".into(),
                    description: "记录押金收款".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(CollectDepositInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "release".into(),
                    description: "释放押金 (全额或扣除罚没后的余额)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ReleaseDepositInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "forfeit".into(),
                    description: "罚没押金 (全额或部分, 含原因)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ForfeitDepositInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get".into(),
                    description: "查询订单的押金明细 + 流水".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetDepositInput)).ok(),
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
            RequestId::new("req").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap()
    }

    #[test]
    fn collect_rejects_an_order_owned_by_another_tenant() {
        let db_path = std::env::temp_dir().join(format!("talos-deposit-{}.db", Uuid::new_v4()));
        let manager = SqliteConnectionManager::file(&db_path);
        let pool = Pool::new(manager).unwrap();
        let conn = pool.get().unwrap();
        conn.execute_batch(
            "CREATE TABLE orders (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL);\
             CREATE TABLE deposits (id TEXT PRIMARY KEY, order_id TEXT NOT NULL, amount REAL NOT NULL, status TEXT NOT NULL, paid_at TEXT, released_at TEXT, forfeited_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, tenant_id TEXT NOT NULL);\
             CREATE TABLE deposit_ledger (id TEXT PRIMARY KEY, deposit_id TEXT, order_id TEXT, entry_type TEXT, amount REAL, balance_after REAL, description TEXT, operator TEXT, created_at TEXT, tenant_id TEXT NOT NULL);\
             CREATE TABLE accounting_entries (id TEXT PRIMARY KEY, order_id TEXT, entry_type TEXT, account TEXT, amount REAL, description TEXT, created_at TEXT, tenant_id TEXT NOT NULL);\
             INSERT INTO orders VALUES ('foreign-order', 'tenant-b');"
        ).unwrap();
        drop(conn);

        let feature = FeatureDeposit {
            pool: Mutex::new(Some(pool)),
        };
        let result = feature.execute(
            "collect",
            serde_json::json!({"orderId":"foreign-order","amount":100}),
            &context("tenant-a"),
        );
        assert!(
            result.is_err(),
            "a tenant must not create a deposit for another tenant's order"
        );
        let _ = std::fs::remove_file(db_path);
    }
}

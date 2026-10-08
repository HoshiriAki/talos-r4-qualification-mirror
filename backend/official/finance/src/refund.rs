//! feature-refund — 退款子模块
//!
//! 命令:
//! - request  — 创建退款申请 (deposit 释放需要审批时)
//! - approve  — 审批通过
//! - reject   — 审批驳回
//! - execute  — 执行退款 (approved→executed)
//! - list     — 按状态/订单查询退款列表

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
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
pub struct RequestRefundInput {
    pub order_id: String,
    pub amount: f64,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApproveRefundInput {
    pub refund_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RejectRefundInput {
    pub refund_id: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteRefundInput {
    pub refund_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListRefundsInput {
    #[serde(default)]
    pub order_id: Option<String>,
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

pub struct FeatureRefund {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureRefund {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureRefund {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: refund pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    fn do_request(
        &self,
        scope: &DataScope,
        input: &RequestRefundInput,
        operator: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "退款金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let deposit_id: String = conn
            .query_row(
                "SELECT id FROM deposits WHERE order_id = ?1 AND tenant_id = ?2",
                params![input.order_id, tenant_id],
                |row| row.get(0),
            )
            .map_err(|_| {
                serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_DEPOSIT_NOT_FOUND".into(),
                    message: format!("订单 {} 没有押金记录", input.order_id),
                    field: Some("orderId".into()),
                    context: None,
                })
                .unwrap_or_default()
            })?;

        let refund_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO refunds (id, deposit_id, order_id, amount, reason, status, requested_by, created_at, updated_at, tenant_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, ?8, ?9)",
            params![refund_id, deposit_id, input.order_id, input.amount, input.reason, operator, now, now, tenant_id],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "refundId": refund_id,
            "depositId": deposit_id,
            "orderId": input.order_id,
            "amount": input.amount,
            "status": "pending",
        }))
    }

    fn do_approve(
        &self,
        scope: &DataScope,
        input: &ApproveRefundInput,
        operator: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        let (_deposit_id, _order_id, status, _amount): (String, String, String, f64) = conn
            .query_row(
                "SELECT deposit_id, order_id, status, amount FROM refunds WHERE id = ?1 AND tenant_id = ?2",
                params![input.refund_id, tenant_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|_| serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REFUND_NOT_FOUND".into(),
                message: format!("退款申请 {} 不存在", input.refund_id),
                field: Some("refundId".into()),
                context: None,
            }).unwrap_or_default())?;

        if status != "pending" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REFUND_NOT_PENDING".into(),
                message: format!("退款申请状态为 {}，不可审批", status),
                field: Some("refundId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute(
            "UPDATE refunds SET status = 'approved', approved_by = ?1, approved_at = ?2, updated_at = ?3 WHERE id = ?4 AND tenant_id = ?5",
            params![operator, now, now, input.refund_id, tenant_id],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "refundId": input.refund_id,
            "status": "approved",
        }))
    }

    fn do_reject(
        &self,
        scope: &DataScope,
        input: &RejectRefundInput,
        operator: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        let status: String = conn
            .query_row(
                "SELECT status FROM refunds WHERE id = ?1 AND tenant_id = ?2",
                params![input.refund_id, tenant_id],
                |row| row.get(0),
            )
            .map_err(|_| {
                serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_REFUND_NOT_FOUND".into(),
                    message: format!("退款申请 {} 不存在", input.refund_id),
                    field: Some("refundId".into()),
                    context: None,
                })
                .unwrap_or_default()
            })?;

        if status != "pending" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REFUND_NOT_PENDING".into(),
                message: format!("退款申请状态为 {}，不可驳回", status),
                field: Some("refundId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute(
            "UPDATE refunds SET status = 'rejected', rejected_by = ?1, rejected_at = ?2, \
             reason = CASE WHEN ?3 != '' THEN reason || ' | 驳回: ' || ?3 ELSE reason END, \
             updated_at = ?4 WHERE id = ?5 AND tenant_id = ?6",
            params![operator, now, input.reason, now, input.refund_id, tenant_id],
        )
        .map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "refundId": input.refund_id,
            "status": "rejected",
        }))
    }

    fn do_execute(
        &self,
        scope: &DataScope,
        input: &ExecuteRefundInput,
        operator: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        let (deposit_id, order_id, status, amount): (String, String, String, f64) = conn
            .query_row(
                "SELECT deposit_id, order_id, status, amount FROM refunds WHERE id = ?1 AND tenant_id = ?2",
                params![input.refund_id, tenant_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|_| serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REFUND_NOT_FOUND".into(),
                message: format!("退款申请 {} 不存在", input.refund_id),
                field: Some("refundId".into()),
                context: None,
            }).unwrap_or_default())?;

        if status != "approved" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_REFUND_NOT_APPROVED".into(),
                message: format!("退款申请状态为 {}，不可执行", status),
                field: Some("refundId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute(
            "UPDATE refunds SET status = 'executed', executed_by = ?1, executed_at = ?2, updated_at = ?3 WHERE id = ?4 AND tenant_id = ?5",
            params![operator, now, now, input.refund_id, tenant_id],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        conn.execute(
            "INSERT INTO deposit_ledger (id, deposit_id, order_id, entry_type, amount, balance_after, description, operator, created_at, tenant_id) \
             VALUES (?1, ?2, ?3, 'refund', ?4, 0, ?5, ?6, ?7, ?8)",
            params![Uuid::new_v4().to_string(), deposit_id, order_id, amount,
                    format!("退款执行 ¥{:.2}", amount), operator, now, tenant_id],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "refundId": input.refund_id,
            "status": "executed",
            "amount": amount,
        }))
    }

    fn do_list(&self, scope: &DataScope, input: &ListRefundsInput) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let tenant_id = scope.tenant_id().as_str();
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let offset = (page - 1) * page_size;

        let mut conditions: Vec<String> = vec!["r.tenant_id = ?1".into()];
        let mut param_values: Vec<String> = vec![tenant_id.to_string()];

        if let Some(ref oid) = input.order_id
            && !oid.is_empty()
        {
            param_values.push(oid.clone());
            conditions.push(format!("r.order_id = ?{}", param_values.len()));
        }
        if let Some(ref s) = input.status
            && !s.is_empty()
        {
            param_values.push(s.clone());
            conditions.push(format!("r.status = ?{}", param_values.len()));
        }

        let where_clause = conditions.join(" AND ");

        let count_sql = format!("SELECT COUNT(*) FROM refunds r WHERE {}", where_clause,);
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();

        let total: i64 = conn
            .query_row(&count_sql, params_ref.as_slice(), |row| row.get(0))
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let data_sql = format!(
            "SELECT r.id, r.deposit_id, r.order_id, r.amount, r.reason, r.status, \
                    r.requested_by, r.approved_by, r.rejected_by, r.executed_by, \
                    r.approved_at, r.rejected_at, r.executed_at, r.created_at \
             FROM refunds r WHERE {} \
             ORDER BY r.created_at DESC LIMIT {} OFFSET {}",
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
                    "depositId": row.get::<_, String>(1)?,
                    "orderId": row.get::<_, String>(2)?,
                    "amount": row.get::<_, f64>(3)?,
                    "reason": row.get::<_, String>(4)?,
                    "status": row.get::<_, String>(5)?,
                    "requestedBy": row.get::<_, String>(6)?,
                    "approvedBy": row.get::<_, Option<String>>(7)?,
                    "rejectedBy": row.get::<_, Option<String>>(8)?,
                    "executedBy": row.get::<_, Option<String>>(9)?,
                    "approvedAt": row.get::<_, Option<String>>(10)?,
                    "rejectedAt": row.get::<_, Option<String>>(11)?,
                    "executedAt": row.get::<_, Option<String>>(12)?,
                    "createdAt": row.get::<_, String>(13)?,
                }))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!({
            "ok": true,
            "refunds": rows,
            "pagination": {
                "page": page,
                "pageSize": page_size,
                "total": total,
            },
        }))
    }
}

impl SystemModule for FeatureRefund {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "refund".into(),
            version: "0.1.0".into(),
            description: "退款审批 — 申请/审批/驳回/执行".into(),
            author: "talos".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "request",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "approve",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "reject",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "execute",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite, EffectClass::Payment],
                SimulationSupport::Blocked,
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
            "request" => {
                let input: RequestRefundInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_request(ctx.data_scope(), &input, operator)
            }
            "approve" => {
                let input: ApproveRefundInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_approve(ctx.data_scope(), &input, operator)
            }
            "reject" => {
                let input: RejectRefundInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_reject(ctx.data_scope(), &input, operator)
            }
            "execute" => {
                let input: ExecuteRefundInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_execute(ctx.data_scope(), &input, operator)
            }
            "list" => {
                let input: ListRefundsInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_list(ctx.data_scope(), &input)
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: refund.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "refund".into(),
            description: "退款审批 — 申请/审批/驳回/执行".into(),
            commands: vec![
                CommandSchema {
                    name: "request".into(),
                    description: "创建退款申请".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(RequestRefundInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "approve".into(),
                    description: "审批通过退款申请".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ApproveRefundInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "reject".into(),
                    description: "驳回退款申请".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(RejectRefundInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "execute".into(),
                    description: "执行已审批的退款".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ExecuteRefundInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "list".into(),
                    description: "按状态/订单查询退款列表".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ListRefundsInput))
                        .ok(),
                    output_schema: None,
                },
            ],
        }
    }
}

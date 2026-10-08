//! feature-invoice — 发票子模块
//!
//! 命令:
//! - issue     — 开具发票 (自动生成 invoice_no，支持普票/专票)
//! - void      — 作废发票 (issued→voided)
//! - red_flush — 红字冲销 (原票作废 + 创建负数冲销记录)
//! - get       — 按 id 或 order_id 查询发票
//! - list      — 按状态/日期范围分页查询

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

fn shanghai_date() -> String {
    let shanghai = chrono_tz::Asia::Shanghai;
    let now = chrono::Utc::now().with_timezone(&shanghai);
    now.format("%Y%m%d").to_string()
}

fn generate_invoice_no(
    conn: &r2d2::PooledConnection<SqliteConnectionManager>,
) -> Result<String, String> {
    let date = shanghai_date();
    let prefix = format!("INV-{}-", date);
    let like_pattern = format!("{}%", prefix);

    let max_seq: Option<String> = conn
        .query_row(
            "SELECT invoice_no FROM invoices WHERE invoice_no LIKE ?1 ORDER BY invoice_no DESC LIMIT 1",
            params![like_pattern],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

    let seq = match max_seq {
        Some(no) => {
            let suffix = no.strip_prefix(&prefix).unwrap_or("0000");
            let n: u32 = suffix.parse().unwrap_or(0);
            n + 1
        }
        None => 1,
    };

    Ok(format!("{}{:04}", prefix, seq))
}

// ═══════════════════════════════════════════════════════════════════
// 输入类型
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IssueInvoiceInput {
    pub order_id: String,
    pub amount: f64,
    #[serde(default = "default_invoice_type")]
    pub invoice_type: String,
    #[serde(default)]
    pub tax_rate: Option<f64>,
}

fn default_invoice_type() -> String {
    "普通发票".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VoidInvoiceInput {
    pub invoice_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RedFlushInvoiceInput {
    pub invoice_id: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetInvoiceInput {
    #[serde(default)]
    pub invoice_id: Option<String>,
    #[serde(default)]
    pub order_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListInvoicesInput {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub date_from: Option<String>,
    #[serde(default)]
    pub date_to: Option<String>,
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub page_size: Option<i64>,
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureInvoice {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureInvoice {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureInvoice {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: invoice pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    fn do_issue(
        &self,
        input: &IssueInvoiceInput,
        _operator: &str,
        scope: &DataScope,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        if input.amount <= 0.0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_NEGATIVE_AMOUNT".into(),
                message: "发票金额必须大于 0".into(),
                field: Some("amount".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        if input.invoice_type != "普通发票" && input.invoice_type != "专用发票" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_INVALID_INVOICE_TYPE".into(),
                message: "发票类型必须为'普通发票'或'专用发票'".into(),
                field: Some("invoiceType".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        // Look up active tax rate
        let tax_rate: f64 = if let Some(tr) = input.tax_rate {
            tr
        } else {
            conn.query_row(
                "SELECT rate FROM tax_config WHERE tax_type = 'vat' AND is_active = 1 ORDER BY effective_from DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .unwrap_or(0.13)
        };

        let invoice_no = generate_invoice_no(&conn)?;
        let tax_amount = (input.amount * tax_rate * 100.0).round() / 100.0;
        let invoice_id = Uuid::new_v4().to_string();

        conn.execute(
            "INSERT INTO invoices (id, order_id, invoice_no, type, amount, tax_rate, tax_amount, status, tenant_id, issued_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'issued', ?8, ?9, ?10)",
            params![invoice_id, input.order_id, invoice_no, input.invoice_type, input.amount, tax_rate, tax_amount, scope.tenant_id().as_str(), now, now],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        // Also create a revenue record
        let revenue_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO revenue_records (id, order_id, amount, recognition_date, source, created_at, tenant_id) \
             VALUES (?1, ?2, ?3, ?4, 'order_complete', ?5, ?6)",
            params![revenue_id, input.order_id, input.amount, &now[..10], now, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        // Accounting entries: debit:receivable, credit:revenue
        let entry1_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
             VALUES (?1, ?2, 'debit', 'receivable', ?3, ?4, ?5, ?6)",
            params![entry1_id, input.order_id, input.amount, format!("发票 {} 应收账款", invoice_no), now, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        let entry2_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
             VALUES (?1, ?2, 'credit', 'revenue', ?3, ?4, ?5, ?6)",
            params![entry2_id, input.order_id, input.amount, format!("发票 {} 收入确认", invoice_no), now, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "invoiceId": invoice_id,
            "invoiceNo": invoice_no,
            "orderId": input.order_id,
            "amount": input.amount,
            "taxRate": tax_rate,
            "taxAmount": tax_amount,
            "status": "issued",
        }))
    }

    fn do_void(
        &self,
        input: &VoidInvoiceInput,
        _operator: &str,
        scope: &DataScope,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let (order_id, status): (String, String) = conn
            .query_row(
                "SELECT order_id, status FROM invoices WHERE id = ?1 AND tenant_id = ?2",
                params![input.invoice_id, scope.tenant_id().as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| {
                serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_INVOICE_NOT_FOUND".into(),
                    message: format!("发票 {} 不存在", input.invoice_id),
                    field: Some("invoiceId".into()),
                    context: None,
                })
                .unwrap_or_default()
            })?;

        if status != "issued" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_INVOICE_NOT_ISSUED".into(),
                message: format!("发票状态为 {}，不可作废", status),
                field: Some("invoiceId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        conn.execute(
            "UPDATE invoices SET status = 'voided', voided_at = ?1 WHERE id = ?2 AND tenant_id = ?3",
            params![now, input.invoice_id, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        // Reverse accounting entries
        let invoice_no: String = conn
            .query_row(
                "SELECT invoice_no FROM invoices WHERE id = ?1 AND tenant_id = ?2",
                params![input.invoice_id, scope.tenant_id().as_str()],
                |row| row.get(0),
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let entry1_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
             VALUES (?1, ?2, 'credit', 'receivable', 0, ?3, ?4, ?5)",
            params![entry1_id, order_id, format!("发票 {} 作废冲回应收", invoice_no), now, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "invoiceId": input.invoice_id,
            "status": "voided",
        }))
    }

    fn do_red_flush(
        &self,
        input: &RedFlushInvoiceInput,
        _operator: &str,
        scope: &DataScope,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        let (order_id, amount, tax_rate, tax_amount, invoice_no, invoice_type, status): (String, f64, f64, f64, String, String, String) = conn
            .query_row(
                "SELECT order_id, amount, tax_rate, tax_amount, invoice_no, type, status FROM invoices WHERE id = ?1 AND tenant_id = ?2",
                params![input.invoice_id, scope.tenant_id().as_str()],
                |row| Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?,
                )),
            )
            .map_err(|_| serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_INVOICE_NOT_FOUND".into(),
                message: format!("发票 {} 不存在", input.invoice_id),
                field: Some("invoiceId".into()),
                context: None,
            }).unwrap_or_default())?;

        if status != "issued" {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_INVOICE_NOT_ISSUED".into(),
                message: format!("发票状态为 {}，不可冲销", status),
                field: Some("invoiceId".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        // Void original
        conn.execute(
            "UPDATE invoices SET status = 'voided', voided_at = ?1 WHERE id = ?2 AND tenant_id = ?3",
            params![now, input.invoice_id, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        // Create negative entry (red flush)
        let new_invoice_no = generate_invoice_no(&conn)?;
        let red_id = Uuid::new_v4().to_string();
        let neg_amount = -amount;
        let neg_tax = -tax_amount;

        conn.execute(
            "INSERT INTO invoices (id, order_id, invoice_no, type, amount, tax_rate, tax_amount, status, tenant_id, issued_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'issued', ?8, ?9, ?10)",
            params![red_id, order_id, new_invoice_no, invoice_type, neg_amount, tax_rate, neg_tax, scope.tenant_id().as_str(), now, now],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        // Reverse revenue
        let rev_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO revenue_records (id, order_id, amount, recognition_date, source, created_at, tenant_id) \
             VALUES (?1, ?2, ?3, ?4, 'other', ?5, ?6)",
            params![rev_id, order_id, neg_amount, &now[..10], now, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        let desc = if !input.reason.is_empty() {
            format!("红字冲销 原发票 {} — {}", invoice_no, input.reason)
        } else {
            format!("红字冲销 原发票 {}", invoice_no)
        };

        let entry1_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
             VALUES (?1, ?2, 'credit', 'receivable', ?3, ?4, ?5, ?6)",
            params![entry1_id, order_id, amount, &desc, now, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        let entry2_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounting_entries (id, order_id, entry_type, account, amount, description, created_at, tenant_id) \
             VALUES (?1, ?2, 'debit', 'revenue', ?3, ?4, ?5, ?6)",
            params![entry2_id, order_id, amount, &desc, now, scope.tenant_id().as_str()],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "originalInvoiceId": input.invoice_id,
            "redInvoiceId": red_id,
            "redInvoiceNo": new_invoice_no,
            "redAmount": neg_amount,
            "status": "red_flushed",
        }))
    }

    fn do_get(&self, input: &GetInvoiceInput, scope: &DataScope) -> Result<Value, String> {
        let conn = self.get_conn()?;

        let invoice: Option<Value> = if let Some(ref id) = input.invoice_id {
            conn.query_row(
                "SELECT id, order_id, invoice_no, type, amount, tax_rate, tax_amount, status, tenant_id, issued_at, voided_at, created_at \
                 FROM invoices WHERE id = ?1 AND tenant_id = ?2",
                params![id, scope.tenant_id().as_str()],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "orderId": row.get::<_, String>(1)?,
                        "invoiceNo": row.get::<_, String>(2)?,
                        "type": row.get::<_, String>(3)?,
                        "amount": row.get::<_, f64>(4)?,
                        "taxRate": row.get::<_, f64>(5)?,
                        "taxAmount": row.get::<_, f64>(6)?,
                        "status": row.get::<_, String>(7)?,
                        "tenantId": row.get::<_, String>(8)?,
                        "issuedAt": row.get::<_, String>(9)?,
                        "voidedAt": row.get::<_, Option<String>>(10)?,
                        "createdAt": row.get::<_, String>(11)?,
                    }))
                },
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
        } else if let Some(ref oid) = input.order_id {
            conn.query_row(
                "SELECT id, order_id, invoice_no, type, amount, tax_rate, tax_amount, status, tenant_id, issued_at, voided_at, created_at \
                 FROM invoices WHERE order_id = ?1 AND tenant_id = ?2 ORDER BY created_at DESC LIMIT 1",
                params![oid, scope.tenant_id().as_str()],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "orderId": row.get::<_, String>(1)?,
                        "invoiceNo": row.get::<_, String>(2)?,
                        "type": row.get::<_, String>(3)?,
                        "amount": row.get::<_, f64>(4)?,
                        "taxRate": row.get::<_, f64>(5)?,
                        "taxAmount": row.get::<_, f64>(6)?,
                        "status": row.get::<_, String>(7)?,
                        "tenantId": row.get::<_, String>(8)?,
                        "issuedAt": row.get::<_, String>(9)?,
                        "voidedAt": row.get::<_, Option<String>>(10)?,
                        "createdAt": row.get::<_, String>(11)?,
                    }))
                },
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
        } else {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_MISSING_PARAM".into(),
                message: "必须提供 invoiceId 或 orderId".into(),
                field: None,
                context: None,
            })
            .unwrap_or_default());
        };

        Ok(serde_json::json!({ "ok": true, "invoice": invoice }))
    }

    fn do_list(&self, input: &ListInvoicesInput, scope: &DataScope) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).min(100);
        let offset = (page - 1) * page_size;

        let mut conditions: Vec<String> = vec!["tenant_id = ?1".into()];
        let mut param_values: Vec<String> = vec![scope.tenant_id().as_str().to_string()];

        if let Some(ref s) = input.status
            && !s.is_empty()
        {
            param_values.push(s.clone());
            conditions.push(format!("status = ?{}", param_values.len()));
        }
        if let Some(ref df) = input.date_from
            && !df.is_empty()
        {
            param_values.push(df.clone());
            conditions.push(format!("issued_at >= ?{}", param_values.len()));
        }
        if let Some(ref dt) = input.date_to
            && !dt.is_empty()
        {
            param_values.push(format!("{}T23:59:59.999+08:00", dt));
            conditions.push(format!("issued_at <= ?{}", param_values.len()));
        }

        let where_clause = conditions.join(" AND ");

        let count_sql = format!("SELECT COUNT(*) FROM invoices WHERE {}", where_clause);
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();

        let total: i64 = conn
            .query_row(&count_sql, params_ref.as_slice(), |row| row.get(0))
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let data_sql = format!(
            "SELECT id, order_id, invoice_no, type, amount, tax_rate, tax_amount, status, issued_at, voided_at, created_at \
             FROM invoices WHERE {} ORDER BY created_at DESC LIMIT {} OFFSET {}",
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
                    "orderId": row.get::<_, String>(1)?,
                    "invoiceNo": row.get::<_, String>(2)?,
                    "type": row.get::<_, String>(3)?,
                    "amount": row.get::<_, f64>(4)?,
                    "taxRate": row.get::<_, f64>(5)?,
                    "taxAmount": row.get::<_, f64>(6)?,
                    "status": row.get::<_, String>(7)?,
                    "issuedAt": row.get::<_, String>(8)?,
                    "voidedAt": row.get::<_, Option<String>>(9)?,
                    "createdAt": row.get::<_, String>(10)?,
                }))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!({
            "ok": true,
            "invoices": rows,
            "pagination": {
                "page": page,
                "pageSize": page_size,
                "total": total,
            },
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════
// SystemModule trait
// ═══════════════════════════════════════════════════════════════════

impl SystemModule for FeatureInvoice {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "invoice".into(),
            version: "0.1.0".into(),
            description: "发票管理 — 开具/作废/红字冲销 + 增值税票种(普票/专票)".into(),
            author: "talos".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "issue",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "void",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "red_flush",
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
            "issue" => {
                let input: IssueInvoiceInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_issue(&input, operator, ctx.data_scope())
            }
            "void" => {
                let input: VoidInvoiceInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_void(&input, operator, ctx.data_scope())
            }
            "red_flush" => {
                let input: RedFlushInvoiceInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_red_flush(&input, operator, ctx.data_scope())
            }
            "get" => {
                let input: GetInvoiceInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_get(&input, ctx.data_scope())
            }
            "list" => {
                let input: ListInvoicesInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_list(&input, ctx.data_scope())
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: invoice.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "invoice".into(),
            description: "发票管理 — 开具/作废/红字冲销 + 增值税票种(普票/专票)".into(),
            commands: vec![
                CommandSchema {
                    name: "issue".into(),
                    description: "开具发票 (自动生成 invoice_no)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(IssueInvoiceInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "void".into(),
                    description: "作废发票".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(VoidInvoiceInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "red_flush".into(),
                    description: "红字冲销 (原票作废 + 创建负数冲销记录)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(RedFlushInvoiceInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get".into(),
                    description: "按 id 或 order_id 查询发票".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetInvoiceInput)).ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "list".into(),
                    description: "按状态/日期范围分页查询发票列表".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ListInvoicesInput))
                        .ok(),
                    output_schema: None,
                },
            ],
        }
    }
}

//! feature-notification — 通知系统模块
//!
//! 命令:
//! - send       — 发送通知 (指定 channel 和 event_type)
//! - list       — 分页列出当前用户的站内信
//! - mark_read  — 标记消息已读 (单条或全部)
//! - get_templates — 列出模板
//! - upsert_template — 创建/更新模板
//! - trigger    — 事件驱动的通知触发

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
pub struct SendInput {
    pub event_type: String,
    pub channel: Option<String>,   // 默认 "in_app"
    pub recipient: Option<String>, // 对于 in_app 为空时取 ctx.user_id
    pub variables: Option<Value>,  // {orderNo, deviceSerialNo, amount, customerName, dueDate}
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListInput {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MarkReadInput {
    pub message_id: Option<String>, // None = 全部已读
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertTemplateInput {
    pub id: Option<String>, // None = create new
    pub event_type: String,
    pub channel: String,
    pub subject_template: String,
    pub body_template: String,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerInput {
    pub event_type: String,
    pub order_id: Option<String>,
    pub variables: Option<Value>,
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureNotification {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureNotification {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureNotification {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: notify pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    // ── 模板变量渲染 ─────────────────────────────────────────────

    /// 将模板字符串中的 {var} 替换为实际值
    fn render_template(template: &str, variables: &Value) -> String {
        let mut result = template.to_string();
        if let Some(obj) = variables.as_object() {
            for (key, val) in obj {
                let placeholder = format!("{{{}}}", key);
                let replacement = match val {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => val.to_string(),
                };
                result = result.replace(&placeholder, &replacement);
            }
        }
        result
    }

    /// 获取模板并渲染，返回 (subject, body)
    fn get_rendered_template(
        &self,
        scope: &DataScope,
        event_type: &str,
        channel: &str,
        variables: &Value,
    ) -> Result<(String, String), String> {
        let conn = self.get_conn()?;

        let row: Option<(String, String)> = conn
            .query_row(
                "SELECT subject_template, body_template FROM notification_templates \
                 WHERE tenant_id = ?1 AND event_type = ?2 AND channel = ?3 AND is_enabled = 1 LIMIT 1",
                params![scope.tenant_id().as_str(), event_type, channel],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        match row {
            Some((ref subject_tpl, ref body_tpl)) => {
                let subject = Self::render_template(subject_tpl, variables);
                let body = Self::render_template(body_tpl, variables);
                Ok((subject, body))
            }
            None => Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_TEMPLATE_NOT_FOUND".into(),
                message: format!("模板不存在: event_type={}, channel={}", event_type, channel),
                field: None,
                context: None,
            })
            .unwrap_or_default()),
        }
    }

    // ── 业务方法 ─────────────────────────────────────────────────

    fn do_send(
        &self,
        scope: &DataScope,
        input: &SendInput,
        recipient: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let channel = input
            .channel
            .clone()
            .unwrap_or_else(|| "in_app".to_string());
        let variables = input.variables.clone().unwrap_or(serde_json::json!({}));

        // 获取并渲染模板
        let (subject, body) =
            self.get_rendered_template(scope, &input.event_type, &channel, &variables)?;

        // 查找 template_id 用于记录
        let template_id: String = conn
            .query_row(
                "SELECT id FROM notification_templates WHERE tenant_id = ?1 AND event_type = ?2 AND channel = ?3 AND is_enabled = 1 LIMIT 1",
                params![scope.tenant_id().as_str(), input.event_type, channel],
                |row| row.get(0),
            )
            .unwrap_or_default();

        let log_id = Uuid::new_v4().to_string();

        match channel.as_str() {
            "email" => {
                // Stub: log via tracing
                tracing::info!(
                    target: "notify",
                    event_type = %input.event_type,
                    channel = "email",
                    recipient = %recipient,
                    subject = %subject,
                    "Email notification (stub) — SMTP not configured"
                );
                conn.execute(
                    "INSERT INTO notification_log (id, template_id, event_type, channel, recipient, subject, body, status, created_at, tenant_id) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'sent', ?8, ?9)",
                    params![log_id, template_id, input.event_type, "email", recipient, subject, body, now, scope.tenant_id().as_str()],
                ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;
            }
            "sms" => {
                // Stub: log via tracing
                tracing::info!(
                    target: "notify",
                    event_type = %input.event_type,
                    channel = "sms",
                    recipient = %recipient,
                    body = %body,
                    "SMS notification (stub) — SMS provider not configured"
                );
                conn.execute(
                    "INSERT INTO notification_log (id, template_id, event_type, channel, recipient, subject, body, status, created_at, tenant_id) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'sent', ?8, ?9)",
                    params![log_id, template_id, input.event_type, "sms", recipient, subject, body, now, scope.tenant_id().as_str()],
                ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;
            }
            _ => {
                // in_app — default
                conn.execute(
                    "INSERT INTO notification_log (id, template_id, event_type, channel, recipient, subject, body, status, read_at, created_at, tenant_id) \
                     VALUES (?1, ?2, ?3, 'in_app', ?4, ?5, ?6, 'sent', NULL, ?7, ?8)",
                    params![log_id, template_id, input.event_type, recipient, subject, body, now, scope.tenant_id().as_str()],
                ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;
            }
        }

        Ok(serde_json::json!({
            "ok": true,
            "messageId": log_id,
            "eventType": input.event_type,
            "channel": channel,
            "status": "sent",
        }))
    }

    fn do_list(
        &self,
        scope: &DataScope,
        input: &ListInput,
        user_id: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let page = input.page.unwrap_or(1).max(1);
        let page_size = input.page_size.unwrap_or(20).clamp(1, 100);
        let offset = (page - 1) * page_size;

        // 统计未读数
        let unread_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notification_log WHERE tenant_id = ?1 AND channel = 'in_app' AND recipient = ?2 AND read_at IS NULL",
                params![scope.tenant_id().as_str(), user_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // 总数
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM notification_log WHERE tenant_id = ?1 AND channel = 'in_app' AND recipient = ?2",
                params![scope.tenant_id().as_str(), user_id],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // 分页数据
        let mut stmt = conn
            .prepare(
                "SELECT id, event_type, channel, subject, body, status, read_at, created_at \
                 FROM notification_log WHERE tenant_id = ?1 AND channel = 'in_app' AND recipient = ?2 \
                 ORDER BY created_at DESC LIMIT ?3 OFFSET ?4",
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let messages: Vec<Value> = stmt
            .query_map(
                params![scope.tenant_id().as_str(), user_id, page_size, offset],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "eventType": row.get::<_, String>(1)?,
                        "channel": row.get::<_, String>(2)?,
                        "subject": row.get::<_, String>(3)?,
                        "body": row.get::<_, String>(4)?,
                        "status": row.get::<_, String>(5)?,
                        "readAt": row.get::<_, Option<String>>(6)?,
                        "createdAt": row.get::<_, String>(7)?,
                    }))
                },
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!({
            "ok": true,
            "messages": messages,
            "unreadCount": unread_count,
            "pagination": {
                "page": page,
                "pageSize": page_size,
                "total": total,
            },
        }))
    }

    fn do_mark_read(
        &self,
        scope: &DataScope,
        input: &MarkReadInput,
        user_id: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();

        if let Some(ref msg_id) = input.message_id {
            // Mark single message read — own message only
            let affected = conn
                .execute(
                    "UPDATE notification_log SET read_at = ?1 WHERE tenant_id = ?2 AND id = ?3 AND recipient = ?4 AND read_at IS NULL",
                    params![now, scope.tenant_id().as_str(), msg_id, user_id],
                )
                .map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

            Ok(serde_json::json!({
                "ok": true,
                "affected": affected,
            }))
        } else {
            // Mark all read
            let affected = conn
                .execute(
                    "UPDATE notification_log SET read_at = ?1 WHERE tenant_id = ?2 AND channel = 'in_app' AND recipient = ?3 AND read_at IS NULL",
                    params![now, scope.tenant_id().as_str(), user_id],
                )
                .map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

            Ok(serde_json::json!({
                "ok": true,
                "affected": affected,
            }))
        }
    }

    fn do_get_templates(&self, scope: &DataScope) -> Result<Value, String> {
        let conn = self.get_conn()?;

        let mut stmt = conn
            .prepare(
                "SELECT id, event_type, channel, subject_template, body_template, is_enabled, created_at, updated_at \
                 FROM notification_templates WHERE tenant_id = ?1 ORDER BY event_type, channel"
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let templates: Vec<Value> = stmt
            .query_map(params![scope.tenant_id().as_str()], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "eventType": row.get::<_, String>(1)?,
                    "channel": row.get::<_, String>(2)?,
                    "subjectTemplate": row.get::<_, String>(3)?,
                    "bodyTemplate": row.get::<_, String>(4)?,
                    "isEnabled": row.get::<_, i64>(5)? != 0,
                    "createdAt": row.get::<_, String>(6)?,
                    "updatedAt": row.get::<_, String>(7)?,
                }))
            })
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(serde_json::json!({ "ok": true, "templates": templates }))
    }

    fn do_upsert_template(
        &self,
        scope: &DataScope,
        input: &UpsertTemplateInput,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let is_enabled = if input.is_enabled.unwrap_or(true) {
            1
        } else {
            0
        };

        if let Some(ref id) = input.id {
            // Update existing
            let affected = conn
                .execute(
                    "UPDATE notification_templates SET event_type = ?1, channel = ?2, subject_template = ?3, \
                     body_template = ?4, is_enabled = ?5, updated_at = ?6 WHERE id = ?7 AND tenant_id = ?8",
                    params![input.event_type, input.channel, input.subject_template, input.body_template, is_enabled, now, id, scope.tenant_id().as_str()],
                )
                .map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

            if affected == 0 {
                return Err(serde_json::to_string(&ErrorPayload {
                    category: "biz".into(),
                    code: "BIZ_TEMPLATE_NOT_FOUND".into(),
                    message: format!("模板 {} 不存在", id),
                    field: Some("id".into()),
                    context: None,
                })
                .unwrap_or_default());
            }

            Ok(serde_json::json!({
                "ok": true,
                "id": id,
                "updated": true,
            }))
        } else {
            // Create new
            let new_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO notification_templates (id, event_type, channel, subject_template, body_template, is_enabled, created_at, updated_at, tenant_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![new_id, input.event_type, input.channel, input.subject_template, input.body_template, is_enabled, now, now, scope.tenant_id().as_str()],
            ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

            Ok(serde_json::json!({
                "ok": true,
                "id": new_id,
                "created": true,
            }))
        }
    }

    fn do_trigger(
        &self,
        scope: &DataScope,
        input: &TriggerInput,
        user_id: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let variables = input.variables.clone().unwrap_or(serde_json::json!({}));
        let mut results: Vec<Value> = Vec::new();

        // Find all enabled templates for this event_type
        let mut stmt = conn
            .prepare(
                "SELECT channel FROM notification_templates WHERE tenant_id = ?1 AND event_type = ?2 AND is_enabled = 1"
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        let channels: Vec<String> = stmt
            .query_map(
                params![scope.tenant_id().as_str(), input.event_type],
                |row| row.get(0),
            )
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        if channels.is_empty() {
            return Ok(
                serde_json::json!({ "ok": true, "results": [], "message": "no enabled templates for this event" }),
            );
        }

        // Determine recipient: for in_app = user_id; for email/sms = variables.customerName fallback
        let in_app_recipient = user_id;
        let external_recipient = variables
            .get("customerName")
            .and_then(|v| v.as_str())
            .unwrap_or(user_id);

        // For order-tied events, resolve order data via variables or DB
        let vars_with_order =
            if let Some(ref order_id) = input.order_id {
                let mut vars = variables.clone();
                // Try to fill in order data
                if let Ok(Some(row)) = conn.query_row(
                    "SELECT o.orderNo, o.deviceSerialNo, o.recipientName, o.estimatedReturnDate \
                     FROM orders o WHERE o.tenant_id = ?1 AND o.id = ?2",
                    params![scope.tenant_id().as_str(), order_id],
                    |row| {
                        let order_no: String = row.get(0)?;
                        let device_sn: String = row.get(1)?;
                        let cust_name: String = row.get(2)?;
                        let due_date: String = row.get(3)?;
                        Ok((order_no, device_sn, cust_name, due_date))
                    },
                ).optional().map_err(|e| format!("SYS_DB_QUERY: {}", e)) {
                let (order_no, device_sn, cust_name, due_date) = row;
                let map = vars.as_object_mut().unwrap();
                map.insert("orderNo".to_string(), Value::String(order_no));
                map.insert("deviceSerialNo".to_string(), Value::String(device_sn));
                map.insert("customerName".to_string(), Value::String(cust_name));
                map.insert("dueDate".to_string(), Value::String(due_date));
            }
                vars
            } else {
                variables.clone()
            };

        for channel in &channels {
            let recipient = match channel.as_str() {
                "in_app" => in_app_recipient.to_string(),
                _ => external_recipient.to_string(),
            };

            let send_input = SendInput {
                event_type: input.event_type.clone(),
                channel: Some(channel.clone()),
                recipient: Some(recipient),
                variables: Some(vars_with_order.clone()),
            };

            match self.do_send(
                scope,
                &send_input,
                &send_input.recipient.clone().unwrap_or_default(),
            ) {
                Ok(result) => results.push(result),
                Err(e) => {
                    tracing::warn!(target: "notify", channel = %channel, error = %e, "Trigger send failed for channel");
                    results.push(serde_json::json!({
                        "channel": channel,
                        "status": "failed",
                        "error": e,
                    }));
                }
            }
        }

        Ok(serde_json::json!({ "ok": true, "results": results }))
    }
}

// ═══════════════════════════════════════════════════════════════════
// SystemModule trait
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(tenant: &str) -> DataScope {
        DataScope::production(
            TenantId::new(tenant).expect("tenant id"),
            Revision::new("r1").expect("revision"),
        )
        .expect("production scope")
    }

    #[test]
    fn notification_data_and_order_hydration_are_tenant_scoped() {
        let db_path = std::env::temp_dir().join(format!("talos-notify-{}.db", Uuid::new_v4()));
        let manager = SqliteConnectionManager::file(&db_path);
        let pool = Pool::builder().max_size(4).build(manager).expect("pool");
        let feature = FeatureNotification {
            pool: Mutex::new(Some(pool.clone())),
        };

        {
            let conn = pool.get().expect("setup connection");
            conn.execute_batch(
                "CREATE TABLE notification_templates (
                   id TEXT PRIMARY KEY, event_type TEXT NOT NULL, channel TEXT NOT NULL,
                   subject_template TEXT NOT NULL, body_template TEXT NOT NULL,
                   is_enabled INTEGER NOT NULL, created_at TEXT NOT NULL,
                   updated_at TEXT NOT NULL, tenant_id TEXT NOT NULL
                 );
                 CREATE TABLE notification_log (
                   id TEXT PRIMARY KEY, template_id TEXT, event_type TEXT NOT NULL,
                   channel TEXT NOT NULL, recipient TEXT NOT NULL, subject TEXT NOT NULL,
                   body TEXT NOT NULL, status TEXT NOT NULL, read_at TEXT,
                   error_message TEXT DEFAULT '', created_at TEXT NOT NULL,
                   tenant_id TEXT NOT NULL
                 );
                 CREATE TABLE orders (
                   id TEXT PRIMARY KEY, orderNo TEXT NOT NULL, deviceSerialNo TEXT NOT NULL,
                   recipientName TEXT NOT NULL, estimatedReturnDate TEXT NOT NULL,
                   tenant_id TEXT NOT NULL
                 );
                 INSERT INTO notification_templates VALUES
                   ('tpl-a', 'shipped', 'in_app', 'Tenant A', 'Order {orderNo}', 1, '', '', 'tenant-a'),
                   ('tpl-b', 'shipped', 'in_app', 'Tenant B', 'Private {orderNo}', 1, '', '', 'tenant-b');
                 INSERT INTO notification_log
                   (id, template_id, event_type, channel, recipient, subject, body, status, read_at, created_at, tenant_id)
                 VALUES
                   ('log-a', 'tpl-a', 'shipped', 'in_app', 'same-user', 'A', 'A body', 'sent', NULL, '2026-07-16', 'tenant-a'),
                   ('log-b', 'tpl-b', 'shipped', 'in_app', 'same-user', 'B', 'B body', 'sent', NULL, '2026-07-16', 'tenant-b');
                 INSERT INTO orders VALUES
                   ('order-b', 'SECRET-B', 'DEVICE-B', 'Customer B', '2026-07-20', 'tenant-b');",
            ).expect("setup schema");
        }

        let tenant_a = scope("tenant-a");
        let templates = feature.do_get_templates(&tenant_a).expect("templates");
        assert_eq!(templates["templates"].as_array().unwrap().len(), 1);
        assert_eq!(templates["templates"][0]["id"], "tpl-a");

        let messages = feature
            .do_list(
                &tenant_a,
                &ListInput {
                    page: None,
                    page_size: None,
                },
                "same-user",
            )
            .expect("messages");
        assert_eq!(messages["pagination"]["total"], 1);
        assert_eq!(messages["messages"][0]["id"], "log-a");

        let marked = feature
            .do_mark_read(
                &tenant_a,
                &MarkReadInput {
                    message_id: Some("log-b".into()),
                },
                "same-user",
            )
            .expect("mark read");
        assert_eq!(marked["affected"], 0);

        feature
            .do_trigger(
                &tenant_a,
                &TriggerInput {
                    event_type: "shipped".into(),
                    order_id: Some("order-b".into()),
                    variables: None,
                },
                "same-user",
            )
            .expect("trigger");

        let conn = pool.get().expect("verify connection");
        let triggered_body: String = conn
            .query_row(
                "SELECT body FROM notification_log
             WHERE tenant_id = 'tenant-a' AND id != 'log-a'",
                [],
                |row| row.get(0),
            )
            .expect("triggered notification");
        assert_eq!(triggered_body, "Order {orderNo}");

        drop(conn);
        drop(feature);
        drop(pool);
        let _ = std::fs::remove_file(db_path);
    }
}

impl SystemModule for FeatureNotification {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "notify".into(),
            version: "0.1.0".into(),
            description: "通知系统 — 多通道 (email/SMS/站内信) + 事件驱动 + 模板引擎".into(),
            author: "talos".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
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
        let user_id = ctx.user_id().unwrap_or("system");
        let scope = ctx.data_scope();

        match command {
            "send" => {
                let input: SendInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                let recipient = input
                    .recipient
                    .clone()
                    .unwrap_or_else(|| user_id.to_string());
                self.do_send(scope, &input, &recipient)
            }
            "list" => {
                let input: ListInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_list(scope, &input, user_id)
            }
            "mark_read" => {
                let input: MarkReadInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_mark_read(scope, &input, user_id)
            }
            "get_templates" => self.do_get_templates(scope),
            "upsert_template" => {
                let input: UpsertTemplateInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_upsert_template(scope, &input)
            }
            "trigger" => {
                let input: TriggerInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_trigger(scope, &input, user_id)
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: notify.{}", command)),
        }
    }

    fn commands(&self) -> Vec<system_core::CommandMetadata> {
        vec![
            system_core::CommandMetadata::new(
                "send",
                system_core::AccessRequirement::Authenticated,
                &[
                    system_core::EffectClass::DatabaseWrite,
                    system_core::EffectClass::Notification,
                ],
                system_core::SimulationSupport::Blocked,
            ),
            system_core::CommandMetadata::new(
                "list",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "mark_read",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "get_templates",
                system_core::AccessRequirement::Authenticated,
                &[system_core::EffectClass::DatabaseRead],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "upsert_template",
                system_core::AccessRequirement::TenantAdmin,
                &[system_core::EffectClass::DatabaseWrite],
                system_core::SimulationSupport::Supported,
            ),
            system_core::CommandMetadata::new(
                "trigger",
                system_core::AccessRequirement::System,
                &[
                    system_core::EffectClass::DatabaseWrite,
                    system_core::EffectClass::Notification,
                ],
                system_core::SimulationSupport::Blocked,
            ),
        ]
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "notify".into(),
            description: "通知系统 — 多通道 (email/SMS/站内信) + 事件驱动 + 模板引擎".into(),
            commands: vec![
                CommandSchema {
                    name: "send".into(),
                    description: "发送通知 (指定 channel 和 event_type)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(SendInput)).ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "list".into(),
                    description: "分页列出当前用户的站内信".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(ListInput)).ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "mark_read".into(),
                    description: "标记消息已读 (单条或全部)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(MarkReadInput)).ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get_templates".into(),
                    description: "列出所有通知模板".into(),
                    version: "0.1.0".into(),
                    input_schema: None,
                    output_schema: None,
                },
                CommandSchema {
                    name: "upsert_template".into(),
                    description: "创建或更新模板".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(UpsertTemplateInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "trigger".into(),
                    description: "事件驱动的通知触发 (遍历所有启用的 channel)".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(TriggerInput)).ok(),
                    output_schema: None,
                },
            ],
        }
    }
}

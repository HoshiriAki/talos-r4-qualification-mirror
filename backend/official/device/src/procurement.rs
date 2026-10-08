//! feature-procurement — 设备采购记录子模块 (official/device)
//!
//! 命令:
//! - record_purchase       — 记录设备采购信息
//! - set_replacement_value — 更新设备重置价值
//! - get                   — 查询设备采购信息

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
pub struct RecordPurchaseInput {
    pub device_serial_no: String,
    pub purchase_price: f64,
    #[serde(default)]
    pub purchase_date: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub invoice_no: String,
    #[serde(default)]
    pub replacement_value: f64,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetReplacementValueInput {
    pub device_serial_no: String,
    pub replacement_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetPurchaseInput {
    pub device_serial_no: String,
}

// ═══════════════════════════════════════════════════════════════════
// Feature struct
// ═══════════════════════════════════════════════════════════════════

pub struct FeatureProcurement {
    pub pool: Mutex<Option<Pool<SqliteConnectionManager>>>,
}

impl Default for FeatureProcurement {
    fn default() -> Self {
        Self::new()
    }
}

impl FeatureProcurement {
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(None),
        }
    }

    fn get_conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, String> {
        let guard = self.pool.lock().map_err(|e| format!("SYS_LOCK: {}", e))?;
        let pool = guard
            .as_ref()
            .ok_or("SYS_POOL_MISSING: procurement pool not set".to_string())?;
        pool.get().map_err(|e| format!("SYS_DB_CONN: {}", e))
    }

    fn do_record_purchase(
        &self,
        scope: &DataScope,
        input: &RecordPurchaseInput,
        _operator: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let now = shanghai_now_iso();
        let tenant_id = scope.tenant_id().as_str();

        // Check if purchase record already exists for this device
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM asset_purchases WHERE tenant_id = ?1 AND device_serial_no = ?2",
                params![tenant_id, input.device_serial_no],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?;

        if existing.is_some() {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ASSET_ALREADY_EXISTS".into(),
                message: format!("设备 {} 已有采购记录", input.device_serial_no),
                field: Some("deviceSerialNo".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        let id = Uuid::new_v4().to_string();
        let purchase_date = if input.purchase_date.is_empty() {
            now.clone()
        } else {
            input.purchase_date.clone()
        };
        let replacement_value = if input.replacement_value > 0.0 {
            input.replacement_value
        } else {
            input.purchase_price
        };

        conn.execute(
            "INSERT INTO asset_purchases (id, device_serial_no, purchase_price, purchase_date, vendor, \
             invoice_no, replacement_value, notes, created_at, tenant_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id, input.device_serial_no, input.purchase_price, purchase_date,
                input.vendor, input.invoice_no, replacement_value, input.notes, now, tenant_id,
            ],
        ).map_err(|e| format!("SYS_DB_INSERT: {}", e))?;

        Ok(serde_json::json!({
            "ok": true,
            "id": id,
            "deviceSerialNo": input.device_serial_no,
            "purchasePrice": input.purchase_price,
            "replacementValue": replacement_value,
        }))
    }

    fn do_set_replacement_value(
        &self,
        scope: &DataScope,
        input: &SetReplacementValueInput,
        _operator: &str,
    ) -> Result<Value, String> {
        let conn = self.get_conn()?;

        let affected = conn.execute(
            "UPDATE asset_purchases SET replacement_value = ?1 WHERE tenant_id = ?2 AND device_serial_no = ?3",
            params![input.replacement_value, scope.tenant_id().as_str(), input.device_serial_no],
        ).map_err(|e| format!("SYS_DB_UPDATE: {}", e))?;

        if affected == 0 {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "biz".into(),
                code: "BIZ_ASSET_NOT_FOUND".into(),
                message: format!("设备 {} 没有采购记录，请先录入", input.device_serial_no),
                field: Some("deviceSerialNo".into()),
                context: None,
            })
            .unwrap_or_default());
        }

        Ok(serde_json::json!({
            "ok": true,
            "deviceSerialNo": input.device_serial_no,
            "replacementValue": input.replacement_value,
        }))
    }

    fn do_get(&self, scope: &DataScope, input: &GetPurchaseInput) -> Result<Value, String> {
        let conn = self.get_conn()?;
        let r: Value = conn
            .query_row(
                "SELECT id, device_serial_no, purchase_price, purchase_date, vendor, \
                        invoice_no, replacement_value, notes, created_at \
                 FROM asset_purchases WHERE tenant_id = ?1 AND device_serial_no = ?2",
                params![scope.tenant_id().as_str(), input.device_serial_no],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "deviceSerialNo": row.get::<_, String>(1)?,
                        "purchasePrice": row.get::<_, f64>(2)?,
                        "purchaseDate": row.get::<_, String>(3)?,
                        "vendor": row.get::<_, String>(4)?,
                        "invoiceNo": row.get::<_, String>(5)?,
                        "replacementValue": row.get::<_, f64>(6)?,
                        "notes": row.get::<_, String>(7)?,
                        "createdAt": row.get::<_, String>(8)?,
                    }))
                },
            )
            .optional()
            .map_err(|e| format!("SYS_DB_QUERY: {}", e))?
            .unwrap_or(serde_json::json!(null));

        Ok(serde_json::json!({ "ok": true, "purchase": r }))
    }
}

impl SystemModule for FeatureProcurement {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            name: "procurement".into(),
            version: "0.1.0".into(),
            description: "设备采购记录 — record_purchase/set_replacement_value/get".into(),
            author: "talos".into(),
            wasm_compatible: false,
            storage: Some("required".into()),
        }
    }

    fn commands(&self) -> Vec<CommandMetadata> {
        vec![
            CommandMetadata::new(
                "record_purchase",
                AccessRequirement::TenantAdmin,
                &[EffectClass::DatabaseWrite],
                SimulationSupport::Supported,
            ),
            CommandMetadata::new(
                "set_replacement_value",
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
        let operator = ctx.user_id().unwrap_or("system");
        let scope = ctx.data_scope();

        match command {
            "record_purchase" => {
                let input: RecordPurchaseInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_record_purchase(scope, &input, operator)
            }
            "set_replacement_value" => {
                let input: SetReplacementValueInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_set_replacement_value(scope, &input, operator)
            }
            "get" => {
                let input: GetPurchaseInput = serde_json::from_value(payload)
                    .map_err(|e| format!("VAL_DESERIALIZE: {}", e))?;
                self.do_get(scope, &input)
            }
            _ => Err(format!("MOD_UNKNOWN_COMMAND: procurement.{}", command)),
        }
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            name: "procurement".into(),
            description: "设备采购记录 — record_purchase/set_replacement_value/get".into(),
            commands: vec![
                CommandSchema {
                    name: "record_purchase".into(),
                    description: "录入设备采购记录".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(RecordPurchaseInput))
                        .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "set_replacement_value".into(),
                    description: "更新设备重置价值".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(
                        SetReplacementValueInput
                    ))
                    .ok(),
                    output_schema: None,
                },
                CommandSchema {
                    name: "get".into(),
                    description: "查询设备采购信息".into(),
                    version: "0.1.0".into(),
                    input_schema: serde_json::to_value(schemars::schema_for!(GetPurchaseInput))
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

    fn initialized_module() -> FeatureProcurement {
        let module = FeatureProcurement::new();
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
            INSERT INTO asset_purchases VALUES
                ('a', 'SHARED', 100, '', '', '', 100, '', '', 'test-tenant'),
                ('b', 'OTHER', 200, '', '', '', 200, '', '', 'other-tenant');",
            )
            .expect("fixtures");
        *module.pool.lock().expect("pool lock") = Some(pool);
        module
    }

    #[test]
    fn purchase_reads_and_updates_are_limited_to_data_scope() {
        let module = initialized_module();
        let scope = crate::test_context();

        let hidden = module
            .do_get(
                scope.data_scope(),
                &GetPurchaseInput {
                    device_serial_no: "OTHER".into(),
                },
            )
            .expect("query succeeds");
        assert!(hidden["purchase"].is_null());

        module
            .do_record_purchase(
                scope.data_scope(),
                &RecordPurchaseInput {
                    device_serial_no: "NEW".into(),
                    purchase_price: 300.0,
                    purchase_date: String::new(),
                    vendor: String::new(),
                    invoice_no: String::new(),
                    replacement_value: 0.0,
                    notes: String::new(),
                },
                "tester",
            )
            .expect("scoped insert succeeds");

        let update = module.do_set_replacement_value(
            scope.data_scope(),
            &SetReplacementValueInput {
                device_serial_no: "OTHER".into(),
                replacement_value: 999.0,
            },
            "tester",
        );
        assert!(update.is_err());

        let conn = module.get_conn().expect("connection");
        let value: f64 = conn
            .query_row(
                "SELECT replacement_value FROM asset_purchases WHERE tenant_id = 'other-tenant'",
                [],
                |row| row.get(0),
            )
            .expect("other tenant record");
        assert_eq!(value, 200.0);
        let inserted_tenant: String = conn
            .query_row(
                "SELECT tenant_id FROM asset_purchases WHERE device_serial_no = 'NEW'",
                [],
                |row| row.get(0),
            )
            .expect("inserted record");
        assert_eq!(inserted_tenant, "test-tenant");
    }
}

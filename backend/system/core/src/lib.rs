use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod audit;
pub mod authority;
pub mod command;
pub mod execution;
#[cfg(feature = "experimental-orchestration")]
pub mod experimental;
pub mod query;
pub mod repository;
pub mod security;
pub mod tenant;
pub mod transport;
pub mod validation;

// ── 核心类型 ──

use crate::transport::http_client::HttpClient;
pub use audit::{AuditPayloadPolicy, ConflictStatus};
pub use authority::{
    ALL_PLATFORM_CAPABILITIES, AuthSessionId, AuthorityContext, IdentityId, PlatformCapability,
    PlatformMembershipId, PlatformRole, PreviewSessionId, TenantMembershipId, TenantRole,
    platform_role_allows,
};
pub use command::{AccessRequirement, CommandMetadata, EffectClass, SimulationSupport};
pub use execution::{
    ActorIdentity, DataScope, ExecutionContext, ExecutionMode, ExecutionPlane, Namespace,
    RequestId, Revision, SimulationId, TenantId, TenantScope,
};
pub use repository::RepositoryScope;

#[cfg(test)]
mod execution_contract_tests {
    use std::sync::Arc;

    use super::{
        ActorIdentity, AuthorityContext, DataScope, ExecutionContext, ExecutionMode, Namespace,
        NoopHttpClient, PlatformMembershipId, PlatformRole, RequestId, Revision, TenantId,
        TenantScope,
    };

    #[test]
    fn tenant_context_exposes_scope_without_mutators() {
        let tenant_id = TenantId::new("tenant-a").unwrap();
        let scope = DataScope::new(
            tenant_id.clone(),
            Namespace::production(),
            Revision::new("rev-1").unwrap(),
        )
        .unwrap();
        let ctx = ExecutionContext::new(
            ActorIdentity::authenticated("staff-1", "staff").unwrap(),
            TenantScope::tenant(tenant_id),
            scope,
            ExecutionMode::Normal,
            RequestId::new("req-1").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap();

        assert_eq!(ctx.data_scope().tenant_id().as_str(), "tenant-a");
        assert_eq!(ctx.execution_mode(), &ExecutionMode::Normal);
        assert_eq!(ctx.actor().id(), Some("staff-1"));
    }

    #[test]
    fn rejects_blank_scope_identifiers() {
        assert!(TenantId::new(" ").is_err());
        assert!(Revision::new("").is_err());
        assert!(RequestId::new("\t").is_err());
    }

    #[test]
    fn platform_scope_requires_an_explicit_factory() {
        let tenant_scope = TenantScope::platform();
        let data_scope = DataScope::platform(Revision::new("control-plane-current").unwrap());

        assert!(tenant_scope.is_platform());
        assert!(data_scope.is_platform());
        assert!(tenant_scope.effective_tenant_id_opt().is_none());
        assert!(data_scope.tenant_id_opt().is_none());

        let ctx = ExecutionContext::new(
            ActorIdentity::with_authority(
                "platform-owner-1",
                AuthorityContext::Platform {
                    membership_id: PlatformMembershipId::new("platform-membership-1").unwrap(),
                    roles: vec![PlatformRole::Owner],
                },
            )
            .unwrap(),
            tenant_scope,
            data_scope,
            ExecutionMode::Normal,
            RequestId::new("req-platform").unwrap(),
            None,
            Arc::new(NoopHttpClient),
        )
        .unwrap();
        assert!(ctx.tenant_scope().is_platform());
        assert!(ctx.data_scope().is_platform());
    }

    #[test]
    fn mismatched_tenant_and_simulation_scope_cannot_create_a_context() {
        let tenant_a = TenantId::new("tenant-a").unwrap();
        let tenant_b = TenantId::new("tenant-b").unwrap();
        let production = DataScope::production(tenant_b, Revision::new("rev-1").unwrap()).unwrap();
        assert!(
            ExecutionContext::new(
                ActorIdentity::system(),
                TenantScope::tenant(tenant_a),
                production,
                ExecutionMode::Normal,
                RequestId::new("req-mismatch").unwrap(),
                None,
                Arc::new(NoopHttpClient),
            )
            .is_err()
        );

        let tenant = TenantId::new("tenant-a").unwrap();
        let production =
            DataScope::production(tenant.clone(), Revision::new("rev-1").unwrap()).unwrap();
        assert!(
            ExecutionContext::new(
                ActorIdentity::system(),
                TenantScope::tenant(tenant),
                production,
                ExecutionMode::Simulation(crate::SimulationId::new("sim-1").unwrap()),
                RequestId::new("req-simulation").unwrap(),
                None,
                Arc::new(NoopHttpClient),
            )
            .is_err()
        );
    }
}

/// 无操作 HTTP 客户端 — 用于 new_test() 与单元测试默认构造
pub struct NoopHttpClient;
impl HttpClient for NoopHttpClient {
    fn send(
        &self,
        _request: crate::transport::http_client::HttpRequest,
    ) -> Result<crate::transport::http_client::HttpResponse, String> {
        Err("NoopHttpClient: no real HTTP client configured".into())
    }
    fn ping(&self, _base_url: &str) -> Result<bool, String> {
        Err("NoopHttpClient: no real HTTP client configured".into())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub wasm_compatible: bool,
    pub storage: Option<String>, // "required" | "optional" | None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSchema {
    pub name: String,
    pub description: String,
    pub version: String, // SemVer per command
    pub input_schema: Option<Value>,
    pub output_schema: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSchema {
    pub name: String,
    pub description: String,
    pub commands: Vec<CommandSchema>,
}

// ── 安全类型 ──

/// 结构化错误载体，替代魔术前缀 RGV587_ERROR
/// 所有模块的 Err(String) 必须是此结构体的 JSON 序列化
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPayload {
    /// 分类：auth / val / rgv / sys
    pub category: String,
    /// 精确错误码，如 "AUTH_002"
    pub code: String,
    /// 人类可读描述
    pub message: String,
    /// 受影响的字段（val 类必填，其他类可选）
    pub field: Option<String>,
    /// 附加上下文
    pub context: Option<Value>,
}

// ── 验证管线类型 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub errors: Vec<FieldError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldError {
    /// 字段路径，支持嵌套：".items[2].name"
    pub field: String,
    /// 结构化错误码，如 "VAL_REQUIRED"、"VAL_DATE_RANGE"
    pub code: String,
    /// 人类可读描述
    pub message: String,
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// 序列化为 JSON 嵌入 Err(String)，保持 backward compatible
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// 净化 trait — 所有 Input 类型必须实现
pub trait Sanitize {
    fn sanitize(&mut self);
}

// ── 类型状态管线（编译期强制验证顺序）──

/// 第 1 层输出：反序列化后，尚未净化
pub struct Unvalidated<T> {
    pub inner: T,
}

/// 第 2 层输出：已净化，尚未验证
pub struct Sanitized<T> {
    pub inner: T,
}

/// 第 3 层输出：已验证，可以进入业务逻辑
pub struct Validated<T> {
    pub inner: T,
}

impl<T: serde::de::DeserializeOwned> TryFrom<Value> for Unvalidated<T> {
    type Error = String;
    fn try_from(value: Value) -> Result<Self, Self::Error> {
        serde_json::from_value::<T>(value)
            .map(|inner| Unvalidated { inner })
            .map_err(|e| {
                serde_json::to_string(&ErrorPayload {
                    category: "val".into(),
                    code: "VAL_DESERIALIZE".into(),
                    message: e.to_string(),
                    field: None,
                    context: None,
                })
                .unwrap_or_default()
            })
    }
}

impl<T: Sanitize> Unvalidated<T> {
    pub fn sanitize(mut self) -> Sanitized<T> {
        self.inner.sanitize();
        Sanitized { inner: self.inner }
    }
}

pub trait Validate {
    fn validate(&self) -> ValidationResult;
}

impl<T: Validate> Sanitized<T> {
    pub fn validate(self) -> Result<Validated<T>, String> {
        let result = self.inner.validate();
        if result.is_valid() {
            Ok(Validated { inner: self.inner })
        } else {
            Err(result.to_json())
        }
    }
}

impl<T> Validated<T> {
    pub fn into_inner(self) -> T {
        self.inner
    }
}

// ── 反序列化前置检查 ──

#[derive(Debug, Clone)]
pub struct DeserializeGuard {
    pub max_payload_bytes: usize,
    pub max_depth: usize,
    pub max_items: usize,
    pub max_string_len: usize,
    pub max_keys: usize,
}

impl Default for DeserializeGuard {
    fn default() -> Self {
        Self {
            max_payload_bytes: 1_048_576,
            max_depth: 64,
            max_items: 10_000,
            max_string_len: 1_048_576,
            max_keys: 1_000,
        }
    }
}

impl DeserializeGuard {
    /// 所有 execute() 入口必须在 serde_json::from_value 之前调用此方法。
    /// 除局部深度/数组/键/字符串约束外，还按 JSON 编码形状执行聚合字节预算；
    /// 这样 Registry/Simulation/模块内调用不能绕过仅存在于 HTTP 层的 body limit。
    pub fn check_raw(&self, payload: &Value) -> Result<(), String> {
        let mut encoded_bytes = 0usize;
        self.check_value(payload, 0, &mut encoded_bytes)
    }

    fn payload_too_large(&self) -> String {
        serde_json::to_string(&ErrorPayload {
            category: "val".into(),
            code: "VAL_PAYLOAD_TOO_LARGE".into(),
            message: format!("JSON 聚合大小超过 {} 字节", self.max_payload_bytes),
            field: None,
            context: None,
        })
        .unwrap_or_default()
    }

    fn charge_bytes(&self, encoded_bytes: &mut usize, additional: usize) -> Result<(), String> {
        *encoded_bytes = encoded_bytes
            .checked_add(additional)
            .ok_or_else(|| self.payload_too_large())?;
        if *encoded_bytes > self.max_payload_bytes {
            return Err(self.payload_too_large());
        }
        Ok(())
    }

    fn encoded_string_bytes(value: &str) -> usize {
        let content = value.chars().fold(0usize, |total, ch| {
            let encoded = match ch {
                '"' | '\\' => 2,
                '\u{0008}' | '\u{0009}' | '\u{000A}' | '\u{000C}' | '\u{000D}' => 2,
                control if control <= '\u{001F}' => 6,
                other => other.len_utf8(),
            };
            total.saturating_add(encoded)
        });
        content.saturating_add(2)
    }

    fn check_value(
        &self,
        value: &Value,
        depth: usize,
        encoded_bytes: &mut usize,
    ) -> Result<(), String> {
        if depth > self.max_depth {
            return Err(serde_json::to_string(&ErrorPayload {
                category: "val".into(),
                code: "VAL_PAYLOAD_TOO_DEEP".into(),
                message: format!("JSON 嵌套深度超过 {}", self.max_depth),
                field: None,
                context: None,
            })
            .unwrap_or_default());
        }

        match value {
            Value::Null => self.charge_bytes(encoded_bytes, 4)?,
            Value::Bool(flag) => self.charge_bytes(encoded_bytes, if *flag { 4 } else { 5 })?,
            Value::Number(number) => self.charge_bytes(encoded_bytes, number.to_string().len())?,
            Value::String(value) => {
                if value.len() > self.max_string_len {
                    return Err(serde_json::to_string(&ErrorPayload {
                        category: "val".into(),
                        code: "VAL_STRING_TOO_LONG".into(),
                        message: format!("字符串长度超过 {}", self.max_string_len),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default());
                }
                self.charge_bytes(encoded_bytes, Self::encoded_string_bytes(value))?;
            }
            Value::Array(items) => {
                if items.len() > self.max_items {
                    return Err(serde_json::to_string(&ErrorPayload {
                        category: "val".into(),
                        code: "VAL_TOO_MANY_ITEMS".into(),
                        message: format!("数组元素数超过 {}", self.max_items),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default());
                }
                self.charge_bytes(encoded_bytes, 2)?; // []
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.charge_bytes(encoded_bytes, 1)?; // comma
                    }
                    self.check_value(item, depth + 1, encoded_bytes)?;
                }
            }
            Value::Object(map) => {
                if map.len() > self.max_keys {
                    return Err(serde_json::to_string(&ErrorPayload {
                        category: "val".into(),
                        code: "VAL_TOO_MANY_KEYS".into(),
                        message: format!("对象键数超过 {}", self.max_keys),
                        field: None,
                        context: None,
                    })
                    .unwrap_or_default());
                }
                self.charge_bytes(encoded_bytes, 2)?; // {}
                for (index, (key, nested)) in map.iter().enumerate() {
                    if index > 0 {
                        self.charge_bytes(encoded_bytes, 1)?; // comma
                    }
                    self.charge_bytes(encoded_bytes, Self::encoded_string_bytes(key))?;
                    self.charge_bytes(encoded_bytes, 1)?; // colon
                    self.check_value(nested, depth + 1, encoded_bytes)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod deserialize_guard_tests {
    use super::DeserializeGuard;
    use serde_json::json;

    #[test]
    fn enforces_aggregate_payload_bytes_across_nested_values() {
        let guard = DeserializeGuard {
            max_payload_bytes: 64,
            ..DeserializeGuard::default()
        };
        let error = guard
            .check_raw(&json!({"outer": {"value": "x".repeat(80)}}))
            .expect_err("nested payload must exceed aggregate byte budget");
        assert!(error.contains("VAL_PAYLOAD_TOO_LARGE"));
    }

    #[test]
    fn accounts_for_json_string_escaping_without_serializing_copy() {
        let guard = DeserializeGuard {
            max_payload_bytes: 16,
            ..DeserializeGuard::default()
        };
        assert!(guard.check_raw(&json!("line\nfeed")).is_ok());
        let error = guard
            .check_raw(&json!("line\nfeed\nmore"))
            .expect_err("escaped JSON string must be charged against byte budget");
        assert!(error.contains("VAL_PAYLOAD_TOO_LARGE"));
    }

    #[test]
    fn preserves_existing_local_array_budget() {
        let guard = DeserializeGuard {
            max_payload_bytes: 1024,
            max_items: 2,
            ..DeserializeGuard::default()
        };
        let error = guard
            .check_raw(&json!([1, 2, 3]))
            .expect_err("array item budget must remain enforced");
        assert!(error.contains("VAL_TOO_MANY_ITEMS"));
    }
}

// ── SQL 安全类型 ──

/// 编译期已知的静态 SQL 字符串
/// 只能从字符串字面量或白名单函数构造——无法从运行时字符串构建
#[derive(Debug, Clone)]
pub struct StaticSql {
    #[allow(dead_code)]
    sql: &'static str,
}

impl StaticSql {
    pub const fn from_literal(sql: &'static str) -> Self {
        Self { sql }
    }
    pub fn from_whitelist(sql: &'static str) -> Self {
        Self { sql }
    }
}

#[macro_export]
macro_rules! sql {
    ($sql:literal) => {
        StaticSql::from_literal($sql)
    };
}

// ── DataStore 类型 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub rows_affected: u64,
    pub last_insert_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStoreHealth {
    pub connected: bool,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStorePoolStats {
    pub active: u32,
    pub idle: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStoreMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub backend: String,
}

// ── DataTransport 类型 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub protocol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportPriority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportOptions {
    pub timeout_ms: u64,
    pub retry_count: u32,
    pub retry_backoff_ms: u64,
    pub compress: bool,
    pub priority: TransportPriority,
}

impl Default for TransportOptions {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,
            retry_count: 3,
            retry_backoff_ms: 1000,
            compress: false,
            priority: TransportPriority::Normal,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportMessage {
    pub source: String,
    pub payload: Value,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportHealth {
    pub connected: bool,
    pub protocol: String,
}

// ── Trait 定义 ──

pub trait SystemModule: Send + Sync {
    fn metadata(&self) -> ModuleMetadata;
    fn init(&mut self, config: Value) -> Result<(), String>;
    /// Every executable command must declare its access, effects, and simulation behavior.
    fn commands(&self) -> Vec<CommandMetadata>;
    /// 所有 module 的 execute() 必须接受 ExecutionContext
    /// ctx 中的 trace_id 由 Runtime 传入，同一请求内的所有子模块共享
    fn execute(
        &self,
        command: &str,
        payload: Value,
        ctx: &ExecutionContext,
    ) -> Result<Value, String>;
    fn schema(&self) -> ModuleSchema;

    /// 进程关闭信号。清理连接池、刷新缓冲区、保存检查点。
    /// 默认无操作——模块按需覆盖。
    fn shutdown(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait Transaction: Send + Sync {
    async fn commit(self: Box<Self>) -> Result<(), String>;
    async fn rollback(self: Box<Self>) -> Result<(), String>;
    async fn execute(&self, query: &StaticSql, params: &[Value]) -> Result<QueryResult, String>;
    async fn query(&self, query: &StaticSql, params: &[Value]) -> Result<Vec<Value>, String>;
}

#[async_trait::async_trait]
pub trait DataStore: Send + Sync {
    fn metadata(&self) -> DataStoreMetadata;
    async fn connect(&mut self, connection_string: &str) -> Result<(), String>;
    async fn disconnect(&mut self) -> Result<(), String>;
    /// query 参数类型为 &StaticSql——不可使用 &str，编译期保证无运行时拼接
    async fn execute(&self, query: &StaticSql, params: &[Value]) -> Result<QueryResult, String>;
    /// 批量执行多条语句，使用同一事务
    async fn execute_batch(
        &self,
        queries: &[(&StaticSql, &[Value])],
    ) -> Result<Vec<QueryResult>, String>;
    async fn query(&self, query: &StaticSql, params: &[Value]) -> Result<Vec<Value>, String>;
    async fn query_one(&self, query: &StaticSql, params: &[Value])
    -> Result<Option<Value>, String>;
    /// 开始事务。SQLite 实现用 BEGIN IMMEDIATE，PostgreSQL 用 BEGIN READ COMMITTED。
    async fn begin(&self) -> Result<Box<dyn Transaction>, String>;
    /// 当前是否在活跃事务中
    fn in_transaction(&self) -> bool;
    fn health_check(&self) -> Result<DataStoreHealth, String>;
    fn pool_stats(&self) -> Option<DataStorePoolStats>;
}

#[async_trait::async_trait]
pub trait DataTransport: Send + Sync {
    fn metadata(&self) -> TransportMetadata;
    async fn connect(&mut self, endpoint: &str) -> Result<(), String>;
    async fn disconnect(&mut self) -> Result<(), String>;
    async fn send(
        &self,
        target: &str,
        payload: Value,
        options: TransportOptions,
    ) -> Result<Value, String>;
    async fn receive(&self, timeout_ms: u64) -> Result<Option<TransportMessage>, String>;
    fn health_check(&self) -> Result<TransportHealth, String>;
}

// ── SQL 方言抽象 ──

pub trait SqlDialect: Send + Sync {
    fn kind(&self) -> DataStoreKind;
    /// 参数占位符：$1 (PG) vs ? (SQLite)
    fn param_placeholder(&self, index: usize) -> String;
    /// 该后端是否支持 RETURNING 子句
    fn supports_returning(&self) -> bool;
    /// 生成 UPSERT 语句
    fn upsert(&self, table: &str, columns: &[&str], key_columns: &[&str]) -> String;
    /// 逻辑类型 → 后端物理类型
    fn map_type(&self, logical: &LogicalType) -> &'static str;

    /// 构建分页查询 SQL + 参数。
    ///
    /// 返回 `(data_sql, data_params, count_sql, count_params)`。
    /// 每个 dialect 自行实现 LIMIT/OFFSET 或 LIMIT + 窗口函数语法。
    fn build_paginated_query(
        &self,
        table_expr: &str,
        columns: &[String],
        column_defs: &std::collections::HashMap<String, query::ColumnDef>,
        filters: &[query::FilterRule],
        allowed_sorts: &[String],
        q: &query::DataQuery,
    ) -> Result<query::PaginatedQuery, query::QueryBuilderError>
    where
        Self: Sized,
    {
        // 默认实现（SQLite 风格：LIMIT ? OFFSET ?）
        query::build_default_paginated_query(
            self,
            table_expr,
            columns,
            column_defs,
            filters,
            allowed_sorts,
            q,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DataStoreKind {
    SQLite,
    PostgreSQL,
}

pub enum LogicalType {
    Text,
    Integer,
    Boolean,
    Timestamp,
    Json,
}

// The in-memory orchestration value types are available only through
// `experimental` with the non-default `experimental-orchestration` feature.

// ══════════════════════════════════════════════════════════════════
// AI Schema 导出
// ══════════════════════════════════════════════════════════════════

pub fn to_openai_function_schema(module: &dyn SystemModule) -> Value {
    let schema = module.schema();
    let functions: Vec<Value> = schema
        .commands
        .iter()
        .map(|cmd| {
            serde_json::json!({
                "name": format!("{}_{}", schema.name, cmd.name),
                "description": cmd.description,
                "parameters": cmd.input_schema.clone().unwrap_or(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                })),
            })
        })
        .collect();

    serde_json::json!({
        "module": schema.name,
        "functions": functions,
    })
}

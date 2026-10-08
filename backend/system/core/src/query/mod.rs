//! 数据库无关的分页查询构建器类型。
//!
//! 所有类型使用 `serde_json::Value` 作为参数载体，
//! 适配器层负责将 `Value` 转换为具体数据库的原生参数类型。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ── 查询规格 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataQuery {
    pub mode: String,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub filter: serde_json::Map<String, Value>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    30
}

fn default_limit() -> u32 {
    50
}

// ── 过滤规则 ──

#[derive(Debug, Clone)]
pub struct FilterRule {
    /// 查询参数中对应的 filter key（camelCase）
    pub key: String,
    /// 映射到的数据库列
    pub column: Vec<String>,
    /// 匹配方式
    pub r#match: FilterMatch,
    /// Between 配对的结束 key（如 "dateTo"），为 None 时从 key 推断
    pub pair_key: Option<String>,
}

#[derive(Debug, Clone)]
pub enum FilterMatch {
    Exact,
    Contains,
    Gte,
    Lte,
    Between,
}

// ── 列定义 ──

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub col_type: String,
    pub table: Option<String>,
    pub column: Option<String>,
    /// 原始 SQL 表达式（如 "COALESCE(o.status, '')"），优先级最高
    pub sql: Option<String>,
}

// ── 产出 ──

#[derive(Debug, Clone)]
pub struct PaginatedQuery {
    pub data_sql: String,
    pub data_params: Vec<Value>,
    pub count_sql: String,
    pub count_params: Vec<Value>,
}

// ── 错误 ──

#[derive(Debug)]
pub struct QueryBuilderError {
    pub error_type: String,
    pub message: String,
    pub field: Option<String>,
}

impl std::fmt::Display for QueryBuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.error_type, self.message)
    }
}

// ── 默认 build_paginated_query 实现 ──
///
/// 为 SqlDialect 提供默认的分页查询构建实现。
/// 使用 `dialect.param_placeholder()` 生成方言特定的参数占位符。
/// SQLite → `?`，PostgreSQL → `$1`, `$2`, ...
pub fn build_default_paginated_query(
    dialect: &dyn crate::SqlDialect,
    table_expr: &str,
    columns: &[String],
    column_defs: &HashMap<String, ColumnDef>,
    filters: &[FilterRule],
    allowed_sorts: &[String],
    query: &DataQuery,
) -> Result<PaginatedQuery, QueryBuilderError> {
    let select_cols = columns.join(", ");

    // 列解析
    let resolve_col = |col_name: &str| -> String {
        if let Some(def) = column_defs.get(col_name) {
            if let Some(ref sql) = def.sql {
                return sql.clone();
            }
            if let (Some(table), Some(col)) = (&def.table, &def.column) {
                return format!("{}.{}", table, col);
            }
        }
        col_name.to_string()
    };

    // WHERE 构建
    let mut where_parts: Vec<String> = Vec::new();
    let mut where_params: Vec<Value> = Vec::new();
    let mut param_counter: usize = 0;

    for rule in filters {
        let raw = query.filter.get(&rule.key);
        if raw.is_none() {
            continue;
        }
        let raw = raw.unwrap();

        let raw_str = match raw {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if raw_str.is_empty() {
            continue;
        }

        match rule.r#match {
            FilterMatch::Between => {
                let col = resolve_col(&rule.column[0]);
                let pair_key = rule.pair_key.as_ref().cloned().or_else(|| {
                    if rule.key.ends_with("From") {
                        Some(rule.key.replace("From", "To"))
                    } else {
                        None
                    }
                });

                let from_val = raw_str;
                let to_val = pair_key
                    .and_then(|pk| query.filter.get(&pk))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                param_counter += 1;
                let ph1 = dialect.param_placeholder(param_counter);
                if from_val.is_empty() && to_val.as_ref().is_some_and(|t| !t.is_empty()) {
                    param_counter += 1;
                    let ph2 = dialect.param_placeholder(param_counter);
                    where_parts.push(format!("({} BETWEEN {} AND {})", col, ph1, ph2));
                    where_params.push(Value::String(from_val));
                    where_params.push(Value::String(to_val.unwrap()));
                } else if from_val.is_empty() {
                    // Nothing to filter
                } else if to_val.as_ref().is_some_and(|t| !t.is_empty()) {
                    param_counter += 1;
                    let ph2 = dialect.param_placeholder(param_counter);
                    where_parts.push(format!("({} BETWEEN {} AND {})", col, ph1, ph2));
                    where_params.push(Value::String(from_val));
                    where_params.push(Value::String(to_val.unwrap()));
                } else {
                    where_parts.push(format!("{} >= {}", col, ph1));
                    where_params.push(Value::String(from_val));
                }
            }
            FilterMatch::Contains => {
                let cols: Vec<String> = rule.column.iter().map(|c| resolve_col(c)).collect();
                let values: Vec<String> = raw_str
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|s| !s.is_empty())
                    .map(|s: &str| s.to_string())
                    .collect();
                let mut parts: Vec<String> = Vec::new();
                for val in &values {
                    let mut inner_parts: Vec<String> = Vec::new();
                    for col_item in &cols {
                        param_counter += 1;
                        let ph = dialect.param_placeholder(param_counter);
                        inner_parts.push(format!("{} LIKE {}", col_item, ph));
                        where_params.push(Value::String(format!("%{}%", val)));
                    }
                    parts.push(format!("({})", inner_parts.join(" OR ")));
                }
                where_parts.push(format!("({})", parts.join(" AND ")));
            }
            ref m => {
                let col = resolve_col(&rule.column[0]);
                let op = match m {
                    FilterMatch::Exact => "=",
                    FilterMatch::Gte => ">=",
                    FilterMatch::Lte => "<=",
                    _ => continue,
                };
                param_counter += 1;
                let ph = dialect.param_placeholder(param_counter);
                where_parts.push(format!("{} {} {}", col, op, ph));
                where_params.push(Value::String(raw_str));
            }
        }
    }

    let where_clause = if where_parts.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_parts.join(" AND "))
    };

    // ORDER BY
    let order_clause = if let Some(ref sort_by) = query.sort_by {
        if allowed_sorts.contains(sort_by) {
            let col = resolve_col(sort_by);
            let order = match query.sort_order.as_deref() {
                Some("asc") => "ASC",
                _ => "DESC",
            };
            format!("ORDER BY {} {}", col, order)
        } else {
            "ORDER BY createdAt DESC".to_string()
        }
    } else {
        "ORDER BY createdAt DESC".to_string()
    };

    // LIMIT / OFFSET
    let (limit, offset) = match query.mode.as_str() {
        "offset" => (query.limit.clamp(1, 200), query.offset),
        _ => {
            let page_size = query.page_size.clamp(1, 200);
            let off = (query.page.max(1).saturating_sub(1)) * page_size;
            (page_size, off)
        }
    };

    let mut data_params = where_params.clone();
    data_params.push(Value::from(limit as i64));
    data_params.push(Value::from(offset as i64));

    let data_sql = format!(
        "SELECT {} FROM {} {} {} LIMIT ? OFFSET ?",
        select_cols, table_expr, where_clause, order_clause
    );

    let count_sql = format!(
        "SELECT COUNT(*) as total FROM {} {}",
        table_expr, where_clause
    );

    Ok(PaginatedQuery {
        data_sql,
        data_params,
        count_sql,
        count_params: where_params,
    })
}

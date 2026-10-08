#![allow(dead_code)]
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataQuery {
    pub mode: String, // "page" | "offset"
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_page_size")]
    pub page_size: u32,
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub filter: serde_json::Map<String, serde_json::Value>,
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

#[derive(Debug, Clone)]
pub struct FilterRule {
    pub key: String,
    pub column: Vec<String>,
    pub r#match: FilterMatch,
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

#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub col_type: String,
    pub table: Option<String>,
    pub column: Option<String>,
    pub sql: Option<String>,
}

#[derive(Debug)]
pub struct PaginatedQuery {
    pub data_sql: String,
    pub data_params: Vec<Value>,
    pub count_sql: String,
    pub count_params: Vec<Value>,
}

pub struct QueryBuilder {
    table_expr: String,
    columns: Vec<String>,
    column_defs: std::collections::HashMap<String, ColumnDef>,
    filters: Vec<FilterRule>,
    allowed_sorts: Vec<String>,
}

#[derive(Debug)]
pub struct QueryBuilderError {
    pub error_type: String,
    pub message: String,
    pub field: Option<String>,
}

impl QueryBuilder {
    pub fn new(
        table_expr: String,
        columns: Vec<String>,
        column_defs: std::collections::HashMap<String, ColumnDef>,
        filters: Vec<FilterRule>,
        allowed_sorts: Vec<String>,
    ) -> Self {
        Self {
            table_expr,
            columns,
            column_defs,
            filters,
            allowed_sorts,
        }
    }

    fn resolve_col(&self, col_name: &str) -> String {
        if let Some(def) = self.column_defs.get(col_name) {
            if let Some(ref sql) = def.sql {
                return sql.clone();
            }
            if let (Some(table), Some(col)) = (&def.table, &def.column) {
                return format!("{}.{}", table, col);
            }
        }
        col_name.to_string()
    }

    pub fn build(&self, query: &DataQuery) -> Result<PaginatedQuery, QueryBuilderError> {
        let select_cols = self.columns.join(", ");

        // WHERE
        let mut where_parts: Vec<String> = Vec::new();
        let mut where_params: Vec<Value> = Vec::new();

        for rule in &self.filters {
            let raw = query.filter.get(&rule.key);
            if raw.is_none() {
                continue;
            }
            let raw = raw.unwrap();

            let raw_str = match raw {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if raw_str.is_empty() {
                continue;
            }

            match rule.r#match {
                FilterMatch::Between => {
                    let col = self.resolve_col(&rule.column[0]);
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
                        .map(|v| v.as_str().unwrap_or("").to_string());

                    if !from_val.is_empty() && to_val.as_ref().is_some_and(|t| !t.is_empty()) {
                        let t = to_val.unwrap();
                        where_parts.push(format!("({} BETWEEN ? AND ?)", col));
                        where_params.push(Value::Text(from_val));
                        where_params.push(Value::Text(t));
                    } else if !from_val.is_empty() {
                        where_parts.push(format!("{} >= ?", col));
                        where_params.push(Value::Text(from_val));
                    } else if let Some(t) = to_val
                        && !t.is_empty()
                    {
                        where_parts.push(format!("{} <= ?", col));
                        where_params.push(Value::Text(t));
                    }
                }
                FilterMatch::Contains => {
                    let cols: Vec<String> =
                        rule.column.iter().map(|c| self.resolve_col(c)).collect();
                    let values: Vec<String> = raw_str
                        .split(|c: char| c.is_whitespace() || c == ',')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect();
                    let mut parts: Vec<String> = Vec::new();
                    for val in &values {
                        let inner = cols
                            .iter()
                            .map(|col| format!("{} LIKE ?", col))
                            .collect::<Vec<_>>()
                            .join(" OR ");
                        parts.push(format!("({})", inner));
                        for _ in 0..cols.len() {
                            where_params.push(Value::Text(format!("%{}%", val)));
                        }
                    }
                    where_parts.push(format!("({})", parts.join(" AND ")));
                }
                ref m => {
                    let col = self.resolve_col(&rule.column[0]);
                    let op = match m {
                        FilterMatch::Exact => "=",
                        FilterMatch::Gte => ">=",
                        FilterMatch::Lte => "<=",
                        _ => continue,
                    };
                    where_parts.push(format!("{} {} ?", col, op));
                    where_params.push(Value::Text(raw_str));
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
            if self.allowed_sorts.contains(sort_by) {
                let col = self.resolve_col(sort_by);
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
        data_params.push(Value::Integer(limit as i64));
        data_params.push(Value::Integer(offset as i64));

        let data_sql = format!(
            "SELECT {} FROM {} {} {} LIMIT ? OFFSET ?",
            select_cols, self.table_expr, where_clause, order_clause
        );

        let count_sql = format!(
            "SELECT COUNT(*) as total FROM {} {}",
            self.table_expr, where_clause
        );

        Ok(PaginatedQuery {
            data_sql,
            data_params,
            count_sql,
            count_params: where_params,
        })
    }
}

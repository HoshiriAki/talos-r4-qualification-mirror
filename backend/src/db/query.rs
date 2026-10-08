//! SQL dialect abstraction for SQLite/PostgreSQL migration (ADR-0003).

pub trait QueryBuilder: Send + Sync {
    fn param_placeholder(&self, idx: usize) -> String;
    fn current_timestamp_sql(&self) -> &'static str;
    fn upsert_sql(&self, table: &str, columns: &[&str], key_columns: &[&str]) -> String;
    fn random_uuid_sql(&self) -> &'static str;
    fn dialect_name(&self) -> &'static str;
}

pub struct PostgresDialect;

impl QueryBuilder for PostgresDialect {
    fn param_placeholder(&self, idx: usize) -> String {
        format!("${idx}")
    }

    fn current_timestamp_sql(&self) -> &'static str {
        "NOW() AT TIME ZONE 'Asia/Shanghai'"
    }

    fn random_uuid_sql(&self) -> &'static str {
        "gen_random_uuid()"
    }

    fn upsert_sql(&self, table: &str, columns: &[&str], key_columns: &[&str]) -> String {
        let cols = columns.join(", ");
        let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("${i}")).collect();
        let key_cols = key_columns.join(", ");
        let updates: Vec<String> = columns
            .iter()
            .filter(|c| !key_columns.contains(c))
            .map(|c| format!("{c} = EXCLUDED.{c}"))
            .collect();
        format!(
            "INSERT INTO {table} ({cols}) VALUES ({}) ON CONFLICT ({key_cols}) DO UPDATE SET {}",
            placeholders.join(", "),
            updates.join(", ")
        )
    }

    fn dialect_name(&self) -> &'static str {
        "postgres"
    }
}

impl dyn QueryBuilder {
    /// Legacy construction seam retained for callers while R4-P8 removes
    /// duplicate database-profile authority. This abstraction currently only
    /// implements PostgreSQL SQL; SQLite business paths use their native
    /// repository implementations and do not select a dialect here.
    pub fn from_env() -> Box<dyn QueryBuilder> {
        Box::new(PostgresDialect)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgres_params_numbered() {
        let d = PostgresDialect;
        assert_eq!(d.param_placeholder(1), "$1");
        assert_eq!(d.param_placeholder(3), "$3");
    }

    #[test]
    fn test_postgres_upsert_contains_conflict() {
        let d = PostgresDialect;
        let sql = d.upsert_sql("widgets", &["id", "name", "color"], &["id"]);
        assert!(sql.contains("ON CONFLICT"));
        assert!(sql.contains("EXCLUDED"));
    }

    #[test]
    fn test_dialect_name() {
        assert_eq!(PostgresDialect.dialect_name(), "postgres");
    }

    #[test]
    fn legacy_factory_has_no_backend_environment_authority() {
        assert_eq!(<dyn QueryBuilder>::from_env().dialect_name(), "postgres");
    }
}

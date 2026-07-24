use crate::backend::Dialect;

pub(crate) struct MySqlDialect;

impl Dialect for MySqlDialect {
    fn database_info(&self) -> &str {
        "SELECT VERSION() AS version, DATABASE() AS `database`, CURRENT_USER() AS `current_user`, \
         @@hostname AS hostname, @@port AS port, @@version_compile_os AS os, \
         @@character_set_server AS charset, @@collation_server AS collation, \
         @@version_comment AS version_comment"
    }

    fn list_tables(&self) -> &str {
        "SELECT t.TABLE_SCHEMA AS schema_name, t.TABLE_NAME AS table_name, \
         t.TABLE_TYPE AS table_type, t.ENGINE AS engine, t.TABLE_ROWS AS row_count, \
         t.DATA_LENGTH + t.INDEX_LENGTH AS total_size, t.TABLE_COMMENT AS `comment` \
         FROM information_schema.TABLES t \
         WHERE t.TABLE_SCHEMA NOT IN ('mysql', 'information_schema', 'performance_schema', 'sys') \
         ORDER BY t.TABLE_SCHEMA, t.TABLE_NAME"
    }

    fn table_columns(&self) -> &str {
        "SELECT c.COLUMN_NAME AS column_name, c.COLUMN_TYPE AS data_type, \
         IF(c.IS_NULLABLE = 'YES', true, false) AS nullable, \
         c.COLUMN_DEFAULT AS default_value, c.ORDINAL_POSITION AS ordinal_position, \
         c.COLUMN_COMMENT AS `comment`, c.COLUMN_KEY AS column_key \
         FROM information_schema.COLUMNS c \
         WHERE c.TABLE_SCHEMA = ? AND c.TABLE_NAME = ? \
         ORDER BY c.ORDINAL_POSITION"
    }

    fn table_indexes(&self) -> &str {
        "SELECT s.INDEX_NAME AS index_name, NOT s.NON_UNIQUE AS is_unique, \
         IF(s.INDEX_NAME = 'PRIMARY', true, false) AS is_primary, \
         GROUP_CONCAT(s.COLUMN_NAME ORDER BY s.SEQ_IN_INDEX SEPARATOR ', ') AS columns, \
         s.INDEX_TYPE AS index_type \
         FROM information_schema.STATISTICS s \
         WHERE s.TABLE_SCHEMA = ? AND s.TABLE_NAME = ? \
         GROUP BY s.INDEX_NAME, s.NON_UNIQUE, s.INDEX_TYPE \
         ORDER BY s.INDEX_NAME"
    }

    fn read_only_prefixes(&self) -> &[&str] {
        &["SELECT", "EXPLAIN", "SHOW", "DESC", "DESCRIBE"]
    }

    fn add_limit(&self, sql: &str, n: usize) -> String {
        let upper = sql.trim().to_uppercase();
        if upper.contains("LIMIT") || upper.contains("TOP ") {
            sql.trim().to_string()
        } else {
            format!("{} LIMIT {}", sql.trim(), n)
        }
    }

    fn build_explain(&self, sql: &str, analyze: bool, format: &str) -> String {
        if analyze {
            format!("EXPLAIN ANALYZE {}", sql)
        } else {
            let format_clause = match format.to_uppercase().as_str() {
                "JSON" => "FORMAT=JSON",
                _ => "",
            };
            if format_clause.is_empty() {
                format!("EXPLAIN {}", sql)
            } else {
                format!("EXPLAIN {} {}", format_clause, sql)
            }
        }
    }

    fn set_statement_timeout_sql(&self, ms: u64) -> Option<String> {
        Some(format!("SET max_execution_time = {}", ms))
    }

    fn kill_own_connection_sql(&self) -> Option<String> {
        Some("KILL CONNECTION CONNECTION_ID()".to_string())
    }

    fn default_port(&self) -> u16 {
        3306
    }

    fn url_scheme(&self) -> &str {
        "mysql"
    }

    fn identifier_quote(&self) -> char {
        '`'
    }

    fn supports_hash_comment(&self) -> bool {
        true
    }
}

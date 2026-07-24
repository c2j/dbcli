pub(crate) mod conn;
pub(crate) mod error;
pub(crate) mod pool;
pub(crate) mod types;

use std::sync::Arc;

use async_trait::async_trait;

use crate::backend::error::DbError;
use crate::backend::{BackendFactory, DbPool, Dialect};
use crate::config::TimeoutConfig;

use self::pool::create_gaussdb_pool;

pub(crate) struct GaussdbFactory;

#[async_trait]
impl BackendFactory for GaussdbFactory {
    fn name(&self) -> &str {
        "GaussDB"
    }

    fn scheme(&self) -> &str {
        "gaussdb"
    }

    fn create_dialect(&self) -> Box<dyn Dialect> {
        Box::new(GaussdbDialect)
    }

    async fn connect(
        &self,
        url: &str,
        _timeout_config: Option<&TimeoutConfig>,
    ) -> Result<Arc<dyn DbPool>, DbError> {
        let pool = create_gaussdb_pool(url).await?;
        Ok(Arc::new(pool))
    }
}

pub(crate) struct GaussdbDialect;

impl Dialect for GaussdbDialect {
    fn database_info(&self) -> &str {
        "SELECT version()::text AS version, current_database()::text AS database, current_user::text AS current_user, inet_server_addr()::text AS hostname, inet_server_port()::text AS port, NULL::text AS os, (SELECT setting FROM pg_settings WHERE name='server_encoding')::text AS charset, (SELECT setting FROM pg_settings WHERE name='lc_collate')::text AS collation, NULL::text AS version_comment"
    }

    fn list_tables(&self) -> &str {
        "SELECT n.nspname AS schema_name, c.relname AS table_name, CASE c.relkind WHEN 'r' THEN 'table' WHEN 'v' THEN 'view' WHEN 'm' THEN 'materialized_view' WHEN 'f' THEN 'foreign_table' WHEN 'p' THEN 'partitioned_table' END AS table_type, NULL AS engine, c.reltuples::bigint AS row_count, pg_total_relation_size(c.oid) AS total_size, obj_description(c.oid, 'pg_class') AS comment FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind IN ('r','v','m','f','p') AND n.nspname NOT IN ('pg_catalog','information_schema') ORDER BY n.nspname, c.relname"
    }

    fn table_columns(&self) -> &str {
        "SELECT a.attname::text AS column_name, pg_catalog.format_type(a.atttypid, a.atttypmod)::text AS data_type, NOT a.attnotnull AS nullable, pg_catalog.pg_get_expr(d.adbin, d.adrelid)::text AS default_value, a.attnum::int4 AS ordinal_position, col_description(a.attrelid, a.attnum)::text AS comment, ic.relname::text AS column_key FROM pg_catalog.pg_attribute a LEFT JOIN pg_catalog.pg_attrdef d ON (a.attrelid = d.adrelid AND a.attnum = d.adnum) LEFT JOIN (pg_catalog.pg_index ix JOIN pg_catalog.pg_class ic ON ic.oid = ix.indexrelid AND ix.indisprimary) ON (ix.indrelid = a.attrelid AND a.attnum = ANY(ix.indkey)) WHERE a.attrelid = (SELECT c.oid FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE c.relname = $2 AND n.nspname = $1) AND NOT a.attisdropped AND attnum > 0 ORDER BY a.attnum"
    }

    fn table_indexes(&self) -> &str {
        "SELECT i.relname::text AS index_name, ix.indisunique AS is_unique, ix.indisprimary AS is_primary, pg_catalog.pg_get_indexdef(ix.indexrelid)::text AS columns, am.amname::text AS index_type FROM pg_catalog.pg_index ix JOIN pg_catalog.pg_class t ON t.oid = ix.indrelid JOIN pg_catalog.pg_class i ON i.oid = ix.indexrelid JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace JOIN pg_catalog.pg_am am ON am.oid = i.relam WHERE n.nspname = $1 AND t.relname = $2 ORDER BY i.relname"
    }

    fn read_only_prefixes(&self) -> &[&str] {
        &["SELECT", "EXPLAIN", "WITH"]
    }

    fn add_limit(&self, sql: &str, n: usize) -> String {
        if sql.to_uppercase().contains("LIMIT") {
            return sql.to_string();
        }
        format!("{}\nLIMIT {}", sql, n)
    }

    fn build_explain(&self, sql: &str, analyze: bool, format: &str) -> String {
        let fmt = match format.to_uppercase().as_str() {
            "TEXT" | "XML" | "JSON" | "YAML" => format.to_uppercase(),
            _ => "TEXT".to_string(),
        };
        if analyze {
            format!("EXPLAIN (ANALYZE, BUFFERS, FORMAT {}) {}", fmt, sql)
        } else {
            format!("EXPLAIN (FORMAT {}) {}", fmt, sql)
        }
    }

    fn set_statement_timeout_sql(&self, ms: u64) -> Option<String> {
        Some(format!("SET statement_timeout = {}", ms))
    }

    fn kill_own_connection_sql(&self) -> Option<String> {
        None
    }

    fn default_port(&self) -> u16 {
        5432
    }

    fn url_scheme(&self) -> &str {
        "gaussdb"
    }

    fn identifier_quote(&self) -> char {
        '"'
    }

    fn supports_hash_comment(&self) -> bool {
        false
    }

    fn supports_dollar_quote(&self) -> bool {
        true
    }
}

#[cfg(all(test, feature = "integration"))]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn gaussdb_connect_and_select_one() {
        let url = std::env::var("GAUSSDB_TEST_URL").unwrap_or_else(|_| {
            "host=127.0.0.1 port=5432 user=gaussdb password=Gaussdb@123 dbname=postgres".to_string()
        });
        let (client, connection) = gaussdb::connect(&url, gaussdb::NoTls)
            .await
            .expect("GaussDB connect failed; set GAUSSDB_TEST_URL or run docker");
        tokio::spawn(async move {
            let _ = connection.await;
        });

        let row = client
            .query_one("SELECT 1::int4 AS val", &[])
            .await
            .expect("query failed");
        let val: i32 = row.get(0);
        assert_eq!(val, 1);
    }
}

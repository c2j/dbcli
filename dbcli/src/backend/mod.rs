// ─── Database Backend Abstraction Layer ─────────────────────────────
//
// This module defines the trait interfaces that decouple the hepta_dbcli
// CLI + MCP server from any specific database driver. Each supported
// database (MySQL, Oracle, GaussDB, etc.) implements these traits in its own
// submodule under backend/.

pub mod error;
pub mod factory;
#[cfg(feature = "gaussdb")]
pub mod gaussdb;
pub mod mysql;
#[cfg(feature = "oracle-rs")]
pub mod oracle;
#[cfg(feature = "oracle")]
pub mod oracle_native;

use async_trait::async_trait;
use serde_json::Value;
use std::fmt;
use std::sync::Arc;

use crate::config::TimeoutConfig;
pub use error::DbError;

// ─── QueryResult ────────────────────────────────────────────────────

/// Unified query result — database-agnostic, normalized to JSON values.
/// This is the single data type that flows from any backend to the CLI/server layer.
#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub row_count: usize,
}

impl QueryResult {
    pub fn empty() -> Self {
        Self {
            columns: vec![],
            rows: vec![],
            row_count: 0,
        }
    }
}

impl fmt::Display for QueryResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "QueryResult {{ columns: {:?}, row_count: {} }}",
            self.columns, self.row_count
        )
    }
}

// ─── DbConn — Single Database Connection ────────────────────────────

/// A single database connection obtained from a pool.
/// All query methods consume `&mut self` because mysql_async requires it;
/// other backends (oracle-rs, etc.) get auto-deref and it's a no-op.
#[async_trait]
pub trait DbConn: Send {
    /// Execute a SQL query and return normalized results.
    /// The returned QueryResult has columns as strings and rows as Vec<serde_json::Value>.
    async fn query(&mut self, sql: &str) -> Result<QueryResult, DbError>;

    /// Execute a parameterized SQL query (e.g. for introspection with ? or :1 bindings).
    /// Parameters are passed as JSON values; each backend converts to native bind format.
    async fn exec(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult, DbError>;

    /// Execute a SQL statement that returns no rows (SET, ALTER SESSION, etc.).
    async fn query_drop(&mut self, sql: &str) -> Result<(), DbError>;

    /// Return a reference to the dialect associated with this connection.
    fn dialect(&self) -> &dyn Dialect;
}

// ─── DbPool — Connection Pool ───────────────────────────────────────

/// A pool of database connections. Each call to `acquire()` returns a
/// fresh or recycled connection from the pool.
#[async_trait]
pub trait DbPool: Send + Sync {
    /// Obtain a connection from the pool.
    async fn acquire(&self) -> Result<Box<dyn DbConn + Send>, DbError>;
}

// ─── Dialect — SQL Syntax & Introspection Adapter ───────────────────

/// Encapsulates all database-specific SQL syntax differences:
/// introspection queries, keyword lists, LIMIT/EXPLAIN generation,
/// connection parameters, and REPL tokenizer rules.
pub trait Dialect: Send + Sync {
    // ── Introspection SQL (returned as &str — all are compile-time constants) ──

    /// Query returning [version, database, current_user, hostname, port, os, charset, collation, version_comment]
    fn database_info(&self) -> &str;

    /// Query returning [schema_name, table_name, table_type, engine, row_count, total_size, comment]
    fn list_tables(&self) -> &str;

    /// Parameterized query (schema_name, table_name) returning
    /// [column_name, data_type, nullable, default_value, ordinal_position, comment, column_key]
    fn table_columns(&self) -> &str;

    /// Parameterized query (schema_name, table_name) returning
    /// [index_name, is_unique, is_primary, columns, index_type]
    fn table_indexes(&self) -> &str;

    // ── Syntax Adapters ──

    /// SQL statement prefixes that are considered read-only for MCP enforcement.
    fn read_only_prefixes(&self) -> &[&str];

    /// Append a row-limiting clause to a SELECT query if it doesn't already have one.
    fn add_limit(&self, sql: &str, n: usize) -> String;

    /// Build an EXPLAIN (or EXPLAIN ANALYZE) statement in the requested format.
    fn build_explain(&self, sql: &str, analyze: bool, format: &str) -> String;

    /// Return the SQL to set per-statement timeout, or None if not supported.
    /// Called before each MCP query to apply the per-call timeout_ms.
    fn set_statement_timeout_sql(&self, ms: u64) -> Option<String>;

    /// Return the SQL to kill the current connection, or None if not supported.
    /// Used for timeout_action=disconnect to force pool recycling.
    fn kill_own_connection_sql(&self) -> Option<String>;

    // ── Connection Metadata ──

    /// Default TCP port for this database.
    fn default_port(&self) -> u16;

    /// URL scheme (e.g. "mysql", "oracle").
    fn url_scheme(&self) -> &str;

    // ── REPL Tokenizer Adapters ──

    /// Character used to quote identifiers (backtick ` for MySQL, double-quote " for Oracle).
    fn identifier_quote(&self) -> char;

    /// Whether this database supports # as a line-comment token (MySQL yes, Oracle no).
    fn supports_hash_comment(&self) -> bool;

    /// Whether this database supports $...$ dollar-quoting (PostgreSQL/GaussDB yes, MySQL/Oracle no).
    fn supports_dollar_quote(&self) -> bool {
        false
    }
}

// ─── BackendFactory — Creates Backends from Configuration ───────────

/// A factory that creates DbPool instances and Dialect objects for a
/// specific database backend. One factory exists per supported database
/// type and can create many connections/pools.
#[async_trait]
pub trait BackendFactory: Send + Sync {
    /// Human-readable name (e.g. "MySQL", "Oracle").
    fn name(&self) -> &str;

    /// URL scheme this factory handles (e.g. "mysql", "oracle").
    fn scheme(&self) -> &str;

    /// Create a dialect instance for this backend.
    fn create_dialect(&self) -> Box<dyn Dialect>;

    /// Create a connection pool from a fully-resolved connection URL.
    async fn connect(
        &self,
        url: &str,
        timeout_config: Option<&TimeoutConfig>,
    ) -> Result<Arc<dyn DbPool>, DbError>;
}

// ─── Scheme-level Defaults (pre-connection lookups) ──────────────────

/// Default TCP port for a database scheme. Used by config resolution
/// before any BackendFactory is instantiated. Each backend's
/// Dialect::default_port() MUST agree with this lookup.
pub(crate) fn default_port_for_scheme(scheme: &str) -> u16 {
    match scheme {
        "oracle" => 1521,
        "gaussdb" => 5432,
        _ => 3306, // mysql and unknown
    }
}

/// SSL URL query parameter for a database scheme. Each backend's
/// driver must accept this format. Used by config URL building.
pub(crate) fn ssl_url_param_for_scheme(scheme: &str) -> &'static str {
    match scheme {
        "oracle" => "",
        "gaussdb" => "?sslmode=require",
        _ => "?ssl-mode=REQUIRED", // mysql
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_port_mysql() {
        assert_eq!(default_port_for_scheme("mysql"), 3306);
    }

    #[test]
    fn test_default_port_oracle() {
        assert_eq!(default_port_for_scheme("oracle"), 1521);
    }

    #[test]
    fn test_default_port_gaussdb() {
        assert_eq!(default_port_for_scheme("gaussdb"), 5432);
    }

    #[test]
    fn test_default_port_unknown_fallsback() {
        assert_eq!(default_port_for_scheme("unknown_db"), 3306);
    }

    #[test]
    fn test_ssl_url_param_mysql() {
        assert_eq!(ssl_url_param_for_scheme("mysql"), "?ssl-mode=REQUIRED");
    }

    #[test]
    fn test_ssl_url_param_oracle() {
        assert_eq!(ssl_url_param_for_scheme("oracle"), "");
    }

    #[test]
    fn test_ssl_url_param_gaussdb() {
        assert_eq!(ssl_url_param_for_scheme("gaussdb"), "?sslmode=require");
    }
}

use mysql_async::prelude::Queryable;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, ErrorData as McpError},
    tool, tool_handler, tool_router, ServerHandler,
};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::config::TimeoutConfig;
use crate::connection;
use crate::output;
use crate::queries;

pub(crate) fn format_error_chain(err: &dyn std::error::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut source = err.source();
    while let Some(e) = source {
        parts.push(e.to_string());
        source = e.source();
    }
    parts.join(" | caused by: ")
}

pub(crate) fn redact_url(url: &str) -> String {
    // mysql://user:password@host:port/db -> mask password
    if let Some(at_pos) = url.find('@') {
        if let Some(colon_pos) = url[..at_pos].rfind(':') {
            let prefix = &url[..colon_pos + 1];
            let suffix = &url[at_pos..];
            return format!("{}****{}", prefix, suffix);
        }
    }
    url.to_string()
}

fn connection_error(url: &str, err: &dyn std::error::Error) -> McpError {
    let chain = format_error_chain(err);
    let redacted = redact_url(url);
    error!(
        "database connection failed: {} (target: {})",
        chain, redacted
    );
    McpError::internal_error(
        format!("Database connection failed: {}", chain),
        Some(json!({
            "target": redacted,
            "hints": [
                "Check if the database server is running",
                "Verify host, port, user, and password in the connection string",
                "Ensure network connectivity and firewall rules allow the connection",
                "Check if SSL/TLS is required (use ssl-mode=REQUIRED in URL)",
            ]
        })),
    )
}

fn query_error(tool: &str, sql: &str, err: &str) -> McpError {
    let sql_preview = if sql.len() > 200 {
        format!("{}...", &sql[..200])
    } else {
        sql.to_string()
    };

    error!("{} failed: {} (sql: {})", tool, err, sql_preview);

    McpError::internal_error(
        format!("{} failed: {}", tool, err),
        Some(json!({
            "sql": sql_preview,
        })),
    )
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ConnectionNameParams {
    #[serde(default)]
    pub connection_name: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetTableMetadataParams {
    pub table_name: String,
    pub schema_name: Option<String>,
    #[serde(default)]
    pub connection_name: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExecuteQueryParams {
    pub sql: String,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_rows: Option<usize>,
    #[serde(default)]
    pub connection_name: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetExecutionPlanParams {
    pub sql: String,
    pub analyze: Option<bool>,
    /// Output format: "TEXT" (default, equivalent to FORMAT=TRADITIONAL) or "JSON"
    #[serde(default)]
    pub format: Option<String>,
    /// Optional per-call statement timeout in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub connection_name: Option<String>,
}

/// Append `LIMIT N` to a SQL SELECT query if it does not already have LIMIT.
fn append_limit_if_needed(sql: &str, max_rows: usize) -> String {
    let trimmed = sql.trim();
    let upper = trimmed.to_uppercase();
    // Check for existing LIMIT or TOP clause
    if !upper.contains("LIMIT") && !upper.contains("TOP ") {
        format!("{} LIMIT {}", trimmed, max_rows)
    } else {
        trimmed.to_string()
    }
}

type ResolveFn = Arc<dyn (Fn() -> Result<String, String>) + Send + Sync>;

struct ConnectionPool {
    pool: Arc<mysql_async::Pool>,
    url: String,
    timeout_config: TimeoutConfig,
    connected_at: Instant,
}

enum ConnectionState {
    Pending(ResolveFn),
    Connecting {
        url: String,
        timeout_config: TimeoutConfig,
    },
    Connected(ConnectionPool),
    Unavailable(String),
}

pub struct MysqlMcp {
    connections: Arc<Mutex<HashMap<String, ConnectionState>>>,
    default_name: String,
    timeout_configs: HashMap<String, TimeoutConfig>,
}

impl MysqlMcp {
    pub fn new_multi_disconnected(
        entries: Vec<(String, String)>,
        default_name: String,
        timeout_configs: HashMap<String, TimeoutConfig>,
    ) -> Self {
        let mut connections = HashMap::new();
        for (name, url) in entries {
            let tc = timeout_configs.get(&name).cloned().unwrap_or_default();
            connections.insert(
                name,
                ConnectionState::Connecting {
                    url,
                    timeout_config: tc,
                },
            );
        }
        Self {
            connections: Arc::new(Mutex::new(connections)),
            default_name,
            timeout_configs,
        }
    }

    pub fn new_multi_lazy(
        entries: Vec<(String, ResolveFn)>,
        default_name: String,
        timeout_configs: HashMap<String, TimeoutConfig>,
    ) -> Self {
        let mut connections = HashMap::new();
        for (name, resolver) in entries {
            connections.insert(name, ConnectionState::Pending(resolver));
        }
        Self {
            connections: Arc::new(Mutex::new(connections)),
            default_name,
            timeout_configs,
        }
    }

    pub async fn try_connect(&self) {
        let (name, url, tc) = {
            let conns = self.connections.lock().await;
            match conns.get(&self.default_name) {
                Some(ConnectionState::Connecting {
                    url,
                    timeout_config,
                }) => (
                    self.default_name.clone(),
                    url.clone(),
                    timeout_config.clone(),
                ),
                _ => return,
            }
        };

        info!("probing database connection '{}' at startup", name);
        let result = connection::do_connect(&url, Some(&tc)).await;

        let mut conns = self.connections.lock().await;
        match result {
            Ok((pool, _conn)) => {
                info!("startup probe: database '{}' connected successfully", name);
                conns.insert(
                    name,
                    ConnectionState::Connected(ConnectionPool {
                        pool,
                        url: url.clone(),
                        timeout_config: tc,
                        connected_at: Instant::now(),
                    }),
                );
            }
            Err(e) => {
                let chain = format_error_chain(e.as_ref());
                let redacted = redact_url(&url);
                error!(
                    "startup probe: database '{}' connection failed: {} (target: {})",
                    name, chain, redacted
                );
                conns.insert(name, ConnectionState::Unavailable(url));
            }
        }
    }

    async fn get_connection(
        &self,
        connection_name: Option<&str>,
    ) -> Result<(Arc<mysql_async::Pool>, mysql_async::Conn), McpError> {
        let name = connection_name.unwrap_or(&self.default_name).to_string();
        let conns = self.connections.lock().await;

        match conns.get(&name) {
            Some(ConnectionState::Connected(pool_state)) => {
                let pool = Arc::clone(&pool_state.pool);
                let tc = pool_state.timeout_config.clone();
                let url = pool_state.url.clone();

                // Check connection max lifetime
                if let Some(max_lifetime) = tc.connection_max_lifetime {
                    if pool_state.connected_at.elapsed() >= max_lifetime {
                        info!(
                            "connection '{}' exceeded max_lifetime ({:?}), recycling",
                            name, max_lifetime
                        );
                        drop(conns);
                        return self.connect_with_url(name, url).await;
                    }
                }

                match pool.get_conn().await {
                    Ok(conn) => {
                        drop(conns);
                        Ok((pool, conn))
                    }
                    Err(e) => {
                        error!("failed to get connection from pool for '{}': {}", name, e);
                        drop(conns);
                        self.connect_with_url(name, url).await
                    }
                }
            }
            Some(ConnectionState::Pending(resolver)) => {
                let resolver = Arc::clone(resolver);
                drop(conns);
                let url = resolver().map_err(|e| {
                    McpError::internal_error(
                        format!(
                            "Failed to resolve database credentials for '{}': {}",
                            name, e
                        ),
                        Some(json!({
                            "connection_name": name,
                            "hint": "Check your polar-mysql configuration and OS keychain access"
                        })),
                    )
                })?;
                info!(
                    "connection URL resolved for '{}', attempting database connection",
                    name
                );
                self.connect_with_url(name, url).await
            }
            Some(ConnectionState::Connecting { url, .. })
            | Some(ConnectionState::Unavailable(url)) => {
                let url = url.clone();
                drop(conns);
                info!("attempting database connection for '{}'", name);
                self.connect_with_url(name, url).await
            }
            None => {
                let available: Vec<&String> = conns.keys().collect();
                Err(McpError::invalid_request(
                    "unknown_connection",
                    Some(json!({
                        "message": format!("Connection '{}' not found", name),
                        "available_connections": available,
                        "default_connection": self.default_name,
                    })),
                ))
            }
        }
    }

    async fn connect_with_url(
        &self,
        name: String,
        url: String,
    ) -> Result<(Arc<mysql_async::Pool>, mysql_async::Conn), McpError> {
        let tc = self.timeout_configs.get(&name).cloned().unwrap_or_default();
        let result = connection::do_connect(&url, Some(&tc)).await;
        let mut conns = self.connections.lock().await;

        match result {
            Ok((pool, conn)) => {
                info!("database '{}' connected successfully", name);
                let conn_pool = ConnectionPool {
                    pool: Arc::clone(&pool),
                    url: url.clone(),
                    timeout_config: tc,
                    connected_at: Instant::now(),
                };
                conns.insert(name, ConnectionState::Connected(conn_pool));
                Ok((pool, conn))
            }
            Err(e) => {
                let err = connection_error(&url, e.as_ref());
                conns.insert(name, ConnectionState::Unavailable(url));
                Err(err)
            }
        }
    }
}

#[tool_router]
impl MysqlMcp {
    #[tool(description = "Get database version and server information")]
    async fn get_database_info(
        &self,
        Parameters(params): Parameters<ConnectionNameParams>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            "tool called: get_database_info connection={}",
            params.connection_name.as_deref().unwrap_or("(default)")
        );
        let (_pool, mut conn) = self
            .get_connection(params.connection_name.as_deref())
            .await?;

        let row: mysql_async::Row = match conn.query_first(queries::DATABASE_INFO).await {
            Ok(Some(row)) => row,
            Ok(None) => {
                return Err(McpError::internal_error(
                    "get_database_info returned no rows",
                    None,
                ));
            }
            Err(e) => {
                return Err(query_error(
                    "get_database_info",
                    queries::DATABASE_INFO,
                    &e.to_string(),
                ));
            }
        };

        let result = json!({
            "version": output::get_column_string(&row, 0),
            "database": output::get_column_string(&row, 1),
            "current_user": output::get_column_string(&row, 2),
            "hostname": output::get_column_string(&row, 3),
            "port": output::get_column_i32(&row, 4),
            "os": output::get_column_string(&row, 5),
            "charset": output::get_column_string(&row, 6),
            "collation": output::get_column_string(&row, 7),
            "version_comment": output::get_column_string(&row, 8),
        });

        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }

    #[tool(description = "List all user tables and views in the database")]
    async fn list_tables(
        &self,
        Parameters(params): Parameters<ConnectionNameParams>,
    ) -> Result<CallToolResult, McpError> {
        info!(
            "tool called: list_tables connection={}",
            params.connection_name.as_deref().unwrap_or("(default)")
        );
        let (_pool, mut conn) = self
            .get_connection(params.connection_name.as_deref())
            .await?;

        let rows: Vec<mysql_async::Row> = match conn.query(queries::LIST_TABLES).await {
            Ok(rows) => rows,
            Err(e) => {
                return Err(query_error(
                    "list_tables",
                    queries::LIST_TABLES,
                    &e.to_string(),
                ));
            }
        };

        let tables: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                json!({
                    "schema_name": output::get_column_string(row, 0),
                    "table_name": output::get_column_string(row, 1),
                    "table_type": output::get_column_string(row, 2),
                    "engine": output::get_column_string(row, 3),
                    "row_count": output::get_column_u64(row, 4),
                    "total_size": output::get_column_u64(row, 5),
                    "comment": output::get_column_string(row, 6),
                })
            })
            .collect();

        Ok(CallToolResult::success(vec![Content::text(
            json!(tables).to_string(),
        )]))
    }

    #[tool(description = "Get column metadata, primary keys, and indexes for a specific table")]
    async fn get_table_metadata(
        &self,
        Parameters(params): Parameters<GetTableMetadataParams>,
    ) -> Result<CallToolResult, McpError> {
        let schema = params.schema_name.as_deref().unwrap_or("public");
        let table = &params.table_name;
        let _name = params
            .connection_name
            .as_deref()
            .unwrap_or(&self.default_name)
            .to_string();
        info!(
            "tool called: get_table_metadata schema={} table={} connection={}",
            schema,
            table,
            params.connection_name.as_deref().unwrap_or("(default)")
        );
        let (_pool, mut conn) = self
            .get_connection(params.connection_name.as_deref())
            .await?;

        // Get columns
        let columns_rows: Vec<mysql_async::Row> =
            match conn.exec(queries::TABLE_COLUMNS, (schema, table)).await {
                Ok(rows) => rows,
                Err(e) => {
                    return Err(query_error(
                        "get_table_metadata (columns)",
                        queries::TABLE_COLUMNS,
                        &e.to_string(),
                    ));
                }
            };

        let columns: Vec<serde_json::Value> = columns_rows
            .iter()
            .map(|row| {
                json!({
                    "column_name": output::get_column_string(row, 0),
                    "data_type": output::get_column_string(row, 1),
                    "nullable": output::get_column_bool(row, 2),
                    "default_value": output::get_column_string(row, 3),
                    "ordinal_position": output::get_column_i32(row, 4),
                    "comment": output::get_column_string(row, 5),
                    "column_key": output::get_column_string(row, 6),
                })
            })
            .collect();

        // Get indexes
        let idx_rows: Vec<mysql_async::Row> =
            match conn.exec(queries::TABLE_INDEXES, (schema, table)).await {
                Ok(rows) => rows,
                Err(e) => {
                    return Err(query_error(
                        "get_table_metadata (indexes)",
                        queries::TABLE_INDEXES,
                        &e.to_string(),
                    ));
                }
            };

        let indexes: Vec<serde_json::Value> = idx_rows
            .iter()
            .map(|row| {
                json!({
                    "index_name": output::get_column_string(row, 0),
                    "is_unique": output::get_column_bool(row, 1),
                    "is_primary": output::get_column_bool(row, 2),
                    "columns": output::get_column_string(row, 3),
                    "index_type": output::get_column_string(row, 4),
                })
            })
            .collect();

        let result = json!({
            "columns": columns,
            "indexes": indexes,
        });

        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }

    #[tool(description = "Execute a read-only SQL query (SELECT or EXPLAIN only)")]
    async fn execute_query(
        &self,
        Parameters(params): Parameters<ExecuteQueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let trimmed = params.sql.trim();
        let _upper = trimmed.to_uppercase();
        let _name = params
            .connection_name
            .as_deref()
            .unwrap_or(&self.default_name)
            .to_string();
        debug!(
            "tool called: execute_query sql_len={} connection={}",
            trimmed.len(),
            params.connection_name.as_deref().unwrap_or("(default)")
        );

        if !crate::cli::is_read_only_mcp(trimmed) {
            error!(
                "execute_query rejected non-SELECT query: {:?}",
                &trimmed[..trimmed.len().min(80)]
            );
            return Err(McpError::invalid_request(
                "invalid_query",
                Some(
                    json!({ "message": "Only SELECT, EXPLAIN, SHOW, and DESCRIBE queries are allowed" }),
                ),
            ));
        }

        let (_pool, mut conn) = self
            .get_connection(params.connection_name.as_deref())
            .await?;

        // Apply per-query timeout if specified
        if let Some(timeout_ms) = params.timeout_ms {
            let set_sql = format!("SET max_execution_time = {}", timeout_ms);
            let _ = conn.query_drop(&set_sql).await;
        }

        // Enforce max_rows at the server level by appending LIMIT
        let max_rows = params.max_rows.unwrap_or(1000).clamp(1, 10000);
        let sql_to_execute = append_limit_if_needed(trimmed, max_rows);

        let rows: Vec<mysql_async::Row> = match conn.query(&sql_to_execute).await {
            Ok(rows) => rows,
            Err(e) => {
                return Err(query_error("execute_query", trimmed, &e.to_string()));
            }
        };

        if rows.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                json!({"columns": [], "rows": [], "row_count": 0}).to_string(),
            )]));
        }

        let total_count = rows.len();
        let truncated = total_count > max_rows;
        let visible = if truncated {
            &rows[..max_rows]
        } else {
            &rows[..]
        };

        let columns: Vec<String> = rows[0]
            .columns_ref()
            .iter()
            .map(|c| c.name_str().to_string())
            .collect();

        let mut result_rows: Vec<Vec<serde_json::Value>> = Vec::with_capacity(visible.len());
        for row in visible {
            let mut result_row: Vec<serde_json::Value> = Vec::with_capacity(columns.len());
            for idx in 0..columns.len() {
                result_row.push(output::format_row_value(row, idx));
            }
            result_rows.push(result_row);
        }

        let mut result = json!({
            "columns": columns,
            "rows": result_rows,
            "row_count": total_count,
        });
        if truncated {
            result["truncated"] = json!(true);
            result["hint"] = json!(format!(
                "Result exceeds max_rows ({max_rows}). Use CLI mode for full output.",
            ));
        }

        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }

    #[tool(description = "Get the execution plan for a SQL query")]
    async fn get_execution_plan(
        &self,
        Parameters(params): Parameters<GetExecutionPlanParams>,
    ) -> Result<CallToolResult, McpError> {
        let _name = params
            .connection_name
            .as_deref()
            .unwrap_or(&self.default_name)
            .to_string();
        info!(
            "tool called: get_execution_plan analyze={} connection={}",
            params.analyze.unwrap_or(false),
            params.connection_name.as_deref().unwrap_or("(default)")
        );

        let (_pool, mut conn) = self
            .get_connection(params.connection_name.as_deref())
            .await?;

        // Apply per-query timeout if specified
        if let Some(timeout_ms) = params.timeout_ms {
            let set_sql = format!("SET max_execution_time = {}", timeout_ms);
            let _ = conn.query_drop(&set_sql).await;
        }

        let analyze = params.analyze.unwrap_or(false);
        let fmt = params.format.as_deref().unwrap_or("TEXT");
        let explain_sql = if analyze {
            format!("EXPLAIN ANALYZE {}", params.sql)
        } else {
            let format_clause = match fmt.to_uppercase().as_str() {
                "JSON" => "FORMAT=JSON",
                _ => "", // TEXT / TRADITIONAL is the default EXPLAIN output
            };
            if format_clause.is_empty() {
                format!("EXPLAIN {}", params.sql)
            } else {
                format!("EXPLAIN {} {}", format_clause, params.sql)
            }
        };

        let rows: Vec<mysql_async::Row> = match conn.query(&explain_sql).await {
            Ok(rows) => rows,
            Err(e) => {
                return Err(query_error(
                    "get_execution_plan",
                    &explain_sql,
                    &e.to_string(),
                ));
            }
        };

        let plan: String = rows
            .iter()
            .filter_map(|row| row.get_opt::<String, usize>(0).and_then(|r| r.ok()))
            .collect::<Vec<String>>()
            .join("\n");

        let result = json!({ "plan": plan });

        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }

    #[tool(description = "List all configured database connections")]
    async fn list_connections(&self) -> Result<CallToolResult, McpError> {
        info!("tool called: list_connections");
        let conns = self.connections.lock().await;
        let connections: Vec<serde_json::Value> = conns
            .iter()
            .map(|(name, state)| {
                let status = match state {
                    ConnectionState::Connected { .. } => "connected",
                    ConnectionState::Connecting { .. } => "connecting",
                    ConnectionState::Pending(_) => "pending",
                    ConnectionState::Unavailable(_) => "unavailable",
                };
                json!({
                    "name": name,
                    "status": status,
                    "is_default": name == &self.default_name,
                })
            })
            .collect();

        let result = json!({
            "connections": connections,
            "default_connection": self.default_name,
        });

        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }
}

#[tool_handler(
    name = "polar-mysql",
    version = "0.1.8",
    instructions = "MCP server for MySQL/PolarDB-X database introspection with multi-connection support"
)]
impl ServerHandler for MysqlMcp {}

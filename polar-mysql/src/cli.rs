use std::path::PathBuf;
use std::str::FromStr;

use mysql_async::prelude::*;
use mysql_async::Pool;

use crate::config::{
    TimeoutConfig, read_config, resolve_env_var_connection, resolve_single_connection,
    rewrite_password_to_sentinel, store_keyring_password,
};
use crate::connection::do_connect;
use crate::output;
use crate::server::format_error_chain;

#[derive(Debug, Clone)]
pub(crate) struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum OutputFormat {
    Table,
    Json,
    Vertical,
    Csv,
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "json" => Ok(OutputFormat::Json),
            "vertical" => Ok(OutputFormat::Vertical),
            "csv" => Ok(OutputFormat::Csv),
            _ => Err(format!(
                "Unknown output format '{}'. Use table, json, vertical, or csv.",
                s
            )),
        }
    }
}

pub(crate) struct CliArgs {
    pub sql: Option<String>,
    pub file: Option<String>,
    pub connection_name: Option<String>,
    pub config_path: Option<String>,
    pub format: OutputFormat,
    pub statement_timeout: Option<String>,
    pub connection_max_lifetime: Option<String>,
    pub no_history: bool,
}

fn value_to_compact_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    }
}

fn strip_leading_comments(sql: &str) -> &str {
    let trimmed = sql.trim_start();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && &bytes[..2] == b"--" {
        match trimmed.find('\n') {
            Some(pos) => strip_leading_comments(&trimmed[pos + 1..]),
            None => "",
        }
    } else if bytes.len() >= 2 && &bytes[..2] == b"/*" {
        let mut depth: usize = 1;
        let mut i = 2;
        while i + 1 < bytes.len() && depth > 0 {
            if &bytes[i..i + 2] == b"/*" {
                depth += 1;
                i += 2;
            } else if &bytes[i..i + 2] == b"*/" {
                depth -= 1;
                i += 2;
            } else {
                i += 1;
            }
        }
        if depth == 0 {
            strip_leading_comments(&trimmed[i..])
        } else {
            ""
        }
    } else {
        trimmed
    }
}

#[allow(dead_code)]
fn is_read_only_query(sql: &str) -> bool {
    let trimmed = sql.trim();
    let stripped = strip_leading_comments(trimmed);
    let upper = stripped.to_uppercase();
    upper.starts_with("SELECT")
        || upper.starts_with("EXPLAIN")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESC")
        || upper.starts_with("DESCRIBE")
        || upper.starts_with("WITH")
}

/// Check if the SQL is a read-only SELECT/EXPLAIN/SHOW/DESC query for MCP enforcement.
pub(crate) fn is_read_only_mcp(sql: &str) -> bool {
    let trimmed = sql.trim();
    let stripped = strip_leading_comments(trimmed);
    let upper = stripped.to_uppercase();
    // For MCP, be strict: only allow SELECT, EXPLAIN, SHOW, DESCRIBE, DESC
    upper.starts_with("SELECT")
        || upper.starts_with("EXPLAIN")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESC")
        || upper.starts_with("DESCRIBE")
}

pub(crate) async fn execute_query(
    conn: &mut mysql_async::Conn,
    sql: &str,
) -> Result<QueryResult, String> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err("Empty SQL statement".to_string());
    }

    let rows: Vec<mysql_async::Row> = conn
        .query(trimmed)
        .await
        .map_err(|e| format!("Query failed: {}", e))?;

    if rows.is_empty() {
        return Ok(QueryResult {
            columns: vec![],
            rows: vec![],
            row_count: 0,
        });
    }

    let columns: Vec<String> = rows[0]
        .columns_ref()
        .iter()
        .map(|c| c.name_str().to_string())
        .collect();

    let mut result_rows: Vec<Vec<serde_json::Value>> = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut result_row: Vec<serde_json::Value> = Vec::with_capacity(columns.len());
        for idx in 0..columns.len() {
            result_row.push(output::format_row_value(row, idx));
        }
        result_rows.push(result_row);
    }

    Ok(QueryResult {
        columns,
        rows: result_rows,
        row_count: rows.len(),
    })
}

pub(crate) fn render_result(
    result: &QueryResult,
    writer: &mut dyn std::io::Write,
    format: OutputFormat,
) -> Result<(), String> {
    if result.columns.is_empty() {
        writeln!(writer, "(0 rows)").map_err(|e| format!("write error: {}", e))?;
        return Ok(());
    }

    match format {
        OutputFormat::Table => {
            let table_str = output::format_table(&result.columns, &result.rows);
            writeln!(writer, "{}", table_str).map_err(|e| format!("write error: {}", e))?;
            let count_label = if result.row_count == 1 {
                "1 row".to_string()
            } else {
                format!("{} rows", result.row_count)
            };
            writeln!(writer, "({})", count_label)
                .map_err(|e| format!("write error: {}", e))?;
        }
        OutputFormat::Json => {
            let v = serde_json::json!({
                "columns": result.columns,
                "rows": result.rows,
                "row_count": result.row_count,
            });
            writeln!(
                writer,
                "{}",
                serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
            )
            .map_err(|e| format!("write error: {}", e))?;
        }
        OutputFormat::Vertical => {
            for (row_idx, row) in result.rows.iter().enumerate() {
                writeln!(writer, "-[ RECORD {} ]-", row_idx + 1)
                    .map_err(|e| format!("write error: {}", e))?;
                for (col_idx, col_name) in result.columns.iter().enumerate() {
                    let val_str = value_to_compact_string(&row[col_idx]);
                    writeln!(writer, "{} | {}", col_name, val_str)
                        .map_err(|e| format!("write error: {}", e))?;
                }
            }
            let count_label = if result.row_count == 1 {
                "1 row".to_string()
            } else {
                format!("{} rows", result.row_count)
            };
            writeln!(writer, "({})", count_label)
                .map_err(|e| format!("write error: {}", e))?;
        }
        OutputFormat::Csv => {
            let mut csv_writer = csv::Writer::from_writer(writer);
            csv_writer
                .write_record(&result.columns)
                .map_err(|e| format!("write error: {}", e))?;
            for row in &result.rows {
                let str_row: Vec<String> = row.iter().map(value_to_compact_string).collect();
                csv_writer
                    .write_record(&str_row)
                    .map_err(|e| format!("write error: {}", e))?;
            }
            csv_writer
                .flush()
                .map_err(|e| format!("write error: {}", e))?;
        }
    }
    Ok(())
}

pub(crate) async fn run_cli(args: CliArgs) -> Result<(), String> {
    // 1. Get SQL from -c, -f, or stdin
    let sql = if let Some(s) = &args.sql {
        s.clone()
    } else if let Some(f) = &args.file {
        std::fs::read_to_string(f).map_err(|e| format!("Failed to read file '{}': {}", f, e))?
    } else {
        let mut input = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
            .map_err(|e| format!("Failed to read stdin: {}", e))?;
        input
    };

    let sql = sql.trim().to_string();
    if sql.is_empty() {
        return Err("No SQL provided. Use -c/--sql, -f/--file, or pipe SQL to stdin.".to_string());
    }

    // 2. Load config
    let config_path = args.config_path.map(PathBuf::from);
    let raw = read_config(config_path)?;

    // 3. Find target connection
    let target_name = args.connection_name.as_deref().unwrap_or(&raw.default_name);
    let target_conn = raw
        .connections
        .iter()
        .find(|c| c.name == target_name)
        .ok_or_else(|| {
            format!(
                "Connection '{}' not found. Available: {:?}",
                target_name,
                raw.connections.iter().map(|c| &c.name).collect::<Vec<_>>()
            )
        })?;

    let target = if raw.is_env_var {
        resolve_env_var_connection(target_conn.url.clone().unwrap())
    } else {
        resolve_single_connection(
            target_conn,
            raw.config_path.clone(),
            raw.base_timeout.as_ref(),
        )?
    };

    // 4. Build effective timeout
    let effective_timeout = TimeoutConfig::from_overrides(
        args.statement_timeout.as_deref(),
        args.connection_max_lifetime.as_deref(),
        Some(&target.timeout_config),
    )
    .map_err(|e| format!("Invalid timeout configuration: {}", e))?;

    // 5. Connect
    let (pool, mut conn) = do_connect(&target.connection_url, Some(&effective_timeout))
        .await
        .map_err(|e| format!("Connection failed: {}", format_error_chain(e.as_ref())))?;

    // 6. Migrate plaintext password to OS keychain on successful connection
    if let (Some(path), Some(plaintext)) = (&target.config_path, &target.plaintext_password) {
        match store_keyring_password(&target.keyring_username, plaintext) {
            Ok(()) => {
                if let Err(e) = rewrite_password_to_sentinel(path, &target.name) {
                    eprintln!(
                        "warning: password stored in keychain but failed to update config: {}",
                        e
                    );
                }
            }
            Err(e) => {
                eprintln!("warning: failed to migrate password to keychain: {}", e);
            }
        }
    }

    // 7. Execute SQL
    let result = execute_query(&mut conn, &sql).await?;

    // 8. Render output
    render_result(&result, &mut std::io::stdout(), args.format)?;

    // 9. Disconnect
    Pool::clone(&pool).disconnect().await.ok();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_leading_comments_plain() {
        assert_eq!(strip_leading_comments("SELECT 1"), "SELECT 1");
    }

    #[test]
    fn test_strip_leading_comments_line() {
        assert_eq!(
            strip_leading_comments("-- list users\nSELECT * FROM users"),
            "SELECT * FROM users"
        );
    }

    #[test]
    fn test_strip_leading_comments_block() {
        assert_eq!(strip_leading_comments("/* hint */ SELECT 1"), "SELECT 1");
    }

    #[test]
    fn test_strip_leading_comments_nested() {
        assert_eq!(
            strip_leading_comments("/* outer /* inner */ rest */ SELECT 1"),
            "SELECT 1"
        );
    }

    #[test]
    fn test_strip_leading_comments_multiple() {
        assert_eq!(
            strip_leading_comments("-- first\n-- second\n/* block */\nSELECT 1"),
            "SELECT 1"
        );
    }

    #[test]
    fn test_is_read_only_select() {
        assert!(is_read_only_query("SELECT 1"));
        assert!(is_read_only_query("select * from users"));
        assert!(is_read_only_query("  SELECT 1"));
    }

    #[test]
    fn test_is_read_only_explain() {
        assert!(is_read_only_query("EXPLAIN SELECT 1"));
        assert!(is_read_only_query("explain select 1"));
    }

    #[test]
    fn test_is_read_only_show() {
        assert!(is_read_only_query("SHOW TABLES"));
        assert!(is_read_only_query("show databases"));
    }

    #[test]
    fn test_is_read_only_describe() {
        assert!(is_read_only_query("DESC users"));
        assert!(is_read_only_query("DESCRIBE users"));
    }

    #[test]
    fn test_is_read_only_false() {
        assert!(!is_read_only_query("INSERT INTO t VALUES (1)"));
        assert!(!is_read_only_query("UPDATE t SET a=1"));
        assert!(!is_read_only_query("DELETE FROM t"));
        assert!(!is_read_only_query("DROP TABLE t"));
    }

    #[test]
    fn test_value_compact_null_and_string() {
        assert_eq!(value_to_compact_string(&serde_json::Value::Null), "NULL");
        assert_eq!(
            value_to_compact_string(&serde_json::Value::String("hello".into())),
            "hello"
        );
        assert_eq!(
            value_to_compact_string(&serde_json::Value::Bool(true)),
            "true"
        );
        assert_eq!(
            value_to_compact_string(&serde_json::Value::Number(serde_json::Number::from(42))),
            "42"
        );
    }

    #[test]
    fn test_render_result_empty_query() {
        let result = QueryResult {
            columns: vec![],
            rows: vec![],
            row_count: 0,
        };
        let mut buf: Vec<u8> = Vec::new();
        render_result(&result, &mut buf, OutputFormat::Table).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "(0 rows)\n");
    }

    #[test]
    fn test_render_result_table() {
        let result = QueryResult {
            columns: vec!["a".into(), "b".into()],
            rows: vec![vec![
                serde_json::Value::String("1".into()),
                serde_json::Value::String("2".into()),
            ]],
            row_count: 1,
        };
        let mut buf: Vec<u8> = Vec::new();
        render_result(&result, &mut buf, OutputFormat::Table).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains('│'));
        assert!(output.contains('─'));
    }

    #[test]
    fn test_render_result_csv() {
        let result = QueryResult {
            columns: vec!["a".into(), "b".into()],
            rows: vec![vec![
                serde_json::Value::String("1".into()),
                serde_json::Value::String("2".into()),
            ]],
            row_count: 1,
        };
        let mut buf: Vec<u8> = Vec::new();
        render_result(&result, &mut buf, OutputFormat::Csv).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let output = output.replace("\r\n", "\n");
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "a,b");
        assert_eq!(lines[1], "1,2");
    }

    #[test]
    fn test_render_result_vertical() {
        let result = QueryResult {
            columns: vec!["col".into()],
            rows: vec![vec![serde_json::Value::String("val".into())]],
            row_count: 1,
        };
        let mut buf: Vec<u8> = Vec::new();
        render_result(&result, &mut buf, OutputFormat::Vertical).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("-[ RECORD 1 ]-"));
        assert!(output.contains("col | val"));
        assert!(output.contains("(1 row)"));
    }

    #[test]
    fn test_output_format_from_str() {
        assert!(matches!("table".parse::<OutputFormat>().unwrap(), OutputFormat::Table));
        assert!(matches!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json));
        assert!(matches!("vertical".parse::<OutputFormat>().unwrap(), OutputFormat::Vertical));
        assert!(matches!("csv".parse::<OutputFormat>().unwrap(), OutputFormat::Csv));
        assert!("invalid".parse::<OutputFormat>().is_err());
    }
}

use async_trait::async_trait;
use serde_json::Value;

use crate::backend::error::DbError;
use crate::backend::{DbConn, Dialect, QueryResult};

use super::dialect::OracleDialect;
use super::types;

static ORACLE_DIALECT: OracleDialect = OracleDialect;

pub(crate) struct OracleConn {
    conn: oracle_rs::Connection,
}

impl OracleConn {
    pub(crate) fn new(conn: oracle_rs::Connection) -> Self {
        Self { conn }
    }
}

fn oracle_result_to_query_result(result: oracle_rs::connection::QueryResult) -> QueryResult {
    if result.rows.is_empty() {
        return QueryResult::empty();
    }

    let columns: Vec<String> = result.columns.iter().map(|c| c.name.clone()).collect();
    let col_count = columns.len();
    let mut rows: Vec<Vec<Value>> = Vec::with_capacity(result.rows.len());
    for row in &result.rows {
        rows.push(types::format_oracle_row(row, col_count));
    }

    QueryResult {
        columns,
        rows,
        row_count: result.rows.len(),
    }
}

fn convert_params(params: &[Value]) -> Vec<oracle_rs::Value> {
    params
        .iter()
        .map(|v| match v {
            Value::String(s) => oracle_rs::Value::String(s.clone()),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    i.into()
                } else if let Some(f) = n.as_f64() {
                    f.into()
                } else {
                    oracle_rs::Value::Null
                }
            }
            Value::Bool(b) => {
                // Oracle bool via string representation for compatibility
                oracle_rs::Value::String(if *b { "1".to_string() } else { "0".to_string() })
            }
            Value::Null => oracle_rs::Value::Null,
            _ => oracle_rs::Value::String(v.to_string()),
        })
        .collect()
}

#[async_trait]
impl DbConn for OracleConn {
    async fn query(&mut self, sql: &str) -> Result<QueryResult, DbError> {
        let result = self
            .conn
            .query(sql, &[])
            .await
            .map_err(|e| DbError::query_with_source("Oracle query failed", e))?;
        Ok(oracle_result_to_query_result(result))
    }

    async fn exec(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult, DbError> {
        let oracle_params = convert_params(params);
        let result = self
            .conn
            .query(sql, &oracle_params)
            .await
            .map_err(|e| DbError::query_with_source("Oracle exec failed", e))?;
        Ok(oracle_result_to_query_result(result))
    }

    async fn query_drop(&mut self, sql: &str) -> Result<(), DbError> {
        self.conn
            .execute(sql, &[])
            .await
            .map_err(|e| DbError::query_with_source("Oracle query_drop failed", e))?;
        Ok(())
    }

    fn dialect(&self) -> &dyn Dialect {
        &ORACLE_DIALECT
    }
}

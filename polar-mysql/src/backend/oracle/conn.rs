use async_trait::async_trait;
use serde_json::Value;

use crate::backend::error::DbError;
use crate::backend::{DbConn, Dialect, QueryResult};

use super::dialect::OracleDialect;

static ORACLE_DIALECT: OracleDialect = OracleDialect;

pub(crate) struct OracleConn {
    conn: oracle::Connection,
}

impl OracleConn {
    pub(crate) fn new(conn: oracle::Connection) -> Self {
        Self { conn }
    }
}

fn result_set_to_query_result(
    result: oracle::ResultSet<oracle::Row>,
) -> Result<QueryResult, DbError> {
    let col_info = result.column_info().to_vec();
    let columns: Vec<String> = col_info.iter().map(|c| c.name().to_string()).collect();
    let col_count = columns.len();

    let mut rows: Vec<Vec<Value>> = Vec::new();
    for row_result in result {
        let row = row_result
            .map_err(|e| DbError::query_with_source("Oracle row fetch failed", e))?;
        let mut values = Vec::with_capacity(col_count);
        for i in 0..col_count {
            values.push(oracle_value_to_json(&row, i));
        }
        rows.push(values);
    }

    let row_count = rows.len();
    Ok(QueryResult {
        columns,
        rows,
        row_count,
    })
}

fn oracle_value_to_json(row: &oracle::Row, idx: usize) -> Value {
    // String first (most common for introspection)
    if let Ok(v) = row.get::<_, String>(idx) {
        if let Ok(json_val) = serde_json::from_str(&v) {
            return json_val;
        }
        return Value::String(v);
    }
    if let Ok(v) = row.get::<_, i64>(idx) {
        return Value::Number(serde_json::Number::from(v));
    }
    if let Ok(v) = row.get::<_, f64>(idx) {
        if let Some(n) = serde_json::Number::from_f64(v) {
            return Value::Number(n);
        }
    }
    Value::Null
}

#[async_trait]
impl DbConn for OracleConn {
    async fn query(&mut self, sql: &str) -> Result<QueryResult, DbError> {
        let result = self
            .conn
            .query(sql, &[] as &[&dyn oracle::sql_type::ToSql])
            .map_err(|e| DbError::query_with_source("Oracle query failed", e))?;
        result_set_to_query_result(result)
    }

    async fn exec(&mut self, sql: &str, params: &[Value]) -> Result<QueryResult, DbError> {
        let string_params: Vec<String> = params
            .iter()
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect();
        let param_refs: Vec<&dyn oracle::sql_type::ToSql> = string_params
            .iter()
            .map(|s| s as &dyn oracle::sql_type::ToSql)
            .collect();

        let result = self
            .conn
            .query(sql, &param_refs)
            .map_err(|e| DbError::query_with_source("Oracle exec failed", e))?;
        result_set_to_query_result(result)
    }

    async fn query_drop(&mut self, sql: &str) -> Result<(), DbError> {
        self.conn
            .execute(sql, &[] as &[&dyn oracle::sql_type::ToSql])
            .map(|_| ())
            .map_err(|e| DbError::query_with_source("Oracle query_drop failed", e))
    }

    fn dialect(&self) -> &dyn Dialect {
        &ORACLE_DIALECT
    }
}

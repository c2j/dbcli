use async_trait::async_trait;
use std::sync::Arc;

use crate::backend::error::DbError;
use crate::backend::{DbConn, DbPool};

use super::conn::OracleConn;

pub(crate) struct OraclePool {
    user: String,
    password: String,
    conn_str: String,
}

impl OraclePool {
    pub(crate) fn new(user: String, password: String, conn_str: String) -> Self {
        Self {
            user,
            password,
            conn_str,
        }
    }
}

#[async_trait]
impl DbPool for OraclePool {
    async fn acquire(&self) -> Result<Box<dyn DbConn + Send>, DbError> {
        let conn = oracle::Connection::connect(
            &self.user,
            &self.password,
            &self.conn_str,
        )
        .map_err(|e| DbError::connection_with_source("Oracle connection failed", e))?;
        Ok(Box::new(OracleConn::new(conn)))
    }
}

pub(crate) fn create_oracle_pool(
    url: &str,
    _timeout_ms: Option<u64>,
) -> Result<Arc<dyn DbPool>, DbError> {
    let (conn_str, user, password) = parse_oracle_url(url)?;
    Ok(Arc::new(OraclePool::new(user, password, conn_str)))
}

fn parse_oracle_url(url: &str) -> Result<(String, String, String), DbError> {
    let without_scheme = url
        .strip_prefix("oracle://")
        .ok_or_else(|| DbError::config("Oracle URL must start with oracle://"))?;

    let (credentials, host_port_service) = without_scheme
        .split_once('@')
        .ok_or_else(|| DbError::config("Oracle URL missing @ separator"))?;

    let (user, password) = credentials
        .split_once(':')
        .ok_or_else(|| DbError::config("Oracle URL missing password"))?;

    let (host_port, service_name) = host_port_service
        .split_once('/')
        .ok_or_else(|| DbError::config("Oracle URL missing service name"))?;

    let conn_str = format!("//{}/{}", host_port, service_name);

    Ok((conn_str, user.to_string(), password.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_oracle_url() {
        assert!(parse_oracle_url("oracle://scott:tiger@localhost:1521/ORCL").is_ok());
    }

    #[test]
    fn test_parse_oracle_url_default_port() {
        assert!(parse_oracle_url("oracle://scott:tiger@localhost/FREEPDB1").is_ok());
    }

    #[test]
    fn test_parse_missing_scheme() {
        assert!(parse_oracle_url("mysql://user:pass@host/db").is_err());
    }
}

#[cfg(all(feature = "oracle", feature = "integration", test))]
mod integration {
    use super::*;
    use crate::backend::DbPool;

    fn oracle_test_url() -> Option<String> {
        std::env::var("POLARDB_ORACLE_TEST_URL").ok()
    }

    #[tokio::test]
    async fn test_connect_and_query() {
        let url = match oracle_test_url() {
            Some(u) => u,
            None => return,
        };
        let pool = create_oracle_pool(&url, None).expect("create pool");
        let mut conn = pool.acquire().await.expect("acquire");
        let result = conn.query("SELECT 'ok' AS status FROM dual").await.expect("query");
        assert_eq!(result.row_count, 1);
    }

    #[tokio::test]
    async fn test_add_limit() {
        let url = match oracle_test_url() {
            Some(u) => u,
            None => return,
        };
        let pool = create_oracle_pool(&url, None).expect("create pool");
        let mut conn = pool.acquire().await.expect("acquire");
        let sql = conn.dialect().add_limit("SELECT * FROM all_tables", 5);
        let result = conn.query(&sql).await.expect("query");
        assert!(result.row_count <= 5);
    }
}

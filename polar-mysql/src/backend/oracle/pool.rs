use async_trait::async_trait;
use std::sync::Arc;

use crate::backend::error::DbError;
use crate::backend::{DbConn, DbPool};

use super::conn::OracleConn;

pub(crate) struct OraclePool {
    config: oracle_rs::Config,
}

impl OraclePool {
    pub(crate) fn new(config: oracle_rs::Config) -> Self {
        Self { config }
    }
}

#[async_trait]
impl DbPool for OraclePool {
    async fn acquire(&self) -> Result<Box<dyn DbConn + Send>, DbError> {
        let conn = oracle_rs::Connection::connect_with_config(self.config.clone())
            .await
            .map_err(|e| DbError::connection_with_source("Oracle connection failed", e))?;
        Ok(Box::new(OracleConn::new(conn)))
    }
}

pub(crate) fn create_oracle_pool(
    url: &str,
    _timeout_ms: Option<u64>,
) -> Result<Arc<dyn DbPool>, DbError> {
    let config = parse_oracle_url(url)?;
    Ok(Arc::new(OraclePool::new(config)))
}

fn parse_oracle_url(url: &str) -> Result<oracle_rs::Config, DbError> {
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

    let (host, port) = if let Some((h, p)) = host_port.split_once(':') {
        (h, p.parse::<u16>().unwrap_or(1521))
    } else {
        (host_port, 1521u16)
    };

    Ok(oracle_rs::Config::new(host, port, service_name, user, password))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_oracle_url_default_port() {
        assert!(parse_oracle_url("oracle://scott:tiger@localhost/FREEPDB1").is_ok());
    }

    #[test]
    fn test_parse_oracle_url_with_port() {
        assert!(parse_oracle_url("oracle://scott:tiger@dbhost:1521/ORCL").is_ok());
    }

    #[test]
    fn test_parse_oracle_url_special_password() {
        assert!(parse_oracle_url("oracle://admin:p%40ssw0rd@host:1521/XE").is_ok());
    }

    #[test]
    fn test_parse_oracle_url_missing_scheme() {
        assert!(parse_oracle_url("mysql://user:pass@host/db").is_err());
    }

    #[test]
    fn test_parse_oracle_url_no_service() {
        assert!(parse_oracle_url("oracle://user:pass@host").is_err());
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
    async fn test_oracle_connect_and_query_dual() {
        let url = match oracle_test_url() {
            Some(u) => u,
            None => {
                eprintln!("SKIP: POLARDB_ORACLE_TEST_URL not set");
                return;
            }
        };

        let pool = create_oracle_pool(&url, None).expect("should create pool");
        let mut conn = pool.acquire().await.expect("should acquire connection");

        let result = conn
            .query("SELECT 'hello' AS greeting, 42 AS answer FROM dual")
            .await
            .expect("should query dual");

        assert_eq!(result.columns, vec!["GREETING", "ANSWER"]);
        assert_eq!(result.row_count, 1);
        assert_eq!(result.rows[0][0].as_str().unwrap(), "hello");
        assert_eq!(result.rows[0][1].as_i64().unwrap(), 42);
    }

    #[tokio::test]
    async fn test_oracle_database_info() {
        let url = match oracle_test_url() {
            Some(u) => u,
            None => return,
        };

        let pool = create_oracle_pool(&url, None).expect("should create pool");
        let mut conn = pool.acquire().await.expect("should acquire connection");
        let sql = conn.dialect().database_info().to_string();

        let result = conn.query(&sql).await.expect("should get database info");
        assert!(!result.rows.is_empty());
        assert_eq!(result.columns.len(), 9);
    }

    #[tokio::test]
    async fn test_oracle_add_limit() {
        let url = match oracle_test_url() {
            Some(u) => u,
            None => return,
        };

        let pool = create_oracle_pool(&url, None).expect("should create pool");
        let mut conn = pool.acquire().await.expect("should acquire connection");
        let sql = conn.dialect().add_limit("SELECT * FROM all_tables", 5);

        let result = conn.query(&sql).await.expect("should query with limit");
        assert!(result.row_count <= 5);
    }
}

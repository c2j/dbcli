pub(crate) mod conn;
pub(crate) mod dialect;
pub(crate) mod pool;
pub(crate) mod types;

use async_trait::async_trait;
use std::sync::Arc;

use crate::backend::error::DbError;
use crate::backend::{BackendFactory, DbPool, Dialect};
use crate::config::TimeoutConfig;

use self::dialect::MySqlDialect;

pub(crate) struct MySqlFactory;

#[async_trait]
impl BackendFactory for MySqlFactory {
    fn name(&self) -> &str {
        "MySQL"
    }

    fn scheme(&self) -> &str {
        "mysql"
    }

    fn create_dialect(&self) -> Box<dyn Dialect> {
        Box::new(MySqlDialect)
    }

    async fn connect(
        &self,
        url: &str,
        timeout_config: Option<&TimeoutConfig>,
    ) -> Result<Arc<dyn DbPool>, DbError> {
        let timeout_ms = timeout_config
            .and_then(|tc| tc.statement_timeout)
            .map(|d| d.as_millis() as u64);

        let pool = pool::create_mysql_pool(url, timeout_ms)?;

        {
            let mut conn = pool.acquire().await.map_err(|e| {
                DbError::connection(format!("MySQL connection probe failed: {}", e))
            })?;
            let _ = conn.query("SELECT 1").await.map_err(|e| {
                DbError::connection(format!("MySQL connection probe query failed: {}", e))
            })?;
        }

        Ok(pool)
    }
}

use std::sync::Arc;

use async_trait::async_trait;
use gaussdb::NoTls;

use crate::backend::error::DbError;
use crate::backend::{DbConn, DbPool};

use super::conn::GaussdbConn;
use super::GaussdbDialect;

/// Single-connection pool — wraps one Arc<gaussdb::Client> with
/// multiplexed queries over one TCP connection. acquire() returns a
/// new wrapper pointing at the same underlying client. If the TCP
/// connection dies, the pool must be recreated (no auto-reconnect).
pub(crate) struct GaussdbPool {
    client: Arc<gaussdb::Client>,
}

pub(crate) async fn create_gaussdb_pool(url: &str) -> Result<GaussdbPool, DbError> {
    let conn_str = normalize_gaussdb_url(url);
    let (client, connection) = gaussdb::connect(&conn_str, NoTls).await.map_err(|e| {
        DbError::connection(format!(
            "GaussDB connect failed: {} (target: {})",
            e,
            redact_password(&conn_str)
        ))
    })?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .simple_query("SET default_transaction_read_only = ON")
        .await;
    Ok(GaussdbPool {
        client: Arc::new(client),
    })
}

/// Convert gaussdb:// URL to postgres:// so tokio-postgres's
/// built-in config parser handles host, port, sslmode, and
/// percent-decoded credentials correctly.
fn normalize_gaussdb_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("gaussdb://") {
        format!("postgres://{}", rest)
    } else {
        url.to_string()
    }
}

fn redact_password(conn_str: &str) -> String {
    if let Some(at) = conn_str.find('@') {
        if let Some(colon) = conn_str[..at].rfind(':') {
            return format!("{}:****@{}", &conn_str[..colon], &conn_str[at + 1..]);
        }
    }
    conn_str.to_string()
}

#[async_trait]
impl DbPool for GaussdbPool {
    async fn acquire(&self) -> Result<Box<dyn DbConn + Send>, DbError> {
        Ok(Box::new(GaussdbConn {
            client: Arc::clone(&self.client),
            dialect: GaussdbDialect,
        }))
    }
}

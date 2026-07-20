use std::sync::Arc;

use mysql_async::prelude::*;
use mysql_async::OptsBuilder;
use tracing::{debug, info};

pub(crate) async fn do_connect(
    url: &str,
    timeout_config: Option<&crate::config::TimeoutConfig>,
) -> Result<
    (Arc<mysql_async::Pool>, mysql_async::Conn),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let opts = OptsBuilder::from_opts(mysql_async::Opts::from_url(url)?);

    let pool = mysql_async::Pool::new(opts);
    let mut conn = pool.get_conn().await.map_err(|e| {
        format!("Connection failed: {}", e)
    })?;

    if let Some(tc) = timeout_config {
        if let Some(st) = tc.statement_timeout {
            let ms = st.as_millis() as u64;
            // MySQL uses max_execution_time (milliseconds) for SELECT statements
            // For compatibility with PolarDB-X which uses max_execution_time
            let set_sql = format!("SET max_execution_time = {}", ms);
            if let Err(e) = conn.query_drop(&set_sql).await {
                tracing::warn!(
                    "failed to apply max_execution_time={}ms for new connection: {}",
                    ms,
                    e
                );
            } else {
                info!("applied max_execution_time={}ms to new connection", ms);
            }
        }
    }

    Ok((Arc::new(pool), conn))
}

/// Apply timeout_action behaviour on a connection after a timeout.
/// "cancel" (default): no-op, connection stays alive.
/// "disconnect": close the current connection so the pool recycles it.
pub(crate) async fn apply_timeout_action(
    conn: &mut mysql_async::Conn,
    action: Option<&str>,
) {
    match action {
        Some("disconnect") => {
            info!("timeout_action=disconnect: closing connection for pool recycling");
            let _ = conn.query_drop("KILL CONNECTION CONNECTION_ID()").await;
        }
        _ => {
            // "cancel" or unspecified — connection stays alive
            debug!("timeout_action=cancel (or default): keeping connection alive");
        }
    }
}

#[allow(dead_code)]
pub(crate) struct ManagedConnection {
    pub pool: Arc<mysql_async::Pool>,
    pub conn: mysql_async::Conn,
}

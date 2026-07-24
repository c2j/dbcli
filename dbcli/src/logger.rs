use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::EnvFilter;

pub(crate) fn log_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("hepta-dbcli")
}

pub(crate) fn init_logging() {
    let dir = log_dir();

    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("warning: cannot create log dir {}: {}", dir.display(), e);
    }

    let file_appender = tracing_appender::rolling::daily(&dir, "polar-mysql.log");
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("polar_mysql=info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(file_appender)
        .with_ansi(false)
        .with_target(false)
        .init();

    info!("log file: {}/polar-mysql.log", dir.display());
}

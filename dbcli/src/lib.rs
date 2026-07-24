#![allow(dead_code)]

pub(crate) mod backend;
pub(crate) mod config;

#[cfg(feature = "integration")]
pub mod test_helpers {
    pub use crate::backend::mysql::MySqlFactory;
    pub use crate::config::TimeoutConfig;

    #[cfg(feature = "oracle-rs")]
    pub use crate::backend::oracle::OracleFactory;

    #[cfg(feature = "gaussdb")]
    pub use crate::backend::gaussdb::GaussdbFactory;
}

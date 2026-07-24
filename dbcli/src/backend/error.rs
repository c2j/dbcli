// ─── Unified Database Error Type ────────────────────────────────────

use std::error::Error as StdError;
use std::fmt;

/// Generic database error that wraps backend-specific errors.
/// The `kind` field classifies the error for programmatic handling;
/// the `source` chain preserves the original error for debugging.
#[derive(Debug)]
pub struct DbError {
    pub kind: DbErrorKind,
    message: String,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbErrorKind {
    /// Connection failed (host unreachable, auth error, etc.)
    ConnectionFailed,
    /// Query execution failed (syntax error, permission denied, etc.)
    QueryFailed,
    /// Configuration error (invalid URL, missing driver, etc.)
    Config,
    /// Unsupported operation for this backend
    Unsupported,
    /// Timeout
    Timeout,
    /// Generic / unknown
    Other,
}

impl DbError {
    pub fn connection(msg: impl Into<String>) -> Self {
        Self {
            kind: DbErrorKind::ConnectionFailed,
            message: msg.into(),
            source: None,
        }
    }

    pub fn connection_with_source(
        msg: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: DbErrorKind::ConnectionFailed,
            message: msg.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn query(msg: impl Into<String>) -> Self {
        Self {
            kind: DbErrorKind::QueryFailed,
            message: msg.into(),
            source: None,
        }
    }

    pub fn query_with_source(
        msg: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: DbErrorKind::QueryFailed,
            message: msg.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self {
            kind: DbErrorKind::Config,
            message: msg.into(),
            source: None,
        }
    }

    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self {
            kind: DbErrorKind::Unsupported,
            message: msg.into(),
            source: None,
        }
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        // Walk the source chain to expose the underlying driver error
        let mut source = self.source();
        if source.is_some() {
            write!(f, ": ")?;
        }
        let mut first = true;
        while let Some(e) = source {
            if !first {
                write!(f, " | caused by: ")?;
            }
            first = false;
            write!(f, "{}", e)?;
            source = e.source();
        }
        Ok(())
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

// Allow converting from common error types

impl From<String> for DbError {
    fn from(s: String) -> Self {
        Self {
            kind: DbErrorKind::Other,
            message: s,
            source: None,
        }
    }
}

impl From<&str> for DbError {
    fn from(s: &str) -> Self {
        Self {
            kind: DbErrorKind::Other,
            message: s.to_string(),
            source: None,
        }
    }
}

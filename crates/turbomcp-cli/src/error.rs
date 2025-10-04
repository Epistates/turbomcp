//! Enhanced error types for CLI operations

use std::fmt;
use thiserror::Error;

/// CLI-specific errors with rich context
#[derive(Error, Debug)]
pub enum CliError {
    /// Transport layer errors
    #[error("Transport error: {0}")]
    Transport(#[from] turbomcp_core::Error),

    /// Invalid command arguments
    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    /// Server returned an error
    #[error("Server error [{code}]: {message}")]
    ServerError { code: i32, message: String },

    /// Operation timed out
    #[error("Operation '{operation}' timed out after {elapsed:?}")]
    Timeout {
        operation: String,
        elapsed: std::time::Duration,
    },

    /// Client not initialized
    #[error("Client not initialized - call 'initialize' first")]
    NotInitialized,

    /// JSON parsing error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// YAML parsing error
    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Configuration error
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    /// Connection failed
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Feature not supported
    #[error("Feature not supported: {0}")]
    NotSupported(String),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

impl CliError {
    /// Get user-friendly suggestions for resolving the error
    pub fn suggestions(&self) -> Vec<&'static str> {
        match self {
            Self::ConnectionFailed(_) => vec![
                "Check if the server is running",
                "Verify the connection URL",
                "Use --transport to specify transport explicitly",
            ],
            Self::NotInitialized => vec![
                "Ensure the server is started before calling operations",
                "Check server logs for initialization errors",
            ],
            Self::Timeout { .. } => vec![
                "Increase timeout with --timeout flag",
                "Check server responsiveness",
                "Verify network connectivity",
            ],
            Self::InvalidArguments(_) => vec![
                "Check argument format (must be valid JSON)",
                "Use --help to see expected format",
            ],
            _ => vec![],
        }
    }

    /// Get the error category for colored output
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::Transport(_) | Self::ConnectionFailed(_) => ErrorCategory::Connection,
            Self::InvalidArguments(_) => ErrorCategory::User,
            Self::ServerError { .. } => ErrorCategory::Server,
            Self::Timeout { .. } => ErrorCategory::Timeout,
            Self::Json(_) | Self::Yaml(_) => ErrorCategory::Parsing,
            Self::Io(_) => ErrorCategory::System,
            Self::Config(_) => ErrorCategory::Config,
            Self::NotSupported(_) => ErrorCategory::NotSupported,
            _ => ErrorCategory::Other,
        }
    }
}

/// Error categories for colored output
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Connection,
    User,
    Server,
    Timeout,
    Parsing,
    System,
    Config,
    NotSupported,
    Other,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection => write!(f, "Connection"),
            Self::User => write!(f, "User Input"),
            Self::Server => write!(f, "Server"),
            Self::Timeout => write!(f, "Timeout"),
            Self::Parsing => write!(f, "Parsing"),
            Self::System => write!(f, "System"),
            Self::Config => write!(f, "Configuration"),
            Self::NotSupported => write!(f, "Not Supported"),
            Self::Other => write!(f, "Error"),
        }
    }
}

/// Helper for creating CliError from strings
impl From<String> for CliError {
    fn from(s: String) -> Self {
        Self::Other(s)
    }
}

impl From<&str> for CliError {
    fn from(s: &str) -> Self {
        Self::Other(s.to_string())
    }
}

impl From<Box<turbomcp_core::Error>> for CliError {
    fn from(err: Box<turbomcp_core::Error>) -> Self {
        Self::Transport(*err)
    }
}

/// Result type for CLI operations
pub type CliResult<T> = Result<T, CliError>;

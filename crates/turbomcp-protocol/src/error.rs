//! Comprehensive error handling with rich context preservation.
//!
//! This module provides a sophisticated error handling system that captures
//! detailed context about failures, supports error chaining, and integrates
//! with observability systems.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

#[cfg(feature = "fancy-errors")]
use miette::Diagnostic;

/// Result type alias for MCP operations
pub type Result<T> = std::result::Result<T, Box<Error>>;

/// Comprehensive error type with rich context information
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "fancy-errors", derive(Diagnostic))]
pub struct Error {
    /// Unique identifier for this error instance
    pub id: Uuid,

    /// Error classification
    pub kind: ErrorKind,

    /// Human-readable error message
    pub message: String,

    /// Additional contextual information
    pub context: ErrorContext,

    /// Optional source error that caused this error
    #[serde(skip)]
    pub source: Option<Box<Error>>,

    /// Stack trace information (when available)
    #[cfg(debug_assertions)]
    #[serde(skip)]
    pub backtrace: std::backtrace::Backtrace,
}

impl Clone for Error {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            kind: self.kind,
            message: self.message.clone(),
            context: self.context.clone(),
            source: self.source.clone(),
            #[cfg(debug_assertions)]
            backtrace: std::backtrace::Backtrace::capture(),
        }
    }
}

impl<'de> Deserialize<'de> for Error {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ErrorData {
            id: Uuid,
            kind: ErrorKind,
            message: String,
            context: ErrorContext,
        }

        let data = ErrorData::deserialize(deserializer)?;
        Ok(Self {
            id: data.id,
            kind: data.kind,
            message: data.message,
            context: data.context,
            source: None,
            #[cfg(debug_assertions)]
            backtrace: std::backtrace::Backtrace::capture(),
        })
    }
}

/// Error classification for programmatic handling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Input validation failed
    Validation,

    /// Authentication or authorization failed
    Authentication,

    /// Resource was not found
    NotFound,

    /// Operation is not permitted
    PermissionDenied,

    /// Request was malformed or invalid
    BadRequest,

    /// Server internal error
    Internal,

    /// Network or transport error
    Transport,

    /// Serialization/deserialization error
    Serialization,

    /// Protocol violation or incompatibility
    Protocol,

    /// Operation timed out
    Timeout,

    /// Resource is temporarily unavailable
    Unavailable,

    /// Rate limit exceeded
    RateLimited,

    /// Configuration error
    Configuration,

    /// External dependency failed
    ExternalService,

    /// Operation was cancelled
    Cancelled,

    /// Handler execution error
    Handler,

    /// Security violation or constraint failure
    Security,
}

/// Rich contextual information for errors
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorContext {
    /// Operation that was being performed
    pub operation: Option<String>,

    /// Component where error occurred
    pub component: Option<String>,

    /// Request ID for tracing
    pub request_id: Option<String>,

    /// User ID (if applicable)
    pub user_id: Option<String>,

    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,

    /// Timestamp when error occurred
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// Retry information
    pub retry_info: Option<RetryInfo>,
}

/// Information about retry attempts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryInfo {
    /// Number of attempts made
    pub attempts: u32,

    /// Maximum attempts allowed
    pub max_attempts: u32,

    /// Next retry delay in milliseconds
    pub retry_after_ms: Option<u64>,
}

impl Error {
    /// Create a new error with the specified kind and message
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Box<Self> {
        Box::new(Self {
            id: Uuid::new_v4(),
            kind,
            message: message.into(),
            context: ErrorContext {
                timestamp: chrono::Utc::now(),
                ..Default::default()
            },
            source: None,
            #[cfg(debug_assertions)]
            backtrace: std::backtrace::Backtrace::capture(),
        })
    }

    /// Create a validation error
    pub fn validation(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Validation, message)
    }

    /// Create an invalid parameters error (MCP -32602)
    ///
    /// This is the standard MCP error code for parameter validation failures,
    /// including missing required parameters, invalid types, out-of-range values,
    /// or any other parameter-related validation errors.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_protocol::Error;
    ///
    /// let error = Error::invalid_params("Email must be valid");
    /// assert_eq!(error.jsonrpc_error_code(), -32602);
    /// ```
    pub fn invalid_params(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Validation, message)
    }

    /// Create an authentication error
    pub fn authentication(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Authentication, message)
    }

    /// Create a not found error
    pub fn not_found(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::NotFound, message)
    }

    /// Create a permission denied error
    pub fn permission_denied(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::PermissionDenied, message)
    }

    /// Create a bad request error
    pub fn bad_request(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::BadRequest, message)
    }

    /// Create an internal error
    pub fn internal(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Internal, message)
    }

    /// Create a transport error
    pub fn transport(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Transport, message)
    }

    /// Create a serialization error
    pub fn serialization(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Serialization, message)
    }

    /// Create a protocol error
    pub fn protocol(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Protocol, message)
    }

    /// Create a JSON-RPC error
    #[must_use]
    pub fn rpc(code: i32, message: &str) -> Box<Self> {
        Self::new(ErrorKind::Protocol, format!("RPC error {code}: {message}"))
    }

    /// Create a timeout error
    pub fn timeout(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Timeout, message)
    }

    /// Create an unavailable error
    pub fn unavailable(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Unavailable, message)
    }

    /// Create a rate limited error
    pub fn rate_limited(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::RateLimited, message)
    }

    /// Create a configuration error
    pub fn configuration(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Configuration, message)
    }

    /// Create an external service error
    pub fn external_service(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::ExternalService, message)
    }

    /// Create a cancelled error
    pub fn cancelled(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Cancelled, message)
    }

    /// Create a handler error - for compatibility with macro-generated code
    pub fn handler(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Handler, message)
    }

    /// Create a security error
    pub fn security(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Security, message)
    }

    /// Add context to this error
    #[must_use]
    pub fn with_context(
        mut self: Box<Self>,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Box<Self> {
        self.context.metadata.insert(key.into(), value.into());
        self
    }

    /// Set the operation being performed
    #[must_use]
    pub fn with_operation(mut self: Box<Self>, operation: impl Into<String>) -> Box<Self> {
        self.context.operation = Some(operation.into());
        self
    }

    /// Set the component where error occurred
    #[must_use]
    pub fn with_component(mut self: Box<Self>, component: impl Into<String>) -> Box<Self> {
        self.context.component = Some(component.into());
        self
    }

    /// Set the request ID for tracing
    #[must_use]
    pub fn with_request_id(mut self: Box<Self>, request_id: impl Into<String>) -> Box<Self> {
        self.context.request_id = Some(request_id.into());
        self
    }

    /// Set the user ID
    #[must_use]
    pub fn with_user_id(mut self: Box<Self>, user_id: impl Into<String>) -> Box<Self> {
        self.context.user_id = Some(user_id.into());
        self
    }

    /// Add retry information
    #[must_use]
    pub fn with_retry_info(mut self: Box<Self>, retry_info: RetryInfo) -> Box<Self> {
        self.context.retry_info = Some(retry_info);
        self
    }

    /// Chain this error with a source error
    #[must_use]
    pub fn with_source(mut self: Box<Self>, source: Box<Self>) -> Box<Self> {
        self.source = Some(source);
        self
    }

    /// Check if this error is retryable based on its kind
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Timeout
                | ErrorKind::Unavailable
                | ErrorKind::Transport
                | ErrorKind::ExternalService
                | ErrorKind::RateLimited
        )
    }

    /// Check if this error indicates a temporary failure
    pub const fn is_temporary(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Timeout
                | ErrorKind::Unavailable
                | ErrorKind::RateLimited
                | ErrorKind::ExternalService
        )
    }

    /// Get the HTTP status code equivalent for this error
    pub const fn http_status_code(&self) -> u16 {
        match self.kind {
            ErrorKind::Validation | ErrorKind::BadRequest => 400,
            ErrorKind::Authentication => 401,
            ErrorKind::PermissionDenied => 403,
            ErrorKind::NotFound => 404,
            ErrorKind::Timeout => 408,
            ErrorKind::RateLimited => 429,
            ErrorKind::Internal
            | ErrorKind::Configuration
            | ErrorKind::Serialization
            | ErrorKind::Protocol
            | ErrorKind::Handler => 500,
            ErrorKind::Transport | ErrorKind::ExternalService | ErrorKind::Unavailable => 503,
            ErrorKind::Cancelled => 499, // Client closed request
            ErrorKind::Security => 403,  // Forbidden - security constraint violation
        }
    }

    /// Convert to a JSON-RPC error code
    pub const fn jsonrpc_error_code(&self) -> i32 {
        match self.kind {
            ErrorKind::BadRequest => -32600, // Invalid Request
            ErrorKind::Validation | ErrorKind::Serialization => -32602, // Invalid params
            ErrorKind::Protocol => -32601,   // Method not found
            ErrorKind::Internal => -32603,   // Internal error
            ErrorKind::NotFound => -32001,   // Custom: Not found
            ErrorKind::Authentication => -32002, // Custom: Authentication failed
            ErrorKind::PermissionDenied => -32003, // Custom: Permission denied
            ErrorKind::Timeout => -32004,    // Custom: Timeout
            ErrorKind::Unavailable => -32005, // Custom: Service unavailable
            ErrorKind::RateLimited => -32006, // Custom: Rate limited
            ErrorKind::Transport => -32007,  // Custom: Transport error
            ErrorKind::Configuration => -32008, // Custom: Configuration error
            ErrorKind::ExternalService => -32009, // Custom: External service error
            ErrorKind::Cancelled => -32010,  // Custom: Operation cancelled
            ErrorKind::Handler => -32011,    // Custom: Handler error
            ErrorKind::Security => -32012,   // Custom: Security constraint violation
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;

        if let Some(operation) = &self.context.operation {
            write!(f, " (operation: {operation})")?;
        }

        if let Some(component) = &self.context.component {
            write!(f, " (component: {component})")?;
        }

        if let Some(request_id) = &self.context.request_id {
            write!(f, " (request_id: {request_id})")?;
        }

        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // Note: We can't return the source error because it's also an Error type
        // which would create infinite recursion. Current design returns None to avoid this.
        // Future enhancement could add proper error chaining with boxed std::error::Error.
        None
    }
}

impl ErrorKind {
    /// Get a human-readable description of this error kind
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Validation => "Input validation failed",
            Self::Authentication => "Authentication failed",
            Self::NotFound => "Resource not found",
            Self::PermissionDenied => "Permission denied",
            Self::BadRequest => "Bad request",
            Self::Internal => "Internal server error",
            Self::Transport => "Transport error",
            Self::Serialization => "Serialization error",
            Self::Protocol => "Protocol error",
            Self::Timeout => "Operation timed out",
            Self::Unavailable => "Service unavailable",
            Self::RateLimited => "Rate limit exceeded",
            Self::Configuration => "Configuration error",
            Self::ExternalService => "External service error",
            Self::Cancelled => "Operation cancelled",
            Self::Handler => "Handler execution error",
            Self::Security => "Security constraint violation",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

/// Convenience macro for creating errors with context
#[macro_export]
macro_rules! mcp_error {
    ($kind:expr, $message:expr) => {
        $crate::error::Error::new($kind, $message)
    };
    ($kind:expr, $message:expr, $($key:expr => $value:expr),+) => {
        {
            let mut error = $crate::error::Error::new($kind, $message);
            $(
                error = error.with_context($key, $value);
            )+
            error
        }
    };
}

/// Extension trait for adding MCP error context to other error types
pub trait ErrorExt<T> {
    /// Convert any error to an MCP error with the specified kind
    ///
    /// # Errors
    ///
    /// Returns an `Error` with the specified kind and message, preserving the source error context.
    fn with_mcp_error(self, kind: ErrorKind, message: impl Into<String>) -> Result<T>;

    /// Convert any error to an MCP internal error
    ///
    /// # Errors
    ///
    /// Returns an `Error` with internal error kind and the provided message.
    fn with_internal_error(self, message: impl Into<String>) -> Result<T>;
}

impl<T, E> ErrorExt<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn with_mcp_error(self, kind: ErrorKind, message: impl Into<String>) -> Result<T> {
        self.map_err(|e| {
            Error::new(kind, format!("{}: {}", message.into(), e))
                .with_context("source_error", e.to_string())
        })
    }

    fn with_internal_error(self, message: impl Into<String>) -> Result<T> {
        self.with_mcp_error(ErrorKind::Internal, message)
    }
}

// Implement From for common error types
impl From<serde_json::Error> for Box<Error> {
    fn from(err: serde_json::Error) -> Self {
        Error::serialization(format!("JSON serialization error: {err}"))
    }
}

impl From<std::io::Error> for Box<Error> {
    fn from(err: std::io::Error) -> Self {
        Error::transport(format!("IO error: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = Error::validation("Invalid input");
        assert_eq!(error.kind, ErrorKind::Validation);
        assert_eq!(error.message, "Invalid input");
    }

    #[test]
    fn test_error_context() {
        let error = Error::internal("Something went wrong")
            .with_operation("test_operation")
            .with_component("test_component")
            .with_request_id("req-123")
            .with_context("key", "value");

        assert_eq!(error.context.operation, Some("test_operation".to_string()));
        assert_eq!(error.context.component, Some("test_component".to_string()));
        assert_eq!(error.context.request_id, Some("req-123".to_string()));
        assert_eq!(
            error.context.metadata.get("key"),
            Some(&serde_json::Value::String("value".to_string()))
        );
    }

    #[test]
    fn test_error_properties() {
        let retryable_error = Error::timeout("Request timed out");
        assert!(retryable_error.is_retryable());
        assert!(retryable_error.is_temporary());

        let permanent_error = Error::validation("Invalid data");
        assert!(!permanent_error.is_retryable());
        assert!(!permanent_error.is_temporary());
    }

    #[test]
    fn test_http_status_codes() {
        assert_eq!(Error::validation("test").http_status_code(), 400);
        assert_eq!(Error::not_found("test").http_status_code(), 404);
        assert_eq!(Error::internal("test").http_status_code(), 500);
    }

    #[test]
    fn test_error_macro() {
        let error = mcp_error!(ErrorKind::Validation, "test message");
        assert_eq!(error.kind, ErrorKind::Validation);
        assert_eq!(error.message, "test message");

        let error_with_context = mcp_error!(
            ErrorKind::Internal,
            "test message",
            "key1" => "value1",
            "key2" => 42
        );
        assert_eq!(error_with_context.context.metadata.len(), 2);
    }
}

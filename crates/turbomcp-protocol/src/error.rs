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
    // ============================================================================
    // MCP-Specific Errors (MCP 2025-06-18 specification)
    // ============================================================================
    /// Tool not found (MCP error code -32001)
    ToolNotFound,

    /// Tool execution failed (MCP error code -32002)
    ToolExecutionFailed,

    /// Prompt not found (MCP error code -32003)
    PromptNotFound,

    /// Resource not found (MCP error code -32004)
    ResourceNotFound,

    /// Resource access denied (MCP error code -32005)
    ResourceAccessDenied,

    /// Capability not supported (MCP error code -32006)
    CapabilityNotSupported,

    /// Protocol version mismatch (MCP error code -32007)
    ProtocolVersionMismatch,

    /// User rejected the request (MCP error code -1)
    ///
    /// Per MCP 2025-06-18 specification, this indicates a user explicitly
    /// rejected a sampling request or similar operation. This is a permanent
    /// failure that should not be retried.
    UserRejected,

    // ============================================================================
    // JSON-RPC Standard Errors
    // ============================================================================
    /// Input validation failed (JSON-RPC -32602)
    Validation,

    /// Request was malformed or invalid (JSON-RPC -32600)
    BadRequest,

    /// Server internal error (JSON-RPC -32603)
    Internal,

    /// Serialization/deserialization error (JSON-RPC -32602)
    Serialization,

    /// Protocol violation or incompatibility (JSON-RPC -32601)
    Protocol,

    // ============================================================================
    // General Application Errors
    // ============================================================================
    /// Authentication or authorization failed
    Authentication,

    /// Operation is not permitted
    PermissionDenied,

    /// Network or transport error
    Transport,

    /// Operation timed out
    Timeout,

    /// Resource is temporarily unavailable
    Unavailable,

    /// Rate limit exceeded (MCP error code -32009)
    RateLimited,

    /// Server overloaded (MCP error code -32010)
    ServerOverloaded,

    /// Configuration error
    Configuration,

    /// External dependency failed
    ExternalService,

    /// Operation was cancelled
    Cancelled,

    /// Security violation or constraint failure
    Security,

    // ============================================================================
    // Deprecated
    // ============================================================================
    /// Generic handler execution error (deprecated - use specific error kinds)
    ///
    /// Replaced by:
    /// - `ToolExecutionFailed` for tool errors
    /// - `PromptNotFound` for prompt errors
    /// - `ResourceNotFound` or `ResourceAccessDenied` for resource errors
    #[deprecated(
        since = "2.1.0",
        note = "Use specific error kinds: ToolExecutionFailed, PromptNotFound, ResourceNotFound, etc."
    )]
    Handler,

    /// Generic not found error (deprecated - use specific error kinds)
    ///
    /// Replaced by:
    /// - `ToolNotFound` for tools
    /// - `PromptNotFound` for prompts
    /// - `ResourceNotFound` for resources
    #[deprecated(
        since = "2.1.0",
        note = "Use specific error kinds: ToolNotFound, PromptNotFound, ResourceNotFound"
    )]
    NotFound,
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
    #[deprecated(
        since = "2.1.0",
        note = "Use specific constructors: tool_not_found(), prompt_not_found(), or resource_not_found()"
    )]
    pub fn not_found(message: impl Into<String>) -> Box<Self> {
        #[allow(deprecated)]
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
    ///
    /// Maps JSON-RPC error codes to appropriate ErrorKind variants to preserve
    /// semantic meaning. Special handling for MCP-specific codes like -1 (user rejection).
    #[must_use]
    pub fn rpc(code: i32, message: &str) -> Box<Self> {
        // Map specific error codes to appropriate ErrorKind to preserve semantics
        let kind = match code {
            -1 => ErrorKind::UserRejected,            // MCP: User rejected request
            -32001 => ErrorKind::ToolNotFound,        // MCP: Tool not found
            -32002 => ErrorKind::ToolExecutionFailed, // MCP: Tool execution failed
            -32003 => ErrorKind::PromptNotFound,      // MCP: Prompt not found
            -32004 => ErrorKind::ResourceNotFound,    // MCP: Resource not found
            -32005 => ErrorKind::ResourceAccessDenied, // MCP: Resource access denied
            -32006 => ErrorKind::CapabilityNotSupported, // MCP: Capability not supported
            -32007 => ErrorKind::ProtocolVersionMismatch, // MCP: Protocol version mismatch
            -32008 => ErrorKind::Authentication,      // MCP: Authentication required
            -32009 => ErrorKind::RateLimited,         // MCP: Rate limited
            -32010 => ErrorKind::ServerOverloaded,    // MCP: Server overloaded
            -32600 => ErrorKind::BadRequest,          // JSON-RPC: Invalid Request
            -32601 => ErrorKind::Protocol,            // JSON-RPC: Method not found
            -32602 => ErrorKind::Validation,          // JSON-RPC: Invalid params
            -32603 => ErrorKind::Internal,            // JSON-RPC: Internal error
            _ => ErrorKind::Protocol,                 // Default to Protocol for unknown codes
        };

        Self::new(kind, format!("RPC error {code}: {message}"))
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

    /// Create a user rejected error
    ///
    /// Per MCP 2025-06-18 specification, this indicates a user explicitly
    /// rejected a request (e.g., declined a sampling request). This is a
    /// permanent failure that should not be retried.
    pub fn user_rejected(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::UserRejected, message)
    }

    /// Create a handler error - for compatibility with macro-generated code
    #[deprecated(
        since = "2.1.0",
        note = "Use specific error constructors: tool_not_found(), tool_execution_failed(), etc."
    )]
    pub fn handler(message: impl Into<String>) -> Box<Self> {
        #[allow(deprecated)]
        Self::new(ErrorKind::Handler, message)
    }

    /// Create a security error
    pub fn security(message: impl Into<String>) -> Box<Self> {
        Self::new(ErrorKind::Security, message)
    }

    // ============================================================================
    // MCP-Specific Error Constructors (MCP 2025-06-18)
    // ============================================================================

    /// Create a tool not found error (MCP error code -32001)
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_protocol::Error;
    ///
    /// let error = Error::tool_not_found("calculate");
    /// assert_eq!(error.jsonrpc_error_code(), -32001);
    /// ```
    pub fn tool_not_found(tool_name: impl Into<String>) -> Box<Self> {
        Self::new(
            ErrorKind::ToolNotFound,
            format!("Tool not found: {}", tool_name.into()),
        )
        .with_operation("tool_lookup")
        .with_component("tool_registry")
    }

    /// Create a tool execution failed error (MCP error code -32002)
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_protocol::Error;
    ///
    /// let error = Error::tool_execution_failed("calculate", "Division by zero");
    /// assert_eq!(error.jsonrpc_error_code(), -32002);
    /// ```
    pub fn tool_execution_failed(
        tool_name: impl Into<String>,
        reason: impl Into<String>,
    ) -> Box<Self> {
        Self::new(
            ErrorKind::ToolExecutionFailed,
            format!("Tool '{}' failed: {}", tool_name.into(), reason.into()),
        )
        .with_operation("tool_execution")
    }

    /// Create a prompt not found error (MCP error code -32003)
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_protocol::Error;
    ///
    /// let error = Error::prompt_not_found("code_review");
    /// assert_eq!(error.jsonrpc_error_code(), -32003);
    /// ```
    pub fn prompt_not_found(prompt_name: impl Into<String>) -> Box<Self> {
        Self::new(
            ErrorKind::PromptNotFound,
            format!("Prompt not found: {}", prompt_name.into()),
        )
        .with_operation("prompt_lookup")
        .with_component("prompt_registry")
    }

    /// Create a resource not found error (MCP error code -32004)
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_protocol::Error;
    ///
    /// let error = Error::resource_not_found("file:///docs/api.md");
    /// assert_eq!(error.jsonrpc_error_code(), -32004);
    /// ```
    pub fn resource_not_found(uri: impl Into<String>) -> Box<Self> {
        Self::new(
            ErrorKind::ResourceNotFound,
            format!("Resource not found: {}", uri.into()),
        )
        .with_operation("resource_lookup")
        .with_component("resource_provider")
    }

    /// Create a resource access denied error (MCP error code -32005)
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_protocol::Error;
    ///
    /// let error = Error::resource_access_denied("file:///etc/passwd", "Path outside allowed directory");
    /// assert_eq!(error.jsonrpc_error_code(), -32005);
    /// ```
    pub fn resource_access_denied(uri: impl Into<String>, reason: impl Into<String>) -> Box<Self> {
        Self::new(
            ErrorKind::ResourceAccessDenied,
            format!(
                "Access denied to resource '{}': {}",
                uri.into(),
                reason.into()
            ),
        )
        .with_operation("resource_access")
        .with_component("resource_security")
    }

    /// Create a capability not supported error (MCP error code -32006)
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_protocol::Error;
    ///
    /// let error = Error::capability_not_supported("sampling");
    /// assert_eq!(error.jsonrpc_error_code(), -32006);
    /// ```
    pub fn capability_not_supported(capability: impl Into<String>) -> Box<Self> {
        Self::new(
            ErrorKind::CapabilityNotSupported,
            format!("Capability not supported: {}", capability.into()),
        )
        .with_operation("capability_check")
    }

    /// Create a protocol version mismatch error (MCP error code -32007)
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_protocol::Error;
    ///
    /// let error = Error::protocol_version_mismatch("2024-11-05", "2025-06-18");
    /// assert_eq!(error.jsonrpc_error_code(), -32007);
    /// ```
    pub fn protocol_version_mismatch(
        client_version: impl Into<String>,
        server_version: impl Into<String>,
    ) -> Box<Self> {
        Self::new(
            ErrorKind::ProtocolVersionMismatch,
            format!(
                "Protocol version mismatch: client={}, server={}",
                client_version.into(),
                server_version.into()
            ),
        )
        .with_operation("version_negotiation")
    }

    /// Create a server overloaded error (MCP error code -32010)
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_protocol::Error;
    ///
    /// let error = Error::server_overloaded();
    /// assert_eq!(error.jsonrpc_error_code(), -32010);
    /// ```
    pub fn server_overloaded() -> Box<Self> {
        Self::new(
            ErrorKind::ServerOverloaded,
            "Server is currently overloaded",
        )
        .with_operation("request_processing")
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
            // Client errors (4xx)
            ErrorKind::Validation | ErrorKind::BadRequest | ErrorKind::UserRejected => 400,
            ErrorKind::Authentication => 401,
            ErrorKind::PermissionDenied | ErrorKind::Security | ErrorKind::ResourceAccessDenied => {
                403
            }
            ErrorKind::ToolNotFound | ErrorKind::PromptNotFound | ErrorKind::ResourceNotFound => {
                404
            }
            ErrorKind::Timeout => 408,
            ErrorKind::RateLimited => 429,
            ErrorKind::Cancelled => 499, // Client closed request

            // Server errors (5xx)
            ErrorKind::Internal
            | ErrorKind::Configuration
            | ErrorKind::Serialization
            | ErrorKind::Protocol
            | ErrorKind::ToolExecutionFailed
            | ErrorKind::CapabilityNotSupported
            | ErrorKind::ProtocolVersionMismatch => 500,

            ErrorKind::Transport
            | ErrorKind::ExternalService
            | ErrorKind::Unavailable
            | ErrorKind::ServerOverloaded => 503,

            // Deprecated (backwards compatibility)
            #[allow(deprecated)]
            ErrorKind::Handler => 500,
            #[allow(deprecated)]
            ErrorKind::NotFound => 404,
        }
    }

    /// Convert to a JSON-RPC error code per MCP 2025-06-18 specification
    pub const fn jsonrpc_error_code(&self) -> i32 {
        match self.kind {
            // JSON-RPC standard error codes
            ErrorKind::BadRequest => -32600, // Invalid Request
            ErrorKind::Protocol => -32601,   // Method not found
            ErrorKind::Validation | ErrorKind::Serialization => -32602, // Invalid params
            ErrorKind::Internal => -32603,   // Internal error

            // MCP-specific error codes (2025-06-18 specification)
            ErrorKind::UserRejected => -1, // User rejected request (sampling spec)
            ErrorKind::ToolNotFound => -32001, // Tool not found
            ErrorKind::ToolExecutionFailed => -32002, // Tool execution error
            ErrorKind::PromptNotFound => -32003, // Prompt not found
            ErrorKind::ResourceNotFound => -32004, // Resource not found
            ErrorKind::ResourceAccessDenied => -32005, // Resource access denied
            ErrorKind::CapabilityNotSupported => -32006, // Capability not supported
            ErrorKind::ProtocolVersionMismatch => -32007, // Protocol version mismatch
            ErrorKind::Authentication => -32008, // Authentication required
            ErrorKind::RateLimited => -32009, // Rate limited
            ErrorKind::ServerOverloaded => -32010, // Server overloaded

            // General application errors (application-defined codes)
            ErrorKind::PermissionDenied => -32011, // Permission denied
            ErrorKind::Timeout => -32012,          // Timeout
            ErrorKind::Unavailable => -32013,      // Service unavailable
            ErrorKind::Transport => -32014,        // Transport error
            ErrorKind::Configuration => -32015,    // Configuration error
            ErrorKind::ExternalService => -32016,  // External service error
            ErrorKind::Cancelled => -32017,        // Operation cancelled
            ErrorKind::Security => -32018,         // Security constraint violation

            // Deprecated (backwards compatibility)
            #[allow(deprecated)]
            ErrorKind::Handler => -32019, // Deprecated: Handler error
            #[allow(deprecated)]
            ErrorKind::NotFound => -32020, // Deprecated: Generic not found
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
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

impl ErrorKind {
    /// Get a human-readable description of this error kind
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            // MCP-specific errors
            Self::UserRejected => "User rejected request",
            Self::ToolNotFound => "Tool not found",
            Self::ToolExecutionFailed => "Tool execution failed",
            Self::PromptNotFound => "Prompt not found",
            Self::ResourceNotFound => "Resource not found",
            Self::ResourceAccessDenied => "Resource access denied",
            Self::CapabilityNotSupported => "Capability not supported",
            Self::ProtocolVersionMismatch => "Protocol version mismatch",

            // JSON-RPC standard errors
            Self::Validation => "Input validation failed",
            Self::BadRequest => "Bad request",
            Self::Internal => "Internal server error",
            Self::Serialization => "Serialization error",
            Self::Protocol => "Protocol error",

            // General application errors
            Self::Authentication => "Authentication failed",
            Self::PermissionDenied => "Permission denied",
            Self::Transport => "Transport error",
            Self::Timeout => "Operation timed out",
            Self::Unavailable => "Service unavailable",
            Self::RateLimited => "Rate limit exceeded",
            Self::ServerOverloaded => "Server overloaded",
            Self::Configuration => "Configuration error",
            Self::ExternalService => "External service error",
            Self::Cancelled => "Operation cancelled",
            Self::Security => "Security constraint violation",

            // Deprecated
            #[allow(deprecated)]
            Self::Handler => "Handler execution error (deprecated)",
            #[allow(deprecated)]
            Self::NotFound => "Resource not found (deprecated)",
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

// ============================================================================
// v3.0: Conversions with turbomcp-core::McpError
// ============================================================================

impl From<turbomcp_core::McpError> for Box<Error> {
    fn from(err: turbomcp_core::McpError) -> Self {
        let kind = match err.kind {
            // MCP-specific errors
            turbomcp_core::ErrorKind::ToolNotFound => ErrorKind::ToolNotFound,
            turbomcp_core::ErrorKind::ToolExecutionFailed => ErrorKind::ToolExecutionFailed,
            turbomcp_core::ErrorKind::PromptNotFound => ErrorKind::PromptNotFound,
            turbomcp_core::ErrorKind::ResourceNotFound => ErrorKind::ResourceNotFound,
            turbomcp_core::ErrorKind::ResourceAccessDenied => ErrorKind::ResourceAccessDenied,
            turbomcp_core::ErrorKind::CapabilityNotSupported => ErrorKind::CapabilityNotSupported,
            turbomcp_core::ErrorKind::ProtocolVersionMismatch => ErrorKind::ProtocolVersionMismatch,
            turbomcp_core::ErrorKind::UserRejected => ErrorKind::UserRejected,
            // JSON-RPC standard (map to protocol equivalents)
            turbomcp_core::ErrorKind::ParseError => ErrorKind::BadRequest, // -32700 -> BadRequest
            turbomcp_core::ErrorKind::InvalidRequest => ErrorKind::BadRequest, // -32600 -> BadRequest
            turbomcp_core::ErrorKind::MethodNotFound => ErrorKind::Protocol, // -32601 -> Protocol
            turbomcp_core::ErrorKind::InvalidParams => ErrorKind::Validation, // -32602 -> Validation
            turbomcp_core::ErrorKind::Internal => ErrorKind::Internal,
            // General application errors
            turbomcp_core::ErrorKind::Authentication => ErrorKind::Authentication,
            turbomcp_core::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
            turbomcp_core::ErrorKind::Transport => ErrorKind::Transport,
            turbomcp_core::ErrorKind::Timeout => ErrorKind::Timeout,
            turbomcp_core::ErrorKind::Unavailable => ErrorKind::Unavailable,
            turbomcp_core::ErrorKind::RateLimited => ErrorKind::RateLimited,
            turbomcp_core::ErrorKind::ServerOverloaded => ErrorKind::ServerOverloaded,
            turbomcp_core::ErrorKind::Configuration => ErrorKind::Configuration,
            turbomcp_core::ErrorKind::ExternalService => ErrorKind::ExternalService,
            turbomcp_core::ErrorKind::Cancelled => ErrorKind::Cancelled,
            turbomcp_core::ErrorKind::Security => ErrorKind::Security,
            turbomcp_core::ErrorKind::Serialization => ErrorKind::Serialization,
        };

        let mut error = Error::new(kind, err.message);

        // Transfer context
        if let Some(ctx) = err.context {
            if let Some(op) = ctx.operation {
                error = error.with_operation(op);
            }
            if let Some(comp) = ctx.component {
                error = error.with_component(comp);
            }
            if let Some(req_id) = ctx.request_id {
                error = error.with_request_id(req_id);
            }
        }

        // Transfer source location as metadata
        if let Some(loc) = err.source_location {
            error = error.with_context("source_location", loc);
        }

        error
    }
}

impl From<&Error> for turbomcp_core::McpError {
    fn from(err: &Error) -> Self {
        use turbomcp_core::ErrorKind as CoreKind;

        let kind = match err.kind {
            // MCP-specific errors
            ErrorKind::ToolNotFound => CoreKind::ToolNotFound,
            ErrorKind::ToolExecutionFailed => CoreKind::ToolExecutionFailed,
            ErrorKind::PromptNotFound => CoreKind::PromptNotFound,
            ErrorKind::ResourceNotFound => CoreKind::ResourceNotFound,
            ErrorKind::ResourceAccessDenied => CoreKind::ResourceAccessDenied,
            ErrorKind::CapabilityNotSupported => CoreKind::CapabilityNotSupported,
            ErrorKind::ProtocolVersionMismatch => CoreKind::ProtocolVersionMismatch,
            ErrorKind::UserRejected => CoreKind::UserRejected,
            // JSON-RPC standard (map from protocol equivalents)
            ErrorKind::Validation => CoreKind::InvalidParams,
            ErrorKind::BadRequest => CoreKind::InvalidRequest,
            ErrorKind::Protocol => CoreKind::MethodNotFound,
            ErrorKind::Internal => CoreKind::Internal,
            ErrorKind::Serialization => CoreKind::Serialization,
            // General application errors
            ErrorKind::Authentication => CoreKind::Authentication,
            ErrorKind::PermissionDenied => CoreKind::PermissionDenied,
            ErrorKind::Transport => CoreKind::Transport,
            ErrorKind::Timeout => CoreKind::Timeout,
            ErrorKind::Unavailable => CoreKind::Unavailable,
            ErrorKind::RateLimited => CoreKind::RateLimited,
            ErrorKind::ServerOverloaded => CoreKind::ServerOverloaded,
            ErrorKind::Configuration => CoreKind::Configuration,
            ErrorKind::ExternalService => CoreKind::ExternalService,
            ErrorKind::Cancelled => CoreKind::Cancelled,
            ErrorKind::Security => CoreKind::Security,
            // Deprecated variants map to closest match
            #[allow(deprecated)]
            ErrorKind::Handler => CoreKind::Internal,
            #[allow(deprecated)]
            ErrorKind::NotFound => CoreKind::ResourceNotFound,
        };

        let mut core_err = turbomcp_core::McpError::new(kind, err.message.clone());

        // Transfer context
        if let Some(op) = &err.context.operation {
            core_err = core_err.with_operation(op.clone());
        }
        if let Some(comp) = &err.context.component {
            core_err = core_err.with_component(comp.clone());
        }
        if let Some(req_id) = &err.context.request_id {
            core_err = core_err.with_request_id(req_id.clone());
        }

        core_err
    }
}

impl From<Box<Error>> for turbomcp_core::McpError {
    fn from(err: Box<Error>) -> Self {
        turbomcp_core::McpError::from(err.as_ref())
    }
}

impl From<Error> for turbomcp_core::McpError {
    fn from(err: Error) -> Self {
        turbomcp_core::McpError::from(&err)
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

    // v3.0: Test McpError conversions
    #[test]
    fn test_core_error_to_protocol_error() {
        let core_err = turbomcp_core::McpError::tool_not_found("calculator");
        let protocol_err: Box<Error> = core_err.into();

        assert_eq!(protocol_err.kind, ErrorKind::ToolNotFound);
        assert!(protocol_err.message.contains("calculator"));
        assert_eq!(protocol_err.context.operation, Some("tool_lookup".to_string()));
    }

    #[test]
    fn test_protocol_error_to_core_error() {
        let protocol_err = Error::tool_not_found("calculator");
        let core_err: turbomcp_core::McpError = protocol_err.into();

        assert_eq!(core_err.kind, turbomcp_core::ErrorKind::ToolNotFound);
        assert!(core_err.message.contains("calculator"));
    }

    #[test]
    fn test_error_roundtrip() {
        // Protocol -> Core -> Protocol should preserve key information
        let original = Error::internal("test error")
            .with_operation("test_op")
            .with_component("test_comp");

        let core_err: turbomcp_core::McpError = original.as_ref().into();
        let back: Box<Error> = core_err.into();

        assert_eq!(back.kind, ErrorKind::Internal);
        assert_eq!(back.message, "test error");
        assert_eq!(back.context.operation, Some("test_op".to_string()));
        assert_eq!(back.context.component, Some("test_comp".to_string()));
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
        assert_eq!(Error::tool_not_found("test").http_status_code(), 404);
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

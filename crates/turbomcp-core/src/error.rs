//! Unified MCP error handling - no_std compatible.
//!
//! This module provides a single error type [`McpError`] for all MCP operations,
//! replacing the previous dual error types (`ServerError` + `protocol::Error`).
//!
//! ## Design Goals
//!
//! 1. **Single Error Type**: One `McpError` across all crates
//! 2. **no_std Compatible**: Core error works without std
//! 3. **Rich Context**: Optional detailed context when `rich-errors` feature enabled
//! 4. **MCP Compliant**: Maps to JSON-RPC error codes per MCP spec
//!
//! ## Features
//!
//! - **Default (no_std)**: Lightweight error with kind, message, and basic context
//! - **`rich-errors`**: Adds UUID tracking and timestamp for observability
//!
//! ## Example
//!
//! ```rust
//! use turbomcp_core::error::{McpError, ErrorKind, McpResult};
//!
//! fn my_tool() -> McpResult<String> {
//!     Err(McpError::new(ErrorKind::ToolNotFound, "calculator"))
//! }
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use core::fmt;
use serde::{Deserialize, Serialize};

/// Result type alias for MCP operations
pub type McpResult<T> = core::result::Result<T, McpError>;

/// Unified MCP error type
///
/// This is the single error type used across all TurboMCP crates in v3.
/// It is `no_std` compatible and maps to JSON-RPC error codes per MCP spec.
///
/// With `rich-errors` feature enabled, includes UUID tracking and timestamps.
///
/// The `context` field is boxed to keep error size small for efficient Result<T, McpError> usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    /// Unique error ID for tracing (only with `rich-errors` feature)
    #[cfg(feature = "rich-errors")]
    pub id: uuid::Uuid,
    /// Error classification
    pub kind: ErrorKind,
    /// Human-readable error message
    pub message: String,
    /// Source location (file:line for debugging)
    /// Note: Never serialized to clients to prevent information leakage
    #[serde(skip_serializing)]
    pub source_location: Option<String>,
    /// Additional context (boxed to keep McpError small)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<alloc::boxed::Box<ErrorContext>>,
    /// Timestamp when error occurred (only with `rich-errors` feature)
    #[cfg(feature = "rich-errors")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Additional error context
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorContext {
    /// Operation being performed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    /// Component where error occurred
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// Request ID for tracing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Structured payload for the JSON-RPC error object's `data` member.
    ///
    /// Unlike [`source_location`](McpError::source_location), this is meant to
    /// reach the client: it is set explicitly by the server author via
    /// [`McpError::with_data`], so it carries only what they chose to expose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Error classification for programmatic handling.
///
/// This enum is `#[non_exhaustive]` — new variants may be added in future
/// minor releases without a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorKind {
    // === MCP-Specific Errors ===
    /// Tool not found (MCP -32001)
    ToolNotFound,
    /// Tool execution failed (MCP -32002)
    ToolExecutionFailed,
    /// Prompt not found (MCP -32003)
    PromptNotFound,
    /// Resource not found (MCP -32004)
    ResourceNotFound,
    /// Resource access denied (MCP -32005)
    ResourceAccessDenied,
    /// Capability not supported (MCP -32006)
    CapabilityNotSupported,
    /// Protocol version mismatch (MCP -32007)
    ProtocolVersionMismatch,
    /// URL elicitation required (MCP -32042)
    UrlElicitationRequired,
    /// User rejected the request (MCP -1)
    UserRejected,

    // === JSON-RPC Standard Errors ===
    /// Parse error (-32700)
    ParseError,
    /// Invalid request (-32600)
    InvalidRequest,
    /// Method not found (-32601)
    MethodNotFound,
    /// Invalid params (-32602)
    InvalidParams,
    /// Internal error (-32603)
    Internal,

    // === General Application Errors ===
    /// Authentication failed
    Authentication,
    /// Permission denied
    PermissionDenied,
    /// Transport/network error
    Transport,
    /// Operation timed out
    Timeout,
    /// Service unavailable
    Unavailable,
    /// Rate limited (-32009)
    RateLimited,
    /// Server overloaded (-32010)
    ServerOverloaded,
    /// Configuration error
    Configuration,
    /// External service failed
    ExternalService,
    /// Operation cancelled
    Cancelled,
    /// Security violation
    Security,
    /// Serialization error
    Serialization,
}

impl McpError {
    /// Create a new error with kind and message
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            #[cfg(feature = "rich-errors")]
            id: uuid::Uuid::new_v4(),
            kind,
            message: message.into(),
            source_location: None,
            context: None,
            #[cfg(feature = "rich-errors")]
            timestamp: chrono::Utc::now(),
        }
    }

    /// Get the error ID (only available with `rich-errors` feature)
    #[cfg(feature = "rich-errors")]
    #[must_use]
    pub const fn id(&self) -> uuid::Uuid {
        self.id
    }

    /// Get the error timestamp (only available with `rich-errors` feature)
    #[cfg(feature = "rich-errors")]
    #[must_use]
    pub const fn timestamp(&self) -> chrono::DateTime<chrono::Utc> {
        self.timestamp
    }

    /// Create a validation/invalid params error
    #[must_use]
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidParams, message)
    }

    /// Create an internal error
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }

    /// Create a safe internal error with sanitized message.
    ///
    /// Use this for errors that may contain sensitive information (file paths,
    /// IP addresses, connection strings, etc.). The message is automatically
    /// sanitized to prevent information leakage per OWASP guidelines.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_core::error::McpError;
    ///
    /// let err = McpError::safe_internal("Failed: postgres://admin:secret@192.168.1.1/db");
    /// assert!(!err.message.contains("secret"));
    /// assert!(!err.message.contains("192.168.1.1"));
    /// ```
    #[must_use]
    pub fn safe_internal(message: impl Into<String>) -> Self {
        let sanitized = crate::security::sanitize_error_message(&message.into());
        Self::new(ErrorKind::Internal, sanitized)
    }

    /// Create a safe tool execution error with sanitized message.
    ///
    /// Like [`safe_internal`](Self::safe_internal), but specifically for tool execution failures.
    #[must_use]
    pub fn safe_tool_execution_failed(
        tool_name: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        let name = tool_name.into();
        let sanitized_reason = crate::security::sanitize_error_message(&reason.into());
        Self::new(
            ErrorKind::ToolExecutionFailed,
            alloc::format!("Tool '{}' failed: {}", name, sanitized_reason),
        )
        .with_operation("tool_execution")
    }

    /// Sanitize this error's message in-place.
    ///
    /// Call this before returning errors to clients in production to ensure
    /// no sensitive information is leaked.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.message = crate::security::sanitize_error_message(&self.message);
        self
    }

    /// Create a parse error
    #[must_use]
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::ParseError, message)
    }

    /// Create an invalid request error
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidRequest, message)
    }

    /// Create a method not found error
    #[must_use]
    pub fn method_not_found(method: impl Into<String>) -> Self {
        let method = method.into();
        Self::new(
            ErrorKind::MethodNotFound,
            alloc::format!("Method not found: {}", method),
        )
    }

    /// Create a tool not found error
    #[must_use]
    pub fn tool_not_found(tool_name: impl Into<String>) -> Self {
        let name = tool_name.into();
        Self::new(
            ErrorKind::ToolNotFound,
            alloc::format!("Tool not found: {}", name),
        )
        .with_operation("tool_lookup")
        .with_component("tool_registry")
    }

    /// Create a tool execution failed error
    #[must_use]
    pub fn tool_execution_failed(tool_name: impl Into<String>, reason: impl Into<String>) -> Self {
        let name = tool_name.into();
        let reason = reason.into();
        Self::new(
            ErrorKind::ToolExecutionFailed,
            alloc::format!("Tool '{}' failed: {}", name, reason),
        )
        .with_operation("tool_execution")
    }

    /// Create a prompt not found error
    #[must_use]
    pub fn prompt_not_found(prompt_name: impl Into<String>) -> Self {
        let name = prompt_name.into();
        Self::new(
            ErrorKind::PromptNotFound,
            alloc::format!("Prompt not found: {}", name),
        )
        .with_operation("prompt_lookup")
        .with_component("prompt_registry")
    }

    /// Create a resource not found error
    ///
    /// Carries `data: {"uri": ...}`, the shape the spec's resources error
    /// example uses, so a client can tell which of several reads failed.
    #[must_use]
    pub fn resource_not_found(uri: impl Into<String>) -> Self {
        let uri = uri.into();
        Self::new(
            ErrorKind::ResourceNotFound,
            alloc::format!("Resource not found: {}", uri),
        )
        .with_operation("resource_lookup")
        .with_component("resource_provider")
        .with_data(serde_json::json!({ "uri": uri }))
    }

    /// Create a resource access denied error
    #[must_use]
    pub fn resource_access_denied(uri: impl Into<String>, reason: impl Into<String>) -> Self {
        let uri = uri.into();
        let reason = reason.into();
        Self::new(
            ErrorKind::ResourceAccessDenied,
            alloc::format!("Access denied to '{}': {}", uri, reason),
        )
        .with_operation("resource_access")
        .with_component("resource_security")
    }

    /// Create a capability not supported error
    #[must_use]
    pub fn capability_not_supported(capability: impl Into<String>) -> Self {
        let cap = capability.into();
        Self::new(
            ErrorKind::CapabilityNotSupported,
            alloc::format!("Capability not supported: {}", cap),
        )
    }

    /// Create a protocol version mismatch error
    #[must_use]
    pub fn protocol_version_mismatch(
        client_version: impl Into<String>,
        server_version: impl Into<String>,
    ) -> Self {
        let client = client_version.into();
        let server = server_version.into();
        Self::new(
            ErrorKind::ProtocolVersionMismatch,
            alloc::format!(
                "Protocol version mismatch: client={}, server={}",
                client,
                server
            ),
        )
    }

    /// Create a timeout error
    #[must_use]
    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Timeout, message)
    }

    /// Create a transport error
    #[must_use]
    pub fn transport(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Transport, message)
    }

    /// Create an authentication error
    #[must_use]
    pub fn authentication(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Authentication, message)
    }

    /// Create a permission denied error
    #[must_use]
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::PermissionDenied, message)
    }

    /// Create a rate limited error
    #[must_use]
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::RateLimited, message)
    }

    /// Create a cancelled error
    #[must_use]
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Cancelled, message)
    }

    /// Create a user rejected error
    #[must_use]
    pub fn user_rejected(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::UserRejected, message)
    }

    /// Create a serialization error
    #[must_use]
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Serialization, message)
    }

    /// Create a security error
    #[must_use]
    pub fn security(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Security, message)
    }

    /// Create an unavailable error
    #[must_use]
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Unavailable, message)
    }

    /// Create a configuration error
    #[must_use]
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Configuration, message)
    }

    /// Create an external service error
    #[must_use]
    pub fn external_service(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::ExternalService, message)
    }

    /// Create a server overloaded error
    #[must_use]
    pub fn server_overloaded() -> Self {
        Self::new(
            ErrorKind::ServerOverloaded,
            "Server is currently overloaded",
        )
    }

    /// Create an error from a JSON-RPC error code
    #[must_use]
    pub fn from_rpc_code(code: i32, message: impl Into<String>) -> Self {
        Self::new(ErrorKind::from_i32(code), message)
    }

    /// Set the operation context
    #[must_use]
    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        let ctx = self
            .context
            .get_or_insert_with(|| alloc::boxed::Box::new(ErrorContext::default()));
        ctx.operation = Some(operation.into());
        self
    }

    /// Set the component context
    #[must_use]
    pub fn with_component(mut self, component: impl Into<String>) -> Self {
        let ctx = self
            .context
            .get_or_insert_with(|| alloc::boxed::Box::new(ErrorContext::default()));
        ctx.component = Some(component.into());
        self
    }

    /// Set the request ID context
    #[must_use]
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        let ctx = self
            .context
            .get_or_insert_with(|| alloc::boxed::Box::new(ErrorContext::default()));
        ctx.request_id = Some(request_id.into());
        self
    }

    /// Attach a structured payload that travels to the client.
    ///
    /// The value becomes the JSON-RPC error object's `data` member (see
    /// `From<McpError> for JsonRpcError`) and the `io.turbomcp/errorData`
    /// entry of a tool result's `_meta` (see [`Self::to_tool_result`]).
    /// Nothing else in the error is forwarded verbatim — `operation`,
    /// `component`, and `source_location` stay server-side — so this is the one
    /// place to put machine-readable detail: a field path, a retry hint, a
    /// validation report.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_core::error::McpError;
    ///
    /// let err = McpError::invalid_params("start must precede end")
    ///     .with_data(serde_json::json!({ "field": "start", "got": "2026-01-02" }));
    /// assert_eq!(err.data().unwrap()["field"], "start");
    /// ```
    #[must_use]
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        let ctx = self
            .context
            .get_or_insert_with(|| alloc::boxed::Box::new(ErrorContext::default()));
        ctx.data = Some(data);
        self
    }

    /// The structured payload set by [`Self::with_data`], if any.
    #[must_use]
    pub fn data(&self) -> Option<&serde_json::Value> {
        self.context.as_ref().and_then(|ctx| ctx.data.as_ref())
    }

    /// Render this error as a tool execution error, preserving its kind.
    ///
    /// Per MCP (SEP-1303) a tool that runs and fails reports the failure in
    /// its result with `isError: true`, not as a JSON-RPC error, so the model
    /// can read it and self-correct. That convention alone would erase the
    /// error's classification, leaving a client unable to tell bad input from
    /// an internal fault. The classification is therefore carried in `_meta`,
    /// which the spec reserves for data the client — not the model — consumes:
    ///
    /// | key | value |
    /// |-----|-------|
    /// | `io.turbomcp/errorKind` | the [`ErrorKind`] as a snake_case string |
    /// | `io.turbomcp/errorCode` | the JSON-RPC code the kind maps to |
    /// | `io.turbomcp/errorData` | the [`Self::with_data`] payload, when set |
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_core::error::McpError;
    ///
    /// let result = McpError::invalid_params("bad date").to_tool_result();
    /// assert!(result.is_error());
    /// let meta = result.meta.unwrap();
    /// assert_eq!(meta["io.turbomcp/errorCode"], -32602);
    /// assert_eq!(meta["io.turbomcp/errorKind"], "invalid_params");
    /// ```
    #[must_use]
    pub fn to_tool_result(&self) -> turbomcp_types::ToolResult {
        use alloc::string::ToString;

        let mut meta = turbomcp_types::MetaMap::new();
        meta.insert(
            crate::meta_keys::ERROR_KIND.to_string(),
            serde_json::to_value(self.kind).unwrap_or(serde_json::Value::Null),
        );
        meta.insert(
            crate::meta_keys::ERROR_CODE.to_string(),
            serde_json::Value::from(self.jsonrpc_error_code()),
        );
        if let Some(data) = self.data() {
            meta.insert(crate::meta_keys::ERROR_DATA.to_string(), data.clone());
        }

        // `message`, not `Display`: the operation/component labels `Display`
        // appends are server-side diagnostics, and this text goes to the model.
        turbomcp_types::ToolResult::error(self.message.clone()).with_meta(meta)
    }

    /// Set the source location (typically file:line)
    #[must_use]
    pub fn with_source_location(mut self, location: impl Into<String>) -> Self {
        self.source_location = Some(location.into());
        self
    }

    /// Check if this error is retryable
    #[must_use]
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

    /// Check if this error is temporary
    #[must_use]
    pub const fn is_temporary(&self) -> bool {
        matches!(
            self.kind,
            ErrorKind::Timeout
                | ErrorKind::Unavailable
                | ErrorKind::RateLimited
                | ErrorKind::ExternalService
                | ErrorKind::ServerOverloaded
        )
    }

    /// Get the JSON-RPC error code for this error
    #[must_use]
    pub const fn jsonrpc_code(&self) -> i32 {
        self.jsonrpc_error_code()
    }

    /// Get the JSON-RPC error code (canonical name)
    #[must_use]
    pub const fn jsonrpc_error_code(&self) -> i32 {
        match self.kind {
            // JSON-RPC standard
            ErrorKind::ParseError => -32700,
            ErrorKind::InvalidRequest => -32600,
            ErrorKind::MethodNotFound => -32601,
            ErrorKind::InvalidParams => -32602,
            // Serialization is a server-side bug; map to Internal so it doesn't
            // collide on the wire with user-visible parameter validation errors.
            ErrorKind::Internal | ErrorKind::Serialization => -32603,
            // MCP specific.
            //
            // The specification assigns exactly two codes of its own —
            // `-32002` (resource not found) and `-32042` (URL elicitation
            // required) — and otherwise tells servers to use the standard
            // JSON-RPC codes for named conditions:
            //
            //   tools.mdx      unknown tool            -> -32602
            //   prompts.mdx    invalid prompt name     -> -32602
            //   prompts.mdx    missing required args   -> -32602
            //   resources.mdx  resource not found      -> -32002
            //   completion.mdx capability unsupported  -> -32601
            //
            // `ErrorKind` stays richer than the wire; the code is the lossy
            // projection of it. Tool failures additionally carry the exact kind
            // in `_meta` under `io.turbomcp/errorKind`, so nothing is lost to a
            // client that wants the detail.
            ErrorKind::UserRejected => -1,
            ErrorKind::ToolNotFound => -32602,
            // NOT -32002: that is the spec's resource-not-found code, and
            // reusing it here made a failed tool call look like a missing
            // resource to any client keying off the code.
            ErrorKind::ToolExecutionFailed => -32603,
            ErrorKind::PromptNotFound => -32602,
            ErrorKind::ResourceNotFound => -32002,
            ErrorKind::ResourceAccessDenied => -32005,
            ErrorKind::CapabilityNotSupported => -32601,
            ErrorKind::ProtocolVersionMismatch => -32007,
            ErrorKind::UrlElicitationRequired => -32042,
            ErrorKind::Authentication => -32008,
            ErrorKind::RateLimited => -32009,
            ErrorKind::ServerOverloaded => -32010,
            // Application specific
            ErrorKind::PermissionDenied => -32011,
            ErrorKind::Timeout => -32012,
            ErrorKind::Unavailable => -32013,
            ErrorKind::Transport => -32014,
            ErrorKind::Configuration => -32015,
            ErrorKind::ExternalService => -32016,
            ErrorKind::Cancelled => -32017,
            ErrorKind::Security => -32018,
        }
    }

    /// Get the HTTP status code equivalent
    #[must_use]
    pub const fn http_status(&self) -> u16 {
        match self.kind {
            // 4xx Client errors
            ErrorKind::InvalidParams
            | ErrorKind::InvalidRequest
            | ErrorKind::UserRejected
            | ErrorKind::ParseError => 400,
            ErrorKind::Authentication => 401,
            ErrorKind::PermissionDenied | ErrorKind::Security | ErrorKind::ResourceAccessDenied => {
                403
            }
            ErrorKind::ToolNotFound
            | ErrorKind::PromptNotFound
            | ErrorKind::ResourceNotFound
            | ErrorKind::MethodNotFound => 404,
            // URL elicitation: server requests client open a URL to continue auth/consent
            ErrorKind::UrlElicitationRequired => 403,
            ErrorKind::Timeout => 408,
            ErrorKind::RateLimited => 429,
            ErrorKind::Cancelled => 499,
            // 5xx Server errors
            ErrorKind::Internal
            | ErrorKind::Configuration
            | ErrorKind::Serialization
            | ErrorKind::ToolExecutionFailed
            | ErrorKind::CapabilityNotSupported
            | ErrorKind::ProtocolVersionMismatch => 500,
            ErrorKind::Transport
            | ErrorKind::ExternalService
            | ErrorKind::Unavailable
            | ErrorKind::ServerOverloaded => 503,
        }
    }
}

impl ErrorKind {
    /// Create ErrorKind from a JSON-RPC error code.
    ///
    /// Includes standard JSON-RPC codes and MCP-specific codes per 2025-11-25 spec.
    #[must_use]
    pub fn from_i32(code: i32) -> Self {
        match code {
            // MCP-specific. `-32002` is the specification's resource-not-found
            // code; the -32001/-32003/-32004 codes are TurboMCP's own pre-3.5.0
            // scheme, still accepted on ingress so a peer running an older
            // TurboMCP is understood correctly.
            -1 => Self::UserRejected,
            -32001 => Self::ToolNotFound,
            -32002 => Self::ResourceNotFound,
            -32003 => Self::PromptNotFound,
            -32004 => Self::ResourceNotFound,
            -32005 => Self::ResourceAccessDenied,
            -32006 => Self::CapabilityNotSupported,
            -32007 => Self::ProtocolVersionMismatch,
            -32008 => Self::Authentication,
            -32009 => Self::RateLimited,
            -32010 => Self::ServerOverloaded,
            // MCP 2025-11-25: URL elicitation required
            -32042 => Self::UrlElicitationRequired,
            // Standard JSON-RPC
            -32600 => Self::InvalidRequest,
            -32601 => Self::MethodNotFound,
            -32602 => Self::InvalidParams,
            -32603 => Self::Internal,
            -32700 => Self::ParseError,
            _ => Self::Internal,
        }
    }

    /// Get a human-readable description
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::ToolNotFound => "Tool not found",
            Self::ToolExecutionFailed => "Tool execution failed",
            Self::PromptNotFound => "Prompt not found",
            Self::ResourceNotFound => "Resource not found",
            Self::ResourceAccessDenied => "Resource access denied",
            Self::CapabilityNotSupported => "Capability not supported",
            Self::ProtocolVersionMismatch => "Protocol version mismatch",
            Self::UrlElicitationRequired => "URL elicitation required",
            Self::UserRejected => "User rejected request",
            Self::ParseError => "Parse error",
            Self::InvalidRequest => "Invalid request",
            Self::MethodNotFound => "Method not found",
            Self::InvalidParams => "Invalid parameters",
            Self::Internal => "Internal error",
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
            Self::Security => "Security violation",
            Self::Serialization => "Serialization error",
        }
    }
}

impl fmt::Display for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ctx) = &self.context {
            if let Some(op) = &ctx.operation {
                write!(f, " (operation: {})", op)?;
            }
            if let Some(comp) = &ctx.component {
                write!(f, " (component: {})", comp)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.description())
    }
}

#[cfg(feature = "std")]
impl std::error::Error for McpError {}

// =========================================================================
// From implementations for common error types
// =========================================================================

impl From<Box<McpError>> for McpError {
    fn from(boxed: Box<McpError>) -> Self {
        *boxed
    }
}

impl From<serde_json::Error> for McpError {
    fn from(err: serde_json::Error) -> Self {
        // Categorize serde_json errors
        let kind = if err.is_syntax() || err.is_eof() {
            ErrorKind::ParseError
        } else if err.is_data() {
            ErrorKind::InvalidParams
        } else {
            ErrorKind::Serialization
        };
        Self::new(kind, alloc::format!("JSON error: {}", err))
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for McpError {
    fn from(err: std::io::Error) -> Self {
        use std::io::ErrorKind as IoKind;
        let kind = match err.kind() {
            IoKind::NotFound => ErrorKind::ResourceNotFound,
            IoKind::PermissionDenied => ErrorKind::PermissionDenied,
            IoKind::ConnectionRefused
            | IoKind::ConnectionReset
            | IoKind::ConnectionAborted
            | IoKind::NotConnected
            | IoKind::BrokenPipe => ErrorKind::Transport,
            IoKind::TimedOut => ErrorKind::Timeout,
            _ => ErrorKind::Internal,
        };
        Self::new(kind, alloc::format!("IO error: {}", err))
    }
}

/// Convenience macro for creating errors with location
#[macro_export]
macro_rules! mcp_err {
    ($kind:expr, $msg:expr) => {
        $crate::error::McpError::new($kind, $msg)
            .with_source_location(concat!(file!(), ":", line!()))
    };
    ($kind:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::error::McpError::new($kind, alloc::format!($fmt, $($arg)*))
            .with_source_location(concat!(file!(), ":", line!()))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn test_error_creation() {
        let err = McpError::invalid_params("missing field");
        assert_eq!(err.kind, ErrorKind::InvalidParams);
        assert!(err.message.contains("missing field"));
    }

    #[test]
    fn test_error_context() {
        let err = McpError::internal("test")
            .with_operation("test_op")
            .with_component("test_comp")
            .with_request_id("req-123");

        let ctx = err.context.unwrap();
        assert_eq!(ctx.operation, Some("test_op".to_string()));
        assert_eq!(ctx.component, Some("test_comp".to_string()));
        assert_eq!(ctx.request_id, Some("req-123".to_string()));
    }

    #[test]
    fn test_jsonrpc_codes() {
        // The spec names the code for each condition; see the egress map.
        assert_eq!(McpError::tool_not_found("x").jsonrpc_code(), -32602);
        assert_eq!(McpError::resource_not_found("x").jsonrpc_code(), -32002);
        assert_eq!(McpError::invalid_params("x").jsonrpc_code(), -32602);
        assert_eq!(McpError::internal("x").jsonrpc_code(), -32603);
    }

    #[test]
    fn test_retryable() {
        assert!(McpError::timeout("x").is_retryable());
        assert!(McpError::rate_limited("x").is_retryable());
        assert!(!McpError::invalid_params("x").is_retryable());
    }

    #[test]
    fn test_http_status() {
        assert_eq!(McpError::tool_not_found("x").http_status(), 404);
        assert_eq!(McpError::authentication("x").http_status(), 401);
        assert_eq!(McpError::internal("x").http_status(), 500);
    }

    #[test]
    fn test_error_size_reasonable() {
        // McpError should fit in 2 cache lines (128 bytes) for efficient Result<T, E>
        assert!(
            core::mem::size_of::<McpError>() <= 128,
            "McpError size: {} bytes (should be ≤128)",
            core::mem::size_of::<McpError>()
        );
    }

    // H-15: ErrorKind::from_i32 maps all known codes
    #[test]
    fn test_error_kind_from_i32() {
        // -32002 is the spec's resource-not-found code. The rest are
        // TurboMCP's pre-3.5.0 scheme, still understood on ingress so an older
        // TurboMCP peer is read correctly.
        assert_eq!(ErrorKind::from_i32(-32001), ErrorKind::ToolNotFound);
        assert_eq!(ErrorKind::from_i32(-32002), ErrorKind::ResourceNotFound);
        assert_eq!(ErrorKind::from_i32(-32003), ErrorKind::PromptNotFound);
        assert_eq!(ErrorKind::from_i32(-32004), ErrorKind::ResourceNotFound);
        assert_eq!(ErrorKind::from_i32(-32005), ErrorKind::ResourceAccessDenied);
        assert_eq!(
            ErrorKind::from_i32(-32006),
            ErrorKind::CapabilityNotSupported
        );
        assert_eq!(
            ErrorKind::from_i32(-32007),
            ErrorKind::ProtocolVersionMismatch
        );
        assert_eq!(ErrorKind::from_i32(-32008), ErrorKind::Authentication);
        assert_eq!(ErrorKind::from_i32(-32009), ErrorKind::RateLimited);
        assert_eq!(ErrorKind::from_i32(-32010), ErrorKind::ServerOverloaded);
        // MCP 2025-11-25: URL elicitation required has its own variant
        assert_eq!(
            ErrorKind::from_i32(-32042),
            ErrorKind::UrlElicitationRequired
        );
        // Standard JSON-RPC codes
        assert_eq!(ErrorKind::from_i32(-32600), ErrorKind::InvalidRequest);
        assert_eq!(ErrorKind::from_i32(-32601), ErrorKind::MethodNotFound);
        assert_eq!(ErrorKind::from_i32(-32602), ErrorKind::InvalidParams);
        assert_eq!(ErrorKind::from_i32(-32603), ErrorKind::Internal);
        assert_eq!(ErrorKind::from_i32(-32700), ErrorKind::ParseError);
        // Unknown codes fall back to Internal
        assert_eq!(ErrorKind::from_i32(-99999), ErrorKind::Internal);
        assert_eq!(ErrorKind::from_i32(0), ErrorKind::Internal);
    }
}

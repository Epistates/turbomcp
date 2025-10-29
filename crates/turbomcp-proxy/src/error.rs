//! Error types for turbomcp-proxy
//!
//! Follows TurboMCP's 3-tier error hierarchy:
//! - Protocol: MCP protocol errors (preserved from turbomcp-protocol)
//! - Transport: Network/transport errors (from turbomcp-transport)
//! - Proxy: Proxy-specific errors (introspection, codegen, configuration)

use thiserror::Error;

/// Result type for proxy operations
pub type ProxyResult<T> = std::result::Result<T, ProxyError>;

/// Main error type for turbomcp-proxy
///
/// Follows TurboMCP error hierarchy pattern:
/// - Wraps protocol errors to preserve error codes (like -1 for user rejection)
/// - Converts transport errors automatically
/// - Provides structured proxy-specific errors with context
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ProxyError {
    /// Protocol-level error from MCP protocol layer
    ///
    /// Preserves the full protocol error with context and error codes.
    /// This is critical for forwarding errors correctly (e.g., user rejection = -1).
    #[error("Protocol error: {0}")]
    Protocol(#[from] Box<turbomcp_protocol::Error>),

    /// Transport layer errors
    ///
    /// Automatically converted from turbomcp-transport errors.
    #[error("Transport error: {0}")]
    Transport(#[from] turbomcp_transport::TransportError),

    /// Introspection error
    ///
    /// Errors during server capability discovery.
    #[error("Introspection error: {message}")]
    Introspection {
        message: String,
        context: Option<String>,
    },

    /// Code generation error
    ///
    /// Errors during template rendering or code generation.
    #[error("Code generation error: {message}")]
    Codegen {
        message: String,
        template: Option<String>,
    },

    /// Configuration error
    ///
    /// Invalid proxy configuration (missing required fields, invalid values).
    #[error("Configuration error: {message}")]
    Configuration {
        message: String,
        key: Option<String>,
    },

    /// Backend connection error
    ///
    /// Failed to connect to backend MCP server.
    #[error("Backend connection error: {message}")]
    BackendConnection {
        message: String,
        backend_type: Option<String>,
    },

    /// Backend operation error
    ///
    /// Backend server returned an error or operation failed.
    #[error("Backend error: {message}")]
    Backend {
        message: String,
        operation: Option<String>,
    },

    /// Schema validation error
    ///
    /// JSON schema validation failed for tool inputs/outputs.
    #[error("Schema validation error: {message}")]
    SchemaValidation {
        message: String,
        schema_path: Option<String>,
    },

    /// Timeout error
    ///
    /// Operation exceeded configured timeout.
    #[error("Timeout: {operation} exceeded {timeout_ms}ms")]
    Timeout { operation: String, timeout_ms: u64 },

    /// Rate limit exceeded
    ///
    /// Too many requests to backend server.
    #[error("Rate limit exceeded: {message}")]
    RateLimitExceeded {
        message: String,
        retry_after_ms: Option<u64>,
    },

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP error (runtime feature only)
    #[cfg(feature = "runtime")]
    #[error("HTTP error: {message}")]
    Http {
        message: String,
        status_code: Option<u16>,
    },
}

impl ProxyError {
    /// Create an introspection error
    pub fn introspection(message: impl Into<String>) -> Self {
        Self::Introspection {
            message: message.into(),
            context: None,
        }
    }

    /// Create an introspection error with context
    pub fn introspection_with_context(
        message: impl Into<String>,
        context: impl Into<String>,
    ) -> Self {
        Self::Introspection {
            message: message.into(),
            context: Some(context.into()),
        }
    }

    /// Create a codegen error
    pub fn codegen(message: impl Into<String>) -> Self {
        Self::Codegen {
            message: message.into(),
            template: None,
        }
    }

    /// Create a codegen error with template context
    pub fn codegen_with_template(message: impl Into<String>, template: impl Into<String>) -> Self {
        Self::Codegen {
            message: message.into(),
            template: Some(template.into()),
        }
    }

    /// Create a configuration error
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
            key: None,
        }
    }

    /// Create a configuration error with key context
    pub fn configuration_with_key(message: impl Into<String>, key: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
            key: Some(key.into()),
        }
    }

    /// Create a backend connection error
    pub fn backend_connection(message: impl Into<String>) -> Self {
        Self::BackendConnection {
            message: message.into(),
            backend_type: None,
        }
    }

    /// Create a backend connection error with backend type
    pub fn backend_connection_with_type(
        message: impl Into<String>,
        backend_type: impl Into<String>,
    ) -> Self {
        Self::BackendConnection {
            message: message.into(),
            backend_type: Some(backend_type.into()),
        }
    }

    /// Create a backend operation error
    pub fn backend(message: impl Into<String>) -> Self {
        Self::Backend {
            message: message.into(),
            operation: None,
        }
    }

    /// Create a backend error with operation context
    pub fn backend_with_operation(
        message: impl Into<String>,
        operation: impl Into<String>,
    ) -> Self {
        Self::Backend {
            message: message.into(),
            operation: Some(operation.into()),
        }
    }

    /// Create a schema validation error
    pub fn schema_validation(message: impl Into<String>) -> Self {
        Self::SchemaValidation {
            message: message.into(),
            schema_path: None,
        }
    }

    /// Create a timeout error
    pub fn timeout(operation: impl Into<String>, timeout_ms: u64) -> Self {
        Self::Timeout {
            operation: operation.into(),
            timeout_ms,
        }
    }

    /// Create a rate limit error
    pub fn rate_limit_exceeded(message: impl Into<String>) -> Self {
        Self::RateLimitExceeded {
            message: message.into(),
            retry_after_ms: None,
        }
    }

    /// Create an HTTP error (runtime feature only)
    #[cfg(feature = "runtime")]
    pub fn http(message: impl Into<String>) -> Self {
        Self::Http {
            message: message.into(),
            status_code: None,
        }
    }

    /// Create an HTTP error with status code
    #[cfg(feature = "runtime")]
    pub fn http_with_status(message: impl Into<String>, status_code: u16) -> Self {
        Self::Http {
            message: message.into(),
            status_code: Some(status_code),
        }
    }

    /// Sanitize error message for client responses
    ///
    /// Removes internal details to prevent information disclosure.
    pub fn sanitize(&self) -> String {
        match self {
            Self::Protocol(_) => "Protocol error occurred".to_string(),
            Self::Transport(_) => "Transport error occurred".to_string(),
            Self::Introspection { .. } => "Server introspection failed".to_string(),
            Self::Codegen { .. } => "Code generation failed".to_string(),
            Self::Configuration { .. } => "Configuration error".to_string(),
            Self::BackendConnection { .. } => "Backend connection failed".to_string(),
            Self::Backend { .. } => "Backend operation failed".to_string(),
            Self::SchemaValidation { .. } => "Schema validation failed".to_string(),
            Self::Timeout { operation, .. } => {
                format!("Operation '{}' timed out", operation)
            }
            Self::RateLimitExceeded { .. } => "Rate limit exceeded".to_string(),
            Self::Serialization(_) => "Data serialization error".to_string(),
            Self::Io(_) => "IO error occurred".to_string(),
            #[cfg(feature = "runtime")]
            Self::Http { status_code, .. } => {
                if let Some(code) = status_code {
                    format!("HTTP error {}", code)
                } else {
                    "HTTP error occurred".to_string()
                }
            }
        }
    }

    /// Check if this is a protocol error
    pub fn is_protocol_error(&self) -> bool {
        matches!(self, Self::Protocol(_))
    }

    /// Check if this is a transport error
    pub fn is_transport_error(&self) -> bool {
        matches!(self, Self::Transport(_))
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport(_)
                | Self::BackendConnection { .. }
                | Self::Timeout { .. }
                | Self::Io(_)
        )
    }
}

/// Extension trait for Result types to add proxy error context
pub trait ProxyErrorExt<T> {
    /// Add introspection context to error
    fn introspection_context(self, context: impl Into<String>) -> ProxyResult<T>;

    /// Add backend context to error
    fn backend_context(self, context: impl Into<String>) -> ProxyResult<T>;

    /// Add configuration context to error
    fn config_context(self, context: impl Into<String>) -> ProxyResult<T>;
}

impl<T, E> ProxyErrorExt<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn introspection_context(self, context: impl Into<String>) -> ProxyResult<T> {
        self.map_err(|e| ProxyError::introspection_with_context(e.to_string(), context.into()))
    }

    fn backend_context(self, context: impl Into<String>) -> ProxyResult<T> {
        self.map_err(|e| ProxyError::backend_with_operation(e.to_string(), context.into()))
    }

    fn config_context(self, context: impl Into<String>) -> ProxyResult<T> {
        self.map_err(|e| ProxyError::configuration_with_key(e.to_string(), context.into()))
    }
}

/// Convert protocol errors from turbomcp-client
impl From<turbomcp_protocol::Error> for ProxyError {
    fn from(err: turbomcp_protocol::Error) -> Self {
        Self::Protocol(Box::new(err))
    }
}

/// Convert proxy errors back to protocol errors for JSON-RPC responses
impl From<ProxyError> for Box<turbomcp_protocol::Error> {
    fn from(err: ProxyError) -> Self {
        match err {
            // Unwrap protocol errors directly to preserve error codes (critical!)
            ProxyError::Protocol(protocol_err) => protocol_err,

            // Map proxy-specific errors to appropriate protocol errors
            ProxyError::Transport(transport_err) => {
                turbomcp_protocol::Error::transport(transport_err.to_string())
            }
            ProxyError::Introspection { message, context } => {
                let msg = if let Some(ctx) = context {
                    format!("{}: {}", message, ctx)
                } else {
                    message
                };
                turbomcp_protocol::Error::internal(msg)
            }
            ProxyError::Codegen { message, template } => {
                let msg = if let Some(tmpl) = template {
                    format!("{} (template: {})", message, tmpl)
                } else {
                    message
                };
                turbomcp_protocol::Error::internal(msg)
            }
            ProxyError::Configuration { message, key } => {
                let msg = if let Some(k) = key {
                    format!("{} (key: {})", message, k)
                } else {
                    message
                };
                turbomcp_protocol::Error::invalid_params(msg)
            }
            ProxyError::BackendConnection {
                message,
                backend_type,
            } => {
                let msg = if let Some(bt) = backend_type {
                    format!("{} (backend: {})", message, bt)
                } else {
                    message
                };
                turbomcp_protocol::Error::transport(msg)
            }
            ProxyError::Backend { message, operation } => {
                let msg = if let Some(op) = operation {
                    format!("{} (operation: {})", message, op)
                } else {
                    message
                };
                turbomcp_protocol::Error::internal(msg)
            }
            ProxyError::SchemaValidation { message, .. } => {
                turbomcp_protocol::Error::invalid_params(message)
            }
            ProxyError::Timeout {
                operation,
                timeout_ms,
            } => turbomcp_protocol::Error::timeout(format!(
                "{} exceeded {}ms",
                operation, timeout_ms
            )),
            ProxyError::RateLimitExceeded { message, .. } => {
                turbomcp_protocol::Error::rate_limited(message)
            }
            ProxyError::Serialization(err) => {
                turbomcp_protocol::Error::serialization(err.to_string())
            }
            ProxyError::Io(err) => turbomcp_protocol::Error::transport(err.to_string()),
            #[cfg(feature = "runtime")]
            ProxyError::Http {
                message,
                status_code,
            } => {
                let msg = if let Some(code) = status_code {
                    format!("{} (HTTP {})", message, code)
                } else {
                    message
                };
                turbomcp_protocol::Error::transport(msg)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = ProxyError::introspection("test");
        assert!(matches!(err, ProxyError::Introspection { .. }));

        let err = ProxyError::configuration("test");
        assert!(matches!(err, ProxyError::Configuration { .. }));
    }

    #[test]
    fn test_error_creation_with_context() {
        let err = ProxyError::introspection_with_context("failed", "stdio backend");
        match err {
            ProxyError::Introspection { message, context } => {
                assert_eq!(message, "failed");
                assert_eq!(context, Some("stdio backend".to_string()));
            }
            _ => panic!("Wrong error type"),
        }
    }

    #[test]
    fn test_error_display() {
        let err = ProxyError::introspection("failed to connect");
        assert!(err.to_string().contains("Introspection error"));
        assert!(err.to_string().contains("failed to connect"));
    }

    #[test]
    fn test_error_sanitization() {
        let err = ProxyError::configuration_with_key("Invalid API key format", "api_key");
        assert_eq!(err.sanitize(), "Configuration error");
    }

    #[test]
    fn test_retryable_errors() {
        let err = ProxyError::timeout("tool_call", 30000);
        assert!(err.is_retryable());

        let err = ProxyError::configuration("bad config");
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_protocol_error_preservation() {
        let protocol_err = turbomcp_protocol::Error::user_rejected("User cancelled");
        let proxy_err = ProxyError::from(protocol_err);

        // Convert back to protocol error
        let back_to_protocol: Box<turbomcp_protocol::Error> = proxy_err.into();

        // Error kind should be preserved (kind is a field, not a method)
        assert_eq!(
            back_to_protocol.kind,
            turbomcp_protocol::ErrorKind::UserRejected
        );
    }

    #[test]
    fn test_error_ext_trait() {
        use std::fs;

        let result: Result<String, std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));

        let proxy_result = result.introspection_context("reading config");
        assert!(proxy_result.is_err());

        match proxy_result.unwrap_err() {
            ProxyError::Introspection { message, context } => {
                assert!(message.contains("file not found"));
                assert_eq!(context, Some("reading config".to_string()));
            }
            _ => panic!("Wrong error type"),
        }
    }
}

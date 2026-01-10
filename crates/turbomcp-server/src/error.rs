//! Server error types and handling
//!
//! v3.0: Uses the unified `McpError` type from `turbomcp-protocol`.
//! The `ServerError` type has been removed in favor of the unified error type.

// Re-export unified error types from protocol
pub use turbomcp_protocol::{ErrorKind, McpError, McpResult};

/// Result type alias for server operations (convenience alias for McpResult)
pub type ServerResult<T> = McpResult<T>;

/// Error recovery strategies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorRecovery {
    /// Retry the operation
    Retry,
    /// Skip and continue
    Skip,
    /// Fail immediately
    Fail,
    /// Graceful degradation
    Degrade,
}

/// Extension trait for server-specific error handling
pub trait ServerErrorExt {
    /// Check if this error should cause server shutdown
    fn is_fatal(&self) -> bool;

    /// Create a lifecycle error
    fn lifecycle(message: impl Into<String>) -> McpError;

    /// Create a shutdown error
    fn shutdown(message: impl Into<String>) -> McpError;

    /// Create a middleware error
    fn middleware(name: impl Into<String>, message: impl Into<String>) -> McpError;

    /// Create a registry error
    fn registry(message: impl Into<String>) -> McpError;

    /// Create a routing error
    fn routing(message: impl Into<String>) -> McpError;

    /// Create a resource exhausted error
    fn resource_exhausted(resource: impl Into<String>) -> McpError;
}

impl ServerErrorExt for McpError {
    fn is_fatal(&self) -> bool {
        // Fatal errors are internal errors that indicate unrecoverable state
        matches!(self.kind, ErrorKind::Internal | ErrorKind::Configuration)
    }

    fn lifecycle(message: impl Into<String>) -> McpError {
        McpError::internal(format!("Lifecycle error: {}", message.into()))
            .with_component("lifecycle")
    }

    fn shutdown(message: impl Into<String>) -> McpError {
        McpError::internal(format!("Shutdown error: {}", message.into()))
            .with_component("shutdown")
    }

    fn middleware(name: impl Into<String>, message: impl Into<String>) -> McpError {
        let name = name.into();
        McpError::internal(format!("Middleware error ({}): {}", name, message.into()))
            .with_component(name)
    }

    fn registry(message: impl Into<String>) -> McpError {
        McpError::internal(format!("Registry error: {}", message.into()))
            .with_component("registry")
    }

    fn routing(message: impl Into<String>) -> McpError {
        McpError::internal(format!("Routing error: {}", message.into()))
            .with_component("routing")
    }

    fn resource_exhausted(resource: impl Into<String>) -> McpError {
        McpError::new(
            ErrorKind::ServerOverloaded,
            format!("Resource exhausted: {}", resource.into()),
        )
    }
}

// Note: From conversions for TransportError and RegistryError cannot be implemented here
// due to orphan rules (McpError is defined externally). Use .map_err() at call sites instead.
// Example: .map_err(|e| McpError::transport(format!("Transport error: {}", e)))

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_error_constructors() {
        let err = McpError::handler("handler failed");
        assert!(err.message.contains("handler failed"));

        let err = McpError::configuration("config error");
        assert!(err.message.contains("config error"));

        let err = McpError::authentication("auth failed");
        assert!(err.message.contains("auth failed"));

        let err = McpError::permission_denied("access denied");
        assert!(err.message.contains("access denied"));

        let err = McpError::rate_limited("too many requests");
        assert!(err.message.contains("too many requests"));

        let err = McpError::timeout("operation timed out");
        assert!(err.message.contains("operation timed out"));

        let err = McpError::resource_not_found("resource");
        assert!(err.message.contains("resource"));

        let err = McpError::internal("internal error");
        assert!(err.message.contains("internal error"));
    }

    #[test]
    fn test_server_error_ext() {
        let err = McpError::lifecycle("startup failed");
        assert!(err.message.contains("Lifecycle error"));
        assert!(err.message.contains("startup failed"));

        let err = McpError::shutdown("shutdown failed");
        assert!(err.message.contains("Shutdown error"));

        let err = McpError::middleware("auth", "failed");
        assert!(err.message.contains("Middleware error"));
        assert!(err.message.contains("auth"));

        let err = McpError::registry("registry error");
        assert!(err.message.contains("Registry error"));

        let err = McpError::routing("route not found");
        assert!(err.message.contains("Routing error"));

        let err = McpError::resource_exhausted("memory");
        assert!(err.message.contains("Resource exhausted"));
        assert_eq!(err.kind, ErrorKind::ServerOverloaded);
    }

    #[test]
    fn test_mcp_error_retryable() {
        assert!(McpError::timeout("op").is_retryable());
        assert!(McpError::rate_limited("too many").is_retryable());
        assert!(McpError::unavailable("service down").is_retryable());

        assert!(!McpError::handler("failed").is_retryable());
        assert!(!McpError::authentication("failed").is_retryable());
        assert!(!McpError::permission_denied("denied").is_retryable());
        assert!(!McpError::resource_not_found("resource").is_retryable());
    }

    #[test]
    fn test_mcp_error_fatal() {
        assert!(McpError::lifecycle("failed").is_fatal());
        assert!(McpError::internal("error").is_fatal());
        assert!(McpError::configuration("bad config").is_fatal());
        // Note: handler() is an alias for internal(), so it is also fatal
        assert!(McpError::handler("failed").is_fatal());

        assert!(!McpError::timeout("op").is_fatal());
        assert!(!McpError::rate_limited("too many").is_fatal());
    }

    #[test]
    fn test_mcp_error_jsonrpc_codes() {
        assert_eq!(McpError::resource_not_found("x").jsonrpc_code(), -32004);
        assert_eq!(McpError::authentication("x").jsonrpc_code(), -32008);
        assert_eq!(McpError::permission_denied("x").jsonrpc_code(), -32011);
        assert_eq!(McpError::rate_limited("x").jsonrpc_code(), -32009);
        assert_eq!(McpError::internal("x").jsonrpc_code(), -32603);
    }

    #[test]
    fn test_error_recovery_enum() {
        let recovery = ErrorRecovery::Retry;
        assert_eq!(recovery, ErrorRecovery::Retry);
        assert_ne!(recovery, ErrorRecovery::Skip);

        assert_eq!(format!("{:?}", ErrorRecovery::Retry), "Retry");
        assert_eq!(format!("{:?}", ErrorRecovery::Skip), "Skip");
        assert_eq!(format!("{:?}", ErrorRecovery::Fail), "Fail");
        assert_eq!(format!("{:?}", ErrorRecovery::Degrade), "Degrade");
    }

    #[test]
    fn test_server_result_type() {
        fn returns_ok() -> ServerResult<i32> {
            Ok(42)
        }

        fn returns_error() -> ServerResult<i32> {
            Err(McpError::handler("test error"))
        }

        assert!(returns_ok().is_ok());
        assert_eq!(returns_ok().unwrap(), 42);

        assert!(returns_error().is_err());
    }

    #[test]
    fn test_error_context() {
        let err = McpError::internal("test")
            .with_operation("test_op")
            .with_component("test_comp")
            .with_request_id("req-123");

        let ctx = err.context.as_ref().unwrap();
        assert_eq!(ctx.operation, Some("test_op".to_string()));
        assert_eq!(ctx.component, Some("test_comp".to_string()));
        assert_eq!(ctx.request_id, Some("req-123".to_string()));
    }
}

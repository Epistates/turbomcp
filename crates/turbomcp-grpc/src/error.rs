//! Error types for gRPC transport
//!
//! This module provides error handling for gRPC operations,
//! mapping between MCP errors and gRPC status codes.

use thiserror::Error;
use tonic::Status;
use turbomcp_core::McpError;

/// Result type for gRPC operations
pub type GrpcResult<T> = Result<T, GrpcError>;

/// Error type for gRPC transport operations
#[derive(Debug, Error)]
pub enum GrpcError {
    /// gRPC transport error
    #[error("gRPC transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    /// gRPC status error
    #[error("gRPC status error: {0}")]
    Status(#[from] Status),

    /// MCP protocol error
    #[error("MCP error: {0}")]
    Mcp(#[from] McpError),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Invalid request
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),

    /// Timeout error
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),
}

impl GrpcError {
    /// Create a serialization error
    #[must_use]
    pub fn serialization(msg: impl Into<String>) -> Self {
        Self::Serialization(msg.into())
    }

    /// Create an invalid request error
    #[must_use]
    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self::InvalidRequest(msg.into())
    }

    /// Create a connection error
    #[must_use]
    pub fn connection(msg: impl Into<String>) -> Self {
        Self::Connection(msg.into())
    }

    /// Create a timeout error
    #[must_use]
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout(msg.into())
    }

    /// Create a configuration error
    #[must_use]
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }
}

impl From<GrpcError> for Status {
    fn from(err: GrpcError) -> Self {
        match err {
            GrpcError::Transport(e) => Status::unavailable(e.to_string()),
            GrpcError::Status(s) => s,
            GrpcError::Mcp(e) => mcp_error_to_status(&e),
            GrpcError::Serialization(msg) | GrpcError::InvalidRequest(msg) => {
                Status::invalid_argument(msg)
            }
            GrpcError::Connection(msg) => Status::unavailable(msg),
            GrpcError::Timeout(msg) => Status::deadline_exceeded(msg),
            GrpcError::Config(msg) => Status::failed_precondition(msg),
        }
    }
}

impl From<serde_json::Error> for GrpcError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

/// Convert MCP error to gRPC status
fn mcp_error_to_status(err: &McpError) -> Status {
    let code = err.jsonrpc_code();

    // Map JSON-RPC error codes to gRPC status codes
    match code {
        -32700 => Status::invalid_argument(format!("Parse error: {err}")),
        -32600 => Status::invalid_argument(format!("Invalid request: {err}")),
        -32601 => Status::unimplemented(format!("Method not found: {err}")),
        -32602 => Status::invalid_argument(format!("Invalid params: {err}")),
        -32603 => Status::internal(format!("Internal error: {err}")),
        // MCP-specific error codes
        -32001 => Status::resource_exhausted(format!("Resource exceeded: {err}")),
        -32002 => Status::cancelled(format!("Request cancelled: {err}")),
        -32042 => Status::unavailable(format!("URL elicitation required: {err}")),
        // Application errors (-32000 to -32099)
        _ if (-32099..=-32000).contains(&code) => {
            Status::internal(format!("Application error: {err}"))
        }
        // Unknown error
        _ => Status::unknown(format!("Unknown error: {err}")),
    }
}

/// Convert gRPC status to MCP error
#[must_use]
pub fn status_to_mcp_error(status: &Status) -> McpError {
    use tonic::Code;

    match status.code() {
        Code::InvalidArgument => McpError::invalid_params(status.message()),
        Code::NotFound | Code::Unimplemented => McpError::method_not_found(status.message()),
        Code::Internal => McpError::internal(status.message()),
        Code::Unavailable => McpError::transport(status.message()),
        Code::DeadlineExceeded => McpError::timeout(status.message()),
        Code::Cancelled => McpError::cancelled(status.message()),
        Code::ResourceExhausted => McpError::rate_limited(status.message()),
        Code::PermissionDenied => McpError::permission_denied(status.message()),
        Code::Unauthenticated => McpError::authentication(status.message()),
        _ => McpError::internal(format!("gRPC error: {}", status.message())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_error_to_status() {
        let err = McpError::method_not_found("unknown_method");
        let status: Status = GrpcError::Mcp(err).into();
        assert_eq!(status.code(), tonic::Code::Unimplemented);
    }

    #[test]
    fn test_status_to_mcp_error() {
        let status = Status::invalid_argument("bad params");
        let err = status_to_mcp_error(&status);
        assert_eq!(err.jsonrpc_code(), -32602);
    }

    #[test]
    fn test_serialization_error() {
        let err = GrpcError::serialization("invalid JSON");
        let status: Status = err.into();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }
}

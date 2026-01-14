//! Error types for MCP operations.
//!
//! This module provides a unified error type for all MCP operations,
//! compatible with JSON-RPC error codes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP error type with JSON-RPC compatible error codes.
///
/// This error type can be used throughout the MCP SDK and is automatically
/// converted to JSON-RPC error responses when needed.
///
/// # Example
///
/// ```
/// use turbomcp_types::McpError;
///
/// // Create common errors
/// let not_found = McpError::method_not_found("tool_xyz");
/// let invalid = McpError::invalid_params("missing 'name' field");
/// let internal = McpError::internal("database connection failed");
///
/// // Custom errors
/// let custom = McpError::new(-32000, "Custom server error");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpError {
    /// JSON-RPC error code
    pub code: i32,
    /// Human-readable error message
    pub message: String,
    /// Optional additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// Standard JSON-RPC error codes
impl McpError {
    /// Parse error (-32700): Invalid JSON received
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid Request (-32600): Invalid JSON-RPC request
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found (-32601): Method does not exist
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params (-32602): Invalid method parameters
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error (-32603): Internal JSON-RPC error
    pub const INTERNAL_ERROR: i32 = -32603;
}

impl McpError {
    /// Create a new MCP error with code and message.
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create a parse error.
    #[must_use]
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(Self::PARSE_ERROR, message)
    }

    /// Create an invalid request error.
    #[must_use]
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(Self::INVALID_REQUEST, message)
    }

    /// Create a method not found error.
    #[must_use]
    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self::new(
            Self::METHOD_NOT_FOUND,
            format!("Method not found: {}", method.into()),
        )
    }

    /// Create an invalid params error.
    #[must_use]
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(Self::INVALID_PARAMS, message)
    }

    /// Create an internal error.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(Self::INTERNAL_ERROR, message)
    }

    /// Create a tool not found error.
    ///
    /// # MCP Spec Compliance (CRITICAL-003)
    ///
    /// Uses INTERNAL_ERROR (-32603) per MCP specification for "not found"
    /// conditions within valid method calls. This is distinct from
    /// METHOD_NOT_FOUND (-32601) which indicates the RPC method itself
    /// doesn't exist.
    #[must_use]
    pub fn tool_not_found(name: impl Into<String>) -> Self {
        Self::new(
            Self::INTERNAL_ERROR,
            format!("Tool not found: {}", name.into()),
        )
    }

    /// Create a resource not found error.
    ///
    /// # MCP Spec Compliance (CRITICAL-003)
    ///
    /// Uses INTERNAL_ERROR (-32603) per MCP specification for "not found"
    /// conditions within valid method calls.
    #[must_use]
    pub fn resource_not_found(uri: impl Into<String>) -> Self {
        Self::new(
            Self::INTERNAL_ERROR,
            format!("Resource not found: {}", uri.into()),
        )
    }

    /// Create a prompt not found error.
    ///
    /// # MCP Spec Compliance (CRITICAL-003)
    ///
    /// Uses INTERNAL_ERROR (-32603) per MCP specification for "not found"
    /// conditions within valid method calls.
    #[must_use]
    pub fn prompt_not_found(name: impl Into<String>) -> Self {
        Self::new(
            Self::INTERNAL_ERROR,
            format!("Prompt not found: {}", name.into()),
        )
    }

    /// Add additional data to the error.
    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Check if this is a parse error.
    #[must_use]
    pub fn is_parse_error(&self) -> bool {
        self.code == Self::PARSE_ERROR
    }

    /// Check if this is an invalid request error.
    #[must_use]
    pub fn is_invalid_request(&self) -> bool {
        self.code == Self::INVALID_REQUEST
    }

    /// Check if this is a method not found error.
    #[must_use]
    pub fn is_method_not_found(&self) -> bool {
        self.code == Self::METHOD_NOT_FOUND
    }

    /// Check if this is an invalid params error.
    #[must_use]
    pub fn is_invalid_params(&self) -> bool {
        self.code == Self::INVALID_PARAMS
    }

    /// Check if this is an internal error.
    #[must_use]
    pub fn is_internal_error(&self) -> bool {
        self.code == Self::INTERNAL_ERROR
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for McpError {}

// Conversions from common error types
impl From<serde_json::Error> for McpError {
    fn from(err: serde_json::Error) -> Self {
        Self::parse_error(err.to_string())
    }
}

impl From<std::io::Error> for McpError {
    fn from(err: std::io::Error) -> Self {
        Self::internal(err.to_string())
    }
}

/// Result type alias for MCP operations.
pub type McpResult<T> = Result<T, McpError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        assert_eq!(McpError::PARSE_ERROR, -32700);
        assert_eq!(McpError::INVALID_REQUEST, -32600);
        assert_eq!(McpError::METHOD_NOT_FOUND, -32601);
        assert_eq!(McpError::INVALID_PARAMS, -32602);
        assert_eq!(McpError::INTERNAL_ERROR, -32603);
    }

    #[test]
    fn test_parse_error() {
        let err = McpError::parse_error("invalid JSON");
        assert!(err.is_parse_error());
        assert_eq!(err.code, -32700);
    }

    #[test]
    fn test_method_not_found() {
        let err = McpError::method_not_found("unknown_method");
        assert!(err.is_method_not_found());
        assert!(err.message.contains("unknown_method"));
    }

    #[test]
    fn test_tool_not_found() {
        let err = McpError::tool_not_found("my_tool");
        assert!(err.message.contains("Tool not found"));
        assert!(err.message.contains("my_tool"));
    }

    #[test]
    fn test_resource_not_found() {
        let err = McpError::resource_not_found("file:///test.txt");
        assert!(err.is_internal_error()); // CRITICAL-003: Uses INTERNAL_ERROR per MCP spec
        assert!(err.message.contains("file:///test.txt"));
    }

    #[test]
    fn test_error_with_data() {
        let err = McpError::internal("something failed")
            .with_data(serde_json::json!({"details": "more info"}));
        assert!(err.data.is_some());
    }

    #[test]
    fn test_error_display() {
        let err = McpError::internal("test error");
        let display = format!("{}", err);
        assert!(display.contains("-32603"));
        assert!(display.contains("test error"));
    }

    #[test]
    fn test_error_serde() {
        let err = McpError::internal("test");
        let json = serde_json::to_string(&err).unwrap();
        let parsed: McpError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, parsed);
    }

    #[test]
    fn test_from_serde_error() {
        let json_err: Result<i32, _> = serde_json::from_str("invalid");
        let mcp_err: McpError = json_err.unwrap_err().into();
        assert!(mcp_err.is_parse_error());
    }
}

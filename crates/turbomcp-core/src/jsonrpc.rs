//! JSON-RPC 2.0 types - no_std compatible.
//!
//! This module provides core JSON-RPC 2.0 types that can be used in `no_std` environments.

use alloc::string::{String, ToString};
use core::fmt;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// JSON-RPC version constant
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC version type (always "2.0")
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JsonRpcVersion;

impl Serialize for JsonRpcVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(JSONRPC_VERSION)
    }
}

impl<'de> Deserialize<'de> for JsonRpcVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let version = String::deserialize(deserializer)?;
        if version == JSONRPC_VERSION {
            Ok(JsonRpcVersion)
        } else {
            Err(serde::de::Error::custom(alloc::format!(
                "Invalid JSON-RPC version: expected '{}', got '{}'",
                JSONRPC_VERSION,
                version
            )))
        }
    }
}

/// Request identifier - can be string or number
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// String identifier
    String(String),
    /// Numeric identifier
    Number(i64),
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => write!(f, "{}", s),
            Self::Number(n) => write!(f, "{}", n),
        }
    }
}

impl From<String> for RequestId {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for RequestId {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<i64> for RequestId {
    fn from(n: i64) -> Self {
        Self::Number(n)
    }
}

impl From<i32> for RequestId {
    fn from(n: i32) -> Self {
        Self::Number(n as i64)
    }
}

/// JSON-RPC request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version
    pub jsonrpc: JsonRpcVersion,
    /// Request method name
    pub method: String,
    /// Request parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// Request identifier
    pub id: RequestId,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request
    #[must_use]
    pub fn new(method: impl Into<String>, params: Option<Value>, id: impl Into<RequestId>) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            method: method.into(),
            params,
            id: id.into(),
        }
    }

    /// Create a request without parameters
    #[must_use]
    pub fn without_params(method: impl Into<String>, id: impl Into<RequestId>) -> Self {
        Self::new(method, None, id)
    }
}

/// JSON-RPC notification (no response expected)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// JSON-RPC version
    pub jsonrpc: JsonRpcVersion,
    /// Notification method name
    pub method: String,
    /// Notification parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    /// Create a new notification
    #[must_use]
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            method: method.into(),
            params,
        }
    }

    /// Create a notification without parameters
    #[must_use]
    pub fn without_params(method: impl Into<String>) -> Self {
        Self::new(method, None)
    }
}

/// JSON-RPC error object
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    /// Create a new error
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create an error with additional data
    #[must_use]
    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    /// Create a parse error (-32700)
    #[must_use]
    pub fn parse_error() -> Self {
        Self::new(-32700, "Parse error")
    }

    /// Create an invalid request error (-32600)
    #[must_use]
    pub fn invalid_request() -> Self {
        Self::new(-32600, "Invalid Request")
    }

    /// Create a method not found error (-32601)
    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        Self::new(-32601, alloc::format!("Method not found: {}", method))
    }

    /// Create an invalid params error (-32602)
    #[must_use]
    pub fn invalid_params(details: &str) -> Self {
        Self::new(-32602, alloc::format!("Invalid params: {}", details))
    }

    /// Create an internal error (-32603)
    #[must_use]
    pub fn internal_error(details: &str) -> Self {
        Self::new(-32603, alloc::format!("Internal error: {}", details))
    }

    /// Get the error code
    #[must_use]
    pub const fn code(&self) -> i32 {
        self.code
    }

    /// Check if this is a parse error
    #[must_use]
    pub const fn is_parse_error(&self) -> bool {
        self.code == -32700
    }

    /// Check if this is an invalid request error
    #[must_use]
    pub const fn is_invalid_request(&self) -> bool {
        self.code == -32600
    }
}

impl fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

/// Response ID - handles the case where parse errors have null ID
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResponseId(pub Option<RequestId>);

impl ResponseId {
    /// Create a response ID from a request ID
    #[must_use]
    pub fn from_request(id: RequestId) -> Self {
        Self(Some(id))
    }

    /// Create a null response ID (for parse errors)
    #[must_use]
    pub fn null() -> Self {
        Self(None)
    }

    /// Get the request ID if present
    #[must_use]
    pub fn as_request_id(&self) -> Option<&RequestId> {
        self.0.as_ref()
    }

    /// Check if this is a null ID
    #[must_use]
    pub fn is_null(&self) -> bool {
        self.0.is_none()
    }
}

/// JSON-RPC response payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcResponsePayload {
    /// Successful response
    Success {
        /// Response result
        result: Value,
    },
    /// Error response
    Error {
        /// Response error
        error: JsonRpcError,
    },
}

/// JSON-RPC response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version
    pub jsonrpc: JsonRpcVersion,
    /// Response payload
    #[serde(flatten)]
    pub payload: JsonRpcResponsePayload,
    /// Response ID
    pub id: ResponseId,
}

impl JsonRpcResponse {
    /// Create a success response
    #[must_use]
    pub fn success(result: Value, id: RequestId) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            payload: JsonRpcResponsePayload::Success { result },
            id: ResponseId::from_request(id),
        }
    }

    /// Create an error response
    #[must_use]
    pub fn error_response(error: JsonRpcError, id: RequestId) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            payload: JsonRpcResponsePayload::Error { error },
            id: ResponseId::from_request(id),
        }
    }

    /// Create a parse error response (null ID)
    #[must_use]
    pub fn parse_error(message: Option<String>) -> Self {
        let error = JsonRpcError {
            code: -32700,
            message: message.unwrap_or_else(|| "Parse error".to_string()),
            data: None,
        };
        Self {
            jsonrpc: JsonRpcVersion,
            payload: JsonRpcResponsePayload::Error { error },
            id: ResponseId::null(),
        }
    }

    /// Check if this is a success response
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self.payload, JsonRpcResponsePayload::Success { .. })
    }

    /// Check if this is an error response
    #[must_use]
    pub fn is_error(&self) -> bool {
        matches!(self.payload, JsonRpcResponsePayload::Error { .. })
    }

    /// Get the result if success
    #[must_use]
    pub fn result(&self) -> Option<&Value> {
        match &self.payload {
            JsonRpcResponsePayload::Success { result } => Some(result),
            JsonRpcResponsePayload::Error { .. } => None,
        }
    }

    /// Get the error if error
    #[must_use]
    pub fn error(&self) -> Option<&JsonRpcError> {
        match &self.payload {
            JsonRpcResponsePayload::Success { .. } => None,
            JsonRpcResponsePayload::Error { error } => Some(error),
        }
    }
}

/// Standard JSON-RPC error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonRpcErrorCode {
    /// Parse error (-32700)
    ParseError,
    /// Invalid request (-32600)
    InvalidRequest,
    /// Method not found (-32601)
    MethodNotFound,
    /// Invalid params (-32602)
    InvalidParams,
    /// Internal error (-32603)
    InternalError,
    /// Application-defined error
    ApplicationError(i32),
}

impl JsonRpcErrorCode {
    /// Get the numeric code
    #[must_use]
    pub const fn code(&self) -> i32 {
        match self {
            Self::ParseError => -32700,
            Self::InvalidRequest => -32600,
            Self::MethodNotFound => -32601,
            Self::InvalidParams => -32602,
            Self::InternalError => -32603,
            Self::ApplicationError(code) => *code,
        }
    }

    /// Get the standard message
    #[must_use]
    pub const fn message(&self) -> &'static str {
        match self {
            Self::ParseError => "Parse error",
            Self::InvalidRequest => "Invalid Request",
            Self::MethodNotFound => "Method not found",
            Self::InvalidParams => "Invalid params",
            Self::InternalError => "Internal error",
            Self::ApplicationError(_) => "Application error",
        }
    }
}

impl fmt::Display for JsonRpcErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message(), self.code())
    }
}

impl From<i32> for JsonRpcErrorCode {
    fn from(code: i32) -> Self {
        match code {
            -32700 => Self::ParseError,
            -32600 => Self::InvalidRequest,
            -32601 => Self::MethodNotFound,
            -32602 => Self::InvalidParams,
            -32603 => Self::InternalError,
            other => Self::ApplicationError(other),
        }
    }
}

impl From<JsonRpcErrorCode> for JsonRpcError {
    fn from(code: JsonRpcErrorCode) -> Self {
        Self {
            code: code.code(),
            message: code.message().to_string(),
            data: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_creation() {
        let req = JsonRpcRequest::new("test", None, "id-1");
        assert_eq!(req.method, "test");
        assert!(req.params.is_none());
    }

    #[test]
    fn test_response_success() {
        let resp = JsonRpcResponse::success(serde_json::json!({"ok": true}), "id-1".into());
        assert!(resp.is_success());
        assert!(!resp.is_error());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(JsonRpcErrorCode::ParseError.code(), -32700);
        assert_eq!(JsonRpcErrorCode::from(-32601), JsonRpcErrorCode::MethodNotFound);
    }

    #[test]
    fn test_request_id_conversion() {
        let id1: RequestId = "test".into();
        assert!(matches!(id1, RequestId::String(_)));

        let id2: RequestId = 42i32.into();
        assert!(matches!(id2, RequestId::Number(42)));
    }
}

//! # JSON-RPC 2.0 Implementation
//!
//! This module provides a complete implementation of JSON-RPC 2.0 protocol
//! with support for batching, streaming, and MCP-specific extensions.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::fmt;

use crate::types::RequestId;

/// JSON-RPC version constant
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC version type
#[derive(Debug, Clone, PartialEq, Eq)]
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
            Err(serde::de::Error::custom(format!(
                "Invalid JSON-RPC version: expected '{JSONRPC_VERSION}', got '{version}'"
            )))
        }
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

/// JSON-RPC response payload - ensures mutual exclusion of result and error
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcResponsePayload {
    /// Successful response with result
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
    /// Response payload (either result or error, never both)
    #[serde(flatten)]
    pub payload: JsonRpcResponsePayload,
    /// Request identifier (required except for parse errors)
    pub id: ResponseId,
}

/// Response ID - handles the special case where parse errors have null ID
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ResponseId(pub Option<RequestId>);

impl ResponseId {
    /// Create a response ID for a normal response
    pub fn from_request(id: RequestId) -> Self {
        Self(Some(id))
    }

    /// Create a null response ID for parse errors
    pub fn null() -> Self {
        Self(None)
    }

    /// Get the request ID if present
    pub fn as_request_id(&self) -> Option<&RequestId> {
        self.0.as_ref()
    }

    /// Check if this is a null ID (parse error)
    pub fn is_null(&self) -> bool {
        self.0.is_none()
    }
}

/// JSON-RPC notification message (no response expected)
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
    /// Create a new JSON-RPC error
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create a new JSON-RPC error with additional data
    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }

    /// Create a parse error (-32700)
    pub fn parse_error() -> Self {
        Self::new(-32700, "Parse error")
    }

    /// Create a parse error with details
    pub fn parse_error_with_details(details: impl Into<String>) -> Self {
        Self::with_data(
            -32700,
            "Parse error",
            serde_json::json!({ "details": details.into() }),
        )
    }

    /// Create an invalid request error (-32600)
    pub fn invalid_request() -> Self {
        Self::new(-32600, "Invalid Request")
    }

    /// Create an invalid request error with reason
    pub fn invalid_request_with_reason(reason: impl Into<String>) -> Self {
        Self::with_data(
            -32600,
            "Invalid Request",
            serde_json::json!({ "reason": reason.into() }),
        )
    }

    /// Create a method not found error (-32601)
    pub fn method_not_found(method: &str) -> Self {
        Self::new(-32601, format!("Method not found: {method}"))
    }

    /// Create an invalid params error (-32602)
    pub fn invalid_params(details: &str) -> Self {
        Self::new(-32602, format!("Invalid params: {details}"))
    }

    /// Create an internal error (-32603)
    pub fn internal_error(details: &str) -> Self {
        Self::new(-32603, format!("Internal error: {details}"))
    }

    /// Check if this is a parse error
    pub fn is_parse_error(&self) -> bool {
        self.code == -32700
    }

    /// Check if this is an invalid request error
    pub fn is_invalid_request(&self) -> bool {
        self.code == -32600
    }

    /// Get the error code
    pub fn code(&self) -> i32 {
        self.code
    }
}

/// JSON-RPC batch request/response
///
/// **IMPORTANT**: JSON-RPC batching is NOT supported in MCP 2025-06-18 specification.
/// This type exists only for defensive deserialization and will return errors if used.
/// Per MCP spec changelog (PR #416), batch support was explicitly removed.
///
/// Do not use this type in new code. It will be removed in a future version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
#[deprecated(
    since = "2.2.3",
    note = "JSON-RPC batching removed from MCP 2025-06-18 spec (PR #416). This type exists only for defensive handling and will be removed."
)]
pub struct JsonRpcBatch<T> {
    /// Batch items
    pub items: Vec<T>,
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
    /// Get the numeric error code
    pub fn code(&self) -> i32 {
        match self {
            Self::ParseError => -32700,
            Self::InvalidRequest => -32600,
            Self::MethodNotFound => -32601,
            Self::InvalidParams => -32602,
            Self::InternalError => -32603,
            Self::ApplicationError(code) => *code,
        }
    }

    /// Get the standard error message
    pub fn message(&self) -> &'static str {
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

impl From<JsonRpcErrorCode> for JsonRpcError {
    fn from(code: JsonRpcErrorCode) -> Self {
        Self {
            code: code.code(),
            message: code.message().to_string(),
            data: None,
        }
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

/// JSON-RPC message type (union of request, response, notification)
///
/// **MCP 2025-06-18 Compliance Note:**
/// Batch variants exist only for defensive deserialization and are NOT supported
/// per MCP specification (PR #416 removed batch support). They will return errors if encountered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    /// Request message (MCP-compliant)
    Request(JsonRpcRequest),
    /// Response message (MCP-compliant)
    Response(JsonRpcResponse),
    /// Notification message (MCP-compliant)
    Notification(JsonRpcNotification),
    /// Batch of messages (NOT SUPPORTED - defensive deserialization only)
    ///
    /// **Deprecated**: MCP 2025-06-18 removed batch support.
    /// This variant exists only to return proper errors if batches are received.
    #[deprecated(since = "2.2.3", note = "Batching removed from MCP spec")]
    #[allow(deprecated)] // Internal use of deprecated batch type for defensive deserialization
    RequestBatch(JsonRpcBatch<JsonRpcRequest>),
    /// Batch of responses (NOT SUPPORTED - defensive deserialization only)
    ///
    /// **Deprecated**: MCP 2025-06-18 removed batch support.
    /// This variant exists only to return proper errors if batches are received.
    #[deprecated(since = "2.2.3", note = "Batching removed from MCP spec")]
    #[allow(deprecated)] // Internal use of deprecated batch type for defensive deserialization
    ResponseBatch(JsonRpcBatch<JsonRpcResponse>),
    /// Mixed batch (NOT SUPPORTED - defensive deserialization only)
    ///
    /// **Deprecated**: MCP 2025-06-18 removed batch support.
    /// This variant exists only to return proper errors if batches are received.
    #[deprecated(since = "2.2.3", note = "Batching removed from MCP spec")]
    #[allow(deprecated)] // Internal use of deprecated batch type for defensive deserialization
    MessageBatch(JsonRpcBatch<JsonRpcMessage>),
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC request
    pub fn new(method: String, params: Option<Value>, id: RequestId) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            method,
            params,
            id,
        }
    }

    /// Create a request with no parameters
    pub fn without_params(method: String, id: RequestId) -> Self {
        Self::new(method, None, id)
    }

    /// Create a request with parameters
    pub fn with_params<P: Serialize>(
        method: String,
        params: P,
        id: RequestId,
    ) -> Result<Self, serde_json::Error> {
        let params_value = serde_json::to_value(params)?;
        Ok(Self::new(method, Some(params_value), id))
    }
}

impl JsonRpcResponse {
    /// Create a successful response
    pub fn success(result: Value, id: RequestId) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            payload: JsonRpcResponsePayload::Success { result },
            id: ResponseId::from_request(id),
        }
    }

    /// Create an error response with request ID
    pub fn error_response(error: JsonRpcError, id: RequestId) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            payload: JsonRpcResponsePayload::Error { error },
            id: ResponseId::from_request(id),
        }
    }

    /// Create a parse error response (id is null)
    pub fn parse_error(message: Option<String>) -> Self {
        let error = JsonRpcError {
            code: JsonRpcErrorCode::ParseError.code(),
            message: message.unwrap_or_else(|| JsonRpcErrorCode::ParseError.message().to_string()),
            data: None,
        };
        Self {
            jsonrpc: JsonRpcVersion,
            payload: JsonRpcResponsePayload::Error { error },
            id: ResponseId::null(),
        }
    }

    /// Check if this is a successful response
    pub fn is_success(&self) -> bool {
        matches!(self.payload, JsonRpcResponsePayload::Success { .. })
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        matches!(self.payload, JsonRpcResponsePayload::Error { .. })
    }

    /// Get the result if this is a success response
    pub fn result(&self) -> Option<&Value> {
        match &self.payload {
            JsonRpcResponsePayload::Success { result } => Some(result),
            JsonRpcResponsePayload::Error { .. } => None,
        }
    }

    /// Get the error if this is an error response
    pub fn error(&self) -> Option<&JsonRpcError> {
        match &self.payload {
            JsonRpcResponsePayload::Success { .. } => None,
            JsonRpcResponsePayload::Error { error } => Some(error),
        }
    }

    /// Get the request ID if this is not a parse error
    pub fn request_id(&self) -> Option<&RequestId> {
        self.id.as_request_id()
    }

    /// Check if this response is for a parse error (has null ID)
    pub fn is_parse_error(&self) -> bool {
        self.id.is_null()
    }

    /// Get mutable reference to result if this is a success response
    pub fn result_mut(&mut self) -> Option<&mut Value> {
        match &mut self.payload {
            JsonRpcResponsePayload::Success { result } => Some(result),
            JsonRpcResponsePayload::Error { .. } => None,
        }
    }

    /// Get mutable reference to error if this is an error response
    pub fn error_mut(&mut self) -> Option<&mut JsonRpcError> {
        match &mut self.payload {
            JsonRpcResponsePayload::Success { .. } => None,
            JsonRpcResponsePayload::Error { error } => Some(error),
        }
    }

    /// Set the result for this response (converts to success response)
    pub fn set_result(&mut self, result: Value) {
        self.payload = JsonRpcResponsePayload::Success { result };
    }

    /// Set the error for this response (converts to error response)
    pub fn set_error(&mut self, error: JsonRpcError) {
        self.payload = JsonRpcResponsePayload::Error { error };
    }
}

impl JsonRpcNotification {
    /// Create a new JSON-RPC notification
    pub fn new(method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: JsonRpcVersion,
            method,
            params,
        }
    }

    /// Create a notification with no parameters
    pub fn without_params(method: String) -> Self {
        Self::new(method, None)
    }

    /// Create a notification with parameters
    pub fn with_params<P: Serialize>(method: String, params: P) -> Result<Self, serde_json::Error> {
        let params_value = serde_json::to_value(params)?;
        Ok(Self::new(method, Some(params_value)))
    }
}

// Allow deprecated warnings for internal implementation of deprecated batch types
// External users will still see deprecation warnings, but implementation won't spam warnings
#[allow(deprecated)]
impl<T> JsonRpcBatch<T> {
    /// Create a new batch
    pub fn new(items: Vec<T>) -> Self {
        Self { items }
    }

    /// Create an empty batch
    pub fn empty() -> Self {
        Self::new(Vec::new())
    }

    /// Add an item to the batch
    pub fn push(&mut self, item: T) {
        self.items.push(item);
    }

    /// Get the number of items in the batch
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if the batch is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterate over batch items
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}

#[allow(deprecated)]
impl<T> IntoIterator for JsonRpcBatch<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

#[allow(deprecated)]
impl<T> From<Vec<T>> for JsonRpcBatch<T> {
    fn from(items: Vec<T>) -> Self {
        Self::new(items)
    }
}

/// Utility functions for JSON-RPC message handling
pub mod utils {
    use super::*;

    /// Parse a JSON-RPC message from a string
    pub fn parse_message(json: &str) -> Result<JsonRpcMessage, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Serialize a JSON-RPC message to a string
    pub fn serialize_message(message: &JsonRpcMessage) -> Result<String, serde_json::Error> {
        serde_json::to_string(message)
    }

    /// Check if a string looks like a JSON-RPC batch
    pub fn is_batch(json: &str) -> bool {
        json.trim_start().starts_with('[')
    }

    /// Extract the method name from a JSON-RPC message string
    pub fn extract_method(json: &str) -> Option<String> {
        // Simple regex-free method extraction for performance
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(json)
            && let Some(method) = value.get("method")
        {
            return method.as_str().map(String::from);
        }
        None
    }
}

/// HTTP boundary types for lenient JSON-RPC parsing
///
/// These types are designed for parsing JSON-RPC messages at HTTP boundaries where
/// the input may not be strictly compliant. They accept any valid JSON structure
/// and can be converted to the canonical types after validation.
///
/// # Usage
///
/// ```rust
/// use turbomcp_protocol::jsonrpc::http::{HttpJsonRpcRequest, HttpJsonRpcResponse};
/// use turbomcp_protocol::jsonrpc::JsonRpcError;
///
/// // Parse lenient request
/// let raw_json = r#"{"jsonrpc":"2.0","method":"test","id":1}"#;
/// let request: HttpJsonRpcRequest = serde_json::from_str(raw_json).unwrap();
///
/// // Validate and use
/// if request.jsonrpc != "2.0" {
///     // Return error with the id we managed to extract
/// }
/// ```
pub mod http {
    use serde::{Deserialize, Serialize};
    use serde_json::Value;

    /// Lenient JSON-RPC request for HTTP boundary parsing
    ///
    /// This type accepts any string for `jsonrpc` and any JSON value for `id`,
    /// allowing proper error handling when clients send non-compliant requests.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct HttpJsonRpcRequest {
        /// JSON-RPC version (should be "2.0" but accepts any string for error handling)
        pub jsonrpc: String,
        /// Request ID (can be string, number, or null)
        #[serde(default)]
        pub id: Option<Value>,
        /// Method name
        pub method: String,
        /// Method parameters
        #[serde(default)]
        pub params: Option<Value>,
    }

    impl HttpJsonRpcRequest {
        /// Check if this is a valid JSON-RPC 2.0 request
        pub fn is_valid(&self) -> bool {
            self.jsonrpc == "2.0" && !self.method.is_empty()
        }

        /// Check if this is a notification (no id)
        pub fn is_notification(&self) -> bool {
            self.id.is_none()
        }

        /// Get the id as a string if it's a string, or convert number to string
        pub fn id_string(&self) -> Option<String> {
            self.id.as_ref().map(|v| match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => v.to_string(),
            })
        }
    }

    /// Lenient JSON-RPC response for HTTP boundary
    ///
    /// Uses separate result/error fields for compatibility with various JSON-RPC
    /// implementations.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct HttpJsonRpcResponse {
        /// JSON-RPC version
        pub jsonrpc: String,
        /// Response ID
        #[serde(default)]
        pub id: Option<Value>,
        /// Success result
        #[serde(skip_serializing_if = "Option::is_none")]
        pub result: Option<Value>,
        /// Error information
        #[serde(skip_serializing_if = "Option::is_none")]
        pub error: Option<super::JsonRpcError>,
    }

    impl HttpJsonRpcResponse {
        /// Create a success response
        pub fn success(id: Option<Value>, result: Value) -> Self {
            Self {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(result),
                error: None,
            }
        }

        /// Create an error response
        pub fn error(id: Option<Value>, error: super::JsonRpcError) -> Self {
            Self {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(error),
            }
        }

        /// Create an error response from error code
        pub fn error_from_code(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
            Self::error(id, super::JsonRpcError::new(code, message))
        }

        /// Create an invalid request error response
        pub fn invalid_request(id: Option<Value>, reason: impl Into<String>) -> Self {
            Self::error(id, super::JsonRpcError::invalid_request_with_reason(reason))
        }

        /// Create a parse error response (id is always null for parse errors)
        pub fn parse_error(details: Option<String>) -> Self {
            Self::error(
                None,
                details
                    .map(super::JsonRpcError::parse_error_with_details)
                    .unwrap_or_else(super::JsonRpcError::parse_error),
            )
        }

        /// Create an internal error response
        pub fn internal_error(id: Option<Value>, details: &str) -> Self {
            Self::error(id, super::JsonRpcError::internal_error(details))
        }

        /// Create a method not found error response
        pub fn method_not_found(id: Option<Value>, method: &str) -> Self {
            Self::error(id, super::JsonRpcError::method_not_found(method))
        }

        /// Check if this is an error response
        pub fn is_error(&self) -> bool {
            self.error.is_some()
        }

        /// Check if this is a success response
        pub fn is_success(&self) -> bool {
            self.result.is_some() && self.error.is_none()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_http_request_parsing() {
            let json = r#"{"jsonrpc":"2.0","method":"test","id":1,"params":{"key":"value"}}"#;
            let request: HttpJsonRpcRequest = serde_json::from_str(json).unwrap();
            assert!(request.is_valid());
            assert!(!request.is_notification());
            assert_eq!(request.method, "test");
        }

        #[test]
        fn test_http_request_invalid_version() {
            let json = r#"{"jsonrpc":"1.0","method":"test","id":1}"#;
            let request: HttpJsonRpcRequest = serde_json::from_str(json).unwrap();
            assert!(!request.is_valid());
        }

        #[test]
        fn test_http_response_success() {
            let response = HttpJsonRpcResponse::success(
                Some(Value::Number(1.into())),
                serde_json::json!({"result": "ok"}),
            );
            assert!(response.is_success());
            assert!(!response.is_error());
        }

        #[test]
        fn test_http_response_error() {
            let response = HttpJsonRpcResponse::invalid_request(
                Some(Value::String("req-1".into())),
                "jsonrpc must be 2.0",
            );
            assert!(!response.is_success());
            assert!(response.is_error());
        }

        #[test]
        fn test_http_response_serialization() {
            let response = HttpJsonRpcResponse::success(
                Some(Value::Number(1.into())),
                serde_json::json!({"data": "test"}),
            );
            let json = serde_json::to_string(&response).unwrap();
            assert!(json.contains(r#""jsonrpc":"2.0""#));
            assert!(json.contains(r#""result""#));
            assert!(!json.contains(r#""error""#));
        }
    }
}

#[cfg(test)]
#[allow(deprecated)] // Tests cover deprecated batch functionality for defensive deserialization
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_jsonrpc_version() {
        let version = JsonRpcVersion;
        let json = serde_json::to_string(&version).unwrap();
        assert_eq!(json, "\"2.0\"");

        let parsed: JsonRpcVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, version);
    }

    #[test]
    fn test_request_creation() {
        let request = JsonRpcRequest::new(
            "test_method".to_string(),
            Some(json!({"key": "value"})),
            RequestId::String("test-id".to_string()),
        );

        assert_eq!(request.method, "test_method");
        assert!(request.params.is_some());
    }

    #[test]
    fn test_response_creation() {
        let response = JsonRpcResponse::success(
            json!({"result": "success"}),
            RequestId::String("test-id".to_string()),
        );

        assert!(response.is_success());
        assert!(!response.is_error());
        assert!(response.result().is_some());
        assert!(response.error().is_none());
        assert!(!response.is_parse_error());
    }

    #[test]
    fn test_error_response() {
        let error = JsonRpcError::from(JsonRpcErrorCode::MethodNotFound);
        let response =
            JsonRpcResponse::error_response(error, RequestId::String("test-id".to_string()));

        assert!(!response.is_success());
        assert!(response.is_error());
        assert!(response.result().is_none());
        assert!(response.error().is_some());
        assert!(!response.is_parse_error());
    }

    #[test]
    fn test_parse_error_response() {
        let response = JsonRpcResponse::parse_error(Some("Invalid JSON".to_string()));

        assert!(!response.is_success());
        assert!(response.is_error());
        assert!(response.result().is_none());
        assert!(response.error().is_some());
        assert!(response.is_parse_error());
        assert!(response.request_id().is_none());

        // Verify the error details
        let error = response.error().unwrap();
        assert_eq!(error.code, JsonRpcErrorCode::ParseError.code());
        assert_eq!(error.message, "Invalid JSON");
    }

    #[test]
    fn test_notification() {
        let notification = JsonRpcNotification::without_params("test_notification".to_string());
        assert_eq!(notification.method, "test_notification");
        assert!(notification.params.is_none());
    }

    #[test]
    fn test_batch() {
        let mut batch = JsonRpcBatch::<JsonRpcRequest>::empty();
        assert!(batch.is_empty());

        batch.push(JsonRpcRequest::without_params(
            "method1".to_string(),
            RequestId::String("1".to_string()),
        ));
        batch.push(JsonRpcRequest::without_params(
            "method2".to_string(),
            RequestId::String("2".to_string()),
        ));

        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
    }

    #[test]
    fn test_serialization() {
        let request = JsonRpcRequest::new(
            "test_method".to_string(),
            Some(json!({"param": "value"})),
            RequestId::String("123".to_string()),
        );

        let json = serde_json::to_string(&request).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.method, request.method);
        assert_eq!(parsed.params, request.params);
    }

    #[test]
    fn test_utils() {
        let json = r#"{"jsonrpc":"2.0","method":"test","id":"123"}"#;

        assert!(!utils::is_batch(json));
        assert_eq!(utils::extract_method(json), Some("test".to_string()));

        let batch_json = r#"[{"jsonrpc":"2.0","method":"test","id":"123"}]"#;
        assert!(utils::is_batch(batch_json));
    }

    #[test]
    fn test_error_codes() {
        let parse_error = JsonRpcErrorCode::ParseError;
        assert_eq!(parse_error.code(), -32700);
        assert_eq!(parse_error.message(), "Parse error");

        let app_error = JsonRpcErrorCode::ApplicationError(-32001);
        assert_eq!(app_error.code(), -32001);
    }
}

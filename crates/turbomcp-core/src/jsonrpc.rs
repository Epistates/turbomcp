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

/// JSON-RPC response payload — enforces JSON-RPC 2.0 §5 mutual exclusion of
/// `result` and `error` on deserialize. `Serialize` keeps the untagged shape
/// so the wire format is unchanged.
#[derive(Debug, Clone, Serialize)]
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

impl<'de> Deserialize<'de> for JsonRpcResponsePayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut object = serde_json::Map::<String, Value>::deserialize(deserializer)?;
        let result_present = object.contains_key("result");
        let error_present = object.contains_key("error");

        match (result_present, error_present) {
            (true, false) => Ok(Self::Success {
                result: object.remove("result").unwrap_or(Value::Null),
            }),
            (false, true) => {
                let error_value = object.remove("error").unwrap_or(Value::Null);
                let error =
                    JsonRpcError::deserialize(error_value).map_err(serde::de::Error::custom)?;
                Ok(Self::Error { error })
            }
            (true, true) => Err(serde::de::Error::custom(
                "JSON-RPC response must contain exactly one of `result` or `error`, not both",
            )),
            (false, false) => Err(serde::de::Error::custom(
                "JSON-RPC response must contain exactly one of `result` or `error`",
            )),
        }
    }
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

// ============================================================================
// Wire Format Types - for router/transport use
// ============================================================================
// These types handle the practical case of deserializing incoming JSON-RPC
// messages where we don't know upfront if it's a request or notification.
// Request IDs are validated at this boundary so routers can rely on
// `id: None` meaning "field absent", not "null or malformed".

/// Incoming JSON-RPC message - can be request or notification.
///
/// This is the "wire format" type used by routers to parse incoming messages.
/// Unlike [`JsonRpcRequest`] which requires an ID, this type can deserialize
/// both requests (with id) and notifications (without id).
///
/// # Example
///
/// ```rust
/// use turbomcp_core::jsonrpc::JsonRpcIncoming;
///
/// // Parse a request
/// let request: JsonRpcIncoming = serde_json::from_str(
///     r#"{"jsonrpc": "2.0", "id": 1, "method": "ping"}"#
/// ).unwrap();
/// assert!(request.is_request());
///
/// // Parse a notification
/// let notification: JsonRpcIncoming = serde_json::from_str(
///     r#"{"jsonrpc": "2.0", "method": "notifications/initialized"}"#
/// ).unwrap();
/// assert!(notification.is_notification());
/// ```
#[derive(Debug, Clone)]
pub struct JsonRpcIncoming {
    /// JSON-RPC version. Required to be "2.0" per JSON-RPC 2.0 §4.
    /// Parsed by [`Self::parse`] and checked with [`Self::is_valid_version`]; raw
    /// deserialize accepts any string for diagnostic purposes (so callers can
    /// report a 1.0/missing-version error).
    pub jsonrpc: String,
    /// Request ID (None for notifications)
    pub id: Option<Value>,
    /// Method name
    pub method: String,
    /// Method parameters
    pub params: Option<Value>,
}

impl<'de> Deserialize<'de> for JsonRpcIncoming {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut object = serde_json::Map::<String, Value>::deserialize(deserializer)?;

        let jsonrpc = match object.remove("jsonrpc") {
            Some(Value::String(version)) => version,
            Some(_) => {
                return Err(serde::de::Error::custom(
                    "JSON-RPC request field `jsonrpc` must be a string",
                ));
            }
            None => return Err(serde::de::Error::missing_field("jsonrpc")),
        };

        let method = match object.remove("method") {
            Some(Value::String(method)) => method,
            Some(_) => {
                return Err(serde::de::Error::custom(
                    "JSON-RPC request field `method` must be a string",
                ));
            }
            None => return Err(serde::de::Error::missing_field("method")),
        };

        let id = match object.remove("id") {
            Some(value) => {
                Some(validate_incoming_request_id(value).map_err(serde::de::Error::custom)?)
            }
            None => None,
        };

        Ok(Self {
            jsonrpc,
            id,
            method,
            params: match object.remove("params") {
                Some(Value::Null) | None => None,
                Some(params) => Some(params),
            },
        })
    }
}

fn validate_incoming_request_id(value: Value) -> Result<Value, &'static str> {
    match &value {
        Value::String(_) => Ok(value),
        Value::Number(number) if number.as_i64().is_some() => Ok(value),
        Value::Null => Err("MCP request `id` must not be null"),
        Value::Number(_) => Err("MCP request `id` must be an integer"),
        _ => Err("MCP request `id` must be a string or integer"),
    }
}

impl JsonRpcIncoming {
    /// Check if this is a request (has an ID)
    #[must_use]
    pub fn is_request(&self) -> bool {
        self.id.is_some()
    }

    /// Check if this is a notification (no ID)
    #[must_use]
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }

    /// Parse from JSON string
    pub fn parse(input: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(input)
    }

    /// Validate the JSON-RPC version field is exactly `"2.0"` per JSON-RPC 2.0 §4.
    /// Callers should invoke this after `parse` to enforce strictness; the raw
    /// deserialize is intentionally lenient so the parse error path can produce
    /// a useful `-32600 Invalid Request` response with the offending value.
    #[must_use]
    pub fn is_valid_version(&self) -> bool {
        self.jsonrpc == "2.0"
    }
}

/// Outgoing JSON-RPC response - wire format for transport.
///
/// This is the "wire format" type used by routers to create responses.
/// It handles the case where notifications should not receive responses
/// (represented by having no id, result, or error).
///
/// # Example
///
/// ```rust
/// use turbomcp_core::jsonrpc::JsonRpcOutgoing;
///
/// // Create a success response
/// let response = JsonRpcOutgoing::success(
///     Some(serde_json::json!(1)),
///     serde_json::json!({"ok": true})
/// );
/// assert!(response.should_send());
///
/// // Create a notification response (should not be sent)
/// let no_response = JsonRpcOutgoing::notification_ack();
/// assert!(!no_response.should_send());
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcOutgoing {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request id, echoed from the originating request.
    ///
    /// Per JSON-RPC 2.0 §5.1, error responses for which the id cannot be
    /// determined (parse error, invalid request) **MUST** contain `id: null`.
    /// We therefore always serialize this field — `None` becomes `null`.
    /// Notifications never produce a `JsonRpcOutgoing` that reaches the wire
    /// (gated by [`Self::should_send`]).
    pub id: Option<Value>,
    /// Result (mutually exclusive with error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error (mutually exclusive with result)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcOutgoing {
    /// Create a success response
    #[must_use]
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response
    #[must_use]
    pub fn error(id: Option<Value>, error: impl Into<JsonRpcError>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error.into()),
        }
    }

    /// Create a notification acknowledgment (should not be sent over wire)
    #[must_use]
    pub fn notification_ack() -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            result: None,
            error: None,
        }
    }

    /// Check if this response should be sent over the wire.
    ///
    /// Per JSON-RPC 2.0, notifications (requests without id) should not
    /// receive responses. This method returns false for such cases.
    #[must_use]
    pub fn should_send(&self) -> bool {
        // Normal request/response messages have an id. Parse and invalid-request
        // errors may not have a readable id but still carry an error payload.
        self.id.is_some() || self.error.is_some()
    }

    /// Serialize to JSON string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Conversion from McpError to JsonRpcError.
///
/// `data` carries whatever the server author attached with
/// [`McpError::with_data`](crate::error::McpError::with_data) and nothing else:
/// the remaining context fields (`operation`, `component`, `source_location`)
/// stay server-side, because they describe TurboMCP internals rather than the
/// client's request.
impl From<crate::error::McpError> for JsonRpcError {
    fn from(err: crate::error::McpError) -> Self {
        Self {
            code: err.jsonrpc_code(),
            message: err.message.clone(),
            data: err.data().cloned(),
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
        assert_eq!(
            JsonRpcErrorCode::from(-32601),
            JsonRpcErrorCode::MethodNotFound
        );
    }

    #[test]
    fn test_request_id_conversion() {
        let id1: RequestId = "test".into();
        assert!(matches!(id1, RequestId::String(_)));

        let id2: RequestId = 42i32.into();
        assert!(matches!(id2, RequestId::Number(42)));
    }

    #[test]
    fn test_incoming_request() {
        let input = r#"{"jsonrpc": "2.0", "id": 1, "method": "ping"}"#;
        let incoming = JsonRpcIncoming::parse(input).unwrap();
        assert!(incoming.is_request());
        assert!(!incoming.is_notification());
        assert_eq!(incoming.method, "ping");
    }

    #[test]
    fn test_incoming_notification() {
        let input = r#"{"jsonrpc": "2.0", "method": "notifications/initialized"}"#;
        let incoming = JsonRpcIncoming::parse(input).unwrap();
        assert!(!incoming.is_request());
        assert!(incoming.is_notification());
    }

    #[test]
    fn test_incoming_rejects_null_id() {
        let input = r#"{"jsonrpc": "2.0", "id": null, "method": "ping"}"#;
        let err = JsonRpcIncoming::parse(input).unwrap_err();
        assert!(err.to_string().contains("must not be null"));
    }

    #[test]
    fn test_incoming_rejects_non_integer_number_id() {
        let input = r#"{"jsonrpc": "2.0", "id": 1.25, "method": "ping"}"#;
        let err = JsonRpcIncoming::parse(input).unwrap_err();
        assert!(err.to_string().contains("must be an integer"));
    }

    #[test]
    fn test_outgoing_success() {
        let response = JsonRpcOutgoing::success(Some(serde_json::json!(1)), serde_json::json!({}));
        assert!(response.should_send());
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_outgoing_success_without_id_is_not_sent() {
        let response = JsonRpcOutgoing::success(None, serde_json::json!({}));
        assert!(!response.should_send());
    }

    #[test]
    fn test_outgoing_notification_ack() {
        let response = JsonRpcOutgoing::notification_ack();
        assert!(!response.should_send());
    }

    /// JSON-RPC 2.0 §5: a response object MUST contain `result` xor `error`.
    /// Both-present and neither-present payloads must be rejected.
    #[test]
    fn test_response_payload_rejects_both_result_and_error() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"result":{},"error":{"code":-32603,"message":"x"}}"#;
        let err = serde_json::from_str::<JsonRpcResponse>(raw)
            .expect_err("response with both result and error must be rejected");
        assert!(err.to_string().contains("exactly one of"));
    }

    #[test]
    fn test_response_payload_rejects_neither_result_nor_error() {
        let raw = r#"{"jsonrpc":"2.0","id":1}"#;
        let err = serde_json::from_str::<JsonRpcResponse>(raw)
            .expect_err("response with neither result nor error must be rejected");
        assert!(err.to_string().contains("exactly one of"));
    }

    #[test]
    fn test_response_payload_accepts_error_only() {
        let raw =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(raw).unwrap();
        assert!(resp.is_error());
        assert_eq!(resp.error().unwrap().code, -32601);
    }

    #[test]
    fn test_response_payload_accepts_null_result() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
        let resp: JsonRpcResponse = serde_json::from_str(raw).unwrap();
        assert!(resp.is_success());
        assert_eq!(resp.result(), Some(&Value::Null));
    }
}

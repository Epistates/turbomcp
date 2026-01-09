//! MCP request/response types for Tower services.

use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use turbomcp_protocol::jsonrpc::JsonRpcRequest;

/// MCP request wrapper for Tower services.
///
/// Wraps a JSON-RPC request with metadata that middleware can read and modify.
#[derive(Debug, Clone)]
pub struct McpRequest {
    /// The underlying JSON-RPC request
    pub request: JsonRpcRequest,
    /// Request metadata (readable/writable by middleware)
    pub metadata: HashMap<String, Value>,
    /// Request creation timestamp
    pub timestamp: Instant,
}

impl McpRequest {
    /// Create a new MCP request.
    #[must_use]
    pub fn new(request: JsonRpcRequest) -> Self {
        Self {
            request,
            metadata: HashMap::new(),
            timestamp: Instant::now(),
        }
    }

    /// Create a request with initial metadata.
    #[must_use]
    pub fn with_metadata(request: JsonRpcRequest, metadata: HashMap<String, Value>) -> Self {
        Self {
            request,
            metadata,
            timestamp: Instant::now(),
        }
    }

    /// Get the request method name.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.request.method
    }

    /// Get request parameters.
    #[must_use]
    pub fn params(&self) -> Option<&Value> {
        self.request.params.as_ref()
    }

    /// Get the request ID.
    #[must_use]
    pub fn id(&self) -> &turbomcp_protocol::MessageId {
        &self.request.id
    }

    /// Insert metadata.
    pub fn insert_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }

    /// Get metadata value.
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    /// Check if metadata key exists.
    #[must_use]
    pub fn has_metadata(&self, key: &str) -> bool {
        self.metadata.contains_key(key)
    }

    /// Time elapsed since request creation.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.timestamp.elapsed()
    }
}

impl From<JsonRpcRequest> for McpRequest {
    fn from(request: JsonRpcRequest) -> Self {
        Self::new(request)
    }
}

/// MCP response wrapper for Tower services.
///
/// Wraps a response with metadata and timing information.
#[derive(Debug, Clone)]
pub struct McpResponse {
    /// The response data (if successful)
    pub result: Option<Value>,
    /// Error information (if failed)
    pub error: Option<turbomcp_protocol::Error>,
    /// Response metadata
    pub metadata: HashMap<String, Value>,
    /// Request-to-response duration
    pub duration: Duration,
}

impl McpResponse {
    /// Create a successful response.
    #[must_use]
    pub fn success(result: Value, duration: Duration) -> Self {
        Self {
            result: Some(result),
            error: None,
            metadata: HashMap::new(),
            duration,
        }
    }

    /// Create an error response.
    #[must_use]
    pub fn error(error: turbomcp_protocol::Error, duration: Duration) -> Self {
        Self {
            result: None,
            error: Some(error),
            metadata: HashMap::new(),
            duration,
        }
    }

    /// Check if the response indicates success.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Check if the response indicates an error.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Insert metadata.
    pub fn insert_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }

    /// Get metadata value.
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    /// Take the result value, leaving None.
    pub fn take_result(&mut self) -> Option<Value> {
        self.result.take()
    }

    /// Take the error, leaving None.
    pub fn take_error(&mut self) -> Option<turbomcp_protocol::Error> {
        self.error.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_protocol::MessageId;
    use turbomcp_protocol::jsonrpc::JsonRpcVersion;

    fn test_request() -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test-1"),
            method: "test/method".to_string(),
            params: Some(json!({"key": "value"})),
        }
    }

    #[test]
    fn test_mcp_request_creation() {
        let req = McpRequest::new(test_request());
        assert_eq!(req.method(), "test/method");
        assert_eq!(req.params(), Some(&json!({"key": "value"})));
        assert!(req.metadata.is_empty());
    }

    #[test]
    fn test_mcp_request_metadata() {
        let mut req = McpRequest::new(test_request());
        req.insert_metadata("user_id", json!("user123"));

        assert!(req.has_metadata("user_id"));
        assert_eq!(req.get_metadata("user_id"), Some(&json!("user123")));
        assert!(!req.has_metadata("nonexistent"));
    }

    #[test]
    fn test_mcp_response_success() {
        let resp = McpResponse::success(json!({"result": "ok"}), Duration::from_millis(100));
        assert!(resp.is_success());
        assert!(!resp.is_error());
        assert_eq!(resp.result, Some(json!({"result": "ok"})));
        assert_eq!(resp.duration, Duration::from_millis(100));
    }

    #[test]
    fn test_mcp_response_error() {
        let err = turbomcp_protocol::Error::internal("Test error");
        let resp = McpResponse::error(err, Duration::from_millis(50));
        assert!(!resp.is_success());
        assert!(resp.is_error());
        assert!(resp.error.is_some());
    }
}

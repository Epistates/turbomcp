//! Zero-copy message types using rkyv serialization
//!
//! This module provides internal message types optimized for zero-copy
//! deserialization using rkyv. These types are used for internal message
//! passing between components while maintaining JSON compatibility for
//! wire format.
//!
//! # Design Philosophy
//!
//! - **Wire format**: JSON (for MCP protocol compliance)
//! - **Internal format**: rkyv (for zero-copy performance)
//! - **Conversion**: JSON bytes stored in AlignedVec, parsed on-demand
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_core::rkyv_types::{InternalMessage, InternalId};
//!
//! // Create an internal message from JSON-RPC
//! let msg = InternalMessage::new()
//!     .with_id(InternalId::Number(1))
//!     .with_method("tools/call")
//!     .with_params_json(r#"{"name": "hello"}"#.as_bytes());
//!
//! // Serialize to bytes (zero-copy ready)
//! let bytes = rkyv::to_bytes::<rancor::Error>(&msg).unwrap();
//!
//! // Access archived data without deserialization
//! let archived = rkyv::access::<ArchivedInternalMessage, rancor::Error>(&bytes).unwrap();
//! assert_eq!(archived.method.as_str(), "tools/call");
//! ```

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use rkyv::{Archive, Deserialize, Serialize};

/// JSON-RPC request/notification ID for internal use
///
/// Matches the JSON-RPC 2.0 spec where ID can be number, string, or null.
#[derive(Archive, Deserialize, Serialize, Debug, Clone, PartialEq)]
#[rkyv(compare(PartialEq))]
#[rkyv(derive(Debug))]
pub enum InternalId {
    /// Numeric ID (most common)
    Number(i64),
    /// String ID
    String(String),
}

impl InternalId {
    /// Create a numeric ID
    #[must_use]
    pub fn number(n: i64) -> Self {
        Self::Number(n)
    }

    /// Create a string ID
    #[must_use]
    pub fn string(s: impl Into<String>) -> Self {
        Self::String(s.into())
    }
}

impl ArchivedInternalId {
    /// Check if this is a numeric ID
    #[must_use]
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Number(_))
    }

    /// Check if this is a string ID
    #[must_use]
    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    /// Get the numeric value if this is a number ID
    #[must_use]
    pub fn as_number(&self) -> Option<i64> {
        match self {
            Self::Number(n) => Some((*n).into()),
            Self::String(_) => None,
        }
    }
}

/// Internal MCP message for zero-copy routing
///
/// This type stores the raw JSON params as bytes, enabling zero-copy
/// access to the message structure while deferring JSON parsing until
/// the params are actually needed.
#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct InternalMessage {
    /// JSON-RPC request ID (None for notifications)
    pub id: Option<InternalId>,
    /// MCP method name (e.g., "tools/call", "resources/read")
    pub method: String,
    /// Raw JSON params bytes (parsed on-demand)
    pub params_raw: Vec<u8>,
    /// Session ID for routing
    pub session_id: Option<String>,
    /// Request correlation ID for tracing
    pub correlation_id: Option<String>,
}

impl Default for InternalMessage {
    fn default() -> Self {
        Self::new()
    }
}

impl InternalMessage {
    /// Create a new empty internal message
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: None,
            method: String::new(),
            params_raw: Vec::new(),
            session_id: None,
            correlation_id: None,
        }
    }

    /// Set the message ID
    #[must_use]
    pub fn with_id(mut self, id: InternalId) -> Self {
        self.id = Some(id);
        self
    }

    /// Set the method name
    #[must_use]
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.method = method.into();
        self
    }

    /// Set raw JSON params bytes
    #[must_use]
    pub fn with_params_raw(mut self, params: Vec<u8>) -> Self {
        self.params_raw = params;
        self
    }

    /// Set params from JSON bytes (convenience method)
    #[must_use]
    pub fn with_params_json(self, json: &[u8]) -> Self {
        self.with_params_raw(json.to_vec())
    }

    /// Set the session ID
    #[must_use]
    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Set the correlation ID
    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// Check if this is a notification (no ID)
    #[must_use]
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }

    /// Check if this is a request (has ID)
    #[must_use]
    pub fn is_request(&self) -> bool {
        self.id.is_some()
    }
}

impl ArchivedInternalMessage {
    /// Check if this is a notification (no ID)
    #[must_use]
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }

    /// Check if this is a request (has ID)
    #[must_use]
    pub fn is_request(&self) -> bool {
        self.id.is_some()
    }

    /// Get the method name
    #[must_use]
    pub fn method_str(&self) -> &str {
        &self.method
    }

    /// Get the raw params bytes
    #[must_use]
    pub fn params_bytes(&self) -> &[u8] {
        &self.params_raw
    }
}

/// Internal MCP response for zero-copy routing
#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct InternalResponse {
    /// JSON-RPC request ID this responds to
    pub id: InternalId,
    /// Raw JSON result bytes (for success)
    pub result_raw: Option<Vec<u8>>,
    /// Error information (for failure)
    pub error: Option<InternalError>,
    /// Request correlation ID for tracing
    pub correlation_id: Option<String>,
}

impl InternalResponse {
    /// Create a successful response
    #[must_use]
    pub fn success(id: InternalId, result: Vec<u8>) -> Self {
        Self {
            id,
            result_raw: Some(result),
            error: None,
            correlation_id: None,
        }
    }

    /// Create an error response
    #[must_use]
    pub fn error(id: InternalId, error: InternalError) -> Self {
        Self {
            id,
            result_raw: None,
            error: Some(error),
            correlation_id: None,
        }
    }

    /// Set the correlation ID
    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// Check if this is a success response
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.result_raw.is_some()
    }

    /// Check if this is an error response
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// Internal error representation
#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct InternalError {
    /// JSON-RPC error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Raw JSON error data (optional)
    pub data_raw: Option<Vec<u8>>,
}

impl InternalError {
    /// Create a new internal error
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data_raw: None,
        }
    }

    /// Set error data
    #[must_use]
    pub fn with_data(mut self, data: Vec<u8>) -> Self {
        self.data_raw = Some(data);
        self
    }

    // Standard JSON-RPC error codes
    /// Parse error (-32700)
    pub const PARSE_ERROR: i32 = -32700;
    /// Invalid request (-32600)
    pub const INVALID_REQUEST: i32 = -32600;
    /// Method not found (-32601)
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// Invalid params (-32602)
    pub const INVALID_PARAMS: i32 = -32602;
    /// Internal error (-32603)
    pub const INTERNAL_ERROR: i32 = -32603;
}

/// Routing hints extracted from message for efficient dispatch
#[derive(Archive, Deserialize, Serialize, Debug, Clone, Default)]
#[rkyv(derive(Debug))]
pub struct RoutingHints {
    /// Tool name for tools/call
    pub tool_name: Option<String>,
    /// Resource URI for resources/read
    pub resource_uri: Option<String>,
    /// Prompt name for prompts/get
    pub prompt_name: Option<String>,
    /// Subscription ID for notifications
    pub subscription_id: Option<String>,
}

impl RoutingHints {
    /// Create empty routing hints
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set tool name
    #[must_use]
    pub fn with_tool_name(mut self, name: impl Into<String>) -> Self {
        self.tool_name = Some(name.into());
        self
    }

    /// Set resource URI
    #[must_use]
    pub fn with_resource_uri(mut self, uri: impl Into<String>) -> Self {
        self.resource_uri = Some(uri.into());
        self
    }

    /// Set prompt name
    #[must_use]
    pub fn with_prompt_name(mut self, name: impl Into<String>) -> Self {
        self.prompt_name = Some(name.into());
        self
    }

    /// Check if any hints are present
    #[must_use]
    pub fn has_hints(&self) -> bool {
        self.tool_name.is_some()
            || self.resource_uri.is_some()
            || self.prompt_name.is_some()
            || self.subscription_id.is_some()
    }
}

/// Batch of internal messages for efficient processing
#[derive(Archive, Deserialize, Serialize, Debug, Clone)]
#[rkyv(derive(Debug))]
pub struct InternalBatch {
    /// Messages in the batch
    pub messages: Vec<InternalMessage>,
    /// Batch-level correlation ID
    pub batch_id: Option<String>,
}

impl InternalBatch {
    /// Create a new batch
    #[must_use]
    pub fn new(messages: Vec<InternalMessage>) -> Self {
        Self {
            messages,
            batch_id: None,
        }
    }

    /// Set batch ID
    #[must_use]
    pub fn with_batch_id(mut self, id: impl Into<String>) -> Self {
        self.batch_id = Some(id.into());
        self
    }

    /// Get the number of messages in the batch
    #[must_use]
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Check if batch is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internal_id() {
        let num_id = InternalId::number(42);
        let str_id = InternalId::string("req-123");

        assert_eq!(num_id, InternalId::Number(42));
        assert_eq!(str_id, InternalId::String("req-123".into()));
    }

    #[test]
    fn test_internal_message_builder() {
        let msg = InternalMessage::new()
            .with_id(InternalId::number(1))
            .with_method("tools/call")
            .with_params_json(br#"{"name":"hello"}"#)
            .with_session_id("sess-123")
            .with_correlation_id("corr-456");

        assert!(msg.is_request());
        assert!(!msg.is_notification());
        assert_eq!(msg.method, "tools/call");
        assert_eq!(msg.session_id, Some("sess-123".into()));
        assert_eq!(msg.correlation_id, Some("corr-456".into()));
    }

    #[test]
    fn test_internal_message_notification() {
        let msg = InternalMessage::new().with_method("notifications/progress");

        assert!(msg.is_notification());
        assert!(!msg.is_request());
    }

    #[test]
    fn test_internal_response_success() {
        let resp =
            InternalResponse::success(InternalId::number(1), br#"{"content":"Hello!"}"#.to_vec());

        assert!(resp.is_success());
        assert!(!resp.is_error());
    }

    #[test]
    fn test_internal_response_error() {
        let resp = InternalResponse::error(
            InternalId::number(1),
            InternalError::new(InternalError::METHOD_NOT_FOUND, "Unknown method"),
        );

        assert!(!resp.is_success());
        assert!(resp.is_error());
    }

    #[test]
    fn test_internal_error_codes() {
        assert_eq!(InternalError::PARSE_ERROR, -32700);
        assert_eq!(InternalError::INVALID_REQUEST, -32600);
        assert_eq!(InternalError::METHOD_NOT_FOUND, -32601);
        assert_eq!(InternalError::INVALID_PARAMS, -32602);
        assert_eq!(InternalError::INTERNAL_ERROR, -32603);
    }

    #[test]
    fn test_routing_hints() {
        let hints = RoutingHints::new()
            .with_tool_name("calculator")
            .with_resource_uri("file:///test.txt");

        assert!(hints.has_hints());
        assert_eq!(hints.tool_name, Some("calculator".into()));
        assert_eq!(hints.resource_uri, Some("file:///test.txt".into()));
    }

    #[test]
    fn test_internal_batch() {
        let msg1 = InternalMessage::new()
            .with_id(InternalId::number(1))
            .with_method("tools/list");
        let msg2 = InternalMessage::new()
            .with_id(InternalId::number(2))
            .with_method("resources/list");

        let batch = InternalBatch::new(vec![msg1, msg2]).with_batch_id("batch-1");

        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
        assert_eq!(batch.batch_id, Some("batch-1".into()));
    }

    #[test]
    fn test_rkyv_roundtrip() {
        let msg = InternalMessage::new()
            .with_id(InternalId::number(42))
            .with_method("tools/call")
            .with_params_json(br#"{"name":"test","args":{}}"#);

        // Serialize to bytes
        let bytes = rkyv::to_bytes::<rancor::Error>(&msg).expect("serialization failed");

        // Access archived data (zero-copy)
        let archived =
            rkyv::access::<ArchivedInternalMessage, rancor::Error>(&bytes).expect("access failed");

        assert_eq!(archived.method_str(), "tools/call");
        assert!(archived.is_request());

        // Full deserialization if needed
        let deserialized: InternalMessage =
            rkyv::deserialize::<InternalMessage, rancor::Error>(archived)
                .expect("deserialization failed");

        assert_eq!(deserialized.method, "tools/call");
        assert_eq!(deserialized.id, Some(InternalId::Number(42)));
    }

    #[test]
    fn test_rkyv_response_roundtrip() {
        let resp = InternalResponse::success(
            InternalId::string("req-abc"),
            br#"{"result":"ok"}"#.to_vec(),
        )
        .with_correlation_id("corr-123");

        let bytes = rkyv::to_bytes::<rancor::Error>(&resp).expect("serialization failed");

        let archived =
            rkyv::access::<ArchivedInternalResponse, rancor::Error>(&bytes).expect("access failed");

        assert!(archived.result_raw.is_some());
        assert!(archived.error.is_none());
    }

    #[test]
    fn test_rkyv_batch_roundtrip() {
        let batch = InternalBatch::new(vec![
            InternalMessage::new()
                .with_id(InternalId::number(1))
                .with_method("a"),
            InternalMessage::new()
                .with_id(InternalId::number(2))
                .with_method("b"),
        ]);

        let bytes = rkyv::to_bytes::<rancor::Error>(&batch).expect("serialization failed");

        let archived =
            rkyv::access::<ArchivedInternalBatch, rancor::Error>(&bytes).expect("access failed");

        assert_eq!(archived.messages.len(), 2);
    }
}

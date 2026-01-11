//! Transport trait and error types for WASI MCP client

use core::fmt;
use serde::{Serialize, de::DeserializeOwned};

/// Error type for transport operations
#[derive(Debug)]
pub enum TransportError {
    /// I/O error during read/write
    Io(String),
    /// JSON serialization/deserialization error
    Json(String),
    /// HTTP error with status code and message
    Http {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },
    /// Connection error
    Connection(String),
    /// Timeout error
    Timeout,
    /// Protocol error (invalid JSON-RPC response)
    Protocol(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::Json(msg) => write!(f, "JSON error: {msg}"),
            Self::Http { status, message } => write!(f, "HTTP {status}: {message}"),
            Self::Connection(msg) => write!(f, "Connection error: {msg}"),
            Self::Timeout => write!(f, "Operation timed out"),
            Self::Protocol(msg) => write!(f, "Protocol error: {msg}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<serde_json::Error> for TransportError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err.to_string())
    }
}

/// Transport trait for MCP communication
///
/// Implementations handle the low-level communication with MCP servers,
/// whether via STDIO, HTTP, or other protocols.
pub trait Transport {
    /// Send a JSON-RPC request and receive a response
    ///
    /// # Arguments
    ///
    /// * `method` - The JSON-RPC method name
    /// * `params` - Optional parameters for the request
    ///
    /// # Returns
    ///
    /// The deserialized response, or an error
    fn request<P, R>(&self, method: &str, params: Option<P>) -> Result<R, TransportError>
    where
        P: Serialize,
        R: DeserializeOwned;

    /// Send a JSON-RPC notification (no response expected)
    ///
    /// # Arguments
    ///
    /// * `method` - The JSON-RPC method name
    /// * `params` - Optional parameters for the notification
    fn notify<P>(&self, method: &str, params: Option<P>) -> Result<(), TransportError>
    where
        P: Serialize;

    /// Check if the transport is connected/ready
    fn is_ready(&self) -> bool;

    /// Close the transport connection
    fn close(&self) -> Result<(), TransportError>;
}

/// JSON-RPC 2.0 request structure
#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcRequest<P> {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<P>,
}

impl<P> JsonRpcRequest<P> {
    pub fn new(id: u64, method: impl Into<String>, params: Option<P>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 notification structure (no id)
#[derive(Debug, Serialize)]
pub(crate) struct JsonRpcNotification<P> {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<P>,
}

impl<P> JsonRpcNotification<P> {
    pub fn new(method: impl Into<String>, params: Option<P>) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 response structure
#[derive(Debug, serde::Deserialize)]
pub(crate) struct JsonRpcResponse<R> {
    #[allow(dead_code)]
    pub jsonrpc: String,
    #[allow(dead_code)]
    pub id: Option<u64>,
    pub result: Option<R>,
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error structure
#[derive(Debug, serde::Deserialize)]
pub(crate) struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[allow(dead_code)]
    pub data: Option<serde_json::Value>,
}

impl<R> JsonRpcResponse<R> {
    /// Extract the result or convert error to TransportError
    pub fn into_result(self) -> Result<R, TransportError> {
        if let Some(error) = self.error {
            return Err(TransportError::Protocol(format!(
                "JSON-RPC error {}: {}",
                error.code, error.message
            )));
        }

        self.result
            .ok_or_else(|| TransportError::Protocol("Missing result in response".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_error_display() {
        let err = TransportError::Io("read failed".into());
        assert_eq!(err.to_string(), "I/O error: read failed");

        let err = TransportError::Http {
            status: 404,
            message: "Not Found".into(),
        };
        assert_eq!(err.to_string(), "HTTP 404: Not Found");

        let err = TransportError::Timeout;
        assert_eq!(err.to_string(), "Operation timed out");
    }

    #[test]
    fn test_jsonrpc_request_serialization() {
        let req = JsonRpcRequest::new(1, "test/method", Some(serde_json::json!({"key": "value"})));
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"test/method\""));
    }

    #[test]
    fn test_jsonrpc_notification_serialization() {
        let notif: JsonRpcNotification<()> = JsonRpcNotification::new("test/notify", None);
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(!json.contains("\"id\""));
        assert!(json.contains("\"method\":\"test/notify\""));
    }
}

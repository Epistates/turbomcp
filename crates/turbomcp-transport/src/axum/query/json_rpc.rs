//! JSON-RPC request and response types
//!
//! This module defines the JSON-RPC 2.0 protocol structures used
//! for MCP communication over HTTP endpoints.

use serde::{Deserialize, Serialize};

/// JSON-RPC request payload
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (should be "2.0")
    pub jsonrpc: String,
    /// Request ID for correlation
    pub id: Option<serde_json::Value>,
    /// Method name to call
    pub method: String,
    /// Method parameters
    pub params: Option<serde_json::Value>,
}

/// JSON-RPC response payload
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version
    pub jsonrpc: String,
    /// Request ID for correlation
    pub id: Option<serde_json::Value>,
    /// Success result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Error information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error object
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// Create new JSON-RPC request
    pub fn new(method: String, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::Value::Number(serde_json::Number::from(1))),
            method,
            params,
        }
    }

    /// Create new JSON-RPC notification (no ID)
    pub fn notification(method: String, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            method,
            params,
        }
    }

    /// Check if this is a notification (no ID)
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

impl JsonRpcResponse {
    /// Create success response
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create error response
    pub fn error(id: Option<serde_json::Value>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

impl JsonRpcError {
    /// Parse error (-32700)
    pub fn parse_error(data: Option<serde_json::Value>) -> Self {
        Self {
            code: -32700,
            message: "Parse error".to_string(),
            data,
        }
    }

    /// Invalid request (-32600)
    pub fn invalid_request(data: Option<serde_json::Value>) -> Self {
        Self {
            code: -32600,
            message: "Invalid Request".to_string(),
            data,
        }
    }

    /// Method not found (-32601)
    pub fn method_not_found(data: Option<serde_json::Value>) -> Self {
        Self {
            code: -32601,
            message: "Method not found".to_string(),
            data,
        }
    }

    /// Invalid params (-32602)
    pub fn invalid_params(data: Option<serde_json::Value>) -> Self {
        Self {
            code: -32602,
            message: "Invalid params".to_string(),
            data,
        }
    }

    /// Internal error (-32603)
    pub fn internal_error(data: Option<serde_json::Value>) -> Self {
        Self {
            code: -32603,
            message: "Internal error".to_string(),
            data,
        }
    }
}

//! JSON-RPC protocol types for HTTP transport
//!
//! This module provides the JSON-RPC 2.0 protocol types used for
//! MCP communication over HTTP.

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
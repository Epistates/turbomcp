//! Zero-copy bridge between JSON-RPC and rkyv internal types
//!
//! This module provides conversion utilities for efficient internal message
//! passing using rkyv while maintaining JSON-RPC compatibility for wire format.
//!
//! # Architecture
//!
//! ```text
//! Wire (JSON) <---> JsonRpc* types <---> Internal* types (rkyv) <---> Handlers
//! ```
//!
//! - **Wire format**: JSON-RPC 2.0 (for MCP protocol compliance)
//! - **Internal format**: rkyv (for zero-copy internal routing)
//! - **Conversion**: Lazy - JSON bytes stored raw, parsed only when needed
//!
//! # Performance Benefits
//!
//! - Zero allocations when routing messages internally
//! - Shared buffer across message chain (no per-message copies)
//! - Method routing without full deserialization
//! - Deferred JSON parsing until handler execution
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_protocol::rkyv_bridge::{to_internal, from_internal};
//! use turbomcp_protocol::JsonRpcRequest;
//!
//! // Convert incoming JSON-RPC to internal format
//! let internal = to_internal(&json_rpc_request)?;
//!
//! // Serialize to rkyv bytes for zero-copy routing
//! let bytes = rkyv::to_bytes::<rancor::Error>(&internal)?;
//!
//! // Access without deserializing (zero-copy)
//! let archived = rkyv::access::<ArchivedInternalMessage, _>(&bytes)?;
//! println!("Method: {}", archived.method_str());
//!
//! // Convert back to JSON-RPC for response
//! let response = from_internal_response(&internal_response)?;
//! ```

use crate::McpError;
use crate::jsonrpc::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, JsonRpcResponsePayload};
use crate::types::RequestId;
use turbomcp_core::rkyv_types::{
    InternalError, InternalId, InternalMessage, InternalResponse, RoutingHints,
};

/// Convert a JSON-RPC request to an internal message for zero-copy routing
///
/// This converts the request ID and method, but stores params as raw JSON bytes
/// to enable zero-copy access during routing.
pub fn to_internal(request: &JsonRpcRequest) -> Result<InternalMessage, McpError> {
    let id = match &request.id {
        RequestId::Number(n) => InternalId::Number(*n),
        RequestId::String(s) => InternalId::String(s.clone()),
        RequestId::Uuid(u) => InternalId::String(u.to_string()),
    };

    let params_raw = if let Some(ref params) = request.params {
        serde_json::to_vec(params)
            .map_err(|e| McpError::serialization(format!("Failed to serialize params: {e}")))?
    } else {
        Vec::new()
    };

    Ok(InternalMessage::new()
        .with_id(id)
        .with_method(&request.method)
        .with_params_raw(params_raw))
}

/// Convert a JSON-RPC request to an internal message with routing hints
///
/// Extracts routing hints (tool name, resource URI, etc.) for efficient dispatch.
pub fn to_internal_with_hints(
    request: &JsonRpcRequest,
) -> Result<(InternalMessage, RoutingHints), McpError> {
    let msg = to_internal(request)?;
    let hints = extract_routing_hints(&request.method, request.params.as_ref())?;
    Ok((msg, hints))
}

/// Extract routing hints from request params
fn extract_routing_hints(
    method: &str,
    params: Option<&serde_json::Value>,
) -> Result<RoutingHints, McpError> {
    let mut hints = RoutingHints::new();

    if let Some(params) = params {
        match method {
            "tools/call" => {
                if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                    hints = hints.with_tool_name(name);
                }
            }
            "resources/read" => {
                if let Some(uri) = params.get("uri").and_then(|v| v.as_str()) {
                    hints = hints.with_resource_uri(uri);
                }
            }
            "prompts/get" => {
                if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                    hints = hints.with_prompt_name(name);
                }
            }
            _ => {}
        }
    }

    Ok(hints)
}

/// Convert an internal response back to JSON-RPC format
pub fn from_internal_response(response: &InternalResponse) -> Result<JsonRpcResponse, McpError> {
    let id = match &response.id {
        InternalId::Number(n) => RequestId::Number(*n),
        InternalId::String(s) => RequestId::String(s.clone()),
    };

    let payload = if let Some(ref result_raw) = response.result_raw {
        let result: serde_json::Value = serde_json::from_slice(result_raw)
            .map_err(|e| McpError::serialization(format!("Failed to parse result: {e}")))?;
        JsonRpcResponsePayload::Success { result }
    } else if let Some(ref error) = response.error {
        JsonRpcResponsePayload::Error {
            error: JsonRpcError {
                code: error.code,
                message: error.message.clone(),
                data: error
                    .data_raw
                    .as_ref()
                    .and_then(|raw| serde_json::from_slice(raw).ok()),
            },
        }
    } else {
        // Neither result nor error - treat as null result
        JsonRpcResponsePayload::Success {
            result: serde_json::Value::Null,
        }
    };

    Ok(JsonRpcResponse {
        jsonrpc: crate::jsonrpc::JsonRpcVersion,
        payload,
        id: crate::jsonrpc::ResponseId::from_request(id),
    })
}

/// Convert an internal message to JSON-RPC request (for testing/debugging)
pub fn from_internal_message(msg: &InternalMessage) -> Result<JsonRpcRequest, McpError> {
    let id = match &msg.id {
        Some(InternalId::Number(n)) => RequestId::Number(*n),
        Some(InternalId::String(s)) => RequestId::String(s.clone()),
        None => {
            return Err(McpError::invalid_params(
                "Cannot convert notification to request",
            ));
        }
    };

    let params = if msg.params_raw.is_empty() {
        None
    } else {
        Some(
            serde_json::from_slice(&msg.params_raw)
                .map_err(|e| McpError::serialization(format!("Failed to parse params: {e}")))?,
        )
    };

    Ok(JsonRpcRequest {
        jsonrpc: crate::jsonrpc::JsonRpcVersion,
        method: msg.method.clone(),
        params,
        id,
    })
}

/// Create an internal success response from a JSON result
pub fn success_response(
    id: InternalId,
    result: &serde_json::Value,
) -> Result<InternalResponse, McpError> {
    let result_raw = serde_json::to_vec(result)
        .map_err(|e| McpError::serialization(format!("Failed to serialize result: {e}")))?;
    Ok(InternalResponse::success(id, result_raw))
}

/// Create an internal error response from an MCP error
pub fn error_response(id: InternalId, error: &McpError) -> InternalResponse {
    let internal_error = InternalError::new(error.jsonrpc_code(), error.to_string());
    InternalResponse::error(id, internal_error)
}

/// Trait for types that can be converted to internal format
pub trait ToInternal {
    /// The internal representation type
    type Internal;

    /// Convert to internal format
    fn to_internal(&self) -> Result<Self::Internal, McpError>;
}

impl ToInternal for JsonRpcRequest {
    type Internal = InternalMessage;

    fn to_internal(&self) -> Result<Self::Internal, McpError> {
        to_internal(self)
    }
}

/// Trait for types that can be converted from internal format
pub trait FromInternal: Sized {
    /// The internal representation type
    type Internal;

    /// Convert from internal format
    fn from_internal(internal: &Self::Internal) -> Result<Self, McpError>;
}

impl FromInternal for JsonRpcResponse {
    type Internal = InternalResponse;

    fn from_internal(internal: &Self::Internal) -> Result<Self, McpError> {
        from_internal_response(internal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::JsonRpcVersion;
    use rancor_crate;
    use rkyv_crate;

    #[test]
    fn test_to_internal_simple() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "tools/list".to_string(),
            params: None,
            id: RequestId::Number(1),
        };

        let internal = to_internal(&request).unwrap();
        assert_eq!(internal.method, "tools/list");
        assert!(internal.is_request());
        assert!(internal.params_raw.is_empty());
    }

    #[test]
    fn test_to_internal_with_params() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({"name": "calculator", "args": {"a": 1, "b": 2}})),
            id: RequestId::String("req-123".to_string()),
        };

        let internal = to_internal(&request).unwrap();
        assert_eq!(internal.method, "tools/call");
        assert!(!internal.params_raw.is_empty());

        // Verify params are valid JSON
        let params: serde_json::Value = serde_json::from_slice(&internal.params_raw).unwrap();
        assert_eq!(params["name"], "calculator");
    }

    #[test]
    fn test_to_internal_with_hints() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({"name": "hello_world"})),
            id: RequestId::Number(42),
        };

        let (msg, hints) = to_internal_with_hints(&request).unwrap();
        assert_eq!(msg.method, "tools/call");
        assert_eq!(hints.tool_name, Some("hello_world".to_string()));
    }

    #[test]
    fn test_from_internal_response_success() {
        let response =
            InternalResponse::success(InternalId::Number(1), br#"{"content":"Hello!"}"#.to_vec());

        let json_rpc = from_internal_response(&response).unwrap();

        match json_rpc.payload {
            JsonRpcResponsePayload::Success { result } => {
                assert_eq!(result["content"], "Hello!");
            }
            _ => panic!("Expected success response"),
        }
    }

    #[test]
    fn test_from_internal_response_error() {
        let error = InternalError::new(InternalError::METHOD_NOT_FOUND, "Method not found");
        let response = InternalResponse::error(InternalId::Number(1), error);

        let json_rpc = from_internal_response(&response).unwrap();

        match json_rpc.payload {
            JsonRpcResponsePayload::Error { error } => {
                assert_eq!(error.code, -32601); // METHOD_NOT_FOUND
                assert_eq!(error.message, "Method not found");
            }
            _ => panic!("Expected error response"),
        }
    }

    #[test]
    fn test_roundtrip() {
        let original = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "prompts/get".to_string(),
            params: Some(serde_json::json!({"name": "greeting", "arguments": {"user": "Alice"}})),
            id: RequestId::Number(99),
        };

        let internal = to_internal(&original).unwrap();
        let restored = from_internal_message(&internal).unwrap();

        assert_eq!(original.method, restored.method);
        assert_eq!(original.id, restored.id);
        assert_eq!(original.params, restored.params);
    }

    #[test]
    fn test_rkyv_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            method: "resources/read".to_string(),
            params: Some(serde_json::json!({"uri": "file:///test.txt"})),
            id: RequestId::Number(1),
        };

        let internal = to_internal(&request).unwrap();

        // Serialize to rkyv bytes
        let bytes = rkyv_crate::to_bytes::<rancor_crate::Error>(&internal).expect("serialize");

        // Zero-copy access
        let archived = rkyv_crate::access::<
            turbomcp_core::rkyv_types::ArchivedInternalMessage,
            rancor_crate::Error,
        >(&bytes)
        .expect("access");

        assert_eq!(archived.method_str(), "resources/read");
        assert!(archived.is_request());
    }

    #[test]
    fn test_success_response_helper() {
        let result = serde_json::json!({"tools": [{"name": "test"}]});
        let response = success_response(InternalId::Number(1), &result).unwrap();

        assert!(response.is_success());
        let restored: serde_json::Value =
            serde_json::from_slice(&response.result_raw.unwrap()).unwrap();
        assert_eq!(restored, result);
    }

    #[test]
    fn test_error_response_helper() {
        let error = McpError::method_not_found("unknown_method");
        let response = error_response(InternalId::Number(1), &error);

        assert!(response.is_error());
        assert_eq!(response.error.as_ref().unwrap().code, -32601);
    }
}

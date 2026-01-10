//! Router utility functions for parsing, validation, and responses

use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

use crate::{McpError, ServerErrorExt, ServerResult};

/// Parse request parameters from JSON-RPC request
pub fn parse_params<T>(request: &JsonRpcRequest) -> ServerResult<T>
where
    T: serde::de::DeserializeOwned,
{
    match &request.params {
        Some(params) => serde_json::from_value(params.clone()).map_err(|e| {
            McpError::routing(format!("Invalid parameters: {e}"))
                .with_operation(request.method.clone())
        }),
        None => Err(McpError::routing("Missing required parameters".to_string())
            .with_operation(request.method.clone())),
    }
}

/// Create a success response for JSON-RPC request
pub fn success_response<T>(request: &JsonRpcRequest, result: T) -> JsonRpcResponse
where
    T: serde::Serialize,
{
    JsonRpcResponse::success(serde_json::to_value(result).unwrap(), request.id.clone())
}

/// Create an error response for JSON-RPC request
pub fn error_response(request: &JsonRpcRequest, error: McpError) -> JsonRpcResponse {
    JsonRpcResponse::error_response(
        turbomcp_protocol::jsonrpc::JsonRpcError {
            code: error.jsonrpc_code(),
            message: error.to_string(),
            data: None,
        },
        request.id.clone(),
    )
}

/// Create a method not found response for JSON-RPC request
pub fn method_not_found_response(request: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse::error_response(
        turbomcp_protocol::jsonrpc::JsonRpcError {
            code: -32601,
            message: format!("Method '{}' not found", request.method),
            data: None,
        },
        request.id.clone(),
    )
}

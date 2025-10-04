//! JSON-RPC HTTP handler for MCP requests

use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
};
use tracing::{error, trace};

use crate::axum::service::McpAppState;
use crate::axum::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::tower::SessionInfo;

/// JSON-RPC HTTP handler
pub async fn json_rpc_handler(
    State(app_state): State<McpAppState>,
    Extension(session): Extension<SessionInfo>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, StatusCode> {
    trace!("Processing JSON-RPC request: {:?}", request);

    // Validate JSON-RPC format
    if request.jsonrpc != "2.0" {
        return Ok(Json(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32600,
                message: "Invalid Request".to_string(),
                data: Some(serde_json::json!({
                    "reason": "jsonrpc field must be '2.0'"
                })),
            }),
        }));
    }

    // Create request object for service
    let service_request = serde_json::json!({
        "jsonrpc": request.jsonrpc,
        "id": request.id,
        "method": request.method,
        "params": request.params
    });

    // Process request through MCP service using AppState helper
    match app_state.process_request(service_request, &session).await {
        Ok(result) => {
            // Broadcast result to SSE clients if it's a notification
            if request.id.is_none() {
                let _ = app_state
                    .sse_sender
                    .send(serde_json::to_string(&result).unwrap_or_default());
            }

            Ok(Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(result),
                error: None,
            }))
        }
        Err(e) => {
            error!("MCP service error: {}", e);

            Ok(Json(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: "Internal error".to_string(),
                    data: Some(serde_json::json!({
                        "reason": e.to_string()
                    })),
                }),
            }))
        }
    }
}

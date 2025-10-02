//! Ping handler for health check operations

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{PingRequest, PingResult},
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle ping request - basic health check response
pub async fn handle(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<PingRequest>(&request) {
        Ok(_ping_request) => {
            // Default ping handler - basic health check response
            let result = PingResult::empty().with_meta(serde_json::json!({
                "status": "healthy",
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "server": "turbomcp-server",
            }));
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

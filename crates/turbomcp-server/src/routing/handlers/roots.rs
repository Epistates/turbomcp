//! Roots handler for MCP roots operations

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{ListRootsRequest, ListRootsResult},
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle list roots request
pub async fn handle_list(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<ListRootsRequest>(&request) {
        Ok(_roots_request) => {
            // TODO: Implement actual roots listing
            let result = ListRootsResult {
                roots: vec![],
                _meta: None,
            };
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

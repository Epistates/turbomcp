//! Roots handler for MCP roots operations

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{ListRootsRequest, ListRootsResult},
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle list roots request
///
/// This handler provides protocol-level routing for roots listing.
/// Applications define their filesystem roots via server configuration,
/// middleware, or custom handlers.
///
/// **Implementation Note:**
/// By default, returns an empty roots list. Applications should:
/// 1. Configure roots via server builder: `.with_roots(vec![...])`
/// 2. Implement custom middleware to provide dynamic roots
/// 3. Override this handler with application-specific logic
///
/// This allows flexibility for different deployment patterns (containers,
/// sandboxed environments, multi-tenant systems, etc.).
pub async fn handle_list(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<ListRootsRequest>(&request) {
        Ok(_roots_request) => {
            // Default: empty roots list (configure via server builder or middleware)
            let result = ListRootsResult {
                roots: vec![],
                _meta: None,
            };
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

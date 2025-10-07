//! Elicitation handler for MCP elicitation operations

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{ElicitRequest, ElicitResult},
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle elicitation request
///
/// This handler provides protocol-level routing for elicitation requests.
/// Elicitation is initiated by tools (not as standalone protocol requests),
/// so this returns a default successful response for protocol compliance.
///
/// **Implementation Note:**
/// Applications implement elicitation using the Context API within tools:
/// ```rust,ignore
/// #[tool("Setup user profile")]
/// async fn setup_profile(&self, ctx: Context) -> McpResult<String> {
///     let schema = json!({"type": "object", "properties": {...}});
///     // Client handles elicitation via ctx.elicit() in application code
///     Ok("Profile configured".to_string())
/// }
/// ```
///
/// See `examples/08_elicitation_server.rs` for complete examples.
pub async fn handle(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<ElicitRequest>(&request) {
        Ok(_elicit_request) => {
            // Protocol compliance: elicitation is user-implemented via Context API
            let result = ElicitResult {
                action: turbomcp_protocol::types::ElicitationAction::Accept,
                content: Some(std::collections::HashMap::new()),
                _meta: None,
            };
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

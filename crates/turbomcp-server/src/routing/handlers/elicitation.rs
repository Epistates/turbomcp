//! Elicitation handler for MCP elicitation operations

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{ElicitRequest, ElicitResult},
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle elicitation request
pub async fn handle(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<ElicitRequest>(&request) {
        Ok(_elicit_request) => {
            // TODO: Implement actual elicitation handling
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

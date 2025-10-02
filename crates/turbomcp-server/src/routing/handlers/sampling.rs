//! Sampling handler for MCP sampling operations

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{CreateMessageRequest, CreateMessageResult},
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle create message request
pub async fn handle_create_message(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<CreateMessageRequest>(&request) {
        Ok(_create_request) => {
            // TODO: Implement actual sampling message creation
            let result = CreateMessageResult {
                model: "default".to_string(),
                role: turbomcp_protocol::types::Role::Assistant,
                content: turbomcp_protocol::types::Content::Text(
                    turbomcp_protocol::types::TextContent {
                        text: "Sample response".to_string(),
                        annotations: None,
                        meta: None,
                    },
                ),
                stop_reason: Some("completed".to_string()),
                _meta: None,
            };
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

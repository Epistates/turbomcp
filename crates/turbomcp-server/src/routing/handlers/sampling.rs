//! Sampling handler for MCP sampling operations

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{CreateMessageRequest, CreateMessageResult},
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle create message request
///
/// This handler provides protocol-level routing for sampling/createMessage requests.
/// The sampling protocol allows servers to request LLM completions from clients.
///
/// **Implementation Note:**
/// This is a server-to-client request pattern. Servers initiate sampling by sending
/// `sampling/createMessage` requests to clients, which then forward them to their
/// configured LLM provider (OpenAI, Anthropic, local model, etc.).
///
/// This handler returns a placeholder response for protocol compliance, but in
/// production MCP servers:
/// 1. The server sends sampling requests TO the client (not receives them)
/// 2. The client implements `SamplingHandler` to route to their LLM provider
/// 3. The client returns the LLM's response back to the server
///
/// See `turbomcp-client` documentation for implementing sampling handlers.
pub async fn handle_create_message(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<CreateMessageRequest>(&request) {
        Ok(_create_request) => {
            // Protocol compliance: sampling is a server-to-client request pattern
            // Servers initiate sampling, clients implement the LLM integration
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
                stop_reason: Some(turbomcp_protocol::types::StopReason::EndTurn),
                _meta: None,
            };
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

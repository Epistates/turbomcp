//! Prompt handlers for MCP prompt operations

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{GetPromptRequest, ListPromptsResult},
};

use super::HandlerContext;
use crate::McpError;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle list prompts request
pub async fn handle_list(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    let prompts = context.registry.get_prompt_definitions();
    let result = ListPromptsResult {
        prompts,
        next_cursor: None,
        _meta: None,
    };
    success_response(&request, result)
}

/// Handle get prompt request
pub async fn handle_get(
    context: &HandlerContext,
    request: JsonRpcRequest,
    ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<GetPromptRequest>(&request) {
        Ok(prompt_request) => {
            if let Some(handler) = context.registry.get_prompt(&prompt_request.name) {
                match handler.handle(prompt_request, ctx).await {
                    Ok(prompt_result) => success_response(&request, prompt_result),
                    Err(e) => error_response(&request, e),
                }
            } else {
                let error = McpError::not_found(format!("Prompt '{}'", prompt_request.name));
                error_response(&request, error)
            }
        }
        Err(e) => error_response(&request, e),
    }
}

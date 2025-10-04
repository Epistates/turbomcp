//! Tool handlers for MCP tool operations - PURE BUSINESS LOGIC ONLY
//!
//! This module contains only the core business logic for tool operations.
//! All cross-cutting concerns (logging, timeout, performance) are handled by middleware.

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{CallToolRequest, ListToolsResult},
};

use super::HandlerContext;
use crate::ServerError;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle list tools request
pub async fn handle_list(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    let tools = context.registry.get_tool_definitions();
    let result = ListToolsResult {
        tools,
        next_cursor: None,
        _meta: None,
    };
    success_response(&request, result)
}

/// Handle call tool request - pure business logic only
pub async fn handle_call(
    context: &HandlerContext,
    request: JsonRpcRequest,
    ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<CallToolRequest>(&request) {
        Ok(call_request) => {
            let tool_name = call_request.name.clone();

            if let Some(handler) = context.registry.get_tool(&tool_name) {
                match handler.handle(call_request, ctx).await {
                    Ok(tool_result) => success_response(&request, tool_result),
                    Err(e) => error_response(&request, e),
                }
            } else {
                let error = ServerError::not_found(format!("Tool '{tool_name}'"));
                error_response(&request, error)
            }
        }
        Err(e) => error_response(&request, e),
    }
}

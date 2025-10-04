//! Resource handlers for MCP resource operations

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{
        EmptyResult, ListResourceTemplatesRequest, ListResourceTemplatesResult,
        ListResourcesResult, ReadResourceRequest, SubscribeRequest, UnsubscribeRequest,
    },
};

use super::HandlerContext;
use crate::ServerError;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle list resources request
pub async fn handle_list(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    let resources = context.registry.get_resource_definitions();
    let result = ListResourcesResult {
        resources,
        next_cursor: None,
        _meta: None,
    };
    success_response(&request, result)
}

/// Handle read resource request
pub async fn handle_read(
    context: &HandlerContext,
    request: JsonRpcRequest,
    ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<ReadResourceRequest>(&request) {
        Ok(read_request) => {
            if let Some(handler) = context.registry.get_resource(&read_request.uri) {
                match handler.handle(read_request, ctx).await {
                    Ok(resource_result) => success_response(&request, resource_result),
                    Err(e) => error_response(&request, e),
                }
            } else {
                let error = ServerError::not_found(format!("Resource '{}'", read_request.uri));
                error_response(&request, error)
            }
        }
        Err(e) => error_response(&request, e),
    }
}

/// Handle subscribe resource request
pub async fn handle_subscribe(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<SubscribeRequest>(&request) {
        Ok(_subscribe_request) => {
            // TODO: Implement resource subscription tracking
            let result = EmptyResult::new();
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

/// Handle unsubscribe resource request
pub async fn handle_unsubscribe(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<UnsubscribeRequest>(&request) {
        Ok(_unsubscribe_request) => {
            // TODO: Implement resource subscription tracking
            let result = EmptyResult::new();
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

/// Handle list resource templates request
pub async fn handle_list_templates(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<ListResourceTemplatesRequest>(&request) {
        Ok(_templates_request) => {
            let templates = vec![]; // TODO: Implement resource template definitions
            let result = ListResourceTemplatesResult {
                resource_templates: templates,
                next_cursor: None,
                _meta: None,
            };
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

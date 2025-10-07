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
///
/// This handler provides protocol-level routing for resource subscription requests.
/// Resource subscriptions allow clients to receive notifications when resources change.
///
/// **Implementation Note:**
/// By default, accepts subscriptions but doesn't track them. Applications implementing
/// dynamic resources should:
/// 1. Store subscription state (e.g., `Arc<RwLock<HashSet<String>>>`)
/// 2. Emit `notifications/resources/updated` when resources change
/// 3. Use middleware or custom handlers to manage subscription lifecycle
///
/// For static resources (files, documentation), subscriptions may not be needed.
pub async fn handle_subscribe(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<SubscribeRequest>(&request) {
        Ok(_subscribe_request) => {
            // Protocol compliance: subscription tracking is application-specific
            let result = EmptyResult::new();
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

/// Handle unsubscribe resource request
///
/// This handler provides protocol-level routing for resource unsubscription requests.
///
/// **Implementation Note:**
/// By default, accepts unsubscription requests. Applications tracking subscriptions
/// should remove the subscription from their state when this is called.
///
/// See `handle_subscribe` documentation for subscription management patterns.
pub async fn handle_unsubscribe(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<UnsubscribeRequest>(&request) {
        Ok(_unsubscribe_request) => {
            // Protocol compliance: subscription cleanup is application-specific
            let result = EmptyResult::new();
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

/// Handle list resource templates request
///
/// This handler provides protocol-level routing for resource template listing.
/// Resource templates define URI patterns for dynamically generated resources.
///
/// **Implementation Note:**
/// By default, returns an empty template list. Applications should define templates
/// using the `#[resource]` attribute with URI patterns:
///
/// ```rust,ignore
/// #[resource("file://{path}")]
/// async fn read_file(&self, path: String) -> McpResult<ResourceContents> {
///     // Dynamic resource based on path parameter
/// }
/// ```
///
/// The framework automatically generates templates from `#[resource]` attributes
/// when using the derive macro. Templates can also be registered programmatically
/// via the server builder or middleware.
pub async fn handle_list_templates(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<ListResourceTemplatesRequest>(&request) {
        Ok(_templates_request) => {
            // Default: no templates (define via #[resource] attributes or server builder)
            let templates = vec![];
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

//! Completion handler for MCP completion operations

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{CompleteRequestParams, CompleteResult},
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle completion request
///
/// This handler provides protocol-level routing for completion requests.
/// Completions provide autocomplete suggestions for prompt arguments and
/// resource URIs, enhancing the user experience in MCP clients.
///
/// **Implementation Note:**
/// By default, returns empty completion results. Applications should implement
/// completion logic in their prompts and resources using the `#[complete]` attribute:
///
/// ```rust,ignore
/// #[prompt("code_review")]
/// async fn code_review(
///     &self,
///     language: String,
///     #[complete] framework: String, // Auto-completion enabled
/// ) -> McpResult<String> { ... }
/// ```
///
/// The framework will automatically route completion requests to the appropriate
/// handlers when using `#[complete]` attributes. Custom completion logic can also
/// be provided via middleware or custom handlers.
pub async fn handle(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<CompleteRequestParams>(&request) {
        Ok(_complete_request) => {
            // Default: no completions (implement via #[complete] attributes or middleware)
            let result = CompleteResult {
                completion: turbomcp_protocol::types::CompletionData {
                    values: vec![],
                    total: Some(0),
                    has_more: Some(false),
                },
                _meta: None,
            };
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

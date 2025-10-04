//! Initialize handler for MCP protocol handshake

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{
        Implementation, InitializeRequest, InitializeResult, LoggingCapabilities,
        PromptsCapabilities, ResourcesCapabilities, ServerCapabilities, ToolsCapabilities,
    },
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle initialize request
pub async fn handle(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<InitializeRequest>(&request) {
        Ok(_init_request) => {
            let result = InitializeResult {
                protocol_version: turbomcp_protocol::PROTOCOL_VERSION.to_string(),
                server_info: Implementation {
                    name: crate::SERVER_NAME.to_string(),
                    title: Some("TurboMCP Server".to_string()),
                    version: crate::SERVER_VERSION.to_string(),
                },
                capabilities: get_server_capabilities(context),
                instructions: None,
                _meta: None,
            };

            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}

/// Get server capabilities based on registry state
fn get_server_capabilities(context: &HandlerContext) -> ServerCapabilities {
    ServerCapabilities {
        tools: if context.registry.tools.is_empty() {
            None
        } else {
            Some(ToolsCapabilities::default())
        },
        prompts: if context.registry.prompts.is_empty() {
            None
        } else {
            Some(PromptsCapabilities::default())
        },
        resources: if context.registry.resources.is_empty() {
            None
        } else {
            Some(ResourcesCapabilities::default())
        },
        logging: if !context.registry.logging.is_empty() {
            Some(LoggingCapabilities)
        } else {
            None
        },
        experimental: None,
        completions: None,
    }
}

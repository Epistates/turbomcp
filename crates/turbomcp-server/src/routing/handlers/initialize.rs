//! Initialize handler for MCP protocol handshake

use tracing::{debug, warn};
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::{
    SUPPORTED_VERSIONS,
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{
        Implementation, InitializeRequest, InitializeResult, LoggingCapabilities,
        PromptsCapabilities, ResourcesCapabilities, ServerCapabilities, ToolsCapabilities,
    },
};

use super::HandlerContext;
use crate::config::ProtocolVersionConfig;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Negotiate protocol version with client using server configuration
///
/// Per MCP spec:
/// 1. If client's version is in our supported list → use client's version
/// 2. If allow_fallback is true → offer our preferred version (client decides to accept or not)
/// 3. If allow_fallback is false → reject immediately
fn negotiate_protocol_version(
    client_version: &str,
    version_config: &ProtocolVersionConfig,
) -> Result<String, String> {
    // Get effective supported versions (config or defaults)
    let supported: Vec<&str> = if version_config.supported.is_empty() {
        SUPPORTED_VERSIONS.to_vec()
    } else {
        version_config
            .supported
            .iter()
            .map(|s| s.as_str())
            .collect()
    };

    // If we support the client's requested version, use it (best case)
    if supported.contains(&client_version) {
        debug!(
            client_version = client_version,
            "Client protocol version supported, using it"
        );
        return Ok(client_version.to_string());
    }

    // Client's version not in our supported list
    if !version_config.allow_fallback {
        // Strict mode: reject immediately
        warn!(
            client_version = client_version,
            supported_versions = ?supported,
            "Client protocol version not supported, fallback disabled"
        );
        return Err(format!(
            "Protocol version '{}' not supported. Supported versions: {:?}",
            client_version, supported
        ));
    }

    // Fallback enabled: offer our preferred version
    // The client will receive this and can choose to continue or disconnect
    debug!(
        client_version = client_version,
        server_version = %version_config.preferred,
        "Client protocol version not supported, offering fallback to preferred version"
    );

    Ok(version_config.preferred.clone())
}

/// Handle initialize request
pub async fn handle(
    context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<InitializeRequest>(&request) {
        Ok(init_request) => {
            // Negotiate protocol version based on client's request and server config
            let negotiated_version = match negotiate_protocol_version(
                &init_request.protocol_version,
                &context.config.protocol_version,
            ) {
                Ok(version) => version,
                Err(msg) => {
                    return error_response(
                        &request,
                        crate::ServerError::Handler {
                            message: msg,
                            context: Some("protocol_version_negotiation".to_string()),
                        },
                    );
                }
            };

            #[allow(clippy::needless_update)] // Default needed for feature-gated fields (description, icons)
            let result = InitializeResult {
                protocol_version: negotiated_version,
                server_info: Implementation {
                    name: context.config.name.clone(),
                    title: context.config.description.clone(),
                    version: context.config.version.clone(),
                    ..Default::default()
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
            Some(LoggingCapabilities {})
        } else {
            None
        },
        experimental: None,
        completions: None,
        #[cfg(feature = "mcp-tasks")]
        tasks: {
            // Import ServerTasksCapabilities for task capability reporting
            use turbomcp_protocol::types::ServerTasksCapabilities;
            // Report task capabilities when task_storage is available
            Some(ServerTasksCapabilities::default())
        },
    }
}

//! JSON-RPC request routing for McpHandler.
//!
//! This module provides the native server's request routing with additional
//! configuration validation beyond the core router.
//!
//! # Architecture
//!
//! The native router layers on top of `turbomcp_core::router`:
//! - **Core router**: Basic MCP method dispatch (shared with WASM)
//! - **Native router**: Protocol negotiation, capability validation
//!
//! # MCP Protocol Compliance
//!
//! This router implements the MCP 2025-11-25 specification:
//! - Initialize request validates `clientInfo` and `protocolVersion`
//! - Notifications (requests without `id`) do not receive responses
//! - Capability structure follows the spec format
//! - Error codes follow JSON-RPC 2.0 standard

use super::config::{ClientCapabilities, ServerConfig};
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::McpError;
use turbomcp_core::handler::McpHandler;

// Re-export canonical JSON-RPC types from turbomcp-core
pub use turbomcp_core::jsonrpc::{JsonRpcIncoming, JsonRpcOutgoing};
// Re-export core router utilities
pub use turbomcp_core::router::{parse_request, serialize_response};

/// Route a JSON-RPC request to the appropriate handler method.
///
/// This is the simple routing function that uses default configuration.
/// For more control, use `route_request_with_config`.
pub async fn route_request<H: McpHandler>(
    handler: &H,
    request: JsonRpcIncoming,
    ctx: &RequestContext,
) -> JsonRpcOutgoing {
    route_request_with_config(handler, request, ctx, None).await
}

/// Route a JSON-RPC request with custom server configuration.
///
/// This function provides full control over protocol negotiation,
/// capability validation, and other server behavior.
///
/// # Additional Validation (vs core router)
///
/// When a `ServerConfig` is provided, this function adds:
/// - Protocol version negotiation
/// - Required client capability validation
pub async fn route_request_with_config<H: McpHandler>(
    handler: &H,
    request: JsonRpcIncoming,
    ctx: &RequestContext,
    config: Option<&ServerConfig>,
) -> JsonRpcOutgoing {
    let id = request.id.clone();

    // Validate message size against configured limit
    if let Some(config) = config
        && let Some(ref params) = request.params
    {
        let estimated_size = params.to_string().len();
        if estimated_size > config.max_message_size {
            return JsonRpcOutgoing::error(
                id,
                McpError::invalid_request(format!(
                    "Message size {} exceeds maximum allowed size of {} bytes",
                    estimated_size, config.max_message_size
                )),
            );
        }
    }

    // For initialize requests, apply native-specific validation
    if request.method == "initialize" {
        let params = request.params.clone().unwrap_or_default();

        // Validate clientInfo is present (MCP spec requirement)
        let Some(client_info) = params.get("clientInfo") else {
            return JsonRpcOutgoing::error(
                id,
                McpError::invalid_params("Missing required field: clientInfo"),
            );
        };

        // Validate clientInfo has required fields
        let client_name = client_info.get("name").and_then(|v| v.as_str());
        let client_version = client_info.get("version").and_then(|v| v.as_str());
        if client_name.is_none() || client_version.is_none() {
            return JsonRpcOutgoing::error(
                id,
                McpError::invalid_params("clientInfo must contain 'name' and 'version' fields"),
            );
        }

        // Extract client's requested protocol version
        let protocol_version = params.get("protocolVersion").and_then(|v| v.as_str());

        // Get protocol config (use default if none provided)
        let protocol_config = config.map(|c| &c.protocol).cloned().unwrap_or_default();

        // Negotiate protocol version
        let negotiated_version = match protocol_config.negotiate(protocol_version) {
            Some(version) => {
                // Log if server fell back to a different version
                if let Some(client_ver) = protocol_version
                    && client_ver != version
                {
                    tracing::warn!(
                        client_version = client_ver,
                        negotiated_version = %version,
                        supported = ?protocol_config.supported_versions,
                        "Protocol version fallback: client requested unsupported version"
                    );
                }
                version
            }
            None => {
                return JsonRpcOutgoing::error(
                    id,
                    McpError::invalid_request(format!(
                        "Unsupported protocol version: {}. Supported versions: {:?}",
                        protocol_version.unwrap_or("none"),
                        protocol_config.supported_versions
                    )),
                );
            }
        };

        // Parse and validate client capabilities if required
        if let Some(cfg) = config {
            let client_caps = ClientCapabilities::from_params(&params);
            let validation = cfg.required_capabilities.validate(&client_caps);

            if let Some(missing) = validation.missing() {
                return JsonRpcOutgoing::error(
                    id,
                    McpError::invalid_request(format!(
                        "Missing required client capabilities: {}",
                        missing.join(", ")
                    )),
                );
            }
        }

        // Use core router with negotiated version
        let core_config = turbomcp_core::router::RouteConfig {
            protocol_version: Some(&negotiated_version),
        };
        return turbomcp_core::router::route_request(handler, request, ctx, &core_config).await;
    }

    // For all other methods, delegate to core router
    let core_config = turbomcp_core::router::RouteConfig::default();
    turbomcp_core::router::route_request(handler, request, ctx, &core_config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use turbomcp_core::error::McpResult;
    use turbomcp_types::{
        Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
    };

    #[derive(Clone)]
    struct TestHandler;

    impl McpHandler for TestHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("test", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![Tool::new("test_tool", "A test tool")]
        }

        fn list_resources(&self) -> Vec<Resource> {
            vec![]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![]
        }

        fn call_tool(
            &self,
            name: &str,
            _args: Value,
            _ctx: &RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ToolResult>> + Send {
            let name = name.to_string();
            async move {
                if name == "test_tool" {
                    Ok(ToolResult::text("Tool executed"))
                } else {
                    Err(McpError::tool_not_found(&name))
                }
            }
        }

        fn read_resource(
            &self,
            uri: &str,
            _ctx: &RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>> + Send {
            let uri = uri.to_string();
            async move { Err(McpError::resource_not_found(&uri)) }
        }

        fn get_prompt(
            &self,
            name: &str,
            _args: Option<Value>,
            _ctx: &RequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>> + Send {
            let name = name.to_string();
            async move { Err(McpError::prompt_not_found(&name)) }
        }
    }

    #[test]
    fn test_parse_request() {
        let input = r#"{"jsonrpc": "2.0", "id": 1, "method": "ping"}"#;
        let request = parse_request(input).unwrap();
        assert_eq!(request.method, "ping");
        assert_eq!(request.id, Some(serde_json::json!(1)));
    }

    #[test]
    fn test_serialize_response() {
        let response = JsonRpcOutgoing::success(Some(serde_json::json!(1)), serde_json::json!({}));
        let serialized = serialize_response(&response).unwrap();
        assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
        assert!(serialized.contains("\"id\":1"));
    }

    #[tokio::test]
    async fn test_route_initialize() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            // MCP spec requires clientInfo with name and version
            params: Some(serde_json::json!({
                "protocolVersion": "2025-11-25",
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                },
                "capabilities": {}
            })),
        };

        let response = route_request(&handler, request, &ctx).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "test");
        // Verify capabilities structure per MCP spec
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["capabilities"]["tools"]["listChanged"], true);
    }

    #[tokio::test]
    async fn test_route_initialize_missing_client_info() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2025-11-25"
            })),
        };

        let response = route_request(&handler, request, &ctx).await;
        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, -32602); // INVALID_PARAMS
        assert!(error.message.contains("clientInfo"));
    }

    #[tokio::test]
    async fn test_route_initialized_notification() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        // Notification has no id
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };

        let response = route_request(&handler, request, &ctx).await;
        // Notification responses should not be sent
        assert!(!response.should_send());
    }

    #[tokio::test]
    async fn test_route_tools_list() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = route_request(&handler, request, &ctx).await;
        assert!(response.result.is_some());

        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "test_tool");
    }

    #[tokio::test]
    async fn test_route_tools_call() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "test_tool",
                "arguments": {}
            })),
        };

        let response = route_request(&handler, request, &ctx).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_route_unknown_method() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "unknown/method".to_string(),
            params: None,
        };

        let response = route_request(&handler, request, &ctx).await;
        assert!(response.result.is_none());
        assert!(response.error.is_some());

        let error = response.error.unwrap();
        assert_eq!(error.code, -32601); // METHOD_NOT_FOUND
    }
}

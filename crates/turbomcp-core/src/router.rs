//! Shared MCP request router for all platforms.
//!
//! This module provides the core routing logic that maps JSON-RPC requests to
//! `McpHandler` methods. It is designed to work on both native and WASM targets.
//!
//! # Design Philosophy
//!
//! - **Unified**: Single router implementation for native and WASM
//! - **no_std Compatible**: Works in `no_std` environments with `alloc`
//! - **Extensible**: Native can layer additional validation (protocol, capabilities)
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_core::router::{route_request, RouteConfig};
//! use turbomcp_core::jsonrpc::{JsonRpcIncoming, JsonRpcOutgoing};
//! use turbomcp_core::context::RequestContext;
//!
//! // Basic routing (WASM-friendly)
//! let response = route_request(&handler, request, &ctx, &RouteConfig::default()).await;
//!
//! // With protocol version override
//! let config = RouteConfig {
//!     protocol_version: Some("2025-06-18"),
//! };
//! let response = route_request(&handler, request, &ctx, &config).await;
//! ```

use alloc::string::ToString;
use serde_json::Value;

use crate::PROTOCOL_VERSION;
use crate::context::RequestContext;
use crate::error::McpError;
use crate::handler::McpHandler;
use crate::jsonrpc::{JsonRpcIncoming, JsonRpcOutgoing};
use turbomcp_types::ServerInfo;

/// Configuration for request routing.
///
/// This provides minimal configuration that works on all platforms.
/// Native platforms can provide additional validation through middleware.
#[derive(Debug, Clone, Default)]
pub struct RouteConfig<'a> {
    /// Override protocol version in initialize response.
    /// If None, uses `PROTOCOL_VERSION` constant.
    pub protocol_version: Option<&'a str>,
}

/// Route a JSON-RPC request to the appropriate handler method.
///
/// This is the core routing function used by both native and WASM platforms.
/// It dispatches MCP methods to the corresponding `McpHandler` methods.
///
/// # Supported Methods
///
/// - `initialize` - Returns server info and capabilities
/// - `initialized` / `notifications/initialized` - Acknowledges initialization
/// - `tools/list` - Lists available tools
/// - `tools/call` - Calls a tool by name
/// - `resources/list` - Lists available resources
/// - `resources/read` - Reads a resource by URI
/// - `prompts/list` - Lists available prompts
/// - `prompts/get` - Gets a prompt by name
/// - `ping` - Health check
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_core::router::{route_request, RouteConfig};
///
/// let config = RouteConfig::default();
/// let response = route_request(&handler, request, &ctx, &config).await;
///
/// if response.should_send() {
///     // Send response to client
/// }
/// ```
pub async fn route_request<H: McpHandler>(
    handler: &H,
    request: JsonRpcIncoming,
    ctx: &RequestContext,
    config: &RouteConfig<'_>,
) -> JsonRpcOutgoing {
    let id = request.id.clone();

    match request.method.as_str() {
        // Initialize handshake
        "initialize" => {
            let params = request.params.clone().unwrap_or_default();

            // Validate clientInfo is present (MCP spec requirement)
            let client_info = params.get("clientInfo");
            if client_info.is_none() {
                return JsonRpcOutgoing::error(
                    id,
                    McpError::invalid_params("Missing required field: clientInfo"),
                );
            }
            let client_info = client_info.unwrap();

            // Validate clientInfo has required fields
            let client_name = client_info.get("name").and_then(|v| v.as_str());
            let client_version = client_info.get("version").and_then(|v| v.as_str());
            if client_name.is_none() || client_version.is_none() {
                return JsonRpcOutgoing::error(
                    id,
                    McpError::invalid_params("clientInfo must contain 'name' and 'version' fields"),
                );
            }

            let protocol_version = config.protocol_version.unwrap_or(PROTOCOL_VERSION);
            let info = handler.server_info();
            let result = build_initialize_result(&info, handler, protocol_version);
            JsonRpcOutgoing::success(id, result)
        }

        // Handle both "initialized" and "notifications/initialized"
        // Per JSON-RPC 2.0, notifications (no id) should not receive responses
        "initialized" | "notifications/initialized" => {
            if id.is_some() {
                JsonRpcOutgoing::success(id, serde_json::json!({}))
            } else {
                JsonRpcOutgoing::notification_ack()
            }
        }

        // Tool methods
        "tools/list" => {
            let tools = handler.list_tools();
            let result = serde_json::json!({ "tools": tools });
            JsonRpcOutgoing::success(id, result)
        }

        "tools/call" => {
            let params = request.params.unwrap_or_default();
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or_default();

            match handler.call_tool(name, args, ctx).await {
                Ok(result) => {
                    let result_value = serde_json::to_value(&result).unwrap_or_default();
                    JsonRpcOutgoing::success(id, result_value)
                }
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        // Resource methods
        "resources/list" => {
            let resources = handler.list_resources();
            let result = serde_json::json!({ "resources": resources });
            JsonRpcOutgoing::success(id, result)
        }

        "resources/read" => {
            let params = request.params.unwrap_or_default();
            let uri = params
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            match handler.read_resource(uri, ctx).await {
                Ok(result) => {
                    let result_value = serde_json::to_value(&result).unwrap_or_default();
                    JsonRpcOutgoing::success(id, result_value)
                }
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        // Prompt methods
        "prompts/list" => {
            let prompts = handler.list_prompts();
            let result = serde_json::json!({ "prompts": prompts });
            JsonRpcOutgoing::success(id, result)
        }

        "prompts/get" => {
            let params = request.params.unwrap_or_default();
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let args = params.get("arguments").cloned();

            match handler.get_prompt(name, args, ctx).await {
                Ok(result) => {
                    let result_value = serde_json::to_value(&result).unwrap_or_default();
                    JsonRpcOutgoing::success(id, result_value)
                }
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        // Ping
        "ping" => JsonRpcOutgoing::success(id, serde_json::json!({})),

        // Unknown method
        _ => JsonRpcOutgoing::error(id, McpError::method_not_found(&request.method)),
    }
}

/// Build the initialize result with server info and capabilities.
///
/// # MCP Spec Compliance
///
/// The capabilities object follows the MCP 2025-11-25 specification:
/// - Each capability is an object (not boolean)
/// - Capabilities are only included if the server supports them
/// - Sub-properties like `listChanged` indicate notification support
fn build_initialize_result<H: McpHandler>(
    info: &ServerInfo,
    handler: &H,
    protocol_version: &str,
) -> Value {
    let has_tools = !handler.list_tools().is_empty();
    let has_resources = !handler.list_resources().is_empty();
    let has_prompts = !handler.list_prompts().is_empty();

    // Build capabilities object per MCP spec
    let mut capabilities = serde_json::Map::new();

    if has_tools {
        capabilities.insert(
            "tools".to_string(),
            serde_json::json!({ "listChanged": true }),
        );
    }

    if has_resources {
        capabilities.insert(
            "resources".to_string(),
            serde_json::json!({ "listChanged": true }),
        );
    }

    if has_prompts {
        capabilities.insert(
            "prompts".to_string(),
            serde_json::json!({ "listChanged": true }),
        );
    }

    // Build server info
    let mut server_info = serde_json::Map::new();
    server_info.insert("name".to_string(), serde_json::json!(info.name));
    server_info.insert("version".to_string(), serde_json::json!(info.version));

    // Build final result
    let mut result = serde_json::Map::new();
    result.insert(
        "protocolVersion".to_string(),
        serde_json::json!(protocol_version),
    );
    result.insert("capabilities".to_string(), Value::Object(capabilities));
    result.insert("serverInfo".to_string(), Value::Object(server_info));

    Value::Object(result)
}

/// Parse a JSON string into a JSON-RPC incoming request.
///
/// This is a convenience function for parsing incoming messages.
pub fn parse_request(input: &str) -> Result<JsonRpcIncoming, McpError> {
    JsonRpcIncoming::parse(input).map_err(|e| McpError::parse_error(e.to_string()))
}

/// Serialize a JSON-RPC outgoing response to a string.
///
/// This is a convenience function for serializing outgoing messages.
pub fn serialize_response(response: &JsonRpcOutgoing) -> Result<alloc::string::String, McpError> {
    response
        .to_json()
        .map_err(|e| McpError::internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::McpResult;
    use crate::marker::MaybeSend;
    use core::future::Future;
    use turbomcp_types::{Prompt, PromptResult, Resource, ResourceResult, Tool, ToolResult};

    #[derive(Clone)]
    struct TestHandler;

    impl McpHandler for TestHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("test-router", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![Tool::new("greet", "Say hello")]
        }

        fn list_resources(&self) -> Vec<Resource> {
            vec![]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![]
        }

        fn call_tool<'a>(
            &'a self,
            name: &'a str,
            args: Value,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a {
            let name = name.to_string();
            async move {
                match name.as_str() {
                    "greet" => {
                        let who = args.get("name").and_then(|v| v.as_str()).unwrap_or("World");
                        Ok(ToolResult::text(alloc::format!("Hello, {}!", who)))
                    }
                    _ => Err(McpError::tool_not_found(&name)),
                }
            }
        }

        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
            let uri = uri.to_string();
            async move { Err(McpError::resource_not_found(&uri)) }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<Value>,
            _ctx: &'a RequestContext,
        ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
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
        let config = RouteConfig::default();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2025-11-25",
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                },
                "capabilities": {}
            })),
        };

        let response = route_request(&handler, request, &ctx, &config).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "test-router");
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["capabilities"]["tools"]["listChanged"], true);
    }

    #[tokio::test]
    async fn test_route_initialize_missing_client_info() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let config = RouteConfig::default();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2025-11-25"
            })),
        };

        let response = route_request(&handler, request, &ctx, &config).await;
        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, -32602); // INVALID_PARAMS
    }

    #[tokio::test]
    async fn test_route_tools_list() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let config = RouteConfig::default();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = route_request(&handler, request, &ctx, &config).await;
        assert!(response.result.is_some());

        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "greet");
    }

    #[tokio::test]
    async fn test_route_tools_call() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let config = RouteConfig::default();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "greet",
                "arguments": {"name": "Alice"}
            })),
        };

        let response = route_request(&handler, request, &ctx, &config).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_route_ping() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let config = RouteConfig::default();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "ping".to_string(),
            params: None,
        };

        let response = route_request(&handler, request, &ctx, &config).await;
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn test_route_notification() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let config = RouteConfig::default();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "notifications/initialized".to_string(),
            params: None,
        };

        let response = route_request(&handler, request, &ctx, &config).await;
        assert!(!response.should_send());
    }

    #[tokio::test]
    async fn test_route_unknown_method() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let config = RouteConfig::default();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "unknown/method".to_string(),
            params: None,
        };

        let response = route_request(&handler, request, &ctx, &config).await;
        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, -32601); // METHOD_NOT_FOUND
    }

    #[tokio::test]
    async fn test_route_with_custom_protocol_version() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let config = RouteConfig {
            protocol_version: Some("2025-06-18"),
        };
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2025-06-18",
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            })),
        };

        let response = route_request(&handler, request, &ctx, &config).await;
        let result = response.result.unwrap();
        assert_eq!(result["protocolVersion"], "2025-06-18");
    }
}

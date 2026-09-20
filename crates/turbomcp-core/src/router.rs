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
//!     protocol_version: Some("2025-11-25"),
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

/// Deliver a client notification to its handler hook, if it has one.
///
/// Notifications get no response, so nothing is returned and a failing hook is
/// swallowed; unrecognised notifications are ignored, per the spec's
/// instruction to tolerate them.
///
/// This is public because `turbomcp-server` wraps this module with its own
/// router that short-circuits notifications before delegating here. Both call
/// this, so wiring a new hook is a one-line change in one place instead of a
/// change that silently applies to only some transports.
pub async fn dispatch_notification<H: McpHandler>(
    handler: &H,
    request: &JsonRpcIncoming,
    ctx: &RequestContext,
) {
    if request.method == "notifications/roots/list_changed" {
        let _ = handler.on_roots_list_changed(ctx).await;
    }
}

/// Lift `params._meta.progressToken` off an incoming request.
///
/// Per the MCP progress utility the token is `string | number`; anything else
/// is ignored rather than rejected, since a malformed `_meta` entry must not
/// fail an otherwise valid request.
fn progress_token(request: &JsonRpcIncoming) -> Option<Value> {
    let token = request
        .params
        .as_ref()?
        .get("_meta")?
        .get("progressToken")?;
    match token {
        Value::String(_) | Value::Number(_) => Some(token.clone()),
        _ => None,
    }
}

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
    if request.is_notification() {
        dispatch_notification(handler, &request, ctx).await;
        return JsonRpcOutgoing::notification_ack();
    }

    // Any request may carry `_meta.progressToken` to opt into progress
    // notifications, so the token is lifted here — once, centrally — rather
    // than in each method arm or each transport. The context is only cloned
    // when a token is actually present, keeping the common path allocation-free.
    let with_token;
    let ctx = match progress_token(&request) {
        Some(token) => {
            with_token = ctx.clone().with_progress_token(token);
            &with_token
        }
        None => ctx,
    };

    let id = request.id.clone();

    match request.method.as_str() {
        // Initialize handshake
        "initialize" => {
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

            // Echo the client's version when it is one we support, rather than
            // always answering with the newest. The lifecycle spec: "If the
            // server supports the requested protocol version, it MUST respond
            // with the same version. Otherwise, the server MUST respond with
            // another protocol version it supports."
            //
            // `turbomcp-server` negotiates ahead of this and passes the result
            // in `config.protocol_version`, so this only decides the answer for
            // callers that route through core directly — the WASM/Worker entry
            // points, which previously told every client "2025-11-25" no matter
            // what it asked for, and then sent it 11-25-only fields.
            let protocol_version = config.protocol_version.unwrap_or_else(|| {
                params
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .and_then(|requested| {
                        crate::SUPPORTED_VERSIONS
                            .iter()
                            .copied()
                            .find(|supported| *supported == requested)
                    })
                    // Unknown version: answer with one we do support, which is
                    // the spec's second branch. The client decides whether to
                    // proceed or disconnect.
                    .unwrap_or(PROTOCOL_VERSION)
            });
            let info = handler.server_info();
            let result = build_initialize_result(&info, handler, protocol_version);
            JsonRpcOutgoing::success(id, result)
        }

        // Handle both "initialized" and "notifications/initialized"
        "initialized" | "notifications/initialized" => {
            JsonRpcOutgoing::success(id, serde_json::json!({}))
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
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(result_value) => JsonRpcOutgoing::success(id, result_value),
                    Err(e) => JsonRpcOutgoing::error(
                        id,
                        McpError::internal(alloc::format!(
                            "Failed to serialize tool result: {}",
                            e
                        )),
                    ),
                },
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        // Resource methods
        "resources/list" => {
            let resources = handler.list_resources();
            let result = serde_json::json!({ "resources": resources });
            JsonRpcOutgoing::success(id, result)
        }

        "resources/templates/list" => {
            let resource_templates = handler.list_resource_templates();
            let result = serde_json::json!({ "resourceTemplates": resource_templates });
            JsonRpcOutgoing::success(id, result)
        }

        "resources/read" => {
            let params = request.params.unwrap_or_default();
            let uri = params
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            match handler.read_resource(uri, ctx).await {
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(result_value) => JsonRpcOutgoing::success(id, result_value),
                    Err(e) => JsonRpcOutgoing::error(
                        id,
                        McpError::internal(alloc::format!(
                            "Failed to serialize resource result: {}",
                            e
                        )),
                    ),
                },
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
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(result_value) => JsonRpcOutgoing::success(id, result_value),
                    Err(e) => JsonRpcOutgoing::error(
                        id,
                        McpError::internal(alloc::format!(
                            "Failed to serialize prompt result: {}",
                            e
                        )),
                    ),
                },
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        // Task methods (SEP-1686)
        "tasks/list" => {
            let params = request.params.unwrap_or_default();
            let cursor = params.get("cursor").and_then(|v| v.as_str());
            let limit = params
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize);

            match handler.list_tasks(cursor, limit, ctx).await {
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(v) => JsonRpcOutgoing::success(id, v),
                    Err(e) => JsonRpcOutgoing::error(id, McpError::internal(e.to_string())),
                },
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        "tasks/get" => {
            let params = request.params.unwrap_or_default();
            let Some(task_id) = params.get("taskId").and_then(|v| v.as_str()) else {
                return JsonRpcOutgoing::error(id, McpError::invalid_params("Missing taskId"));
            };

            match handler.get_task(task_id, ctx).await {
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(v) => JsonRpcOutgoing::success(id, v),
                    Err(e) => JsonRpcOutgoing::error(id, McpError::internal(e.to_string())),
                },
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        "tasks/cancel" => {
            let params = request.params.unwrap_or_default();
            let Some(task_id) = params.get("taskId").and_then(|v| v.as_str()) else {
                return JsonRpcOutgoing::error(id, McpError::invalid_params("Missing taskId"));
            };

            match handler.cancel_task(task_id, ctx).await {
                Ok(result) => match serde_json::to_value(&result) {
                    Ok(v) => JsonRpcOutgoing::success(id, v),
                    Err(e) => JsonRpcOutgoing::error(id, McpError::internal(e.to_string())),
                },
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        "tasks/result" => {
            let params = request.params.unwrap_or_default();
            let Some(task_id) = params.get("taskId").and_then(|v| v.as_str()) else {
                return JsonRpcOutgoing::error(id, McpError::invalid_params("Missing taskId"));
            };

            match handler.get_task_result(task_id, ctx).await {
                Ok(result) => JsonRpcOutgoing::success(id, result),
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        // Resource subscriptions
        "resources/subscribe" => {
            let params = request.params.unwrap_or_default();
            let Some(uri) = params.get("uri").and_then(|v| v.as_str()) else {
                return JsonRpcOutgoing::error(id, McpError::invalid_params("Missing uri"));
            };
            match handler.subscribe(uri, ctx).await {
                Ok(()) => JsonRpcOutgoing::success(id, serde_json::json!({})),
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        "resources/unsubscribe" => {
            let params = request.params.unwrap_or_default();
            let Some(uri) = params.get("uri").and_then(|v| v.as_str()) else {
                return JsonRpcOutgoing::error(id, McpError::invalid_params("Missing uri"));
            };
            match handler.unsubscribe(uri, ctx).await {
                Ok(()) => JsonRpcOutgoing::success(id, serde_json::json!({})),
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        // Logging
        "logging/setLevel" => {
            let params = request.params.unwrap_or_default();
            let Some(level) = params.get("level").and_then(|v| v.as_str()) else {
                return JsonRpcOutgoing::error(id, McpError::invalid_params("Missing level"));
            };
            match handler.set_log_level(level, ctx).await {
                Ok(()) => JsonRpcOutgoing::success(id, serde_json::json!({})),
                Err(err) => JsonRpcOutgoing::error(id, err),
            }
        }

        // Completions
        "completion/complete" => {
            let params = request.params.unwrap_or_default();
            match handler.complete(params, ctx).await {
                Ok(value) => JsonRpcOutgoing::success(id, value),
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
    let capabilities = match serde_json::to_value(handler.server_capabilities()) {
        Ok(Value::Object(map)) => map,
        Ok(_) | Err(_) => serde_json::Map::new(),
    };

    // Preserve the full ServerInfo payload so initialize responses stay aligned
    // with the shared MCP type definitions as metadata fields evolve.
    let server_info = match serde_json::to_value(info) {
        Ok(Value::Object(map)) => map,
        Ok(_) | Err(_) => {
            let mut fallback = serde_json::Map::new();
            fallback.insert("name".to_string(), serde_json::json!(info.name));
            fallback.insert("version".to_string(), serde_json::json!(info.version));
            fallback
        }
    };

    // Build final result
    let mut result = serde_json::Map::new();
    result.insert(
        "protocolVersion".to_string(),
        serde_json::json!(protocol_version),
    );
    result.insert("capabilities".to_string(), Value::Object(capabilities));
    result.insert("serverInfo".to_string(), Value::Object(server_info));

    // Optional per the schema: emit it only when the server supplies one, so
    // clients can distinguish "no guidance" from "empty guidance".
    if let Some(instructions) = handler.instructions() {
        result.insert("instructions".to_string(), Value::String(instructions));
    }

    Value::Object(result)
}

/// Parse a JSON string into a JSON-RPC incoming request.
///
/// This is a convenience function for parsing incoming messages.
pub fn parse_request(input: &str) -> Result<JsonRpcIncoming, McpError> {
    let value: serde_json::Value =
        serde_json::from_str(input).map_err(|e| McpError::parse_error(e.to_string()))?;
    parse_request_from_value(value)
}

/// Parse a pre-parsed `serde_json::Value` into a JSON-RPC incoming request.
/// Avoids the second JSON parse on the line transports' hot path: callers can
/// parse once into `Value` (to detect server-to-client responses), then use this
/// to convert into the typed request without re-parsing the source string.
pub fn parse_request_from_value(value: serde_json::Value) -> Result<JsonRpcIncoming, McpError> {
    let request: JsonRpcIncoming =
        serde_json::from_value(value).map_err(|e| McpError::invalid_request(e.to_string()))?;
    if !request.is_valid_version() {
        return Err(McpError::invalid_request(
            "jsonrpc field must be exactly \"2.0\"",
        ));
    }
    Ok(request)
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
    use crate::error::{ErrorKind, McpResult};
    use crate::marker::MaybeSend;
    use core::future::Future;
    use std::collections::HashMap;
    use turbomcp_types::{
        Prompt, PromptResult, Resource, ResourceResult, ResourceTemplate, ServerCapabilities,
        ServerTasksCapabilities, ServerTasksRequestsCapabilities, TasksCancelCapabilities,
        TasksListCapabilities, TasksToolsCallCapabilities, TasksToolsCapabilities, Tool,
        ToolResult,
    };

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
    fn test_parse_request_rejects_invalid_jsonrpc_version() {
        let input = r#"{"jsonrpc": "1.0", "id": 1, "method": "ping"}"#;
        let error = parse_request(input).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidRequest);
    }

    #[test]
    fn test_parse_request_rejects_invalid_id_as_invalid_request() {
        let input = r#"{"jsonrpc": "2.0", "id": null, "method": "ping"}"#;
        let error = parse_request(input).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidRequest);
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
    async fn test_route_initialize_preserves_server_info_metadata() {
        #[derive(Clone)]
        struct MetadataHandler;

        #[allow(clippy::manual_async_fn)]
        impl McpHandler for MetadataHandler {
            fn server_info(&self) -> ServerInfo {
                ServerInfo::new("test-router", "1.0.0")
                    .with_title("Test Router")
                    .with_description("Initialize metadata should survive serialization")
                    .with_website_url("https://example.com")
                    .with_icon(
                        turbomcp_types::Icon::new("https://example.com/icon.png")
                            .with_mime_type("image/png"),
                    )
            }

            fn list_tools(&self) -> Vec<Tool> {
                vec![]
            }

            fn list_resources(&self) -> Vec<Resource> {
                vec![]
            }

            fn list_prompts(&self) -> Vec<Prompt> {
                vec![]
            }

            fn call_tool<'a>(
                &'a self,
                _name: &'a str,
                _args: Value,
                _ctx: &'a RequestContext,
            ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a {
                async move { unreachable!("tool calls are not used in this test") }
            }

            fn read_resource<'a>(
                &'a self,
                _uri: &'a str,
                _ctx: &'a RequestContext,
            ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
                async move { unreachable!("resource reads are not used in this test") }
            }

            fn get_prompt<'a>(
                &'a self,
                _name: &'a str,
                _args: Option<Value>,
                _ctx: &'a RequestContext,
            ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
                async move { unreachable!("prompt reads are not used in this test") }
            }
        }

        let handler = MetadataHandler;
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
        let result = response.result.expect("initialize should succeed");
        assert_eq!(result["serverInfo"]["title"], "Test Router");
        assert_eq!(
            result["serverInfo"]["description"],
            "Initialize metadata should survive serialization"
        );
        assert_eq!(result["serverInfo"]["websiteUrl"], "https://example.com");
        assert_eq!(
            result["serverInfo"]["icons"][0]["src"],
            "https://example.com/icon.png"
        );
    }

    #[tokio::test]
    async fn test_route_initialize_uses_handler_capabilities() {
        #[derive(Clone)]
        struct CapabilityHandler;

        #[allow(clippy::manual_async_fn)]
        impl McpHandler for CapabilityHandler {
            fn server_info(&self) -> ServerInfo {
                ServerInfo::new("capability-router", "1.0.0")
            }

            fn server_capabilities(&self) -> ServerCapabilities {
                ServerCapabilities {
                    tasks: Some(ServerTasksCapabilities {
                        list: Some(TasksListCapabilities {}),
                        cancel: Some(TasksCancelCapabilities {}),
                        requests: Some(ServerTasksRequestsCapabilities {
                            tools: Some(TasksToolsCapabilities {
                                call: Some(TasksToolsCallCapabilities {}),
                            }),
                        }),
                    }),
                    extensions: Some(HashMap::from([(
                        "trace".to_string(),
                        serde_json::json!({"version": "1"}),
                    )])),
                    ..Default::default()
                }
            }

            fn list_tools(&self) -> Vec<Tool> {
                vec![]
            }

            fn list_resources(&self) -> Vec<Resource> {
                vec![]
            }

            fn list_prompts(&self) -> Vec<Prompt> {
                vec![]
            }

            fn call_tool<'a>(
                &'a self,
                _name: &'a str,
                _args: Value,
                _ctx: &'a RequestContext,
            ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a {
                async move { unreachable!("tool calls are not used in this test") }
            }

            fn read_resource<'a>(
                &'a self,
                _uri: &'a str,
                _ctx: &'a RequestContext,
            ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
                async move { unreachable!("resource reads are not used in this test") }
            }

            fn get_prompt<'a>(
                &'a self,
                _name: &'a str,
                _args: Option<Value>,
                _ctx: &'a RequestContext,
            ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
                async move { unreachable!("prompt reads are not used in this test") }
            }
        }

        let handler = CapabilityHandler;
        let ctx = RequestContext::stdio();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "DRAFT-2026-v1",
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                },
                "capabilities": {}
            })),
        };

        let response = route_request(&handler, request, &ctx, &RouteConfig::default()).await;
        let result = response.result.expect("initialize should succeed");
        assert!(result["capabilities"]["tasks"]["requests"]["tools"]["call"].is_object());
        assert_eq!(
            result["capabilities"]["extensions"]["trace"]["version"],
            "1"
        );
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
    async fn test_route_resource_templates_list() {
        #[derive(Clone)]
        struct TemplateHandler;

        impl McpHandler for TemplateHandler {
            fn server_info(&self) -> ServerInfo {
                ServerInfo::new("template-router", "1.0.0")
            }

            fn list_tools(&self) -> Vec<Tool> {
                vec![]
            }

            fn list_resources(&self) -> Vec<Resource> {
                vec![]
            }

            fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
                vec![ResourceTemplate::new("file://{path}", "file")]
            }

            fn list_prompts(&self) -> Vec<Prompt> {
                vec![]
            }

            async fn call_tool<'a>(
                &'a self,
                _name: &'a str,
                _args: Value,
                _ctx: &'a RequestContext,
            ) -> McpResult<ToolResult> {
                unreachable!("tool calls are not used in this test")
            }

            async fn read_resource<'a>(
                &'a self,
                _uri: &'a str,
                _ctx: &'a RequestContext,
            ) -> McpResult<ResourceResult> {
                unreachable!("resource reads are not used in this test")
            }

            async fn get_prompt<'a>(
                &'a self,
                _name: &'a str,
                _args: Option<Value>,
                _ctx: &'a RequestContext,
            ) -> McpResult<PromptResult> {
                unreachable!("prompt reads are not used in this test")
            }
        }

        let ctx = RequestContext::stdio();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "resources/templates/list".to_string(),
            params: None,
        };

        let response =
            route_request(&TemplateHandler, request, &ctx, &RouteConfig::default()).await;
        assert!(response.error.is_none());
        let result = response.result.expect("resource templates result");
        let templates = result["resourceTemplates"].as_array().unwrap();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0]["uriTemplate"], "file://{path}");
        assert_eq!(templates[0]["name"], "file");
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
    async fn test_route_request_method_without_id_is_not_sent() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let config = RouteConfig::default();
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "tools/list".to_string(),
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
            protocol_version: Some("2025-11-25"),
        };
        let request = JsonRpcIncoming {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2025-11-25",
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            })),
        };

        let response = route_request(&handler, request, &ctx, &config).await;
        let result = response.result.unwrap();
        assert_eq!(result["protocolVersion"], "2025-11-25");
    }
}

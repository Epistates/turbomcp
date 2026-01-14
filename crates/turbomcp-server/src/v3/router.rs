//! JSON-RPC request routing for McpHandler.
//!
//! This module provides the core request routing logic that maps JSON-RPC
//! requests to McpHandler methods.
//!
//! # MCP Protocol Compliance
//!
//! This router implements the MCP 2025-11-25 specification:
//! - Initialize request validates `clientInfo` and `protocolVersion`
//! - Notifications (requests without `id`) do not receive responses
//! - Capability structure follows the spec format
//! - Error codes follow JSON-RPC 2.0 standard

use serde::{Deserialize, Serialize};
use serde_json::Value;
use turbomcp_types::{McpError, McpResult, ServerInfo};

use super::config::{ClientCapabilities, ServerConfig};
use super::{McpHandler, RequestContext};

/// Client information from initialize request.
#[derive(Debug, Clone, Default)]
pub struct ClientInfo {
    /// Client name
    pub name: String,
    /// Client version
    pub version: String,
}

/// JSON-RPC request structure.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request ID (optional for notifications)
    #[serde(default)]
    pub id: Option<Value>,
    /// Method name
    pub method: String,
    /// Method parameters
    #[serde(default)]
    pub params: Option<Value>,
}

/// JSON-RPC response structure.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    /// Request ID (echoed from request)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// Result (mutually exclusive with error)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Error (mutually exclusive with result)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC error structure.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl From<McpError> for JsonRpcError {
    fn from(err: McpError) -> Self {
        Self {
            code: err.code,
            message: err.message,
            data: err.data,
        }
    }
}

impl JsonRpcResponse {
    /// Create a success response.
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create an error response.
    pub fn error(id: Option<Value>, error: impl Into<JsonRpcError>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error.into()),
        }
    }

    /// Check if this response should be sent over the wire.
    ///
    /// Per JSON-RPC 2.0, notifications (requests without id) should not
    /// receive responses. This method returns false for such cases.
    #[must_use]
    pub fn should_send(&self) -> bool {
        // A response should be sent if:
        // 1. It has an id (normal request-response)
        // 2. It has a result or error (explicit response content)
        self.id.is_some() || self.result.is_some() || self.error.is_some()
    }
}

/// Route a JSON-RPC request to the appropriate handler method.
///
/// This is the simple routing function that uses default configuration.
/// For more control, use `route_request_with_config`.
pub async fn route_request<H: McpHandler>(
    handler: &H,
    request: JsonRpcRequest,
    ctx: &RequestContext,
) -> JsonRpcResponse {
    route_request_with_config(handler, request, ctx, None).await
}

/// Route a JSON-RPC request with custom server configuration.
///
/// This function provides full control over protocol negotiation,
/// capability validation, and other server behavior.
pub async fn route_request_with_config<H: McpHandler>(
    handler: &H,
    request: JsonRpcRequest,
    ctx: &RequestContext,
    config: Option<&ServerConfig>,
) -> JsonRpcResponse {
    let id = request.id.clone();

    match request.method.as_str() {
        // Initialization with protocol negotiation and capability validation
        "initialize" => {
            let params = request.params.clone().unwrap_or_default();

            // CRITICAL-001: Validate clientInfo is present (MCP spec requirement)
            let client_info = params.get("clientInfo");
            if client_info.is_none() {
                return JsonRpcResponse::error(
                    id,
                    McpError::invalid_params("Missing required field: clientInfo"),
                );
            }
            let client_info = client_info.unwrap();

            // Validate clientInfo has required fields
            let client_name = client_info.get("name").and_then(|v| v.as_str());
            let client_version = client_info.get("version").and_then(|v| v.as_str());
            if client_name.is_none() || client_version.is_none() {
                return JsonRpcResponse::error(
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
                Some(version) => version,
                None => {
                    return JsonRpcResponse::error(
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
                    return JsonRpcResponse::error(
                        id,
                        McpError::invalid_request(format!(
                            "Missing required client capabilities: {}",
                            missing.join(", ")
                        )),
                    );
                }
            }

            let info = handler.server_info();
            let result = initialize_result_with_version(&info, handler, &negotiated_version);
            JsonRpcResponse::success(id, result)
        }
        // CRITICAL-004: Handle both "initialized" and "notifications/initialized"
        // Per JSON-RPC 2.0, notifications (no id) should not receive responses
        "initialized" | "notifications/initialized" => {
            // This is a notification - only respond if there's an id (for compatibility)
            if id.is_some() {
                JsonRpcResponse::success(id, serde_json::json!({}))
            } else {
                // Return a marker response that indicates no response should be sent
                // The transport layer should check for this and skip sending
                JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: None,
                }
            }
        }

        // Tool methods
        "tools/list" => {
            let tools = handler.list_tools();
            let result = serde_json::json!({
                "tools": tools
            });
            JsonRpcResponse::success(id, result)
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
                    JsonRpcResponse::success(id, result_value)
                }
                Err(err) => JsonRpcResponse::error(id, err),
            }
        }

        // Resource methods
        "resources/list" => {
            let resources = handler.list_resources();
            let result = serde_json::json!({
                "resources": resources
            });
            JsonRpcResponse::success(id, result)
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
                    JsonRpcResponse::success(id, result_value)
                }
                Err(err) => JsonRpcResponse::error(id, err),
            }
        }

        // Prompt methods
        "prompts/list" => {
            let prompts = handler.list_prompts();
            let result = serde_json::json!({
                "prompts": prompts
            });
            JsonRpcResponse::success(id, result)
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
                    JsonRpcResponse::success(id, result_value)
                }
                Err(err) => JsonRpcResponse::error(id, err),
            }
        }

        // Ping
        "ping" => JsonRpcResponse::success(id, serde_json::json!({})),

        // Unknown method
        _ => JsonRpcResponse::error(id, McpError::method_not_found(&request.method)),
    }
}

/// Generate the initialize result with a specific protocol version.
///
/// # MCP Spec Compliance (CRITICAL-002)
///
/// The capabilities object follows the MCP 2025-11-25 specification:
/// - Each capability is an object (not boolean)
/// - Capabilities are only included if the server supports them
/// - Sub-properties like `listChanged` indicate notification support
fn initialize_result_with_version<H: McpHandler>(
    info: &ServerInfo,
    handler: &H,
    protocol_version: &str,
) -> Value {
    let has_tools = !handler.list_tools().is_empty();
    let has_resources = !handler.list_resources().is_empty();
    let has_prompts = !handler.list_prompts().is_empty();

    // Build capabilities object per MCP spec
    // Only include capabilities that are actually supported
    let mut capabilities = serde_json::Map::new();

    if has_tools {
        // Tools capability with listChanged notification support
        capabilities.insert(
            "tools".to_string(),
            serde_json::json!({
                "listChanged": true
            }),
        );
    }

    if has_resources {
        // Resources capability with listChanged notification support
        capabilities.insert(
            "resources".to_string(),
            serde_json::json!({
                "listChanged": true
            }),
        );
    }

    if has_prompts {
        // Prompts capability with listChanged notification support
        capabilities.insert(
            "prompts".to_string(),
            serde_json::json!({
                "listChanged": true
            }),
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

/// Parse a JSON string into a JSON-RPC request.
pub fn parse_request(input: &str) -> McpResult<JsonRpcRequest> {
    serde_json::from_str(input).map_err(|e| McpError::parse_error(e.to_string()))
}

/// Serialize a JSON-RPC response to a string.
pub fn serialize_response(response: &JsonRpcResponse) -> McpResult<String> {
    serde_json::to_string(response).map_err(|e| McpError::internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_types::{Prompt, PromptResult, Resource, ResourceResult, Tool, ToolResult};

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
        let response = JsonRpcResponse::success(Some(serde_json::json!(1)), serde_json::json!({}));
        let serialized = serialize_response(&response).unwrap();
        assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
        assert!(serialized.contains("\"id\":1"));
    }

    #[tokio::test]
    async fn test_route_initialize() {
        let handler = TestHandler;
        let ctx = RequestContext::new();
        let request = JsonRpcRequest {
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
        let ctx = RequestContext::new();
        let request = JsonRpcRequest {
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
        assert_eq!(error.code, McpError::INVALID_PARAMS);
        assert!(error.message.contains("clientInfo"));
    }

    #[tokio::test]
    async fn test_route_initialized_notification() {
        let handler = TestHandler;
        let ctx = RequestContext::new();
        // Notification has no id
        let request = JsonRpcRequest {
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
        let ctx = RequestContext::new();
        let request = JsonRpcRequest {
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
        let ctx = RequestContext::new();
        let request = JsonRpcRequest {
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
        let ctx = RequestContext::new();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "unknown/method".to_string(),
            params: None,
        };

        let response = route_request(&handler, request, &ctx).await;
        assert!(response.result.is_none());
        assert!(response.error.is_some());

        let error = response.error.unwrap();
        assert_eq!(error.code, McpError::METHOD_NOT_FOUND);
    }
}

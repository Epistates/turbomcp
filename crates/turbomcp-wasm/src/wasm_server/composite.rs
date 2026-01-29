//! Server composition through handler mounting.
//!
//! This module enables composing multiple MCP servers into a single server,
//! with automatic namespacing through prefixes. This allows building modular
//! servers from smaller, focused handlers.
//!
//! # Security
//!
//! The composite server includes secure CORS handling:
//!
//! - Echoes the request `Origin` header instead of using wildcard `*`
//! - Adds `Vary: Origin` header for proper caching behavior
//! - Falls back to `*` only for non-browser clients (no Origin header)
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::{McpServer, CompositeServer};
//!
//! // Create individual servers
//! let weather = McpServer::builder("weather", "1.0.0")
//!     .tool("get_forecast", "Get weather forecast", get_forecast)
//!     .build();
//!
//! let news = McpServer::builder("news", "1.0.0")
//!     .tool("get_headlines", "Get news headlines", get_headlines)
//!     .build();
//!
//! // Compose into a single server
//! let server = CompositeServer::builder("main-server", "1.0.0")
//!     .mount(weather, "weather")  // weather_get_forecast
//!     .mount(news, "news")        // news_get_headlines
//!     .build();
//!
//! // All tools are namespaced: "weather_get_forecast", "news_get_headlines"
//! server.handle(req).await
//! ```

use std::sync::Arc;

use turbomcp_core::types::capabilities::ServerCapabilities;
use turbomcp_core::types::core::Implementation;
use worker::{Headers, Request, Response};

use super::context::RequestContext;
use super::server::McpServer;
use super::types::{JsonRpcRequest, JsonRpcResponse};

/// A composite server that mounts multiple MCP servers with prefixes.
///
/// This enables modular server design by combining multiple servers into
/// a single namespace. Each mounted server's tools, resources, and prompts
/// are automatically prefixed to avoid naming conflicts.
///
/// # Namespacing Rules
///
/// - **Tools**: `{prefix}_{tool_name}` (e.g., `weather_get_forecast`)
/// - **Resources**: `{prefix}://{original_uri}` (e.g., `weather://api/forecast`)
/// - **Prompts**: `{prefix}_{prompt_name}` (e.g., `weather_forecast_prompt`)
///
/// # Example
///
/// ```ignore
/// let composite = CompositeServer::builder("my-gateway", "1.0.0")
///     .mount(weather_server, "weather")
///     .mount(news_server, "news")
///     .build();
///
/// // Handle incoming request
/// let response = composite.handle(request).await?;
/// ```
#[derive(Clone)]
pub struct CompositeServer {
    name: String,
    version: String,
    description: Option<String>,
    mounted: Arc<Vec<MountedServer>>,
}

impl std::fmt::Debug for CompositeServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeServer")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("description", &self.description)
            .field("mounted_count", &self.mounted.len())
            .finish()
    }
}

/// Internal struct to hold a mounted server with its prefix.
#[derive(Clone)]
struct MountedServer {
    prefix: String,
    server: McpServer,
}

/// Builder for creating a composite server.
pub struct CompositeServerBuilder {
    name: String,
    version: String,
    description: Option<String>,
    mounted: Vec<MountedServer>,
}

impl std::fmt::Debug for CompositeServerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeServerBuilder")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("description", &self.description)
            .field("mounted_count", &self.mounted.len())
            .finish()
    }
}

impl CompositeServerBuilder {
    /// Create a new composite server builder.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: None,
            mounted: Vec::new(),
        }
    }

    /// Set the server description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Mount a server with the given prefix.
    ///
    /// All tools, resources, and prompts from the server will be namespaced
    /// with the prefix.
    ///
    /// # Panics
    ///
    /// Panics if a server with the same prefix is already mounted. This prevents
    /// silent shadowing of tools/resources/prompts which could lead to confusing
    /// runtime behavior.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let server = CompositeServer::builder("main", "1.0.0")
    ///     .mount(weather_server, "weather")
    ///     .mount(news_server, "news")
    ///     .build();
    /// ```
    #[must_use]
    pub fn mount(mut self, server: McpServer, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();

        // Validate no duplicate prefixes
        if self.mounted.iter().any(|m| m.prefix == prefix) {
            panic!(
                "CompositeServer: duplicate prefix '{}' - each mounted server must have a unique prefix",
                prefix
            );
        }

        self.mounted.push(MountedServer { prefix, server });
        self
    }

    /// Try to mount a server with the given prefix, returning an error on duplicate.
    ///
    /// This is the fallible version of [`mount`](Self::mount) for cases where
    /// you want to handle duplicate prefixes gracefully rather than panicking.
    ///
    /// # Errors
    ///
    /// Returns an error if a server with the same prefix is already mounted.
    pub fn try_mount(
        mut self,
        server: McpServer,
        prefix: impl Into<String>,
    ) -> Result<Self, String> {
        let prefix = prefix.into();

        if self.mounted.iter().any(|m| m.prefix == prefix) {
            return Err(format!(
                "duplicate prefix '{}' - each mounted server must have a unique prefix",
                prefix
            ));
        }

        self.mounted.push(MountedServer { prefix, server });
        Ok(self)
    }

    /// Build the composite server.
    pub fn build(self) -> CompositeServer {
        CompositeServer {
            name: self.name,
            version: self.version,
            description: self.description,
            mounted: Arc::new(self.mounted),
        }
    }
}

impl CompositeServer {
    /// Create a new composite server builder.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let server = CompositeServer::builder("my-gateway", "1.0.0")
    ///     .mount(server1, "prefix1")
    ///     .build();
    /// ```
    pub fn builder(name: impl Into<String>, version: impl Into<String>) -> CompositeServerBuilder {
        CompositeServerBuilder::new(name, version)
    }

    /// Get the number of mounted servers.
    pub fn server_count(&self) -> usize {
        self.mounted.len()
    }

    /// Get all mounted prefixes.
    pub fn prefixes(&self) -> Vec<&str> {
        self.mounted.iter().map(|m| m.prefix.as_str()).collect()
    }

    /// Handle an incoming Cloudflare Worker request.
    ///
    /// This routes requests to the appropriate mounted server based on
    /// the namespaced tool/resource/prompt names.
    pub async fn handle(&self, mut req: Request) -> worker::Result<Response> {
        // SECURITY: Extract Origin header early for CORS responses.
        // We echo this back instead of using wildcard "*".
        let request_origin = req.headers().get("origin").ok().flatten();
        let origin_ref = request_origin.as_deref();

        // Handle CORS preflight
        if req.method() == worker::Method::Options {
            return self.cors_preflight_response(origin_ref);
        }

        // Parse JSON-RPC request
        let body = req.text().await?;
        let rpc_request: JsonRpcRequest = match serde_json::from_str(&body) {
            Ok(r) => r,
            Err(e) => {
                return self.json_rpc_error_response(
                    None,
                    -32700,
                    &format!("Parse error: {}", e),
                    origin_ref,
                );
            }
        };

        let id = rpc_request.id.clone();

        // Route based on method
        let result = match rpc_request.method.as_str() {
            "initialize" => self.handle_initialize(&rpc_request).await,
            "tools/list" => self.handle_list_tools(),
            "tools/call" => self.handle_call_tool(&rpc_request).await,
            "resources/list" => self.handle_list_resources(),
            "resources/read" => self.handle_read_resource(&rpc_request).await,
            "resources/templates/list" => self.handle_list_resource_templates(),
            "prompts/list" => self.handle_list_prompts(),
            "prompts/get" => self.handle_get_prompt(&rpc_request).await,
            method => {
                return self.json_rpc_error_response(
                    id.clone(),
                    -32601,
                    &format!("Method not found: {}", method),
                    origin_ref,
                );
            }
        };

        match result {
            Ok(value) => self.json_rpc_success_response(id, value, origin_ref),
            Err(e) => self.json_rpc_error_response(id, -32603, &e, origin_ref),
        }
    }

    // =========================================================================
    // Namespacing Helpers
    // =========================================================================

    /// Prefix a tool name.
    fn prefix_tool_name(prefix: &str, name: &str) -> String {
        format!("{}_{}", prefix, name)
    }

    /// Prefix a resource URI.
    fn prefix_resource_uri(prefix: &str, uri: &str) -> String {
        format!("{}://{}", prefix, uri)
    }

    /// Prefix a prompt name.
    fn prefix_prompt_name(prefix: &str, name: &str) -> String {
        format!("{}_{}", prefix, name)
    }

    /// Parse a prefixed tool name into (prefix, original_name).
    fn parse_prefixed_tool(name: &str) -> Option<(&str, &str)> {
        name.split_once('_')
    }

    /// Parse a prefixed resource URI into (prefix, original_uri).
    fn parse_prefixed_uri(uri: &str) -> Option<(&str, &str)> {
        uri.split_once("://")
    }

    /// Parse a prefixed prompt name into (prefix, original_name).
    fn parse_prefixed_prompt(name: &str) -> Option<(&str, &str)> {
        name.split_once('_')
    }

    /// Find a mounted server by prefix.
    fn find_server(&self, prefix: &str) -> Option<&MountedServer> {
        self.mounted.iter().find(|m| m.prefix == prefix)
    }

    // =========================================================================
    // Request Handlers
    // =========================================================================

    async fn handle_initialize(&self, _req: &JsonRpcRequest) -> Result<serde_json::Value, String> {
        let capabilities = self.aggregate_capabilities();

        let server_info = Implementation {
            name: self.name.clone(),
            title: None,
            description: self.description.clone(),
            version: self.version.clone(),
            icon: None,
        };

        Ok(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": capabilities,
            "serverInfo": server_info
        }))
    }

    fn handle_list_tools(&self) -> Result<serde_json::Value, String> {
        let mut tools = Vec::new();

        for mounted in self.mounted.iter() {
            for tool in mounted.server.tools() {
                let mut prefixed = tool.clone();
                prefixed.name = Self::prefix_tool_name(&mounted.prefix, &tool.name);
                tools.push(prefixed);
            }
        }

        Ok(serde_json::json!({
            "tools": tools
        }))
    }

    async fn handle_call_tool(&self, req: &JsonRpcRequest) -> Result<serde_json::Value, String> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| "Missing params".to_string())?;

        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing tool name".to_string())?;

        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        // Parse prefixed name
        let (prefix, original_name) =
            Self::parse_prefixed_tool(name).ok_or_else(|| format!("Tool not found: {}", name))?;

        // Find the mounted server
        let mounted = self
            .find_server(prefix)
            .ok_or_else(|| format!("Tool not found: {}", name))?;

        // Create context
        let ctx = Arc::new(RequestContext::new());

        // Call the tool
        let result = mounted
            .server
            .call_tool_internal(original_name, args, ctx)
            .await?;

        Ok(serde_json::json!({
            "content": result.content,
            "isError": result.is_error
        }))
    }

    fn handle_list_resources(&self) -> Result<serde_json::Value, String> {
        let mut resources = Vec::new();

        for mounted in self.mounted.iter() {
            for resource in mounted.server.resources() {
                let mut prefixed = resource.clone();
                prefixed.uri = Self::prefix_resource_uri(&mounted.prefix, &resource.uri);
                resources.push(prefixed);
            }
        }

        Ok(serde_json::json!({
            "resources": resources
        }))
    }

    fn handle_list_resource_templates(&self) -> Result<serde_json::Value, String> {
        let mut templates = Vec::new();

        for mounted in self.mounted.iter() {
            for template in mounted.server.resource_templates() {
                let mut prefixed = template.clone();
                prefixed.uri_template =
                    Self::prefix_resource_uri(&mounted.prefix, &template.uri_template);
                templates.push(prefixed);
            }
        }

        Ok(serde_json::json!({
            "resourceTemplates": templates
        }))
    }

    async fn handle_read_resource(
        &self,
        req: &JsonRpcRequest,
    ) -> Result<serde_json::Value, String> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| "Missing params".to_string())?;

        let uri = params
            .get("uri")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing resource URI".to_string())?;

        // Parse prefixed URI
        let (prefix, original_uri) =
            Self::parse_prefixed_uri(uri).ok_or_else(|| format!("Resource not found: {}", uri))?;

        // Find the mounted server
        let mounted = self
            .find_server(prefix)
            .ok_or_else(|| format!("Resource not found: {}", uri))?;

        // Create context
        let ctx = Arc::new(RequestContext::new());

        // Read the resource
        let result = mounted
            .server
            .read_resource_internal(original_uri, ctx)
            .await?;

        Ok(serde_json::json!({
            "contents": result.contents
        }))
    }

    fn handle_list_prompts(&self) -> Result<serde_json::Value, String> {
        let mut prompts = Vec::new();

        for mounted in self.mounted.iter() {
            for prompt in mounted.server.prompts() {
                let mut prefixed = prompt.clone();
                prefixed.name = Self::prefix_prompt_name(&mounted.prefix, &prompt.name);
                prompts.push(prefixed);
            }
        }

        Ok(serde_json::json!({
            "prompts": prompts
        }))
    }

    async fn handle_get_prompt(&self, req: &JsonRpcRequest) -> Result<serde_json::Value, String> {
        let params = req
            .params
            .as_ref()
            .ok_or_else(|| "Missing params".to_string())?;

        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing prompt name".to_string())?;

        let args = params.get("arguments").cloned();

        // Parse prefixed name
        let (prefix, original_name) = Self::parse_prefixed_prompt(name)
            .ok_or_else(|| format!("Prompt not found: {}", name))?;

        // Find the mounted server
        let mounted = self
            .find_server(prefix)
            .ok_or_else(|| format!("Prompt not found: {}", name))?;

        // Create context
        let ctx = Arc::new(RequestContext::new());

        // Get the prompt
        let result = mounted
            .server
            .get_prompt_internal(original_name, args, ctx)
            .await?;

        Ok(serde_json::json!({
            "description": result.description,
            "messages": result.messages
        }))
    }

    // =========================================================================
    // Capability Aggregation
    // =========================================================================

    fn aggregate_capabilities(&self) -> ServerCapabilities {
        let has_tools = self.mounted.iter().any(|m| !m.server.tools.is_empty());
        let has_resources = self
            .mounted
            .iter()
            .any(|m| !m.server.resources.is_empty() || !m.server.resource_templates.is_empty());
        let has_prompts = self.mounted.iter().any(|m| !m.server.prompts.is_empty());

        ServerCapabilities {
            experimental: None,
            logging: None,
            tasks: None,
            prompts: if has_prompts {
                Some(turbomcp_core::types::capabilities::PromptsCapability {
                    list_changed: Some(false),
                })
            } else {
                None
            },
            resources: if has_resources {
                Some(turbomcp_core::types::capabilities::ResourcesCapability {
                    subscribe: Some(false),
                    list_changed: Some(false),
                })
            } else {
                None
            },
            tools: if has_tools {
                Some(turbomcp_core::types::capabilities::ToolsCapability {
                    list_changed: Some(false),
                })
            } else {
                None
            },
        }
    }

    // =========================================================================
    // Response Helpers
    // =========================================================================

    /// Create CORS headers for responses.
    ///
    /// SECURITY: Echoes the request Origin header instead of using wildcard `*`.
    fn cors_headers(&self, request_origin: Option<&str>) -> Headers {
        let headers = Headers::new();
        // SECURITY: Echo the request origin instead of using wildcard.
        let origin = request_origin.unwrap_or("*");
        let _ = headers.set("Access-Control-Allow-Origin", origin);
        if request_origin.is_some() {
            let _ = headers.set("Vary", "Origin");
        }
        let _ = headers.set("Access-Control-Allow-Methods", "POST, OPTIONS");
        let _ = headers.set("Access-Control-Allow-Headers", "Content-Type");
        let _ = headers.set("Access-Control-Max-Age", "86400");
        headers
    }

    fn cors_preflight_response(&self, request_origin: Option<&str>) -> worker::Result<Response> {
        Ok(Response::empty()?
            .with_status(204)
            .with_headers(self.cors_headers(request_origin)))
    }

    fn json_rpc_success_response(
        &self,
        id: Option<serde_json::Value>,
        result: serde_json::Value,
        request_origin: Option<&str>,
    ) -> worker::Result<Response> {
        let response = JsonRpcResponse::success(id, result);
        let json =
            serde_json::to_string(&response).map_err(|e| worker::Error::from(e.to_string()))?;

        let headers = self.cors_headers(request_origin);
        let _ = headers.set("Content-Type", "application/json");

        Ok(Response::ok(json)?.with_headers(headers))
    }

    fn json_rpc_error_response(
        &self,
        id: Option<serde_json::Value>,
        code: i32,
        message: &str,
        request_origin: Option<&str>,
    ) -> worker::Result<Response> {
        let response = JsonRpcResponse::error(id, code, message);
        let json =
            serde_json::to_string(&response).map_err(|e| worker::Error::from(e.to_string()))?;

        let headers = self.cors_headers(request_origin);
        let _ = headers.set("Content-Type", "application/json");

        Ok(Response::ok(json)?.with_headers(headers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_weather_server() -> McpServer {
        McpServer::builder("weather", "1.0.0")
            .description("Weather service")
            .tool_raw("get_forecast", "Get weather forecast", |_args| async {
                "Sunny, 72°F".to_string()
            })
            .build()
    }

    fn create_test_news_server() -> McpServer {
        McpServer::builder("news", "1.0.0")
            .description("News service")
            .tool_raw("get_headlines", "Get news headlines", |_args| async {
                "Breaking: AI advances continue".to_string()
            })
            .build()
    }

    #[test]
    fn test_composite_builder() {
        let weather = create_test_weather_server();
        let news = create_test_news_server();

        let composite = CompositeServer::builder("main", "1.0.0")
            .description("Main gateway")
            .mount(weather, "weather")
            .mount(news, "news")
            .build();

        assert_eq!(composite.server_count(), 2);
        assert_eq!(composite.prefixes(), vec!["weather", "news"]);
    }

    #[test]
    fn test_list_tools_prefixed() {
        let weather = create_test_weather_server();
        let news = create_test_news_server();

        let composite = CompositeServer::builder("main", "1.0.0")
            .mount(weather, "weather")
            .mount(news, "news")
            .build();

        let result = composite.handle_list_tools().unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();

        assert_eq!(tools.len(), 2);

        let tool_names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .collect();

        assert!(tool_names.contains(&"weather_get_forecast"));
        assert!(tool_names.contains(&"news_get_headlines"));
    }

    #[test]
    #[should_panic(expected = "duplicate prefix 'weather'")]
    fn test_duplicate_prefix_panics() {
        let weather1 = create_test_weather_server();
        let weather2 = create_test_weather_server();

        let _composite = CompositeServer::builder("main", "1.0.0")
            .mount(weather1, "weather")
            .mount(weather2, "weather"); // Duplicate!
    }

    #[test]
    fn test_try_mount_duplicate_returns_error() {
        let weather1 = create_test_weather_server();
        let weather2 = create_test_weather_server();

        let result = CompositeServer::builder("main", "1.0.0")
            .try_mount(weather1, "weather")
            .unwrap()
            .try_mount(weather2, "weather");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("duplicate prefix"));
    }

    #[test]
    fn test_try_mount_success() {
        let weather = create_test_weather_server();
        let news = create_test_news_server();

        let composite = CompositeServer::builder("main", "1.0.0")
            .try_mount(weather, "weather")
            .unwrap()
            .try_mount(news, "news")
            .unwrap()
            .build();

        assert_eq!(composite.server_count(), 2);
    }

    #[tokio::test]
    async fn test_call_tool_routed() {
        let weather = create_test_weather_server();
        let news = create_test_news_server();

        let composite = CompositeServer::builder("main", "1.0.0")
            .mount(weather, "weather")
            .mount(news, "news")
            .build();

        // Call weather tool
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "weather_get_forecast",
                "arguments": {}
            })),
        };

        let result = composite.handle_call_tool(&req).await.unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        assert!(!content.is_empty());

        // Call news tool
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(2)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "news_get_headlines",
                "arguments": {}
            })),
        };

        let result = composite.handle_call_tool(&req).await.unwrap();
        let content = result.get("content").unwrap().as_array().unwrap();
        assert!(!content.is_empty());
    }

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let weather = create_test_weather_server();

        let composite = CompositeServer::builder("main", "1.0.0")
            .mount(weather, "weather")
            .build();

        // Unknown prefix
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "unknown_tool",
                "arguments": {}
            })),
        };

        let result = composite.handle_call_tool(&req).await;
        assert!(result.is_err());

        // No underscore
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "notool",
                "arguments": {}
            })),
        };

        let result = composite.handle_call_tool(&req).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_prefixed_tool() {
        assert_eq!(
            CompositeServer::parse_prefixed_tool("weather_get_forecast"),
            Some(("weather", "get_forecast"))
        );
        assert_eq!(
            CompositeServer::parse_prefixed_tool("a_b_c"),
            Some(("a", "b_c"))
        );
        assert_eq!(CompositeServer::parse_prefixed_tool("notool"), None);
    }

    #[test]
    fn test_parse_prefixed_uri() {
        assert_eq!(
            CompositeServer::parse_prefixed_uri("weather://api/current"),
            Some(("weather", "api/current"))
        );
        assert_eq!(
            CompositeServer::parse_prefixed_uri("news://feed/top"),
            Some(("news", "feed/top"))
        );
        assert_eq!(CompositeServer::parse_prefixed_uri("noproto"), None);
    }

    #[test]
    fn test_aggregate_capabilities() {
        let weather = create_test_weather_server();
        let news = create_test_news_server();

        let composite = CompositeServer::builder("main", "1.0.0")
            .mount(weather, "weather")
            .mount(news, "news")
            .build();

        let caps = composite.aggregate_capabilities();
        assert!(caps.tools.is_some());
        assert!(caps.resources.is_none()); // No resources in test servers
        assert!(caps.prompts.is_none()); // No prompts in test servers
    }
}

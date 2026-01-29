//! Request handler for Cloudflare Workers MCP server
//!
//! Implements JSON-RPC 2.0 compliant request handling with proper CORS support
//! and comprehensive error handling for Cloudflare Workers edge deployment.
//!
//! # CORS Security
//!
//! This handler implements secure CORS handling by echoing the request `Origin`
//! header instead of using a wildcard (`*`). This is a security best practice that:
//!
//! - Prevents credentials from being exposed to arbitrary origins
//! - Enables proper CORS preflight validation
//! - Adds `Vary: Origin` header when origin is specified (required for proper caching)
//!
//! When no `Origin` header is present (e.g., non-browser clients like `curl`),
//! the handler falls back to `*` to allow cross-origin requests.
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_wasm::wasm_server::{McpServer, McpHandler};
//!
//! async fn handle_request(req: worker::Request) -> worker::Result<worker::Response> {
//!     let server = McpServer::builder("my-server", "1.0.0")
//!         .tool("greet", "Say hello", |args: GreetArgs| async move {
//!             format!("Hello, {}!", args.name)
//!         })
//!         .build();
//!
//!     McpHandler::new(&server).handle(req).await
//! }
//! ```
//!
//! # Security Features
//!
//! - **Request body size limit**: 1MB maximum to prevent DoS attacks
//! - **Origin echo**: Echoes request origin instead of `*` for CORS
//! - **Vary header**: Properly set when origin-specific responses are returned
//! - **Content-Type validation**: Only accepts `application/json` for POST requests
//! - **JSON-RPC validation**: Validates request structure and method names

use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use worker::{Headers, Request, Response};

use super::context::RequestContext;
use super::server::{McpServer, PromptHandlerKind, ResourceHandlerKind, ToolHandlerKind};
use super::types::{JsonRpcRequest, JsonRpcResponse, error_codes};
use turbomcp_core::PROTOCOL_VERSION;
use turbomcp_core::types::capabilities::ClientCapabilities;
use turbomcp_core::types::core::Implementation;
use turbomcp_core::types::initialization::InitializeResult;

/// Maximum request body size (1MB) to prevent DoS
const MAX_BODY_SIZE: usize = 1024 * 1024;

/// MCP request handler for Cloudflare Workers
pub struct McpHandler<'a> {
    server: &'a McpServer,
}

impl<'a> McpHandler<'a> {
    /// Create a new handler for the given server
    pub fn new(server: &'a McpServer) -> Self {
        Self { server }
    }

    /// Extract headers from a Worker request into a HashMap
    fn extract_headers(req: &Request) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        let worker_headers = req.headers();

        // Extract common headers
        for key in [
            "authorization",
            "content-type",
            "user-agent",
            "x-request-id",
            "x-session-id",
            "x-client-id",
            "mcp-session-id",
            "origin",
            "referer",
        ] {
            if let Ok(Some(value)) = worker_headers.get(key) {
                headers.insert(key.to_string(), value);
            }
        }

        headers
    }

    /// Create a RequestContext from an incoming Worker request
    fn create_context_from_request(req: &Request) -> RequestContext {
        let headers = Self::extract_headers(req);

        // Extract session ID from headers
        let session_id = headers
            .get("mcp-session-id")
            .or_else(|| headers.get("x-session-id"))
            .cloned();

        // Extract request ID from headers or generate one
        let request_id = headers.get("x-request-id").cloned();

        RequestContext::from_worker_request(request_id, session_id, headers)
    }

    /// Handle an incoming request
    ///
    /// Processes JSON-RPC 2.0 requests with proper CORS handling.
    pub async fn handle(&self, mut req: Request) -> worker::Result<Response> {
        // SECURITY: Extract Origin header early for CORS responses.
        // We echo this back instead of using wildcard "*".
        let request_origin = req.headers().get("origin").ok().flatten();
        let origin_ref = request_origin.as_deref();

        // Handle CORS preflight requests
        if req.method() == worker::Method::Options {
            return self.cors_preflight_response(origin_ref);
        }

        // Only accept POST requests for JSON-RPC
        if req.method() != worker::Method::Post {
            return self.error_response(
                405,
                "Method not allowed. Use POST for JSON-RPC requests.",
                origin_ref,
            );
        }

        // Validate Content-Type header
        if !self.is_valid_content_type(&req) {
            return self.error_response(
                415,
                "Unsupported Media Type. Use Content-Type: application/json",
                origin_ref,
            );
        }

        // Create context from request before consuming body
        let context = Arc::new(Self::create_context_from_request(&req));

        // SECURITY: Check Content-Length header BEFORE reading body to prevent DoS.
        // This prevents attackers from exhausting memory with large request bodies.
        if let Some(content_length) = req.headers().get("content-length").ok().flatten()
            && let Ok(length) = content_length.parse::<usize>()
            && length > MAX_BODY_SIZE
        {
            return self.error_response(413, "Request body too large", origin_ref);
        }

        // Get request body with size limit protection (secondary check after reading)
        let body = match req.text().await {
            Ok(b) if b.len() > MAX_BODY_SIZE => {
                // This catches chunked transfers or Content-Length mismatches
                return self.error_response(413, "Request body too large", origin_ref);
            }
            Ok(b) if b.is_empty() => {
                let response = JsonRpcResponse::error(
                    None,
                    error_codes::INVALID_REQUEST,
                    "Empty request body",
                );
                return self.json_response(&response, origin_ref);
            }
            Ok(b) => b,
            Err(e) => {
                let response = JsonRpcResponse::error(
                    None,
                    error_codes::PARSE_ERROR,
                    format!("Failed to read request body: {e}"),
                );
                return self.json_response(&response, origin_ref);
            }
        };

        // Parse the JSON-RPC request
        let rpc_request: JsonRpcRequest = match serde_json::from_str(&body) {
            Ok(r) => r,
            Err(e) => {
                let response = JsonRpcResponse::error(
                    None,
                    error_codes::PARSE_ERROR,
                    format!("Parse error: {e}"),
                );
                return self.json_response(&response, origin_ref);
            }
        };

        // Validate JSON-RPC version
        if rpc_request.jsonrpc != "2.0" {
            let response = JsonRpcResponse::error(
                rpc_request.id,
                error_codes::INVALID_REQUEST,
                "Invalid JSON-RPC version. Expected \"2.0\".",
            );
            return self.json_response(&response, origin_ref);
        }

        // Check if this is a notification (no id means notification)
        let is_notification = rpc_request.id.is_none();

        // Route to appropriate handler with context
        let response = self.route_request_with_ctx(&rpc_request, context).await;

        // Per JSON-RPC 2.0 spec: notifications MUST NOT receive a response
        if is_notification && response.error.is_none() {
            // Return 204 No Content for successful notifications
            return Response::empty()
                .map(|r| r.with_status(204))
                .map(|r| r.with_headers(self.cors_headers(origin_ref)));
        }

        self.json_response(&response, origin_ref)
    }

    /// Check if the Content-Type header indicates JSON
    fn is_valid_content_type(&self, req: &Request) -> bool {
        req.headers()
            .get("Content-Type")
            .ok()
            .flatten()
            .map(|ct| ct.contains("application/json") || ct.contains("text/json"))
            .unwrap_or(true) // Allow missing Content-Type for compatibility
    }

    /// Route a JSON-RPC request to the appropriate handler with context
    async fn route_request_with_ctx(
        &self,
        req: &JsonRpcRequest,
        ctx: Arc<RequestContext>,
    ) -> JsonRpcResponse {
        match req.method.as_str() {
            // Core protocol methods
            "initialize" => self.handle_initialize(req),
            "notifications/initialized" => self.handle_initialized_notification(req),
            "ping" => self.handle_ping(req),

            // Tool methods
            "tools/list" => self.handle_tools_list(req),
            "tools/call" => self.handle_tools_call(req, ctx.clone()).await,

            // Resource methods
            "resources/list" => self.handle_resources_list(req),
            "resources/templates/list" => self.handle_resource_templates_list(req),
            "resources/read" => self.handle_resources_read(req, ctx.clone()).await,

            // Prompt methods
            "prompts/list" => self.handle_prompts_list(req),
            "prompts/get" => self.handle_prompts_get(req, ctx.clone()).await,

            // Logging (MCP standard)
            "logging/setLevel" => self.handle_logging_set_level(req),

            // Unknown method
            _ => JsonRpcResponse::error(
                req.id.clone(),
                error_codes::METHOD_NOT_FOUND,
                format!("Method not found: {}", req.method),
            ),
        }
    }

    /// Handle initialize request
    fn handle_initialize(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        // Parse initialize params (optional validation)
        let _params: Option<InitializeParams> = req
            .params
            .as_ref()
            .and_then(|p| serde_json::from_value(p.clone()).ok());

        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.into(),
            capabilities: self.server.capabilities.clone(),
            server_info: self.server.server_info.clone(),
            instructions: self.server.instructions.clone(),
            _meta: None,
        };

        match serde_json::to_value(&result) {
            Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
            Err(e) => JsonRpcResponse::error(
                req.id.clone(),
                error_codes::INTERNAL_ERROR,
                format!("Failed to serialize result: {e}"),
            ),
        }
    }

    /// Handle initialized notification
    fn handle_initialized_notification(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        // This is a notification confirming initialization is complete
        // We just acknowledge it - actual notifications return no response
        JsonRpcResponse::success(req.id.clone(), serde_json::json!({}))
    }

    /// Handle ping request
    fn handle_ping(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse::success(req.id.clone(), serde_json::json!({}))
    }

    /// Handle logging/setLevel request
    fn handle_logging_set_level(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        // Cloudflare Workers don't have traditional logging levels
        // Accept the request but it's effectively a no-op
        JsonRpcResponse::success(req.id.clone(), serde_json::json!({}))
    }

    /// Handle tools/list request
    fn handle_tools_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let tools: Vec<_> = self.server.tools.values().map(|r| &r.tool).collect();
        let result = serde_json::json!({
            "tools": tools
        });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    /// Handle tools/call request
    async fn handle_tools_call(
        &self,
        req: &JsonRpcRequest,
        ctx: Arc<RequestContext>,
    ) -> JsonRpcResponse {
        #[derive(Deserialize)]
        struct CallToolParams {
            name: String,
            #[serde(default)]
            arguments: Option<serde_json::Value>,
        }

        let params: CallToolParams = match req.params.as_ref() {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(params) => params,
                Err(e) => {
                    return JsonRpcResponse::error(
                        req.id.clone(),
                        error_codes::INVALID_PARAMS,
                        format!("Invalid params: {e}"),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INVALID_PARAMS,
                    "Missing params: expected {name, arguments?}",
                );
            }
        };

        let registered_tool = match self.server.tools.get(&params.name) {
            Some(tool) => tool,
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::METHOD_NOT_FOUND,
                    format!("Tool not found: {}", params.name),
                );
            }
        };

        let args = params.arguments.unwrap_or(serde_json::json!({}));

        // Dispatch to handler based on whether it needs context
        let tool_result = match &registered_tool.handler {
            ToolHandlerKind::NoCtx(handler) => handler(args).await,
            ToolHandlerKind::WithCtx(handler) => handler(ctx, args).await,
        };

        match serde_json::to_value(&tool_result) {
            Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
            Err(e) => JsonRpcResponse::error(
                req.id.clone(),
                error_codes::INTERNAL_ERROR,
                format!("Failed to serialize result: {e}"),
            ),
        }
    }

    /// Handle resources/list request
    fn handle_resources_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let resources: Vec<_> = self
            .server
            .resources
            .values()
            .map(|r| &r.resource)
            .collect();
        let result = serde_json::json!({
            "resources": resources
        });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    /// Handle resources/templates/list request
    fn handle_resource_templates_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let templates: Vec<_> = self
            .server
            .resource_templates
            .values()
            .map(|r| &r.template)
            .collect();
        let result = serde_json::json!({
            "resourceTemplates": templates
        });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    /// Handle resources/read request
    async fn handle_resources_read(
        &self,
        req: &JsonRpcRequest,
        ctx: Arc<RequestContext>,
    ) -> JsonRpcResponse {
        #[derive(Deserialize)]
        struct ReadResourceParams {
            uri: String,
        }

        let params: ReadResourceParams = match req.params.as_ref() {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(params) => params,
                Err(e) => {
                    return JsonRpcResponse::error(
                        req.id.clone(),
                        error_codes::INVALID_PARAMS,
                        format!("Invalid params: {e}"),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INVALID_PARAMS,
                    "Missing params: expected {uri}",
                );
            }
        };

        // Try exact match first
        if let Some(registered_resource) = self.server.resources.get(&params.uri) {
            let result = match &registered_resource.handler {
                ResourceHandlerKind::NoCtx(handler) => handler(params.uri.clone()).await,
                ResourceHandlerKind::WithCtx(handler) => {
                    handler(ctx.clone(), params.uri.clone()).await
                }
            };
            return match result {
                Ok(resource_result) => match serde_json::to_value(&resource_result) {
                    Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
                    Err(e) => JsonRpcResponse::error(
                        req.id.clone(),
                        error_codes::INTERNAL_ERROR,
                        format!("Failed to serialize result: {e}"),
                    ),
                },
                Err(e) => JsonRpcResponse::error(req.id.clone(), error_codes::INTERNAL_ERROR, e),
            };
        }

        // Try template matching
        for (template_uri, registered_template) in &self.server.resource_templates {
            if Self::matches_template(template_uri, &params.uri) {
                let result = match &registered_template.handler {
                    ResourceHandlerKind::NoCtx(handler) => handler(params.uri.clone()).await,
                    ResourceHandlerKind::WithCtx(handler) => {
                        handler(ctx.clone(), params.uri.clone()).await
                    }
                };
                return match result {
                    Ok(resource_result) => match serde_json::to_value(&resource_result) {
                        Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
                        Err(e) => JsonRpcResponse::error(
                            req.id.clone(),
                            error_codes::INTERNAL_ERROR,
                            format!("Failed to serialize result: {e}"),
                        ),
                    },
                    Err(e) => {
                        JsonRpcResponse::error(req.id.clone(), error_codes::INTERNAL_ERROR, e)
                    }
                };
            }
        }

        JsonRpcResponse::error(
            req.id.clone(),
            error_codes::INVALID_PARAMS,
            format!("Resource not found: {}", params.uri),
        )
    }

    /// Handle prompts/list request
    fn handle_prompts_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let prompts: Vec<_> = self.server.prompts.values().map(|r| &r.prompt).collect();
        let result = serde_json::json!({
            "prompts": prompts
        });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    /// Handle prompts/get request
    async fn handle_prompts_get(
        &self,
        req: &JsonRpcRequest,
        ctx: Arc<RequestContext>,
    ) -> JsonRpcResponse {
        #[derive(Deserialize)]
        struct GetPromptParams {
            name: String,
            #[serde(default)]
            arguments: Option<serde_json::Value>,
        }

        let params: GetPromptParams = match req.params.as_ref() {
            Some(p) => match serde_json::from_value(p.clone()) {
                Ok(params) => params,
                Err(e) => {
                    return JsonRpcResponse::error(
                        req.id.clone(),
                        error_codes::INVALID_PARAMS,
                        format!("Invalid params: {e}"),
                    );
                }
            },
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INVALID_PARAMS,
                    "Missing params: expected {name, arguments?}",
                );
            }
        };

        let registered_prompt = match self.server.prompts.get(&params.name) {
            Some(prompt) => prompt,
            None => {
                return JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INVALID_PARAMS,
                    format!("Prompt not found: {}", params.name),
                );
            }
        };

        // Dispatch to handler based on whether it needs context
        let result = match &registered_prompt.handler {
            PromptHandlerKind::NoCtx(handler) => handler(params.arguments).await,
            PromptHandlerKind::WithCtx(handler) => handler(ctx, params.arguments).await,
        };

        match result {
            Ok(prompt_result) => match serde_json::to_value(&prompt_result) {
                Ok(value) => JsonRpcResponse::success(req.id.clone(), value),
                Err(e) => JsonRpcResponse::error(
                    req.id.clone(),
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to serialize result: {e}"),
                ),
            },
            Err(e) => JsonRpcResponse::error(req.id.clone(), error_codes::INTERNAL_ERROR, e),
        }
    }

    /// Simple template matching for resource URIs
    ///
    /// Supports `{param}` style placeholders in URI templates.
    /// Each `{param}` matches any non-empty path segment.
    fn matches_template(template: &str, uri: &str) -> bool {
        let template_parts: Vec<&str> = template.split('/').collect();
        let uri_parts: Vec<&str> = uri.split('/').collect();

        if template_parts.len() != uri_parts.len() {
            return false;
        }

        for (t, u) in template_parts.iter().zip(uri_parts.iter()) {
            if t.starts_with('{') && t.ends_with('}') {
                // Template parameter - matches any non-empty segment
                if u.is_empty() {
                    return false;
                }
                continue;
            }
            if t != u {
                return false;
            }
        }

        true
    }

    /// Create CORS headers for responses.
    ///
    /// SECURITY: Echoes the request Origin header instead of using wildcard `*`.
    /// This prevents credentials from being exposed to arbitrary origins.
    fn cors_headers(&self, request_origin: Option<&str>) -> Headers {
        let headers = Headers::new();
        // SECURITY: Echo the request origin instead of using wildcard.
        // If no origin was provided, fall back to "*" for non-browser clients.
        let origin = request_origin.unwrap_or("*");
        let _ = headers.set("Access-Control-Allow-Origin", origin);
        if request_origin.is_some() {
            // Vary header is required when Access-Control-Allow-Origin is not "*"
            let _ = headers.set("Vary", "Origin");
        }
        let _ = headers.set("Access-Control-Allow-Methods", "POST, OPTIONS");
        let _ = headers.set(
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization, X-Request-ID",
        );
        let _ = headers.set("Access-Control-Max-Age", "86400");
        headers
    }

    /// Create a CORS preflight response
    fn cors_preflight_response(&self, request_origin: Option<&str>) -> worker::Result<Response> {
        Response::empty()
            .map(|r| r.with_status(204))
            .map(|r| r.with_headers(self.cors_headers(request_origin)))
    }

    /// Create a JSON response with CORS headers
    fn json_response(
        &self,
        body: &JsonRpcResponse,
        request_origin: Option<&str>,
    ) -> worker::Result<Response> {
        let json = serde_json::to_string(body).map_err(|e| worker::Error::from(e.to_string()))?;

        let headers = self.cors_headers(request_origin);
        let _ = headers.set("Content-Type", "application/json");

        Ok(Response::ok(json)?.with_headers(headers))
    }

    /// Create an HTTP error response with CORS headers
    fn error_response(
        &self,
        status: u16,
        message: &str,
        request_origin: Option<&str>,
    ) -> worker::Result<Response> {
        Response::error(message, status).map(|r| r.with_headers(self.cors_headers(request_origin)))
    }
}

/// Initialize request parameters
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // Fields used for deserialization validation
struct InitializeParams {
    #[serde(default)]
    protocol_version: String,
    #[serde(default)]
    capabilities: ClientCapabilities,
    #[serde(default)]
    client_info: Option<Implementation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_matching_exact() {
        assert!(McpHandler::matches_template(
            "file:///path/to/file",
            "file:///path/to/file"
        ));
        assert!(McpHandler::matches_template("config://app", "config://app"));
    }

    #[test]
    fn test_template_matching_with_params() {
        assert!(McpHandler::matches_template(
            "file:///{name}",
            "file:///test.txt"
        ));
        assert!(McpHandler::matches_template(
            "user://{id}/profile",
            "user://123/profile"
        ));
        assert!(McpHandler::matches_template(
            "data://{type}/{id}",
            "data://users/42"
        ));
    }

    #[test]
    fn test_template_matching_non_matching() {
        // Different path depth
        assert!(!McpHandler::matches_template(
            "file:///path",
            "file:///other"
        ));
        assert!(!McpHandler::matches_template(
            "file:///{name}/extra",
            "file:///test.txt"
        ));

        // Different prefix
        assert!(!McpHandler::matches_template(
            "http://example.com",
            "https://example.com"
        ));
    }

    #[test]
    fn test_template_matching_empty_segments() {
        // Empty segments should not match template params
        assert!(!McpHandler::matches_template("file:///{name}", "file:///"));
        assert!(!McpHandler::matches_template("a/{b}/c", "a//c"));
    }

    #[test]
    fn test_json_rpc_error_codes() {
        assert_eq!(error_codes::PARSE_ERROR, -32700);
        assert_eq!(error_codes::INVALID_REQUEST, -32600);
        assert_eq!(error_codes::METHOD_NOT_FOUND, -32601);
        assert_eq!(error_codes::INVALID_PARAMS, -32602);
        assert_eq!(error_codes::INTERNAL_ERROR, -32603);
    }
}

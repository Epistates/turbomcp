//! WASM-compatible middleware system for MCP servers.
//!
//! This module provides a middleware trait with typed hooks for each MCP operation,
//! enabling request interception, modification, and short-circuiting.
//!
//! # Security
//!
//! The middleware stack includes secure CORS handling:
//!
//! - Echoes the request `Origin` header instead of using wildcard `*`
//! - Adds `Vary: Origin` header for proper caching behavior
//! - Falls back to `*` only for non-browser clients (no Origin header)
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::middleware::{McpMiddleware, Next, MiddlewareStack};
//! use std::sync::Arc;
//!
//! struct LoggingMiddleware;
//!
//! impl McpMiddleware for LoggingMiddleware {
//!     fn on_call_tool<'a>(
//!         &'a self,
//!         name: &'a str,
//!         args: serde_json::Value,
//!         ctx: Arc<RequestContext>,
//!         next: Next<'a>,
//!     ) -> BoxFuture<'a, Result<ToolResult, String>> {
//!         Box::pin(async move {
//!             println!("Calling tool: {}", name);
//!             let result = next.call_tool(name, args, ctx).await;
//!             println!("Tool result: {:?}", result.is_ok());
//!             result
//!         })
//!     }
//! }
//!
//! let server = McpServer::builder("my-server", "1.0.0")
//!     .tool("hello", "Say hello", hello_handler)
//!     .build();
//!
//! let with_middleware = MiddlewareStack::new(server)
//!     .with_middleware(LoggingMiddleware);
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;
use turbomcp_core::PROTOCOL_VERSION;
use turbomcp_core::types::capabilities::ClientCapabilities;
use turbomcp_core::types::core::Implementation;
use turbomcp_core::types::initialization::InitializeResult;
use turbomcp_core::types::prompts::Prompt;
use turbomcp_core::types::resources::Resource;
use turbomcp_core::types::tools::Tool;
use turbomcp_core::{MaybeSend, MaybeSync};
use worker::{Headers, Request, Response};

use super::context::RequestContext;
use super::server::McpServer;
use super::types::{
    JsonRpcRequest, JsonRpcResponse, PromptResult, ResourceResult, ToolResult, error_codes,
};

/// Boxed future type for middleware hooks.
///
/// Note: WASM is single-threaded so futures don't need Send bounds.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Result type for tool operations.
pub type ToolOpResult = Result<ToolResult, String>;

/// Result type for resource operations.
pub type ResourceOpResult = Result<ResourceResult, String>;

/// Result type for prompt operations.
pub type PromptOpResult = Result<PromptResult, String>;

/// Result type for lifecycle operations.
pub type LifecycleResult = Result<(), String>;

/// WASM-compatible middleware trait with hooks for each MCP operation.
///
/// Implement this trait to intercept and modify MCP requests and responses.
/// Each hook receives the request parameters and a `Next` object for calling
/// the next middleware or the final handler.
///
/// # Default Implementations
///
/// All hooks have default implementations that simply pass through to the next
/// middleware. Override only the hooks you need.
pub trait McpMiddleware: MaybeSend + MaybeSync + 'static {
    /// Hook called when listing tools.
    ///
    /// Can filter, modify, or replace the tool list.
    fn on_list_tools<'a>(&'a self, next: Next<'a>) -> Vec<Tool> {
        next.list_tools()
    }

    /// Hook called when listing resources.
    fn on_list_resources<'a>(&'a self, next: Next<'a>) -> Vec<Resource> {
        next.list_resources()
    }

    /// Hook called when listing prompts.
    fn on_list_prompts<'a>(&'a self, next: Next<'a>) -> Vec<Prompt> {
        next.list_prompts()
    }

    /// Hook called when a tool is invoked.
    ///
    /// Can modify arguments, short-circuit with an error, or transform the result.
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: Arc<RequestContext>,
        next: Next<'a>,
    ) -> BoxFuture<'a, ToolOpResult> {
        Box::pin(async move { next.call_tool(name, args, ctx).await })
    }

    /// Hook called when a resource is read.
    fn on_read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: Arc<RequestContext>,
        next: Next<'a>,
    ) -> BoxFuture<'a, ResourceOpResult> {
        Box::pin(async move { next.read_resource(uri, ctx).await })
    }

    /// Hook called when a prompt is retrieved.
    fn on_get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: Arc<RequestContext>,
        next: Next<'a>,
    ) -> BoxFuture<'a, PromptOpResult> {
        Box::pin(async move { next.get_prompt(name, args, ctx).await })
    }

    /// Hook called when the server is initialized.
    ///
    /// Can perform setup tasks, validate configuration, or short-circuit
    /// initialization by returning an error.
    fn on_initialize<'a>(&'a self, next: Next<'a>) -> BoxFuture<'a, LifecycleResult> {
        Box::pin(async move { next.initialize().await })
    }

    /// Hook called when the server is shutting down.
    ///
    /// Can perform cleanup tasks like flushing buffers or closing connections.
    fn on_shutdown<'a>(&'a self, next: Next<'a>) -> BoxFuture<'a, LifecycleResult> {
        Box::pin(async move { next.shutdown().await })
    }
}

/// Continuation for calling the next middleware or handler.
///
/// This struct is passed to each middleware hook and provides methods
/// to continue processing with the next middleware in the chain.
pub struct Next<'a> {
    server: &'a McpServer,
    middlewares: &'a [Arc<dyn McpMiddleware>],
    index: usize,
}

impl<'a> std::fmt::Debug for Next<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Next")
            .field("index", &self.index)
            .field(
                "remaining_middlewares",
                &(self.middlewares.len() - self.index),
            )
            .finish()
    }
}

impl<'a> Next<'a> {
    fn new(server: &'a McpServer, middlewares: &'a [Arc<dyn McpMiddleware>], index: usize) -> Self {
        Self {
            server,
            middlewares,
            index,
        }
    }

    /// List tools from the next middleware or handler.
    pub fn list_tools(self) -> Vec<Tool> {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.server, self.middlewares, self.index + 1);
            middleware.on_list_tools(next)
        } else {
            self.server.tools().iter().cloned().cloned().collect()
        }
    }

    /// List resources from the next middleware or handler.
    pub fn list_resources(self) -> Vec<Resource> {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.server, self.middlewares, self.index + 1);
            middleware.on_list_resources(next)
        } else {
            self.server.resources().iter().cloned().cloned().collect()
        }
    }

    /// List prompts from the next middleware or handler.
    pub fn list_prompts(self) -> Vec<Prompt> {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.server, self.middlewares, self.index + 1);
            middleware.on_list_prompts(next)
        } else {
            self.server.prompts().iter().cloned().cloned().collect()
        }
    }

    /// Call a tool through the next middleware or handler.
    pub async fn call_tool(
        self,
        name: &str,
        args: Value,
        ctx: Arc<RequestContext>,
    ) -> ToolOpResult {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.server, self.middlewares, self.index + 1);
            middleware.on_call_tool(name, args, ctx, next).await
        } else {
            // Call the actual server handler
            self.server.call_tool_internal(name, args, ctx).await
        }
    }

    /// Read a resource through the next middleware or handler.
    pub async fn read_resource(self, uri: &str, ctx: Arc<RequestContext>) -> ResourceOpResult {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.server, self.middlewares, self.index + 1);
            middleware.on_read_resource(uri, ctx, next).await
        } else {
            // Call the actual server handler
            self.server.read_resource_internal(uri, ctx).await
        }
    }

    /// Get a prompt through the next middleware or handler.
    pub async fn get_prompt(
        self,
        name: &str,
        args: Option<Value>,
        ctx: Arc<RequestContext>,
    ) -> PromptOpResult {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.server, self.middlewares, self.index + 1);
            middleware.on_get_prompt(name, args, ctx, next).await
        } else {
            // Call the actual server handler
            self.server.get_prompt_internal(name, args, ctx).await
        }
    }

    /// Run initialization through the next middleware or handler.
    pub async fn initialize(self) -> LifecycleResult {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.server, self.middlewares, self.index + 1);
            middleware.on_initialize(next).await
        } else {
            // Default initialization does nothing
            Ok(())
        }
    }

    /// Run shutdown through the next middleware or handler.
    pub async fn shutdown(self) -> LifecycleResult {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.server, self.middlewares, self.index + 1);
            middleware.on_shutdown(next).await
        } else {
            // Default shutdown does nothing
            Ok(())
        }
    }
}

/// A server wrapped with a middleware stack.
///
/// This wraps an `McpServer` and runs requests through the middleware chain
/// before reaching the actual handlers.
pub struct MiddlewareStack {
    server: McpServer,
    middlewares: Vec<Arc<dyn McpMiddleware>>,
}

impl std::fmt::Debug for MiddlewareStack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiddlewareStack")
            .field("middleware_count", &self.middlewares.len())
            .finish()
    }
}

impl MiddlewareStack {
    /// Create a new middleware stack wrapping the given server.
    pub fn new(server: McpServer) -> Self {
        Self {
            server,
            middlewares: Vec::new(),
        }
    }

    /// Add a middleware to the stack.
    ///
    /// Middlewares are called in the order they are added.
    #[must_use]
    pub fn with_middleware<M: McpMiddleware>(mut self, middleware: M) -> Self {
        self.middlewares.push(Arc::new(middleware));
        self
    }

    /// Get the number of middlewares in the stack.
    pub fn middleware_count(&self) -> usize {
        self.middlewares.len()
    }

    /// Get a reference to the underlying server.
    pub fn server(&self) -> &McpServer {
        &self.server
    }

    fn next(&self) -> Next<'_> {
        Next::new(&self.server, &self.middlewares, 0)
    }

    /// List tools through the middleware chain.
    pub fn list_tools(&self) -> Vec<Tool> {
        self.next().list_tools()
    }

    /// List resources through the middleware chain.
    pub fn list_resources(&self) -> Vec<Resource> {
        self.next().list_resources()
    }

    /// List prompts through the middleware chain.
    pub fn list_prompts(&self) -> Vec<Prompt> {
        self.next().list_prompts()
    }

    /// Call a tool through the middleware chain.
    pub async fn call_tool(
        &self,
        name: &str,
        args: Value,
        ctx: Arc<RequestContext>,
    ) -> ToolOpResult {
        self.next().call_tool(name, args, ctx).await
    }

    /// Read a resource through the middleware chain.
    pub async fn read_resource(&self, uri: &str, ctx: Arc<RequestContext>) -> ResourceOpResult {
        self.next().read_resource(uri, ctx).await
    }

    /// Get a prompt through the middleware chain.
    pub async fn get_prompt(
        &self,
        name: &str,
        args: Option<Value>,
        ctx: Arc<RequestContext>,
    ) -> PromptOpResult {
        self.next().get_prompt(name, args, ctx).await
    }

    /// Run initialization through the middleware chain.
    pub async fn initialize(&self) -> LifecycleResult {
        self.next().initialize().await
    }

    /// Run shutdown through the middleware chain.
    pub async fn shutdown(&self) -> LifecycleResult {
        self.next().shutdown().await
    }

    /// Handle an incoming Cloudflare Worker request through the middleware chain.
    ///
    /// This is the main entry point for your Worker's fetch handler when using middleware.
    /// Tool calls, resource reads, and prompt gets are routed through the middleware chain.
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
        const MAX_BODY_SIZE: usize = 1024 * 1024;
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

        // Route to appropriate handler with context through middleware
        let response = self.route_request(&rpc_request, context).await;

        // Per JSON-RPC 2.0 spec: notifications MUST NOT receive a response
        if is_notification && response.error.is_none() {
            return Response::empty()
                .map(|r| r.with_status(204))
                .map(|r| r.with_headers(self.cors_headers(origin_ref)));
        }

        self.json_response(&response, origin_ref)
    }

    /// Extract headers from a Worker request into a HashMap
    fn extract_headers(req: &Request) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        let worker_headers = req.headers();

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
        let session_id = headers
            .get("mcp-session-id")
            .or_else(|| headers.get("x-session-id"))
            .cloned();
        let request_id = headers.get("x-request-id").cloned();
        RequestContext::from_worker_request(request_id, session_id, headers)
    }

    /// Check if the Content-Type header indicates JSON
    fn is_valid_content_type(&self, req: &Request) -> bool {
        req.headers()
            .get("Content-Type")
            .ok()
            .flatten()
            .map(|ct| ct.contains("application/json") || ct.contains("text/json"))
            .unwrap_or(true)
    }

    /// Route a JSON-RPC request to the appropriate handler through middleware
    async fn route_request(
        &self,
        req: &JsonRpcRequest,
        ctx: Arc<RequestContext>,
    ) -> JsonRpcResponse {
        match req.method.as_str() {
            // Core protocol methods (no middleware)
            "initialize" => self.handle_initialize(req),
            "notifications/initialized" => self.handle_initialized_notification(req),
            "ping" => self.handle_ping(req),

            // Tool methods (through middleware)
            "tools/list" => self.handle_tools_list(req),
            "tools/call" => self.handle_tools_call(req, ctx).await,

            // Resource methods (through middleware)
            "resources/list" => self.handle_resources_list(req),
            "resources/templates/list" => self.handle_resource_templates_list(req),
            "resources/read" => self.handle_resources_read(req, ctx).await,

            // Prompt methods (through middleware)
            "prompts/list" => self.handle_prompts_list(req),
            "prompts/get" => self.handle_prompts_get(req, ctx).await,

            // Logging
            "logging/setLevel" => self.handle_logging_set_level(req),

            // Unknown method
            _ => JsonRpcResponse::error(
                req.id.clone(),
                error_codes::METHOD_NOT_FOUND,
                format!("Method not found: {}", req.method),
            ),
        }
    }

    fn handle_initialize(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        #[allow(dead_code)]
        struct InitializeParams {
            #[serde(default)]
            protocol_version: String,
            #[serde(default)]
            capabilities: ClientCapabilities,
            #[serde(default)]
            client_info: Option<Implementation>,
        }

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

    fn handle_initialized_notification(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse::success(req.id.clone(), serde_json::json!({}))
    }

    fn handle_ping(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse::success(req.id.clone(), serde_json::json!({}))
    }

    fn handle_logging_set_level(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        JsonRpcResponse::success(req.id.clone(), serde_json::json!({}))
    }

    fn handle_tools_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        // Route through middleware
        let tools = self.list_tools();
        let result = serde_json::json!({ "tools": tools });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    async fn handle_tools_call(
        &self,
        req: &JsonRpcRequest,
        ctx: Arc<RequestContext>,
    ) -> JsonRpcResponse {
        #[derive(Deserialize)]
        struct CallToolParams {
            name: String,
            #[serde(default)]
            arguments: Option<Value>,
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

        let args = params.arguments.unwrap_or(serde_json::json!({}));

        // Route through middleware chain
        match self.call_tool(&params.name, args, ctx).await {
            Ok(tool_result) => match serde_json::to_value(&tool_result) {
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

    fn handle_resources_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        // Route through middleware
        let resources = self.list_resources();
        let result = serde_json::json!({ "resources": resources });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    fn handle_resource_templates_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        let templates: Vec<_> = self
            .server
            .resource_templates
            .values()
            .map(|r| &r.template)
            .collect();
        let result = serde_json::json!({ "resourceTemplates": templates });
        JsonRpcResponse::success(req.id.clone(), result)
    }

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

        // Route through middleware chain
        match self.read_resource(&params.uri, ctx).await {
            Ok(resource_result) => match serde_json::to_value(&resource_result) {
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

    fn handle_prompts_list(&self, req: &JsonRpcRequest) -> JsonRpcResponse {
        // Route through middleware
        let prompts = self.list_prompts();
        let result = serde_json::json!({ "prompts": prompts });
        JsonRpcResponse::success(req.id.clone(), result)
    }

    async fn handle_prompts_get(
        &self,
        req: &JsonRpcRequest,
        ctx: Arc<RequestContext>,
    ) -> JsonRpcResponse {
        #[derive(Deserialize)]
        struct GetPromptParams {
            name: String,
            #[serde(default)]
            arguments: Option<Value>,
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

        // Route through middleware chain
        match self.get_prompt(&params.name, params.arguments, ctx).await {
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
        let _ = headers.set(
            "Access-Control-Allow-Headers",
            "Content-Type, Authorization, X-Request-ID",
        );
        let _ = headers.set("Access-Control-Max-Age", "86400");
        headers
    }

    fn cors_preflight_response(&self, request_origin: Option<&str>) -> worker::Result<Response> {
        Response::empty()
            .map(|r| r.with_status(204))
            .map(|r| r.with_headers(self.cors_headers(request_origin)))
    }

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

    fn error_response(
        &self,
        status: u16,
        message: &str,
        request_origin: Option<&str>,
    ) -> worker::Result<Response> {
        Response::error(message, status).map(|r| r.with_headers(self.cors_headers(request_origin)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A simple counting middleware for testing.
    struct CountingMiddleware {
        tool_calls: AtomicU32,
        resource_reads: AtomicU32,
        prompt_gets: AtomicU32,
        initializes: AtomicU32,
        shutdowns: AtomicU32,
    }

    impl CountingMiddleware {
        fn new() -> Self {
            Self {
                tool_calls: AtomicU32::new(0),
                resource_reads: AtomicU32::new(0),
                prompt_gets: AtomicU32::new(0),
                initializes: AtomicU32::new(0),
                shutdowns: AtomicU32::new(0),
            }
        }

        fn tool_calls(&self) -> u32 {
            self.tool_calls.load(Ordering::Relaxed)
        }

        fn initializes(&self) -> u32 {
            self.initializes.load(Ordering::Relaxed)
        }

        fn shutdowns(&self) -> u32 {
            self.shutdowns.load(Ordering::Relaxed)
        }
    }

    impl McpMiddleware for CountingMiddleware {
        fn on_call_tool<'a>(
            &'a self,
            name: &'a str,
            args: Value,
            ctx: Arc<RequestContext>,
            next: Next<'a>,
        ) -> BoxFuture<'a, ToolOpResult> {
            self.tool_calls.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move { next.call_tool(name, args, ctx).await })
        }

        fn on_read_resource<'a>(
            &'a self,
            uri: &'a str,
            ctx: Arc<RequestContext>,
            next: Next<'a>,
        ) -> BoxFuture<'a, ResourceOpResult> {
            self.resource_reads.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move { next.read_resource(uri, ctx).await })
        }

        fn on_get_prompt<'a>(
            &'a self,
            name: &'a str,
            args: Option<Value>,
            ctx: Arc<RequestContext>,
            next: Next<'a>,
        ) -> BoxFuture<'a, PromptOpResult> {
            self.prompt_gets.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move { next.get_prompt(name, args, ctx).await })
        }

        fn on_initialize<'a>(&'a self, next: Next<'a>) -> BoxFuture<'a, LifecycleResult> {
            self.initializes.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move { next.initialize().await })
        }

        fn on_shutdown<'a>(&'a self, next: Next<'a>) -> BoxFuture<'a, LifecycleResult> {
            self.shutdowns.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move { next.shutdown().await })
        }
    }

    /// A middleware that blocks certain tools.
    struct BlockingMiddleware {
        blocked_tools: Vec<String>,
    }

    impl BlockingMiddleware {
        fn new(blocked: Vec<&str>) -> Self {
            Self {
                blocked_tools: blocked.into_iter().map(String::from).collect(),
            }
        }
    }

    impl McpMiddleware for BlockingMiddleware {
        fn on_call_tool<'a>(
            &'a self,
            name: &'a str,
            args: Value,
            ctx: Arc<RequestContext>,
            next: Next<'a>,
        ) -> BoxFuture<'a, ToolOpResult> {
            let blocked = self.blocked_tools.clone();
            let name_owned = name.to_string();
            Box::pin(async move {
                if blocked.contains(&name_owned) {
                    return Err(format!("Tool '{}' is blocked", name_owned));
                }
                next.call_tool(&name_owned, args, ctx).await
            })
        }
    }

    #[test]
    fn test_middleware_stack_creation() {
        let server = McpServer::builder("test", "1.0.0").build();
        let stack = MiddlewareStack::new(server)
            .with_middleware(CountingMiddleware::new())
            .with_middleware(BlockingMiddleware::new(vec!["blocked"]));

        assert_eq!(stack.middleware_count(), 2);
    }

    #[test]
    fn test_list_tools_empty_server() {
        let server = McpServer::builder("test", "1.0.0").build();
        let stack = MiddlewareStack::new(server);
        let tools = stack.list_tools();
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_lifecycle_hooks() {
        let server = McpServer::builder("test", "1.0.0").build();
        let counting = Arc::new(CountingMiddleware::new());

        // We need to wrap the Arc in a struct that implements McpMiddleware
        struct CountingWrapper(Arc<CountingMiddleware>);

        impl McpMiddleware for CountingWrapper {
            fn on_initialize<'a>(&'a self, next: Next<'a>) -> BoxFuture<'a, LifecycleResult> {
                self.0.initializes.fetch_add(1, Ordering::Relaxed);
                Box::pin(async move { next.initialize().await })
            }

            fn on_shutdown<'a>(&'a self, next: Next<'a>) -> BoxFuture<'a, LifecycleResult> {
                self.0.shutdowns.fetch_add(1, Ordering::Relaxed);
                Box::pin(async move { next.shutdown().await })
            }
        }

        let stack = MiddlewareStack::new(server).with_middleware(CountingWrapper(counting.clone()));

        stack.initialize().await.unwrap();
        stack.shutdown().await.unwrap();

        assert_eq!(counting.initializes(), 1);
        assert_eq!(counting.shutdowns(), 1);
    }

    #[tokio::test]
    async fn test_blocking_middleware() {
        let server = McpServer::builder("test", "1.0.0").build();
        let stack =
            MiddlewareStack::new(server).with_middleware(BlockingMiddleware::new(vec!["blocked"]));

        let ctx = Arc::new(RequestContext::new());
        let result = stack.call_tool("blocked", serde_json::json!({}), ctx).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("blocked"));
    }

    #[tokio::test]
    async fn test_counting_middleware_tool_calls() {
        // Create a server with a test tool
        async fn test_tool(_args: serde_json::Value) -> String {
            "ok".to_string()
        }

        let server = McpServer::builder("test", "1.0.0")
            .tool_raw("test_tool", "A test tool", test_tool)
            .build();

        let counting = Arc::new(CountingMiddleware::new());

        // Wrap the Arc in a struct that implements McpMiddleware
        struct CountingWrapper(Arc<CountingMiddleware>);

        impl McpMiddleware for CountingWrapper {
            fn on_call_tool<'a>(
                &'a self,
                name: &'a str,
                args: Value,
                ctx: Arc<RequestContext>,
                next: Next<'a>,
            ) -> BoxFuture<'a, ToolOpResult> {
                self.0.tool_calls.fetch_add(1, Ordering::Relaxed);
                Box::pin(async move { next.call_tool(name, args, ctx).await })
            }
        }

        let stack = MiddlewareStack::new(server).with_middleware(CountingWrapper(counting.clone()));

        // Call the tool multiple times
        let ctx1 = Arc::new(RequestContext::new());
        let ctx2 = Arc::new(RequestContext::new());

        let result1 = stack
            .call_tool("test_tool", serde_json::json!({}), ctx1)
            .await;
        let result2 = stack
            .call_tool("test_tool", serde_json::json!({}), ctx2)
            .await;

        assert!(result1.is_ok());
        assert!(result2.is_ok());
        assert_eq!(counting.tool_calls(), 2);
    }
}

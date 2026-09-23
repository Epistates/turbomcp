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
//!     ) -> BoxFuture<'a, ToolOpResult> {
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

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use turbomcp_core::error::McpResult;
use turbomcp_core::handler::McpHandler;
use turbomcp_core::{MaybeSend, MaybeSync};
use turbomcp_types::{
    Implementation, Prompt, Resource, ResourceTemplate, ServerCapabilities, Tool,
};
use worker::{Request, Response};

use super::context::{RequestContext, shared_context};
use super::server::{McpServer, into_core_tool_result};
use super::types::{PromptResult, ResourceResult, ToolResult};

/// Boxed future type for middleware hooks.
///
/// WASM is single-threaded, so on `wasm32` the future carries no `Send`
/// bound. Host builds (tests) need one: the stack is itself an `McpHandler`,
/// and native handler futures must be `Send`.
#[cfg(target_arch = "wasm32")]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Boxed future type for middleware hooks (host builds).
#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Result type for tool operations.
///
/// The error is an [`McpError`](turbomcp_core::error::McpError) so that its
/// kind — and with it the JSON-RPC code — survives the chain: an unknown tool
/// stays `-32602` and a middleware refusing a call can say why, instead of
/// every failure being flattened into `-32603`.
pub type ToolOpResult = McpResult<ToolResult>;

/// Result type for resource operations.
pub type ResourceOpResult = McpResult<ResourceResult>;

/// Result type for prompt operations.
pub type PromptOpResult = McpResult<PromptResult>;

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
/// before reaching the actual handlers. The stack is itself an [`McpHandler`],
/// so it is served by the same core router as every other WASM entry point
/// and can be wrapped further (visibility, authentication, Streamable HTTP).
#[derive(Clone)]
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
    /// This is the main entry point for your Worker's fetch handler when using
    /// middleware. It serves the same stateless endpoint as
    /// [`McpServer::handle`]; tool calls, resource reads, prompt gets and the
    /// three listings pass through the chain.
    pub async fn handle(&self, req: Request) -> worker::Result<Response> {
        super::endpoint::serve(
            self,
            req,
            &super::EndpointConfig::default(),
            |ctx| ctx,
            super::endpoint::admit_all,
        )
        .await
    }
}

#[allow(clippy::manual_async_fn)]
impl McpHandler for MiddlewareStack {
    fn server_info(&self) -> Implementation {
        self.server.server_info()
    }

    fn instructions(&self) -> Option<String> {
        self.server.instructions()
    }

    fn server_capabilities(&self) -> ServerCapabilities {
        self.server.server_capabilities()
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.next().list_tools()
    }

    fn list_resources(&self) -> Vec<Resource> {
        self.next().list_resources()
    }

    fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        self.server.list_resource_templates()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        self.next().list_prompts()
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<turbomcp_types::ToolResult>> + MaybeSend + 'a {
        async move {
            self.next()
                .call_tool(name, args, shared_context(ctx))
                .await
                .map(into_core_tool_result)
        }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
        async move { self.next().read_resource(uri, shared_context(ctx)).await }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
        async move {
            self.next()
                .get_prompt(name, args, shared_context(ctx))
                .await
        }
    }

    fn page_size(&self) -> Option<usize> {
        self.server.page_size()
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
                    return Err(turbomcp_core::error::McpError::permission_denied(format!(
                        "Tool '{}' is blocked",
                        name_owned
                    )));
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

        let error = result.unwrap_err();
        assert!(error.message.contains("blocked"));
        assert_eq!(
            error.kind,
            turbomcp_core::error::ErrorKind::PermissionDenied
        );
    }

    async fn route(stack: &MiddlewareStack, method: &str, params: Value) -> Value {
        let request = turbomcp_core::jsonrpc::JsonRpcIncoming {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::json!(1)),
            method: method.into(),
            params: Some(params),
        };
        let ctx = crate::wasm_server::context::new_wasm_context();
        serde_json::to_value(super::super::endpoint::route(stack, request, &ctx, None).await)
            .unwrap()
    }

    /// The stack used to answer every failure `-32603`; the kind a middleware
    /// or the server chose now reaches the wire.
    #[tokio::test]
    async fn errors_keep_their_codes_through_the_stack() {
        let server = McpServer::builder("test", "1.0.0")
            .tool_raw("blocked", "Blocked", |_args: Value| async { "never" })
            .build();
        let stack =
            MiddlewareStack::new(server).with_middleware(BlockingMiddleware::new(vec!["blocked"]));

        let unknown = route(&stack, "tools/call", serde_json::json!({"name": "nope"})).await;
        assert_eq!(unknown["error"]["code"], -32602);

        let blocked = route(&stack, "tools/call", serde_json::json!({"name": "blocked"})).await;
        assert_eq!(
            blocked["error"]["code"],
            turbomcp_core::error::McpError::permission_denied("").jsonrpc_code()
        );

        let missing = route(
            &stack,
            "resources/read",
            serde_json::json!({"uri": "file:///nope"}),
        )
        .await;
        assert_eq!(missing["error"]["code"], -32002);
    }

    /// The stack answers the whole protocol, not just the methods it hooks.
    #[tokio::test]
    async fn stack_is_a_full_mcp_handler() {
        let stack = MiddlewareStack::new(McpServer::builder("test", "1.0.0").build());
        let ping = route(&stack, "ping", serde_json::json!({})).await;
        assert_eq!(ping["result"], serde_json::json!({}));

        let init = route(
            &stack,
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "c", "version": "1"}
            }),
        )
        .await;
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
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

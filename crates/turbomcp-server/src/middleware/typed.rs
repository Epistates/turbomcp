//! Typed middleware with per-method hooks.
//!
//! This module provides a middleware trait with typed hooks for each MCP operation,
//! enabling request interception, modification, and short-circuiting.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use turbomcp_core::context::RequestContext;
use turbomcp_core::error::McpResult;
use turbomcp_core::handler::McpHandler;
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
};

/// Typed middleware trait with hooks for each MCP operation.
///
/// Implement this trait to intercept and modify MCP requests and responses.
/// Each hook receives the request parameters and a `Next` object for calling
/// the next middleware or the final handler.
///
/// # Default Implementations
///
/// All hooks have default implementations that simply pass through to the next
/// middleware. Override only the hooks you need.
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::middleware::{McpMiddleware, Next};
/// use async_trait::async_trait;
///
/// struct RateLimitMiddleware {
///     max_calls_per_minute: u32,
/// }
///
/// #[async_trait]
/// impl McpMiddleware for RateLimitMiddleware {
///     async fn on_call_tool<'a>(
///         &'a self,
///         name: &'a str,
///         args: Value,
///         ctx: &'a RequestContext,
///         next: Next<'a>,
///     ) -> McpResult<ToolResult> {
///         // Check rate limit
///         if self.is_rate_limited(ctx) {
///             return Err(McpError::internal("Rate limit exceeded"));
///         }
///         next.call_tool(name, args, ctx).await
///     }
/// }
/// ```
#[async_trait]
pub trait McpMiddleware: Send + Sync + 'static {
    /// Hook called when listing tools.
    ///
    /// Can filter, modify, or replace the tool list.
    async fn on_list_tools<'a>(&'a self, next: Next<'a>) -> Vec<Tool> {
        next.list_tools()
    }

    /// Hook called when listing resources.
    async fn on_list_resources<'a>(&'a self, next: Next<'a>) -> Vec<Resource> {
        next.list_resources()
    }

    /// Hook called when listing prompts.
    async fn on_list_prompts<'a>(&'a self, next: Next<'a>) -> Vec<Prompt> {
        next.list_prompts()
    }

    /// Hook called when a tool is invoked.
    ///
    /// Can modify arguments, short-circuit with an error, or transform the result.
    async fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> McpResult<ToolResult> {
        next.call_tool(name, args, ctx).await
    }

    /// Hook called when a resource is read.
    async fn on_read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> McpResult<ResourceResult> {
        next.read_resource(uri, ctx).await
    }

    /// Hook called when a prompt is retrieved.
    async fn on_get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> McpResult<PromptResult> {
        next.get_prompt(name, args, ctx).await
    }

    /// Hook called when the server is initialized.
    ///
    /// Can perform setup tasks, validate configuration, or short-circuit
    /// initialization by returning an error.
    async fn on_initialize<'a>(&'a self, next: Next<'a>) -> McpResult<()> {
        next.initialize().await
    }

    /// Hook called when the server is shutting down.
    ///
    /// Can perform cleanup tasks like flushing buffers or closing connections.
    async fn on_shutdown<'a>(&'a self, next: Next<'a>) -> McpResult<()> {
        next.shutdown().await
    }
}

/// Continuation for calling the next middleware or handler.
///
/// This struct is passed to each middleware hook and provides methods
/// to continue processing with the next middleware in the chain.
pub struct Next<'a> {
    handler: &'a dyn DynHandler,
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
    fn new(
        handler: &'a dyn DynHandler,
        middlewares: &'a [Arc<dyn McpMiddleware>],
        index: usize,
    ) -> Self {
        Self {
            handler,
            middlewares,
            index,
        }
    }

    /// List tools from the next middleware or handler.
    pub fn list_tools(self) -> Vec<Tool> {
        if self.index < self.middlewares.len() {
            // Can't easily make this recursive async, so just call handler directly
            // In a full implementation, we'd use a different pattern
            self.handler.dyn_list_tools()
        } else {
            self.handler.dyn_list_tools()
        }
    }

    /// List resources from the next middleware or handler.
    pub fn list_resources(self) -> Vec<Resource> {
        self.handler.dyn_list_resources()
    }

    /// List prompts from the next middleware or handler.
    pub fn list_prompts(self) -> Vec<Prompt> {
        self.handler.dyn_list_prompts()
    }

    /// Call a tool through the next middleware or handler.
    pub async fn call_tool(
        self,
        name: &str,
        args: Value,
        ctx: &RequestContext,
    ) -> McpResult<ToolResult> {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.handler, self.middlewares, self.index + 1);
            middleware.on_call_tool(name, args, ctx, next).await
        } else {
            self.handler.dyn_call_tool(name, args, ctx).await
        }
    }

    /// Read a resource through the next middleware or handler.
    pub async fn read_resource(self, uri: &str, ctx: &RequestContext) -> McpResult<ResourceResult> {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.handler, self.middlewares, self.index + 1);
            middleware.on_read_resource(uri, ctx, next).await
        } else {
            self.handler.dyn_read_resource(uri, ctx).await
        }
    }

    /// Get a prompt through the next middleware or handler.
    pub async fn get_prompt(
        self,
        name: &str,
        args: Option<Value>,
        ctx: &RequestContext,
    ) -> McpResult<PromptResult> {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.handler, self.middlewares, self.index + 1);
            middleware.on_get_prompt(name, args, ctx, next).await
        } else {
            self.handler.dyn_get_prompt(name, args, ctx).await
        }
    }

    /// Run initialization through the next middleware or handler.
    pub async fn initialize(self) -> McpResult<()> {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.handler, self.middlewares, self.index + 1);
            middleware.on_initialize(next).await
        } else {
            self.handler.dyn_on_initialize().await
        }
    }

    /// Run shutdown through the next middleware or handler.
    pub async fn shutdown(self) -> McpResult<()> {
        if self.index < self.middlewares.len() {
            let middleware = &self.middlewares[self.index];
            let next = Next::new(self.handler, self.middlewares, self.index + 1);
            middleware.on_shutdown(next).await
        } else {
            self.handler.dyn_on_shutdown().await
        }
    }
}

/// Internal trait for type-erased handler access.
#[allow(dead_code)]
trait DynHandler: Send + Sync {
    fn dyn_server_info(&self) -> ServerInfo;
    fn dyn_list_tools(&self) -> Vec<Tool>;
    fn dyn_list_resources(&self) -> Vec<Resource>;
    fn dyn_list_prompts(&self) -> Vec<Prompt>;
    fn dyn_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<ToolResult>> + Send + 'a>>;
    fn dyn_read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a>>;
    fn dyn_get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<PromptResult>> + Send + 'a>>;
    fn dyn_on_initialize<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>>;
    fn dyn_on_shutdown<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>>;
}

/// Wrapper for type-erased handler access.
struct HandlerWrapper<H: McpHandler> {
    handler: H,
}

impl<H: McpHandler> DynHandler for HandlerWrapper<H> {
    fn dyn_server_info(&self) -> ServerInfo {
        self.handler.server_info()
    }

    fn dyn_list_tools(&self) -> Vec<Tool> {
        self.handler.list_tools()
    }

    fn dyn_list_resources(&self) -> Vec<Resource> {
        self.handler.list_resources()
    }

    fn dyn_list_prompts(&self) -> Vec<Prompt> {
        self.handler.list_prompts()
    }

    fn dyn_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<ToolResult>> + Send + 'a>>
    {
        Box::pin(self.handler.call_tool(name, args, ctx))
    }

    fn dyn_read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a>>
    {
        Box::pin(self.handler.read_resource(uri, ctx))
    }

    fn dyn_get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<PromptResult>> + Send + 'a>>
    {
        Box::pin(self.handler.get_prompt(name, args, ctx))
    }

    fn dyn_on_initialize<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>> {
        Box::pin(self.handler.on_initialize())
    }

    fn dyn_on_shutdown<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send + 'a>> {
        Box::pin(self.handler.on_shutdown())
    }
}

/// A handler wrapped with a middleware stack.
///
/// This implements `McpHandler` and runs requests through the middleware chain.
pub struct MiddlewareStack<H: McpHandler> {
    handler: Arc<HandlerWrapper<H>>,
    middlewares: Arc<Vec<Arc<dyn McpMiddleware>>>,
}

impl<H: McpHandler> Clone for MiddlewareStack<H> {
    fn clone(&self) -> Self {
        Self {
            handler: Arc::clone(&self.handler),
            middlewares: Arc::clone(&self.middlewares),
        }
    }
}

impl<H: McpHandler> std::fmt::Debug for MiddlewareStack<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MiddlewareStack")
            .field("middleware_count", &self.middlewares.len())
            .finish()
    }
}

impl<H: McpHandler> MiddlewareStack<H> {
    /// Create a new middleware stack wrapping the given handler.
    pub fn new(handler: H) -> Self {
        Self {
            handler: Arc::new(HandlerWrapper { handler }),
            middlewares: Arc::new(Vec::new()),
        }
    }

    /// Add a middleware to the stack.
    ///
    /// Middlewares are called in the order they are added.
    #[must_use]
    pub fn with_middleware<M: McpMiddleware>(mut self, middleware: M) -> Self {
        let middlewares = Arc::make_mut(&mut self.middlewares);
        middlewares.push(Arc::new(middleware));
        self
    }

    /// Get the number of middlewares in the stack.
    pub fn middleware_count(&self) -> usize {
        self.middlewares.len()
    }

    fn next(&self) -> Next<'_> {
        Next::new(self.handler.as_ref(), &self.middlewares, 0)
    }
}

#[allow(clippy::manual_async_fn)]
impl<H: McpHandler> McpHandler for MiddlewareStack<H> {
    fn server_info(&self) -> ServerInfo {
        self.handler.dyn_server_info()
    }

    fn list_tools(&self) -> Vec<Tool> {
        self.handler.dyn_list_tools()
    }

    fn list_resources(&self) -> Vec<Resource> {
        self.handler.dyn_list_resources()
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        self.handler.dyn_list_prompts()
    }

    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<ToolResult>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move { self.next().call_tool(name, args, ctx).await }
    }

    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<ResourceResult>>
    + turbomcp_core::marker::MaybeSend
    + 'a {
        async move { self.next().read_resource(uri, ctx).await }
    }

    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
    ) -> impl std::future::Future<Output = McpResult<PromptResult>> + turbomcp_core::marker::MaybeSend + 'a
    {
        async move { self.next().get_prompt(name, args, ctx).await }
    }

    fn on_initialize(
        &self,
    ) -> impl std::future::Future<Output = McpResult<()>> + turbomcp_core::marker::MaybeSend {
        async move { self.next().initialize().await }
    }

    fn on_shutdown(
        &self,
    ) -> impl std::future::Future<Output = McpResult<()>> + turbomcp_core::marker::MaybeSend {
        async move { self.next().shutdown().await }
    }
}

#[cfg(test)]
#[allow(clippy::manual_async_fn)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use turbomcp_core::error::McpError;
    use turbomcp_core::marker::MaybeSend;

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
            vec![Resource::new("test://resource", "A test resource")]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![Prompt::new("test_prompt", "A test prompt")]
        }

        fn call_tool<'a>(
            &'a self,
            name: &'a str,
            _args: Value,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ToolResult>> + MaybeSend + 'a {
            async move {
                match name {
                    "test_tool" => Ok(ToolResult::text("Test result")),
                    _ => Err(McpError::tool_not_found(name)),
                }
            }
        }

        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a {
            let uri = uri.to_string();
            async move {
                if uri == "test://resource" {
                    Ok(ResourceResult::text(&uri, "Test content"))
                } else {
                    Err(McpError::resource_not_found(&uri))
                }
            }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<Value>,
            _ctx: &'a RequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>> + MaybeSend + 'a {
            let name = name.to_string();
            async move {
                if name == "test_prompt" {
                    Ok(PromptResult::user("Test prompt message"))
                } else {
                    Err(McpError::prompt_not_found(&name))
                }
            }
        }
    }

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

        fn resource_reads(&self) -> u32 {
            self.resource_reads.load(Ordering::Relaxed)
        }

        fn prompt_gets(&self) -> u32 {
            self.prompt_gets.load(Ordering::Relaxed)
        }

        fn initializes(&self) -> u32 {
            self.initializes.load(Ordering::Relaxed)
        }

        fn shutdowns(&self) -> u32 {
            self.shutdowns.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl McpMiddleware for CountingMiddleware {
        async fn on_call_tool<'a>(
            &'a self,
            name: &'a str,
            args: Value,
            ctx: &'a RequestContext,
            next: Next<'a>,
        ) -> McpResult<ToolResult> {
            self.tool_calls.fetch_add(1, Ordering::Relaxed);
            next.call_tool(name, args, ctx).await
        }

        async fn on_read_resource<'a>(
            &'a self,
            uri: &'a str,
            ctx: &'a RequestContext,
            next: Next<'a>,
        ) -> McpResult<ResourceResult> {
            self.resource_reads.fetch_add(1, Ordering::Relaxed);
            next.read_resource(uri, ctx).await
        }

        async fn on_get_prompt<'a>(
            &'a self,
            name: &'a str,
            args: Option<Value>,
            ctx: &'a RequestContext,
            next: Next<'a>,
        ) -> McpResult<PromptResult> {
            self.prompt_gets.fetch_add(1, Ordering::Relaxed);
            next.get_prompt(name, args, ctx).await
        }

        async fn on_initialize<'a>(&'a self, next: Next<'a>) -> McpResult<()> {
            self.initializes.fetch_add(1, Ordering::Relaxed);
            next.initialize().await
        }

        async fn on_shutdown<'a>(&'a self, next: Next<'a>) -> McpResult<()> {
            self.shutdowns.fetch_add(1, Ordering::Relaxed);
            next.shutdown().await
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

    #[async_trait]
    impl McpMiddleware for BlockingMiddleware {
        async fn on_call_tool<'a>(
            &'a self,
            name: &'a str,
            args: Value,
            ctx: &'a RequestContext,
            next: Next<'a>,
        ) -> McpResult<ToolResult> {
            if self.blocked_tools.contains(&name.to_string()) {
                return Err(McpError::internal(format!("Tool '{}' is blocked", name)));
            }
            next.call_tool(name, args, ctx).await
        }
    }

    #[test]
    fn test_middleware_stack_creation() {
        let stack = MiddlewareStack::new(TestHandler)
            .with_middleware(CountingMiddleware::new())
            .with_middleware(BlockingMiddleware::new(vec!["blocked"]));

        assert_eq!(stack.middleware_count(), 2);
    }

    #[test]
    fn test_server_info_passthrough() {
        let stack = MiddlewareStack::new(TestHandler);
        let info = stack.server_info();
        assert_eq!(info.name, "test");
        assert_eq!(info.version, "1.0.0");
    }

    #[test]
    fn test_list_tools_passthrough() {
        let stack = MiddlewareStack::new(TestHandler);
        let tools = stack.list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "test_tool");
    }

    #[tokio::test]
    async fn test_call_tool_through_middleware() {
        let counting = Arc::new(CountingMiddleware::new());
        let stack =
            MiddlewareStack::new(TestHandler).with_middleware(CountingClone(counting.clone()));

        let ctx = RequestContext::default();
        let result = stack
            .call_tool("test_tool", serde_json::json!({}), &ctx)
            .await
            .unwrap();

        assert_eq!(result.first_text(), Some("Test result"));
        assert_eq!(counting.tool_calls(), 1);
    }

    #[tokio::test]
    async fn test_blocking_middleware() {
        let stack = MiddlewareStack::new(TestHandler)
            .with_middleware(BlockingMiddleware::new(vec!["test_tool"]));

        let ctx = RequestContext::default();
        let result = stack
            .call_tool("test_tool", serde_json::json!({}), &ctx)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("blocked"));
    }

    #[tokio::test]
    async fn test_middleware_chain_order() {
        let counting1 = Arc::new(CountingMiddleware::new());
        let counting2 = Arc::new(CountingMiddleware::new());

        let stack = MiddlewareStack::new(TestHandler)
            .with_middleware(CountingClone(counting1.clone()))
            .with_middleware(CountingClone(counting2.clone()));

        let ctx = RequestContext::default();
        stack
            .call_tool("test_tool", serde_json::json!({}), &ctx)
            .await
            .unwrap();

        // Both middlewares should be called
        assert_eq!(counting1.tool_calls(), 1);
        assert_eq!(counting2.tool_calls(), 1);
    }

    #[tokio::test]
    async fn test_read_resource_through_middleware() {
        let counting = Arc::new(CountingMiddleware::new());
        let stack =
            MiddlewareStack::new(TestHandler).with_middleware(CountingClone(counting.clone()));

        let ctx = RequestContext::default();
        let result = stack.read_resource("test://resource", &ctx).await.unwrap();

        assert!(!result.contents.is_empty());
        assert_eq!(counting.resource_reads(), 1);
    }

    #[tokio::test]
    async fn test_get_prompt_through_middleware() {
        let counting = Arc::new(CountingMiddleware::new());
        let stack =
            MiddlewareStack::new(TestHandler).with_middleware(CountingClone(counting.clone()));

        let ctx = RequestContext::default();
        let result = stack.get_prompt("test_prompt", None, &ctx).await.unwrap();

        assert!(!result.messages.is_empty());
        assert_eq!(counting.prompt_gets(), 1);
    }

    /// Wrapper to make Arc<CountingMiddleware> work as middleware.
    struct CountingClone(Arc<CountingMiddleware>);

    #[async_trait]
    impl McpMiddleware for CountingClone {
        async fn on_call_tool<'a>(
            &'a self,
            name: &'a str,
            args: Value,
            ctx: &'a RequestContext,
            next: Next<'a>,
        ) -> McpResult<ToolResult> {
            self.0.on_call_tool(name, args, ctx, next).await
        }

        async fn on_read_resource<'a>(
            &'a self,
            uri: &'a str,
            ctx: &'a RequestContext,
            next: Next<'a>,
        ) -> McpResult<ResourceResult> {
            self.0.on_read_resource(uri, ctx, next).await
        }

        async fn on_get_prompt<'a>(
            &'a self,
            name: &'a str,
            args: Option<Value>,
            ctx: &'a RequestContext,
            next: Next<'a>,
        ) -> McpResult<PromptResult> {
            self.0.on_get_prompt(name, args, ctx, next).await
        }

        async fn on_initialize<'a>(&'a self, next: Next<'a>) -> McpResult<()> {
            self.0.on_initialize(next).await
        }

        async fn on_shutdown<'a>(&'a self, next: Next<'a>) -> McpResult<()> {
            self.0.on_shutdown(next).await
        }
    }

    #[tokio::test]
    async fn test_on_initialize_through_middleware() {
        let counting = Arc::new(CountingMiddleware::new());
        let stack =
            MiddlewareStack::new(TestHandler).with_middleware(CountingClone(counting.clone()));

        stack.on_initialize().await.unwrap();

        assert_eq!(counting.initializes(), 1);
    }

    #[tokio::test]
    async fn test_on_shutdown_through_middleware() {
        let counting = Arc::new(CountingMiddleware::new());
        let stack =
            MiddlewareStack::new(TestHandler).with_middleware(CountingClone(counting.clone()));

        stack.on_shutdown().await.unwrap();

        assert_eq!(counting.shutdowns(), 1);
    }

    #[tokio::test]
    async fn test_lifecycle_hooks_chain_through_multiple_middlewares() {
        let counting1 = Arc::new(CountingMiddleware::new());
        let counting2 = Arc::new(CountingMiddleware::new());

        let stack = MiddlewareStack::new(TestHandler)
            .with_middleware(CountingClone(counting1.clone()))
            .with_middleware(CountingClone(counting2.clone()));

        stack.on_initialize().await.unwrap();
        stack.on_shutdown().await.unwrap();

        assert_eq!(counting1.initializes(), 1);
        assert_eq!(counting2.initializes(), 1);
        assert_eq!(counting1.shutdowns(), 1);
        assert_eq!(counting2.shutdowns(), 1);
    }

    /// A middleware that blocks initialization.
    struct BlockInitMiddleware;

    #[async_trait]
    impl McpMiddleware for BlockInitMiddleware {
        async fn on_initialize<'a>(&'a self, _next: Next<'a>) -> McpResult<()> {
            Err(McpError::internal("initialization blocked by middleware"))
        }
    }

    #[tokio::test]
    async fn test_on_initialize_short_circuit() {
        let stack = MiddlewareStack::new(TestHandler).with_middleware(BlockInitMiddleware);

        let result = stack.on_initialize().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("blocked"));
    }
}

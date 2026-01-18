//! Core handler trait for MCP servers.
//!
//! This module re-exports the unified `McpHandler` trait from `turbomcp-core`
//! and provides the `McpHandlerExt` extension trait for native transport runners.
//!
//! # Unified Architecture
//!
//! The `McpHandler` trait is defined in `turbomcp-core` and works on both native
//! and WASM targets. This module extends it with native-only transport methods.
//!
//! # Portable Code Pattern
//!
//! The TurboMCP architecture enables writing portable servers that work on both native
//! and WASM without platform-specific code in your business logic:
//!
//! ```rust,ignore
//! use turbomcp::prelude::*;
//!
//! #[derive(Clone)]
//! struct Calculator;
//!
//! #[server(name = "calculator", version = "1.0.0")]
//! impl Calculator {
//!     /// Add two numbers together
//!     #[tool]
//!     async fn add(
//!         &self,
//!         #[description("First number")] a: i64,
//!         #[description("Second number")] b: i64,
//!     ) -> i64 {
//!         a + b
//!     }
//! }
//!
//! // Native entry point (STDIO by default)
//! #[cfg(not(target_arch = "wasm32"))]
//! #[tokio::main]
//! async fn main() {
//!     Calculator.run().await.unwrap();
//! }
//!
//! // WASM entry point (Cloudflare Workers)
//! #[cfg(target_arch = "wasm32")]
//! #[event(fetch)]
//! async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
//!     Calculator.handle_worker_request(req).await
//! }
//! ```
//!
//! Note: The server implementation (Calculator) is identical - only the entry
//! point differs per platform.
//!
//! # Transport Architecture
//!
//! Transport implementations are in the `transport` module:
//! - `transport::stdio` - STDIO (line-based JSON-RPC)
//! - `transport::tcp` - TCP sockets (line-based JSON-RPC)
//! - `transport::unix` - Unix domain sockets (line-based JSON-RPC)
//! - `transport::http` - HTTP POST (Axum-based JSON-RPC)
//! - `transport::websocket` - WebSocket (Axum-based bidirectional)
//!
//! All line-based transports share the `LineTransportRunner` abstraction.
//!
//! # Default Entry Point
//!
//! For the simplest possible server, just use `.run()`:
//!
//! ```rust,ignore
//! #[tokio::main]
//! async fn main() {
//!     MyServer.run().await.unwrap();
//! }
//! ```
//!
//! This uses STDIO transport, which is the MCP default and works with
//! Claude Desktop and other MCP clients out of the box.

use std::future::Future;

use serde_json::Value;
use turbomcp_core::error::{McpError, McpResult};

// Re-export the unified McpHandler from core
pub use turbomcp_core::handler::McpHandler;

// Use the server's rich context for native transports
use super::RequestContext;

/// Extension trait for running McpHandler on various transports.
///
/// This trait provides simple, zero-config entry points for running MCP servers.
/// For advanced configuration (rate limits, connection limits, etc.), use the
/// builder pattern via `McpServerExt::builder()`.
///
/// # Design Philosophy
///
/// - **Simple**: `handler.run()` → runs with STDIO (Claude Desktop compatible)
/// - **Direct transport**: `handler.run_http("...")` → specific transport, default config
/// - **Configurable**: `handler.builder().transport(...).serve()` → full control
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp::prelude::*;
///
/// #[tokio::main]
/// async fn main() {
///     // Simplest: STDIO (default)
///     MyServer.run().await?;
///
///     // Specific transport, default config
///     MyServer.run_http("0.0.0.0:8080").await?;
///
///     // Full configuration via builder
///     MyServer.builder()
///         .transport(Transport::http("0.0.0.0:8080"))
///         .with_rate_limit(100, Duration::from_secs(1))
///         .serve()
///         .await?;
/// }
/// ```
pub trait McpHandlerExt: McpHandler {
    /// Run with the default transport (STDIO).
    ///
    /// STDIO is the MCP standard transport, compatible with Claude Desktop
    /// and other MCP clients. This is the recommended entry point for most servers.
    #[cfg(feature = "stdio")]
    fn run(&self) -> impl Future<Output = McpResult<()>> + Send;

    /// Run on STDIO transport (explicit, equivalent to `run()`).
    #[cfg(feature = "stdio")]
    fn run_stdio(&self) -> impl Future<Output = McpResult<()>> + Send;

    /// Run on HTTP transport (JSON-RPC over HTTP POST).
    #[cfg(feature = "http")]
    fn run_http(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send;

    /// Run on WebSocket transport (bidirectional JSON-RPC).
    #[cfg(feature = "websocket")]
    fn run_websocket(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send;

    /// Run on TCP transport (line-based JSON-RPC).
    #[cfg(feature = "tcp")]
    fn run_tcp(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send;

    /// Run on Unix domain socket transport (line-based JSON-RPC).
    #[cfg(feature = "unix")]
    fn run_unix(&self, path: &str) -> impl Future<Output = McpResult<()>> + Send;

    /// Handle a single JSON-RPC request (for serverless environments).
    ///
    /// Useful for AWS Lambda, Cloudflare Workers, and other serverless
    /// environments where you process one request at a time.
    fn handle_request(
        &self,
        request: Value,
        ctx: RequestContext,
    ) -> impl Future<Output = McpResult<Value>> + Send;
}

/// Blanket implementation of McpHandlerExt for all McpHandler types.
///
/// Each transport method delegates to the corresponding module in `super::transport`.
impl<T: McpHandler> McpHandlerExt for T {
    #[cfg(feature = "stdio")]
    fn run(&self) -> impl Future<Output = McpResult<()>> + Send {
        super::transport::stdio::run(self)
    }

    #[cfg(feature = "stdio")]
    fn run_stdio(&self) -> impl Future<Output = McpResult<()>> + Send {
        super::transport::stdio::run(self)
    }

    #[cfg(feature = "http")]
    fn run_http(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send {
        let addr = addr.to_string();
        let handler = self.clone();
        async move { super::transport::http::run(&handler, &addr).await }
    }

    #[cfg(feature = "websocket")]
    fn run_websocket(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send {
        let addr = addr.to_string();
        let handler = self.clone();
        async move { super::transport::websocket::run(&handler, &addr).await }
    }

    #[cfg(feature = "tcp")]
    fn run_tcp(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send {
        let addr = addr.to_string();
        let handler = self.clone();
        async move { super::transport::tcp::run(&handler, &addr).await }
    }

    #[cfg(feature = "unix")]
    fn run_unix(&self, path: &str) -> impl Future<Output = McpResult<()>> + Send {
        let path = path.to_string();
        let handler = self.clone();
        async move { super::transport::unix::run(&handler, &path).await }
    }

    fn handle_request(
        &self,
        request: Value,
        ctx: RequestContext,
    ) -> impl Future<Output = McpResult<Value>> + Send {
        let handler = self.clone();
        async move {
            let request_str = serde_json::to_string(&request)
                .map_err(|e| McpError::internal(format!("Failed to serialize request: {e}")))?;

            let parsed = super::router::parse_request(&request_str)?;
            let core_ctx = ctx.to_core_context();
            let response = super::router::route_request(&handler, parsed, &core_ctx).await;

            serde_json::to_value(&response)
                .map_err(|e| McpError::internal(format!("Failed to serialize response: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::context::RequestContext as CoreRequestContext;
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

        fn call_tool<'a>(
            &'a self,
            name: &'a str,
            _args: Value,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<ToolResult>> + Send + 'a {
            let name = name.to_string();
            async move {
                if name == "test_tool" {
                    Ok(ToolResult::text("Tool executed"))
                } else {
                    Err(McpError::tool_not_found(&name))
                }
            }
        }

        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a {
            let uri = uri.to_string();
            async move { Err(McpError::resource_not_found(&uri)) }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<Value>,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>> + Send + 'a {
            let name = name.to_string();
            async move { Err(McpError::prompt_not_found(&name)) }
        }
    }

    #[tokio::test]
    async fn test_handle_request() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping"
        });

        let response = handler.handle_request(request, ctx).await.unwrap();
        assert!(response.get("result").is_some());
    }

    #[tokio::test]
    async fn test_handle_request_tools_list() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list"
        });

        let response = handler.handle_request(request, ctx).await.unwrap();
        let result = response.get("result").unwrap();
        let tools = result.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "test_tool");
    }
}

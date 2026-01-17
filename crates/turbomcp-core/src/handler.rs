//! Unified MCP handler trait for cross-platform server implementations.
//!
//! This module provides the core `McpHandler` trait that defines the interface for
//! all MCP server operations. The trait is designed to work identically on native
//! and WASM targets through platform-adaptive bounds.
//!
//! # Design Philosophy
//!
//! The `McpHandler` trait follows several key design principles:
//!
//! 1. **Unified Definition**: Single trait definition works on both native and WASM
//! 2. **Platform-Adaptive Bounds**: Uses `MaybeSend`/`MaybeSync` for conditional thread safety
//! 3. **Zero-Boilerplate**: Automatically implemented by the `#[server]` macro
//! 4. **no_std Compatible**: Core trait works in `no_std` environments with `alloc`
//!
//! # Platform Behavior
//!
//! - **Native**: Methods return `impl Future + Send`, enabling multi-threaded executors
//! - **WASM**: Methods return `impl Future`, compatible with single-threaded runtimes
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp::prelude::*;
//!
//! #[derive(Clone)]
//! struct MyServer;
//!
//! #[server(name = "my-server", version = "1.0.0")]
//! impl MyServer {
//!     #[tool]
//!     async fn greet(&self, name: String) -> String {
//!         format!("Hello, {}!", name)
//!     }
//! }
//!
//! // On native:
//! #[tokio::main]
//! async fn main() {
//!     MyServer.run_stdio().await.unwrap();
//! }
//!
//! // On WASM (Cloudflare Workers):
//! #[event(fetch)]
//! async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
//!     MyServer.handle_worker_request(req).await
//! }
//! ```

use alloc::vec::Vec;
use core::future::Future;
use serde_json::Value;

use crate::context::RequestContext;
use crate::error::McpResult;
use crate::marker::{MaybeSend, MaybeSync};
use turbomcp_types::{
    Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
};

/// The unified MCP handler trait.
///
/// This trait defines the complete interface for an MCP server. It's designed to:
/// - Work identically on native (std) and WASM (no_std) targets
/// - Be automatically implemented by the `#[server]` macro
/// - Enable zero-boilerplate server development
///
/// # Required Methods
///
/// - [`server_info`](McpHandler::server_info): Returns server metadata
/// - [`list_tools`](McpHandler::list_tools): Returns available tools
/// - [`list_resources`](McpHandler::list_resources): Returns available resources
/// - [`list_prompts`](McpHandler::list_prompts): Returns available prompts
/// - [`call_tool`](McpHandler::call_tool): Executes a tool
/// - [`read_resource`](McpHandler::read_resource): Reads a resource
/// - [`get_prompt`](McpHandler::get_prompt): Gets a prompt
///
/// # Optional Hooks
///
/// - [`on_initialize`](McpHandler::on_initialize): Called during server initialization
/// - [`on_shutdown`](McpHandler::on_shutdown): Called during server shutdown
///
/// # Thread Safety
///
/// The trait requires `MaybeSend + MaybeSync` bounds, which translate to:
/// - **Native**: `Send + Sync` required for multi-threaded execution
/// - **WASM**: No thread safety requirements (single-threaded)
///
/// # Manual Implementation
///
/// While the `#[server]` macro is recommended, you can implement manually:
///
/// ```rust
/// use core::future::Future;
/// use serde_json::Value;
/// use turbomcp_core::handler::McpHandler;
/// use turbomcp_core::context::RequestContext;
/// use turbomcp_core::error::{McpError, McpResult};
/// use turbomcp_types::{Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult};
///
/// #[derive(Clone)]
/// struct MyHandler;
///
/// impl McpHandler for MyHandler {
///     fn server_info(&self) -> ServerInfo {
///         ServerInfo::new("my-handler", "1.0.0")
///     }
///
///     fn list_tools(&self) -> Vec<Tool> {
///         vec![Tool::new("hello", "Say hello")]
///     }
///
///     fn list_resources(&self) -> Vec<Resource> {
///         vec![]
///     }
///
///     fn list_prompts(&self) -> Vec<Prompt> {
///         vec![]
///     }
///
///     fn call_tool<'a>(
///         &'a self,
///         name: &'a str,
///         args: Value,
///         _ctx: &'a RequestContext,
///     ) -> impl Future<Output = McpResult<ToolResult>> + 'a {
///         let name = name.to_string();
///         async move {
///             match name.as_str() {
///                 "hello" => {
///                     let who = args.get("name")
///                         .and_then(|v| v.as_str())
///                         .unwrap_or("World");
///                     Ok(ToolResult::text(format!("Hello, {}!", who)))
///                 }
///                 _ => Err(McpError::tool_not_found(&name))
///             }
///         }
///     }
///
///     fn read_resource<'a>(
///         &'a self,
///         uri: &'a str,
///         _ctx: &'a RequestContext,
///     ) -> impl Future<Output = McpResult<ResourceResult>> + 'a {
///         let uri = uri.to_string();
///         async move { Err(McpError::resource_not_found(&uri)) }
///     }
///
///     fn get_prompt<'a>(
///         &'a self,
///         name: &'a str,
///         _args: Option<Value>,
///         _ctx: &'a RequestContext,
///     ) -> impl Future<Output = McpResult<PromptResult>> + 'a {
///         let name = name.to_string();
///         async move { Err(McpError::prompt_not_found(&name)) }
///     }
/// }
/// ```
pub trait McpHandler: Clone + MaybeSend + MaybeSync + 'static {
    // ===== Server Metadata =====

    /// Returns server information (name, version, description, etc.)
    ///
    /// This is called during the MCP `initialize` handshake to provide
    /// server metadata to the client.
    fn server_info(&self) -> ServerInfo;

    // ===== Capability Listings =====

    /// Returns all available tools.
    ///
    /// Called in response to `tools/list` requests. The returned tools
    /// will be advertised to clients with their schemas.
    fn list_tools(&self) -> Vec<Tool>;

    /// Returns all available resources.
    ///
    /// Called in response to `resources/list` requests.
    fn list_resources(&self) -> Vec<Resource>;

    /// Returns all available prompts.
    ///
    /// Called in response to `prompts/list` requests.
    fn list_prompts(&self) -> Vec<Prompt>;

    // ===== Request Handlers =====

    /// Calls a tool by name with the given arguments.
    ///
    /// Called in response to `tools/call` requests.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `args` - JSON arguments for the tool
    /// * `ctx` - Request context with metadata
    ///
    /// # Returns
    ///
    /// The tool result or an error. Use `McpError::tool_not_found()`
    /// for unknown tools.
    fn call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + MaybeSend + 'a;

    /// Reads a resource by URI.
    ///
    /// Called in response to `resources/read` requests.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to read
    /// * `ctx` - Request context with metadata
    ///
    /// # Returns
    ///
    /// The resource content or an error. Use `McpError::resource_not_found()`
    /// for unknown resources.
    fn read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + MaybeSend + 'a;

    /// Gets a prompt by name with optional arguments.
    ///
    /// Called in response to `prompts/get` requests.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the prompt
    /// * `args` - Optional JSON arguments for the prompt
    /// * `ctx` - Request context with metadata
    ///
    /// # Returns
    ///
    /// The prompt messages or an error. Use `McpError::prompt_not_found()`
    /// for unknown prompts.
    fn get_prompt<'a>(
        &'a self,
        name: &'a str,
        args: Option<Value>,
        ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + MaybeSend + 'a;

    // ===== Lifecycle Hooks =====

    /// Called when the server is initialized.
    ///
    /// Override this to perform setup tasks like loading configuration,
    /// establishing database connections, or warming caches.
    ///
    /// Default implementation does nothing.
    fn on_initialize(&self) -> impl Future<Output = McpResult<()>> + MaybeSend {
        async { Ok(()) }
    }

    /// Called when the server is shutting down.
    ///
    /// Override this to perform cleanup tasks like flushing buffers,
    /// closing connections, or saving state.
    ///
    /// Default implementation does nothing.
    fn on_shutdown(&self) -> impl Future<Output = McpResult<()>> + MaybeSend {
        async { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::McpError;

    #[derive(Clone)]
    struct TestHandler;

    impl McpHandler for TestHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("test-handler", "1.0.0")
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
                        Ok(ToolResult::text(format!("Hello, {}!", who)))
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
    fn test_server_info() {
        let handler = TestHandler;
        let info = handler.server_info();
        assert_eq!(info.name, "test-handler");
        assert_eq!(info.version, "1.0.0");
    }

    #[test]
    fn test_list_tools() {
        let handler = TestHandler;
        let tools = handler.list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "greet");
    }

    #[tokio::test]
    async fn test_call_tool() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let args = serde_json::json!({"name": "Alice"});

        let result = handler.call_tool("greet", args, &ctx).await.unwrap();
        assert_eq!(result.first_text(), Some("Hello, Alice!"));
    }

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let handler = TestHandler;
        let ctx = RequestContext::stdio();
        let args = serde_json::json!({});

        let result = handler.call_tool("unknown", args, &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_lifecycle_hooks() {
        let handler = TestHandler;
        assert!(handler.on_initialize().await.is_ok());
        assert!(handler.on_shutdown().await.is_ok());
    }

    // Verify that the trait object is Send + Sync on native
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_handler_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TestHandler>();
    }
}

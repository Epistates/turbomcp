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
    LoggingCapabilities, Prompt, PromptResult, PromptsCapabilities, Resource, ResourceResult,
    ResourceTemplate, ResourcesCapabilities, ServerCapabilities, ServerInfo, Tool, ToolResult,
    ToolsCapabilities,
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
///
/// # Clone Bound Rationale
///
/// The `Clone` bound is required because MCP handlers are typically shared across multiple
/// concurrent connections and requests. This enables:
///
/// - **Multi-connection support**: Each connection can hold its own handler instance
/// - **Cheap sharing**: Handlers follow the Arc-cloning pattern (like Axum/Tower services)
/// - **Zero-cost abstraction**: Clone typically just increments an Arc reference count
///
/// ## Recommended Pattern
///
/// Wrap your server state in `Arc` for cheap cloning:
///
/// ```rust,ignore
/// use std::sync::Arc;
/// use turbomcp::prelude::*;
///
/// #[derive(Clone)]
/// struct MyServer {
///     state: Arc<ServerState>,
/// }
///
/// struct ServerState {
///     database: Database,
///     cache: Cache,
///     // Heavy resources that shouldn't be cloned
/// }
///
/// #[server(name = "my-server", version = "1.0.0")]
/// impl MyServer {
///     #[tool]
///     async fn process(&self, input: String) -> String {
///         // Access shared state via Arc (cheap clone on each call)
///         self.state.database.query(&input).await
///     }
/// }
/// ```
///
/// Cloning `MyServer` only increments the Arc reference count, not the actual state.
pub trait McpHandler: Clone + MaybeSend + MaybeSync + 'static {
    // ===== Server Metadata =====

    /// Returns server information (name, version, description, etc.)
    ///
    /// This is called during the MCP `initialize` handshake to provide
    /// server metadata to the client.
    fn server_info(&self) -> ServerInfo;

    /// Natural-language guidance returned to the client as the `instructions`
    /// field of the `initialize` result.
    ///
    /// The spec describes it as a hint for how to use the server and its
    /// tools — a client may surface it to the model the way a system prompt
    /// would. Distinct from [`ServerInfo::description`], which identifies the
    /// implementation rather than instructing its user.
    ///
    /// `None` by default, in which case the field is omitted. The `#[server]`
    /// macro implements this from its `instructions = "..."` argument.
    fn instructions(&self) -> Option<alloc::string::String> {
        None
    }

    /// Returns the server capabilities advertised during initialization.
    ///
    /// Override this when the server supports capabilities that cannot be
    /// inferred from the static tool/resource/prompt listings, such as draft
    /// `extensions`, logging, completions, or task endpoints.
    fn server_capabilities(&self) -> ServerCapabilities {
        let mut capabilities = ServerCapabilities::default();

        if !self.list_tools().is_empty() {
            capabilities.tools = Some(ToolsCapabilities {
                list_changed: Some(true),
            });
        }

        if !self.list_resources().is_empty() || !self.list_resource_templates().is_empty() {
            capabilities.resources = Some(ResourcesCapabilities {
                subscribe: None,
                list_changed: Some(true),
            });
        }

        if !self.list_prompts().is_empty() {
            capabilities.prompts = Some(PromptsCapabilities {
                list_changed: Some(true),
            });
        }

        // Every handler can emit `notifications/message` through the request
        // context, and the spec says a server that emits log messages MUST
        // declare `logging`. Declaring it also commits the server to accepting
        // `logging/setLevel`, which the router handles on every handler's
        // behalf.
        capabilities.logging = Some(LoggingCapabilities::default());

        capabilities
    }

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

    /// Returns all available resource URI templates.
    ///
    /// Called in response to `resources/templates/list` requests. Servers with
    /// dynamic resources should return URI templates here rather than exposing
    /// templated strings as concrete `resources/list` entries.
    fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        Vec::new()
    }

    /// Returns all available prompts.
    ///
    /// Called in response to `prompts/list` requests.
    fn list_prompts(&self) -> Vec<Prompt>;

    // ===== Per-caller listings =====
    //
    // The router answers the `*/list` requests through these, passing the
    // request's context. They default to the context-free listings above; a
    // layer whose catalogue depends on the caller — per-session visibility,
    // say — overrides them. Without them such a layer could gate calls by
    // session but not listings, so a client was offered tools it could not
    // call and never shown the ones it could.

    /// The tools visible to the caller of `ctx`. Defaults to [`Self::list_tools`].
    fn list_tools_for(&self, _ctx: &RequestContext) -> Vec<Tool> {
        self.list_tools()
    }

    /// The resources visible to the caller of `ctx`. Defaults to
    /// [`Self::list_resources`].
    fn list_resources_for(&self, _ctx: &RequestContext) -> Vec<Resource> {
        self.list_resources()
    }

    /// The resource templates visible to the caller of `ctx`. Defaults to
    /// [`Self::list_resource_templates`].
    fn list_resource_templates_for(&self, _ctx: &RequestContext) -> Vec<ResourceTemplate> {
        self.list_resource_templates()
    }

    /// The prompts visible to the caller of `ctx`. Defaults to
    /// [`Self::list_prompts`].
    fn list_prompts_for(&self, _ctx: &RequestContext) -> Vec<Prompt> {
        self.list_prompts()
    }

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

    // ===== Task Management (SEP-1686) =====

    /// Lists all active and recent tasks.
    ///
    /// # Arguments
    ///
    /// * `cursor` - Opaque pagination cursor
    /// * `limit` - Maximum number of tasks to return
    /// * `ctx` - Request context
    fn list_tasks<'a>(
        &'a self,
        _cursor: Option<&'a str>,
        _limit: Option<usize>,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<turbomcp_types::ListTasksResult>> + MaybeSend + 'a {
        async {
            Err(crate::error::McpError::capability_not_supported(
                "tasks/list",
            ))
        }
    }

    /// Gets the current state of a specific task.
    ///
    /// # Arguments
    ///
    /// * `task_id` - Unique task identifier
    /// * `ctx` - Request context
    fn get_task<'a>(
        &'a self,
        _task_id: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<turbomcp_types::Task>> + MaybeSend + 'a {
        async {
            Err(crate::error::McpError::capability_not_supported(
                "tasks/get",
            ))
        }
    }

    /// Cancels a running task.
    ///
    /// # Arguments
    ///
    /// * `task_id` - Unique task identifier
    /// * `ctx` - Request context
    fn cancel_task<'a>(
        &'a self,
        _task_id: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<turbomcp_types::Task>> + MaybeSend + 'a {
        async {
            Err(crate::error::McpError::capability_not_supported(
                "tasks/cancel",
            ))
        }
    }

    /// Gets the result of a completed task.
    ///
    /// # Arguments
    ///
    /// * `task_id` - Unique task identifier
    /// * `ctx` - Request context
    fn get_task_result<'a>(
        &'a self,
        _task_id: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<Value>> + MaybeSend + 'a {
        async {
            Err(crate::error::McpError::capability_not_supported(
                "tasks/result",
            ))
        }
    }

    // ===== Resource subscriptions (MCP 2025-11-25) =====

    /// Subscribes to update notifications for the given resource URI.
    ///
    /// Called in response to `resources/subscribe` requests. The default
    /// implementation returns `capability_not_supported`. Servers that
    /// advertise `resources.subscribe = true` MUST override this method —
    /// the router calls it whenever a client invokes `resources/subscribe`.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI being subscribed to.
    /// * `ctx` - Request context.
    fn subscribe<'a>(
        &'a self,
        _uri: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<()>> + MaybeSend + 'a {
        async {
            Err(crate::error::McpError::capability_not_supported(
                "resources/subscribe",
            ))
        }
    }

    /// Cancels a previously installed resource subscription.
    ///
    /// Default implementation returns `capability_not_supported`.
    fn unsubscribe<'a>(
        &'a self,
        _uri: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<()>> + MaybeSend + 'a {
        async {
            Err(crate::error::McpError::capability_not_supported(
                "resources/unsubscribe",
            ))
        }
    }

    // ===== Logging (MCP 2025-11-25) =====

    /// Sets the minimum log level the server should emit via
    /// `notifications/message`.
    ///
    /// Called in response to `logging/setLevel`. The level is the raw spec
    /// string (`"debug" | "info" | "notice" | "warning" | "error" |
    /// "critical" | "alert" | "emergency"`), already validated.
    ///
    /// The router records the level for the session and filters the
    /// context's log helpers by it, so the default accepts and does nothing
    /// more. Override to observe level changes — for example to adjust a
    /// logger of your own — or return an error to refuse one.
    fn set_log_level<'a>(
        &'a self,
        _level: &'a str,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<()>> + MaybeSend + 'a {
        async { Ok(()) }
    }

    // ===== Completions (MCP 2025-11-25) =====

    /// Returns argument completion suggestions.
    ///
    /// Called in response to `completion/complete`. `params` is the raw
    /// JSON-RPC `params` object (i.e. the `CompleteRequestParams` shape:
    /// `{ ref: …, argument: { name, value }, context?: { arguments } }`).
    /// The return value is the raw `CompleteResult` shape (`{ completion:
    /// { values, total?, hasMore? }, _meta? }`).
    ///
    /// We accept and return `serde_json::Value` here because the typed
    /// `CompleteRequestParams` / `CompleteResult` live in `turbomcp-protocol`
    /// (which depends on this crate, so we cannot depend on it here without
    /// inverting the layer cake). Higher-level wrappers in `turbomcp` /
    /// `#[server]` may expose typed signatures over this raw shape.
    fn complete<'a>(
        &'a self,
        _params: Value,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<Value>> + MaybeSend + 'a {
        async {
            Err(crate::error::McpError::capability_not_supported(
                "completion/complete",
            ))
        }
    }

    // ===== Pagination =====

    /// Maximum entries per page for `tools/list`, `resources/list`,
    /// `resources/templates/list` and `prompts/list`.
    ///
    /// `None` — the default — means the server returns its whole catalogue in
    /// one response and never emits `nextCursor`. Pagination is optional for
    /// servers, so that is conformant, and it is the safe default: a client
    /// that does not follow cursors would silently see a truncated catalogue if
    /// paging were switched on underneath it.
    ///
    /// Override to opt in. The router then pages the list, mints an opaque
    /// cursor, and rejects a malformed or foreign one with `-32602`:
    ///
    /// ```rust,ignore
    /// fn page_size(&self) -> Option<usize> {
    ///     Some(100)
    /// }
    /// ```
    ///
    /// A server whose catalogue is too large to materialise at all wants a
    /// different shape than this — page it inside `list_tools` and keep the
    /// cursor yourself — but for everything else this is the whole feature.
    fn page_size(&self) -> Option<usize> {
        None
    }

    // ===== Client Notifications =====

    /// Called when the client reports that its filesystem roots changed.
    ///
    /// Dispatched on `notifications/roots/list_changed`. Servers that cache the
    /// result of [`RequestContext::list_roots`] should invalidate it here and
    /// re-query. The default does nothing.
    ///
    /// Notifications carry no response, so a returned error is logged by the
    /// caller rather than reaching the client.
    ///
    /// [`RequestContext::list_roots`]: crate::context::RequestContext::list_roots
    fn on_roots_list_changed<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> impl Future<Output = McpResult<()>> + MaybeSend + 'a {
        async { Ok(()) }
    }

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

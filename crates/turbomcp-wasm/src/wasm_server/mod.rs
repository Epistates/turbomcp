//! WASM Server MCP Implementation
//!
//! This module provides a full MCP server implementation that runs in WASM environments,
//! including Cloudflare Workers, Deno Deploy, and other edge/serverless platforms.
//! It handles incoming HTTP requests and routes them to registered tool/resource/prompt handlers.
//!
//! # Features
//!
//! - Zero tokio dependencies - uses wasm-bindgen-futures for async
//! - Full MCP protocol support (tools, resources, prompts)
//! - Type-safe handler registration with automatic JSON schema generation
//! - Ergonomic API inspired by axum's IntoResponse pattern
//! - Idiomatic error handling with `?` operator support
//! - Context injection for accessing request metadata, session, and headers
//! - Integration with Cloudflare Workers SDK
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::*;
//! use worker::*;
//! use serde::Deserialize;
//! use std::sync::Arc;
//!
//! #[derive(Deserialize, schemars::JsonSchema)]
//! struct HelloArgs {
//!     name: String,
//! }
//!
//! // Simple handler - just return a String!
//! async fn hello(args: HelloArgs) -> String {
//!     format!("Hello, {}!", args.name)
//! }
//!
//! // With error handling using ?
//! async fn fetch_data(args: FetchArgs) -> Result<Json<Data>, ToolError> {
//!     let data = do_fetch(&args.url).await?;
//!     Ok(Json(data))
//! }
//!
//! // With request context for session/header access
//! async fn auth_tool(ctx: Arc<RequestContext>, args: AuthArgs) -> Result<String, ToolError> {
//!     if !ctx.is_authenticated() {
//!         return Err(ToolError::new("Unauthorized"));
//!     }
//!     let session = ctx.session_id().unwrap_or("unknown");
//!     Ok(format!("Session: {}", session))
//! }
//!
//! #[event(fetch)]
//! async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
//!     let server = McpServer::builder("my-mcp-server", "1.0.0")
//!         .tool("hello", "Say hello to someone", hello)
//!         .tool("fetch", "Fetch data from URL", fetch_data)
//!         .tool_with_ctx("auth", "Authenticated tool", auth_tool)
//!         .build();
//!
//!     server.handle(req).await
//! }
//! ```
//!
//! # Handler Return Types
//!
//! Tool handlers can return any type that implements `IntoToolResponse`:
//!
//! - `String`, `&str` - Returns as text content
//! - `Json<T>` - Serializes to JSON text
//! - `ToolResult` - Full control over the response
//! - `Result<T, E>` where `T: IntoToolResponse`, `E: Into<ToolError>` - Automatic error handling
//! - `()` - Empty success response
//! - `Option<T>` - None returns "No result"
//!
//! # Context Injection
//!
//! Handlers can receive request context by using `_with_ctx` variants:
//!
//! - `tool_with_ctx` - Tool handler with `Arc<RequestContext>` first parameter
//! - `resource_with_ctx` - Resource handler with context
//! - `prompt_with_ctx` - Prompt handler with context
//!
//! The `RequestContext` provides access to:
//! - `request_id()` - Unique request identifier
//! - `session_id()` - Session ID from headers
//! - `user_id()` - User ID (set by auth middleware)
//! - `headers()` - HTTP headers
//! - `header(name)` - Get a specific header (case-insensitive)
//! - `is_authenticated()` - Check authentication status
//! - `has_role(role)` - Check for a specific role
//! - `get_metadata(key)` - Get custom metadata
//!
//! # Building for WASM Environments
//!
//! ```bash
//! # Build for Cloudflare Workers
//! cargo build --target wasm32-unknown-unknown --release
//!
//! # Or using wrangler (Cloudflare)
//! wrangler dev
//! ```

mod composite;
mod context;
mod ext;
mod handler;
mod handler_traits;
#[cfg(test)]
mod integration_tests;
pub mod middleware;
mod response;
mod rich_context;
mod server;
mod traits;
mod types;
mod visibility;

#[cfg(feature = "auth")]
mod auth_middleware;

#[cfg(feature = "streamable")]
pub mod streamable;

#[cfg(feature = "streamable")]
pub mod durable_objects;

// Re-export the extension trait for unified McpHandler support
// This enables "write once, run everywhere" - any McpHandler can be used
// directly in WASM via .handle_worker_request()
pub use ext::WasmHandlerExt;

// Re-export the main server types
pub use server::{McpServer, McpServerBuilder};

// Re-export composite server types for modular server composition
pub use composite::{CompositeServer, CompositeServerBuilder};

// Re-export visibility layer for progressive disclosure
pub use visibility::{ComponentFilter, VisibilityLayer, VisibilitySessionGuard};

// Re-export request context for handlers
pub use context::RequestContext;

// Re-export rich context for session state, logging, and progress
pub use rich_context::{
    LogLevel, ProgressCallback, RichContextExt, SessionStateGuard, StateError,
    active_sessions_count, cleanup_session_state,
};

// Re-export result types
pub use types::{PromptResult, ResourceResult, ToolResult};

// Re-export the response trait and types for ergonomic handlers
pub use response::{Image, IntoToolResponse, Json, Text, ToolError, WorkerError, WorkerResultExt};

// Re-export handler traits for advanced use cases
pub use response::IntoToolError;
pub use traits::{
    IntoPromptResponse, IntoResourceResponse, PromptHandlerFn, ResourceHandlerFn, ResultExt,
    ToolHandlerFn,
};

// Re-export handler trait bounds for advanced use cases
pub use handler_traits::{
    IntoPromptHandler, IntoPromptHandlerWithCtx, IntoResourceHandler, IntoResourceHandlerWithCtx,
    IntoToolHandler, IntoToolHandlerWithCtx,
};

// Re-export authentication middleware when auth feature is enabled
#[cfg(feature = "auth")]
pub use auth_middleware::{AuthExt, WithAuth};

// Re-export streamable HTTP transport when streamable feature is enabled
#[cfg(feature = "streamable")]
pub use streamable::{MemorySessionStore, StreamableExt, StreamableHandler};

// Re-export Durable Objects integration when streamable feature is enabled
#[cfg(feature = "streamable")]
pub use durable_objects::{
    DurableObjectRateLimiter, DurableObjectSessionStore, DurableObjectStateStore,
    DurableObjectTokenStore, OAuthTokenData, RateLimitConfig, RateLimitResult, StateStoreError,
    TokenStoreError,
};

// Re-export middleware types
pub use middleware::{
    BoxFuture, LifecycleResult, McpMiddleware, MiddlewareStack, Next, PromptOpResult,
    ResourceOpResult, ToolOpResult,
};

/// Re-export worker types for convenience
pub use worker::{Context, Env, Request, Response};

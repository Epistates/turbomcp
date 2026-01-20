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
//! - Integration with Cloudflare Workers SDK
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::*;
//! use worker::*;
//! use serde::Deserialize;
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
//! #[event(fetch)]
//! async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
//!     let server = McpServer::builder("my-mcp-server", "1.0.0")
//!         .tool("hello", "Say hello to someone", hello)
//!         .tool("fetch", "Fetch data from URL", fetch_data)
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
//! # Building for WASM Environments
//!
//! ```bash
//! # Build for Cloudflare Workers
//! cargo build --target wasm32-unknown-unknown --release
//!
//! # Or using wrangler (Cloudflare)
//! wrangler dev
//! ```

mod ext;
mod handler;
mod handler_traits;
#[cfg(test)]
mod integration_tests;
mod response;
mod server;
mod traits;
mod types;

#[cfg(feature = "auth")]
mod auth_middleware;

// Re-export the extension trait for unified McpHandler support
// This enables "write once, run everywhere" - any McpHandler can be used
// directly in WASM via .handle_worker_request()
pub use ext::WasmHandlerExt;

// Re-export the main server types
pub use server::{McpServer, McpServerBuilder};

// Re-export result types
pub use types::{PromptResult, ResourceResult, ToolResult};

// Re-export the response trait and types for ergonomic handlers
pub use response::{Image, IntoToolResponse, Json, Text, ToolError};

// Re-export handler traits for advanced use cases
pub use response::IntoToolError;
pub use traits::{
    IntoPromptResponse, IntoResourceResponse, PromptHandlerFn, ResourceHandlerFn, ResultExt,
    ToolHandlerFn,
};

// Re-export handler trait bounds for advanced use cases
pub use handler_traits::{IntoPromptHandler, IntoResourceHandler, IntoToolHandler};

// Re-export authentication middleware when auth feature is enabled
#[cfg(feature = "auth")]
pub use auth_middleware::{AuthExt, WithAuth};

/// Re-export worker types for convenience
pub use worker::{Context, Env, Request, Response};

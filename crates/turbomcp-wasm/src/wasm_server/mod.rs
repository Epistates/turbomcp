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
//! - Integration with Cloudflare Workers SDK
//!
//! # Example
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::{McpServer, ToolResult};
//! use worker::*;
//!
//! #[event(fetch)]
//! async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
//!     let server = McpServer::builder("my-mcp-server", "1.0.0")
//!         .with_tool("hello", "Say hello to someone", |args: HelloArgs| async move {
//!             Ok(ToolResult::text(format!("Hello, {}!", args.name)))
//!         })
//!         .with_tool("add", "Add two numbers", |args: AddArgs| async move {
//!             Ok(ToolResult::text(format!("{}", args.a + args.b)))
//!         })
//!         .build();
//!
//!     server.handle(req).await
//! }
//!
//! #[derive(serde::Deserialize, schemars::JsonSchema)]
//! struct HelloArgs {
//!     name: String,
//! }
//!
//! #[derive(serde::Deserialize, schemars::JsonSchema)]
//! struct AddArgs {
//!     a: i64,
//!     b: i64,
//! }
//! ```
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

mod handler;
mod server;
mod types;

pub use handler::McpHandler;
pub use server::{McpServer, McpServerBuilder};
pub use types::{PromptResult, ResourceResult, ToolResult};

/// Re-export worker types for convenience
pub use worker::{Context, Env, Request, Response};

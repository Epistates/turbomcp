//! `TurboMCP` WebAssembly Bindings
//!
//! This crate provides WebAssembly bindings for TurboMCP, enabling MCP clients and servers
//! to run in browsers, WASI environments, and Cloudflare Workers.
//!
//! # Features
//!
//! - **browser** (default): Browser MCP client using wasm-bindgen and web-sys
//! - **wasi**: WASI Preview 2 MCP client for server-side WASM runtimes
//! - **wasm-server**: Full MCP server support for WASM environments (Cloudflare Workers, etc.)
//! - **macros**: Zero-boilerplate procedural macros for WASM MCP servers
//!
//! # Browser Usage (Client)
//!
//! ```javascript
//! import init, { McpClient, Tool, Content } from 'turbomcp-wasm';
//!
//! await init();
//!
//! const client = new McpClient("https://api.example.com/mcp");
//! await client.initialize();
//!
//! const tools = await client.listTools();
//! console.log("Available tools:", tools);
//!
//! const result = await client.callTool("my_tool", { arg: "value" });
//! console.log("Result:", result);
//! ```
//!
//! # Cloudflare Workers Usage (Server)
//!
//! Build MCP servers that run on Cloudflare's edge network:
//!
//! ```ignore
//! use turbomcp_wasm::wasm_server::{McpServer, ToolResult};
//! use worker::*;
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, schemars::JsonSchema)]
//! struct HelloArgs {
//!     name: String,
//! }
//!
//! #[derive(Deserialize, schemars::JsonSchema)]
//! struct AddArgs {
//!     a: i64,
//!     b: i64,
//! }
//!
//! #[event(fetch)]
//! async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
//!     let server = McpServer::builder("my-mcp-server", "1.0.0")
//!         .description("My MCP server running on Cloudflare Workers")
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
//! ```
//!
//! ## Cloudflare Worker Setup
//!
//! ```toml
//! # Cargo.toml
//! [dependencies]
//! turbomcp-wasm = { version = "3.0", default-features = false, features = ["wasm-server"] }
//! worker = "0.7"
//! serde = { version = "1.0", features = ["derive"] }
//! schemars = "1.0"  # For automatic JSON schema generation
//! getrandom = { version = "0.3", features = ["wasm_js"] }
//! ```
//!
//! ```bash
//! # Build for Cloudflare Workers
//! wrangler dev
//!
//! # Or build manually
//! cargo build --target wasm32-unknown-unknown --release
//! ```
//!
//! # Macro Usage (Zero-Boilerplate)
//!
//! With the `macros` feature, you can define MCP servers with minimal code:
//!
//! ```ignore
//! use turbomcp_wasm::prelude::*;
//! use serde::Deserialize;
//!
//! #[derive(Clone)]
//! struct MyServer {
//!     greeting: String,
//! }
//!
//! #[derive(Deserialize, schemars::JsonSchema)]
//! struct GreetArgs {
//!     name: String,
//! }
//!
//! #[server(name = "my-server", version = "1.0.0")]
//! impl MyServer {
//!     #[tool("Greet someone by name")]
//!     async fn greet(&self, args: GreetArgs) -> String {
//!         format!("{}, {}!", self.greeting, args.name)
//!     }
//!
//!     #[tool("Get server status")]
//!     async fn status(&self) -> String {
//!         "Server is running".to_string()
//!     }
//!
//!     #[resource("config://app")]
//!     async fn config(&self, uri: String) -> ResourceResult {
//!         ResourceResult::text(&uri, r#"{"theme": "dark"}"#)
//!     }
//!
//!     #[prompt("Default greeting")]
//!     async fn greeting_prompt(&self) -> PromptResult {
//!         PromptResult::user("Hello! How can I help?")
//!     }
//! }
//!
//! #[event(fetch)]
//! async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
//!     let server = MyServer { greeting: "Hello".into() };
//!     server.into_mcp_server().handle(req).await
//! }
//! ```
//!
//! ## Macros Setup
//!
//! ```toml
//! [dependencies]
//! turbomcp-wasm = { version = "3.0", default-features = false, features = ["macros"] }
//! worker = "0.7"
//! serde = { version = "1.0", features = ["derive"] }
//! schemars = "1.0"
//! ```
//!
//! # WASI Usage (Client)
//!
//! For WASI environments (Wasmtime, WasmEdge, Wasmer, etc.):
//!
//! ```ignore
//! use turbomcp_wasm::wasi::{McpClient, StdioTransport, HttpTransport};
//!
//! // STDIO transport (for MCP servers via process communication)
//! let transport = StdioTransport::new();
//! let mut client = McpClient::with_stdio(transport);
//! client.initialize()?;
//!
//! // HTTP transport (for HTTP-based MCP servers)
//! let transport = HttpTransport::new("https://api.example.com/mcp")
//!     .with_header("Authorization", "Bearer token");
//! let mut client = McpClient::with_http(transport);
//! client.initialize()?;
//!
//! // Use the client
//! let tools = client.list_tools()?;
//! let result = client.call_tool("my_tool", Some(serde_json::json!({"arg": "value"})))?;
//! ```
//!
//! ## Building for WASI
//!
//! ```bash
//! # Add the wasm32-wasip2 target
//! rustup target add wasm32-wasip2
//!
//! # Build with WASI feature
//! cargo build -p turbomcp-wasm --target wasm32-wasip2 --features wasi --no-default-features
//!
//! # Run with Wasmtime (with HTTP support)
//! wasmtime run --wasi http target/wasm32-wasip2/debug/your_app.wasm
//! ```
//!
//! # Binary Size
//!
//! This crate targets minimal binary size with proper optimization:
//!
//! | Configuration | Unoptimized | With wasm-opt |
//! |---------------|-------------|---------------|
//! | Core types    | ~400KB      | ~150KB        |
//! | + JSON        | ~600KB      | ~250KB        |
//! | + HTTP client | ~1.1MB      | ~400KB        |
//!
//! For smallest binaries, build with `--profile wasm-release` and use `wasm-opt -Oz`:
//! ```bash
//! # Browser target
//! cargo build -p turbomcp-wasm --target wasm32-unknown-unknown --profile wasm-release
//! wasm-opt -Oz -o optimized.wasm target/wasm32-unknown-unknown/wasm-release/turbomcp_wasm.wasm
//!
//! # WASI target
//! cargo build -p turbomcp-wasm --target wasm32-wasip2 --features wasi \
//!     --no-default-features --profile wasm-release
//! wasm-opt -Oz -o optimized.wasm target/wasm32-wasip2/wasm-release/turbomcp_wasm.wasm
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Re-export core types for WASM consumers
pub use turbomcp_core::error::{ErrorKind, McpError};
pub use turbomcp_core::types::{
    capabilities::{ClientCapabilities, ServerCapabilities},
    content::{Content, ResourceContent},
    core::{Implementation, Role},
    initialization::{InitializeRequest, InitializeResult},
    prompts::{GetPromptResult, Prompt, PromptArgument},
    resources::{Resource, ResourceTemplate},
    tools::{CallToolResult, Tool, ToolInputSchema},
};

#[cfg(feature = "browser")]
#[cfg_attr(docsrs, doc(cfg(feature = "browser")))]
pub mod browser;

#[cfg(feature = "wasi")]
#[cfg_attr(docsrs, doc(cfg(feature = "wasi")))]
pub mod wasi;

#[cfg(feature = "wasm-server")]
#[cfg_attr(docsrs, doc(cfg(feature = "wasm-server")))]
pub mod wasm_server;

#[cfg(feature = "wasm-server")]
#[cfg_attr(docsrs, doc(cfg(feature = "wasm-server")))]
pub mod prelude;

#[cfg(all(feature = "auth", target_arch = "wasm32"))]
#[cfg_attr(docsrs, doc(cfg(feature = "auth")))]
pub mod auth;

// Re-export proc macros when the macros feature is enabled
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
pub use turbomcp_wasm_macros::{prompt, resource, server, tool};

/// Version of the TurboMCP WASM bindings
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP protocol version supported (re-exported from core - single source of truth)
pub use turbomcp_core::PROTOCOL_VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        // Verify version is a valid semver-like format (contains at least one dot)
        assert!(VERSION.contains('.'), "VERSION should be semver format");
    }

    #[test]
    fn test_protocol_version() {
        assert_eq!(PROTOCOL_VERSION, "2025-11-25");
    }

    #[test]
    fn test_core_types_available() {
        // Verify core types are re-exported correctly
        let _impl = Implementation {
            name: "test".to_string(),
            title: None,
            description: None,
            version: "1.0.0".to_string(),
            icon: None,
        };

        let _caps = ClientCapabilities::default();
        let _content = Content::Text {
            text: "hello".to_string(),
            annotations: None,
        };
    }
}

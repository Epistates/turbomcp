//! # TurboMCP - Model Context Protocol SDK
//!
//! Rust SDK for the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/)
//! with zero-boilerplate macros, transport-agnostic design, and WASM support.
//!
//! ## Features
//!
//! - **Zero Boilerplate** - `#[server]`, `#[tool]`, `#[resource]`, `#[prompt]` macros
//! - **Transport Agnostic** - STDIO, HTTP, WebSocket, TCP, Unix sockets
//! - **Runtime Selection** - Choose transport at runtime without recompilation
//! - **BYO Server** - Integrate with existing Axum/Tower infrastructure
//! - **WASM Ready** - no_std compatible core for edge deployment
//! - **Type Safe** - Automatic JSON schema generation from Rust types
//!
//! ## Quick Start
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
//!     async fn add(&self, a: i64, b: i64) -> i64 {
//!         a + b
//!     }
//!
//!     /// Multiply two numbers
//!     #[tool]
//!     async fn multiply(&self, a: i64, b: i64) -> i64 {
//!         a * b
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     Calculator.serve().await.unwrap();
//! }
//! ```
//!
//! ## Runtime Transport Selection
//!
//! ```rust,ignore
//! use turbomcp::prelude::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     let transport = std::env::var("MCP_TRANSPORT").unwrap_or_default();
//!
//!     Calculator.builder()
//!         .transport(match transport.as_str() {
//!             "http" => Transport::http("0.0.0.0:8080"),
//!             "ws" => Transport::websocket("0.0.0.0:8080"),
//!             "tcp" => Transport::tcp("0.0.0.0:9000"),
//!             _ => Transport::stdio(),
//!         })
//!         .serve()
//!         .await
//!         .unwrap();
//! }
//! ```
//!
//! ## BYO Server (Axum Integration)
//!
//! ```rust,ignore
//! use axum::{Router, routing::get};
//! use turbomcp::prelude::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Get MCP routes as an Axum router
//!     let mcp = Calculator.builder().into_axum_router();
//!
//!     // Merge with your existing routes
//!     let app = Router::new()
//!         .route("/health", get(|| async { "OK" }))
//!         .merge(mcp);
//!
//!     // Run with your own server
//!     let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
//!     axum::serve(listener, app).await.unwrap();
//! }
//! ```
//!
//! ## Feature Flags
//!
//! Choose the right feature set for your use case:
//!
//! ```toml
//! # Minimal (STDIO only, recommended for CLI tools)
//! turbomcp = { version = "3.0", default-features = false, features = ["minimal"] }
//!
//! # Full (all transports)
//! turbomcp = { version = "3.0", features = ["full"] }
//! ```
//!
//! Available features:
//! - `stdio` - Standard I/O transport (default, works with Claude Desktop)
//! - `http` - HTTP/SSE transport
//! - `websocket` - WebSocket bidirectional transport
//! - `tcp` - Raw TCP socket transport
//! - `unix` - Unix domain socket transport

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]
#![warn(clippy::all)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::struct_excessive_bools,
    clippy::missing_panics_doc,
    clippy::default_trait_access,
    clippy::missing_const_for_fn,
    clippy::use_self,
    clippy::uninlined_format_args
)]

/// TurboMCP version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// TurboMCP crate name
pub const CRATE_NAME: &str = env!("CARGO_PKG_NAME");

// ============================================================================
// Core Re-exports
// ============================================================================

// Re-export macros
pub use turbomcp_macros::{description, prompt, resource, server, tool};

// Re-export core types
pub use turbomcp_core::context::RequestContext;
pub use turbomcp_core::error::{McpError, McpResult};
pub use turbomcp_core::handler::McpHandler;

// Re-export types
pub use turbomcp_types::{
    IntoPromptResult, IntoResourceResult, IntoToolResult, Message, Prompt, PromptArgument,
    PromptResult, Resource, ResourceContent, ResourceResult, ServerInfo, Tool, ToolInputSchema,
    ToolResult,
};

// Re-export server builder and transport
pub use turbomcp_server::{
    McpHandlerExt, McpServerExt, ServerBuilder, ServerConfig, ServerConfigBuilder, Transport,
};

// Re-export protocol types for advanced usage
pub use turbomcp_protocol::{
    CallToolRequest, CallToolResult, ClientCapabilities, Image, InitializeRequest,
    InitializeResult, IntoToolError, IntoToolResponse, Json, JsonRpcError, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, MessageId, ServerCapabilities, Text, ToolError,
};

// ============================================================================
// Optional Re-exports
// ============================================================================

/// Authentication and OAuth support
#[cfg(feature = "auth")]
pub use turbomcp_auth as auth;

/// DPoP (RFC 9449) support
#[cfg(feature = "dpop")]
pub use turbomcp_dpop as dpop;

/// Client library for full-stack development
#[cfg(feature = "client-integration")]
pub use turbomcp_client;

// ============================================================================
// Internal Macro Support
// ============================================================================

/// Internal module for macro-generated code.
///
/// **WARNING: This module is not part of the public API.**
///
/// These re-exports are used by procedural macros to generate code that
/// references dependencies without requiring users to add them to their
/// Cargo.toml.
#[doc(hidden)]
pub mod __macro_support {
    #[cfg(feature = "http")]
    pub use axum;

    pub use schemars;
    pub use serde_json;
    pub use tokio;
    pub use tower;
    pub use tracing;
    pub use uuid;

    pub use turbomcp_core;
    pub use turbomcp_protocol;
    pub use turbomcp_server;
    pub use turbomcp_transport;
    pub use turbomcp_types;
}

// ============================================================================
// Prelude
// ============================================================================

/// Convenient prelude for TurboMCP applications.
///
/// Import everything you need with a single use statement:
///
/// ```rust,ignore
/// use turbomcp::prelude::*;
///
/// #[derive(Clone)]
/// struct MyServer;
///
/// #[server(name = "my-server", version = "1.0.0")]
/// impl MyServer {
///     #[tool("My tool")]
///     async fn my_tool(&self) -> McpResult<String> {
///         Ok("Hello, world!".to_string())
///     }
/// }
/// ```
pub mod prelude {
    // Macros
    pub use super::{description, prompt, resource, server, tool};

    // Version info
    pub use super::{CRATE_NAME, VERSION};

    // Core traits and types
    pub use super::{
        McpError, McpHandler, McpHandlerExt, McpResult, McpServerExt, ServerBuilder, ServerConfig,
        ServerConfigBuilder, Transport,
    };

    // Result types for handlers
    pub use super::{
        IntoPromptResult, IntoResourceResult, IntoToolResult, PromptResult, ResourceResult,
        ToolResult,
    };

    // Common protocol types
    pub use super::{
        CallToolRequest, CallToolResult, Message, Prompt, PromptArgument, RequestContext, Resource,
        ResourceContent, ServerInfo, Tool, ToolInputSchema,
    };

    // Unified response types
    pub use super::{Image, IntoToolError, IntoToolResponse, Json, Text, ToolError};

    // Common external types
    pub use async_trait::async_trait;
    pub use serde::{Deserialize, Serialize};
    pub use serde_json;

    // ============================================================================
    // Transport Re-exports
    // ============================================================================

    /// Streamable HTTP server configuration
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use turbomcp_transport::streamable_http::{
        StreamableHttpConfig, StreamableHttpConfigBuilder,
    };

    /// Streamable HTTP client transport
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use turbomcp_transport::streamable_http_client::{
        RetryPolicy, StreamableHttpClientConfig, StreamableHttpClientTransport,
    };

    /// WebSocket bidirectional transport
    #[cfg(feature = "websocket")]
    #[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
    pub use turbomcp_transport::websocket_bidirectional::{
        WebSocketBidirectionalConfig, WebSocketBidirectionalTransport,
    };

    /// TCP transport
    #[cfg(feature = "tcp")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tcp")))]
    pub use turbomcp_transport::tcp::{TcpTransport, TcpTransportBuilder};

    /// Unix domain socket transport
    #[cfg(all(unix, feature = "unix"))]
    #[cfg_attr(docsrs, doc(cfg(all(unix, feature = "unix"))))]
    pub use turbomcp_transport::unix::{UnixTransport, UnixTransportBuilder};

    /// Client types for full-stack development
    #[cfg(feature = "client-integration")]
    #[cfg_attr(docsrs, doc(cfg(feature = "client-integration")))]
    pub use turbomcp_client::{Client, ClientBuilder, ClientCapabilities};
}

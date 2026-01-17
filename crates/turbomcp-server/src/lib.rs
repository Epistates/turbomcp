//! # TurboMCP Server
//!
//! Production-ready MCP (Model Context Protocol) server implementation with
//! zero-boilerplate development, transport-agnostic design, and WASM support.
//!
//! ## Features
//!
//! - **Zero Boilerplate** - Use `#[server]` and `#[tool]` macros for instant setup
//! - **Transport Agnostic** - STDIO, HTTP, WebSocket, TCP, Unix sockets
//! - **Runtime Selection** - Choose transport at runtime without recompilation
//! - **BYO Server** - Integrate with existing Axum/Tower infrastructure
//! - **WASM Ready** - no_std compatible core for edge deployment
//! - **Graceful Shutdown** - Clean termination with in-flight request handling
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use turbomcp_server::v3::prelude::*;
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
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // Simplest: uses STDIO by default
//!     Calculator.serve().await.unwrap();
//! }
//! ```
//!
//! ## Runtime Transport Selection
//!
//! ```rust,ignore
//! use turbomcp_server::v3::prelude::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     let transport = std::env::var("MCP_TRANSPORT").unwrap_or_default();
//!
//!     Calculator.builder()
//!         .transport(match transport.as_str() {
//!             "http" => Transport::http("0.0.0.0:8080"),
//!             "ws" => Transport::websocket("0.0.0.0:8080"),
//!             _ => Transport::stdio(),
//!         })
//!         .serve()
//!         .await
//!         .unwrap();
//! }
//! ```
//!
//! ## Bring Your Own Server (Axum Integration)
//!
//! ```rust,ignore
//! use axum::Router;
//! use turbomcp_server::v3::prelude::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Get MCP as an Axum router
//!     let mcp = Calculator.builder().into_axum_router();
//!
//!     // Merge with your app
//!     let app = Router::new()
//!         .route("/health", get(|| async { "OK" }))
//!         .merge(mcp);
//!
//!     let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
//!     axum::serve(listener, app).await?;
//! }
//! ```

#![deny(missing_docs)]
#![warn(missing_debug_implementations)]
#![warn(clippy::all)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::struct_excessive_bools,
    clippy::missing_panics_doc,
    clippy::default_trait_access
)]

/// v3 server architecture - the recommended API.
///
/// This module provides a clean, modern architecture with:
/// - Unified `McpHandler` trait for all MCP operations
/// - Zero-boilerplate through macro-generated implementations
/// - Transport-agnostic design (works on WASM and native)
/// - no_std compatible core (`turbomcp-core`)
/// - Single unified error type (`McpError`)
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::v3::prelude::*;
///
/// #[derive(Clone)]
/// struct MyServer;
///
/// #[server(name = "my-server", version = "1.0.0")]
/// impl MyServer {
///     #[tool]
///     async fn greet(&self, name: String) -> String {
///         format!("Hello, {}!", name)
///     }
/// }
///
/// #[tokio::main]
/// async fn main() {
///     MyServer.serve().await.unwrap();
/// }
/// ```
pub mod v3;

// Re-export v3 as the primary API
pub use v3::*;

/// Prelude for common server functionality.
///
/// Import everything you need with a single use statement:
///
/// ```rust,ignore
/// use turbomcp_server::prelude::*;
/// ```
pub mod prelude {
    pub use crate::v3::prelude::*;
}

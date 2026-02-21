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
//! use turbomcp_server::prelude::*;
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
//! use turbomcp_server::prelude::*;
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
//! use turbomcp_server::prelude::*;
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
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::struct_excessive_bools,
    clippy::default_trait_access
)]
// Note: missing_errors_doc and missing_panics_doc are now workspace-level warnings
// to improve API documentation quality for enterprise adoption

// Core modules
mod builder;
mod composite;
mod config;
mod context;
mod handler;
pub mod middleware;
mod router;
mod visibility;

/// Transport implementations for different protocols.
pub mod transport;

/// Progressive disclosure through component visibility control.
pub use visibility::{VisibilityLayer, VisibilitySessionGuard};

/// Server composition through handler mounting.
pub use composite::CompositeHandler;

/// Typed middleware for MCP request processing.
pub use middleware::{McpMiddleware, MiddlewareStack, Next};

// Public exports
pub use builder::{McpServerExt, ServerBuilder, Transport};
pub use config::{
    CapabilityValidation, ClientCapabilities, ConfigValidationError, ConnectionCounter,
    ConnectionGuard, ConnectionLimits, ProtocolConfig, RateLimitConfig, RateLimiter,
    RequiredCapabilities, SUPPORTED_PROTOCOL_VERSIONS, ServerConfig, ServerConfigBuilder,
};
pub use context::{RequestContext, TransportType};
pub use handler::McpHandlerExt;
pub use router::{
    JsonRpcIncoming, JsonRpcOutgoing, parse_request, route_request, route_request_with_config,
    serialize_response,
};

// Re-export McpHandler from core for unified architecture
pub use turbomcp_core::handler::McpHandler;

/// Type alias for backward compatibility (use `JsonRpcIncoming` for new code).
pub type JsonRpcRequest = JsonRpcIncoming;
/// Type alias for backward compatibility (use `JsonRpcOutgoing` for new code).
pub type JsonRpcResponse = JsonRpcOutgoing;

/// Internal module for macro-generated code.
#[doc(hidden)]
pub mod __macro_support {
    pub use schemars;
    pub use serde_json;
    pub use tokio;
    pub use tracing;
    pub use uuid;

    pub use turbomcp_core;
    pub use turbomcp_protocol;
    pub use turbomcp_types;
}

/// Prelude for easy imports.
///
/// This prelude provides everything needed to build MCP servers:
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::prelude::*;
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
pub mod prelude {
    // Core traits
    pub use super::{
        CompositeHandler, McpHandler, McpHandlerExt, McpMiddleware, McpServerExt, MiddlewareStack,
        VisibilityLayer, VisibilitySessionGuard,
    };

    // Builder and transport
    pub use super::{ServerBuilder, Transport};

    // Context types
    pub use super::{RequestContext, TransportType};

    // Configuration types
    pub use super::{
        ConnectionLimits, ProtocolConfig, RateLimitConfig, RateLimiter, RequiredCapabilities,
        ServerConfig, ServerConfigBuilder,
    };

    // Re-export error types from turbomcp-core (unified error handling)
    pub use turbomcp_core::error::{McpError, McpResult};

    // Re-export types from turbomcp-types
    pub use turbomcp_types::{
        // Result conversion traits
        IntoPromptResult,
        IntoResourceResult,
        IntoToolResult,
        // Core types
        Message,
        Prompt,
        PromptArgument,
        PromptResult,
        Resource,
        ResourceContents,
        ResourceResult,
        ServerInfo,
        Tool,
        ToolInputSchema,
        ToolResult,
    };
}

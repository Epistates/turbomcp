//! TurboMCP v3 - Pristine server architecture.
//!
//! This module provides the v3 server implementation with:
//! - Single source of truth for types via `turbomcp-types`
//! - Unified `McpHandler` trait for all MCP operations
//! - Zero boilerplate through macro-generated implementations
//! - Transport-agnostic design (works on WASM and native)
//! - Fluent builder API for runtime configuration
//!
//! # Quick Start
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
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     // Simplest: uses STDIO by default
//!     Calculator.serve().await.unwrap();
//! }
//! ```
//!
//! # Runtime Transport Selection
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
//!             _ => Transport::stdio(),
//!         })
//!         .serve()
//!         .await
//!         .unwrap();
//! }
//! ```
//!
//! # Bring Your Own Server (Axum Integration)
//!
//! ```rust,ignore
//! use axum::Router;
//! use turbomcp::prelude::*;
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

mod builder;
mod config;
mod context;
mod handler;
mod router;
pub mod transport;

pub use builder::{McpServerExt, ServerBuilder, Transport};
pub use config::{
    CapabilityValidation, ClientCapabilities, ConnectionCounter, ConnectionGuard, ConnectionLimits,
    ProtocolConfig, RateLimitConfig, RateLimiter, RequiredCapabilities,
    SUPPORTED_PROTOCOL_VERSIONS, ServerConfig, ServerConfigBuilder,
};
pub use context::{RequestContext, TransportType};
pub use handler::McpHandlerExt;
// Re-export McpHandler from core for unified architecture
pub use router::{
    JsonRpcIncoming, JsonRpcOutgoing, parse_request, route_request, route_request_with_config,
    serialize_response,
};
pub use turbomcp_core::handler::McpHandler;
/// Type alias for backward compatibility (use `JsonRpcIncoming` for new code).
pub type JsonRpcRequest = JsonRpcIncoming;
/// Type alias for backward compatibility (use `JsonRpcOutgoing` for new code).
pub type JsonRpcResponse = JsonRpcOutgoing;

/// v3 prelude for easy imports.
///
/// This prelude provides everything needed to build v3 MCP servers:
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
pub mod prelude {
    // Core traits
    pub use super::{McpHandler, McpHandlerExt, McpServerExt};

    // Builder and transport
    pub use super::{ServerBuilder, Transport};

    // Context types
    pub use super::{RequestContext, TransportType};

    // Configuration types
    pub use super::{
        ConnectionLimits, ProtocolConfig, RateLimitConfig, RateLimiter, RequiredCapabilities,
        ServerConfig, ServerConfigBuilder,
    };

    // Re-export error types from turbomcp-core (unified v3 error handling)
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
        ResourceContent,
        ResourceResult,
        ServerInfo,
        Tool,
        ToolInputSchema,
        ToolResult,
    };
}

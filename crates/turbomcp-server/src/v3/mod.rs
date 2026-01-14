//! TurboMCP v3 - Pristine server architecture.
//!
//! This module provides the v3 server implementation with:
//! - Single source of truth for types via `turbomcp-types`
//! - Unified `McpHandler` trait for all MCP operations
//! - Zero boilerplate through macro-generated implementations
//! - Transport-agnostic design (works on WASM and native)
//!
//! # Example
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
//!     Calculator.run_stdio().await.unwrap();
//! }
//! ```

mod config;
mod context;
mod handler;
mod router;

pub use config::{
    CapabilityValidation, ClientCapabilities, ConnectionCounter, ConnectionGuard, ConnectionLimits,
    ProtocolConfig, RateLimitConfig, RateLimiter, RequiredCapabilities,
    SUPPORTED_PROTOCOL_VERSIONS, ServerConfig, ServerConfigBuilder,
};
pub use context::{RequestContext, TransportType};
pub use handler::{McpHandler, McpHandlerExt};
pub use router::{
    JsonRpcRequest, JsonRpcResponse, parse_request, route_request, route_request_with_config,
    serialize_response,
};

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
/// impl McpHandler for MyServer {
///     fn server_info(&self) -> ServerInfo {
///         ServerInfo::new("my-server", "1.0.0")
///     }
///     // ... other trait methods
/// }
/// ```
pub mod prelude {
    // Core traits
    pub use super::{McpHandler, McpHandlerExt};

    // Context types
    pub use super::{RequestContext, TransportType};

    // Configuration types
    pub use super::{
        ConnectionLimits, ProtocolConfig, RateLimitConfig, RateLimiter, RequiredCapabilities,
        ServerConfig, ServerConfigBuilder,
    };

    // Re-export types from turbomcp-types
    pub use turbomcp_types::{
        // Result types
        IntoPromptResult,
        IntoResourceResult,
        IntoToolResult,
        McpError,
        McpResult,
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

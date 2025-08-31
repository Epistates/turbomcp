//! # TurboMCP Server
//!
//! MCP (Model Context Protocol) server implementation with graceful shutdown,
//! routing, lifecycle management, and comprehensive observability features.
//!
//! ## Features
//!
//! - **Graceful Shutdown** - Shutdown handling with signal support
//! - **Multi-Transport** - STDIO, TCP, Unix socket support with runtime selection
//! - **Middleware Stack** - Authentication, rate limiting, and security headers
//! - **Request Routing** - Efficient handler registration and dispatch
//! - **Health Monitoring** - Comprehensive health checks and metrics
//! - **Error Recovery** - Robust error handling and recovery mechanisms
//! - **MCP Compliance** - Full support for tools, prompts, resources, and sampling
//!
//! ## Example
//!
//! ```no_run
//! use turbomcp_server::ServerBuilder;
//! use tokio::signal;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let server = ServerBuilder::new()
//!         .name("MyServer")
//!         .version("1.0.0")
//!         .build();
//!     
//!     // Get shutdown handle for graceful termination
//!     let shutdown_handle = server.shutdown_handle();
//!     
//!     // In production: spawn server and wait for shutdown
//!     // tokio::spawn(async move { server.run_stdio().await });
//!     // signal::ctrl_c().await?;
//!     // shutdown_handle.shutdown().await;
//!     
//!     Ok(())
//! }
//! ```

#![deny(missing_docs)]
#![warn(missing_debug_implementations)]
#![warn(clippy::all)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,  // Error documentation in progress
    clippy::must_use_candidate,  // Too pedantic for library APIs
    clippy::return_self_not_must_use,  // Constructor methods don't need must_use
    clippy::struct_excessive_bools,  // Sometimes bools are the right design
    clippy::missing_panics_doc,  // Panic docs added where genuinely needed
    clippy::default_trait_access  // Default::default() is sometimes clearer
)]

/// Server name
pub const SERVER_NAME: &str = "turbomcp-server";
/// Server version
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod config;
pub mod error;
pub mod handlers;
pub mod lifecycle;
pub mod metrics;
pub mod middleware;
pub mod registry;
pub mod routing;
pub mod server;

// Re-export main types for convenience
pub use config::{Configuration, ConfigurationBuilder, ServerConfig};
pub use error::{ServerError, ServerResult};
pub use handlers::{
    CompletionHandler, ElicitationHandler, LoggingHandler, PingHandler, PromptHandler,
    ResourceHandler, ResourceTemplateHandler, SamplingHandler, ToolHandler,
};
pub use lifecycle::{HealthStatus, ServerLifecycle, ShutdownSignal};
pub use metrics::{MetricsCollector, ServerMetrics};
pub use middleware::{
    AuthenticationMiddleware, LoggingMiddleware, Middleware, MiddlewareLayer, MiddlewareStack,
    RateLimitMiddleware, SecurityHeadersConfig, SecurityHeadersMiddleware,
};
pub use registry::{HandlerRegistry, Registry, RegistryBuilder};
pub use routing::{RequestRouter, Route, Router};
pub use server::{McpServer, ServerBuilder, ShutdownHandle};

// Re-export protocol types
pub use turbomcp_protocol::jsonrpc::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, JsonRpcVersion,
};
pub use turbomcp_protocol::types::{CallToolRequest, CallToolResult, ListToolsResult, Tool};
pub use turbomcp_protocol::types::{ClientCapabilities, ServerCapabilities};

// Re-export core functionality
pub use turbomcp_core::{MessageId, RequestContext};

/// Default server configuration
#[must_use]
pub fn default_config() -> ServerConfig {
    ServerConfig::default()
}

/// Create a new server builder
#[must_use]
pub fn server() -> ServerBuilder {
    ServerBuilder::new()
}

/// Prelude for common server functionality
pub mod prelude {
    pub use crate::{
        AuthenticationMiddleware, HealthStatus, LoggingMiddleware, McpServer, Middleware,
        MiddlewareStack, PromptHandler, RateLimitMiddleware, Registry, RegistryBuilder,
        RequestRouter, ResourceHandler, Router, SamplingHandler, SecurityHeadersConfig,
        SecurityHeadersMiddleware, ServerBuilder, ServerConfig, ServerError, ServerLifecycle,
        ServerResult, ToolHandler, default_config, server,
    };

    // Re-export macros
    pub use turbomcp_macros::{prompt, resource, server as server_macro, tool};
}

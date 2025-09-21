//! # TurboMCP Server
//!
//! MCP (Model Context Protocol) server implementation with graceful shutdown,
//! routing, lifecycle management, and comprehensive observability features.
//!
//! ## Features
//!
//! - **Roots Configuration** - Configurable filesystem roots via builder API or macro
//! - **Elicitation Support** - Server-initiated requests for interactive user input
//! - **Sampling Protocol** - Bidirectional LLM sampling with client interaction
//! - **Graceful Shutdown** - Shutdown handling with signal support
//! - **Multi-Transport** - STDIO, TCP, Unix, WebSocket, HTTP/SSE support
//! - **Middleware Stack** - Authentication, rate limiting, and security headers
//! - **Request Routing** - Efficient handler registration and dispatch
//! - **Health Monitoring** - Comprehensive health checks and metrics
//! - **Error Recovery** - Robust error handling and recovery mechanisms
//! - **MCP Compliance** - Full support for tools, prompts, resources, roots, sampling, and elicitation
//! - **Server-Initiated Requests** - Support for sampling and elicitation via `ServerCapabilities`
//!
//! ## Example
//!
//! ```no_run
//! use turbomcp_server::ServerBuilder;
//! use turbomcp_protocol::types::Root;
//! use tokio::signal;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let server = ServerBuilder::new()
//!         .name("MyServer")
//!         .version("1.0.0")
//!         // Configure filesystem roots
//!         .root("file:///workspace", Some("Workspace".to_string()))
//!         .root("file:///tmp", Some("Temp".to_string()))
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
//!
//! ## Elicitation Support (New in 1.0.3)
//!
//! The server now includes comprehensive elicitation support for interactive user input:
//!
//! - `ElicitationCoordinator` for managing elicitation lifecycle
//! - Request/response correlation with timeouts
//! - Retry logic with configurable attempts
//! - Priority-based request queuing
//! - Automatic cleanup of expired requests
//!
//! ## Sampling Support (New in 1.0.3)
//!
//! The server provides built-in support for server-initiated sampling requests to clients:
//!
//! ```rust,no_run
//! use turbomcp_server::sampling::SamplingExt;
//! use turbomcp_core::RequestContext;
//! use turbomcp_protocol::types::{CreateMessageRequest, SamplingMessage, Role, Content, TextContent};
//!
//! async fn my_tool(ctx: RequestContext) -> Result<String, Box<dyn std::error::Error>> {
//!     // Create a sampling request
//!     let request = CreateMessageRequest {
//!         messages: vec![SamplingMessage {
//!             role: Role::User,
//!             content: Content::Text(TextContent {
//!                 text: "What is 2+2?".to_string(),
//!                 annotations: None,
//!                 meta: None,
//!             }),
//!         }],
//!         max_tokens: 50,
//!         // ... other fields
//!         # model_preferences: None,
//!         # system_prompt: Some("You are a helpful math assistant.".to_string()),
//!         # include_context: Some(turbomcp_protocol::types::IncludeContext::None),
//!         # temperature: Some(0.7),
//!         # stop_sequences: None,
//!         # metadata: None,
//!     };
//!     
//!     // Send the request to the client
//!     let result = ctx.create_message(request).await?;
//!     Ok(format!("Response: {:?}", result))
//! }
//! ```
//!
//! **Sampling Features:**
//! - Client-side sampling configuration support
//! - Server-side sampling metadata tracking
//! - Integration with elicitation for dynamic sampling decisions
//! - Configurable timeouts and retry logic
//!
//! ## Compile-Time Routing (Experimental - New in 1.0.3)
//!
//! Zero-cost compile-time router generation for high-throughput scenarios:
//!
//! - Zero-cost compile-time router generation
//! - Type-safe route matching at compile time
//! - Automatic handler registration through macros
//! - Performance optimization for high-throughput scenarios
//!
//! *Note: Compile-time routing is experimental and may have limitations with some MCP protocol methods.*

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
pub mod elicitation;
pub mod error;
pub mod handlers;
// Temporarily disabled - compile-time routing replaces these
// #[cfg(feature = "http")]
// pub mod http_server;
pub mod lifecycle;
pub mod metrics;
pub mod middleware;
pub mod registry;
pub mod routing;
pub mod sampling;
pub mod server;
pub mod shared;
// #[cfg(feature = "http")]
// pub mod simple_http;

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
pub use shared::SharedServer;

// Re-export protocol types
pub use turbomcp_protocol::jsonrpc::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, JsonRpcVersion,
};
pub use turbomcp_protocol::types::{CallToolRequest, CallToolResult, ListToolsResult, Tool};
pub use turbomcp_protocol::types::{ClientCapabilities, ServerCapabilities};

// Re-export core functionality
pub use turbomcp_core::{MessageId, RequestContext};

// Elicitation support
pub use elicitation::{ElicitationCoordinator, ElicitationTransport, SharedElicitationCoordinator};

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

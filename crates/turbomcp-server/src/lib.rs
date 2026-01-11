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
//! ## Elicitation Support
//!
//! The server includes comprehensive elicitation support for interactive user input:
//!
//! - `ElicitationCoordinator` for managing elicitation lifecycle
//! - Request/response correlation with timeouts
//! - Retry logic with configurable attempts
//! - Priority-based request queuing
//! - Automatic cleanup of expired requests
//!
//! ## Sampling Support
//!
//! The server provides built-in support for server-initiated sampling requests to clients:
//!
//! ```rust,no_run
//! use turbomcp_server::sampling::SamplingExt;
//! use turbomcp_protocol::RequestContext;
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
//!             metadata: None,
//!         }],
//!         max_tokens: 50,
//!         model_preferences: None,
//!         system_prompt: Some("You are a helpful math assistant.".to_string()),
//!         include_context: Some(turbomcp_protocol::types::IncludeContext::None),
//!         temperature: Some(0.7),
//!         stop_sequences: None,
//!         task: None,
//!         tools: None,
//!         tool_choice: None,
//!         _meta: None,
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
//! ## Compile-Time Routing (Experimental)
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

// Sprint 2.7: Compile-time security warnings for dangerous feature combinations
//
// Note: These are placeholder warnings for future security features:
// - dpop-ed25519: EdDSA-based DPoP (more secure than RS256)
// - tls: TLS/HTTPS support (recommended for production HTTP servers)
//
// When these features are added, uncomment the checks below:
//
// #[cfg(all(feature = "dpop", not(feature = "dpop-ed25519")))]
// compile_error!("DPoP should use EdDSA or ES256, not RS256");
//
// #[cfg(all(feature = "http", not(feature = "tls")))]
// const _HTTP_TLS_WARNING: () = ();

/// Server name
pub const SERVER_NAME: &str = "turbomcp-server";
/// Server version
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod capabilities;
pub mod config;
pub mod elicitation;
pub mod error;
pub mod error_sanitization; // Sprint 3.1: Error message sanitization
pub mod handler_validation;
pub mod handlers; // Sprint 2.4: Handler name injection prevention
// Temporarily disabled - compile-time routing replaces these
// #[cfg(feature = "http")]
// pub mod http_server;
pub mod lifecycle;
pub mod logging;
pub mod metrics;
#[cfg(feature = "middleware")]
pub mod middleware;
pub mod observability;
pub mod registry;
pub mod routing;
pub mod runtime;
pub mod sampling;
pub mod security_checks; // Sprint 2.6: Runtime security validation
pub mod server;
pub mod service;
#[cfg(feature = "experimental-tasks")]
pub mod task_storage; // Tasks API storage and lifecycle (SEP-1686)
#[cfg(feature = "multi-tenancy")]
pub mod tenant_context; // Sprint 3.2: Multi-tenant context extension trait
pub mod timeout;
// #[cfg(feature = "http")]
// pub mod simple_http;

// Re-export main types for convenience
pub use config::{
    Configuration, ConfigurationBuilder, LogOutput, LogRotation, LoggingConfig,
    ProtocolVersionConfig, ServerConfig,
};
pub use logging::LoggingGuard;
pub use error::{ErrorRecovery, McpError, McpResult, ServerErrorExt, ServerResult};
pub use handlers::{
    CompletionHandler, ElicitationHandler, LoggingHandler, PingHandler, PromptHandler,
    ResourceHandler, ResourceTemplateHandler, SamplingHandler, ToolHandler,
};
pub use lifecycle::{HealthStatus, ServerLifecycle, ShutdownSignal};
pub use metrics::{MetricsCollector, ServerMetrics};

// Re-export middleware components (feature-gated)
#[cfg(feature = "middleware")]
pub use middleware::{
    AuditConfig, AuditLayer, MiddlewareStack, SecurityConfig, SecurityLayer, TimeoutConfig,
    TimeoutLayer, ValidationConfig, ValidationLayer,
};

#[cfg(all(feature = "middleware", feature = "auth"))]
pub use middleware::{AuthConfig, AuthLayer};

#[cfg(all(feature = "middleware", feature = "rate-limiting"))]
pub use middleware::{RateLimitConfig, RateLimitLayer};

#[cfg(feature = "multi-tenancy")]
pub use tenant_context::TenantContextExt; // Extension trait for tenant support

pub use observability::{
    ObservabilityConfig, ObservabilityGuard, OtlpProtocol, PerformanceMonitor, SamplingConfig,
    SecurityAuditLogger, global_observability,
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
pub use turbomcp_protocol::{MessageId, RequestContext};

// Elicitation support
pub use elicitation::{ElicitationCoordinator, ElicitationTransport, SharedElicitationCoordinator};

// Transport configuration (for ergonomic access)
#[cfg(feature = "websocket")]
pub use config::WebSocketServerConfig;

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
    // Core types (always available)
    pub use crate::{
        HealthStatus, LogOutput, LogRotation, LoggingConfig, LoggingGuard, McpError, McpResult,
        McpServer, PromptHandler, Registry, RegistryBuilder, RequestRouter, ResourceHandler,
        Router, SamplingHandler, ServerBuilder, ServerConfig, ServerErrorExt, ServerLifecycle,
        ServerResult, ToolHandler, default_config, server,
    };

    // Middleware types (requires middleware feature)
    #[cfg(feature = "middleware")]
    pub use crate::{
        AuditConfig, AuditLayer, MiddlewareStack, SecurityConfig, SecurityLayer, TimeoutConfig,
        TimeoutLayer, ValidationConfig, ValidationLayer,
    };

    // Auth middleware (requires middleware + auth features)
    #[cfg(all(feature = "middleware", feature = "auth"))]
    pub use crate::{AuthConfig, AuthLayer};

    // Rate limiting middleware (requires middleware + rate-limiting features)
    #[cfg(all(feature = "middleware", feature = "rate-limiting"))]
    pub use crate::{RateLimitConfig, RateLimitLayer};

    // Re-export macros
    pub use turbomcp_macros::{prompt, resource, server as server_macro, tool};
}

//! # TurboMCP - Model Context Protocol SDK
//!
//! Rust SDK for the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/)
//! with SIMD acceleration, resilient transport layer, graceful shutdown, and ergonomic APIs.
//!
//! ## Features
//!
//! ### Core MCP Protocol Support
//! - **MCP 2025-06-18 Specification** - Full compliance with latest protocol including elicitation
//! - **Type Safety** - Compile-time validation with automatic schema generation
//! - **Context Injection** - Dependency injection and observability with structured logging
//! - **Zero-Overhead Macros** - Ergonomic `#[server]`, `#[tool]`, `#[resource]`, `#[prompt]` attributes
//!
//! ### Advanced Protocol Features
//! - **Roots Support** - Configurable filesystem roots via macro or builder API with OS-aware defaults
//! - **Zero Ceremony Elicitation** - Server-initiated user input with beautiful title-first builders (Enhanced 1.0.4)  
//! - **Sampling Protocol** - Bidirectional LLM sampling capabilities with metadata tracking
//! - **Compile-Time Routing** - Zero-cost compile-time router generation (experimental)
//!
//! ### Transport & Performance
//! - **Multi-Transport** - STDIO, TCP, Unix sockets, WebSocket, HTTP/SSE with runtime selection
//! - **SIMD-Accelerated JSON** - `simd-json` and `sonic-rs` for fast processing  
//! - **Robust** - Circuit breakers, retry logic, graceful shutdown
//! - **WebSocket Bidirectional** - Full-duplex communication for real-time elicitation
//! - **HTTP Server-Sent Events** - Server-push capabilities for lightweight deployments
//!
//! ### Enterprise Features
//! - **OAuth 2.1 MCP Compliance** - RFC 8707/9728/7591 compliant with MCP resource binding
//! - **Multi-Provider OAuth** - Google, GitHub, Microsoft with PKCE and security hardening
//! - **Security Headers** - CORS, CSP, HSTS protection with redirect attack prevention
//! - **Rate Limiting** - Token bucket algorithm with configurable strategies
//! - **Middleware Stack** - Authentication, logging, security headers
//!
//! ## Quick Start
//!
//! ```rust
//! use turbomcp::prelude::*;
//!
//! #[derive(Clone)]
//! struct Calculator;
//!
//! #[server(
//!     name = "calculator-server",
//!     version = "1.0.0",
//!     // Configure filesystem roots directly in the macro
//!     root = "file:///workspace:Workspace",
//!     root = "file:///tmp:Temporary Files"
//! )]
//! impl Calculator {
//!     #[tool("Add two numbers")]
//!     async fn add(&self, ctx: Context, a: i32, b: i32) -> McpResult<i32> {
//!         // Context injection provides automatic:
//!         // - Request correlation and distributed tracing
//!         // - Structured logging with metadata
//!         // - Performance monitoring and metrics
//!         ctx.info(&format!("Adding {} + {}", a, b)).await?;
//!         Ok(a + b)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let server = Calculator;
//!     server.run_stdio().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Graceful Shutdown
//!
//! TurboMCP provides graceful shutdown for all transport methods:
//!
//! ```no_run
//! use turbomcp::prelude::*;
//! use std::sync::Arc;
//!
//! #[derive(Clone)]
//! struct Calculator {
//!     operations: Arc<std::sync::atomic::AtomicU64>,
//! }
//!
//! #[server]
//! impl Calculator {
//!     #[tool("Add numbers")]
//!     async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
//!         Ok(a + b)
//!     }
//! }
//!
//! // Example: Get shutdown handle for graceful termination
//! let server = Calculator {
//!     operations: Arc::new(std::sync::atomic::AtomicU64::new(0)),
//! };
//!
//! // This gives you a handle to gracefully shutdown the server
//! let (server, shutdown_handle) = server.into_server_with_shutdown().unwrap();
//!
//! // In production, you'd spawn the server and wait for signals:
//! // tokio::spawn(async move { server.run_stdio().await });
//! // signal::ctrl_c().await?;
//! // shutdown_handle.shutdown().await;
//! ```
//!
//! ## Runtime Transport Selection
//!
//! ```ignore
//! // Note: This example requires the `tcp` and `unix` features to compile
//! // cargo run --features "tcp,unix"
//! use turbomcp::prelude::*;
//! use std::sync::Arc;
//!
//! #[derive(Clone)]
//! struct Calculator {
//!     operations: Arc<std::sync::atomic::AtomicU64>,
//! }
//!
//! #[server]
//! impl Calculator {
//!     #[tool("Add numbers")]
//!     async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
//!         Ok(a + b)
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let server = Calculator {
//!         operations: Arc::new(std::sync::atomic::AtomicU64::new(0)),
//!     };
//!
//!     // Runtime transport selection based on environment
//!     match std::env::var("TRANSPORT").as_deref() {
//!         Ok("tcp") => {
//!             // Requires "tcp" feature
//!             let port: u16 = std::env::var("PORT")
//!                 .unwrap_or_else(|_| "8080".to_string())
//!                 .parse()
//!                 .unwrap_or(8080);
//!             server.run_tcp(format!("127.0.0.1:{}", port)).await?;
//!         }
//!         Ok("unix") => {
//!             // Requires "unix" feature
//!             let path = std::env::var("SOCKET_PATH")
//!                 .unwrap_or_else(|_| "/tmp/mcp.sock".to_string());
//!             server.run_unix(path).await?;
//!         }
//!         _ => {
//!             // Always available (default feature)
//!             server.run_stdio().await?;
//!         }
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Error Handling
//!
//! TurboMCP provides ergonomic error handling with the `mcp_error!` macro:
//!
//! ```rust
//! use turbomcp::prelude::*;
//! use std::sync::Arc;
//!
//! #[derive(Clone)]
//! struct Calculator {
//!     operations: Arc<std::sync::atomic::AtomicU64>,
//! }
//!
//! #[server]
//! impl Calculator {
//!     #[tool("Divide two numbers")]
//!     async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
//!         if b == 0.0 {
//!             return Err(turbomcp::McpError::Tool("Cannot divide by zero".to_string()));
//!         }
//!         Ok(a / b)
//!     }
//! }
//! ```
//!
//! ## Elicitation Support - Zero Ceremony Builders (Enhanced in 1.0.4)
//!
//! TurboMCP provides ergonomic elicitation with zero ceremony builders for intuitive APIs:
//!
//! ```rust,ignore
//! use turbomcp::prelude::*;
//! use turbomcp::elicitation_api::ElicitationResult;
//! use turbomcp_macros::elicit;
//!
//! #[derive(Clone)]
//! struct ConfigServer;
//!
//! #[server]
//! impl ConfigServer {
//!     #[tool("Configure deployment")]
//!     async fn deploy(&self, ctx: Context, project: String) -> McpResult<String> {
//!         // Example elicitation for deployment configuration
//!         // Note: Actual implementation depends on client capabilities
//!
//!         Ok(format!("Deployed {} to production", project))
//!     }
//! }
//! ```
//!
//! **Key Features:**
//! - **Zero Ceremony**: `text("Title")`, `checkbox("Label")`, `integer_field("Name")`
//! - **No Boilerplate**: Field method accepts builders directly via `Into<T>`
//! - **Smart Constructors**: Title-first API eliminates nested parentheses
//! - **Backward Compatible**: Builder variants available as `*_builder()`
//!
//! ## Sampling Support (New in 1.0.3)
//!
//! Server-initiated sampling requests enable bidirectional LLM communication:
//!
//! ```rust,ignore
//! use turbomcp::prelude::*;
//! use turbomcp_protocol::CreateMessageRequest;
//!
//! #[derive(Clone)]
//! struct AIAssistant;
//!
//! #[server]
//! impl AIAssistant {
//!     #[tool("Get AI assistance for code review")]
//!     async fn code_review(&self, ctx: Context, code: String) -> McpResult<String> {
//!         // Example: Create a sampling request for AI analysis
//!         // Note: Requires client with LLM capability
//!
//!         // Fallback to simple analysis
//!         let issues = code.matches("TODO").count() + code.matches("FIXME").count();
//!         Ok(format!("Static analysis: {} lines, {} issues found", code.lines().count(), issues))
//!     }
//! }
//! ```
//!
//! ## OAuth 2.0 Authentication
//!
//! Built-in OAuth 2.0 support with multiple providers and all standard flows:
//!
//! ```rust,no_run
//! use turbomcp::prelude::*;
//! use std::collections::HashMap;
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! #[derive(Clone)]
//! struct AuthenticatedServer {
//!     user_sessions: Arc<RwLock<HashMap<String, String>>>,
//! }
//!
//! #[server]
//! impl AuthenticatedServer {
//!     #[tool("Get authenticated user profile")]
//!     async fn get_user_profile(&self, ctx: Context, session_token: String) -> McpResult<String> {
//!         let sessions = self.user_sessions.read().await;
//!         if let Some(user_id) = sessions.get(&session_token) {
//!             Ok(format!("Authenticated user: {}", user_id))
//!         } else {
//!             Err(McpError::InvalidInput("Authentication required".to_string()))
//!         }
//!     }
//!
//!     #[tool("Start OAuth flow")]
//!     async fn start_oauth_flow(&self, provider: String) -> McpResult<String> {
//!         match provider.as_str() {
//!             "github" | "google" | "microsoft" => {
//!                 Ok(format!("Visit: https://{}.com/oauth/authorize", provider))
//!             }
//!             _ => Err(McpError::InvalidInput(format!("Unknown provider: {}", provider))),
//!         }
//!     }
//! }
//! ```
//!
//! **OAuth Features:**
//! - 🔐 **Multiple Providers** - Google, GitHub, Microsoft, custom OAuth 2.0
//! - 🛡️ **Always-On PKCE** - Security enabled by default
//! - 🔄 **All OAuth Flows** - Authorization Code, Client Credentials, Device Code
//! - 👥 **Session Management** - User session tracking with cleanup
//!
//! ## Advanced Features
//!
//! TurboMCP supports resources and prompts alongside tools:
//!
//! ```rust
//! use turbomcp::prelude::*;
//! use std::sync::Arc;
//!
//! #[derive(Clone)]
//! struct Calculator {
//!     operations: Arc<std::sync::atomic::AtomicU64>,
//! }
//!
//! #[server]
//! impl Calculator {
//!     #[tool("Add numbers")]
//!     async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
//!         Ok(a + b)
//!     }
//!
//!     #[resource("calc://history")]
//!     async fn history(&self, _uri: String) -> McpResult<String> {
//!         Ok("Calculation history data".to_string())
//!     }
//!     
//!     #[prompt("Generate calculation report for {operation}")]
//!     async fn calc_report(&self, operation: String) -> McpResult<String> {
//!         Ok(format!("Report for {operation} operations"))
//!     }
//! }
//! ```
//!
//! ## 🎯 Feature Selection Guide
//!
//! Choose the right feature set for your use case to minimize dependencies:
//!
//! ### For Basic Tool Servers (Recommended for Beginners)
//! ```toml
//! [dependencies]
//! turbomcp = { version = "1.0", default-features = false, features = ["minimal"] }
//! ```
//! **What you get:** STDIO transport, core macros, basic error handling  
//! **Perfect for:** Simple MCP tools, CLI integrations, getting started  
//! **Transport:** `server.run_stdio().await`
//!
//! ### For Production Web Servers
//! ```toml
//! [dependencies]
//! turbomcp = { version = "1.0", features = ["full"] }
//! ```
//! **What you get:** All transports, authentication, databases, full feature set  
//! **Perfect for:** Production deployments, web applications, enterprise usage  
//! **Transports:** HTTP/SSE, WebSocket, TCP, Unix, STDIO
//!
//! ### Custom Feature Selection
//! ```toml
//! [dependencies]
//! turbomcp = {
//!     version = "1.0",
//!     default-features = false,
//!     features = ["minimal", "http", "schema-generation"]
//! }
//! ```
//! **Mix and match:** Start with `minimal` and add only what you need
//!
//! Available feature combinations:
//! - **`minimal`** - Just STDIO (works everywhere, 📦 smallest footprint)
//! - **`network`** - STDIO + TCP (network deployment ready)
//! - **`server-only`** - TCP + Unix (headless servers, no STDIO)
//! - **`full`** - Everything included (⚡ maximum functionality)
//!
//! For more examples and advanced usage, see the [examples directory](https://github.com/Epistates/turbomcp/tree/main/crates/turbomcp/examples).
//!
//! ## 📚 Generated Methods Reference
//!
//! The `#[server]` macro generates several key methods for your server implementation:
//!
//! ### Transport Methods
//!
//! The macro automatically generates transport methods that let you run your server
//! over different protocols. **All transport changes are one-line swaps:**
//!
//! ```rust,no_run
//! # use turbomcp::prelude::*;
//! # #[derive(Clone)]
//! # struct MyServer;
//! #
//! # #[server]
//! # impl MyServer {
//! #   #[tool("Example tool")]
//! #   async fn example(&self) -> McpResult<String> { Ok("test".to_string()) }
//! # }
//! #
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Each transport method consumes the server, so choose one:
//!
//! // STDIO (standard MCP transport)
//! MyServer.run_stdio().await?;
//!
//! // Or HTTP with Server-Sent Events (web-compatible)
//! // MyServer.run_http("127.0.0.1:8080").await?;
//! // MyServer.run_http_with_path("0.0.0.0:3000", "/api/mcp").await?;
//!
//! // Or TCP sockets (high-performance)
//! // MyServer.run_tcp("127.0.0.1:9000").await?;
//!
//! // Or Unix domain sockets (local IPC)
//! // MyServer.run_unix("/tmp/mcp.sock").await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Metadata and Testing Methods
//!
//! ```rust,ignore
//! # use turbomcp::prelude::*;
//! # #[derive(Clone)] struct MyServer;
//! # #[server(name="test", version="1.0.0")] impl MyServer {
//! #   #[tool("Test")] async fn test(&self) -> McpResult<String> { Ok("test".to_string()) }
//! # }
//! // Get server information (name, version, description)
//! let (name, version, description) = MyServer::server_info();
//!
//! // Get tool metadata for testing and introspection
//! let metadata = MyServer::__turbomcp_tool_metadata_test();
//!
//! // Test tool handlers directly (bypasses transport layer)
//! use turbomcp::CallToolRequest;
//! let server = MyServer;
//! let request = CallToolRequest {
//!     name: "test".to_string(),
//!     arguments: None
//! };
//! let result = server.__turbomcp_tool_handler_test(request, Default::default()).await;
//! ```
//!
//! ### Handler Registration
//!
//! The macro implements the `HandlerRegistration` trait automatically:
//!
//! ```rust,ignore
//! # use turbomcp::prelude::*;
//! # #[derive(Clone)] struct MyServer;
//! # #[server] impl MyServer {}
//! use turbomcp::{HandlerRegistration, ServerBuilder};
//!
//! let server = MyServer;
//! let mut builder = ServerBuilder::new();
//!
//! // All #[tool], #[resource], and #[prompt] methods are registered automatically
//! server.register_handlers(&mut builder).unwrap();
//! ```
//!
//! ### Feature-Gated Methods
//!
//! Some methods are only available when the corresponding features are enabled:
//!
//! - **`run_http()`, `run_http_with_path()`** - Requires `http` feature
//! - **`run_tcp()`** - Requires `tcp` feature  
//! - **`run_unix()`** - Requires `unix` feature (Unix systems only)
//!
//! ### Context Injection
//!
//! The macro automatically detects `Context` parameters and injects them properly:
//!
//! ```rust,ignore
//! # use turbomcp::prelude::*;
//! # #[derive(Clone)] struct Server;
//! # #[server] impl Server {
//! #[tool("Context can appear anywhere in parameters")]
//! async fn flexible_context(&self, name: String, ctx: Context, age: u32) -> McpResult<String> {
//!     ctx.info("Context works at any position").await?;
//!     Ok(format!("Hello {} ({})", name, age))
//! }
//!
//! #[tool("Context is optional")]  
//! async fn no_context(&self, message: String) -> McpResult<String> {
//!     Ok(format!("Received: {}", message))
//! }
//! # }
//! ```
//!
//! This generates the correct handler functions that extract parameters and inject context appropriately.
//!
//! ## Architecture
//!
//! - **MCP 2025-06-18 Specification** - Full protocol compliance including elicitation
//! - **Multi-Transport Support** - STDIO, TCP, Unix, WebSocket, HTTP/SSE
//! - **Bidirectional Communication** - Server-initiated requests via elicitation and sampling
//! - **Graceful Shutdown** - Lifecycle management
//! - **Zero-Overhead Macros** - Ergonomic `#[server]`, `#[tool]`, `#[resource]` attributes
//! - **Type Safety** - Compile-time validation and automatic schema generation
//! - **SIMD Acceleration** - High-throughput JSON processing

#![deny(missing_docs)]
#![warn(clippy::all)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,  // Too pedantic for library APIs
    clippy::return_self_not_must_use,  // Constructor methods don't need must_use
    clippy::struct_excessive_bools,  // Sometimes bools are the right design
    clippy::missing_panics_doc,  // Panic docs added where genuinely needed
    clippy::default_trait_access,  // Default::default() is sometimes clearer
    clippy::missing_const_for_fn,  // Const fn where it makes sense, not everywhere
    clippy::use_self,  // Sometimes explicit types are clearer
    clippy::uninlined_format_args  // Sometimes variables in format! are clearer
)]

use std::collections::HashMap;
use std::sync::Arc;

// async_trait re-exported below
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

// Re-export core types for convenience (all re-exported at top level of turbomcp_protocol)
// v2.0: Re-export essential types from turbomcp_protocol root + module-qualified types
pub use turbomcp_protocol::{
    CallToolRequest,
    CallToolResult,
    ClientCapabilities,
    InitializeRequest,
    InitializeResult,
    // JSON-RPC types
    JsonRpcError,
    JsonRpcNotification,
    JsonRpcRequest,
    JsonRpcResponse,
    // Core types (at protocol root)
    MessageId,
    RequestContext,
    ServerCapabilities,
};

// Re-export commonly used types from turbomcp_protocol::types
pub use turbomcp_protocol::types::{
    CompleteResult, CompletionResponse, Content, CreateMessageRequest, CreateMessageResult,
    ElicitRequest, ElicitResult, ElicitationAction, GetPromptResult, ImageContent, Implementation,
    ListRootsResult, PingRequest, PingResult, Resource, SamplingMessage, TextContent, Tool,
    ToolInputSchema,
};
pub use turbomcp_server::{
    McpServer, McpServer as Server, ServerBuilder, ServerError, ServerResult, ShutdownHandle,
    handlers,
};

// Re-export async_trait for macros
pub use async_trait::async_trait;

// CRITICAL FIX: Re-export dependencies needed by macro-generated code
// This allows generated code to use ::turbomcp::axum::Router instead of axum::Router
#[cfg(feature = "http")]
pub use axum;
// tokio and turbomcp_transport are always dependencies, always re-export for macros
pub use tokio;
// Re-export uuid for HTTP session ID generation in macro-generated code
pub use uuid;
// Re-export core and protocol types for macro use
pub use turbomcp_protocol;
pub use turbomcp_transport;
// Re-export tracing for logging in macro-generated code
pub use tracing;

// Core TurboMCP modules
// 2.0.0: Auth and DPoP extracted to separate optional crates
#[cfg(feature = "auth")]
pub use turbomcp_auth as auth;

#[cfg(feature = "dpop")]
pub use turbomcp_dpop as dpop;
pub mod context;
pub mod context_factory;
pub mod elicitation;
pub mod elicitation_api;
pub mod helpers;

pub mod injection;
pub mod lifespan;
pub mod progress;
pub mod registry;

pub mod router;
/// Runtime support for bidirectional MCP communication
///
/// Provides dispatchers and event loops for implementing bidirectional features
/// (sampling, elicitation, roots, ping) across different transport layers.
pub mod runtime;
pub mod server;
pub mod session;
pub mod simd;
#[cfg(feature = "http")]
pub mod sse_server;
pub mod structured;
#[cfg(test)]
pub mod test_utils;
pub mod transport;
pub mod validation;

#[cfg(feature = "uri-templates")]
pub mod uri;

#[cfg(feature = "schema-generation")]
pub mod schema;

// Re-export from submodules
// Note: auth and session both define SessionConfig, so we rename one to avoid ambiguous re-exports
#[cfg(feature = "auth")]
pub use turbomcp_auth::{AuthContext, AuthManager, AuthProvider, OAuth2Config};

// DPoP re-exports (when both auth and dpop features enabled)
pub use crate::context::*;
pub use crate::context_factory::{
    ContextCreationStrategy, ContextFactory, ContextFactoryConfig, ContextFactoryProvider,
    CorrelationId, RequestScope,
};
pub use crate::elicitation::*;
pub use crate::elicitation_api::{
    ElicitationBuilder,
    ElicitationData,
    ElicitationExtract,
    ElicitationManager,
    ElicitationResult,
    // Zero ceremony constructors - beautiful title-first API
    // Zero-ceremony builder functions removed - use MCP-compliant types directly
    elicit,
};
pub use crate::helpers::*;
pub use crate::injection::*;
pub use crate::lifespan::*;
pub use crate::progress::*;
pub use crate::registry::*;
pub use crate::router::{ToolRouter, ToolRouterExt};
pub use crate::server::*;
pub use crate::session::*;
pub use crate::simd::*;
#[cfg(feature = "http")]
pub use crate::sse_server::*;
pub use crate::structured::*;
pub use crate::transport::*;
pub use crate::validation::*;
#[cfg(all(feature = "auth", feature = "dpop"))]
pub use turbomcp_dpop::{DpopKeyManager, DpopProofGenerator};

// Re-export inventory for macro use
pub use inventory;

// Re-export macros
pub use turbomcp_macros::{
    completion, elicit, elicitation, mcp_error, mcp_text, ping, prompt, resource, server, template,
    tool, tool_result,
};

/// Convenient prelude for `TurboMCP` applications
///
/// This prelude includes everything needed for most MCP integrations,
/// eliminating the need for complex import chains from multiple crates.
///
/// ## Key Features
/// - **Zero-boilerplate server setup**: Use `#[server]` and `#[tool]` macros
/// - **Rich error handling**: Built-in `From` implementations and extension methods
/// - **Complete type coverage**: All MCP protocol types included
/// - **Ergonomic error conversion**: `.tool_error()`, `.network_error()` methods
///
/// ## Basic Usage
/// ```rust
/// use turbomcp::prelude::*;
///
/// #[derive(Clone)]
/// struct MyServer;
///
/// #[server(name = "my-server", version = "1.0.0")]
/// impl MyServer {
///     #[tool("My tool")]
///     async fn my_tool(&self, ctx: Context) -> McpResult<String> {
///         // Automatic error conversion with context
///         let data = std::fs::read_to_string("file.txt")
///             .tool_error("Failed to read configuration")?;
///         
///         Ok(format!("Data: {}", data))
///     }
/// }
/// ```
pub mod prelude {
    // Re-export procedural macros for zero-boilerplate development
    pub use super::{
        completion, elicit, elicitation, mcp_error, mcp_text, ping, prompt, resource, server,
        template, tool, tool_result,
    };

    // Core types (always available)
    pub use super::{
        CallToolRequest, CallToolResult, Context, ElicitationManager, HandlerMetadata,
        HandlerRegistration, McpError, McpErrorExt, McpResult, McpServer, RequestContext, Server,
        ServerBuilder, ServerError, Transport, TransportConfig, TransportFactory, TurboMcpServer,
        error_text, handlers, prompt_result, resource_result, text, tool_error, tool_success,
    };

    // Auth types (feature-gated)
    #[cfg(feature = "auth")]
    pub use super::{AuthContext, AuthManager, AuthProvider, OAuth2Config};

    // Re-export essential protocol types to avoid manual imports
    pub use turbomcp_protocol::types::{
        Content, GetPromptResult, Prompt, ReadResourceResult, Resource, TextContent, Tool,
    };

    // Re-export commonly needed external types
    pub use async_trait::async_trait;
    pub use serde::{Deserialize, Serialize};

    // ============================================================================
    // Streamable HTTP v2 (MCP 2025-06-18 Compliant) - RECOMMENDED
    // ============================================================================

    /// Streamable HTTP v2 server configuration and runtime
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use turbomcp_transport::streamable_http_v2::{
        StreamableHttpConfig, StreamableHttpConfigBuilder, create_router as create_http_router,
        run_server as run_http_server,
    };

    /// Security configuration types for HTTP transport
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use turbomcp_transport::security::{SecurityConfigBuilder, SecurityValidator};

    /// Streamable HTTP v2 client transport and configuration
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use turbomcp_transport::streamable_http_client::{
        RetryPolicy, StreamableHttpClientConfig, StreamableHttpClientTransport,
    };

    // ============================================================================
    // Ergonomic Type Aliases for Beautiful DX
    // ============================================================================

    /// Ergonomic alias for HTTP server configuration builder
    ///
    /// Instead of:
    /// ```ignore
    /// use turbomcp_transport::streamable_http_v2::StreamableHttpConfigBuilder;
    /// ```
    ///
    /// Use:
    /// ```
    /// use turbomcp::prelude::*;
    /// # #[cfg(feature = "http")]
    /// let config = HttpConfig::new()
    ///     .with_bind_address("0.0.0.0:8080")
    ///     .build();
    /// ```
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use StreamableHttpConfigBuilder as HttpConfig;

    /// Ergonomic alias for HTTP client configuration
    ///
    /// Instead of:
    /// ```ignore
    /// use turbomcp_transport::streamable_http_client::StreamableHttpClientConfig;
    /// ```
    ///
    /// Use:
    /// ```
    /// use turbomcp::prelude::*;
    /// # #[cfg(feature = "http")]
    /// let config = HttpClientConfig {
    ///     base_url: "http://localhost:8080".to_string(),
    ///     ..Default::default()
    /// };
    /// ```
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub use StreamableHttpClientConfig as HttpClientConfig;

    // ============================================================================
    // Other Transport Implementations
    // ============================================================================

    /// WebSocket bidirectional transport
    #[cfg(feature = "websocket")]
    #[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
    pub use turbomcp_transport::websocket_bidirectional::{
        WebSocketBidirectionalConfig, WebSocketBidirectionalTransport,
    };

    /// TCP transport
    #[cfg(feature = "tcp")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tcp")))]
    pub use turbomcp_transport::tcp::TcpTransport;

    /// Unix domain socket transport
    #[cfg(all(unix, feature = "unix"))]
    #[cfg_attr(docsrs, doc(cfg(all(unix, feature = "unix"))))]
    pub use turbomcp_transport::unix::UnixTransport;
}

/// `TurboMCP` result type
pub type McpResult<T> = Result<T, McpError>;

/// `TurboMCP` error type
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// Server error
    #[error("Server error: {0}")]
    Server(#[from] turbomcp_server::ServerError),

    /// Protocol error  
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Tool execution error
    #[error("Tool error: {0}")]
    Tool(String),

    /// Resource access error
    #[error("Resource error: {0}")]
    Resource(String),

    /// Prompt processing error
    #[error("Prompt error: {0}")]
    Prompt(String),

    /// Context error
    #[error("Context error: {0}")]
    Context(String),

    /// Authorization/authentication error
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// Network/connectivity error
    #[error("Network error: {0}")]
    Network(String),

    /// Invalid input error
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Schema generation error
    #[error("Schema error: {0}")]
    Schema(String),

    /// Transport error
    #[error("Transport error: {0}")]
    Transport(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Internal error - for backwards compatibility
    #[error("Internal error: {0}")]
    Internal(String),

    /// Invalid request error
    #[error("Invalid request: {0}")]
    InvalidRequest(String),
}

impl McpError {
    /// Create an internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Create an invalid request error
    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self::InvalidRequest(msg.into())
    }

    /// Create a tool error
    pub fn tool(msg: impl Into<String>) -> Self {
        Self::Tool(msg.into())
    }

    /// Create a protocol error
    pub fn protocol(msg: impl Into<String>) -> Self {
        Self::Protocol(msg.into())
    }

    /// Create a resource error
    pub fn resource(msg: impl Into<String>) -> Self {
        Self::Resource(msg.into())
    }

    /// Create an unauthorized error
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }

    /// Create a network error
    pub fn network(msg: impl Into<String>) -> Self {
        Self::Network(msg.into())
    }

    /// Create an invalid input error
    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }
}

impl From<turbomcp_transport::core::TransportError> for McpError {
    fn from(err: turbomcp_transport::core::TransportError) -> Self {
        Self::Transport(err.to_string())
    }
}

impl From<Box<turbomcp_protocol::Error>> for McpError {
    fn from(core_error: Box<turbomcp_protocol::Error>) -> Self {
        // Convert core error to server error first, then to McpError
        let server_error: turbomcp_server::ServerError = core_error.into();
        Self::Server(server_error)
    }
}

// Add From implementations for common error types to reduce boilerplate
impl From<std::io::Error> for McpError {
    fn from(err: std::io::Error) -> Self {
        Self::Transport(format!("IO error: {err}"))
    }
}

#[cfg(feature = "http")]
impl From<reqwest::Error> for McpError {
    fn from(err: reqwest::Error) -> Self {
        Self::Network(format!("Network error: {err}"))
    }
}

#[cfg(feature = "uri-templates")]
impl From<regex::Error> for McpError {
    fn from(err: regex::Error) -> Self {
        Self::InvalidInput(format!("Regex error: {err}"))
    }
}

impl From<chrono::ParseError> for McpError {
    fn from(err: chrono::ParseError) -> Self {
        Self::InvalidInput(format!("Date parse error: {err}"))
    }
}

impl Clone for McpError {
    fn clone(&self) -> Self {
        match self {
            Self::Server(e) => {
                // Convert the server error to string to avoid any complex cloning issues
                let error_msg = format!("{e}");
                Self::Server(turbomcp_server::ServerError::Internal(error_msg))
            }
            Self::Protocol(s) => Self::Protocol(s.clone()),
            Self::Tool(s) => Self::Tool(s.clone()),
            Self::Resource(s) => Self::Resource(s.clone()),
            Self::Prompt(s) => Self::Prompt(s.clone()),
            Self::Context(s) => Self::Context(s.clone()),
            Self::Unauthorized(s) => Self::Unauthorized(s.clone()),
            Self::Network(s) => Self::Network(s.clone()),
            Self::InvalidInput(s) => Self::InvalidInput(s.clone()),
            Self::Schema(s) => Self::Schema(s.clone()),
            Self::Transport(s) => Self::Transport(s.clone()),
            Self::Internal(s) => Self::Internal(s.clone()),
            Self::InvalidRequest(s) => Self::InvalidRequest(s.clone()),
            Self::Serialization(e) => {
                // Convert the serialization error to string to avoid cloning complexity
                let error_msg = format!("{e}");
                let io_error = std::io::Error::other(error_msg);
                Self::Serialization(serde_json::Error::io(io_error))
            }
        }
    }
}

/// Extension trait for convenient error conversion
pub trait McpErrorExt<T> {
    /// Convert any error to a Tool error with context
    fn tool_error(self, context: impl Into<String>) -> McpResult<T>;

    /// Convert any error to a Protocol error with context
    fn protocol_error(self, context: impl Into<String>) -> McpResult<T>;

    /// Convert any error to a Resource error with context
    fn resource_error(self, context: impl Into<String>) -> McpResult<T>;

    /// Convert any error to a Network error with context
    fn network_error(self, context: impl Into<String>) -> McpResult<T>;

    /// Convert any error to a Transport error with context
    fn transport_error(self, context: impl Into<String>) -> McpResult<T>;
}

impl<T, E: std::fmt::Display> McpErrorExt<T> for Result<T, E> {
    fn tool_error(self, context: impl Into<String>) -> McpResult<T> {
        self.map_err(|e| McpError::Tool(format!("{}: {}", context.into(), e)))
    }

    fn protocol_error(self, context: impl Into<String>) -> McpResult<T> {
        self.map_err(|e| McpError::Protocol(format!("{}: {}", context.into(), e)))
    }

    fn resource_error(self, context: impl Into<String>) -> McpResult<T> {
        self.map_err(|e| McpError::Resource(format!("{}: {}", context.into(), e)))
    }

    fn network_error(self, context: impl Into<String>) -> McpResult<T> {
        self.map_err(|e| McpError::Network(format!("{}: {}", context.into(), e)))
    }

    fn transport_error(self, context: impl Into<String>) -> McpResult<T> {
        self.map_err(|e| McpError::Transport(format!("{}: {}", context.into(), e)))
    }
}

/// TurboMCP server trait for ergonomic server definition
#[async_trait::async_trait]
pub trait TurboMcpServer: Send + Sync + 'static + HandlerRegistration {
    /// Get server name
    fn name(&self) -> &'static str {
        "TurboMCP Server"
    }

    /// Get server version
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Get server description
    fn description(&self) -> Option<&str> {
        None
    }

    /// Server initialization hook
    async fn startup(&self) -> McpResult<()> {
        Ok(())
    }

    /// Server shutdown hook  
    async fn shutdown(&self) -> McpResult<()> {
        Ok(())
    }

    /// Run server with STDIO transport
    /// Note: Disabled due to async trait lifetime constraints
    /*
    async fn run_stdio(self: Arc<Self>) -> McpResult<()> {
        // Initialize server
        self.startup().await?;

        // Build and run MCP server
        let server = self.build_server().await?;
        let result = server.run_stdio().await;

        // Cleanup
        self.shutdown().await?;

        Ok(result?)
    }
    */
    /// Build the underlying MCP server
    async fn build_server(&self) -> McpResult<McpServer> {
        let mut builder = ServerBuilder::new()
            .name(self.name())
            .version(self.version());

        if let Some(desc) = self.description() {
            builder = builder.description(desc);
        }

        // Register handlers
        self.register_with_builder(&mut builder).await?;

        Ok(builder.build())
    }
}

/// Context for `TurboMCP` handlers with dependency injection
#[derive(Clone)]
pub struct Context {
    /// Request context from MCP core
    pub request: RequestContext,
    /// Server-specific data
    pub data: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    /// Handler metadata
    pub handler: HandlerMetadata,
    /// Dependency injection container
    pub container: context::Container,
}

/// Metadata about the current handler
#[derive(Debug, Clone)]
pub struct HandlerMetadata {
    /// Handler name
    pub name: String,
    /// Handler type (tool, prompt, resource)
    pub handler_type: String,
    /// Handler description
    pub description: Option<String>,
}

impl Context {
    /// Create a new context
    #[must_use]
    pub fn new(request: RequestContext, handler: HandlerMetadata) -> Self {
        Self {
            request,
            data: Arc::new(RwLock::new(HashMap::new())),
            handler,
            container: context::Container::new(),
        }
    }

    /// Create a new context with a shared container
    #[must_use]
    pub fn with_container(
        request: RequestContext,
        handler: HandlerMetadata,
        container: context::Container,
    ) -> Self {
        Self {
            request,
            data: Arc::new(RwLock::new(HashMap::new())),
            handler,
            container,
        }
    }

    /// Resolve a service from the dependency injection container
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Context`] if:
    /// - The service is not registered in the container
    /// - Type mismatch occurs during downcast
    /// - Circular dependency is detected
    pub async fn resolve<T: 'static + Clone>(&self, name: &str) -> McpResult<T> {
        self.container.resolve_with_dependencies(name).await
    }

    /// Resolve a service by type name (convenience method)
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Context`] if:
    /// - The service is not registered in the container
    /// - Type mismatch occurs during downcast
    /// - Circular dependency is detected
    pub async fn resolve_by_type<T: 'static + Clone>(&self) -> McpResult<T> {
        let type_name = std::any::type_name::<T>();
        self.resolve(type_name).await
    }

    /// Register a service with the container
    pub async fn register<T: 'static + Send + Sync>(&self, name: &str, service: T) {
        self.container.register(name, service).await;
    }

    /// Register a singleton factory with the container
    pub async fn register_singleton<F, T>(&self, name: &str, factory: F)
    where
        F: Fn() -> T + Send + Sync + 'static,
        T: Send + Sync + Clone + 'static,
    {
        self.container.register_singleton(name, factory).await;
    }

    /// Log an info message to the client
    ///
    /// # Errors
    ///
    /// Currently infallible - returns `Ok(())` in all cases.
    /// Logging failures are handled internally by the tracing infrastructure.
    pub async fn info<S: AsRef<str>>(&self, message: S) -> McpResult<()> {
        tracing::info!("{}", message.as_ref());
        // Logging notification sent via tracing infrastructure
        Ok(())
    }

    /// Log a warning message to the client
    ///
    /// # Errors
    ///
    /// Currently infallible - returns `Ok(())` in all cases.
    /// Logging failures are handled internally by the tracing infrastructure.
    pub async fn warn<S: AsRef<str>>(&self, message: S) -> McpResult<()> {
        tracing::warn!("{}", message.as_ref());
        // Logging notification sent via tracing infrastructure
        Ok(())
    }

    /// Log an error message to the client
    ///
    /// # Errors
    ///
    /// Currently infallible - returns `Ok(())` in all cases.
    /// Logging failures are handled internally by the tracing infrastructure.
    pub async fn error<S: AsRef<str>>(&self, message: S) -> McpResult<()> {
        tracing::error!("{}", message.as_ref());
        // Logging notification sent via tracing infrastructure
        Ok(())
    }

    /// Report progress for long-running operations
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Context`] if progress reporting fails due to
    /// invalid progress values or notification system errors.
    pub async fn report_progress(&self, progress: f64, total: Option<f64>) -> McpResult<()> {
        tracing::debug!("Progress: {} / {:?}", progress, total);

        // Generate or use existing progress token
        let token = crate::progress::ProgressToken::new();

        // Update progress using the global progress manager
        crate::progress::global_progress_manager().update_progress(&token, progress, total)?;

        // Progress notification sent to MCP client via notification system
        // Integrated with the MCP notification protocol

        Ok(())
    }

    /// Store data in context
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Serialization`] if the value cannot be serialized to JSON.
    pub async fn set<T: Serialize>(&self, key: &str, value: T) -> McpResult<()> {
        let json_value = serde_json::to_value(value)?;
        self.data.write().await.insert(key.to_string(), json_value);
        Ok(())
    }

    /// Retrieve data from context
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Serialization`] if the stored value cannot be deserialized to type `T`.
    pub async fn get<T: for<'de> Deserialize<'de>>(&self, key: &str) -> McpResult<Option<T>> {
        if let Some(value) = self.data.read().await.get(key) {
            Ok(Some(serde_json::from_value(value.clone())?))
        } else {
            Ok(None)
        }
    }

    /// Get user ID from the request context
    #[must_use]
    pub fn user_id(&self) -> Option<&str> {
        self.request.user()
    }

    /// Check if request is authenticated
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.request.is_authenticated()
    }

    /// Get user roles from request context
    #[must_use]
    pub fn roles(&self) -> Vec<String> {
        self.request.roles()
    }

    /// Check if user has any of the required roles
    pub fn has_any_role<S: AsRef<str>>(&self, required: &[S]) -> bool {
        self.request.has_any_role(required)
    }

    /// Get session ID from request context
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.request.session_id.as_deref()
    }

    /// Get client ID from request context
    #[must_use]
    pub fn client_id(&self) -> Option<&str> {
        self.request.client_id.as_deref()
    }

    /// Get request ID
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.request.request_id
    }

    /// Get metadata from request context
    #[must_use]
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.request.get_metadata(key)
    }

    /// Check if request is cancelled
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.request.is_cancelled()
    }

    /// Get elapsed time since request started
    #[must_use]
    pub fn elapsed(&self) -> std::time::Duration {
        self.request.elapsed()
    }

    /// Send a sampling request to the client (server-initiated LLM communication)
    ///
    /// This method allows the server to request the client to perform sampling
    /// (LLM inference) on behalf of the server, enabling bidirectional AI communication.
    ///
    /// # Arguments
    ///
    /// * `request` - The sampling request as a JSON value containing CreateMessageRequest
    ///
    /// # Returns
    ///
    /// A Result containing the client's response or an error
    ///
    /// # Errors
    ///
    /// Returns [`McpError::Context`] if:
    /// - Server capabilities are not available in the context
    /// - The sampling request fails
    /// - The client does not support sampling
    ///
    /// # Example
    ///
    /// ```ignore
    /// use turbomcp::{CreateMessageRequest, SamplingMessage, Role, Content, TextContent};
    ///
    /// let request = CreateMessageRequest {
    ///     messages: vec![SamplingMessage {
    ///         role: Role::User,
    ///         content: Content::Text(TextContent {
    ///             text: "Analyze this code".to_string(),
    ///             annotations: None,
    ///             meta: None,
    ///         }),
    ///         metadata: None,
    ///     }],
    ///     max_tokens: 500,
    ///     model_preferences: None,
    ///     system_prompt: None,
    ///     include_context: None,
    ///     temperature: None,
    ///     stop_sequences: None,
    ///     _meta: None,
    /// };
    ///
    /// let response = ctx.create_message(request).await?;
    /// ```
    pub async fn create_message(
        &self,
        request: turbomcp_protocol::types::CreateMessageRequest,
    ) -> McpResult<turbomcp_protocol::types::CreateMessageResult> {
        if let Some(capabilities) = self.request.server_to_client() {
            capabilities
                .create_message(request, self.request.clone())
                .await
                .map_err(|e| McpError::Context(format!("Sampling failed: {}", e)))
        } else {
            Err(McpError::Context(
                "Server capabilities not available for sampling".to_string(),
            ))
        }
    }
}

/// Helper trait for handler registration
#[async_trait::async_trait]
pub trait HandlerRegistration {
    /// Register with a server builder
    async fn register_with_builder(&self, builder: &mut ServerBuilder) -> McpResult<()>;
}

/// Handler type enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerType {
    /// Tool handler
    Tool,
    /// Prompt handler  
    Prompt,
    /// Resource handler
    Resource,
}

/// Handler registration information
#[derive(Debug, Clone)]
pub struct HandlerInfo {
    /// Handler name
    pub name: String,
    /// Handler type
    pub handler_type: HandlerType,
    /// Handler description
    pub description: Option<String>,
    /// Handler schema
    pub schema: Option<serde_json::Value>,
}

// The default server implementation will be generated by the #[server] macro

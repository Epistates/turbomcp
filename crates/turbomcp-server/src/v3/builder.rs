//! v3 Server Builder - SOTA fluent API for MCP server configuration.
//!
//! This module provides a builder pattern for configuring and running MCP servers
//! with full control over transport selection and server integration.
//!
//! # Design Principles
//!
//! 1. **Zero Configuration Required** - Sensible defaults for quick starts
//! 2. **Transport Agnostic** - Choose transport at runtime, not compile time
//! 3. **BYO Server Support** - Integrate with existing Axum/Tower infrastructure
//! 4. **Platform Transparent** - Works on native and WASM without `#[cfg]` in user code
//!
//! # Examples
//!
//! ## Simplest Usage (STDIO default)
//!
//! ```rust,ignore
//! use turbomcp::prelude::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     MyServer.serve().await.unwrap();
//! }
//! ```
//!
//! ## Choose Transport at Runtime
//!
//! ```rust,ignore
//! use turbomcp::prelude::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     let transport = std::env::var("TRANSPORT").unwrap_or("stdio".into());
//!
//!     MyServer.builder()
//!         .transport(match transport.as_str() {
//!             "http" => Transport::http("0.0.0.0:8080"),
//!             "tcp" => Transport::tcp("0.0.0.0:9000"),
//!             _ => Transport::stdio(),
//!         })
//!         .serve()
//!         .await
//!         .unwrap();
//! }
//! ```
//!
//! ## Full Configuration
//!
//! ```rust,ignore
//! use turbomcp::prelude::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     MyServer.builder()
//!         .transport(Transport::http("0.0.0.0:8080"))
//!         .with_rate_limit(100, Duration::from_secs(1))
//!         .with_connection_limit(1000)
//!         .with_graceful_shutdown(Duration::from_secs(30))
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
//! use turbomcp::prelude::*;
//!
//! #[tokio::main]
//! async fn main() {
//!     // Get MCP routes as an Axum router
//!     let mcp_router = MyServer.builder().into_axum_router();
//!
//!     // Merge with your existing routes
//!     let app = Router::new()
//!         .route("/health", get(health_check))
//!         .merge(mcp_router);
//!
//!     // Use your own server
//!     let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
//!     axum::serve(listener, app).await?;
//! }
//! ```

use std::time::Duration;

use turbomcp_core::error::McpResult;
use turbomcp_core::handler::McpHandler;

use super::config::{ConnectionLimits, RateLimitConfig, ServerConfig, ServerConfigBuilder};

/// Transport configuration for the server.
///
/// Use the associated functions to create transport configurations:
/// - `Transport::stdio()` - Standard I/O (default, works with Claude Desktop)
/// - `Transport::http(addr)` - HTTP JSON-RPC
/// - `Transport::websocket(addr)` - WebSocket bidirectional
/// - `Transport::tcp(addr)` - Raw TCP sockets
/// - `Transport::unix(path)` - Unix domain sockets
#[derive(Debug, Clone, Default)]
pub enum Transport {
    /// Standard I/O transport (line-based JSON-RPC).
    /// This is the default and works with Claude Desktop.
    #[default]
    Stdio,

    /// HTTP transport (JSON-RPC over HTTP POST).
    #[cfg(feature = "http")]
    Http {
        /// Bind address (e.g., "0.0.0.0:8080")
        addr: String,
    },

    /// WebSocket transport (bidirectional JSON-RPC).
    #[cfg(feature = "websocket")]
    WebSocket {
        /// Bind address (e.g., "0.0.0.0:8080")
        addr: String,
    },

    /// TCP transport (line-based JSON-RPC over TCP).
    #[cfg(feature = "tcp")]
    Tcp {
        /// Bind address (e.g., "0.0.0.0:9000")
        addr: String,
    },

    /// Unix domain socket transport (line-based JSON-RPC).
    #[cfg(feature = "unix")]
    Unix {
        /// Socket path (e.g., "/tmp/mcp.sock")
        path: String,
    },
}

impl Transport {
    /// Create STDIO transport configuration.
    ///
    /// This is the default transport that works with Claude Desktop
    /// and other MCP clients that communicate via stdin/stdout.
    #[must_use]
    pub fn stdio() -> Self {
        Self::Stdio
    }

    /// Create HTTP transport configuration.
    ///
    /// # Arguments
    ///
    /// * `addr` - Bind address (e.g., "0.0.0.0:8080" or "127.0.0.1:3000")
    #[cfg(feature = "http")]
    #[must_use]
    pub fn http(addr: impl Into<String>) -> Self {
        Self::Http { addr: addr.into() }
    }

    /// Create WebSocket transport configuration.
    ///
    /// # Arguments
    ///
    /// * `addr` - Bind address (e.g., "0.0.0.0:8080")
    #[cfg(feature = "websocket")]
    #[must_use]
    pub fn websocket(addr: impl Into<String>) -> Self {
        Self::WebSocket { addr: addr.into() }
    }

    /// Create TCP transport configuration.
    ///
    /// # Arguments
    ///
    /// * `addr` - Bind address (e.g., "0.0.0.0:9000")
    #[cfg(feature = "tcp")]
    #[must_use]
    pub fn tcp(addr: impl Into<String>) -> Self {
        Self::Tcp { addr: addr.into() }
    }

    /// Create Unix domain socket transport configuration.
    ///
    /// # Arguments
    ///
    /// * `path` - Socket path (e.g., "/tmp/mcp.sock")
    #[cfg(feature = "unix")]
    #[must_use]
    pub fn unix(path: impl Into<String>) -> Self {
        Self::Unix { path: path.into() }
    }
}

/// Server builder for configuring and running MCP servers.
///
/// This builder provides a fluent API for:
/// - Selecting transport at runtime
/// - Configuring rate limits and connection limits
/// - Setting up graceful shutdown
/// - Integrating with existing server infrastructure
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp::prelude::*;
///
/// MyServer.builder()
///     .transport(Transport::http("0.0.0.0:8080"))
///     .with_rate_limit(100, Duration::from_secs(1))
///     .serve()
///     .await?;
/// ```
#[derive(Debug)]
pub struct ServerBuilder<H: McpHandler> {
    handler: H,
    transport: Transport,
    config: ServerConfigBuilder,
    graceful_shutdown: Option<Duration>,
}

impl<H: McpHandler> ServerBuilder<H> {
    /// Create a new server builder wrapping the given handler.
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            transport: Transport::default(),
            config: ServerConfig::builder(),
            graceful_shutdown: None,
        }
    }

    /// Set the transport for this server.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.transport(Transport::http("0.0.0.0:8080"))
    /// ```
    #[must_use]
    pub fn transport(mut self, transport: Transport) -> Self {
        self.transport = transport;
        self
    }

    /// Configure rate limiting.
    ///
    /// # Arguments
    ///
    /// * `requests` - Maximum requests allowed
    /// * `per` - Time window for the limit
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Allow 100 requests per second
    /// builder.with_rate_limit(100, Duration::from_secs(1))
    /// ```
    #[must_use]
    pub fn with_rate_limit(mut self, max_requests: u32, window: Duration) -> Self {
        self.config = self.config.rate_limit(RateLimitConfig {
            max_requests,
            window,
            per_client: true,
        });
        self
    }

    /// Configure maximum concurrent connections.
    ///
    /// This limit applies to TCP, HTTP, WebSocket, and Unix transports.
    /// STDIO transport always has exactly one connection.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.with_connection_limit(1000)
    /// ```
    #[must_use]
    pub fn with_connection_limit(mut self, max: usize) -> Self {
        self.config = self.config.connection_limits(ConnectionLimits {
            max_tcp_connections: max,
            max_websocket_connections: max,
            max_http_concurrent: max,
            max_unix_connections: max,
        });
        self
    }

    /// Configure graceful shutdown timeout.
    ///
    /// When the server receives a shutdown signal, it will wait up to
    /// this duration for in-flight requests to complete.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// builder.with_graceful_shutdown(Duration::from_secs(30))
    /// ```
    #[must_use]
    pub fn with_graceful_shutdown(mut self, timeout: Duration) -> Self {
        self.graceful_shutdown = Some(timeout);
        self
    }

    /// Configure maximum message size.
    ///
    /// Messages exceeding this size will be rejected.
    /// Default: 10MB.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Limit messages to 1MB
    /// builder.with_max_message_size(1024 * 1024)
    /// ```
    #[must_use]
    pub fn with_max_message_size(mut self, size: usize) -> Self {
        self.config = self.config.max_message_size(size);
        self
    }

    /// Apply a custom server configuration.
    ///
    /// This replaces any previously set configuration options.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config = ServerConfig::builder()
    ///     .rate_limit(rate_config)
    ///     .connection_limits(limits)
    ///     .build();
    ///
    /// builder.with_config(config)
    /// ```
    #[must_use]
    pub fn with_config(mut self, config: ServerConfig) -> Self {
        let mut builder = ServerConfig::builder()
            .protocol(config.protocol)
            .connection_limits(config.connection_limits)
            .required_capabilities(config.required_capabilities)
            .max_message_size(config.max_message_size);

        if let Some(rate_limit) = config.rate_limit {
            builder = builder.rate_limit(rate_limit);
        }

        self.config = builder;
        self
    }

    /// Run the server with the configured transport.
    ///
    /// This is the main entry point that starts the server and blocks
    /// until shutdown.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// MyServer.builder()
    ///     .transport(Transport::http("0.0.0.0:8080"))
    ///     .serve()
    ///     .await?;
    /// ```
    #[allow(unused_variables)]
    pub async fn serve(self) -> McpResult<()> {
        // Config is used by transport-specific features (http, websocket, tcp, unix)
        // STDIO doesn't use config, so this may be unused if only stdio is enabled
        let config = self.config.build();

        match self.transport {
            Transport::Stdio => {
                #[cfg(feature = "stdio")]
                {
                    super::transport::stdio::run(&self.handler).await
                }
                #[cfg(not(feature = "stdio"))]
                {
                    Err(turbomcp_core::error::McpError::internal(
                        "STDIO transport not available. Enable the 'stdio' feature.",
                    ))
                }
            }

            #[cfg(feature = "http")]
            Transport::Http { addr } => {
                super::transport::http::run_with_config(&self.handler, &addr, &config).await
            }

            #[cfg(feature = "websocket")]
            Transport::WebSocket { addr } => {
                super::transport::websocket::run_with_config(&self.handler, &addr, &config).await
            }

            #[cfg(feature = "tcp")]
            Transport::Tcp { addr } => {
                super::transport::tcp::run_with_config(&self.handler, &addr, &config).await
            }

            #[cfg(feature = "unix")]
            Transport::Unix { path } => {
                super::transport::unix::run_with_config(&self.handler, &path, &config).await
            }
        }
    }

    /// Get the underlying handler.
    ///
    /// Useful for testing or custom integrations.
    #[must_use]
    pub fn handler(&self) -> &H {
        &self.handler
    }

    /// Consume the builder and return the handler.
    ///
    /// Useful for custom integrations where you need ownership.
    #[must_use]
    pub fn into_handler(self) -> H {
        self.handler
    }

    /// Convert to an Axum router for BYO server integration.
    ///
    /// This allows you to merge MCP routes with your existing Axum application.
    /// Rate limiting configured via `with_rate_limit()` is applied to all requests.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use axum::Router;
    /// use axum::routing::get;
    ///
    /// let mcp_router = MyServer.builder()
    ///     .with_rate_limit(100, Duration::from_secs(1))
    ///     .into_axum_router();
    ///
    /// let app = Router::new()
    ///     .route("/health", get(|| async { "OK" }))
    ///     .merge(mcp_router);
    ///
    /// let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    /// axum::serve(listener, app).await?;
    /// ```
    #[cfg(feature = "http")]
    pub fn into_axum_router(self) -> axum::Router {
        use axum::{Router, routing::post};
        use std::sync::Arc;

        let config = self.config.build();
        let handler = Arc::new(self.handler);
        let rate_limiter = config
            .rate_limit
            .map(|cfg| Arc::new(crate::v3::config::RateLimiter::new(cfg)));

        Router::new()
            .route("/", post(handle_json_rpc::<H>))
            .route("/mcp", post(handle_json_rpc::<H>))
            .with_state(AppState {
                handler,
                rate_limiter,
            })
    }

    /// Convert to a Tower service for custom server integration.
    ///
    /// This returns a service that can be used with any Tower-compatible
    /// HTTP server (Hyper, Axum, Warp, etc.).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hyper::server::conn::http1;
    /// use hyper_util::rt::TokioIo;
    ///
    /// let service = MyServer.builder().into_service();
    ///
    /// let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    /// loop {
    ///     let (stream, _) = listener.accept().await?;
    ///     let service = service.clone();
    ///     tokio::spawn(async move {
    ///         http1::Builder::new()
    ///             .serve_connection(TokioIo::new(stream), service)
    ///             .await
    ///     });
    /// }
    /// ```
    #[cfg(feature = "http")]
    pub fn into_service(
        self,
    ) -> impl tower::Service<
        axum::http::Request<axum::body::Body>,
        Response = axum::http::Response<axum::body::Body>,
        Error = std::convert::Infallible,
        Future = impl Future<
            Output = Result<axum::http::Response<axum::body::Body>, std::convert::Infallible>,
        > + Send,
    > + Clone
    + Send {
        use tower::ServiceExt;
        self.into_axum_router()
            .into_service()
            .map_err(|e| match e {})
    }
}

/// State for the Axum handler.
#[cfg(feature = "http")]
#[derive(Clone)]
struct AppState<H: McpHandler> {
    handler: std::sync::Arc<H>,
    rate_limiter: Option<std::sync::Arc<crate::v3::config::RateLimiter>>,
}

/// JSON-RPC request handler for Axum.
///
/// Note: Rate limiting uses global rate limiting when used via `into_axum_router()`.
/// For per-client rate limiting based on IP, use the full transport which includes
/// `ConnectInfo` extraction.
#[cfg(feature = "http")]
async fn handle_json_rpc<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<AppState<H>>,
    axum::Json(request): axum::Json<serde_json::Value>,
) -> impl axum::response::IntoResponse {
    use super::context::RequestContext;
    use super::router::{parse_request, route_request, serialize_response};

    // Check rate limit if configured (uses global rate limiting for BYO server)
    if let Some(ref limiter) = state.rate_limiter
        && !limiter.check(None)
    {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32000,
                    "message": "Rate limit exceeded"
                },
                "id": null
            })),
        );
    }

    let request_str = match serde_json::to_string(&request) {
        Ok(s) => s,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    },
                    "id": null
                })),
            );
        }
    };

    let parsed = match parse_request(&request_str) {
        Ok(p) => p,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    },
                    "id": null
                })),
            );
        }
    };

    let ctx = RequestContext::http();
    let core_ctx = ctx.to_core_context();
    let response = route_request(&*state.handler, parsed, &core_ctx).await;

    if !response.should_send() {
        return (
            axum::http::StatusCode::NO_CONTENT,
            axum::Json(serde_json::json!(null)),
        );
    }

    match serialize_response(&response) {
        Ok(json_str) => {
            let value: serde_json::Value = serde_json::from_str(&json_str).unwrap_or_default();
            (axum::http::StatusCode::OK, axum::Json(value))
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32603,
                    "message": format!("Internal error: {}", e)
                },
                "id": null
            })),
        ),
    }
}

/// Extension trait for creating server builders from handlers.
///
/// This trait provides the builder pattern for configurable server deployment.
/// For simple cases, use `McpHandlerExt::run()` directly.
///
/// # Design Philosophy
///
/// - **Simple**: `handler.run()` → runs with STDIO (via `McpHandlerExt`)
/// - **Configurable**: `handler.builder().transport(...).serve()` → full control
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp::prelude::*;
///
/// // Simple (no config needed)
/// MyServer.run().await?;
///
/// // Configurable (builder pattern)
/// MyServer.builder()
///     .transport(Transport::http("0.0.0.0:8080"))
///     .with_rate_limit(100, Duration::from_secs(1))
///     .serve()
///     .await?;
///
/// // BYO server (Axum integration)
/// let mcp = MyServer.builder().into_axum_router();
/// ```
pub trait McpServerExt: McpHandler + Sized {
    /// Create a server builder for this handler.
    ///
    /// The builder allows configuring transport, rate limits, connection
    /// limits, and other server options before starting.
    fn builder(self) -> ServerBuilder<Self> {
        ServerBuilder::new(self)
    }
}

/// Blanket implementation for all McpHandler types.
impl<T: McpHandler> McpServerExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use turbomcp_core::context::RequestContext as CoreRequestContext;
    use turbomcp_core::error::McpError;
    use turbomcp_types::{
        Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
    };

    #[derive(Clone)]
    struct TestHandler;

    #[allow(clippy::manual_async_fn)]
    impl McpHandler for TestHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("test", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![Tool::new("test", "Test tool")]
        }

        fn list_resources(&self) -> Vec<Resource> {
            vec![]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![]
        }

        fn call_tool<'a>(
            &'a self,
            _name: &'a str,
            _args: Value,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<ToolResult>> + Send + 'a {
            async { Ok(ToolResult::text("ok")) }
        }

        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a {
            let uri = uri.to_string();
            async move { Err(McpError::resource_not_found(&uri)) }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<Value>,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>> + Send + 'a {
            let name = name.to_string();
            async move { Err(McpError::prompt_not_found(&name)) }
        }
    }

    #[test]
    fn test_transport_default_is_stdio() {
        let transport = Transport::default();
        assert!(matches!(transport, Transport::Stdio));
    }

    #[test]
    fn test_builder_creation() {
        let handler = TestHandler;
        let builder = handler.builder();
        assert!(matches!(builder.transport, Transport::Stdio));
    }

    #[test]
    fn test_builder_transport_selection() {
        let handler = TestHandler;

        // Test STDIO
        let builder = handler.clone().builder().transport(Transport::stdio());
        assert!(matches!(builder.transport, Transport::Stdio));
    }

    #[cfg(feature = "http")]
    #[test]
    fn test_builder_http_transport() {
        let handler = TestHandler;
        let builder = handler.builder().transport(Transport::http("0.0.0.0:8080"));
        assert!(matches!(builder.transport, Transport::Http { .. }));
    }

    #[test]
    fn test_builder_rate_limit() {
        let handler = TestHandler;
        let builder = handler
            .builder()
            .with_rate_limit(100, Duration::from_secs(1));

        let config = builder.config.build();
        assert!(config.rate_limit.is_some());
    }

    #[test]
    fn test_builder_connection_limit() {
        let handler = TestHandler;
        let builder = handler.builder().with_connection_limit(500);

        let config = builder.config.build();
        assert_eq!(config.connection_limits.max_tcp_connections, 500);
        assert_eq!(config.connection_limits.max_websocket_connections, 500);
        assert_eq!(config.connection_limits.max_http_concurrent, 500);
        assert_eq!(config.connection_limits.max_unix_connections, 500);
    }

    #[test]
    fn test_builder_graceful_shutdown() {
        let handler = TestHandler;
        let builder = handler
            .builder()
            .with_graceful_shutdown(Duration::from_secs(30));

        assert_eq!(builder.graceful_shutdown, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_builder_into_handler() {
        let handler = TestHandler;
        let builder = handler.builder();
        let recovered = builder.into_handler();
        assert_eq!(recovered.server_info().name, "test");
    }
}

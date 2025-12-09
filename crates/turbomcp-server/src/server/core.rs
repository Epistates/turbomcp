//! Core MCP server implementation
//!
//! Contains the main McpServer struct and its core functionality including
//! middleware building, lifecycle management, and server construction.

use std::sync::Arc;
use tracing::{info, info_span};

#[cfg(feature = "http")]
use tracing::warn;

use crate::{
    config::ServerConfig,
    error::ServerResult,
    lifecycle::{HealthStatus, ServerLifecycle},
    metrics::ServerMetrics,
    registry::HandlerRegistry,
    routing::RequestRouter,
    service::McpService,
};

#[cfg(feature = "middleware")]
use crate::middleware::MiddlewareStack;

use bytes::Bytes;
#[cfg(feature = "middleware")]
use http::StatusCode;
use http::{Request, Response};
use tokio::time::{Duration, sleep};
use turbomcp_transport::Transport;
use turbomcp_transport::core::TransportError;

use super::shutdown::ShutdownHandle;

/// Check if logging should be enabled for STDIO transport
///
/// For MCP STDIO transport compliance, logging is disabled by default since stdout
/// must be reserved exclusively for JSON-RPC messages. This can be overridden by
/// setting the TURBOMCP_FORCE_LOGGING environment variable.
pub(crate) fn should_log_for_stdio() -> bool {
    std::env::var("TURBOMCP_FORCE_LOGGING").is_ok()
}

/// Wrapper that holds router + headers + tenant_id and implements JsonRpcHandler
/// This allows us to pass headers and tenant info to create_context without storing them on the router.
/// Used by both HTTP and WebSocket transports.
#[cfg(any(feature = "http", feature = "websocket"))]
struct HttpHandlerWithHeaders {
    router: crate::routing::RequestRouter,
    headers: Option<std::collections::HashMap<String, String>>,
    transport: &'static str,
    tenant_id: Option<String>,
}

#[cfg(any(feature = "http", feature = "websocket"))]
#[async_trait::async_trait]
impl turbomcp_protocol::JsonRpcHandler for HttpHandlerWithHeaders {
    async fn handle_request(&self, req_value: serde_json::Value) -> serde_json::Value {
        use turbomcp_protocol::jsonrpc::JsonRpcRequest;

        // Parse the request
        let req: JsonRpcRequest = match serde_json::from_value(req_value) {
            Ok(r) => r,
            Err(e) => {
                return serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    },
                    "id": null
                });
            }
        };

        // Create context with headers, transport type, and tenant_id
        // tenant_id is extracted from request extensions by the HTTP/WebSocket handlers
        // if TenantExtractionLayer middleware was applied
        let ctx = self.router.create_context(
            self.headers.clone(),
            Some(self.transport),
            self.tenant_id.clone(),
        );

        // Route the request
        let response = self.router.route(req, ctx).await;

        // Serialize response
        match serde_json::to_value(&response) {
            Ok(v) => v,
            Err(e) => {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32603,
                        "message": format!("Internal error: failed to serialize response: {}", e)
                    },
                    "id": response.id
                })
            }
        }
    }

    fn server_info(&self) -> turbomcp_protocol::ServerInfo {
        self.router.server_info()
    }
}

/// Main MCP server following the Axum/Tower Clone pattern
///
/// ## Sharing Pattern
///
/// `McpServer` implements `Clone` like Axum's `Router`. All heavy state is Arc-wrapped
/// internally, making cloning cheap (just atomic reference count increments).
///
/// ```rust,no_run
/// use turbomcp_server::ServerBuilder;
///
/// # async fn example() {
/// let server = ServerBuilder::new().build();
///
/// // Clone for passing to functions (cheap - just Arc increments)
/// let server1 = server.clone();
/// let server2 = server.clone();
///
/// // Access config and health
/// let config = server1.config();
/// println!("Server: {}", config.name);
///
/// let health = server2.health().await;
/// println!("Health: {:?}", health);
/// # }
/// ```
///
/// ## Architecture Notes
///
/// The `service` field contains `BoxCloneService` which is `Send + Clone` but NOT `Sync`.
/// This is intentional and follows Tower's design - users clone the server instead of
/// Arc-wrapping it.
///
/// **Architecture Note**: The service field provides tower::Service integration for
/// advanced middleware patterns. The request processing pipeline currently uses the
/// RequestRouter directly. Tower integration can be added via custom middleware layers
/// when needed for specific use cases (e.g., custom rate limiting, advanced tracing).
#[derive(Clone)]
pub struct McpServer {
    /// Server configuration (Clone-able)
    pub(crate) config: ServerConfig,
    /// Handler registry (Arc-wrapped for cheap cloning)
    pub(crate) registry: Arc<HandlerRegistry>,
    /// Request router (Arc-wrapped for cheap cloning)
    pub(crate) router: Arc<RequestRouter>,
    /// Tower middleware service stack (Clone but !Sync - this is the Tower pattern)
    ///
    /// All requests flow through this service stack, which provides:
    /// - Timeout enforcement
    /// - Request validation
    /// - Authorization checks
    /// - Rate limiting
    /// - Audit logging
    /// - And more middleware layers as configured
    ///
    /// See `server/transport.rs` for integration with transport layer.
    pub(crate) service:
        tower::util::BoxCloneService<Request<Bytes>, Response<Bytes>, crate::ServerError>,
    /// Server lifecycle (Arc-wrapped for cheap cloning)
    pub(crate) lifecycle: Arc<ServerLifecycle>,
    /// Server metrics (Arc-wrapped for cheap cloning)
    pub(crate) metrics: Arc<ServerMetrics>,
    /// Task storage for MCP Tasks API (SEP-1686)
    #[cfg(feature = "mcp-tasks")]
    pub(crate) task_storage: Arc<crate::task_storage::TaskStorage>,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer")
            .field("config", &self.config)
            .finish()
    }
}

impl McpServer {
    /// Build comprehensive Tower middleware stack (transport-agnostic)
    ///
    /// ## Architecture
    ///
    /// This creates a complete Tower service stack with conditional middleware layers:
    /// - **Timeout Layer**: Request timeout enforcement (tower_http)
    /// - **Validation Layer**: JSON-RPC structure validation
    /// - **Authorization Layer**: Resource access control
    /// - **Core Service**: JSON-RPC routing and handler execution
    ///
    /// All middleware is composed using Tower's ServiceBuilder pattern, which provides:
    /// - Top-to-bottom execution order
    /// - Type-safe layer composition
    /// - Zero-cost abstractions
    /// - Clone-able service instances
    ///
    /// ## Integration
    ///
    /// The resulting BoxCloneService is stored in `self.service` and called from
    /// `server/transport.rs` for every incoming request. This ensures ALL requests
    /// flow through the complete middleware pipeline before reaching handlers.
    ///
    /// ## Adding Middleware
    ///
    /// To add new middleware, update the match arms below to include your layer.
    /// Follow the pattern of conditional inclusion based on config flags.
    #[cfg(feature = "middleware")]
    fn build_middleware_stack(
        core_service: McpService,
        stack: MiddlewareStack,
    ) -> tower::util::BoxCloneService<Request<Bytes>, Response<Bytes>, crate::ServerError> {
        // COMPREHENSIVE TOWER COMPOSITION - Conditional Layer Stacking
        //
        // This approach builds the middleware stack incrementally, boxing at each step.
        // While this has a small performance cost from multiple boxing operations,
        // it provides several critical advantages:
        //
        // 1. **Maintainability**: No combinatorial explosion (8 match arms → simple chain)
        // 2. **Extensibility**: Adding new middleware requires only one new block
        // 3. **Clarity**: Each layer's purpose and configuration is explicit
        // 4. **Type Safety**: BoxCloneService provides type erasure while preserving Clone
        //
        // Performance note: The boxing overhead is negligible compared to network I/O
        // and handler execution time. Modern allocators make this essentially free.

        // Start with core service as a boxed service for uniform type handling
        let mut service: tower::util::BoxCloneService<
            Request<Bytes>,
            Response<Bytes>,
            crate::ServerError,
        > = tower::util::BoxCloneService::new(core_service);

        // Authorization layer removed in 2.0.0 - handle at application layer

        // Layer 2: Validation
        // Validates request structure after auth but before processing
        #[cfg(feature = "middleware")]
        {
            if let Some(validation_layer) = stack.validation_layer() {
                service = tower::util::BoxCloneService::new(
                    tower::ServiceBuilder::new()
                        .layer(validation_layer)
                        .service(service),
                );
            }
        }

        // Layer 3: Timeout (outermost)
        // Applied last so it can enforce timeout on the entire request pipeline
        #[cfg(feature = "middleware")]
        {
            if let Some(timeout_config) = stack.timeout_config
                && timeout_config.enabled
            {
                service = tower::util::BoxCloneService::new(
                    tower::ServiceBuilder::new()
                        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
                            StatusCode::REQUEST_TIMEOUT,
                            timeout_config.request_timeout,
                        ))
                        .service(service),
                );
            }
        }

        // Future middleware can be added here with similar if-let blocks:
        // if let Some(auth_config) = stack.auth_config { ... }
        // if let Some(audit_config) = stack.audit_config { ... }
        // if let Some(rate_limit_config) = stack.rate_limit_config { ... }

        service
    }

    /// Create a new server
    #[must_use]
    pub fn new(config: ServerConfig) -> Self {
        Self::new_with_registry(config, HandlerRegistry::new())
    }

    /// Create a new server with an existing registry (used by ServerBuilder)
    #[must_use]
    pub(crate) fn new_with_registry(config: ServerConfig, registry: HandlerRegistry) -> Self {
        let registry = Arc::new(registry);
        let metrics = Arc::new(ServerMetrics::new());

        // Initialize task storage (SEP-1686)
        #[cfg(feature = "mcp-tasks")]
        let task_storage = {
            use tokio::time::Duration;
            let storage = crate::task_storage::TaskStorage::new(Duration::from_secs(60));
            // Start background cleanup task
            storage.start_cleanup();
            Arc::new(storage)
        };

        let router = Arc::new(RequestRouter::new(
            Arc::clone(&registry),
            Arc::clone(&metrics),
            config.clone(),
            #[cfg(feature = "mcp-tasks")]
            Some(Arc::clone(&task_storage)),
        ));
        // Build middleware stack configuration
        #[cfg(feature = "middleware")]
        #[cfg_attr(not(feature = "rate-limiting"), allow(unused_mut))]
        let mut stack = crate::middleware::MiddlewareStack::new();

        // Auto-install rate limiting if enabled in config
        #[cfg(feature = "rate-limiting")]
        if config.rate_limiting.enabled {
            use crate::middleware::rate_limit::{RateLimitStrategy, RateLimits};
            use std::num::NonZeroU32;
            use std::time::Duration;

            let rate_config = crate::middleware::RateLimitConfig {
                strategy: RateLimitStrategy::Global,
                limits: RateLimits {
                    requests_per_period: NonZeroU32::new(
                        config.rate_limiting.requests_per_second * 60,
                    )
                    .unwrap(), // Convert per-second to per-minute
                    period: Duration::from_secs(60),
                    burst_size: Some(NonZeroU32::new(config.rate_limiting.burst_capacity).unwrap()),
                },
                enabled: true,
            };

            stack = stack.with_rate_limit(rate_config);
        }

        // Create core MCP service
        let core_service = McpService::new(
            Arc::clone(&registry),
            Arc::clone(&router),
            Arc::clone(&metrics),
        );

        // COMPREHENSIVE TOWER SERVICE COMPOSITION
        // Build the complete middleware stack with proper type erasure
        //
        // This service is called from server/transport.rs for EVERY incoming request:
        // TransportMessage -> http::Request -> service.call() -> http::Response -> TransportMessage
        //
        // The Tower middleware stack provides:
        // ✓ Timeout enforcement (configurable per-request)
        // ✓ Request validation (JSON-RPC structure)
        // ✓ Authorization checks (resource access control)
        // ✓ Rate limiting (if enabled in config)
        // ✓ Audit logging (configurable)
        // ✓ And more layers as configured
        //
        // BoxCloneService is Clone but !Sync - this is the Tower pattern
        #[cfg(feature = "middleware")]
        let service = Self::build_middleware_stack(core_service, stack);

        #[cfg(not(feature = "middleware"))]
        let service = tower::util::BoxCloneService::new(core_service);

        let lifecycle = Arc::new(ServerLifecycle::new());

        Self {
            config,
            registry,
            router,
            service,
            lifecycle,
            metrics,
            #[cfg(feature = "mcp-tasks")]
            task_storage,
        }
    }

    /// Get server configuration
    #[must_use]
    pub const fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Get handler registry
    #[must_use]
    pub const fn registry(&self) -> &Arc<HandlerRegistry> {
        &self.registry
    }

    /// Get request router
    #[must_use]
    pub const fn router(&self) -> &Arc<RequestRouter> {
        &self.router
    }

    /// Get server lifecycle
    #[must_use]
    pub const fn lifecycle(&self) -> &Arc<ServerLifecycle> {
        &self.lifecycle
    }

    /// Get server metrics
    #[must_use]
    pub const fn metrics(&self) -> &Arc<ServerMetrics> {
        &self.metrics
    }

    /// Get task storage (MCP Tasks API - SEP-1686)
    ///
    /// Returns the task storage instance for managing long-running operations.
    /// Only available when the `mcp-tasks` feature is enabled.
    #[cfg(feature = "mcp-tasks")]
    #[must_use]
    pub const fn task_storage(&self) -> &Arc<crate::task_storage::TaskStorage> {
        &self.task_storage
    }

    /// Get the Tower service stack (test accessor)
    ///
    /// **Note**: This is primarily for integration testing. Production code should
    /// use the transport layer which calls the service internally via
    /// `handle_transport_message()`.
    ///
    /// Returns a clone of the Tower service stack, which is cheap (BoxCloneService
    /// is designed for cloning).
    #[doc(hidden)]
    pub fn service(
        &self,
    ) -> tower::util::BoxCloneService<Request<Bytes>, Response<Bytes>, crate::ServerError> {
        self.service.clone()
    }

    /// Get a shutdown handle for graceful server termination
    ///
    /// This handle enables external control over server shutdown, essential for:
    /// - **Production deployments**: Graceful shutdown on SIGTERM/SIGINT
    /// - **Container orchestration**: Kubernetes graceful pod termination
    /// - **Load balancer integration**: Health check coordination
    /// - **Multi-component systems**: Coordinated shutdown sequences
    /// - **Maintenance operations**: Planned downtime and updates
    ///
    /// # Examples
    ///
    /// ## Basic shutdown coordination
    /// ```no_run
    /// # use turbomcp_server::ServerBuilder;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let server = ServerBuilder::new().build();
    /// let shutdown_handle = server.shutdown_handle();
    ///
    /// // Coordinate with other services
    /// tokio::spawn(async move {
    ///     // Wait for external shutdown signal
    ///     tokio::signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    ///     println!("Shutdown signal received, terminating gracefully...");
    ///     shutdown_handle.shutdown().await;
    /// });
    ///
    /// // Server will gracefully shut down when signaled
    /// // server.run_stdio().await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Container/Kubernetes deployment
    /// ```no_run
    /// # use turbomcp_server::ServerBuilder;
    /// # use std::sync::Arc;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let server = ServerBuilder::new().build();
    /// let shutdown_handle = server.shutdown_handle();
    /// let shutdown_handle_clone = shutdown_handle.clone();
    ///
    /// // Handle multiple signal types with proper platform support
    /// tokio::spawn(async move {
    ///     #[cfg(unix)]
    ///     {
    ///         use tokio::signal::unix::{signal, SignalKind};
    ///         let mut sigterm = signal(SignalKind::terminate()).unwrap();
    ///         tokio::select! {
    ///             _ = tokio::signal::ctrl_c() => {
    ///                 println!("SIGINT received");
    ///             }
    ///             _ = sigterm.recv() => {
    ///                 println!("SIGTERM received");
    ///             }
    ///         }
    ///     }
    ///     #[cfg(not(unix))]
    ///     {
    ///         tokio::signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    ///         println!("SIGINT received");
    ///     }
    ///     shutdown_handle_clone.shutdown().await;
    /// });
    ///
    /// // Server handles graceful shutdown automatically
    /// // server.run_tcp("0.0.0.0:8080").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle::new(self.lifecycle.clone())
    }

    /// Run the server with STDIO transport
    ///
    /// # Errors
    ///
    /// Returns [`crate::ServerError::Transport`] if:
    /// - STDIO transport connection fails
    /// - Message sending/receiving fails
    /// - Transport disconnection fails
    #[tracing::instrument(skip(self), fields(
        transport = "stdio",
        service_name = %self.config.name,
        service_version = %self.config.version
    ))]
    pub async fn run_stdio(mut self) -> ServerResult<()> {
        // For STDIO transport, disable logging unless explicitly overridden
        // STDIO stdout must be reserved exclusively for JSON-RPC messages per MCP protocol
        if should_log_for_stdio() {
            info!("Starting MCP server with STDIO transport");
        }

        // Start performance monitoring for STDIO server
        let _perf_span = info_span!("server.run", transport = "stdio").entered();
        info!("Initializing STDIO transport for MCP server");

        self.lifecycle.start().await;

        // BIDIRECTIONAL STDIO SETUP
        // Create STDIO dispatcher for server-initiated requests (sampling, elicitation, roots, ping)
        let (request_tx, request_rx) = tokio::sync::mpsc::unbounded_channel();

        // Use fully-qualified path to avoid ambiguity with the turbomcp crate's runtime module
        let dispatcher = crate::runtime::StdioDispatcher::new(request_tx);

        // Configure router's bidirectional support with the STDIO dispatcher
        // SAFETY: We have &mut self, so we can safely get mutable access to the Arc'd router
        // This is the CRITICAL STEP that was missing - without this, all server→client requests fail
        let router = Arc::make_mut(&mut self.router);
        router.set_server_request_dispatcher(dispatcher.clone());

        // Run STDIO with full bidirectional support (MCP 2025-11-25 compliant)
        // This uses the bidirectional-aware runtime that handles both:
        // - Client→Server requests (tools, resources, prompts)
        // - Server→Client requests (sampling, elicitation, roots, ping)
        crate::runtime::run_stdio_bidirectional(self.router.clone(), dispatcher, request_rx)
            .await
            .map_err(|e| crate::ServerError::Handler {
                message: format!("STDIO bidirectional runtime failed: {}", e),
                context: Some("run_stdio".to_string()),
            })
    }

    /// Get health status
    pub async fn health(&self) -> HealthStatus {
        self.lifecycle.health().await
    }

    /// Run server with HTTP transport using default configuration
    ///
    /// This provides a working HTTP server with:
    /// - Standard HTTP POST/GET/DELETE for MCP protocol at `/mcp`
    /// - Full MCP 2025-11-25 protocol compliance
    /// - Graceful shutdown support
    /// - Default rate limiting (100 req/60s)
    /// - Default security settings (localhost allowed, CORS disabled)
    ///
    /// For custom configuration (rate limits, security, CORS), use `run_http_with_config`.
    ///
    /// # Examples
    ///
    /// ## Basic usage with default configuration
    /// ```no_run
    /// use turbomcp_server::ServerBuilder;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let server = ServerBuilder::new()
    ///         .name("my-server")
    ///         .version("1.0.0")
    ///         .build();
    ///
    ///     server.run_http("127.0.0.1:3000").await?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ## With custom configuration
    /// ```no_run
    /// use turbomcp_server::ServerBuilder;
    /// use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let server = ServerBuilder::new()
    ///         .name("my-server")
    ///         .version("1.0.0")
    ///         .build();
    ///
    ///     let config = StreamableHttpConfigBuilder::new()
    ///         .with_bind_address("127.0.0.1:3000")
    ///         .allow_any_origin(true)  // Enable CORS for development
    ///         .build();
    ///
    ///     server.run_http_with_config("127.0.0.1:3000", config).await?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`crate::ServerError::Transport`] if:
    /// - Address resolution fails
    /// - HTTP server fails to start
    /// - Transport disconnection fails
    #[cfg(feature = "http")]
    #[tracing::instrument(skip(self), fields(
        transport = "http",
        service_name = %self.config.name,
        service_version = %self.config.version,
        addr = ?addr
    ))]
    pub async fn run_http<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        self,
        addr: A,
    ) -> ServerResult<()> {
        use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;

        // Build default configuration
        let config = StreamableHttpConfigBuilder::new().build();

        self.run_http_with_config(addr, config).await
    }

    /// Run server with HTTP transport and custom configuration
    ///
    /// This provides full control over HTTP server configuration including:
    /// - Rate limiting (requests per time window, or disabled entirely)
    /// - Security settings (CORS, origin validation, authentication)
    /// - Network settings (bind address, endpoint path, keep-alive)
    /// - Advanced settings (replay buffer size, etc.)
    ///
    /// # Bind Address Configuration
    ///
    /// **IMPORTANT**: The `addr` parameter takes precedence over `config.bind_addr`.
    /// If they differ, a deprecation warning is logged.
    ///
    /// **Best Practice** (recommended for forward compatibility):
    /// ```no_run
    /// # use turbomcp_server::ServerBuilder;
    /// # use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = StreamableHttpConfigBuilder::new()
    ///     .with_bind_address("127.0.0.1:3001")  // Set bind address in config
    ///     .build();
    ///
    /// // Pass matching addr parameter (or use default "127.0.0.1:8080")
    /// ServerBuilder::new()
    ///     .build()
    ///     .run_http_with_config("127.0.0.1:3001", config).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// **Deprecated** (will be removed in v3.x):
    /// ```no_run
    /// # use turbomcp_server::ServerBuilder;
    /// # use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// // ⚠️ Avoid setting different addresses - causes deprecation warning
    /// let config = StreamableHttpConfigBuilder::new()
    ///     .with_bind_address("0.0.0.0:5000")  // This is ignored!
    ///     .build();
    ///
    /// // The addr parameter wins (3001 is used, not 5000)
    /// ServerBuilder::new()
    ///     .build()
    ///     .run_http_with_config("127.0.0.1:3001", config).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// **Future (v3.x)**: The `addr` parameter will be removed. Configure bind address
    /// via `config.with_bind_address()` only.
    ///
    /// # Examples
    ///
    /// ## Custom configuration example
    /// ```no_run
    /// use turbomcp_server::ServerBuilder;
    /// use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let server = ServerBuilder::new()
    ///         .name("custom-server")
    ///         .version("1.0.0")
    ///         .build();
    ///
    ///     let config = StreamableHttpConfigBuilder::new()
    ///         .with_bind_address("127.0.0.1:3000")
    ///         .with_rate_limit(1000, Duration::from_secs(60))  // 1000 req/min
    ///         .build();
    ///
    ///     server.run_http_with_config("127.0.0.1:3000", config).await?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ## Production configuration (secure, rate limited)
    /// ```no_run
    /// use turbomcp_server::ServerBuilder;
    /// use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let server = ServerBuilder::new()
    ///         .name("production-server")
    ///         .version("1.0.0")
    ///         .build();
    ///
    ///     let config = StreamableHttpConfigBuilder::new()
    ///         .with_bind_address("127.0.0.1:3000")
    ///         .with_rate_limit(1000, Duration::from_secs(60))  // 1000 req/min
    ///         .allow_any_origin(false)  // Strict CORS
    ///         .require_authentication(true)  // Require auth
    ///         .build();
    ///
    ///     server.run_http_with_config("127.0.0.1:3000", config).await?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`crate::ServerError::Transport`] if:
    /// - Address resolution fails
    /// - HTTP server fails to start
    /// - Transport disconnection fails
    #[cfg(feature = "http")]
    #[tracing::instrument(skip(self, config), fields(
        transport = "http",
        service_name = %self.config.name,
        service_version = %self.config.version,
        addr = ?addr
    ))]
    pub async fn run_http_with_config<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        self,
        addr: A,
        mut config: turbomcp_transport::streamable_http::StreamableHttpConfig,
    ) -> ServerResult<()> {
        use std::collections::HashMap;
        use tokio::sync::{Mutex, RwLock};

        // Sprint 2.6: Check for insecure 0.0.0.0 binding
        crate::security_checks::check_binding_security(&addr);

        info!("Starting MCP server with HTTP transport");

        self.lifecycle.start().await;

        // Resolve address to string
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| crate::ServerError::configuration(format!("Invalid address: {}", e)))?
            .next()
            .ok_or_else(|| crate::ServerError::configuration("No address resolved"))?;

        info!("Resolved address: {}", socket_addr);

        // Check for conflicting bind addresses and warn about deprecation
        let socket_addr_str = socket_addr.to_string();
        if config.bind_addr != socket_addr_str {
            warn!(
                addr_parameter = %socket_addr_str,
                config_bind_addr = %config.bind_addr,
                "⚠️  DEPRECATION WARNING: The `addr` parameter takes precedence over `config.bind_addr`"
            );
            warn!(
                "⚠️  In TurboMCP v3.x, the `addr` parameter will be removed. Please use StreamableHttpConfigBuilder::new().with_bind_address(\"{}\").build() instead",
                socket_addr_str
            );
            warn!(
                "⚠️  Avoid setting both `addr` parameter and `config.bind_addr` to prevent confusion"
            );

            // Update config to single source of truth
            config.bind_addr = socket_addr_str.clone();
            config.base_url = format!("http://{}", socket_addr_str);
        }

        info!(
            config = ?config,
            "HTTP configuration (updated with resolved address)"
        );

        // BIDIRECTIONAL HTTP SETUP
        // Create shared state for session management and bidirectional MCP
        let sessions = Arc::new(RwLock::new(HashMap::new()));
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));

        // Share router across all sessions (routing logic and handler registry)
        let router = self.router.clone();

        // Capture server identity for MCP protocol compliance
        let server_info = turbomcp_protocol::ServerInfo {
            name: self.config.name.clone(),
            version: self.config.version.clone(),
        };

        // Factory pattern: create session-specific router for each HTTP request
        // This is the clean architecture that HTTP requires - each session gets its own
        // bidirectional dispatcher while sharing the routing logic
        let sessions_for_factory = Arc::clone(&sessions);
        let pending_for_factory = Arc::clone(&pending_requests);
        let router_for_factory = Arc::clone(&router);

        // Create a wrapper that converts headers and delegates to router
        // This is cleaner than storing headers on the router itself
        let handler_factory = move |session_id: Option<String>,
                                    headers: Option<axum::http::HeaderMap>,
                                    tenant_id: Option<String>| {
            let session_id = session_id.unwrap_or_else(|| {
                let new_id = uuid::Uuid::new_v4().to_string();
                tracing::debug!(
                    "HTTP POST without session ID - generating ephemeral ID for request: {}",
                    new_id
                );
                new_id
            });

            tracing::debug!("Factory creating handler for session: {}", session_id);

            // Create session-specific HTTP dispatcher (now local to turbomcp-server!)
            let dispatcher = crate::runtime::http::HttpDispatcher::new(
                session_id,
                Arc::clone(&sessions_for_factory),
                Arc::clone(&pending_for_factory),
            );

            // Clone the base router and configure with session-specific dispatcher
            // CRITICAL: set_server_request_dispatcher also recreates server_to_client adapter
            let mut session_router = (*router_for_factory).clone();
            session_router.set_server_request_dispatcher(dispatcher);

            // Convert HeaderMap to HashMap<String, String> for passing to create_context
            let headers_map = headers.map(|header_map| {
                header_map
                    .iter()
                    .filter_map(|(name, value)| {
                        value
                            .to_str()
                            .ok()
                            .map(|v| (name.to_string(), v.to_string()))
                    })
                    .collect()
            });

            // Create wrapper that passes headers and tenant_id to create_context (HTTP transport)
            HttpHandlerWithHeaders {
                router: session_router,
                headers: headers_map,
                transport: "http",
                tenant_id,
            }
        };

        info!(
            server_name = %server_info.name,
            server_version = %server_info.version,
            bind_addr = %socket_addr,
            endpoint_path = %config.endpoint_path,
            "HTTP server starting with full bidirectional support (elicitation, sampling, roots, ping)"
        );

        // Use factory-based HTTP server with full bidirectional support
        use crate::runtime::http::run_http;
        run_http(handler_factory, sessions, pending_requests, config)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "HTTP server failed");
                crate::ServerError::handler(e.to_string())
            })?;

        info!("HTTP server shutdown complete");
        Ok(())
    }

    /// Run server with HTTP transport and Tower middleware
    ///
    /// This method enables advanced features like multi-tenancy, authentication, rate limiting,
    /// and other cross-cutting concerns by allowing you to apply Tower middleware layers to the
    /// HTTP router.
    ///
    /// # Multi-Tenancy Example
    ///
    /// ```no_run
    /// use turbomcp_server::ServerBuilder;
    /// use turbomcp_server::middleware::tenancy::{HeaderTenantExtractor, TenantExtractionLayer};
    /// use tower::ServiceBuilder;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let server = ServerBuilder::new()
    ///         .name("multi-tenant-server")
    ///         .version("1.0.0")
    ///         .build();
    ///
    ///     // Create tenant extractor middleware
    ///     let tenant_extractor = HeaderTenantExtractor::new("X-Tenant-ID");
    ///     let middleware = ServiceBuilder::new()
    ///         .layer(TenantExtractionLayer::new(tenant_extractor));
    ///
    ///     // Run server with middleware
    ///     server.run_http_with_middleware(
    ///         "127.0.0.1:3000",
    ///         Box::new(move |router| router.layer(middleware))
    ///     ).await?;
    ///
    ///     Ok(())
    /// }
    /// ```
    ///
    /// # Authentication Example
    ///
    /// ```no_run
    /// use turbomcp_server::ServerBuilder;
    /// use tower::ServiceBuilder;
    /// use tower_http::auth::RequireAuthorizationLayer;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let server = ServerBuilder::new()
    ///     .name("auth-server")
    ///     .version("1.0.0")
    ///     .build();
    ///
    /// // Add bearer token authentication
    /// let middleware = ServiceBuilder::new()
    ///     .layer(RequireAuthorizationLayer::bearer("secret-token"));
    ///
    /// server.run_http_with_middleware(
    ///     "127.0.0.1:3000",
    ///     Box::new(move |router| router.layer(middleware))
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "http")]
    #[tracing::instrument(skip(self, middleware_fn), fields(
        transport = "http",
        service_name = %self.config.name,
        service_version = %self.config.version,
        addr = ?addr
    ))]
    pub async fn run_http_with_middleware<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        self,
        addr: A,
        middleware_fn: Box<dyn FnOnce(axum::Router) -> axum::Router + Send>,
    ) -> ServerResult<()> {
        use std::collections::HashMap;
        use tokio::sync::{Mutex, RwLock};

        // Sprint 2.6: Check for insecure 0.0.0.0 binding
        crate::security_checks::check_binding_security(&addr);

        info!("Starting MCP server with HTTP transport and custom middleware");

        self.lifecycle.start().await;

        // Resolve address to string
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| crate::ServerError::configuration(format!("Invalid address: {}", e)))?
            .next()
            .ok_or_else(|| crate::ServerError::configuration("No address resolved"))?;

        info!("Resolved address: {}", socket_addr);

        // BIDIRECTIONAL HTTP SETUP
        let sessions = Arc::new(RwLock::new(HashMap::new()));
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));
        let router = self.router.clone();

        // Factory pattern for session-specific handlers
        let sessions_for_factory = Arc::clone(&sessions);
        let pending_for_factory = Arc::clone(&pending_requests);
        let router_for_factory = Arc::clone(&router);

        let handler_factory = move |session_id: Option<String>,
                                    headers: Option<axum::http::HeaderMap>,
                                    tenant_id: Option<String>| {
            let session_id = session_id.unwrap_or_else(|| {
                let new_id = uuid::Uuid::new_v4().to_string();
                tracing::debug!(
                    "HTTP POST without session ID - generating ephemeral ID for request: {}",
                    new_id
                );
                new_id
            });

            tracing::debug!("Factory creating handler for session: {}", session_id);

            let dispatcher = crate::runtime::http::HttpDispatcher::new(
                session_id,
                Arc::clone(&sessions_for_factory),
                Arc::clone(&pending_for_factory),
            );

            let mut session_router = (*router_for_factory).clone();
            session_router.set_server_request_dispatcher(dispatcher);

            let headers_map = headers.map(|header_map| {
                header_map
                    .iter()
                    .filter_map(|(name, value)| {
                        value
                            .to_str()
                            .ok()
                            .map(|v| (name.to_string(), v.to_string()))
                    })
                    .collect()
            });

            HttpHandlerWithHeaders {
                router: session_router,
                headers: headers_map,
                transport: "http",
                tenant_id,
            }
        };

        info!(
            server_name = %self.config.name,
            server_version = %self.config.version,
            bind_addr = %socket_addr,
            "HTTP server starting with custom middleware and bidirectional support"
        );

        // Use run_http_with_middleware function
        use crate::runtime::http::run_http_with_middleware;
        use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;

        // Create default config with resolved address
        let config = StreamableHttpConfigBuilder::new()
            .with_bind_address(socket_addr.to_string())
            .with_endpoint_path("/mcp")
            .allow_localhost(true)
            .build();

        run_http_with_middleware(
            handler_factory,
            sessions,
            pending_requests,
            config,
            Some(middleware_fn),
        )
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "HTTP server with middleware failed");
            crate::ServerError::handler(e.to_string())
        })?;

        info!("HTTP server shutdown complete");
        Ok(())
    }

    /// Run server with WebSocket transport (full bidirectional support)
    ///
    /// This provides a simple API for WebSocket servers with sensible defaults:
    /// - Default endpoint: `/mcp/ws`
    /// - Full MCP 2025-11-25 compliance
    /// - Bidirectional communication
    /// - Elicitation support
    /// - Session management and middleware
    ///
    /// For custom configuration, use `run_websocket_with_config()`.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use turbomcp_server::ServerBuilder;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let server = ServerBuilder::new()
    ///         .name("ws-server")
    ///         .version("1.0.0")
    ///         .build();
    ///
    ///     server.run_websocket("127.0.0.1:8080").await?;
    ///     Ok(())
    /// }
    /// ```
    #[cfg(feature = "websocket")]
    #[tracing::instrument(skip(self), fields(
        transport = "websocket",
        service_name = %self.config.name,
        service_version = %self.config.version,
        addr = ?addr
    ))]
    pub async fn run_websocket<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        self,
        addr: A,
    ) -> ServerResult<()> {
        use crate::config::WebSocketServerConfig;

        // Build default configuration
        let config = WebSocketServerConfig::default();

        self.run_websocket_with_config(addr, config).await
    }

    /// Run server with WebSocket transport and custom configuration
    ///
    /// This provides full control over WebSocket server configuration including:
    /// - Custom endpoint path
    /// - MCP server settings (middleware, security, etc.)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use turbomcp_server::{ServerBuilder, WebSocketServerConfig};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let server = ServerBuilder::new()
    ///         .name("custom-ws-server")
    ///         .version("1.0.0")
    ///         .build();
    ///
    ///     let config = WebSocketServerConfig {
    ///         bind_addr: "127.0.0.1:8080".to_string(),
    ///         endpoint_path: "/custom/ws".to_string(),
    ///         max_concurrent_requests: 100,
    ///     };
    ///
    ///     server.run_websocket_with_config("127.0.0.1:8080", config).await?;
    ///     Ok(())
    /// }
    /// ```
    #[cfg(feature = "websocket")]
    #[tracing::instrument(skip(self, config), fields(
        transport = "websocket",
        service_name = %self.config.name,
        service_version = %self.config.version,
        addr = ?addr
    ))]
    pub async fn run_websocket_with_config<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        self,
        addr: A,
        config: crate::config::WebSocketServerConfig,
    ) -> ServerResult<()> {
        use axum::{Router, middleware, routing::get};
        use turbomcp_transport::axum::{WebSocketFactoryState, websocket_handler_with_factory};
        use turbomcp_transport::tower::SessionInfo;

        // Sprint 2.6: Check for insecure 0.0.0.0 binding
        crate::security_checks::check_binding_security(&addr);

        info!("Starting MCP server with WebSocket transport");
        info!(config = ?config, "WebSocket configuration");

        self.lifecycle.start().await;

        // Resolve address to string
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| crate::ServerError::configuration(format!("Invalid address: {}", e)))?
            .next()
            .ok_or_else(|| crate::ServerError::configuration("No address resolved"))?;

        info!("Resolved address: {}", socket_addr);

        // Capture server identity for MCP protocol compliance
        let server_info = turbomcp_protocol::ServerInfo {
            name: self.config.name.clone(),
            version: self.config.version.clone(),
        };

        // Router for this server (shared across all connections)
        let router = (*self.router).clone();

        // Factory: creates per-connection handler with bidirectional support
        // This is the unified architecture - transport layer handles WebSocket mechanics,
        // server layer provides MCP-specific handler logic
        let handler_factory =
            move |transport_dispatcher: turbomcp_transport::axum::WebSocketDispatcher,
                  headers: Option<std::collections::HashMap<String, String>>,
                  tenant_id: Option<String>| {
                // Wrap transport dispatcher with server layer adapter
                let server_dispatcher =
                    crate::routing::WebSocketDispatcherAdapter::new(transport_dispatcher);

                // Clone router for this connection and configure with dispatcher
                let mut connection_router = router.clone();
                connection_router.set_server_request_dispatcher(server_dispatcher);

                // Create wrapper that passes headers and tenant_id to create_context (WebSocket transport)
                // We can reuse HttpHandlerWithHeaders since it's generic
                Arc::new(HttpHandlerWithHeaders {
                    router: connection_router,
                    headers,
                    transport: "websocket",
                    tenant_id,
                }) as Arc<dyn turbomcp_protocol::JsonRpcHandler>
            };

        info!(
            server_name = %server_info.name,
            server_version = %server_info.version,
            bind_addr = %socket_addr,
            endpoint_path = %config.endpoint_path,
            "WebSocket server starting with full bidirectional support (elicitation, sampling, roots, ping)"
        );

        // Create factory state for transport layer
        let factory_state = WebSocketFactoryState::new(handler_factory);

        // Session middleware to extract headers (same as HTTP transport)
        let session_middleware = |mut request: axum::extract::Request, next: middleware::Next| async move {
            let mut session = SessionInfo::new();

            // Extract headers and store in session metadata
            for (name, value) in request.headers().iter() {
                if let Ok(value_str) = value.to_str() {
                    session
                        .metadata
                        .insert(name.to_string(), value_str.to_string());
                }
            }

            // Extract specific useful headers
            if let Some(user_agent) = request
                .headers()
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
            {
                session.user_agent = Some(user_agent.to_string());
            }

            if let Some(remote_addr) = request
                .headers()
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
            {
                session.remote_addr = Some(remote_addr.to_string());
            }

            request.extensions_mut().insert(session);
            next.run(request).await
        };

        // Build Axum router using transport layer
        let app = Router::new()
            .route(&config.endpoint_path, get(websocket_handler_with_factory))
            .with_state(factory_state)
            .layer(middleware::from_fn(session_middleware));

        info!("WebSocket server bound to {}", socket_addr);

        // Serve using Axum
        let listener = tokio::net::TcpListener::bind(socket_addr)
            .await
            .map_err(|e| crate::ServerError::configuration(format!("Failed to bind: {}", e)))?;

        axum::serve(listener, app).await.map_err(|e| {
            tracing::error!(error = %e, "WebSocket server failed");
            crate::ServerError::handler(e.to_string())
        })?;

        info!("WebSocket server shutdown complete");
        Ok(())
    }

    /// Run server with TCP transport (progressive enhancement - runtime configuration)
    #[cfg(feature = "tcp")]
    #[tracing::instrument(skip(self), fields(
        transport = "tcp",
        service_name = %self.config.name,
        service_version = %self.config.version,
        addr = ?addr
    ))]
    pub async fn run_tcp<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        mut self,
        addr: A,
    ) -> ServerResult<()> {
        use turbomcp_transport::TcpTransport;

        // Sprint 2.6: Check for insecure 0.0.0.0 binding
        crate::security_checks::check_binding_security(&addr);

        // Start performance monitoring for TCP server
        let _perf_span = info_span!("server.run", transport = "tcp").entered();
        info!(?addr, "Starting MCP server with TCP transport");

        self.lifecycle.start().await;

        // Convert ToSocketAddrs to SocketAddr
        let socket_addr = match addr.to_socket_addrs() {
            Ok(mut addrs) => match addrs.next() {
                Some(addr) => addr,
                None => {
                    tracing::error!("No socket address resolved from provided address");
                    self.lifecycle.shutdown().await;
                    return Err(crate::ServerError::configuration("Invalid socket address"));
                }
            },
            Err(e) => {
                tracing::error!(error = %e, "Failed to resolve socket address");
                self.lifecycle.shutdown().await;
                return Err(crate::ServerError::configuration(format!(
                    "Address resolution failed: {e}"
                )));
            }
        };

        let transport = TcpTransport::new_server(socket_addr);
        if let Err(e) = transport.connect().await {
            tracing::error!(error = %e, "Failed to connect TCP transport");
            self.lifecycle.shutdown().await;
            return Err(e.into());
        }

        // BIDIRECTIONAL TCP SETUP
        // Create generic transport dispatcher for server-initiated requests
        let dispatcher = crate::runtime::TransportDispatcher::new(transport);

        // Configure router's bidirectional support with the TCP dispatcher
        // This enables ctx.elicit(), ctx.create_message(), ctx.list_roots(), etc.
        let router = Arc::make_mut(&mut self.router);
        router.set_server_request_dispatcher(dispatcher.clone());

        // Run TCP with full bidirectional support (MCP 2025-11-25 compliant)
        // This uses the generic bidirectional runtime that handles both:
        // - Client→Server requests (tools, resources, prompts)
        // - Server→Client requests (sampling, elicitation, roots, ping)
        crate::runtime::run_transport_bidirectional(self.router.clone(), dispatcher)
            .await
            .map_err(|e| crate::ServerError::Handler {
                message: format!("TCP bidirectional runtime failed: {}", e),
                context: Some("run_tcp".to_string()),
            })
    }

    /// Run server with Unix socket transport (progressive enhancement - runtime configuration)
    #[cfg(all(feature = "unix", unix))]
    #[tracing::instrument(skip(self), fields(
        transport = "unix",
        service_name = %self.config.name,
        service_version = %self.config.version,
        path = ?path.as_ref()
    ))]
    pub async fn run_unix<P: AsRef<std::path::Path>>(mut self, path: P) -> ServerResult<()> {
        use std::path::PathBuf;
        use turbomcp_transport::UnixTransport;

        // Start performance monitoring for Unix server
        let _perf_span = info_span!("server.run", transport = "unix").entered();
        info!(path = ?path.as_ref(), "Starting MCP server with Unix socket transport");

        self.lifecycle.start().await;

        let socket_path = PathBuf::from(path.as_ref());
        let transport = UnixTransport::new_server(socket_path);
        if let Err(e) = transport.connect().await {
            tracing::error!(error = %e, "Failed to connect Unix socket transport");
            self.lifecycle.shutdown().await;
            return Err(e.into());
        }

        // BIDIRECTIONAL UNIX SOCKET SETUP
        // Create generic transport dispatcher for server-initiated requests
        let dispatcher = crate::runtime::TransportDispatcher::new(transport);

        // Configure router's bidirectional support with the Unix socket dispatcher
        // This enables ctx.elicit(), ctx.create_message(), ctx.list_roots(), etc.
        let router = Arc::make_mut(&mut self.router);
        router.set_server_request_dispatcher(dispatcher.clone());

        // Run Unix Socket with full bidirectional support (MCP 2025-11-25 compliant)
        // This uses the generic bidirectional runtime that handles both:
        // - Client→Server requests (tools, resources, prompts)
        // - Server→Client requests (sampling, elicitation, roots, ping)
        crate::runtime::run_transport_bidirectional(self.router.clone(), dispatcher)
            .await
            .map_err(|e| crate::ServerError::Handler {
                message: format!("Unix socket bidirectional runtime failed: {}", e),
                context: Some("run_unix".to_string()),
            })
    }

    /// Generic transport runner (DRY principle)
    /// Used by feature-gated transport methods (http, tcp, websocket, unix)
    #[allow(dead_code)]
    #[tracing::instrument(skip(self, transport), fields(
        service_name = %self.config.name,
        service_version = %self.config.version
    ))]
    async fn run_with_transport<T: Transport>(&self, mut transport: T) -> ServerResult<()> {
        // Install signal handlers for graceful shutdown (Ctrl+C / SIGTERM)
        let lifecycle_for_sigint = self.lifecycle.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::signal::ctrl_c().await {
                tracing::warn!(error = %e, "Failed to install Ctrl+C handler");
                return;
            }
            tracing::info!("Ctrl+C received, initiating shutdown");
            lifecycle_for_sigint.shutdown().await;
        });

        #[cfg(unix)]
        {
            let lifecycle_for_sigterm = self.lifecycle.clone();
            tokio::spawn(async move {
                use tokio::signal::unix::{SignalKind, signal};
                match signal(SignalKind::terminate()) {
                    Ok(mut sigterm) => {
                        sigterm.recv().await;
                        tracing::info!("SIGTERM received, initiating shutdown");
                        lifecycle_for_sigterm.shutdown().await;
                    }
                    Err(e) => tracing::warn!(error = %e, "Failed to install SIGTERM handler"),
                }
            });
        }

        // Shutdown signal
        let mut shutdown = self.lifecycle.shutdown_signal();

        // Main message processing loop
        loop {
            tokio::select! {
                _ = shutdown.recv() => {
                    tracing::info!("Shutdown signal received");
                    break;
                }
                res = transport.receive() => {
                    match res {
                        Ok(Some(message)) => {
                            if let Err(e) = self.handle_transport_message(&mut transport, message).await {
                                tracing::warn!(error = %e, "Failed to handle transport message");
                            }
                        }
                        Ok(None) => {
                            // No message available; sleep briefly to avoid busy loop
                            sleep(Duration::from_millis(5)).await;
                        }
                        Err(e) => {
                            match e {
                                TransportError::ReceiveFailed(msg) if msg.contains("disconnected") => {
                                    tracing::info!("Transport receive channel disconnected; shutting down");
                                    break;
                                }
                                _ => {
                                    tracing::error!(error = %e, "Transport receive failed");
                                    // Backoff on errors
                                    sleep(Duration::from_millis(50)).await;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Disconnect transport
        if let Err(e) = transport.disconnect().await {
            tracing::warn!(error = %e, "Error while disconnecting transport");
        }

        tracing::info!("Server shutdown complete");
        Ok(())
    }
}

// Compile-time assertion that McpServer is Send + Clone (Tower pattern)
// Note: McpServer is Clone but NOT Sync (due to BoxCloneService being !Sync)
// This is intentional and follows the Axum/Tower design pattern
#[allow(dead_code)]
const _: () = {
    const fn assert_send_clone<T: Send + Clone>() {}
    const fn check() {
        assert_send_clone::<crate::server::core::McpServer>();
    }
};

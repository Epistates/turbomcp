//! Core MCP server implementation
//!
//! Contains the main McpServer struct and its core functionality including
//! middleware building, lifecycle management, and server construction.

use std::sync::Arc;
use tracing::{info, info_span};

use crate::{
    config::ServerConfig,
    error::ServerResult,
    lifecycle::{HealthStatus, ServerLifecycle},
    metrics::ServerMetrics,
    middleware::{MiddlewareStack, RateLimitConfig},
    registry::HandlerRegistry,
    routing::RequestRouter,
    service::McpService,
};

use bytes::Bytes;
use http::{Request, Response};
use tokio::time::{Duration, sleep};
use turbomcp_transport::core::TransportError;
use turbomcp_transport::{StdioTransport, Transport};

use super::shutdown::ShutdownHandle;

/// Check if logging should be enabled for STDIO transport
///
/// For MCP STDIO transport compliance, logging is disabled by default since stdout
/// must be reserved exclusively for JSON-RPC messages. This can be overridden by
/// setting the TURBOMCP_FORCE_LOGGING environment variable.
pub(crate) fn should_log_for_stdio() -> bool {
    std::env::var("TURBOMCP_FORCE_LOGGING").is_ok()
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
/// **Current Status**: The service field is built but not yet wired into the request
/// processing pipeline. See `server/transport.rs` TODOs for integration plan.
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
    fn build_middleware_stack(
        core_service: McpService,
        stack: MiddlewareStack,
    ) -> tower::util::BoxCloneService<Request<Bytes>, Response<Bytes>, crate::ServerError> {
        // WORLD-CLASS TOWER COMPOSITION - Conditional Layer Stacking
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

        // Layer 1: Authorization (innermost - closest to handler)
        // Applied first so it can reject unauthorized requests before expensive operations
        if let Some(authz_config) = stack.authz_config {
            service = tower::util::BoxCloneService::new(
                tower::ServiceBuilder::new()
                    .layer(crate::middleware::AuthzLayer::new(authz_config))
                    .service(service),
            );
        }

        // Layer 2: Validation
        // Validates request structure after auth but before processing
        if let Some(validation_config) = stack.validation_config {
            service = tower::util::BoxCloneService::new(
                tower::ServiceBuilder::new()
                    .layer(crate::middleware::ValidationLayer::new(validation_config))
                    .service(service),
            );
        }

        // Layer 3: Timeout (outermost)
        // Applied last so it can enforce timeout on the entire request pipeline
        if let Some(timeout_config) = stack.timeout_config
            && timeout_config.enabled
        {
            service = tower::util::BoxCloneService::new(
                tower::ServiceBuilder::new()
                    .layer(tower_http::timeout::TimeoutLayer::new(
                        timeout_config.request_timeout,
                    ))
                    .service(service),
            );
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
        let router = Arc::new(RequestRouter::new(
            Arc::clone(&registry),
            Arc::clone(&metrics),
        ));
        // Build middleware stack configuration
        let mut stack = MiddlewareStack::new();

        // Auto-install rate limiting if enabled in config
        if config.rate_limiting.enabled {
            use crate::middleware::rate_limit::{RateLimitStrategy, RateLimits};
            use std::num::NonZeroU32;
            use std::time::Duration;

            let rate_config = RateLimitConfig {
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

        // WORLD-CLASS TOWER SERVICE COMPOSITION
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
        let service = Self::build_middleware_stack(core_service, stack);

        let lifecycle = Arc::new(ServerLifecycle::new());

        Self {
            config,
            registry,
            router,
            service,
            lifecycle,
            metrics,
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
    /// Returns [`ServerError::Transport`] if:
    /// - STDIO transport connection fails
    /// - Message sending/receiving fails
    /// - Transport disconnection fails
    #[tracing::instrument(skip(self), fields(
        transport = "stdio",
        service_name = %self.config.name,
        service_version = %self.config.version
    ))]
    pub async fn run_stdio(self) -> ServerResult<()> {
        // For STDIO transport, disable logging unless explicitly overridden
        // STDIO stdout must be reserved exclusively for JSON-RPC messages per MCP protocol
        if should_log_for_stdio() {
            info!("Starting MCP server with STDIO transport");
        }

        // Start performance monitoring for STDIO server
        let _perf_span = info_span!("server.run", transport = "stdio").entered();
        info!("Initializing STDIO transport for MCP server");

        self.lifecycle.start().await;

        // Initialize STDIO transport
        let mut transport = StdioTransport::new();
        if let Err(e) = transport.connect().await {
            if should_log_for_stdio() {
                tracing::error!(error = %e, "Failed to connect stdio transport");
            } else {
                // Critical errors can go to stderr for debugging
                eprintln!("TurboMCP STDIO transport failed to connect: {}", e);
            }
            self.lifecycle.shutdown().await;
            return Err(e.into());
        }

        self.run_with_transport_stdio_aware(transport).await
    }

    /// Get health status
    pub async fn health(&self) -> HealthStatus {
        self.lifecycle.health().await
    }

    /// Run server with HTTP transport - Simple HTTP/JSON-RPC server
    /// Run server with HTTP transport
    ///
    /// This provides a working HTTP server with:
    /// - Standard HTTP POST for request/response at `/mcp`
    /// - Full MCP protocol compliance
    /// - Graceful shutdown support
    ///
    /// Note: WebSocket and SSE support temporarily disabled due to DashMap lifetime
    /// variance issues that require architectural changes to the handler registry.
    #[cfg(feature = "http")]
    #[tracing::instrument(skip(self), fields(
        transport = "http",
        service_name = %self.config.name,
        service_version = %self.config.version,
        addr = ?_addr
    ))]
    pub async fn run_http<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        self,
        _addr: A,
    ) -> ServerResult<()> {
        // HTTP support is now provided via compile-time routing in the macro-generated code
        // This avoids all DashMap lifetime issues and provides maximum performance
        Err(crate::ServerError::configuration(
            "Direct HTTP support has been replaced with compile-time routing. \
             Use the #[server] macro which generates into_mcp_router() and run_http() methods \
             with MCP 2025-06-18 compliance, zero lifetime issues, and maximum performance.",
        ))
    }

    /// Run server with WebSocket transport (progressive enhancement - runtime configuration)
    /// Note: WebSocket transport in this library is primarily client-oriented
    /// For production WebSocket servers, consider using the ServerBuilder with WebSocket middleware
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
        tracing::info!(
            ?addr,
            "WebSocket transport server mode not implemented - WebSocket transport is client-oriented"
        );
        tracing::info!(
            "Consider using ServerBuilder with WebSocket middleware for WebSocket server functionality"
        );
        Err(crate::ServerError::configuration(
            "WebSocket server transport not supported - use ServerBuilder with middleware",
        ))
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
        self,
        addr: A,
    ) -> ServerResult<()> {
        use turbomcp_transport::TcpTransport;

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

        let mut transport = TcpTransport::new_server(socket_addr);
        if let Err(e) = transport.connect().await {
            tracing::error!(error = %e, "Failed to connect TCP transport");
            self.lifecycle.shutdown().await;
            return Err(e.into());
        }

        self.run_with_transport(transport).await
    }

    /// Run server with Unix socket transport (progressive enhancement - runtime configuration)
    #[cfg(all(feature = "unix", unix))]
    #[tracing::instrument(skip(self), fields(
        transport = "unix",
        service_name = %self.config.name,
        service_version = %self.config.version,
        path = ?path.as_ref()
    ))]
    pub async fn run_unix<P: AsRef<std::path::Path>>(self, path: P) -> ServerResult<()> {
        use std::path::PathBuf;
        use turbomcp_transport::UnixTransport;

        // Start performance monitoring for Unix server
        let _perf_span = info_span!("server.run", transport = "unix").entered();
        info!(path = ?path.as_ref(), "Starting MCP server with Unix socket transport");

        self.lifecycle.start().await;

        let socket_path = PathBuf::from(path.as_ref());
        let mut transport = UnixTransport::new_server(socket_path);
        if let Err(e) = transport.connect().await {
            tracing::error!(error = %e, "Failed to connect Unix socket transport");
            self.lifecycle.shutdown().await;
            return Err(e.into());
        }

        self.run_with_transport(transport).await
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

    /// STDIO-aware transport runner that respects MCP protocol logging requirements
    #[tracing::instrument(skip(self, transport), fields(
        service_name = %self.config.name,
        service_version = %self.config.version,
        transport = "stdio"
    ))]
    async fn run_with_transport_stdio_aware<T: Transport>(
        &self,
        mut transport: T,
    ) -> ServerResult<()> {
        // Install signal handlers for graceful shutdown (Ctrl+C / SIGTERM)
        let lifecycle_for_sigint = self.lifecycle.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::signal::ctrl_c().await {
                if should_log_for_stdio() {
                    tracing::warn!(error = %e, "Failed to install Ctrl+C handler");
                }
                return;
            }
            if should_log_for_stdio() {
                tracing::info!("Ctrl+C received, initiating shutdown");
            }
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
                        if should_log_for_stdio() {
                            tracing::info!("SIGTERM received, initiating shutdown");
                        }
                        lifecycle_for_sigterm.shutdown().await;
                    }
                    Err(e) => {
                        if should_log_for_stdio() {
                            tracing::warn!(error = %e, "Failed to install SIGTERM handler");
                        }
                    }
                }
            });
        }

        // Shutdown signal
        let mut shutdown = self.lifecycle.shutdown_signal();

        // Main message processing loop
        loop {
            tokio::select! {
                _ = shutdown.recv() => {
                    if should_log_for_stdio() {
                        tracing::info!("Shutdown signal received");
                    }
                    break;
                }
                res = transport.receive() => {
                    match res {
                        Ok(Some(message)) => {
                            if let Err(e) = self.handle_transport_message_stdio_aware(&mut transport, message).await
                                && should_log_for_stdio() {
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
                                    if should_log_for_stdio() {
                                        tracing::info!("Transport receive channel disconnected; shutting down");
                                    }
                                    break;
                                }
                                _ => {
                                    if should_log_for_stdio() {
                                        tracing::error!(error = %e, "Transport receive failed");
                                    }
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
        if let Err(e) = transport.disconnect().await
            && should_log_for_stdio()
        {
            tracing::warn!(error = %e, "Error while disconnecting transport");
        }

        if should_log_for_stdio() {
            tracing::info!("Server shutdown complete");
        }
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

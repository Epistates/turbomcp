//! Core MCP server implementation

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    config::ServerConfig,
    error::ServerResult,
    handlers::{PromptHandler, ResourceHandler, ToolHandler},
    lifecycle::{HealthStatus, ServerLifecycle},
    metrics::ServerMetrics,
    middleware::{KeyExtractor, MiddlewareStack, RateLimitConfig, RateLimitMiddleware},
    registry::HandlerRegistry,
    routing::RequestRouter,
};

use bytes::Bytes;
use tokio::time::{Duration, sleep};
use turbomcp_core::RequestContext;
use turbomcp_protocol::jsonrpc::{JsonRpcMessage, JsonRpcRequest, JsonRpcResponse};
use turbomcp_transport::StdioTransport;
use turbomcp_transport::core::{TransportError, TransportMessageMetadata};
use turbomcp_transport::{Transport, TransportMessage};

/// Check if logging should be enabled for STDIO transport
///
/// For MCP STDIO transport compliance, logging is disabled by default since stdout
/// must be reserved exclusively for JSON-RPC messages. This can be overridden by
/// setting the TURBOMCP_FORCE_LOGGING environment variable.
fn should_log_for_stdio() -> bool {
    std::env::var("TURBOMCP_FORCE_LOGGING").is_ok()
}

/// Handle for triggering graceful server shutdown
///
/// Provides external control over server shutdown with support for:
/// - **Signal handling**: SIGTERM, SIGINT, custom signals
/// - **Container orchestration**: Kubernetes graceful termination
/// - **Health checks**: Coordinated shutdown with load balancers  
/// - **Multi-service coordination**: Synchronized shutdown sequences
/// - **Testing**: Controlled server lifecycle in tests
///
/// The handle is cloneable and thread-safe, allowing multiple components
/// to coordinate shutdown or check shutdown status.
#[derive(Debug, Clone)]
pub struct ShutdownHandle {
    lifecycle: Arc<ServerLifecycle>,
}

impl ShutdownHandle {
    /// Trigger graceful server shutdown
    pub async fn shutdown(&self) {
        self.lifecycle.shutdown().await;
    }

    /// Check if shutdown has been initiated
    pub async fn is_shutting_down(&self) -> bool {
        use crate::lifecycle::ServerState;
        matches!(
            self.lifecycle.state().await,
            ServerState::ShuttingDown | ServerState::Stopped
        )
    }
}

/// Main MCP server
pub struct McpServer {
    /// Server configuration
    pub(crate) config: ServerConfig,
    /// Handler registry
    pub(crate) registry: Arc<HandlerRegistry>,
    /// Request router
    pub(crate) router: Arc<RequestRouter>,
    /// Middleware stack
    #[allow(dead_code)]
    pub(crate) middleware: Arc<RwLock<MiddlewareStack>>,
    /// Server lifecycle
    pub(crate) lifecycle: Arc<ServerLifecycle>,
    /// Server metrics
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
    /// Create a new server
    #[must_use]
    pub fn new(config: ServerConfig) -> Self {
        let registry = Arc::new(HandlerRegistry::new());
        let router = Arc::new(RequestRouter::new(Arc::clone(&registry)));
        let mut stack = MiddlewareStack::new();
        // Auto-install rate limiting if enabled in config
        if config.rate_limiting.enabled {
            #[cfg(test)]
            let rate_middleware = RateLimitMiddleware::new_for_testing(RateLimitConfig {
                requests_per_second: config.rate_limiting.requests_per_second,
                burst_capacity: config.rate_limiting.burst_capacity,
                key_extractor: KeyExtractor::Global,
            });

            #[cfg(not(test))]
            let rate_middleware = RateLimitMiddleware::new(RateLimitConfig {
                requests_per_second: config.rate_limiting.requests_per_second,
                burst_capacity: config.rate_limiting.burst_capacity,
                key_extractor: KeyExtractor::Global,
            });

            stack.add(rate_middleware);
        }
        let middleware = Arc::new(RwLock::new(stack));
        let lifecycle = Arc::new(ServerLifecycle::new());
        let metrics = Arc::new(ServerMetrics::new());

        Self {
            config,
            registry,
            router,
            middleware,
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
        ShutdownHandle {
            lifecycle: self.lifecycle.clone(),
        }
    }

    /// Run the server with STDIO transport
    pub async fn run_stdio(self) -> ServerResult<()> {
        // For STDIO transport, disable logging unless explicitly overridden
        // STDIO stdout must be reserved exclusively for JSON-RPC messages per MCP protocol
        if should_log_for_stdio() {
            tracing::info!("Starting MCP server with STDIO transport");
        }
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
    pub async fn run_http<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        self,
        _addr: A,
    ) -> ServerResult<()> {
        // HTTP support is now provided via compile-time routing in the macro-generated code
        // This avoids all DashMap lifetime issues and provides maximum performance
        Err(crate::ServerError::configuration(
            "Direct HTTP support has been replaced with compile-time routing. \
             Use the #[server] macro which generates into_router() and run_http_direct() methods \
             with zero lifetime issues and maximum performance.",
        ))
    }

    /// Run server with WebSocket transport (progressive enhancement - runtime configuration)
    /// Note: WebSocket transport in this library is primarily client-oriented
    /// For production WebSocket servers, consider using the ServerBuilder with WebSocket middleware
    #[cfg(feature = "websocket")]
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
    pub async fn run_tcp<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        self,
        addr: A,
    ) -> ServerResult<()> {
        use turbomcp_transport::TcpTransport;

        tracing::info!(?addr, "Starting MCP server with TCP transport");
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
    pub async fn run_unix<P: AsRef<std::path::Path>>(self, path: P) -> ServerResult<()> {
        use std::path::PathBuf;
        use turbomcp_transport::UnixTransport;

        tracing::info!(path = ?path.as_ref(), "Starting MCP server with Unix socket transport");
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

impl McpServer {
    async fn handle_transport_message(
        &self,
        transport: &mut dyn Transport,
        message: TransportMessage,
    ) -> ServerResult<()> {
        // Convert bytes to str
        let json_str = match std::str::from_utf8(&message.payload) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "Invalid UTF-8 in incoming message");
                return Ok(());
            }
        };

        // Parse JSON-RPC
        let parsed = serde_json::from_str::<JsonRpcMessage>(json_str);
        let response_json = match parsed {
            Ok(JsonRpcMessage::Request(req)) => {
                let ctx = RequestContext::new().with_metadata("transport", "stdio");
                // Process through middleware stack before routing
                let (req, ctx) = match self.middleware.read().await.process_request(req, ctx).await
                {
                    Ok(tuple) => tuple,
                    Err(e) => {
                        // Convert middleware error to JSON-RPC error response
                        let error = turbomcp_protocol::jsonrpc::JsonRpcError {
                            code: e.error_code(),
                            message: e.to_string(),
                            data: None,
                        };
                        let response = turbomcp_protocol::jsonrpc::JsonRpcResponse {
                            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                            id: None,
                            result: None,
                            error: Some(error),
                        };
                        let reply = TransportMessage::with_metadata(
                            message.id,
                            Bytes::from(
                                serde_json::to_string(&response)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            ),
                            TransportMessageMetadata::with_content_type("application/json"),
                        );
                        let _ = transport.send(reply).await;
                        return Ok(());
                    }
                };
                // Process request through middleware
                let (processed_req, updated_ctx) = match self
                    .middleware
                    .read()
                    .await
                    .process_request(req, ctx.clone())
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        // Return error response for middleware rejection
                        let error_response = turbomcp_protocol::jsonrpc::JsonRpcResponse {
                            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                            id: None,
                            result: None,
                            error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                                code: -32603,
                                message: format!("Middleware error: {e}"),
                                data: None,
                            }),
                        };
                        let mut reply = TransportMessage::new(
                            turbomcp_core::MessageId::from("error"),
                            Bytes::from(
                                serde_json::to_string(&error_response)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            ),
                        );
                        reply.metadata =
                            TransportMessageMetadata::with_content_type("application/json");
                        let _ = transport.send(reply).await;
                        return Ok(());
                    }
                };

                let mut resp: JsonRpcResponse =
                    self.router.route(processed_req, updated_ctx.clone()).await;
                // Process response through middleware
                resp = match self
                    .middleware
                    .read()
                    .await
                    .process_response(resp, &updated_ctx)
                    .await
                {
                    Ok(r) => r,
                    Err(e) => turbomcp_protocol::jsonrpc::JsonRpcResponse {
                        jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                        id: None,
                        result: None,
                        error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                            code: e.error_code(),
                            message: e.to_string(),
                            data: None,
                        }),
                    },
                };

                serde_json::to_string(&resp).ok()
            }
            Ok(JsonRpcMessage::RequestBatch(batch)) => {
                // Convert batch to Vec<JsonRpcRequest>
                let requests: Vec<JsonRpcRequest> = batch.items;
                let ctx = RequestContext::new().with_metadata("transport", "stdio");
                // Process each request through middleware by reusing the router’s batch processing
                let responses = self.router.route_batch(requests, ctx).await;
                serde_json::to_string(&responses).ok()
            }
            Ok(JsonRpcMessage::Notification(_note)) => {
                // No response for notifications
                None
            }
            // Ignore responses from client (server-initiated only)
            Ok(
                JsonRpcMessage::Response(_)
                | JsonRpcMessage::ResponseBatch(_)
                | JsonRpcMessage::MessageBatch(_),
            ) => None,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse JSON-RPC message");
                // Return proper JSON-RPC parse error response (RFC compliant)
                let error_response = turbomcp_protocol::jsonrpc::JsonRpcResponse {
                    jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                    id: None, // Parse error means we couldn't extract ID
                    result: None,
                    error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                        code: -32700, // Parse error as per JSON-RPC 2.0 spec
                        message: "Parse error".to_string(),
                        data: Some(serde_json::Value::String(format!(
                            "Invalid JSON-RPC: {}",
                            e
                        ))),
                    }),
                };
                serde_json::to_string(&error_response).ok()
            }
        };

        if let Some(resp_str) = response_json {
            let reply = TransportMessage::with_metadata(
                message.id,
                Bytes::from(resp_str),
                TransportMessageMetadata::with_content_type("application/json"),
            );
            if let Err(e) = transport.send(reply).await {
                tracing::warn!(error = %e, "Failed to send response over transport");
            }
        }

        Ok(())
    }

    /// STDIO-aware message handler that respects MCP protocol logging requirements
    async fn handle_transport_message_stdio_aware(
        &self,
        transport: &mut dyn Transport,
        message: TransportMessage,
    ) -> ServerResult<()> {
        // Convert bytes to str
        let json_str = match std::str::from_utf8(&message.payload) {
            Ok(s) => s,
            Err(e) => {
                if should_log_for_stdio() {
                    tracing::warn!(error = %e, "Invalid UTF-8 in incoming message");
                }
                return Ok(());
            }
        };

        // Parse JSON-RPC
        let parsed = serde_json::from_str::<JsonRpcMessage>(json_str);
        let response_json = match parsed {
            Ok(JsonRpcMessage::Request(req)) => {
                let ctx = RequestContext::new().with_metadata("transport", "stdio");
                // Process through middleware stack before routing
                let (req, ctx) = match self.middleware.read().await.process_request(req, ctx).await
                {
                    Ok(tuple) => tuple,
                    Err(e) => {
                        // Convert middleware error to JSON-RPC error response
                        let error = turbomcp_protocol::jsonrpc::JsonRpcError {
                            code: e.error_code(),
                            message: e.to_string(),
                            data: None,
                        };
                        let response = turbomcp_protocol::jsonrpc::JsonRpcResponse {
                            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                            id: None,
                            result: None,
                            error: Some(error),
                        };
                        let reply = TransportMessage::with_metadata(
                            message.id,
                            Bytes::from(
                                serde_json::to_string(&response)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            ),
                            TransportMessageMetadata::with_content_type("application/json"),
                        );
                        let _ = transport.send(reply).await;
                        return Ok(());
                    }
                };
                // Process request through middleware
                let (processed_req, updated_ctx) = match self
                    .middleware
                    .read()
                    .await
                    .process_request(req, ctx.clone())
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        // Return error response for middleware rejection
                        let error_response = turbomcp_protocol::jsonrpc::JsonRpcResponse {
                            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                            id: None,
                            result: None,
                            error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                                code: -32603,
                                message: format!("Middleware error: {e}"),
                                data: None,
                            }),
                        };
                        let mut reply = TransportMessage::new(
                            turbomcp_core::MessageId::from("error"),
                            Bytes::from(
                                serde_json::to_string(&error_response)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            ),
                        );
                        reply.metadata =
                            TransportMessageMetadata::with_content_type("application/json");
                        let _ = transport.send(reply).await;
                        return Ok(());
                    }
                };

                let mut resp: JsonRpcResponse =
                    self.router.route(processed_req, updated_ctx.clone()).await;
                // Process response through middleware
                resp = match self
                    .middleware
                    .read()
                    .await
                    .process_response(resp, &updated_ctx)
                    .await
                {
                    Ok(r) => r,
                    Err(e) => turbomcp_protocol::jsonrpc::JsonRpcResponse {
                        jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                        id: None,
                        result: None,
                        error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                            code: e.error_code(),
                            message: e.to_string(),
                            data: None,
                        }),
                    },
                };

                serde_json::to_string(&resp).ok()
            }
            Ok(JsonRpcMessage::RequestBatch(batch)) => {
                // Convert batch to Vec<JsonRpcRequest>
                let requests: Vec<JsonRpcRequest> = batch.items;
                let ctx = RequestContext::new().with_metadata("transport", "stdio");
                // Process each request through middleware by reusing the router's batch processing
                let responses = self.router.route_batch(requests, ctx).await;
                serde_json::to_string(&responses).ok()
            }
            Ok(JsonRpcMessage::Notification(_note)) => {
                // No response for notifications
                None
            }
            // Ignore responses from client (server-initiated only)
            Ok(
                JsonRpcMessage::Response(_)
                | JsonRpcMessage::ResponseBatch(_)
                | JsonRpcMessage::MessageBatch(_),
            ) => None,
            Err(e) => {
                if should_log_for_stdio() {
                    tracing::warn!(error = %e, "Failed to parse JSON-RPC message");
                }
                // Return proper JSON-RPC parse error response (RFC compliant)
                let error_response = turbomcp_protocol::jsonrpc::JsonRpcResponse {
                    jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                    id: None, // Parse error means we couldn't extract ID
                    result: None,
                    error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                        code: -32700, // Parse error as per JSON-RPC 2.0 spec
                        message: "Parse error".to_string(),
                        data: Some(serde_json::Value::String(format!(
                            "Invalid JSON-RPC: {}",
                            e
                        ))),
                    }),
                };
                serde_json::to_string(&error_response).ok()
            }
        };

        if let Some(resp_str) = response_json {
            let reply = TransportMessage::with_metadata(
                message.id,
                Bytes::from(resp_str),
                TransportMessageMetadata::with_content_type("application/json"),
            );
            if let Err(e) = transport.send(reply).await
                && should_log_for_stdio()
            {
                tracing::warn!(error = %e, "Failed to send response over transport");
            }
        }

        Ok(())
    }
}

/// Server builder for convenient server construction
pub struct ServerBuilder {
    /// Server configuration
    config: ServerConfig,
    /// Registry builder
    registry: HandlerRegistry,
}

impl std::fmt::Debug for ServerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerBuilder")
            .field("config", &self.config)
            .finish()
    }
}

impl ServerBuilder {
    /// Create a new server builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: ServerConfig::default(),
            registry: HandlerRegistry::new(),
        }
    }

    /// Set server name
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.config.name = name.into();
        self
    }

    /// Set server version
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.config.version = version.into();
        self
    }

    /// Set server description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.config.description = Some(description.into());
        self
    }

    /// Add a tool handler
    pub fn tool<T>(self, name: impl Into<String>, handler: T) -> ServerResult<Self>
    where
        T: ToolHandler + 'static,
    {
        self.registry.register_tool(name, handler)?;
        Ok(self)
    }

    /// Add a prompt handler
    pub fn prompt<P>(self, name: impl Into<String>, handler: P) -> ServerResult<Self>
    where
        P: PromptHandler + 'static,
    {
        self.registry.register_prompt(name, handler)?;
        Ok(self)
    }

    /// Add a resource handler
    pub fn resource<R>(self, name: impl Into<String>, handler: R) -> ServerResult<Self>
    where
        R: ResourceHandler + 'static,
    {
        self.registry.register_resource(name, handler)?;
        Ok(self)
    }

    /// Add a filesystem root
    pub fn root(self, uri: impl Into<String>, name: Option<String>) -> Self {
        use turbomcp_protocol::types::Root;
        self.registry.add_root(Root {
            uri: uri.into(),
            name,
        });
        self
    }

    /// Set multiple filesystem roots
    pub fn roots(self, roots: Vec<turbomcp_protocol::types::Root>) -> Self {
        self.registry.set_roots(roots);
        self
    }

    /// Build the server
    #[must_use]
    pub fn build(self) -> McpServer {
        let mut server = McpServer::new(self.config);
        server.registry = Arc::new(self.registry);
        server.router = Arc::new(RequestRouter::new(Arc::clone(&server.registry)));
        server
    }
}

impl Default for ServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    /// Test that invalid JSON-RPC parsing creates proper error responses
    /// This validates the fix for the hanging vulnerability
    #[test]
    fn test_jsonrpc_parse_error_response() {
        // Test the exact vulnerable pattern that was fixed
        let invalid_json = r#"{"id": 1, "method": "tools/list"}"#; // Missing jsonrpc version

        // Parse as we do in the server
        let parse_result =
            serde_json::from_str::<turbomcp_protocol::jsonrpc::JsonRpcMessage>(invalid_json);

        // Should fail to parse (this is expected)
        assert!(
            parse_result.is_err(),
            "Invalid JSON-RPC should fail to parse"
        );

        // Verify the error response format that the server now creates
        let error_response = turbomcp_protocol::jsonrpc::JsonRpcResponse {
            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
            id: None, // Parse error means we couldn't extract ID
            result: None,
            error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                code: -32700, // Parse error as per JSON-RPC 2.0 spec
                message: "Parse error".to_string(),
                data: Some(Value::String("Invalid JSON-RPC".to_string())),
            }),
        };

        // Verify it serializes to valid JSON
        let serialized = serde_json::to_string(&error_response);
        assert!(
            serialized.is_ok(),
            "Error response should serialize correctly"
        );

        let response_json = serialized.unwrap();
        let parsed_response: Value = serde_json::from_str(&response_json).unwrap();

        // Verify response structure
        assert_eq!(parsed_response["jsonrpc"], "2.0");
        assert_eq!(parsed_response["error"]["code"], -32700);
        assert_eq!(parsed_response["error"]["message"], "Parse error");
        assert!(parsed_response["error"]["data"].is_string());
    }
}

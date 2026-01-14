//! Core handler trait for v3 MCP servers.
//!
//! The `McpHandler` trait is the unified interface for all MCP operations.
//! It's automatically implemented by the `#[server]` macro.

use std::future::Future;

use serde_json::Value;
use turbomcp_types::{
    McpError, McpResult, Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool,
    ToolResult,
};

use super::RequestContext;

/// Core handler trait for MCP servers.
///
/// This trait defines the interface for all MCP operations. It's designed to be:
/// - **Transport-agnostic**: Works the same on STDIO, HTTP, WebSocket, and WASM
/// - **Zero-boilerplate**: Automatically implemented by the `#[server]` macro
/// - **Ergonomic**: Simple return types that auto-convert to MCP results
///
/// # Implementation
///
/// You typically don't implement this trait manually. Instead, use the `#[server]` macro:
///
/// ```rust,ignore
/// use turbomcp::prelude::*;
///
/// #[derive(Clone)]
/// struct MyServer;
///
/// #[server(name = "my-server", version = "1.0.0")]
/// impl MyServer {
///     #[tool]
///     async fn my_tool(&self, arg: String) -> String {
///         format!("Hello, {}!", arg)
///     }
/// }
/// ```
///
/// # Manual Implementation
///
/// If you need to implement this trait manually (rare), here's an example:
///
/// ```
/// use std::future::Future;
/// use serde_json::Value;
/// use turbomcp_types::*;
/// use turbomcp_server::v3::{McpHandler, RequestContext};
///
/// #[derive(Clone)]
/// struct MyHandler;
///
/// impl McpHandler for MyHandler {
///     fn server_info(&self) -> ServerInfo {
///         ServerInfo::new("my-handler", "1.0.0")
///     }
///
///     fn list_tools(&self) -> Vec<Tool> {
///         vec![Tool::new("greet", "Say hello")]
///     }
///
///     fn list_resources(&self) -> Vec<Resource> {
///         vec![]
///     }
///
///     fn list_prompts(&self) -> Vec<Prompt> {
///         vec![]
///     }
///
///     fn call_tool(
///         &self,
///         name: &str,
///         args: Value,
///         _ctx: &RequestContext,
///     ) -> impl Future<Output = McpResult<ToolResult>> + Send {
///         let name = name.to_string();
///         async move {
///             match name.as_str() {
///                 "greet" => {
///                     let who = args.get("name")
///                         .and_then(|v| v.as_str())
///                         .unwrap_or("World");
///                     Ok(ToolResult::text(format!("Hello, {}!", who)))
///                 }
///                 _ => Err(McpError::tool_not_found(&name))
///             }
///         }
///     }
///
///     fn read_resource(
///         &self,
///         uri: &str,
///         _ctx: &RequestContext,
///     ) -> impl Future<Output = McpResult<ResourceResult>> + Send {
///         let uri = uri.to_string();
///         async move {
///             Err(McpError::resource_not_found(&uri))
///         }
///     }
///
///     fn get_prompt(
///         &self,
///         name: &str,
///         _args: Option<Value>,
///         _ctx: &RequestContext,
///     ) -> impl Future<Output = McpResult<PromptResult>> + Send {
///         let name = name.to_string();
///         async move {
///             Err(McpError::prompt_not_found(&name))
///         }
///     }
/// }
/// ```
pub trait McpHandler: Clone + Send + Sync + 'static {
    // ===== Server Metadata =====

    /// Returns server information (name, version, description, etc.)
    fn server_info(&self) -> ServerInfo;

    // ===== Capability Listings =====

    /// Returns all available tools.
    ///
    /// Called in response to `tools/list` requests.
    fn list_tools(&self) -> Vec<Tool>;

    /// Returns all available resources.
    ///
    /// Called in response to `resources/list` requests.
    fn list_resources(&self) -> Vec<Resource>;

    /// Returns all available prompts.
    ///
    /// Called in response to `prompts/list` requests.
    fn list_prompts(&self) -> Vec<Prompt>;

    // ===== Request Handlers =====

    /// Calls a tool by name with the given arguments.
    ///
    /// Called in response to `tools/call` requests.
    ///
    /// # Arguments
    /// * `name` - The name of the tool to call
    /// * `args` - JSON arguments for the tool
    /// * `ctx` - Request context with metadata and cancellation
    ///
    /// # Returns
    /// The tool result or an error
    fn call_tool(
        &self,
        name: &str,
        args: Value,
        ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + Send;

    /// Reads a resource by URI.
    ///
    /// Called in response to `resources/read` requests.
    ///
    /// # Arguments
    /// * `uri` - The URI of the resource to read
    /// * `ctx` - Request context with metadata and cancellation
    ///
    /// # Returns
    /// The resource content or an error
    fn read_resource(
        &self,
        uri: &str,
        ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + Send;

    /// Gets a prompt by name with optional arguments.
    ///
    /// Called in response to `prompts/get` requests.
    ///
    /// # Arguments
    /// * `name` - The name of the prompt
    /// * `args` - Optional JSON arguments for the prompt
    /// * `ctx` - Request context with metadata and cancellation
    ///
    /// # Returns
    /// The prompt messages or an error
    fn get_prompt(
        &self,
        name: &str,
        args: Option<Value>,
        ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + Send;

    // ===== Optional Hooks =====

    /// Called when the server is initialized.
    ///
    /// Override this to perform setup tasks like loading configuration,
    /// establishing database connections, etc.
    ///
    /// Default implementation does nothing.
    fn on_initialize(&self) -> impl Future<Output = McpResult<()>> + Send {
        async { Ok(()) }
    }

    /// Called when the server is shutting down.
    ///
    /// Override this to perform cleanup tasks like flushing buffers,
    /// closing connections, etc.
    ///
    /// Default implementation does nothing.
    fn on_shutdown(&self) -> impl Future<Output = McpResult<()>> + Send {
        async { Ok(()) }
    }
}

/// Extension trait for running McpHandler on various transports.
///
/// This trait is automatically implemented for all types that implement `McpHandler`.
/// The actual transport implementations are feature-gated.
pub trait McpHandlerExt: McpHandler {
    /// Run the handler on STDIO transport.
    ///
    /// This is the default transport for MCP servers.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use turbomcp::prelude::*;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     MyServer.run_stdio().await.unwrap();
    /// }
    /// ```
    #[cfg(feature = "stdio")]
    fn run_stdio(&self) -> impl Future<Output = McpResult<()>> + Send;

    /// Run the handler on HTTP transport with Server-Sent Events.
    ///
    /// # Arguments
    /// * `addr` - The address to bind to (e.g., "0.0.0.0:8080")
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use turbomcp::prelude::*;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     MyServer.run_http("0.0.0.0:8080").await.unwrap();
    /// }
    /// ```
    #[cfg(feature = "http")]
    fn run_http(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send;

    /// Run the handler on HTTP transport with custom configuration.
    ///
    /// Supports rate limiting via `ServerConfig::rate_limit`.
    ///
    /// # Arguments
    /// * `addr` - The address to bind to (e.g., "0.0.0.0:8080")
    /// * `config` - Server configuration including rate limits
    #[cfg(feature = "http")]
    fn run_http_with_config(
        &self,
        addr: &str,
        config: &super::config::ServerConfig,
    ) -> impl Future<Output = McpResult<()>> + Send;

    /// Run the handler on WebSocket transport.
    ///
    /// # Arguments
    /// * `addr` - The address to bind to (e.g., "0.0.0.0:8080")
    #[cfg(feature = "websocket")]
    fn run_websocket(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send;

    /// Run the handler on WebSocket transport with custom configuration.
    ///
    /// Supports rate limiting via `ServerConfig::rate_limit`.
    ///
    /// # Arguments
    /// * `addr` - The address to bind to (e.g., "0.0.0.0:8080")
    /// * `config` - Server configuration including rate limits
    #[cfg(feature = "websocket")]
    fn run_websocket_with_config(
        &self,
        addr: &str,
        config: &super::config::ServerConfig,
    ) -> impl Future<Output = McpResult<()>> + Send;

    /// Run the handler on TCP transport.
    ///
    /// # Arguments
    /// * `addr` - The address to bind to (e.g., "0.0.0.0:9000")
    #[cfg(feature = "tcp")]
    fn run_tcp(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send;

    /// Run the handler on TCP transport with custom configuration.
    ///
    /// # Arguments
    /// * `addr` - The address to bind to (e.g., "0.0.0.0:9000")
    /// * `config` - Server configuration including connection limits
    #[cfg(feature = "tcp")]
    fn run_tcp_with_config(
        &self,
        addr: &str,
        config: &super::config::ServerConfig,
    ) -> impl Future<Output = McpResult<()>> + Send;

    /// Handle a single WASM request.
    ///
    /// This is for serverless environments like Cloudflare Workers.
    ///
    /// # Arguments
    /// * `request` - The JSON-RPC request as a JSON value
    /// * `ctx` - Request context
    ///
    /// # Returns
    /// The JSON-RPC response
    fn handle_request(
        &self,
        request: Value,
        ctx: RequestContext,
    ) -> impl Future<Output = McpResult<Value>> + Send;
}

/// Maximum message size for STDIO transport (10MB).
/// This prevents memory exhaustion from maliciously large messages.
const STDIO_MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

// Provide a blanket implementation using the router
impl<T: McpHandler> McpHandlerExt for T {
    #[cfg(feature = "stdio")]
    fn run_stdio(&self) -> impl Future<Output = McpResult<()>> + Send {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let handler = self.clone();
        async move {
            let stdin = tokio::io::stdin();
            let mut stdout = tokio::io::stdout();
            let mut reader = BufReader::new(stdin);
            let mut line = String::new();

            // Call on_initialize hook
            handler.on_initialize().await?;

            loop {
                line.clear();
                let bytes_read = reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| McpError::internal(format!("Failed to read from stdin: {}", e)))?;

                if bytes_read == 0 {
                    // EOF - clean shutdown
                    break;
                }

                // Check message size limit to prevent memory exhaustion
                if line.len() > STDIO_MAX_MESSAGE_SIZE {
                    let error_response = super::router::JsonRpcResponse::error(
                        None,
                        McpError::invalid_request(format!(
                            "Message exceeds maximum size of {} bytes",
                            STDIO_MAX_MESSAGE_SIZE
                        )),
                    );
                    let response_str = super::router::serialize_response(&error_response)?;
                    stdout.write_all(response_str.as_bytes()).await.ok();
                    stdout.write_all(b"\n").await.ok();
                    stdout.flush().await.ok();
                    continue;
                }

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                // Parse and route the request
                let ctx = RequestContext::stdio();
                match super::router::parse_request(trimmed) {
                    Ok(request) => {
                        let response = super::router::route_request(&handler, request, &ctx).await;
                        // CRITICAL-004: Only send response if it should be sent
                        // (notifications don't receive responses per JSON-RPC 2.0)
                        if response.should_send() {
                            let response_str = super::router::serialize_response(&response)?;
                            stdout
                                .write_all(response_str.as_bytes())
                                .await
                                .map_err(|e| {
                                    McpError::internal(format!("Failed to write to stdout: {}", e))
                                })?;
                            stdout.write_all(b"\n").await.map_err(|e| {
                                McpError::internal(format!("Failed to write newline: {}", e))
                            })?;
                            stdout.flush().await.map_err(|e| {
                                McpError::internal(format!("Failed to flush stdout: {}", e))
                            })?;
                        }
                    }
                    Err(e) => {
                        // Send parse error response
                        let response = super::router::JsonRpcResponse::error(None, e);
                        let response_str = super::router::serialize_response(&response)?;
                        stdout
                            .write_all(response_str.as_bytes())
                            .await
                            .map_err(|e| {
                                McpError::internal(format!("Failed to write to stdout: {}", e))
                            })?;
                        stdout.write_all(b"\n").await.map_err(|e| {
                            McpError::internal(format!("Failed to write newline: {}", e))
                        })?;
                        stdout.flush().await.map_err(|e| {
                            McpError::internal(format!("Failed to flush stdout: {}", e))
                        })?;
                    }
                }
            }

            // Call on_shutdown hook
            handler.on_shutdown().await?;
            Ok(())
        }
    }

    #[cfg(feature = "http")]
    fn run_http(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send {
        use axum::{Router, extract::DefaultBodyLimit, routing::post};
        use std::net::SocketAddr;

        let handler = self.clone();
        let addr = addr.to_string();

        async move {
            // Call on_initialize hook
            handler.on_initialize().await?;

            // Create axum app with handler state and security limits
            // DefaultBodyLimit prevents DoS via large request bodies (10MB limit)
            let app = Router::new()
                .route("/", post(handle_json_rpc::<T>))
                .route("/mcp", post(handle_json_rpc::<T>))
                .layer(DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB max body
                .with_state(handler.clone());

            // Parse address
            let socket_addr: SocketAddr = addr
                .parse()
                .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

            // Create listener
            let listener = tokio::net::TcpListener::bind(socket_addr)
                .await
                .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

            tracing::info!("v3 MCP server listening on http://{}", socket_addr);

            // Run server
            // Note: For production, consider adding tower::limit::ConcurrencyLimitLayer
            // to limit concurrent connections
            axum::serve(listener, app)
                .await
                .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

            // Call on_shutdown hook
            handler.on_shutdown().await?;
            Ok(())
        }
    }

    #[cfg(feature = "http")]
    fn run_http_with_config(
        &self,
        addr: &str,
        config: &super::config::ServerConfig,
    ) -> impl Future<Output = McpResult<()>> + Send {
        use axum::{Router, extract::DefaultBodyLimit, routing::post};
        use std::net::SocketAddr;
        use std::sync::Arc;

        let handler = self.clone();
        let addr = addr.to_string();
        let rate_limiter = config
            .rate_limit
            .as_ref()
            .map(|cfg| Arc::new(super::config::RateLimiter::new(cfg.clone())));

        async move {
            // Call on_initialize hook
            handler.on_initialize().await?;

            // Create axum app with handler state and rate limiting
            let app = if let Some(limiter) = rate_limiter {
                let state = HttpState {
                    handler: handler.clone(),
                    rate_limiter: Some(limiter),
                };
                Router::new()
                    .route("/", post(handle_json_rpc_with_rate_limit::<T>))
                    .route("/mcp", post(handle_json_rpc_with_rate_limit::<T>))
                    .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
                    .with_state(state)
            } else {
                let state = HttpState {
                    handler: handler.clone(),
                    rate_limiter: None,
                };
                Router::new()
                    .route("/", post(handle_json_rpc_with_rate_limit::<T>))
                    .route("/mcp", post(handle_json_rpc_with_rate_limit::<T>))
                    .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
                    .with_state(state)
            };

            // Parse address
            let socket_addr: SocketAddr = addr
                .parse()
                .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

            // Create listener
            let listener = tokio::net::TcpListener::bind(socket_addr)
                .await
                .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

            let rate_limit_info = config
                .rate_limit
                .as_ref()
                .map(|cfg| {
                    format!(
                        " (rate limit: {}/{}s)",
                        cfg.max_requests,
                        cfg.window.as_secs()
                    )
                })
                .unwrap_or_default();
            tracing::info!(
                "v3 MCP server listening on http://{}{}",
                socket_addr,
                rate_limit_info
            );

            // Run server with connection info for rate limiting
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

            // Call on_shutdown hook
            handler.on_shutdown().await?;
            Ok(())
        }
    }

    #[cfg(feature = "websocket")]
    fn run_websocket(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send {
        use axum::{Router, routing::get};
        use std::net::SocketAddr;

        let handler = self.clone();
        let addr = addr.to_string();

        async move {
            // Call on_initialize hook
            handler.on_initialize().await?;

            // Create axum app with WebSocket upgrade handler
            let app = Router::new()
                .route("/", get(ws_upgrade_handler::<T>))
                .route("/ws", get(ws_upgrade_handler::<T>))
                .with_state(handler.clone());

            // Parse address
            let socket_addr: SocketAddr = addr
                .parse()
                .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

            // Create listener
            let listener = tokio::net::TcpListener::bind(socket_addr)
                .await
                .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

            tracing::info!("v3 MCP WebSocket server listening on ws://{}", socket_addr);

            // Run server
            axum::serve(listener, app)
                .await
                .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

            // Call on_shutdown hook
            handler.on_shutdown().await?;
            Ok(())
        }
    }

    #[cfg(feature = "websocket")]
    fn run_websocket_with_config(
        &self,
        addr: &str,
        config: &super::config::ServerConfig,
    ) -> impl Future<Output = McpResult<()>> + Send {
        use axum::{Router, routing::get};
        use std::net::SocketAddr;
        use std::sync::Arc;

        let handler = self.clone();
        let addr = addr.to_string();
        let rate_limiter = config
            .rate_limit
            .as_ref()
            .map(|cfg| Arc::new(super::config::RateLimiter::new(cfg.clone())));

        async move {
            // Call on_initialize hook
            handler.on_initialize().await?;

            // Create axum app with WebSocket upgrade handler and rate limiting
            let state = WebSocketState {
                handler: handler.clone(),
                rate_limiter,
            };
            let app = Router::new()
                .route("/", get(ws_upgrade_handler_with_rate_limit::<T>))
                .route("/ws", get(ws_upgrade_handler_with_rate_limit::<T>))
                .with_state(state);

            // Parse address
            let socket_addr: SocketAddr = addr
                .parse()
                .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

            // Create listener
            let listener = tokio::net::TcpListener::bind(socket_addr)
                .await
                .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

            let rate_limit_info = config
                .rate_limit
                .as_ref()
                .map(|cfg| {
                    format!(
                        " (rate limit: {}/{}s)",
                        cfg.max_requests,
                        cfg.window.as_secs()
                    )
                })
                .unwrap_or_default();
            tracing::info!(
                "v3 MCP WebSocket server listening on ws://{}{}",
                socket_addr,
                rate_limit_info
            );

            // Run server with connection info for rate limiting
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

            // Call on_shutdown hook
            handler.on_shutdown().await?;
            Ok(())
        }
    }

    #[cfg(feature = "tcp")]
    fn run_tcp(&self, addr: &str) -> impl Future<Output = McpResult<()>> + Send {
        // Use default configuration - extract needed values to avoid lifetime issues
        let config = super::config::ServerConfig::default();
        let max_connections = config.connection_limits.max_tcp_connections;

        use std::sync::Arc;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpListener;

        let handler = self.clone();
        let addr = addr.to_string();

        async move {
            // Call on_initialize hook
            handler.on_initialize().await?;

            // Create connection counter for limiting concurrent connections
            let connection_counter =
                Arc::new(super::config::ConnectionCounter::new(max_connections));

            // Create TCP listener
            let listener = TcpListener::bind(&addr)
                .await
                .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

            tracing::info!(
                "v3 MCP server listening on tcp://{} (max {} connections)",
                addr,
                max_connections
            );

            loop {
                let (stream, peer_addr) = listener
                    .accept()
                    .await
                    .map_err(|e| McpError::internal(format!("Accept error: {}", e)))?;

                // Try to acquire a connection slot
                let guard = match connection_counter.try_acquire_arc() {
                    Some(guard) => guard,
                    None => {
                        tracing::warn!(
                            "Connection from {} rejected: at capacity ({}/{})",
                            peer_addr,
                            connection_counter.current(),
                            connection_counter.max()
                        );
                        // Send a "server busy" error and close connection
                        let mut stream = stream;
                        let error_response = super::router::JsonRpcResponse::error(
                            None,
                            McpError::internal("Server at maximum connection capacity"),
                        );
                        if let Ok(response_str) = super::router::serialize_response(&error_response)
                        {
                            let _ = stream.write_all(response_str.as_bytes()).await;
                            let _ = stream.write_all(b"\n").await;
                            let _ = stream.flush().await;
                        }
                        continue;
                    }
                };

                tracing::debug!(
                    "New TCP connection from {} ({}/{})",
                    peer_addr,
                    connection_counter.current(),
                    connection_counter.max()
                );

                let handler = handler.clone();
                tokio::spawn(async move {
                    // Guard is moved into the task and will be dropped when task ends,
                    // releasing the connection slot
                    let _guard = guard;

                    let (reader, mut writer) = stream.into_split();
                    let mut reader = BufReader::new(reader);
                    let mut line = String::new();

                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                let trimmed = line.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }

                                let ctx = RequestContext::tcp();
                                match super::router::parse_request(trimmed) {
                                    Ok(request) => {
                                        let response =
                                            super::router::route_request(&handler, request, &ctx)
                                                .await;
                                        // CRITICAL-004: Only send response if it should be sent
                                        if response.should_send() {
                                            if let Ok(response_str) =
                                                super::router::serialize_response(&response)
                                            {
                                                let _ =
                                                    writer.write_all(response_str.as_bytes()).await;
                                                let _ = writer.write_all(b"\n").await;
                                                let _ = writer.flush().await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let error = super::router::JsonRpcResponse::error(
                                            None,
                                            McpError::parse_error(e.to_string()),
                                        );
                                        if let Ok(error_str) =
                                            super::router::serialize_response(&error)
                                        {
                                            let _ = writer.write_all(error_str.as_bytes()).await;
                                            let _ = writer.write_all(b"\n").await;
                                            let _ = writer.flush().await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Read error from {}: {}", peer_addr, e);
                                break;
                            }
                        }
                    }

                    tracing::debug!("TCP connection from {} closed", peer_addr);
                });
            }
        }
    }

    #[cfg(feature = "tcp")]
    fn run_tcp_with_config(
        &self,
        addr: &str,
        config: &super::config::ServerConfig,
    ) -> impl Future<Output = McpResult<()>> + Send {
        use std::sync::Arc;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpListener;

        let handler = self.clone();
        let addr = addr.to_string();
        let max_connections = config.connection_limits.max_tcp_connections;

        async move {
            // Call on_initialize hook
            handler.on_initialize().await?;

            // Create connection counter for limiting concurrent connections
            let connection_counter =
                Arc::new(super::config::ConnectionCounter::new(max_connections));

            // Create TCP listener
            let listener = TcpListener::bind(&addr)
                .await
                .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

            tracing::info!(
                "v3 MCP server listening on tcp://{} (max {} connections)",
                addr,
                max_connections
            );

            loop {
                let (stream, peer_addr) = listener
                    .accept()
                    .await
                    .map_err(|e| McpError::internal(format!("Accept error: {}", e)))?;

                // Try to acquire a connection slot
                let guard = match connection_counter.try_acquire_arc() {
                    Some(guard) => guard,
                    None => {
                        tracing::warn!(
                            "Connection from {} rejected: at capacity ({}/{})",
                            peer_addr,
                            connection_counter.current(),
                            connection_counter.max()
                        );
                        // Send a "server busy" error and close connection
                        let mut stream = stream;
                        let error_response = super::router::JsonRpcResponse::error(
                            None,
                            McpError::internal("Server at maximum connection capacity"),
                        );
                        if let Ok(response_str) = super::router::serialize_response(&error_response)
                        {
                            use tokio::io::AsyncWriteExt;
                            let _ = stream.write_all(response_str.as_bytes()).await;
                            let _ = stream.write_all(b"\n").await;
                            let _ = stream.flush().await;
                        }
                        continue;
                    }
                };

                tracing::debug!(
                    "New TCP connection from {} ({}/{})",
                    peer_addr,
                    connection_counter.current(),
                    connection_counter.max()
                );

                let handler = handler.clone();
                tokio::spawn(async move {
                    // Guard is moved into the task and will be dropped when task ends,
                    // releasing the connection slot
                    let _guard = guard;

                    let (reader, mut writer) = stream.into_split();
                    let mut reader = BufReader::new(reader);
                    let mut line = String::new();

                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) => break, // EOF
                            Ok(_) => {
                                let trimmed = line.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }

                                let ctx = RequestContext::tcp();
                                match super::router::parse_request(trimmed) {
                                    Ok(request) => {
                                        let response =
                                            super::router::route_request(&handler, request, &ctx)
                                                .await;
                                        // CRITICAL-004: Only send response if it should be sent
                                        if response.should_send() {
                                            if let Ok(response_str) =
                                                super::router::serialize_response(&response)
                                            {
                                                let _ =
                                                    writer.write_all(response_str.as_bytes()).await;
                                                let _ = writer.write_all(b"\n").await;
                                                let _ = writer.flush().await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let response =
                                            super::router::JsonRpcResponse::error(None, e);
                                        if let Ok(response_str) =
                                            super::router::serialize_response(&response)
                                        {
                                            let _ = writer.write_all(response_str.as_bytes()).await;
                                            let _ = writer.write_all(b"\n").await;
                                            let _ = writer.flush().await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("Read error from {}: {}", peer_addr, e);
                                break;
                            }
                        }
                    }

                    tracing::debug!("TCP connection from {} closed", peer_addr);
                });
            }
        }
    }

    fn handle_request(
        &self,
        request: Value,
        ctx: RequestContext,
    ) -> impl Future<Output = McpResult<Value>> + Send {
        let handler = self.clone();
        async move {
            // Parse the request from Value
            let json_rpc_request: super::router::JsonRpcRequest =
                serde_json::from_value(request)
                    .map_err(|e| McpError::parse_error(e.to_string()))?;

            // Route the request
            let response = super::router::route_request(&handler, json_rpc_request, &ctx).await;

            // Convert response to Value
            serde_json::to_value(&response).map_err(|e| McpError::internal(e.to_string()))
        }
    }
}

/// Axum handler for JSON-RPC requests over HTTP.
#[cfg(feature = "http")]
async fn handle_json_rpc<H: McpHandler>(
    axum::extract::State(handler): axum::extract::State<H>,
    axum::Json(request): axum::Json<super::router::JsonRpcRequest>,
) -> axum::Json<super::router::JsonRpcResponse> {
    let ctx = RequestContext::http();
    let response = super::router::route_request(&handler, request, &ctx).await;
    axum::Json(response)
}

/// HTTP state with rate limiting support (HIGH-004).
#[cfg(feature = "http")]
#[derive(Clone)]
struct HttpState<H: McpHandler> {
    handler: H,
    rate_limiter: Option<std::sync::Arc<super::config::RateLimiter>>,
}

/// Axum handler for JSON-RPC requests with rate limiting (HIGH-004).
#[cfg(feature = "http")]
async fn handle_json_rpc_with_rate_limit<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<HttpState<H>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::Json(request): axum::Json<super::router::JsonRpcRequest>,
) -> Result<axum::Json<super::router::JsonRpcResponse>, axum::http::StatusCode> {
    // Check rate limit if configured
    if let Some(ref limiter) = state.rate_limiter {
        let client_id = addr.ip().to_string();
        if !limiter.check(Some(&client_id)) {
            tracing::warn!("Rate limit exceeded for client {}", client_id);
            return Err(axum::http::StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let ctx = RequestContext::http();
    let response = super::router::route_request(&state.handler, request, &ctx).await;
    Ok(axum::Json(response))
}

/// Axum handler for WebSocket upgrade requests.
#[cfg(feature = "websocket")]
async fn ws_upgrade_handler<H: McpHandler>(
    ws: axum::extract::ws::WebSocketUpgrade,
    axum::extract::State(handler): axum::extract::State<H>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_websocket(socket, handler))
}

/// WebSocket state with rate limiting support (HIGH-004).
#[cfg(feature = "websocket")]
#[derive(Clone)]
struct WebSocketState<H: McpHandler> {
    handler: H,
    rate_limiter: Option<std::sync::Arc<super::config::RateLimiter>>,
}

/// Axum handler for WebSocket upgrade requests with rate limiting (HIGH-004).
#[cfg(feature = "websocket")]
async fn ws_upgrade_handler_with_rate_limit<H: McpHandler>(
    ws: axum::extract::ws::WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<WebSocketState<H>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    // Check rate limit on connection (not per message)
    if let Some(ref limiter) = state.rate_limiter {
        let client_id = addr.ip().to_string();
        if !limiter.check(Some(&client_id)) {
            tracing::warn!("Rate limit exceeded for WebSocket client {}", client_id);
            return Err(axum::http::StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let handler = state.handler.clone();
    let rate_limiter = state.rate_limiter.clone();
    let client_addr = addr;

    Ok(ws.on_upgrade(move |socket| {
        handle_websocket_with_rate_limit(socket, handler, rate_limiter, client_addr)
    }))
}

/// Handle a WebSocket connection with per-message rate limiting.
#[cfg(feature = "websocket")]
async fn handle_websocket_with_rate_limit<H: McpHandler>(
    socket: axum::extract::ws::WebSocket,
    handler: H,
    rate_limiter: Option<std::sync::Arc<super::config::RateLimiter>>,
    client_addr: std::net::SocketAddr,
) {
    use axum::extract::ws::Message;
    use futures::SinkExt;
    use futures::stream::StreamExt;

    let client_id = client_addr.ip().to_string();
    let (mut sender, mut receiver) = socket.split();

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!("WebSocket receive error: {}", e);
                break;
            }
        };

        // Only process text messages (JSON-RPC)
        let text = match msg {
            Message::Text(text) => {
                // Check message size limit
                if text.len() > WEBSOCKET_MAX_MESSAGE_SIZE {
                    tracing::warn!(
                        "WebSocket message exceeds size limit ({} > {})",
                        text.len(),
                        WEBSOCKET_MAX_MESSAGE_SIZE
                    );
                    continue;
                }
                text.to_string()
            }
            Message::Binary(data) => {
                // Try to interpret binary as UTF-8 text for JSON-RPC
                match String::from_utf8(data.to_vec()) {
                    Ok(text) if text.len() <= WEBSOCKET_MAX_MESSAGE_SIZE => text,
                    _ => {
                        tracing::warn!(
                            "Received binary WebSocket frame that isn't valid UTF-8 or exceeds size limit"
                        );
                        continue;
                    }
                }
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(_) => break,
        };

        // Check rate limit per message
        if let Some(ref limiter) = rate_limiter {
            if !limiter.check(Some(&client_id)) {
                tracing::warn!(
                    "Rate limit exceeded for WebSocket message from {}",
                    client_id
                );
                // Send rate limit error response
                let error_response = super::router::JsonRpcResponse::error(
                    None,
                    McpError::new(-32000, "Rate limit exceeded"),
                );
                if let Ok(response_str) = super::router::serialize_response(&error_response) {
                    let _ = sender.send(Message::Text(response_str.into())).await;
                }
                continue;
            }
        }

        let ctx = RequestContext::websocket();
        match super::router::parse_request(&text) {
            Ok(request) => {
                let response = super::router::route_request(&handler, request, &ctx).await;
                if response.should_send() {
                    if let Ok(response_str) = super::router::serialize_response(&response) {
                        if sender
                            .send(Message::Text(response_str.into()))
                            .await
                            .is_err()
                        {
                            tracing::error!("Failed to send WebSocket response");
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                let error = super::router::JsonRpcResponse::error(
                    None,
                    McpError::parse_error(e.to_string()),
                );
                if let Ok(error_str) = super::router::serialize_response(&error) {
                    let _ = sender.send(Message::Text(error_str.into())).await;
                }
            }
        }
    }
}

/// Maximum WebSocket message size (10MB).
/// This prevents memory exhaustion from large frames.
#[cfg(feature = "websocket")]
const WEBSOCKET_MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Handle a WebSocket connection.
///
/// # Security Considerations
/// - Message size is limited to 10MB to prevent DoS
/// - Binary frames are logged if they fail UTF-8 conversion
/// - Ping/pong is handled automatically
///
/// # Note
/// For production, consider adding a ping timeout mechanism to detect
/// zombie connections. This can be done by wrapping the receive loop
/// with `tokio::time::timeout`.
#[cfg(feature = "websocket")]
async fn handle_websocket<H: McpHandler>(socket: axum::extract::ws::WebSocket, handler: H) {
    use axum::extract::ws::Message;
    use futures::SinkExt;
    use futures::stream::StreamExt;

    let (mut sender, mut receiver) = socket.split();

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!("WebSocket receive error: {}", e);
                break;
            }
        };

        // Only process text messages (JSON-RPC)
        let text = match msg {
            Message::Text(text) => {
                // Check message size limit
                if text.len() > WEBSOCKET_MAX_MESSAGE_SIZE {
                    tracing::warn!(
                        "WebSocket message exceeds size limit ({} > {})",
                        text.len(),
                        WEBSOCKET_MAX_MESSAGE_SIZE
                    );
                    let error_response = super::router::JsonRpcResponse::error(
                        None,
                        McpError::invalid_request("Message exceeds maximum size"),
                    );
                    if let Ok(response_str) = super::router::serialize_response(&error_response) {
                        let _ = sender.send(Message::Text(response_str.into())).await;
                    }
                    continue;
                }
                text
            }
            Message::Binary(data) => {
                // Check message size limit
                if data.len() > WEBSOCKET_MAX_MESSAGE_SIZE {
                    tracing::warn!(
                        "WebSocket binary message exceeds size limit ({} > {})",
                        data.len(),
                        WEBSOCKET_MAX_MESSAGE_SIZE
                    );
                    continue;
                }
                // Try to interpret binary as UTF-8 text
                match String::from_utf8(data.to_vec()) {
                    Ok(text) => text.into(),
                    Err(e) => {
                        tracing::debug!(
                            "WebSocket binary frame is not valid UTF-8 (len={}): {}",
                            data.len(),
                            e
                        );
                        continue;
                    }
                }
            }
            Message::Ping(data) => {
                // Respond to ping with pong
                if sender.send(Message::Pong(data)).await.is_err() {
                    break;
                }
                continue;
            }
            Message::Pong(_) => continue,
            Message::Close(_) => break,
        };

        // Parse and route the request
        let ctx = RequestContext::websocket();
        let response = match super::router::parse_request(&text) {
            Ok(request) => super::router::route_request(&handler, request, &ctx).await,
            Err(e) => super::router::JsonRpcResponse::error(None, e),
        };

        // CRITICAL-004: Only send response if it should be sent
        if response.should_send() {
            match super::router::serialize_response(&response) {
                Ok(response_str) => {
                    if sender
                        .send(Message::Text(response_str.into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to serialize WebSocket response: {}", e);
                    break;
                }
            }
        }
    }

    tracing::debug!("WebSocket connection closed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestHandler;

    impl McpHandler for TestHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("test-handler", "1.0.0").with_description("A test handler")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![
                Tool::new("add", "Add two numbers"),
                Tool::new("greet", "Say hello"),
            ]
        }

        fn list_resources(&self) -> Vec<Resource> {
            vec![
                Resource::new("config://app", "app-config").with_mime_type("application/json"), // HIGH-001: mimeType support
            ]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![
                Prompt::new("greeting", "A friendly greeting")
                    .with_required_arg("name", "Name to greet") // HIGH-002: prompt arguments
                    .with_optional_arg("style", "Greeting style"),
            ]
        }

        fn call_tool(
            &self,
            name: &str,
            args: Value,
            _ctx: &RequestContext,
        ) -> impl Future<Output = McpResult<ToolResult>> + Send {
            let name = name.to_string();
            async move {
                match name.as_str() {
                    "add" => {
                        let a = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                        let b = args.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                        Ok(ToolResult::text(format!("{}", a + b)))
                    }
                    "greet" => {
                        let who = args.get("name").and_then(|v| v.as_str()).unwrap_or("World");
                        Ok(ToolResult::text(format!("Hello, {}!", who)))
                    }
                    _ => Err(McpError::tool_not_found(&name)),
                }
            }
        }

        fn read_resource(
            &self,
            uri: &str,
            _ctx: &RequestContext,
        ) -> impl Future<Output = McpResult<ResourceResult>> + Send {
            let uri = uri.to_string();
            async move {
                if uri == "config://app" {
                    Ok(ResourceResult::json(
                        &uri,
                        &serde_json::json!({
                            "debug": true,
                            "version": "1.0.0"
                        }),
                    )?)
                } else {
                    Err(McpError::resource_not_found(&uri))
                }
            }
        }

        fn get_prompt(
            &self,
            name: &str,
            _args: Option<Value>,
            _ctx: &RequestContext,
        ) -> impl Future<Output = McpResult<PromptResult>> + Send {
            let name = name.to_string();
            async move {
                if name == "greeting" {
                    Ok(PromptResult::user("Hello! How can I help you today?"))
                } else {
                    Err(McpError::prompt_not_found(&name))
                }
            }
        }
    }

    #[test]
    fn test_server_info() {
        let handler = TestHandler;
        let info = handler.server_info();
        assert_eq!(info.name, "test-handler");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.description, Some("A test handler".into()));
    }

    #[test]
    fn test_list_tools() {
        let handler = TestHandler;
        let tools = handler.list_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "add");
        assert_eq!(tools[1].name, "greet");
    }

    #[test]
    fn test_list_resources() {
        let handler = TestHandler;
        let resources = handler.list_resources();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "config://app");
    }

    #[test]
    fn test_list_prompts() {
        let handler = TestHandler;
        let prompts = handler.list_prompts();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].name, "greeting");
    }

    #[tokio::test]
    async fn test_call_tool_add() {
        let handler = TestHandler;
        let ctx = RequestContext::new();
        let args = serde_json::json!({"a": 2, "b": 3});

        let result = handler.call_tool("add", args, &ctx).await.unwrap();
        assert_eq!(result.first_text(), Some("5"));
    }

    #[tokio::test]
    async fn test_call_tool_greet() {
        let handler = TestHandler;
        let ctx = RequestContext::new();
        let args = serde_json::json!({"name": "Alice"});

        let result = handler.call_tool("greet", args, &ctx).await.unwrap();
        assert_eq!(result.first_text(), Some("Hello, Alice!"));
    }

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let handler = TestHandler;
        let ctx = RequestContext::new();
        let args = serde_json::json!({});

        let result = handler.call_tool("unknown", args, &ctx).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        // CRITICAL-003: Uses INTERNAL_ERROR per MCP spec for "not found" within valid method
        assert_eq!(err.code, McpError::INTERNAL_ERROR);
    }

    #[tokio::test]
    async fn test_read_resource() {
        let handler = TestHandler;
        let ctx = RequestContext::new();

        let result = handler.read_resource("config://app", &ctx).await.unwrap();
        let text = result.first_text().unwrap();
        assert!(text.contains("debug"));
        assert!(text.contains("true"));
    }

    #[tokio::test]
    async fn test_read_resource_not_found() {
        let handler = TestHandler;
        let ctx = RequestContext::new();

        let result = handler.read_resource("config://unknown", &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_prompt() {
        let handler = TestHandler;
        let ctx = RequestContext::new();

        let result = handler.get_prompt("greeting", None, &ctx).await.unwrap();
        assert!(!result.is_empty());
    }

    #[tokio::test]
    async fn test_get_prompt_not_found() {
        let handler = TestHandler;
        let ctx = RequestContext::new();

        let result = handler.get_prompt("unknown", None, &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_handle_request_initialize() {
        let handler = TestHandler;
        let ctx = RequestContext::new();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                },
                "capabilities": {}
            }
        });

        let response = handler.handle_request(request, ctx).await.unwrap();
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 1);
        assert!(response["result"].is_object());
        assert_eq!(response["result"]["serverInfo"]["name"], "test-handler");
        // Verify MCP-compliant capability structure
        assert!(
            response["result"]["capabilities"]["tools"]["listChanged"]
                .as_bool()
                .unwrap_or(false)
        );
    }

    #[tokio::test]
    async fn test_handle_request_tools_call() {
        let handler = TestHandler;
        let ctx = RequestContext::new();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "add",
                "arguments": {"a": 10, "b": 20}
            }
        });

        let response = handler.handle_request(request, ctx).await.unwrap();
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 2);
        assert!(response.get("error").is_none());
        // The result contains a ToolResult with content
        let result = &response["result"];
        assert!(result["content"].is_array());
    }

    #[tokio::test]
    async fn test_handle_request_ping() {
        let handler = TestHandler;
        let ctx = RequestContext::new();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "ping"
        });

        let response = handler.handle_request(request, ctx).await.unwrap();
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["id"], 3);
        assert!(response.get("error").is_none());
    }

    // HIGH-001: Verify resources include mimeType
    #[test]
    fn test_resource_mimetype() {
        let handler = TestHandler;
        let resources = handler.list_resources();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].uri, "config://app");
        assert_eq!(resources[0].mime_type, Some("application/json".into()));
    }

    // HIGH-002: Verify prompts include arguments
    #[test]
    fn test_prompt_arguments() {
        let handler = TestHandler;
        let prompts = handler.list_prompts();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].name, "greeting");

        let args = prompts[0]
            .arguments
            .as_ref()
            .expect("should have arguments");
        assert_eq!(args.len(), 2);

        // First argument is required
        assert_eq!(args[0].name, "name");
        assert_eq!(args[0].description, Some("Name to greet".into()));
        assert_eq!(args[0].required, Some(true));

        // Second argument is optional
        assert_eq!(args[1].name, "style");
        assert_eq!(args[1].description, Some("Greeting style".into()));
        assert_eq!(args[1].required, Some(false));
    }
}

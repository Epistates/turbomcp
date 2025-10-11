//! Core Client implementation for MCP communication
//!
//! This module contains the main `Client<T>` struct and its implementation,
//! providing the core MCP client functionality including:
//!
//! - Connection initialization and lifecycle management
//! - Message processing and bidirectional communication
//! - MCP operation support (tools, prompts, resources, sampling, etc.)
//! - Plugin middleware integration
//! - Handler registration and management
//!
//! # Architecture
//!
//! `Client<T>` is implemented as a cheaply-cloneable Arc wrapper with interior
//! mutability (same pattern as reqwest and AWS SDK):
//!
//! - **AtomicBool** for initialized flag (lock-free)
//! - **Arc<Mutex<...>>** for handlers/plugins (infrequent mutation)
//! - **`Arc<ClientInner<T>>`** for cheap cloning

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use turbomcp_protocol::jsonrpc::*;
use turbomcp_protocol::types::{
    ClientCapabilities as ProtocolClientCapabilities, InitializeResult as ProtocolInitializeResult,
    *,
};
use turbomcp_protocol::{Error, PROTOCOL_VERSION, Result};
use turbomcp_transport::{Transport, TransportMessage};

use super::config::InitializeResult;
use super::protocol::ProtocolClient;
use crate::{ClientCapabilities, handlers::HandlerRegistry, sampling::SamplingHandler};

/// Inner client state with interior mutability
///
/// This structure contains the actual client state and is wrapped in Arc<...>
/// to enable cheap cloning (same pattern as reqwest and AWS SDK).
pub(super) struct ClientInner<T: Transport + 'static> {
    /// Protocol client for low-level communication
    pub(super) protocol: ProtocolClient<T>,

    /// Client capabilities (immutable after construction)
    pub(super) capabilities: ClientCapabilities,

    /// Initialization state (lock-free atomic boolean)
    pub(super) initialized: AtomicBool,

    /// Optional sampling handler (mutex for dynamic updates)
    pub(super) sampling_handler: Arc<StdMutex<Option<Arc<dyn SamplingHandler>>>>,

    /// Handler registry for bidirectional communication (mutex for registration)
    pub(super) handlers: Arc<StdMutex<HandlerRegistry>>,

    /// Plugin registry for middleware (tokio mutex - holds across await)
    pub(super) plugin_registry: Arc<tokio::sync::Mutex<crate::plugins::PluginRegistry>>,
}

/// The core MCP client implementation
///
/// Client provides a comprehensive interface for communicating with MCP servers,
/// supporting all protocol features including tools, prompts, resources, sampling,
/// elicitation, and bidirectional communication patterns.
///
/// # Clone Pattern
///
/// `Client<T>` is cheaply cloneable via Arc (same pattern as reqwest and AWS SDK).
/// All clones share the same underlying connection and state:
///
/// ```rust,no_run
/// use turbomcp_client::Client;
/// use turbomcp_transport::stdio::StdioTransport;
///
/// # async fn example() -> turbomcp_protocol::Result<()> {
/// let client = Client::new(StdioTransport::new());
/// client.initialize().await?;
///
/// // Cheap clone - shares same connection
/// let client2 = client.clone();
/// tokio::spawn(async move {
///     client2.list_tools().await.ok();
/// });
/// # Ok(())
/// # }
/// ```
///
/// The client must be initialized before use by calling `initialize()` to perform
/// the MCP handshake and capability negotiation.
///
/// # Features
///
/// - **Protocol Compliance**: Full MCP 2025-06-18 specification support
/// - **Bidirectional Communication**: Server-initiated requests and client responses
/// - **Plugin Middleware**: Extensible request/response processing
/// - **Handler Registry**: Callbacks for server-initiated operations
/// - **Connection Management**: Robust error handling and recovery
/// - **Type Safety**: Compile-time guarantees for MCP message types
/// - **Cheap Cloning**: Arc-based sharing like reqwest/AWS SDK
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::Client;
/// use turbomcp_transport::stdio::StdioTransport;
/// use std::collections::HashMap;
///
/// # async fn example() -> turbomcp_protocol::Result<()> {
/// // Create and initialize client (no mut needed!)
/// let client = Client::new(StdioTransport::new());
/// let init_result = client.initialize().await?;
/// println!("Connected to: {}", init_result.server_info.name);
///
/// // Use MCP operations
/// let tools = client.list_tools().await?;
/// let mut args = HashMap::new();
/// args.insert("input".to_string(), serde_json::json!("test"));
/// let result = client.call_tool("my_tool", Some(args)).await?;
/// # Ok(())
/// # }
/// ```
pub struct Client<T: Transport + 'static> {
    pub(super) inner: Arc<ClientInner<T>>,
}

/// Clone implementation via Arc (same pattern as reqwest/AWS SDK)
///
/// Cloning a Client is cheap (just an Arc clone) and all clones share
/// the same underlying connection and state.
impl<T: Transport + 'static> Clone for Client<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: Transport + 'static> Drop for ClientInner<T> {
    fn drop(&mut self) {
        // Shutdown the dispatcher's background task when the LAST Client reference is dropped
        // This prevents the background task from running forever after all clients are dropped
        tracing::debug!("Last Client reference dropped - shutting down message dispatcher");
        self.protocol.dispatcher().shutdown();
    }
}

impl<T: Transport + 'static> Client<T> {
    /// Create a new client with the specified transport
    ///
    /// Creates a new MCP client instance with default capabilities.
    /// The client must be initialized before use by calling `initialize()`.
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport implementation to use for communication
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_transport::stdio::StdioTransport;
    ///
    /// let transport = StdioTransport::new();
    /// let client = Client::new(transport);
    /// ```
    pub fn new(transport: T) -> Self {
        let client = Self {
            inner: Arc::new(ClientInner {
                protocol: ProtocolClient::new(transport),
                capabilities: ClientCapabilities::default(),
                initialized: AtomicBool::new(false),
                sampling_handler: Arc::new(StdMutex::new(None)),
                handlers: Arc::new(StdMutex::new(HandlerRegistry::new())),
                plugin_registry: Arc::new(tokio::sync::Mutex::new(
                    crate::plugins::PluginRegistry::new(),
                )),
            }),
        };

        // Register dispatcher handlers for bidirectional communication
        client.register_dispatcher_handlers();

        client
    }

    /// Create a new client with custom capabilities
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport implementation to use
    /// * `capabilities` - The client capabilities to negotiate
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::{Client, ClientCapabilities};
    /// use turbomcp_transport::stdio::StdioTransport;
    ///
    /// let capabilities = ClientCapabilities {
    ///     tools: true,
    ///     prompts: true,
    ///     resources: false,
    ///     sampling: false,
    /// };
    ///
    /// let transport = StdioTransport::new();
    /// let client = Client::with_capabilities(transport, capabilities);
    /// ```
    pub fn with_capabilities(transport: T, capabilities: ClientCapabilities) -> Self {
        let client = Self {
            inner: Arc::new(ClientInner {
                protocol: ProtocolClient::new(transport),
                capabilities,
                initialized: AtomicBool::new(false),
                sampling_handler: Arc::new(StdMutex::new(None)),
                handlers: Arc::new(StdMutex::new(HandlerRegistry::new())),
                plugin_registry: Arc::new(tokio::sync::Mutex::new(
                    crate::plugins::PluginRegistry::new(),
                )),
            }),
        };

        // Register dispatcher handlers for bidirectional communication
        client.register_dispatcher_handlers();

        client
    }
}

// ============================================================================
// HTTP-Specific Convenience Constructors (Feature-Gated)
// ============================================================================

#[cfg(feature = "http")]
impl Client<turbomcp_transport::streamable_http_client::StreamableHttpClientTransport> {
    /// Connect to an HTTP MCP server (convenience method)
    ///
    /// This is a beautiful one-liner alternative to manual configuration.
    /// Creates an HTTP client, connects, and initializes in one call.
    ///
    /// # Arguments
    ///
    /// * `url` - The base URL of the MCP server (e.g., "http://localhost:8080")
    ///
    /// # Returns
    ///
    /// Returns an initialized `Client` ready to use.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The URL is invalid
    /// - Connection to the server fails
    /// - Initialization handshake fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    ///
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// // Beautiful one-liner - balanced with server DX
    /// let client = Client::connect_http("http://localhost:8080").await?;
    ///
    /// // Now use it directly
    /// let tools = client.list_tools().await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Compare to verbose approach (10+ lines):
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_transport::streamable_http_client::{
    ///     StreamableHttpClientConfig, StreamableHttpClientTransport
    /// };
    ///
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let config = StreamableHttpClientConfig {
    ///     base_url: "http://localhost:8080".to_string(),
    ///     ..Default::default()
    /// };
    /// let transport = StreamableHttpClientTransport::new(config);
    /// let client = Client::new(transport);
    /// client.initialize().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect_http(url: impl Into<String>) -> Result<Self> {
        use turbomcp_transport::streamable_http_client::{
            StreamableHttpClientConfig, StreamableHttpClientTransport,
        };

        let config = StreamableHttpClientConfig {
            base_url: url.into(),
            ..Default::default()
        };

        let transport = StreamableHttpClientTransport::new(config);
        let client = Self::new(transport);

        // Initialize connection immediately
        client.initialize().await?;

        Ok(client)
    }

    /// Connect to an HTTP MCP server with custom configuration
    ///
    /// Provides more control than `connect_http()` while still being ergonomic.
    ///
    /// # Arguments
    ///
    /// * `url` - The base URL of the MCP server
    /// * `config_fn` - Function to customize the configuration
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use std::time::Duration;
    ///
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let client = Client::connect_http_with("http://localhost:8080", |config| {
    ///     config.timeout = Duration::from_secs(60);
    ///     config.endpoint_path = "/api/mcp".to_string();
    /// }).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect_http_with<F>(url: impl Into<String>, config_fn: F) -> Result<Self>
    where
        F: FnOnce(&mut turbomcp_transport::streamable_http_client::StreamableHttpClientConfig),
    {
        use turbomcp_transport::streamable_http_client::{
            StreamableHttpClientConfig, StreamableHttpClientTransport,
        };

        let mut config = StreamableHttpClientConfig {
            base_url: url.into(),
            ..Default::default()
        };

        config_fn(&mut config);

        let transport = StreamableHttpClientTransport::new(config);
        let client = Self::new(transport);

        client.initialize().await?;

        Ok(client)
    }
}

// ============================================================================
// TCP-Specific Convenience Constructors (Feature-Gated)
// ============================================================================

#[cfg(feature = "tcp")]
impl Client<turbomcp_transport::tcp::TcpTransport> {
    /// Connect to a TCP MCP server (convenience method)
    ///
    /// Beautiful one-liner for TCP connections - balanced DX.
    ///
    /// # Arguments
    ///
    /// * `addr` - Server address (e.g., "127.0.0.1:8765" or localhost:8765")
    ///
    /// # Returns
    ///
    /// Returns an initialized `Client` ready to use.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # #[cfg(feature = "tcp")]
    /// use turbomcp_client::Client;
    ///
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let client = Client::connect_tcp("127.0.0.1:8765").await?;
    /// let tools = client.list_tools().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect_tcp(addr: impl AsRef<str>) -> Result<Self> {
        use std::net::SocketAddr;
        use turbomcp_transport::tcp::TcpTransport;

        let server_addr: SocketAddr = addr
            .as_ref()
            .parse()
            .map_err(|e| Error::bad_request(format!("Invalid address: {}", e)))?;

        // Client binds to 0.0.0.0:0 (any available port)
        let bind_addr: SocketAddr = if server_addr.is_ipv6() {
            "[::]:0".parse().unwrap()
        } else {
            "0.0.0.0:0".parse().unwrap()
        };

        let transport = TcpTransport::new_client(bind_addr, server_addr);
        let client = Self::new(transport);

        client.initialize().await?;

        Ok(client)
    }
}

// ============================================================================
// Unix Socket-Specific Convenience Constructors (Feature-Gated)
// ============================================================================

#[cfg(all(unix, feature = "unix"))]
impl Client<turbomcp_transport::unix::UnixTransport> {
    /// Connect to a Unix socket MCP server (convenience method)
    ///
    /// Beautiful one-liner for Unix socket IPC - balanced DX.
    ///
    /// # Arguments
    ///
    /// * `path` - Socket file path (e.g., "/tmp/mcp.sock")
    ///
    /// # Returns
    ///
    /// Returns an initialized `Client` ready to use.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # #[cfg(all(unix, feature = "unix"))]
    /// use turbomcp_client::Client;
    ///
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let client = Client::connect_unix("/tmp/mcp.sock").await?;
    /// let tools = client.list_tools().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn connect_unix(path: impl Into<std::path::PathBuf>) -> Result<Self> {
        use turbomcp_transport::unix::UnixTransport;

        let transport = UnixTransport::new_client(path.into());
        let client = Self::new(transport);

        client.initialize().await?;

        Ok(client)
    }
}

impl<T: Transport + 'static> Client<T> {
    /// Register message handlers with the dispatcher
    ///
    /// This method sets up the callbacks that handle server-initiated requests
    /// and notifications. The dispatcher's background task routes incoming
    /// messages to these handlers.
    ///
    /// This is called automatically during Client construction (in `new()` and
    /// `with_capabilities()`), so you don't need to call it manually.
    ///
    /// ## How It Works
    ///
    /// The handlers are synchronous closures that spawn async tasks to do the
    /// actual work. This allows the dispatcher to continue routing messages
    /// without blocking on handler execution.
    fn register_dispatcher_handlers(&self) {
        let dispatcher = self.inner.protocol.dispatcher();
        let client_for_requests = self.clone();
        let client_for_notifications = self.clone();

        // Request handler (elicitation, sampling, etc.)
        let request_handler = Arc::new(move |request: JsonRpcRequest| {
            let client = client_for_requests.clone();
            // Spawn async task to handle the request
            tokio::spawn(async move {
                if let Err(e) = client.handle_request(request).await {
                    tracing::error!("Error handling server request: {}", e);
                }
            });
            Ok(())
        });

        // Notification handler
        let notification_handler = Arc::new(move |notification: JsonRpcNotification| {
            let client = client_for_notifications.clone();
            // Spawn async task to handle the notification
            tokio::spawn(async move {
                if let Err(e) = client.handle_notification(notification).await {
                    tracing::error!("Error handling server notification: {}", e);
                }
            });
            Ok(())
        });

        // Register handlers synchronously - no race condition!
        // The set_* methods are now synchronous with std::sync::Mutex
        dispatcher.set_request_handler(request_handler);
        dispatcher.set_notification_handler(notification_handler);
        tracing::debug!("Dispatcher handlers registered successfully");
    }

    /// Handle server-initiated requests (elicitation, sampling, roots)
    ///
    /// This method is called by the MessageDispatcher when it receives a request
    /// from the server. It routes the request to the appropriate handler based on
    /// the method name.
    async fn handle_request(&self, request: JsonRpcRequest) -> Result<()> {
        match request.method.as_str() {
            "sampling/createMessage" => {
                let handler_opt = self
                    .inner
                    .sampling_handler
                    .lock()
                    .expect("sampling_handler mutex poisoned")
                    .clone();
                if let Some(handler) = handler_opt {
                    let params: CreateMessageRequest =
                        serde_json::from_value(request.params.unwrap_or(serde_json::Value::Null))
                            .map_err(|e| {
                            Error::protocol(format!("Invalid createMessage params: {}", e))
                        })?;

                    match handler.handle_create_message(params).await {
                        Ok(result) => {
                            let result_value = serde_json::to_value(result).map_err(|e| {
                                Error::protocol(format!("Failed to serialize response: {}", e))
                            })?;
                            let response = JsonRpcResponse::success(result_value, request.id);
                            self.send_response(response).await?;
                        }
                        Err(e) => {
                            let error = turbomcp_protocol::jsonrpc::JsonRpcError {
                                code: -32603,
                                message: format!("Sampling handler error: {}", e),
                                data: None,
                            };
                            let response = JsonRpcResponse::error_response(error, request.id);
                            self.send_response(response).await?;
                        }
                    }
                } else {
                    // No handler configured
                    let error = turbomcp_protocol::jsonrpc::JsonRpcError {
                        code: -32601,
                        message: "Sampling not supported".to_string(),
                        data: None,
                    };
                    let response = JsonRpcResponse::error_response(error, request.id);
                    self.send_response(response).await?;
                }
            }
            "roots/list" => {
                // Handle roots/list request from server
                // Clone the handler Arc to avoid holding mutex across await
                let handler_opt = self
                    .inner
                    .handlers
                    .lock()
                    .expect("handlers mutex poisoned")
                    .roots
                    .clone();

                let roots_result = if let Some(handler) = handler_opt {
                    handler.handle_roots_request().await
                } else {
                    // No handler - return empty list per MCP spec
                    Ok(Vec::new())
                };

                match roots_result {
                    Ok(roots) => {
                        let result_value =
                            serde_json::to_value(turbomcp_protocol::types::ListRootsResult {
                                roots,
                                _meta: None,
                            })
                            .map_err(|e| {
                                Error::protocol(format!(
                                    "Failed to serialize roots response: {}",
                                    e
                                ))
                            })?;
                        let response = JsonRpcResponse::success(result_value, request.id);
                        self.send_response(response).await?;
                    }
                    Err(e) => {
                        let error = turbomcp_protocol::jsonrpc::JsonRpcError {
                            code: -32603,
                            message: format!("Roots handler error: {}", e),
                            data: None,
                        };
                        let response = JsonRpcResponse::error_response(error, request.id);
                        self.send_response(response).await?;
                    }
                }
            }
            "elicitation/create" => {
                // Clone handler Arc before await to avoid holding mutex across await
                let handler_opt = self
                    .inner
                    .handlers
                    .lock()
                    .expect("handlers mutex poisoned")
                    .elicitation
                    .clone();
                if let Some(handler) = handler_opt {
                    // Parse elicitation request params as MCP protocol type
                    let proto_request: turbomcp_protocol::types::ElicitRequest =
                        serde_json::from_value(request.params.unwrap_or(serde_json::Value::Null))
                            .map_err(|e| {
                            Error::protocol(format!("Invalid elicitation params: {}", e))
                        })?;

                    // Wrap protocol request with ID for handler (preserves type safety!)
                    let handler_request =
                        crate::handlers::ElicitationRequest::new(request.id.clone(), proto_request);

                    // Call the registered elicitation handler
                    match handler.handle_elicitation(handler_request).await {
                        Ok(elicit_response) => {
                            // Convert handler response back to protocol type
                            let proto_result = elicit_response.into_protocol();
                            let result_value = serde_json::to_value(proto_result).map_err(|e| {
                                Error::protocol(format!(
                                    "Failed to serialize elicitation response: {}",
                                    e
                                ))
                            })?;
                            let response = JsonRpcResponse::success(result_value, request.id);
                            self.send_response(response).await?;
                        }
                        Err(e) => {
                            // Convert handler error to JSON-RPC error using centralized mapping
                            let response =
                                JsonRpcResponse::error_response(e.into_jsonrpc_error(), request.id);
                            self.send_response(response).await?;
                        }
                    }
                } else {
                    // No handler configured - elicitation not supported
                    let error = turbomcp_protocol::jsonrpc::JsonRpcError {
                        code: -32601,
                        message: "Elicitation not supported - no handler registered".to_string(),
                        data: None,
                    };
                    let response = JsonRpcResponse::error_response(error, request.id);
                    self.send_response(response).await?;
                }
            }
            _ => {
                // Unknown method
                let error = turbomcp_protocol::jsonrpc::JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                };
                let response = JsonRpcResponse::error_response(error, request.id);
                self.send_response(response).await?;
            }
        }
        Ok(())
    }

    /// Handle server-initiated notifications
    ///
    /// Routes notifications to appropriate handlers based on method name.
    /// MCP defines several notification types that servers can send to clients:
    ///
    /// - `notifications/progress` - Progress updates for long-running operations
    /// - `notifications/message` - Log messages from server
    /// - `notifications/resources/updated` - Resource content changed
    /// - `notifications/resources/list_changed` - Resource list changed
    async fn handle_notification(&self, notification: JsonRpcNotification) -> Result<()> {
        match notification.method.as_str() {
            "notifications/progress" => {
                // Route to progress handler
                let handler_opt = self
                    .inner
                    .handlers
                    .lock()
                    .expect("handlers mutex poisoned")
                    .get_progress_handler();

                if let Some(handler) = handler_opt {
                    // Parse progress notification
                    let progress: crate::handlers::ProgressNotification = serde_json::from_value(
                        notification.params.unwrap_or(serde_json::Value::Null),
                    )
                    .map_err(|e| {
                        Error::protocol(format!("Invalid progress notification: {}", e))
                    })?;

                    // Call handler (errors are logged but don't fail the flow)
                    if let Err(e) = handler.handle_progress(progress).await {
                        tracing::error!("Progress handler error: {}", e);
                    }
                } else {
                    tracing::debug!("Received progress notification but no handler registered");
                }
            }

            "notifications/message" => {
                // Route to log handler
                let handler_opt = self
                    .inner
                    .handlers
                    .lock()
                    .expect("handlers mutex poisoned")
                    .get_log_handler();

                if let Some(handler) = handler_opt {
                    // Parse log message
                    let log: crate::handlers::LoggingNotification = serde_json::from_value(
                        notification.params.unwrap_or(serde_json::Value::Null),
                    )
                    .map_err(|e| Error::protocol(format!("Invalid log notification: {}", e)))?;

                    // Call handler
                    if let Err(e) = handler.handle_log(log).await {
                        tracing::error!("Log handler error: {}", e);
                    }
                } else {
                    tracing::debug!("Received log notification but no handler registered");
                }
            }

            "notifications/resources/updated" => {
                // Route to resource update handler
                let handler_opt = self
                    .inner
                    .handlers
                    .lock()
                    .expect("handlers mutex poisoned")
                    .get_resource_update_handler();

                if let Some(handler) = handler_opt {
                    // Parse resource update notification
                    let update: crate::handlers::ResourceUpdatedNotification =
                        serde_json::from_value(
                            notification.params.unwrap_or(serde_json::Value::Null),
                        )
                        .map_err(|e| {
                            Error::protocol(format!("Invalid resource update notification: {}", e))
                        })?;

                    // Call handler
                    if let Err(e) = handler.handle_resource_update(update).await {
                        tracing::error!("Resource update handler error: {}", e);
                    }
                } else {
                    tracing::debug!(
                        "Received resource update notification but no handler registered"
                    );
                }
            }

            "notifications/resources/list_changed" => {
                // Route to resource list changed handler
                let handler_opt = self
                    .inner
                    .handlers
                    .lock()
                    .expect("handlers mutex poisoned")
                    .get_resource_list_changed_handler();

                if let Some(handler) = handler_opt {
                    if let Err(e) = handler.handle_resource_list_changed().await {
                        tracing::error!("Resource list changed handler error: {}", e);
                    }
                } else {
                    tracing::debug!(
                        "Resource list changed notification received (no handler registered)"
                    );
                }
            }

            "notifications/prompts/list_changed" => {
                // Route to prompt list changed handler
                let handler_opt = self
                    .inner
                    .handlers
                    .lock()
                    .expect("handlers mutex poisoned")
                    .get_prompt_list_changed_handler();

                if let Some(handler) = handler_opt {
                    if let Err(e) = handler.handle_prompt_list_changed().await {
                        tracing::error!("Prompt list changed handler error: {}", e);
                    }
                } else {
                    tracing::debug!(
                        "Prompt list changed notification received (no handler registered)"
                    );
                }
            }

            "notifications/tools/list_changed" => {
                // Route to tool list changed handler
                let handler_opt = self
                    .inner
                    .handlers
                    .lock()
                    .expect("handlers mutex poisoned")
                    .get_tool_list_changed_handler();

                if let Some(handler) = handler_opt {
                    if let Err(e) = handler.handle_tool_list_changed().await {
                        tracing::error!("Tool list changed handler error: {}", e);
                    }
                } else {
                    tracing::debug!(
                        "Tool list changed notification received (no handler registered)"
                    );
                }
            }

            "notifications/cancelled" => {
                // Route to cancellation handler
                let handler_opt = self
                    .inner
                    .handlers
                    .lock()
                    .expect("handlers mutex poisoned")
                    .get_cancellation_handler();

                if let Some(handler) = handler_opt {
                    // Parse cancellation notification
                    let cancellation: crate::handlers::CancelledNotification =
                        serde_json::from_value(
                            notification.params.unwrap_or(serde_json::Value::Null),
                        )
                        .map_err(|e| {
                            Error::protocol(format!("Invalid cancellation notification: {}", e))
                        })?;

                    // Call handler
                    if let Err(e) = handler.handle_cancellation(cancellation).await {
                        tracing::error!("Cancellation handler error: {}", e);
                    }
                } else {
                    tracing::debug!("Cancellation notification received (no handler registered)");
                }
            }

            _ => {
                // Unknown notification type
                tracing::debug!("Received unknown notification: {}", notification.method);
            }
        }

        Ok(())
    }

    async fn send_response(&self, response: JsonRpcResponse) -> Result<()> {
        let payload = serde_json::to_vec(&response)
            .map_err(|e| Error::protocol(format!("Failed to serialize response: {}", e)))?;

        let message = TransportMessage::new(
            turbomcp_protocol::MessageId::from("response".to_string()),
            payload.into(),
        );

        self.inner
            .protocol
            .transport()
            .send(message)
            .await
            .map_err(|e| Error::transport(format!("Failed to send response: {}", e)))?;

        Ok(())
    }

    /// Initialize the connection with the MCP server
    ///
    /// Performs the initialization handshake with the server, negotiating capabilities
    /// and establishing the protocol version. This method must be called before
    /// any other operations can be performed.
    ///
    /// # Returns
    ///
    /// Returns an `InitializeResult` containing server information and negotiated capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The transport connection fails
    /// - The server rejects the initialization request
    /// - Protocol negotiation fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    ///
    /// let result = client.initialize().await?;
    /// println!("Server: {} v{}", result.server_info.name, result.server_info.version);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn initialize(&self) -> Result<InitializeResult> {
        // Auto-connect transport if not already connected
        // This provides consistent DX across all transports (Stdio, TCP, HTTP, WebSocket, Unix)
        let transport = self.inner.protocol.transport();
        let transport_state = transport.state().await;
        if !matches!(
            transport_state,
            turbomcp_transport::TransportState::Connected
        ) {
            tracing::debug!(
                "Auto-connecting transport (current state: {:?})",
                transport_state
            );
            transport
                .connect()
                .await
                .map_err(|e| Error::transport(format!("Failed to connect transport: {}", e)))?;
            tracing::info!("Transport connected successfully");
        }

        // Build client capabilities based on registered handlers (automatic detection)
        let mut client_caps = ProtocolClientCapabilities::default();

        // Detect sampling capability from handler
        if let Some(sampling_caps) = self.get_sampling_capabilities() {
            client_caps.sampling = Some(sampling_caps);
        }

        // Detect elicitation capability from handler
        if let Some(elicitation_caps) = self.get_elicitation_capabilities() {
            client_caps.elicitation = Some(elicitation_caps);
        }

        // Detect roots capability from handler
        if let Some(roots_caps) = self.get_roots_capabilities() {
            client_caps.roots = Some(roots_caps);
        }

        // Send MCP initialization request
        let request = InitializeRequest {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: client_caps,
            client_info: turbomcp_protocol::types::Implementation {
                name: "turbomcp-client".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("TurboMCP Client".to_string()),
            },
            _meta: None,
        };

        let protocol_response: ProtocolInitializeResult = self
            .inner
            .protocol
            .request("initialize", Some(serde_json::to_value(request)?))
            .await?;

        // AtomicBool: lock-free store with Ordering::Relaxed
        self.inner.initialized.store(true, Ordering::Relaxed);

        // Send initialized notification
        self.inner
            .protocol
            .notify("notifications/initialized", None)
            .await?;

        // Convert protocol response to client response type
        Ok(InitializeResult {
            server_info: protocol_response.server_info,
            server_capabilities: protocol_response.capabilities,
        })
    }

    /// Execute a protocol method with plugin middleware
    ///
    /// This is a generic helper for wrapping protocol calls with plugin middleware.
    pub(crate) async fn execute_with_plugins<R>(
        &self,
        method_name: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R>
    where
        R: serde::de::DeserializeOwned + serde::Serialize + Clone,
    {
        // Create JSON-RPC request for plugin context
        let json_rpc_request = turbomcp_protocol::jsonrpc::JsonRpcRequest {
            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
            id: turbomcp_protocol::MessageId::Number(1),
            method: method_name.to_string(),
            params: params.clone(),
        };

        // 1. Create request context for plugins
        let mut req_ctx =
            crate::plugins::RequestContext::new(json_rpc_request, std::collections::HashMap::new());

        // 2. Execute before_request plugin middleware
        if let Err(e) = self
            .inner
            .plugin_registry
            .lock()
            .await
            .execute_before_request(&mut req_ctx)
            .await
        {
            return Err(Error::bad_request(format!(
                "Plugin before_request failed: {}",
                e
            )));
        }

        // 3. Execute the actual protocol call
        let start_time = std::time::Instant::now();
        let protocol_result: Result<R> = self
            .inner
            .protocol
            .request(method_name, req_ctx.params().cloned())
            .await;
        let duration = start_time.elapsed();

        // 4. Prepare response context
        let mut resp_ctx = match protocol_result {
            Ok(ref response) => {
                let response_value = serde_json::to_value(response.clone())?;
                crate::plugins::ResponseContext::new(req_ctx, Some(response_value), None, duration)
            }
            Err(ref e) => {
                crate::plugins::ResponseContext::new(req_ctx, None, Some(*e.clone()), duration)
            }
        };

        // 5. Execute after_response plugin middleware
        if let Err(e) = self
            .inner
            .plugin_registry
            .lock()
            .await
            .execute_after_response(&mut resp_ctx)
            .await
        {
            return Err(Error::bad_request(format!(
                "Plugin after_response failed: {}",
                e
            )));
        }

        // 6. Return the final result, checking for plugin modifications
        match protocol_result {
            Ok(ref response) => {
                // Check if plugins modified the response
                if let Some(modified_response) = resp_ctx.response {
                    // Try to deserialize the modified response
                    if let Ok(modified_result) =
                        serde_json::from_value::<R>(modified_response.clone())
                    {
                        return Ok(modified_result);
                    }
                }

                // No plugin modifications, use original response
                Ok(response.clone())
            }
            Err(e) => {
                // Check if plugins provided an error recovery response
                if let Some(recovery_response) = resp_ctx.response {
                    if let Ok(recovery_result) = serde_json::from_value::<R>(recovery_response) {
                        Ok(recovery_result)
                    } else {
                        Err(e)
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Subscribe to resource change notifications
    ///
    /// Registers interest in receiving notifications when the specified
    /// resource changes. The server will send notifications when the
    /// resource is modified, created, or deleted.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to monitor
    ///
    /// # Returns
    ///
    /// Returns `EmptyResult` on successful subscription.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The URI is invalid or empty
    /// - The server doesn't support subscriptions
    /// - The request fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Subscribe to file changes
    /// client.subscribe("file:///watch/directory").await?;
    /// println!("Subscribed to resource changes");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn subscribe(&self, uri: &str) -> Result<EmptyResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        if uri.is_empty() {
            return Err(Error::bad_request("Subscription URI cannot be empty"));
        }

        // Send resources/subscribe request with plugin middleware
        let request = SubscribeRequest {
            uri: uri.to_string(),
        };

        self.execute_with_plugins(
            "resources/subscribe",
            Some(serde_json::to_value(request).map_err(|e| {
                Error::protocol(format!("Failed to serialize subscribe request: {}", e))
            })?),
        )
        .await
    }

    /// Unsubscribe from resource change notifications
    ///
    /// Cancels a previous subscription to resource changes. After unsubscribing,
    /// the client will no longer receive notifications for the specified resource.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to stop monitoring
    ///
    /// # Returns
    ///
    /// Returns `EmptyResult` on successful unsubscription.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The URI is invalid or empty
    /// - No active subscription exists for the URI
    /// - The request fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_protocol::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Unsubscribe from file changes
    /// client.unsubscribe("file:///watch/directory").await?;
    /// println!("Unsubscribed from resource changes");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unsubscribe(&self, uri: &str) -> Result<EmptyResult> {
        if !self.inner.initialized.load(Ordering::Relaxed) {
            return Err(Error::bad_request("Client not initialized"));
        }

        if uri.is_empty() {
            return Err(Error::bad_request("Unsubscription URI cannot be empty"));
        }

        // Send resources/unsubscribe request with plugin middleware
        let request = UnsubscribeRequest {
            uri: uri.to_string(),
        };

        self.execute_with_plugins(
            "resources/unsubscribe",
            Some(serde_json::to_value(request).map_err(|e| {
                Error::protocol(format!("Failed to serialize unsubscribe request: {}", e))
            })?),
        )
        .await
    }

    /// Get the client's capabilities configuration
    pub fn capabilities(&self) -> &ClientCapabilities {
        &self.inner.capabilities
    }

    /// Initialize all registered plugins
    ///
    /// This should be called after registration but before using the client.
    pub async fn initialize_plugins(&self) -> Result<()> {
        // Set up client context for plugins with actual client capabilities
        let mut capabilities = std::collections::HashMap::new();
        capabilities.insert(
            "protocol_version".to_string(),
            serde_json::json!("2024-11-05"),
        );
        capabilities.insert(
            "mcp_version".to_string(),
            serde_json::json!(env!("CARGO_PKG_VERSION")),
        );
        capabilities.insert(
            "supports_notifications".to_string(),
            serde_json::json!(true),
        );
        capabilities.insert(
            "supports_sampling".to_string(),
            serde_json::json!(self.has_sampling_handler()),
        );
        capabilities.insert("supports_progress".to_string(), serde_json::json!(true));
        capabilities.insert("supports_roots".to_string(), serde_json::json!(true));

        // Extract client configuration
        let mut config = std::collections::HashMap::new();
        config.insert(
            "client_name".to_string(),
            serde_json::json!("turbomcp-client"),
        );
        config.insert(
            "initialized".to_string(),
            serde_json::json!(self.inner.initialized.load(Ordering::Relaxed)),
        );
        config.insert(
            "plugin_count".to_string(),
            serde_json::json!(self.inner.plugin_registry.lock().await.plugin_count()),
        );

        let context = crate::plugins::PluginContext::new(
            "turbomcp-client".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            capabilities,
            config,
            vec![], // Will be populated by the registry
        );

        self.inner
            .plugin_registry
            .lock()
            .await
            .set_client_context(context);

        // Note: Individual plugins are initialized automatically during registration
        // via PluginRegistry::register_plugin(). This method ensures the registry
        // has proper client context for any future plugin registrations.
        Ok(())
    }

    /// Cleanup all registered plugins
    ///
    /// This should be called when the client is being shut down.
    pub async fn cleanup_plugins(&self) -> Result<()> {
        // Clear the plugin registry - plugins will be dropped and cleaned up automatically
        // The Rust ownership system ensures proper cleanup when the Arc<dyn ClientPlugin>
        // references are dropped.

        // Note: The plugin system uses RAII (Resource Acquisition Is Initialization)
        // pattern where plugins clean up their resources in their Drop implementation.
        // Replace the registry with a fresh one (mutex ensures safe access)
        *self.inner.plugin_registry.lock().await = crate::plugins::PluginRegistry::new();
        Ok(())
    }

    // Note: Capability detection methods (has_*_handler, get_*_capabilities)
    // are defined in their respective operation modules:
    // - sampling.rs: has_sampling_handler, get_sampling_capabilities
    // - handlers.rs: has_elicitation_handler, has_roots_handler
    //
    // Additional capability getters for elicitation and roots added below
    // since they're used during initialization

    /// Get elicitation capabilities if handler is registered
    /// Automatically detects capability based on registered handler
    fn get_elicitation_capabilities(
        &self,
    ) -> Option<turbomcp_protocol::types::ElicitationCapabilities> {
        if self.has_elicitation_handler() {
            // Currently returns default capabilities. In the future, schema_validation support
            // could be detected from handler traits by adding a HasSchemaValidation marker trait
            // that handlers could implement. For now, handlers validate schemas themselves.
            Some(turbomcp_protocol::types::ElicitationCapabilities::default())
        } else {
            None
        }
    }

    /// Get roots capabilities if handler is registered
    fn get_roots_capabilities(&self) -> Option<turbomcp_protocol::types::RootsCapabilities> {
        if self.has_roots_handler() {
            // Roots capabilities indicate whether list can change
            Some(turbomcp_protocol::types::RootsCapabilities {
                list_changed: Some(true), // Support dynamic roots by default
            })
        } else {
            None
        }
    }
}

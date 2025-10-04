//! Core Client implementation for MCP communication
//!
//! This module contains the main Client<T> struct and its implementation,
//! providing the core MCP client functionality including:
//!
//! - Connection initialization and lifecycle management
//! - Message processing and bidirectional communication
//! - MCP operation support (tools, prompts, resources, sampling, etc.)
//! - Plugin middleware integration
//! - Handler registration and management

use std::sync::Arc;

use turbomcp_core::{Error, PROTOCOL_VERSION, Result};
use turbomcp_protocol::jsonrpc::*;
use turbomcp_protocol::types::{
    ClientCapabilities as ProtocolClientCapabilities, InitializeResult as ProtocolInitializeResult,
    *,
};
use turbomcp_transport::{Transport, TransportMessage};

use super::config::InitializeResult;
use super::protocol::ProtocolClient;
use crate::{ClientCapabilities, handlers::HandlerRegistry, sampling::SamplingHandler};

/// The core MCP client implementation
///
/// Client provides a comprehensive interface for communicating with MCP servers,
/// supporting all protocol features including tools, prompts, resources, sampling,
/// elicitation, and bidirectional communication patterns.
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
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::Client;
/// use turbomcp_transport::stdio::StdioTransport;
/// use std::collections::HashMap;
///
/// # async fn example() -> turbomcp_core::Result<()> {
/// // Create and initialize client
/// let mut client = Client::new(StdioTransport::new());
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
#[derive(Debug)]
pub struct Client<T: Transport> {
    pub(super) protocol: ProtocolClient<T>,
    pub(super) capabilities: ClientCapabilities,
    pub(super) initialized: bool,
    #[allow(dead_code)]
    pub(super) sampling_handler: Option<Arc<dyn SamplingHandler>>,
    /// Handler registry for bidirectional communication
    pub(super) handlers: HandlerRegistry,
    /// Plugin registry for middleware and extensibility
    pub(super) plugin_registry: crate::plugins::PluginRegistry,
}

impl<T: Transport> Client<T> {
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
        Self {
            protocol: ProtocolClient::new(transport),
            capabilities: ClientCapabilities::default(),
            initialized: false,
            sampling_handler: None,
            handlers: HandlerRegistry::new(),
            plugin_registry: crate::plugins::PluginRegistry::new(),
        }
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
        Self {
            protocol: ProtocolClient::new(transport),
            capabilities,
            initialized: false,
            sampling_handler: None,
            handlers: HandlerRegistry::new(),
            plugin_registry: crate::plugins::PluginRegistry::new(),
        }
    }

    /// Process incoming messages from the server
    ///
    /// This method should be called in a loop to handle server-initiated requests
    /// like sampling. It processes one message at a time.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if a message was processed, `Ok(false)` if no message was available.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    ///
    /// // Process messages in background
    /// tokio::spawn(async move {
    ///     loop {
    ///         if let Err(e) = client.process_message().await {
    ///             eprintln!("Error processing message: {}", e);
    ///         }
    ///     }
    /// });
    /// # Ok(())
    /// # }
    /// ```
    pub async fn process_message(&mut self) -> Result<bool> {
        // Try to receive a message without blocking
        let message = match self.protocol.transport_mut().receive().await {
            Ok(Some(msg)) => msg,
            Ok(None) => return Ok(false),
            Err(e) => {
                return Err(Error::transport(format!(
                    "Failed to receive message: {}",
                    e
                )));
            }
        };

        // Parse as JSON-RPC message
        let json_msg: JsonRpcMessage = serde_json::from_slice(&message.payload)
            .map_err(|e| Error::protocol(format!("Invalid JSON-RPC message: {}", e)))?;

        match json_msg {
            JsonRpcMessage::Request(request) => {
                self.handle_request(request).await?;
                Ok(true)
            }
            JsonRpcMessage::Response(_) => {
                // Responses are handled by the protocol client during request/response flow
                Ok(true)
            }
            JsonRpcMessage::Notification(notification) => {
                self.handle_notification(notification).await?;
                Ok(true)
            }
            JsonRpcMessage::RequestBatch(_)
            | JsonRpcMessage::ResponseBatch(_)
            | JsonRpcMessage::MessageBatch(_) => {
                // Batch operations not yet supported
                Ok(true)
            }
        }
    }

    async fn handle_request(&mut self, request: JsonRpcRequest) -> Result<()> {
        match request.method.as_str() {
            "sampling/createMessage" => {
                if let Some(handler) = &self.sampling_handler {
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
            "elicitation/create" => {
                if let Some(handler) = &self.handlers.elicitation {
                    // Parse elicitation request params
                    let params: crate::handlers::ElicitationRequest =
                        serde_json::from_value(request.params.unwrap_or(serde_json::Value::Null))
                            .map_err(|e| {
                            Error::protocol(format!("Invalid elicitation params: {}", e))
                        })?;

                    // Call the registered elicitation handler
                    match handler.handle_elicitation(params).await {
                        Ok(elicit_response) => {
                            let result_value =
                                serde_json::to_value(elicit_response).map_err(|e| {
                                    Error::protocol(format!(
                                        "Failed to serialize elicitation response: {}",
                                        e
                                    ))
                                })?;
                            let response = JsonRpcResponse::success(result_value, request.id);
                            self.send_response(response).await?;
                        }
                        Err(e) => {
                            // Map handler errors to JSON-RPC errors
                            let (code, message) = match e {
                                crate::handlers::HandlerError::UserCancelled => {
                                    (-32800, "User cancelled elicitation request".to_string())
                                }
                                crate::handlers::HandlerError::Timeout { timeout_seconds } => (
                                    -32801,
                                    format!(
                                        "Elicitation request timed out after {} seconds",
                                        timeout_seconds
                                    ),
                                ),
                                crate::handlers::HandlerError::InvalidInput { details } => {
                                    (-32602, format!("Invalid user input: {}", details))
                                }
                                _ => (-32603, format!("Elicitation handler error: {}", e)),
                            };
                            let error = turbomcp_protocol::jsonrpc::JsonRpcError {
                                code,
                                message,
                                data: None,
                            };
                            let response = JsonRpcResponse::error_response(error, request.id);
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

    async fn handle_notification(&mut self, _notification: JsonRpcNotification) -> Result<()> {
        // Handle notifications if needed
        // Currently MCP doesn't define client-side notifications
        Ok(())
    }

    async fn send_response(&mut self, response: JsonRpcResponse) -> Result<()> {
        let payload = serde_json::to_vec(&response)
            .map_err(|e| Error::protocol(format!("Failed to serialize response: {}", e)))?;

        let message = TransportMessage::new(
            turbomcp_core::MessageId::from("response".to_string()),
            payload.into(),
        );

        self.protocol
            .transport_mut()
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
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    ///
    /// let result = client.initialize().await?;
    /// println!("Server: {} v{}", result.server_info.name, result.server_info.version);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn initialize(&mut self) -> Result<InitializeResult> {
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
            client_info: turbomcp_protocol::Implementation {
                name: "turbomcp-client".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("TurboMCP Client".to_string()),
            },
            _meta: None,
        };

        let protocol_response: ProtocolInitializeResult = self
            .protocol
            .request("initialize", Some(serde_json::to_value(request)?))
            .await?;
        self.initialized = true;

        // Send initialized notification
        self.protocol
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
        &mut self,
        method_name: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R>
    where
        R: serde::de::DeserializeOwned + serde::Serialize + Clone,
    {
        // Create JSON-RPC request for plugin context
        let json_rpc_request = turbomcp_protocol::jsonrpc::JsonRpcRequest {
            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
            id: turbomcp_core::MessageId::Number(1),
            method: method_name.to_string(),
            params: params.clone(),
        };

        // 1. Create request context for plugins
        let mut req_ctx =
            crate::plugins::RequestContext::new(json_rpc_request, std::collections::HashMap::new());

        // 2. Execute before_request plugin middleware
        if let Err(e) = self
            .plugin_registry
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
            .plugin_registry
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
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Subscribe to file changes
    /// client.subscribe("file:///watch/directory").await?;
    /// println!("Subscribed to resource changes");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn subscribe(&mut self, uri: &str) -> Result<EmptyResult> {
        if !self.initialized {
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
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Unsubscribe from file changes
    /// client.unsubscribe("file:///watch/directory").await?;
    /// println!("Unsubscribed from resource changes");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn unsubscribe(&mut self, uri: &str) -> Result<EmptyResult> {
        if !self.initialized {
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
        &self.capabilities
    }

    /// Initialize all registered plugins
    ///
    /// This should be called after registration but before using the client.
    pub async fn initialize_plugins(&mut self) -> Result<()> {
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
            serde_json::json!(self.initialized),
        );
        config.insert(
            "plugin_count".to_string(),
            serde_json::json!(self.plugin_registry.plugin_count()),
        );

        let context = crate::plugins::PluginContext::new(
            "turbomcp-client".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            capabilities,
            config,
            vec![], // Will be populated by the registry
        );

        self.plugin_registry.set_client_context(context);

        // Note: Individual plugins are initialized automatically during registration
        // via PluginRegistry::register_plugin(). This method ensures the registry
        // has proper client context for any future plugin registrations.
        Ok(())
    }

    /// Cleanup all registered plugins
    ///
    /// This should be called when the client is being shut down.
    pub async fn cleanup_plugins(&mut self) -> Result<()> {
        // Clear the plugin registry - plugins will be dropped and cleaned up automatically
        // The Rust ownership system ensures proper cleanup when the Arc<dyn ClientPlugin>
        // references are dropped.

        // Note: The plugin system uses RAII (Resource Acquisition Is Initialization)
        // pattern where plugins clean up their resources in their Drop implementation.
        // No explicit cleanup is needed beyond clearing the registry.

        self.plugin_registry = crate::plugins::PluginRegistry::new();
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
            // TODO: Could detect schema_validation support from handler traits in the future
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

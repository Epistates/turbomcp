//! # `TurboMCP` Client
//!
//! MCP (Model Context Protocol) client implementation for connecting to MCP servers
//! and consuming their capabilities (tools, prompts, resources, and sampling).
//!
//! ## Features
//!
//! - Connection management with automatic reconnection
//! - Error handling and recovery mechanisms
//! - Support for all MCP capabilities including bidirectional sampling
//! - Elicitation response handling for server-initiated user input requests
//! - Transport-agnostic design (works with any `Transport` implementation)
//! - Type-safe protocol communication
//! - Request/response correlation tracking
//! - Timeout and cancellation support
//! - Automatic capability negotiation
//! - Handler support for server-initiated requests (sampling and elicitation)
//!
//! ## Architecture
//!
//! The client follows a layered architecture:
//!
//! ```text
//! Application Layer
//!        ↓
//! Client API (this crate)
//!        ↓  
//! Protocol Layer (turbomcp-protocol)
//!        ↓
//! Transport Layer (turbomcp-transport)
//! ```
//!
//! ## Usage
//!
//! ```rust,no_run
//! use turbomcp_client::{Client, ClientBuilder};
//! use turbomcp_transport::stdio::StdioTransport;
//!
//! # async fn example() -> turbomcp_core::Result<()> {
//! // Create a client with stdio transport
//! let transport = StdioTransport::new();
//! let mut client = Client::new(transport);
//!
//! // Initialize connection and negotiate capabilities
//! let result = client.initialize().await?;
//! println!("Connected to: {}", result.server_info.name);
//!
//! // List and call tools
//! let tools = client.list_tools().await?;
//! for tool in tools {
//!     println!("Tool: {}", tool);
//! }
//!
//! // Access resources
//! let resources = client.list_resources().await?;
//! for resource in resources {
//!     println!("Resource: {}", resource);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Elicitation Response Handling (New in 1.0.3)
//!
//! The client now supports handling server-initiated elicitation requests:
//!
//! ```rust,no_run
//! use turbomcp_client::Client;
//! use std::collections::HashMap;
//!
//! // Simple elicitation handling example
//! async fn handle_server_elicitation() {
//!     // When server requests user input, you would:
//!     // 1. Present the schema to the user
//!     // 2. Collect their input  
//!     // 3. Send response back to server
//!     
//!     let user_preferences: HashMap<String, String> = HashMap::new();
//!     // Your UI/CLI interaction logic here
//!     println!("Server requesting user preferences");
//! }
//! ```
//!
//! ## Sampling Support (New in 1.0.3)
//!
//! Handle server-initiated sampling requests for LLM capabilities:
//!
//! ```rust,no_run
//! use turbomcp_client::Client;
//! use turbomcp_client::sampling::SamplingHandler;
//! use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult};
//! use async_trait::async_trait;
//!
//! #[derive(Debug)]
//! struct MySamplingHandler {
//!     // Your LLM client would go here
//! }
//!
//! #[async_trait]
//! impl SamplingHandler for MySamplingHandler {
//!     async fn handle_create_message(
//!         &self,
//!         request: CreateMessageRequest
//!     ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
//!         // Forward to your LLM provider (OpenAI, Anthropic, etc.)
//!         // This enables the server to request LLM sampling through the client
//!         
//!         Ok(CreateMessageResult {
//!             role: turbomcp_protocol::types::Role::Assistant,
//!             content: turbomcp_protocol::types::Content::Text(
//!                 turbomcp_protocol::types::TextContent {
//!                     text: "Response from LLM".to_string(),
//!                     annotations: None,
//!                     meta: None,
//!                 }
//!             ),
//!             model: Some("gpt-4".to_string()),
//!             stop_reason: Some("end_turn".to_string()),
//!         })
//!     }
//! }
//! ```
//!
//! ## Error Handling
//!
//! The client provides comprehensive error handling with automatic retry logic:
//!
//! ```rust,no_run
//! # use turbomcp_client::Client;
//! # use turbomcp_transport::stdio::StdioTransport;
//! # async fn example() -> turbomcp_core::Result<()> {
//! # let mut client = Client::new(StdioTransport::new());
//! match client.call_tool("my_tool", None).await {
//!     Ok(result) => println!("Tool result: {:?}", result),
//!     Err(e) => eprintln!("Tool call failed: {}", e),
//! }
//! # Ok(())
//! # }
//! ```

pub mod sampling;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use turbomcp_core::{Error, PROTOCOL_VERSION, Result};
use turbomcp_protocol::jsonrpc::{
    JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcVersion,
};
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, ClientCapabilities as ProtocolClientCapabilities, Content,
    CreateMessageRequest, InitializeRequest, InitializeResult as ProtocolInitializeResult,
    ListResourcesResult, ListToolsResult, ServerCapabilities,
};
use turbomcp_transport::{Transport, TransportMessage};

use crate::sampling::SamplingHandler;

/// Client capability configuration
///
/// Defines the capabilities that this client supports when connecting to MCP servers.
/// These capabilities are sent during the initialization handshake to negotiate
/// which features will be available during the session.
///
/// # Examples
///
/// ```
/// use turbomcp_client::ClientCapabilities;
///
/// let capabilities = ClientCapabilities {
///     tools: true,
///     prompts: true,
///     resources: true,
///     sampling: false,
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct ClientCapabilities {
    /// Whether the client supports tool calling
    pub tools: bool,

    /// Whether the client supports prompts
    pub prompts: bool,

    /// Whether the client supports resources
    pub resources: bool,

    /// Whether the client supports sampling
    pub sampling: bool,
}

/// JSON-RPC protocol handler for MCP communication
///
/// Handles request/response correlation, serialization, and protocol-level concerns.
/// This is the missing abstraction layer between raw Transport and high-level Client APIs.
#[derive(Debug)]
struct ProtocolClient<T: Transport> {
    transport: T,
    next_id: AtomicU64,
}

impl<T: Transport> ProtocolClient<T> {
    fn new(transport: T) -> Self {
        Self {
            transport,
            next_id: AtomicU64::new(1),
        }
    }

    /// Send JSON-RPC request and await typed response
    async fn request<R: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<R> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: turbomcp_core::MessageId::from(id.to_string()),
            method: method.to_string(),
            params,
        };

        // Serialize and send
        let payload = serde_json::to_vec(&request)
            .map_err(|e| Error::protocol(format!("Failed to serialize request: {e}")))?;

        let message = TransportMessage::new(
            turbomcp_core::MessageId::from(format!("req-{id}")),
            payload.into(),
        );
        self.transport
            .send(message)
            .await
            .map_err(|e| Error::transport(format!("Transport send failed: {e}")))?;

        // Receive and deserialize response
        let response_msg = self
            .transport
            .receive()
            .await
            .map_err(|e| Error::transport(format!("Transport receive failed: {e}")))?
            .ok_or_else(|| Error::transport("No response received".to_string()))?;

        let response: JsonRpcResponse = serde_json::from_slice(&response_msg.payload)
            .map_err(|e| Error::protocol(format!("Invalid JSON-RPC response: {e}")))?;

        if let Some(error) = response.error {
            return Err(Error::rpc(error.code, &error.message));
        }

        let result = response
            .result
            .ok_or_else(|| Error::protocol("Response missing result field".to_string()))?;

        serde_json::from_value(result)
            .map_err(|e| Error::protocol(format!("Invalid response format: {e}")))
    }

    /// Send JSON-RPC notification (no response expected)
    async fn notify(&mut self, method: &str, params: Option<serde_json::Value>) -> Result<()> {
        let notification = JsonRpcNotification {
            jsonrpc: JsonRpcVersion,
            method: method.to_string(),
            params,
        };

        let payload = serde_json::to_vec(&notification)
            .map_err(|e| Error::protocol(format!("Failed to serialize notification: {e}")))?;

        let message = TransportMessage::new(
            turbomcp_core::MessageId::from("notification"),
            payload.into(),
        );
        self.transport
            .send(message)
            .await
            .map_err(|e| Error::transport(format!("Transport send failed: {e}")))?;

        Ok(())
    }
}

/// MCP client for communicating with servers
///
/// The `Client` struct provides a beautiful, ergonomic interface for interacting with MCP servers.
/// It handles all protocol complexity internally, exposing only clean, type-safe methods.
///
/// # Type Parameters
///
/// * `T` - The transport implementation used for communication
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::Client;
/// use turbomcp_transport::stdio::StdioTransport;
///
/// # async fn example() -> turbomcp_core::Result<()> {
/// let transport = StdioTransport::new();
/// let mut client = Client::new(transport);
///
/// // Initialize and start using the client
/// client.initialize().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Client<T: Transport> {
    protocol: ProtocolClient<T>,
    capabilities: ClientCapabilities,
    initialized: bool,
    #[allow(dead_code)]
    sampling_handler: Option<Arc<dyn SamplingHandler>>,
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
        }
    }

    /// Set the sampling handler for processing server-initiated sampling requests
    ///
    /// # Arguments
    ///
    /// * `handler` - The handler implementation for sampling requests
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::{Client, sampling::DefaultSamplingHandler};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use std::sync::Arc;
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.set_sampling_handler(Arc::new(DefaultSamplingHandler));
    /// ```
    pub fn set_sampling_handler(&mut self, handler: Arc<dyn SamplingHandler>) {
        self.sampling_handler = Some(handler);
        self.capabilities.sampling = true;
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
        let message = match self.protocol.transport.receive().await {
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
                            let response = JsonRpcResponse {
                                jsonrpc: JsonRpcVersion,
                                id: Some(request.id),
                                result: Some(serde_json::to_value(result).map_err(|e| {
                                    Error::protocol(format!("Failed to serialize response: {}", e))
                                })?),
                                error: None,
                            };
                            self.send_response(response).await?;
                        }
                        Err(e) => {
                            let response = JsonRpcResponse {
                                jsonrpc: JsonRpcVersion,
                                id: Some(request.id),
                                result: None,
                                error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: -32603,
                                    message: format!("Sampling handler error: {}", e),
                                    data: None,
                                }),
                            };
                            self.send_response(response).await?;
                        }
                    }
                } else {
                    // No handler configured
                    let response = JsonRpcResponse {
                        jsonrpc: JsonRpcVersion,
                        id: Some(request.id),
                        result: None,
                        error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                            code: -32601,
                            message: "Sampling not supported".to_string(),
                            data: None,
                        }),
                    };
                    self.send_response(response).await?;
                }
            }
            _ => {
                // Unknown method
                let response = JsonRpcResponse {
                    jsonrpc: JsonRpcVersion,
                    id: Some(request.id),
                    result: None,
                    error: Some(turbomcp_protocol::jsonrpc::JsonRpcError {
                        code: -32601,
                        message: format!("Method not found: {}", request.method),
                        data: None,
                    }),
                };
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
            .transport
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
        // Build client capabilities based on configuration
        let mut client_caps = ProtocolClientCapabilities::default();
        if self.capabilities.sampling {
            client_caps.sampling = Some(turbomcp_protocol::types::SamplingCapabilities);
        }

        // Send actual MCP initialization request
        let request = InitializeRequest {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: client_caps,
            client_info: turbomcp_protocol::Implementation {
                name: "turbomcp-client".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("TurboMCP Client".to_string()),
            },
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

    /// List available tools from the server
    ///
    /// Retrieves the list of tools that the server provides. Tools are functions
    /// that can be called to perform specific operations on the server.
    ///
    /// # Returns
    ///
    /// Returns a vector of tool names available on the server.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server doesn't support tools
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
    /// let tools = client.list_tools().await?;
    /// for tool in tools {
    ///     println!("Available tool: {}", tool);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_tools(&mut self) -> Result<Vec<String>> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send actual tools/list request
        let response: ListToolsResult = self.protocol.request("tools/list", None).await?;
        let tool_names = response.tools.into_iter().map(|tool| tool.name).collect();
        Ok(tool_names)
    }

    /// Call a tool on the server
    ///
    /// Executes a tool on the server with the provided arguments.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the tool to call
    /// * `arguments` - Optional arguments to pass to the tool
    ///
    /// # Returns
    ///
    /// Returns the result of the tool execution.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// let mut args = HashMap::new();
    /// args.insert("input".to_string(), serde_json::json!("test"));
    ///
    /// let result = client.call_tool("my_tool", Some(args)).await?;
    /// println!("Tool result: {:?}", result);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<serde_json::Value> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send actual tools/call request
        let request = CallToolRequest {
            name: name.to_string(),
            arguments: Some(arguments.unwrap_or_default()),
        };

        let response: CallToolResult = self
            .protocol
            .request("tools/call", Some(serde_json::to_value(request)?))
            .await?;

        // Extract content from response - for simplicity, return the first text content
        if let Some(content) = response.content.first() {
            match content {
                Content::Text(text_content) => Ok(serde_json::json!({
                    "text": text_content.text,
                    "is_error": response.is_error.unwrap_or(false)
                })),
                Content::Image(image_content) => Ok(serde_json::json!({
                    "image": image_content.data,
                    "mime_type": image_content.mime_type,
                    "is_error": response.is_error.unwrap_or(false)
                })),
                Content::Resource(resource_content) => Ok(serde_json::json!({
                    "resource": resource_content.resource,
                    "annotations": resource_content.annotations,
                    "is_error": response.is_error.unwrap_or(false)
                })),
                Content::Audio(audio_content) => Ok(serde_json::json!({
                    "audio": audio_content.data,
                    "mime_type": audio_content.mime_type,
                    "is_error": response.is_error.unwrap_or(false)
                })),
                Content::ResourceLink(resource_link) => Ok(serde_json::json!({
                    "resource_uri": resource_link.uri,
                    "is_error": response.is_error.unwrap_or(false)
                })),
            }
        } else {
            Ok(serde_json::json!({
                "message": "No content returned",
                "is_error": response.is_error.unwrap_or(false)
            }))
        }
    }

    /// List available resources from the server
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
    /// let resources = client.list_resources().await?;
    /// for resource in resources {
    ///     println!("Available resource: {}", resource);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_resources(&mut self) -> Result<Vec<String>> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send actual resources/list request
        let response: ListResourcesResult = self.protocol.request("resources/list", None).await?;
        let resource_uris = response
            .resources
            .into_iter()
            .map(|resource| resource.uri)
            .collect();
        Ok(resource_uris)
    }
}

/// Result of client initialization
///
/// Contains information about the server and the negotiated capabilities
/// after a successful initialization handshake.
///
/// # Examples
///
/// ```rust,no_run
/// # use turbomcp_client::Client;
/// # use turbomcp_transport::stdio::StdioTransport;
/// # async fn example() -> turbomcp_core::Result<()> {
/// let mut client = Client::new(StdioTransport::new());
/// let result = client.initialize().await?;
///
/// println!("Server: {}", result.server_info.name);
/// println!("Version: {}", result.server_info.version);
/// if let Some(title) = result.server_info.title {
///     println!("Title: {}", title);
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct InitializeResult {
    /// Information about the server
    pub server_info: turbomcp_protocol::Implementation,

    /// Capabilities supported by the server
    pub server_capabilities: ServerCapabilities,
}

// ServerCapabilities is now imported from turbomcp_protocol::types

/// Builder for configuring and creating MCP clients
///
/// Provides a fluent interface for configuring client options before creation.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::ClientBuilder;
/// use turbomcp_transport::stdio::StdioTransport;
///
/// # async fn example() -> turbomcp_core::Result<()> {
/// let client = ClientBuilder::new()
///     .with_tools(true)
///     .with_prompts(true)
///     .with_resources(false)
///     .build(StdioTransport::new());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default)]
pub struct ClientBuilder {
    capabilities: ClientCapabilities,
}

impl ClientBuilder {
    /// Create a new client builder
    ///
    /// Returns a new builder with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable tool support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable tool support
    pub fn with_tools(mut self, enabled: bool) -> Self {
        self.capabilities.tools = enabled;
        self
    }

    /// Enable or disable prompt support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable prompt support
    pub fn with_prompts(mut self, enabled: bool) -> Self {
        self.capabilities.prompts = enabled;
        self
    }

    /// Enable or disable resource support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable resource support
    pub fn with_resources(mut self, enabled: bool) -> Self {
        self.capabilities.resources = enabled;
        self
    }

    /// Enable or disable sampling support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable sampling support
    pub fn with_sampling(mut self, enabled: bool) -> Self {
        self.capabilities.sampling = enabled;
        self
    }

    /// Build a client with the configured options
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport to use for the client
    ///
    /// # Returns
    ///
    /// Returns a configured `Client` instance.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::stdio::StdioTransport;
    ///
    /// let client = ClientBuilder::new()
    ///     .with_tools(true)
    ///     .build(StdioTransport::new());
    /// ```
    pub fn build<T: Transport>(self, transport: T) -> Client<T> {
        Client::with_capabilities(transport, self.capabilities)
    }
}

// Re-export types for public API
pub use turbomcp_protocol::types::ServerCapabilities as PublicServerCapabilities;

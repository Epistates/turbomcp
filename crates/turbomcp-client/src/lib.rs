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
//!     println!("Tool: {} - {}", tool.name, tool.description.as_deref().unwrap_or("No description"));
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
//!         // Allows the server to request LLM sampling through the client
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
//!             _meta: None,
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

pub mod handlers;
pub mod llm;
pub mod plugins;
pub mod sampling;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;

use turbomcp_core::{Error, PROTOCOL_VERSION, Result};
use turbomcp_protocol::jsonrpc::{
    JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcVersion,
};
use turbomcp_protocol::types::{
    CallToolRequest,
    CallToolResult,
    ClientCapabilities as ProtocolClientCapabilities,
    CompleteResult,
    Content,
    CreateMessageRequest,
    EmptyResult,
    GetPromptRequest,
    GetPromptResult,
    InitializeRequest,
    InitializeResult as ProtocolInitializeResult,
    ListPromptsResult,
    ListResourceTemplatesResult,
    ListResourcesResult,
    ListRootsResult,
    ListToolsResult,
    LogLevel,
    // Missing protocol method types
    PingResult,
    Prompt,
    PromptInput,
    ReadResourceRequest,
    ReadResourceResult,
    ServerCapabilities,
    SetLevelRequest,
    SetLevelResult,
    SubscribeRequest,
    Tool,
    UnsubscribeRequest,
};
use turbomcp_transport::{Transport, TransportMessage};

use crate::handlers::{
    ElicitationHandler, HandlerRegistry, LogHandler, ProgressHandler, ResourceUpdateHandler,
};
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

        if let Some(error) = response.error() {
            return Err(Error::rpc(error.code, &error.message));
        }

        let result = response
            .result()
            .ok_or_else(|| Error::protocol("Response missing result field".to_string()))?;

        serde_json::from_value(result.clone())
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
    /// Handler registry for bidirectional communication
    handlers: HandlerRegistry,
    /// Plugin registry for middleware and extensibility
    plugin_registry: crate::plugins::PluginRegistry,
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

    /// Set the sampling handler for processing server-initiated sampling requests
    ///
    /// # Arguments
    ///
    /// * `handler` - The handler implementation for sampling requests
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::{Client, sampling::SamplingHandler};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult};
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct ExampleHandler;
    ///
    /// #[async_trait]
    /// impl SamplingHandler for ExampleHandler {
    ///     async fn handle_create_message(
    ///         &self,
    ///         _request: CreateMessageRequest,
    ///     ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
    ///         // Handle sampling request
    ///         todo!("Implement sampling logic")
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.set_sampling_handler(Arc::new(ExampleHandler));
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

    /// List all available tools from the MCP server
    ///
    /// Returns a list of complete tool definitions with schemas that can be used
    /// for form generation, validation, and documentation. Tools represent
    /// executable functions provided by the server.
    ///
    /// # Returns
    ///
    /// Returns a vector of complete `Tool` objects with schemas and metadata.
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
    ///     println!("Tool: {} - {}", tool.name, tool.description.as_deref().unwrap_or("No description"));
    ///     // Access full inputSchema for form generation
    ///     let schema = &tool.input_schema;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_tools(&mut self) -> Result<Vec<Tool>> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send tools/list request with plugin middleware
        let response: ListToolsResult = self.execute_with_plugins("tools/list", None).await?;
        Ok(response.tools) // Return full Tool objects with schemas
    }

    /// List available tool names from the MCP server
    ///
    /// Returns only the tool names for cases where full schemas are not needed.
    /// For most use cases, prefer `list_tools()` which provides complete tool definitions.
    ///
    /// # Returns
    ///
    /// Returns a vector of tool names available on the server.
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
    /// let tool_names = client.list_tool_names().await?;
    /// for name in tool_names {
    ///     println!("Available tool: {}", name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_tool_names(&mut self) -> Result<Vec<String>> {
        let tools = self.list_tools().await?;
        Ok(tools.into_iter().map(|tool| tool.name).collect())
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

        // 🎉 TurboMCP v1.0.7: Clean plugin execution with macro!
        let request_data = CallToolRequest {
            name: name.to_string(),
            arguments: Some(arguments.unwrap_or_default()),
            _meta: None,
        };

        with_plugins!(self, "tools/call", request_data, {
            // Core protocol call - plugins execute automatically around this
            let result: CallToolResult = self
                .protocol
                .request("tools/call", Some(serde_json::to_value(&request_data)?))
                .await?;

            Ok(self.extract_tool_content(&result))
        })
    }

    /// Execute a protocol method with plugin middleware
    ///
    /// This is a generic helper for wrapping protocol calls with plugin middleware.
    async fn execute_with_plugins<R>(
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

    /// Helper method to extract content from CallToolResult
    fn extract_tool_content(&self, response: &CallToolResult) -> serde_json::Value {
        // Extract content from response - for simplicity, return the first text content
        if let Some(content) = response.content.first() {
            match content {
                Content::Text(text_content) => serde_json::json!({
                    "text": text_content.text,
                    "is_error": response.is_error.unwrap_or(false)
                }),
                Content::Image(image_content) => serde_json::json!({
                    "image": image_content.data,
                    "mime_type": image_content.mime_type,
                    "is_error": response.is_error.unwrap_or(false)
                }),
                Content::Resource(resource_content) => serde_json::json!({
                    "resource": resource_content.resource,
                    "annotations": resource_content.annotations,
                    "is_error": response.is_error.unwrap_or(false)
                }),
                Content::Audio(audio_content) => serde_json::json!({
                    "audio": audio_content.data,
                    "mime_type": audio_content.mime_type,
                    "is_error": response.is_error.unwrap_or(false)
                }),
                Content::ResourceLink(resource_link) => serde_json::json!({
                    "resource_uri": resource_link.uri,
                    "is_error": response.is_error.unwrap_or(false)
                }),
            }
        } else {
            serde_json::json!({
                "message": "No content returned",
                "is_error": response.is_error.unwrap_or(false)
            })
        }
    }

    /// Request completion suggestions from the server
    ///
    /// # Arguments
    ///
    /// * `handler_name` - The completion handler name
    /// * `argument_value` - The partial value to complete
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
    /// let result = client.complete("complete_path", "/usr/b").await?;
    /// println!("Completions: {:?}", result.values);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn complete(
        &mut self,
        handler_name: &str,
        argument_value: &str,
    ) -> Result<turbomcp_protocol::types::CompletionResponse> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Create proper completion request using protocol types
        use turbomcp_protocol::types::{
            ArgumentInfo, CompleteRequestParams, CompletionReference, PromptReferenceData,
        };

        let request_params = CompleteRequestParams {
            argument: ArgumentInfo {
                name: "partial".to_string(),
                value: argument_value.to_string(),
            },
            reference: CompletionReference::Prompt(PromptReferenceData {
                name: handler_name.to_string(),
                title: None,
            }),
            context: None,
            _meta: None,
        };

        let serialized_params = serde_json::to_value(&request_params)?;

        with_plugins!(self, "completion/complete", serialized_params, {
            // Core protocol call - plugins execute automatically around this
            let result: CompleteResult = self
                .protocol
                .request("completion/complete", Some(serialized_params))
                .await?;

            Ok(result.completion)
        })
    }

    /// Complete a prompt argument with full MCP protocol support
    ///
    /// This method provides access to the complete MCP completion protocol,
    /// allowing specification of argument names, prompt references, and context.
    ///
    /// # Arguments
    ///
    /// * `prompt_name` - Name of the prompt to complete for
    /// * `argument_name` - Name of the argument being completed
    /// * `argument_value` - Current value for completion matching
    /// * `context` - Optional context with previously resolved arguments
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_protocol::types::CompletionContext;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Complete with context
    /// let mut context_args = HashMap::new();
    /// context_args.insert("language".to_string(), "rust".to_string());
    /// let context = CompletionContext { arguments: Some(context_args) };
    ///
    /// let completions = client.complete_prompt(
    ///     "code_review",
    ///     "framework",
    ///     "tok",
    ///     Some(context)
    /// ).await?;
    ///
    /// for completion in completions.values {
    ///     println!("Suggestion: {}", completion);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn complete_prompt(
        &mut self,
        prompt_name: &str,
        argument_name: &str,
        argument_value: &str,
        context: Option<turbomcp_protocol::types::CompletionContext>,
    ) -> Result<turbomcp_protocol::types::CompletionResponse> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        use turbomcp_protocol::types::{
            ArgumentInfo, CompleteRequestParams, CompletionReference, PromptReferenceData,
        };

        let request_params = CompleteRequestParams {
            argument: ArgumentInfo {
                name: argument_name.to_string(),
                value: argument_value.to_string(),
            },
            reference: CompletionReference::Prompt(PromptReferenceData {
                name: prompt_name.to_string(),
                title: None,
            }),
            context,
            _meta: None,
        };

        let serialized_params = serde_json::to_value(&request_params)?;

        with_plugins!(self, "completion/complete", serialized_params, {
            let result: CompleteResult = self
                .protocol
                .request("completion/complete", Some(serialized_params))
                .await?;

            Ok(result.completion)
        })
    }

    /// Complete a resource template URI with full MCP protocol support
    ///
    /// This method provides completion for resource template URIs, allowing
    /// servers to suggest values for URI template variables.
    ///
    /// # Arguments
    ///
    /// * `resource_uri` - Resource template URI (e.g., "/files/{path}")
    /// * `argument_name` - Name of the argument being completed
    /// * `argument_value` - Current value for completion matching
    /// * `context` - Optional context with previously resolved arguments
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
    /// let completions = client.complete_resource(
    ///     "/files/{path}",
    ///     "path",
    ///     "/home/user/doc",
    ///     None
    /// ).await?;
    ///
    /// for completion in completions.values {
    ///     println!("Path suggestion: {}", completion);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn complete_resource(
        &mut self,
        resource_uri: &str,
        argument_name: &str,
        argument_value: &str,
        context: Option<turbomcp_protocol::types::CompletionContext>,
    ) -> Result<turbomcp_protocol::types::CompletionResponse> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        use turbomcp_protocol::types::{
            ArgumentInfo, CompleteRequestParams, CompletionReference, ResourceTemplateReferenceData,
        };

        let request_params = CompleteRequestParams {
            argument: ArgumentInfo {
                name: argument_name.to_string(),
                value: argument_value.to_string(),
            },
            reference: CompletionReference::ResourceTemplate(ResourceTemplateReferenceData {
                uri: resource_uri.to_string(),
            }),
            context,
            _meta: None,
        };

        let serialized_params = serde_json::to_value(&request_params)?;

        with_plugins!(self, "completion/complete", serialized_params, {
            let result: CompleteResult = self
                .protocol
                .request("completion/complete", Some(serialized_params))
                .await?;

            Ok(result.completion)
        })
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

        // Execute with plugin middleware
        let response: ListResourcesResult =
            self.execute_with_plugins("resources/list", None).await?;

        let resource_uris = response
            .resources
            .into_iter()
            .map(|resource| resource.uri)
            .collect();
        Ok(resource_uris)
    }

    /// Send a ping request to check server health and connectivity
    ///
    /// Sends a ping request to the server to verify the connection is active
    /// and the server is responding. This is useful for health checks and
    /// connection validation.
    ///
    /// # Returns
    ///
    /// Returns `PingResult` on successful ping.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server is not responding
    /// - The connection has failed
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
    /// let result = client.ping().await?;
    /// println!("Server is responding");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn ping(&mut self) -> Result<PingResult> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send ping request with plugin middleware (no parameters needed)
        let response: PingResult = self.execute_with_plugins("ping", None).await?;
        Ok(response)
    }

    /// Read the content of a specific resource by URI
    ///
    /// Retrieves the content of a resource identified by its URI. Clients can
    /// access specific files, documents, or other resources
    /// provided by the server.
    ///
    /// # Arguments
    ///
    /// * `uri` - The URI of the resource to read
    ///
    /// # Returns
    ///
    /// Returns `ReadResourceResult` containing the resource content.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The URI is invalid or empty
    /// - The resource doesn't exist
    /// - Access to the resource is denied
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
    /// let result = client.read_resource("file:///path/to/document.txt").await?;
    /// for content in result.contents {
    ///     println!("Resource content: {:?}", content);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_resource(&mut self, uri: &str) -> Result<ReadResourceResult> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        if uri.is_empty() {
            return Err(Error::bad_request("Resource URI cannot be empty"));
        }

        // Send read_resource request
        let request = ReadResourceRequest {
            uri: uri.to_string(),
            _meta: None,
        };

        let response: ReadResourceResult = self
            .execute_with_plugins("resources/read", Some(serde_json::to_value(request)?))
            .await?;
        Ok(response)
    }

    /// List available prompt templates from the server
    ///
    /// Retrieves the complete list of prompt templates that the server provides,
    /// including all metadata: title, description, and argument schemas. This is
    /// the MCP-compliant implementation that provides everything needed for UI generation
    /// and dynamic form creation.
    ///
    /// # Returns
    ///
    /// Returns a vector of `Prompt` objects containing:
    /// - `name`: Programmatic identifier
    /// - `title`: Human-readable display name (optional)
    /// - `description`: Description of what the prompt does (optional)
    /// - `arguments`: Array of argument schemas with validation info (optional)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server doesn't support prompts
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
    /// let prompts = client.list_prompts().await?;
    /// for prompt in prompts {
    ///     println!("Prompt: {} ({})", prompt.name, prompt.title.unwrap_or("No title".to_string()));
    ///     if let Some(args) = prompt.arguments {
    ///         println!("  Arguments: {:?}", args);
    ///         for arg in args {
    ///             let required = arg.required.unwrap_or(false);
    ///             println!("    - {}: {} (required: {})", arg.name,
    ///                     arg.description.unwrap_or("No description".to_string()), required);
    ///         }
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_prompts(&mut self) -> Result<Vec<Prompt>> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Execute with plugin middleware - return full Prompt objects per MCP spec
        let response: ListPromptsResult = self.execute_with_plugins("prompts/list", None).await?;
        Ok(response.prompts)
    }

    /// Get a specific prompt template with argument support
    ///
    /// Retrieves a specific prompt template from the server with support for
    /// parameter substitution. When arguments are provided, the server will
    /// substitute them into the prompt template using {parameter} syntax.
    ///
    /// This is the MCP-compliant implementation that supports the full protocol specification.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the prompt to retrieve
    /// * `arguments` - Optional parameters for template substitution
    ///
    /// # Returns
    ///
    /// Returns `GetPromptResult` containing the prompt template with parameters substituted.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The prompt name is empty
    /// - The prompt doesn't exist
    /// - Required arguments are missing
    /// - Argument types don't match schema
    /// - The request fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_protocol::PromptInput;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Get prompt without arguments (template form)
    /// let template = client.get_prompt("greeting", None).await?;
    /// println!("Template has {} messages", template.messages.len());
    ///
    /// // Get prompt with arguments (substituted form)
    /// let mut args = HashMap::new();
    /// args.insert("name".to_string(), serde_json::Value::String("Alice".to_string()));
    /// args.insert("greeting".to_string(), serde_json::Value::String("Hello".to_string()));
    ///
    /// let result = client.get_prompt("greeting", Some(args)).await?;
    /// println!("Generated prompt with {} messages", result.messages.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_prompt(
        &mut self,
        name: &str,
        arguments: Option<PromptInput>,
    ) -> Result<GetPromptResult> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        if name.is_empty() {
            return Err(Error::bad_request("Prompt name cannot be empty"));
        }

        // Send prompts/get request with full argument support
        let request = GetPromptRequest {
            name: name.to_string(),
            arguments, // Support for parameter substitution
            _meta: None,
        };

        self.execute_with_plugins("prompts/get", Some(serde_json::to_value(request).unwrap()))
            .await
    }

    /// List available filesystem root directories
    ///
    /// Retrieves the list of root directories that the server has access to.
    /// This is useful for understanding what parts of the filesystem are
    /// available for resource access.
    ///
    /// # Returns
    ///
    /// Returns a vector of root directory URIs available on the server.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server doesn't support filesystem access
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
    /// let roots = client.list_roots().await?;
    /// for root_uri in roots {
    ///     println!("Available root: {}", root_uri);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_roots(&mut self) -> Result<Vec<String>> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send roots/list request with plugin middleware
        let response: ListRootsResult = self.execute_with_plugins("roots/list", None).await?;
        let root_uris = response.roots.into_iter().map(|root| root.uri).collect();
        Ok(root_uris)
    }

    /// Set the logging level for the server
    ///
    /// Controls the verbosity of server logging. Clients can
    /// adjust the amount of log information they receive from the server.
    ///
    /// # Arguments
    ///
    /// * `level` - The desired logging level (Error, Warn, Info, Debug)
    ///
    /// # Returns
    ///
    /// Returns `SetLevelResult` on successful level change.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server doesn't support logging control
    /// - The request fails
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::Client;
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_protocol::types::LogLevel;
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let mut client = Client::new(StdioTransport::new());
    /// client.initialize().await?;
    ///
    /// // Set server to debug logging
    /// client.set_log_level(LogLevel::Debug).await?;
    /// println!("Server logging level set to debug");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_log_level(&mut self, level: LogLevel) -> Result<SetLevelResult> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send logging/setLevel request
        let request = SetLevelRequest { level };

        let response: SetLevelResult = self
            .execute_with_plugins("logging/setLevel", Some(serde_json::to_value(request)?))
            .await?;
        Ok(response)
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
            Some(serde_json::to_value(request).unwrap()),
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
            Some(serde_json::to_value(request).unwrap()),
        )
        .await
    }

    /// List available resource templates
    ///
    /// Retrieves the list of resource templates that define URI patterns
    /// for accessing different types of resources. Templates help clients
    /// understand what resources are available and how to access them.
    ///
    /// # Returns
    ///
    /// Returns a vector of resource template URI patterns.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client is not initialized
    /// - The server doesn't support resource templates
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
    /// let templates = client.list_resource_templates().await?;
    /// for template in templates {
    ///     println!("Resource template: {}", template);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_resource_templates(&mut self) -> Result<Vec<String>> {
        if !self.initialized {
            return Err(Error::bad_request("Client not initialized"));
        }

        // Send resources/templates request with plugin middleware
        let response: ListResourceTemplatesResult = self
            .execute_with_plugins("resources/templates", None)
            .await?;
        let template_uris = response
            .resource_templates
            .into_iter()
            .map(|template| template.uri_template)
            .collect();
        Ok(template_uris)
    }

    // ============================================================================
    // HANDLER REGISTRATION METHODS
    // ============================================================================

    /// Register an elicitation handler for processing user input requests
    ///
    /// Elicitation handlers are called when the server needs user input during
    /// operations. The handler should present the request to the user and
    /// collect their response according to the provided schema.
    ///
    /// # Arguments
    ///
    /// * `handler` - The elicitation handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_client::handlers::{ElicitationHandler, ElicitationRequest, ElicitationResponse, ElicitationAction, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    /// use serde_json::json;
    ///
    /// #[derive(Debug)]
    /// struct MyElicitationHandler;
    ///
    /// #[async_trait]
    /// impl ElicitationHandler for MyElicitationHandler {
    ///     async fn handle_elicitation(
    ///         &self,
    ///         request: ElicitationRequest,
    ///     ) -> HandlerResult<ElicitationResponse> {
    ///         Ok(ElicitationResponse {
    ///             action: ElicitationAction::Accept,
    ///             content: Some(json!({"user_input": "example"})),
    ///         })
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.on_elicitation(Arc::new(MyElicitationHandler));
    /// ```
    pub fn on_elicitation(&mut self, handler: Arc<dyn ElicitationHandler>) {
        self.handlers.set_elicitation_handler(handler);
    }

    /// Register a progress handler for processing operation progress updates
    ///
    /// Progress handlers receive notifications about long-running server operations.
    /// Display progress bars, status updates, or other
    /// feedback to users.
    ///
    /// # Arguments
    ///
    /// * `handler` - The progress handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_client::handlers::{ProgressHandler, ProgressNotification, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct MyProgressHandler;
    ///
    /// #[async_trait]
    /// impl ProgressHandler for MyProgressHandler {
    ///     async fn handle_progress(&self, notification: ProgressNotification) -> HandlerResult<()> {
    ///         println!("Progress: {:?}", notification);
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.on_progress(Arc::new(MyProgressHandler));
    /// ```
    pub fn on_progress(&mut self, handler: Arc<dyn ProgressHandler>) {
        self.handlers.set_progress_handler(handler);
    }

    /// Register a log handler for processing server log messages
    ///
    /// Log handlers receive log messages from the server and can route them
    /// to the client's logging system. This is useful for debugging and
    /// maintaining a unified log across client and server.
    ///
    /// # Arguments
    ///
    /// * `handler` - The log handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_client::handlers::{LogHandler, LogMessage, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct MyLogHandler;
    ///
    /// #[async_trait]
    /// impl LogHandler for MyLogHandler {
    ///     async fn handle_log(&self, log: LogMessage) -> HandlerResult<()> {
    ///         println!("Server log: {}", log.message);
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.on_log(Arc::new(MyLogHandler));
    /// ```
    pub fn on_log(&mut self, handler: Arc<dyn LogHandler>) {
        self.handlers.set_log_handler(handler);
    }

    /// Register a resource update handler for processing resource change notifications
    ///
    /// Resource update handlers receive notifications when subscribed resources
    /// change on the server. Supports reactive updates to cached data or
    /// UI refreshes when server-side resources change.
    ///
    /// # Arguments
    ///
    /// * `handler` - The resource update handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::Client;
    /// use turbomcp_client::handlers::{ResourceUpdateHandler, ResourceUpdateNotification, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct MyResourceUpdateHandler;
    ///
    /// #[async_trait]
    /// impl ResourceUpdateHandler for MyResourceUpdateHandler {
    ///     async fn handle_resource_update(
    ///         &self,
    ///         notification: ResourceUpdateNotification,
    ///     ) -> HandlerResult<()> {
    ///         println!("Resource updated: {}", notification.uri);
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let mut client = Client::new(StdioTransport::new());
    /// client.on_resource_update(Arc::new(MyResourceUpdateHandler));
    /// ```
    pub fn on_resource_update(&mut self, handler: Arc<dyn ResourceUpdateHandler>) {
        self.handlers.set_resource_update_handler(handler);
    }

    /// Check if an elicitation handler is registered
    pub fn has_elicitation_handler(&self) -> bool {
        self.handlers.has_elicitation_handler()
    }

    /// Check if a progress handler is registered
    pub fn has_progress_handler(&self) -> bool {
        self.handlers.has_progress_handler()
    }

    /// Check if a log handler is registered
    pub fn has_log_handler(&self) -> bool {
        self.handlers.has_log_handler()
    }

    /// Check if a resource update handler is registered
    pub fn has_resource_update_handler(&self) -> bool {
        self.handlers.has_resource_update_handler()
    }

    /// Get the client's capabilities configuration
    pub fn capabilities(&self) -> &ClientCapabilities {
        &self.capabilities
    }

    // ============================================================================
    // PLUGIN MANAGEMENT
    // ============================================================================

    /// Register a plugin with the client
    ///
    /// # Arguments
    ///
    /// * `plugin` - The plugin to register
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::plugins::{MetricsPlugin, PluginConfig};
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let mut client = turbomcp_client::Client::new(turbomcp_transport::stdio::StdioTransport::new());
    /// let metrics_plugin = Arc::new(MetricsPlugin::new(PluginConfig::Metrics));
    /// client.register_plugin(metrics_plugin).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn register_plugin(
        &mut self,
        plugin: std::sync::Arc<dyn crate::plugins::ClientPlugin>,
    ) -> Result<()> {
        self.plugin_registry
            .register_plugin(plugin)
            .await
            .map_err(|e| Error::bad_request(format!("Failed to register plugin: {}", e)))
    }

    /// Check if a plugin is registered
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin to check
    pub fn has_plugin(&self, name: &str) -> bool {
        self.plugin_registry.has_plugin(name)
    }

    /// Get plugin data for a specific plugin type
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::plugins::MetricsPlugin;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = turbomcp_client::Client::new(turbomcp_transport::stdio::StdioTransport::new());
    /// if let Some(plugin) = client.get_plugin("metrics") {
    ///     // Use plugin data
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn get_plugin(
        &self,
        name: &str,
    ) -> Option<std::sync::Arc<dyn crate::plugins::ClientPlugin>> {
        self.plugin_registry.get_plugin(name)
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
            serde_json::json!(self.sampling_handler.is_some()),
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
}

/// Thread-safe wrapper for sharing Client across async tasks
///
/// This wrapper encapsulates the Arc/Mutex complexity and provides a clean API
/// for concurrent access to MCP client functionality. It addresses the limitations
/// identified in PR feedback where Client requires `&mut self` for all operations
/// but needs to be shared across multiple async tasks.
///
/// # Design Rationale
///
/// All Client methods require `&mut self` because:
/// - MCP connections maintain state (initialized flag, connection status)
/// - Request correlation tracking for JSON-RPC requires mutation
/// - Handler and plugin registries need mutable access
///
/// While Client implements Send + Sync, this only means it's safe to move/share
/// between threads, not that multiple tasks can mutate it concurrently.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::{Client, SharedClient};
/// use turbomcp_transport::stdio::StdioTransport;
///
/// # async fn example() -> turbomcp_core::Result<()> {
/// let transport = StdioTransport::new();
/// let client = Client::new(transport);
/// let shared = SharedClient::new(client);
///
/// // Initialize once
/// shared.initialize().await?;
///
/// // Clone for sharing across tasks
/// let shared1 = shared.clone();
/// let shared2 = shared.clone();
///
/// // Both tasks can use the client concurrently
/// let handle1 = tokio::spawn(async move {
///     shared1.list_tools().await
/// });
///
/// let handle2 = tokio::spawn(async move {
///     shared2.list_prompts().await
/// });
///
/// let (tools, prompts) = tokio::try_join!(handle1, handle2).unwrap();
/// # Ok(())
/// # }
/// ```
pub struct SharedClient<T: Transport> {
    inner: Arc<Mutex<Client<T>>>,
}

impl<T: Transport> SharedClient<T> {
    /// Create a new shared client wrapper
    ///
    /// Takes ownership of a Client and wraps it for thread-safe sharing.
    /// The original client can no longer be accessed directly after this call.
    pub fn new(client: Client<T>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(client)),
        }
    }

    /// Initialize the MCP connection
    ///
    /// This method should be called once before using any other client operations.
    /// It negotiates capabilities with the server and establishes the communication protocol.
    pub async fn initialize(&self) -> Result<InitializeResult> {
        self.inner.lock().await.initialize().await
    }

    /// List all available tools from the MCP server
    ///
    /// Returns a list of complete tool definitions with schemas that can be used
    /// for form generation, validation, and documentation. Tools represent
    /// executable functions provided by the server.
    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        self.inner.lock().await.list_tools().await
    }

    /// List available tool names from the MCP server
    ///
    /// Returns only the tool names for cases where full schemas are not needed.
    /// For most use cases, prefer `list_tools()` which provides complete tool definitions.
    pub async fn list_tool_names(&self) -> Result<Vec<String>> {
        self.inner.lock().await.list_tool_names().await
    }

    /// Execute a tool with the given name and arguments
    ///
    /// Calls a specific tool on the MCP server with the provided arguments.
    /// The arguments should match the tool's expected parameter schema.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<serde_json::Value> {
        self.inner.lock().await.call_tool(name, arguments).await
    }

    /// List all available prompts from the MCP server
    ///
    /// Returns full Prompt objects with metadata including name, title, description,
    /// and argument schemas. This information can be used to generate UI forms
    /// for prompt parameter collection.
    pub async fn list_prompts(&self) -> Result<Vec<Prompt>> {
        self.inner.lock().await.list_prompts().await
    }

    /// Get a prompt with optional argument substitution
    ///
    /// Retrieves a prompt from the server. If arguments are provided, template
    /// parameters (e.g., `{parameter}`) will be substituted with the given values.
    /// Pass `None` for arguments to get the raw template form.
    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<PromptInput>,
    ) -> Result<GetPromptResult> {
        self.inner.lock().await.get_prompt(name, arguments).await
    }

    /// List available resources from the MCP server
    ///
    /// Resources represent data or content that can be read by the client.
    /// Returns a list of resource identifiers and metadata.
    pub async fn list_resources(&self) -> Result<Vec<String>> {
        self.inner.lock().await.list_resources().await
    }

    /// Read a specific resource from the MCP server
    ///
    /// Retrieves the content of a resource identified by its URI.
    /// The content format depends on the specific resource type.
    pub async fn read_resource(&self, uri: &str) -> Result<ReadResourceResult> {
        self.inner.lock().await.read_resource(uri).await
    }

    /// List resource templates from the MCP server
    ///
    /// Resource templates define patterns for generating resource URIs.
    /// They allow servers to describe families of related resources.
    pub async fn list_resource_templates(&self) -> Result<Vec<String>> {
        self.inner.lock().await.list_resource_templates().await
    }

    /// Set the logging level for the MCP server
    ///
    /// Controls the verbosity of logs sent from the server to the client.
    /// Higher log levels provide more detailed information.
    pub async fn set_log_level(&self, level: LogLevel) -> Result<SetLevelResult> {
        self.inner.lock().await.set_log_level(level).await
    }

    /// Subscribe to notifications from a specific URI
    ///
    /// Registers interest in receiving notifications when the specified
    /// resource or endpoint changes. Used for real-time updates.
    pub async fn subscribe(&self, uri: &str) -> Result<EmptyResult> {
        self.inner.lock().await.subscribe(uri).await
    }

    /// Unsubscribe from notifications for a specific URI
    ///
    /// Removes a previously registered subscription to stop receiving
    /// notifications for the specified resource or endpoint.
    pub async fn unsubscribe(&self, uri: &str) -> Result<EmptyResult> {
        self.inner.lock().await.unsubscribe(uri).await
    }

    /// Send a ping to test connection health
    ///
    /// Verifies that the MCP connection is still active and responsive.
    /// Used for health checking and keepalive functionality.
    pub async fn ping(&self) -> Result<PingResult> {
        self.inner.lock().await.ping().await
    }

    /// Get the client's configured capabilities
    ///
    /// Returns the capabilities that this client supports.
    /// These are negotiated during initialization.
    pub async fn capabilities(&self) -> ClientCapabilities {
        let client = self.inner.lock().await;
        client.capabilities().clone()
    }

    /// Request argument completion from the MCP server
    ///
    /// Provides autocompletion suggestions for prompt arguments and resource URIs.
    /// Supports rich, IDE-like experiences with contextual suggestions.
    ///
    /// # Arguments
    ///
    /// * `handler_name` - The completion handler name
    /// * `argument_value` - The partial value to complete
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::{Client, SharedClient};
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let shared = SharedClient::new(Client::new(StdioTransport::new()));
    /// shared.initialize().await?;
    ///
    /// let result = shared.complete("complete_path", "/usr/b").await?;
    /// println!("Completions: {:?}", result.values);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn complete(
        &self,
        handler_name: &str,
        argument_value: &str,
    ) -> Result<turbomcp_protocol::types::CompletionResponse> {
        self.inner
            .lock()
            .await
            .complete(handler_name, argument_value)
            .await
    }

    /// Complete a prompt argument with full MCP protocol support
    ///
    /// This method provides access to the complete MCP completion protocol,
    /// allowing specification of argument names, prompt references, and context.
    ///
    /// # Arguments
    ///
    /// * `prompt_name` - Name of the prompt to complete for
    /// * `argument_name` - Name of the argument being completed
    /// * `argument_value` - Current value for completion matching
    /// * `context` - Optional context with previously resolved arguments
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::{Client, SharedClient};
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # use turbomcp_protocol::types::CompletionContext;
    /// # use std::collections::HashMap;
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let shared = SharedClient::new(Client::new(StdioTransport::new()));
    /// shared.initialize().await?;
    ///
    /// // Complete with context
    /// let mut context_args = HashMap::new();
    /// context_args.insert("language".to_string(), "rust".to_string());
    /// let context = CompletionContext { arguments: Some(context_args) };
    ///
    /// let completions = shared.complete_prompt(
    ///     "code_review",
    ///     "framework",
    ///     "tok",
    ///     Some(context)
    /// ).await?;
    ///
    /// for completion in completions.values {
    ///     println!("Suggestion: {}", completion);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn complete_prompt(
        &self,
        prompt_name: &str,
        argument_name: &str,
        argument_value: &str,
        context: Option<turbomcp_protocol::types::CompletionContext>,
    ) -> Result<turbomcp_protocol::types::CompletionResponse> {
        self.inner
            .lock()
            .await
            .complete_prompt(prompt_name, argument_name, argument_value, context)
            .await
    }

    /// Complete a resource template URI with full MCP protocol support
    ///
    /// This method provides completion for resource template URIs, allowing
    /// servers to suggest values for URI template variables.
    ///
    /// # Arguments
    ///
    /// * `resource_uri` - Resource template URI (e.g., "/files/{path}")
    /// * `argument_name` - Name of the argument being completed
    /// * `argument_value` - Current value for completion matching
    /// * `context` - Optional context with previously resolved arguments
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::{Client, SharedClient};
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let shared = SharedClient::new(Client::new(StdioTransport::new()));
    /// shared.initialize().await?;
    ///
    /// let completions = shared.complete_resource(
    ///     "/files/{path}",
    ///     "path",
    ///     "/home/user/doc",
    ///     None
    /// ).await?;
    ///
    /// for completion in completions.values {
    ///     println!("Path suggestion: {}", completion);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn complete_resource(
        &self,
        resource_uri: &str,
        argument_name: &str,
        argument_value: &str,
        context: Option<turbomcp_protocol::types::CompletionContext>,
    ) -> Result<turbomcp_protocol::types::CompletionResponse> {
        self.inner
            .lock()
            .await
            .complete_resource(resource_uri, argument_name, argument_value, context)
            .await
    }

    /// List filesystem roots available to the server
    ///
    /// Returns filesystem root directories that the server has access to.
    /// This helps servers understand their operating boundaries and available
    /// resources within the filesystem.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use turbomcp_client::{Client, SharedClient};
    /// # use turbomcp_transport::stdio::StdioTransport;
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let shared = SharedClient::new(Client::new(StdioTransport::new()));
    /// shared.initialize().await?;
    ///
    /// let roots = shared.list_roots().await?;
    /// for root_uri in roots {
    ///     println!("Available root: {}", root_uri);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_roots(&self) -> Result<Vec<String>> {
        self.inner.lock().await.list_roots().await
    }

    /// Register an elicitation handler for processing server requests for user information
    ///
    /// Elicitation handlers respond to server requests for additional information
    /// from users during interactions. Supports interactive workflows where
    /// servers can gather necessary information dynamically.
    ///
    /// # Arguments
    ///
    /// * `handler` - The elicitation handler implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::{Client, SharedClient};
    /// use turbomcp_client::handlers::{ElicitationHandler, ElicitationRequest, ElicitationResponse, ElicitationAction, HandlerResult};
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use async_trait::async_trait;
    /// use std::sync::Arc;
    ///
    /// #[derive(Debug)]
    /// struct MyElicitationHandler;
    ///
    /// #[async_trait]
    /// impl ElicitationHandler for MyElicitationHandler {
    ///     async fn handle_elicitation(&self, request: ElicitationRequest) -> HandlerResult<ElicitationResponse> {
    ///         // Process user input request and return response
    ///         Ok(ElicitationResponse {
    ///             action: ElicitationAction::Accept,
    ///             content: Some(serde_json::json!({"name": "example"})),
    ///         })
    ///     }
    /// }
    ///
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let shared = SharedClient::new(Client::new(StdioTransport::new()));
    /// shared.on_elicitation(Arc::new(MyElicitationHandler)).await;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn on_elicitation(&self, handler: Arc<dyn crate::handlers::ElicitationHandler>) {
        self.inner.lock().await.on_elicitation(handler);
    }

    /// Register a progress handler for processing server progress notifications
    ///
    /// Progress handlers receive updates about long-running operations on the server.
    /// Provides progress bars, status updates, and better user experience during
    /// extended operations.
    ///
    /// # Arguments
    ///
    /// * `handler` - The progress handler implementation
    pub async fn on_progress(&self, handler: Arc<dyn crate::handlers::ProgressHandler>) {
        self.inner.lock().await.on_progress(handler);
    }

    /// Register a log handler for processing server log messages
    ///
    /// Log handlers receive log messages from the server and can route them
    /// to the client's logging system. This is useful for debugging and
    /// maintaining a unified log across client and server.
    ///
    /// # Arguments
    ///
    /// * `handler` - The log handler implementation
    pub async fn on_log(&self, handler: Arc<dyn crate::handlers::LogHandler>) {
        self.inner.lock().await.on_log(handler);
    }

    /// Register a resource update handler for processing resource change notifications
    ///
    /// Resource update handlers receive notifications when subscribed resources
    /// change on the server. Supports reactive updates to cached data or
    /// UI refreshes when server-side resources change.
    ///
    /// # Arguments
    ///
    /// * `handler` - The resource update handler implementation
    pub async fn on_resource_update(
        &self,
        handler: Arc<dyn crate::handlers::ResourceUpdateHandler>,
    ) {
        self.inner.lock().await.on_resource_update(handler);
    }

    /// Check if an elicitation handler is registered
    pub async fn has_elicitation_handler(&self) -> bool {
        self.inner.lock().await.has_elicitation_handler()
    }

    /// Check if a progress handler is registered
    pub async fn has_progress_handler(&self) -> bool {
        self.inner.lock().await.has_progress_handler()
    }

    /// Check if a log handler is registered
    pub async fn has_log_handler(&self) -> bool {
        self.inner.lock().await.has_log_handler()
    }

    /// Check if a resource update handler is registered
    pub async fn has_resource_update_handler(&self) -> bool {
        self.inner.lock().await.has_resource_update_handler()
    }
}

impl<T: Transport> Clone for SharedClient<T> {
    /// Clone the shared client for use in multiple async tasks
    ///
    /// This creates a new reference to the same underlying client,
    /// allowing multiple tasks to share access safely.
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
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

/// Connection configuration for the client
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// Request timeout in milliseconds
    pub timeout_ms: u64,

    /// Maximum number of retry attempts
    pub max_retries: u32,

    /// Retry delay in milliseconds
    pub retry_delay_ms: u64,

    /// Keep-alive interval in milliseconds
    pub keepalive_ms: u64,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000,    // 30 seconds
            max_retries: 3,        // 3 attempts
            retry_delay_ms: 1_000, // 1 second
            keepalive_ms: 60_000,  // 60 seconds
        }
    }
}

/// Builder for configuring and creating MCP clients
///
/// Provides a fluent interface for configuring client options before creation.
/// The enhanced builder pattern supports comprehensive configuration including:
/// - Protocol capabilities
/// - Plugin registration
/// - LLM provider configuration
/// - Handler registration
/// - Connection settings
/// - Session management
///
/// # Examples
///
/// Basic usage:
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
///
/// Advanced configuration:
/// ```rust,no_run
/// use turbomcp_client::{ClientBuilder, ConnectionConfig};
/// use turbomcp_client::plugins::{MetricsPlugin, PluginConfig};
/// use turbomcp_client::llm::{OpenAIProvider, LLMProviderConfig};
/// use turbomcp_transport::stdio::StdioTransport;
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let client = ClientBuilder::new()
///     .with_tools(true)
///     .with_prompts(true)
///     .with_resources(true)
///     .with_sampling(true)
///     .with_connection_config(ConnectionConfig {
///         timeout_ms: 60_000,
///         max_retries: 5,
///         retry_delay_ms: 2_000,
///         keepalive_ms: 30_000,
///     })
///     .with_plugin(Arc::new(MetricsPlugin::new(PluginConfig::Metrics)))
///     .with_llm_provider("openai", Arc::new(OpenAIProvider::new(LLMProviderConfig {
///         api_key: std::env::var("OPENAI_API_KEY")?,
///         model: "gpt-4".to_string(),
///         ..Default::default()
///     })?))
///     .build(StdioTransport::new())
///     .await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default)]
pub struct ClientBuilder {
    capabilities: ClientCapabilities,
    connection_config: ConnectionConfig,
    plugins: Vec<Arc<dyn crate::plugins::ClientPlugin>>,
    llm_providers: HashMap<String, Arc<dyn crate::llm::LLMProvider>>,
    elicitation_handler: Option<Arc<dyn crate::handlers::ElicitationHandler>>,
    progress_handler: Option<Arc<dyn crate::handlers::ProgressHandler>>,
    log_handler: Option<Arc<dyn crate::handlers::LogHandler>>,
    resource_update_handler: Option<Arc<dyn crate::handlers::ResourceUpdateHandler>>,
    session_config: Option<crate::llm::SessionConfig>,
}

// Default implementation is now derived

impl ClientBuilder {
    /// Create a new client builder
    ///
    /// Returns a new builder with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    // ============================================================================
    // CAPABILITY CONFIGURATION
    // ============================================================================

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

    /// Configure all capabilities at once
    ///
    /// # Arguments
    ///
    /// * `capabilities` - The capabilities configuration
    pub fn with_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    // ============================================================================
    // CONNECTION CONFIGURATION
    // ============================================================================

    /// Configure connection settings
    ///
    /// # Arguments
    ///
    /// * `config` - The connection configuration
    pub fn with_connection_config(mut self, config: ConnectionConfig) -> Self {
        self.connection_config = config;
        self
    }

    /// Set request timeout
    ///
    /// # Arguments
    ///
    /// * `timeout_ms` - Timeout in milliseconds
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.connection_config.timeout_ms = timeout_ms;
        self
    }

    /// Set maximum retry attempts
    ///
    /// # Arguments
    ///
    /// * `max_retries` - Maximum number of retries
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.connection_config.max_retries = max_retries;
        self
    }

    /// Set retry delay
    ///
    /// # Arguments
    ///
    /// * `delay_ms` - Retry delay in milliseconds
    pub fn with_retry_delay(mut self, delay_ms: u64) -> Self {
        self.connection_config.retry_delay_ms = delay_ms;
        self
    }

    /// Set keep-alive interval
    ///
    /// # Arguments
    ///
    /// * `interval_ms` - Keep-alive interval in milliseconds
    pub fn with_keepalive(mut self, interval_ms: u64) -> Self {
        self.connection_config.keepalive_ms = interval_ms;
        self
    }

    // ============================================================================
    // PLUGIN SYSTEM CONFIGURATION
    // ============================================================================

    /// Register a plugin with the client
    ///
    /// Plugins provide middleware functionality for request/response processing,
    /// metrics collection, retry logic, caching, and other cross-cutting concerns.
    ///
    /// # Arguments
    ///
    /// * `plugin` - The plugin implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::{ClientBuilder, ConnectionConfig};
    /// use turbomcp_client::plugins::{MetricsPlugin, RetryPlugin, PluginConfig, RetryConfig};
    /// use std::sync::Arc;
    ///
    /// let client = ClientBuilder::new()
    ///     .with_plugin(Arc::new(MetricsPlugin::new(PluginConfig::Metrics)))
    ///     .with_plugin(Arc::new(RetryPlugin::new(PluginConfig::Retry(RetryConfig {
    ///         max_retries: 5,
    ///         base_delay_ms: 1000,
    ///         max_delay_ms: 30000,
    ///         backoff_multiplier: 2.0,
    ///         retry_on_timeout: true,
    ///         retry_on_connection_error: true,
    ///     }))));
    /// ```
    pub fn with_plugin(mut self, plugin: Arc<dyn crate::plugins::ClientPlugin>) -> Self {
        self.plugins.push(plugin);
        self
    }

    /// Register multiple plugins at once
    ///
    /// # Arguments
    ///
    /// * `plugins` - Vector of plugin implementations
    pub fn with_plugins(mut self, plugins: Vec<Arc<dyn crate::plugins::ClientPlugin>>) -> Self {
        self.plugins.extend(plugins);
        self
    }

    // ============================================================================
    // LLM PROVIDER CONFIGURATION
    // ============================================================================

    /// Register an LLM provider
    ///
    /// LLM providers handle server-initiated sampling requests by forwarding them
    /// to language model services like OpenAI, Anthropic, or local models.
    ///
    /// # Arguments
    ///
    /// * `name` - Unique name for the provider
    /// * `provider` - The LLM provider implementation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_client::llm::{OpenAIProvider, AnthropicProvider, LLMProviderConfig};
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    /// let client = ClientBuilder::new()
    ///     .with_llm_provider("openai", Arc::new(OpenAIProvider::new(LLMProviderConfig {
    ///         api_key: std::env::var("OPENAI_API_KEY")?,
    ///         model: "gpt-4".to_string(),
    ///         ..Default::default()
    ///     })?))
    ///     .with_llm_provider("anthropic", Arc::new(AnthropicProvider::new(LLMProviderConfig {
    ///         api_key: std::env::var("ANTHROPIC_API_KEY")?,
    ///         model: "claude-3-5-sonnet-20241022".to_string(),
    ///         ..Default::default()
    ///     })?));
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_llm_provider(
        mut self,
        name: impl Into<String>,
        provider: Arc<dyn crate::llm::LLMProvider>,
    ) -> Self {
        self.llm_providers.insert(name.into(), provider);
        self
    }

    /// Register multiple LLM providers at once
    ///
    /// # Arguments
    ///
    /// * `providers` - Map of provider names to implementations
    pub fn with_llm_providers(
        mut self,
        providers: HashMap<String, Arc<dyn crate::llm::LLMProvider>>,
    ) -> Self {
        self.llm_providers.extend(providers);
        self
    }

    /// Configure session management for conversations
    ///
    /// # Arguments
    ///
    /// * `config` - Session configuration for conversation tracking
    pub fn with_session_config(mut self, config: crate::llm::SessionConfig) -> Self {
        self.session_config = Some(config);
        self
    }

    // ============================================================================
    // HANDLER REGISTRATION
    // ============================================================================

    /// Register an elicitation handler for processing user input requests
    ///
    /// # Arguments
    ///
    /// * `handler` - The elicitation handler implementation
    pub fn with_elicitation_handler(
        mut self,
        handler: Arc<dyn crate::handlers::ElicitationHandler>,
    ) -> Self {
        self.elicitation_handler = Some(handler);
        self
    }

    /// Register a progress handler for processing operation progress updates
    ///
    /// # Arguments
    ///
    /// * `handler` - The progress handler implementation
    pub fn with_progress_handler(
        mut self,
        handler: Arc<dyn crate::handlers::ProgressHandler>,
    ) -> Self {
        self.progress_handler = Some(handler);
        self
    }

    /// Register a log handler for processing server log messages
    ///
    /// # Arguments
    ///
    /// * `handler` - The log handler implementation
    pub fn with_log_handler(mut self, handler: Arc<dyn crate::handlers::LogHandler>) -> Self {
        self.log_handler = Some(handler);
        self
    }

    /// Register a resource update handler for processing resource change notifications
    ///
    /// # Arguments
    ///
    /// * `handler` - The resource update handler implementation
    pub fn with_resource_update_handler(
        mut self,
        handler: Arc<dyn crate::handlers::ResourceUpdateHandler>,
    ) -> Self {
        self.resource_update_handler = Some(handler);
        self
    }

    // ============================================================================
    // BUILD METHODS
    // ============================================================================

    /// Build a client with the configured options
    ///
    /// Creates a new client instance with all the configured options. The client
    /// will be initialized with the registered plugins, handlers, and providers.
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport to use for the client
    ///
    /// # Returns
    ///
    /// Returns a configured `Client` instance wrapped in a Result for async setup.
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
    ///     .build(StdioTransport::new())
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn build<T: Transport>(self, transport: T) -> Result<Client<T>> {
        // Create base client with capabilities
        let mut client = Client::with_capabilities(transport, self.capabilities);

        // Register handlers
        if let Some(handler) = self.elicitation_handler {
            client.on_elicitation(handler);
        }
        if let Some(handler) = self.progress_handler {
            client.on_progress(handler);
        }
        if let Some(handler) = self.log_handler {
            client.on_log(handler);
        }
        if let Some(handler) = self.resource_update_handler {
            client.on_resource_update(handler);
        }

        // Set up LLM providers if any are configured
        if !self.llm_providers.is_empty() {
            // Create LLM registry and register providers
            let mut registry = crate::llm::LLMRegistry::new();
            for (name, provider) in self.llm_providers {
                registry
                    .register_provider(&name, provider)
                    .await
                    .map_err(|e| {
                        Error::configuration(format!(
                            "Failed to register LLM provider '{}': {}",
                            name, e
                        ))
                    })?;
            }

            // Configure session management if provided
            if let Some(session_config) = self.session_config {
                registry
                    .configure_sessions(session_config)
                    .await
                    .map_err(|e| {
                        Error::configuration(format!("Failed to configure sessions: {}", e))
                    })?;
            }

            // Set up the registry as the sampling handler
            let sampling_handler = Arc::new(registry);
            client.set_sampling_handler(sampling_handler);
        }

        // Apply connection configuration (store for future use in actual connections)
        // Note: The current Client doesn't expose connection config setters,
        // so we'll store this for when the transport supports it

        // Register plugins with the client
        let has_plugins = !self.plugins.is_empty();
        for plugin in self.plugins {
            client.register_plugin(plugin).await.map_err(|e| {
                Error::bad_request(format!("Failed to register plugin during build: {}", e))
            })?;
        }

        // Initialize plugins after registration
        if has_plugins {
            client.initialize_plugins().await.map_err(|e| {
                Error::bad_request(format!("Failed to initialize plugins during build: {}", e))
            })?;
        }

        Ok(client)
    }

    /// Build a client synchronously with basic configuration only
    ///
    /// This is a convenience method for simple use cases where no async setup
    /// is required. For advanced features like LLM providers, use `build()` instead.
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
    ///     .build_sync(StdioTransport::new());
    /// ```
    pub fn build_sync<T: Transport>(self, transport: T) -> Client<T> {
        let mut client = Client::with_capabilities(transport, self.capabilities);

        // Register synchronous handlers only
        if let Some(handler) = self.elicitation_handler {
            client.on_elicitation(handler);
        }
        if let Some(handler) = self.progress_handler {
            client.on_progress(handler);
        }
        if let Some(handler) = self.log_handler {
            client.on_log(handler);
        }
        if let Some(handler) = self.resource_update_handler {
            client.on_resource_update(handler);
        }

        client
    }

    // ============================================================================
    // CONFIGURATION ACCESS
    // ============================================================================

    /// Get the current capabilities configuration
    pub fn capabilities(&self) -> &ClientCapabilities {
        &self.capabilities
    }

    /// Get the current connection configuration
    pub fn connection_config(&self) -> &ConnectionConfig {
        &self.connection_config
    }

    /// Get the number of registered plugins
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Get the number of registered LLM providers
    pub fn llm_provider_count(&self) -> usize {
        self.llm_providers.len()
    }

    /// Check if any handlers are registered
    pub fn has_handlers(&self) -> bool {
        self.elicitation_handler.is_some()
            || self.progress_handler.is_some()
            || self.log_handler.is_some()
            || self.resource_update_handler.is_some()
    }
}

// Re-export types for public API
pub use turbomcp_protocol::types::ServerCapabilities as PublicServerCapabilities;

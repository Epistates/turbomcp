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
//! use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult, Role, Content, TextContent};
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
//!             role: Role::Assistant,
//!             content: Content::Text(
//!                 TextContent {
//!                     text: "Response from LLM".to_string(),
//!                     annotations: None,
//!                     meta: None,
//!                 }
//!             ),
//!             model: "gpt-4".to_string(),
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

pub mod client;
pub mod handlers;
pub mod llm;
pub mod plugins;
pub mod sampling;

// Re-export key types for convenience
pub use client::{ConnectionInfo, ConnectionState, ManagerConfig, ServerGroup, SessionManager};

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use turbomcp_core::{Error, Result};
use turbomcp_protocol::types::{
    EmptyResult, GetPromptResult, LogLevel, PingResult, Prompt, PromptInput, ReadResourceResult,
    SetLevelResult, Tool,
};
use turbomcp_transport::Transport;

// Note: Handler types are now used only in client/operations modules

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
// Note: ProtocolClient implementation moved to client/protocol.rs for better modularity
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
// Re-export Client from the core module
pub use client::core::Client;

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
    /// println!("Completions: {:?}", result.completion);
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
    /// for completion in completions.completion.values {
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
    /// for completion in completions.completion.values {
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

    // Note: roots/list is a SERVER->CLIENT request per MCP 2025-06-18 specification.
    // The server asks the client for its filesystem roots, not the other way around.
    // Clients should implement a roots handler to respond to server requests.
    // See: ServerRequest = PingRequest | CreateMessageRequest | ListRootsRequest | ElicitRequest

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
// Re-export InitializeResult from config module
pub use client::config::InitializeResult;

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
    // Robustness configuration
    enable_resilience: bool,
    retry_config: Option<turbomcp_transport::resilience::RetryConfig>,
    circuit_breaker_config: Option<turbomcp_transport::resilience::CircuitBreakerConfig>,
    health_check_config: Option<turbomcp_transport::resilience::HealthCheckConfig>,
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
    // ROBUSTNESS & RESILIENCE CONFIGURATION
    // ============================================================================

    /// Enable resilient transport with circuit breaker, retry, and health checking
    ///
    /// When enabled, the transport layer will automatically:
    /// - Retry failed operations with exponential backoff
    /// - Use circuit breaker pattern to prevent cascade failures
    /// - Perform periodic health checks
    /// - Deduplicate messages
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::stdio::StdioTransport;
    ///
    /// let client = ClientBuilder::new()
    ///     .enable_resilience()
    ///     .build(StdioTransport::new());
    /// ```
    pub fn enable_resilience(mut self) -> Self {
        self.enable_resilience = true;
        self
    }

    /// Configure retry behavior for resilient transport
    ///
    /// # Arguments
    ///
    /// * `config` - Retry configuration
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::resilience::RetryConfig;
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use std::time::Duration;
    ///
    /// let client = ClientBuilder::new()
    ///     .enable_resilience()
    ///     .with_retry_config(RetryConfig {
    ///         max_attempts: 5,
    ///         base_delay: Duration::from_millis(100),
    ///         max_delay: Duration::from_secs(30),
    ///         backoff_multiplier: 2.0,
    ///         jitter_factor: 0.1,
    ///         retry_on_connection_error: true,
    ///         retry_on_timeout: true,
    ///         custom_retry_conditions: Vec::new(),
    ///     })
    ///     .build(StdioTransport::new());
    /// ```
    pub fn with_retry_config(
        mut self,
        config: turbomcp_transport::resilience::RetryConfig,
    ) -> Self {
        self.retry_config = Some(config);
        self.enable_resilience = true; // Auto-enable resilience
        self
    }

    /// Configure circuit breaker for resilient transport
    ///
    /// # Arguments
    ///
    /// * `config` - Circuit breaker configuration
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::resilience::CircuitBreakerConfig;
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use std::time::Duration;
    ///
    /// let client = ClientBuilder::new()
    ///     .enable_resilience()
    ///     .with_circuit_breaker_config(CircuitBreakerConfig {
    ///         failure_threshold: 5,
    ///         success_threshold: 2,
    ///         timeout: Duration::from_secs(60),
    ///         rolling_window_size: 100,
    ///         minimum_requests: 10,
    ///     })
    ///     .build(StdioTransport::new());
    /// ```
    pub fn with_circuit_breaker_config(
        mut self,
        config: turbomcp_transport::resilience::CircuitBreakerConfig,
    ) -> Self {
        self.circuit_breaker_config = Some(config);
        self.enable_resilience = true; // Auto-enable resilience
        self
    }

    /// Configure health checking for resilient transport
    ///
    /// # Arguments
    ///
    /// * `config` - Health check configuration
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::resilience::HealthCheckConfig;
    /// use turbomcp_transport::stdio::StdioTransport;
    /// use std::time::Duration;
    ///
    /// let client = ClientBuilder::new()
    ///     .enable_resilience()
    ///     .with_health_check_config(HealthCheckConfig {
    ///         interval: Duration::from_secs(30),
    ///         timeout: Duration::from_secs(5),
    ///         failure_threshold: 3,
    ///         success_threshold: 1,
    ///         custom_check: None,
    ///     })
    ///     .build(StdioTransport::new());
    /// ```
    pub fn with_health_check_config(
        mut self,
        config: turbomcp_transport::resilience::HealthCheckConfig,
    ) -> Self {
        self.health_check_config = Some(config);
        self.enable_resilience = true; // Auto-enable resilience
        self
    }

    /// Use high-reliability resilience preset
    ///
    /// Configures retry, circuit breaker, and health checking for high-reliability scenarios
    pub fn with_high_reliability(mut self) -> Self {
        let (retry, circuit, health) = turbomcp_transport::resilience::presets::high_reliability();
        self.retry_config = Some(retry);
        self.circuit_breaker_config = Some(circuit);
        self.health_check_config = Some(health);
        self.enable_resilience = true;
        self
    }

    /// Use high-performance resilience preset
    ///
    /// Configures retry, circuit breaker, and health checking for high-throughput scenarios
    pub fn with_high_performance(mut self) -> Self {
        let (retry, circuit, health) = turbomcp_transport::resilience::presets::high_performance();
        self.retry_config = Some(retry);
        self.circuit_breaker_config = Some(circuit);
        self.health_check_config = Some(health);
        self.enable_resilience = true;
        self
    }

    /// Use resource-constrained resilience preset
    ///
    /// Configures retry, circuit breaker, and health checking for low-resource scenarios
    pub fn with_resource_constrained(mut self) -> Self {
        let (retry, circuit, health) =
            turbomcp_transport::resilience::presets::resource_constrained();
        self.retry_config = Some(retry);
        self.circuit_breaker_config = Some(circuit);
        self.health_check_config = Some(health);
        self.enable_resilience = true;
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

    /// Build a client with resilient transport (circuit breaker, retry, health checking)
    ///
    /// When resilience features are enabled via `enable_resilience()` or any resilience
    /// configuration method, this wraps the transport in a `TurboTransport` that provides:
    /// - Automatic retry with exponential backoff
    /// - Circuit breaker pattern for fast failure
    /// - Health checking and monitoring
    /// - Message deduplication
    ///
    /// # Arguments
    ///
    /// * `transport` - The base transport to wrap with resilience features
    ///
    /// # Returns
    ///
    /// Returns a configured `Client<TurboTransport>` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if plugin initialization or LLM provider setup fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use turbomcp_client::ClientBuilder;
    /// use turbomcp_transport::stdio::StdioTransport;
    ///
    /// # async fn example() -> turbomcp_core::Result<()> {
    /// let client = ClientBuilder::new()
    ///     .with_high_reliability()
    ///     .build_resilient(StdioTransport::new())
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn build_resilient<T: Transport + 'static>(
        self,
        transport: T,
    ) -> Result<Client<turbomcp_transport::resilience::TurboTransport>> {
        use turbomcp_transport::resilience::TurboTransport;

        // Get configurations or use defaults
        let retry_config = self.retry_config.unwrap_or_default();
        let circuit_config = self.circuit_breaker_config.unwrap_or_default();
        let health_config = self.health_check_config.unwrap_or_default();

        // Wrap transport in TurboTransport
        let robust_transport = TurboTransport::new(
            Box::new(transport),
            retry_config,
            circuit_config,
            health_config,
        );

        // Create client with resilient transport
        let mut client = Client::with_capabilities(robust_transport, self.capabilities);

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

            if let Some(session_config) = self.session_config {
                registry
                    .configure_sessions(session_config)
                    .await
                    .map_err(|e| {
                        Error::configuration(format!("Failed to configure sessions: {}", e))
                    })?;
            }

            client.set_sampling_handler(Arc::new(registry));
        }

        // Register plugins
        let has_plugins = !self.plugins.is_empty();
        for plugin in self.plugins {
            client.register_plugin(plugin).await.map_err(|e| {
                Error::bad_request(format!("Failed to register plugin during build: {}", e))
            })?;
        }

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

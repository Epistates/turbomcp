//! Shared client wrapper for thread-safe MCP client usage
//!
//! This module provides the SharedClient which wraps the main Client in a
//! thread-safe interface allowing concurrent access across multiple threads.

use std::sync::Arc;
use tokio::sync::Mutex;

use turbomcp_core::Result;
use turbomcp_transport::Transport;
use turbomcp_protocol::types::*;

use super::config::InitializeResult;
use crate::Client;

/// Thread-safe shared client wrapper
///
/// SharedClient wraps a Client in Arc<Mutex<>> to provide thread-safe access
/// to MCP functionality. All methods delegate to the underlying Client while
/// ensuring thread safety.
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
        let mut client = self.inner.lock().await;
        client.initialize().await
    }

    // ============================================================================
    // TOOL OPERATIONS
    // ============================================================================

    /// List all available tools from the MCP server
    pub async fn list_tools(&self) -> Result<Vec<Tool>> {
        let mut client = self.inner.lock().await;
        client.list_tools().await
    }

    /// List available tool names from the MCP server
    pub async fn list_tool_names(&self) -> Result<Vec<String>> {
        let mut client = self.inner.lock().await;
        client.list_tool_names().await
    }

    /// Call a tool on the server
    pub async fn call_tool(
        &self,
        name: impl Into<String>,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        let mut client = self.inner.lock().await;
        client.call_tool(name, arguments).await
    }

    /// Call a tool with typed arguments
    pub async fn call_tool_typed<T: serde::Serialize>(
        &self,
        name: impl Into<String>,
        arguments: T,
    ) -> Result<CallToolResult> {
        let mut client = self.inner.lock().await;
        client.call_tool_typed(name, arguments).await
    }

    /// Validate tool input against the tool's schema
    pub async fn validate_tool_input(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<()> {
        let mut client = self.inner.lock().await;
        client.validate_tool_input(tool_name, arguments).await
    }

    // ============================================================================
    // RESOURCE OPERATIONS
    // ============================================================================

    /// List available resources from the server
    pub async fn list_resources(&self) -> Result<Vec<Resource>> {
        let mut client = self.inner.lock().await;
        client.list_resources().await
    }

    /// Read the content of a specific resource by URI
    pub async fn read_resource(&self, uri: impl Into<String>) -> Result<ReadResourceResult> {
        let mut client = self.inner.lock().await;
        client.read_resource(uri).await
    }

    /// List available resource templates
    pub async fn list_resource_templates(&self) -> Result<Vec<ResourceTemplate>> {
        let mut client = self.inner.lock().await;
        client.list_resource_templates().await
    }

    // ============================================================================
    // PROMPT OPERATIONS
    // ============================================================================

    /// List available prompt templates from the server
    pub async fn list_prompts(&self) -> Result<Vec<Prompt>> {
        let mut client = self.inner.lock().await;
        client.list_prompts().await
    }

    /// Get a specific prompt template with argument support
    pub async fn get_prompt(
        &self,
        name: impl Into<String>,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<GetPromptResult> {
        let mut client = self.inner.lock().await;
        client.get_prompt(name, arguments).await
    }

    /// Get a prompt with typed arguments
    pub async fn get_prompt_typed<T: serde::Serialize>(
        &self,
        name: impl Into<String>,
        arguments: T,
    ) -> Result<GetPromptResult> {
        let mut client = self.inner.lock().await;
        client.get_prompt_typed(name, arguments).await
    }

    /// Validate prompt arguments against the prompt's schema
    pub async fn validate_prompt_arguments(
        &self,
        prompt_name: &str,
        arguments: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        let mut client = self.inner.lock().await;
        client.validate_prompt_arguments(prompt_name, arguments).await
    }

    // ============================================================================
    // FILESYSTEM ROOTS OPERATIONS
    // ============================================================================

    /// List available filesystem root directories
    pub async fn list_roots(&self) -> Result<Vec<Root>> {
        let mut client = self.inner.lock().await;
        client.list_roots().await
    }

    // ============================================================================
    // SAMPLING OPERATIONS
    // ============================================================================

    /// Create a message using server-side LLM sampling
    pub async fn create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult> {
        let mut client = self.inner.lock().await;
        client.create_message(request).await
    }

    /// Handle a sampling request from the server
    pub async fn handle_sampling_request(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult> {
        let mut client = self.inner.lock().await;
        client.handle_sampling_request(request).await
    }

    // ============================================================================
    // ELICITATION OPERATIONS
    // ============================================================================

    /// Handle an elicitation request from the server
    pub async fn handle_elicitation(&self, request: ElicitRequest) -> Result<ElicitResult> {
        let mut client = self.inner.lock().await;
        client.handle_elicitation(request).await
    }

    /// Respond to an elicitation request with user input
    pub async fn respond_to_elicitation(
        &self,
        request_id: String,
        response: serde_json::Map<String, serde_json::Value>,
    ) -> Result<()> {
        let mut client = self.inner.lock().await;
        client.respond_to_elicitation(request_id, response).await
    }

    // ============================================================================
    // COMPLETION OPERATIONS
    // ============================================================================

    /// Get completions for an argument
    pub async fn complete(
        &self,
        request: CompleteRequestParams,
    ) -> Result<CompletionResponse> {
        let mut client = self.inner.lock().await;
        client.complete(request).await
    }

    // ============================================================================
    // LOGGING OPERATIONS
    // ============================================================================

    /// Set the logging level on the server
    pub async fn set_logging_level(&self, level: LogLevel) -> Result<EmptyResult> {
        let mut client = self.inner.lock().await;
        client.set_logging_level(level).await
    }

    // ============================================================================
    // CONNECTION MANAGEMENT
    // ============================================================================

    /// Connect to the MCP server
    pub async fn connect(&self) -> Result<()> {
        let mut client = self.inner.lock().await;
        client.connect().await
    }

    /// Disconnect from the MCP server
    pub async fn disconnect(&self) -> Result<()> {
        let mut client = self.inner.lock().await;
        client.disconnect().await
    }

    /// Check if connected to the server
    pub async fn is_connected(&self) -> bool {
        let client = self.inner.lock().await;
        client.is_connected()
    }

    // ============================================================================
    // CAPABILITY AND CONFIGURATION
    // ============================================================================

    /// Get the client's capabilities configuration
    pub async fn get_capabilities(&self) -> crate::ClientCapabilities {
        let client = self.inner.lock().await;
        client.get_capabilities().clone()
    }

    /// Get server capabilities (requires initialization)
    pub async fn get_server_capabilities(&self) -> Option<ServerCapabilities> {
        let client = self.inner.lock().await;
        client.get_server_capabilities().cloned()
    }

    // ============================================================================
    // PLUGIN OPERATIONS
    // ============================================================================

    /// Register a plugin with the client
    pub async fn register_plugin(
        &self,
        plugin: std::sync::Arc<dyn crate::plugins::ClientPlugin>,
    ) -> Result<()> {
        let mut client = self.inner.lock().await;
        client.register_plugin(plugin).await
    }

    /// Unregister a plugin by name
    pub async fn unregister_plugin(&self, plugin_name: &str) -> Result<()> {
        let mut client = self.inner.lock().await;
        client.unregister_plugin(plugin_name).await
    }

    /// Get plugin data for a specific plugin type
    pub async fn get_plugin_data<T: serde::de::DeserializeOwned + 'static>(
        &self,
    ) -> Result<Option<T>> {
        let client = self.inner.lock().await;
        client.get_plugin_data().await
    }

    /// Initialize all registered plugins
    pub async fn initialize_plugins(&self) -> Result<()> {
        let mut client = self.inner.lock().await;
        client.initialize_plugins().await
    }

    // ============================================================================
    // HANDLER OPERATIONS
    // ============================================================================

    /// Register an elicitation handler
    pub async fn register_elicitation_handler(
        &self,
        handler: std::sync::Arc<dyn crate::handlers::ElicitationHandler>,
    ) {
        let mut client = self.inner.lock().await;
        client.register_elicitation_handler(handler);
    }

    /// Register a progress handler
    pub async fn register_progress_handler(
        &self,
        handler: std::sync::Arc<dyn crate::handlers::ProgressHandler>,
    ) {
        let mut client = self.inner.lock().await;
        client.register_progress_handler(handler);
    }

    /// Register a log handler
    pub async fn register_log_handler(
        &self,
        handler: std::sync::Arc<dyn crate::handlers::LogHandler>,
    ) {
        let mut client = self.inner.lock().await;
        client.register_log_handler(handler);
    }

    /// Register a resource update handler
    pub async fn register_resource_update_handler(
        &self,
        handler: std::sync::Arc<dyn crate::handlers::ResourceUpdateHandler>,
    ) {
        let mut client = self.inner.lock().await;
        client.register_resource_update_handler(handler);
    }

    // ============================================================================
    // UTILITY METHODS
    // ============================================================================

    /// Get connection configuration
    pub async fn get_connection_config(&self) -> super::config::ConnectionConfig {
        let client = self.inner.lock().await;
        client.get_connection_config().clone()
    }

    /// Set request timeout
    pub async fn set_timeout(&self, timeout_ms: u64) {
        let mut client = self.inner.lock().await;
        client.set_timeout(timeout_ms);
    }

    /// Clone the shared client (increases reference count)
    pub fn clone_shared(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Convert back to a regular client (consumes the SharedClient)
    ///
    /// This method will block if other threads are currently using the client.
    /// Use with caution in async contexts.
    pub async fn into_client(self) -> Client<T> {
        Arc::try_unwrap(self.inner)
            .map_err(|_| "Cannot unwrap SharedClient: multiple references exist")
            .unwrap()
            .into_inner()
    }

    /// Access the inner client with a closure
    ///
    /// This method provides temporary mutable access to the inner client
    /// while maintaining thread safety.
    pub async fn with_client<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Client<T>) -> R,
    {
        let mut client = self.inner.lock().await;
        f(&mut *client)
    }

    /// Access the inner client immutably with a closure
    ///
    /// This method provides temporary read-only access to the inner client
    /// while maintaining thread safety.
    pub async fn with_client_ref<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Client<T>) -> R,
    {
        let client = self.inner.lock().await;
        f(&*client)
    }
}

impl<T: Transport> Clone for SharedClient<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}
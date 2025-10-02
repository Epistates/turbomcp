//! Client builder pattern for MCP client construction
//!
//! Provides a fluent interface for configuring client options before creation.

use std::collections::HashMap;
use std::sync::Arc;
use turbomcp_transport::Transport;

use super::config::ConnectionConfig;
use crate::ClientCapabilities;

/// Builder for configuring and creating MCP clients
///
/// Provides a fluent interface for configuring client options before creation.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::{ClientBuilder, ClientCapabilities};
/// use turbomcp_transport::stdio::StdioTransport;
/// use std::sync::Arc;
///
/// # async fn example() -> turbomcp_core::Result<()> {
/// let mut client = ClientBuilder::new()
///     .with_tools(true)
///     .with_prompts(true)
///     .with_resources(true)
///     .with_sampling(true)
///     .with_elicitation(true)
///     .with_timeout(60_000) // 60 seconds
///     .with_max_retries(5)
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

    /// Enable or disable elicitation support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable elicitation support
    pub fn with_elicitation(mut self, enabled: bool) -> Self {
        self.capabilities.elicitation = enabled;
        self
    }

    /// Enable or disable roots support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable roots support
    pub fn with_roots(mut self, enabled: bool) -> Self {
        self.capabilities.roots = enabled;
        self
    }

    /// Enable or disable logging support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable logging support
    pub fn with_logging(mut self, enabled: bool) -> Self {
        self.capabilities.logging = enabled;
        self
    }

    /// Enable or disable completion support
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable completion support
    pub fn with_completion(mut self, enabled: bool) -> Self {
        self.capabilities.completion = enabled;
        self
    }

    /// Set all capabilities from a ClientCapabilities struct
    ///
    /// # Arguments
    ///
    /// * `capabilities` - The capabilities to set
    pub fn with_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    // ============================================================================
    // CONNECTION CONFIGURATION
    // ============================================================================

    /// Set request timeout in milliseconds
    ///
    /// # Arguments
    ///
    /// * `timeout_ms` - Timeout in milliseconds
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.connection_config.timeout_ms = timeout_ms;
        self
    }

    /// Set maximum number of retry attempts
    ///
    /// # Arguments
    ///
    /// * `max_retries` - Maximum retry attempts
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.connection_config.max_retries = max_retries;
        self
    }

    /// Set retry delay in milliseconds
    ///
    /// # Arguments
    ///
    /// * `retry_delay_ms` - Delay between retries in milliseconds
    pub fn with_retry_delay(mut self, retry_delay_ms: u64) -> Self {
        self.connection_config.retry_delay_ms = retry_delay_ms;
        self
    }

    /// Set keep-alive interval in milliseconds
    ///
    /// # Arguments
    ///
    /// * `keepalive_ms` - Keep-alive interval in milliseconds
    pub fn with_keepalive(mut self, keepalive_ms: u64) -> Self {
        self.connection_config.keepalive_ms = keepalive_ms;
        self
    }

    /// Set connection configuration
    ///
    /// # Arguments
    ///
    /// * `config` - Connection configuration
    pub fn with_connection_config(mut self, config: ConnectionConfig) -> Self {
        self.connection_config = config;
        self
    }

    // ============================================================================
    // PLUGIN CONFIGURATION
    // ============================================================================

    /// Add a plugin to the client
    ///
    /// # Arguments
    ///
    /// * `plugin` - Plugin to add
    pub fn with_plugin(mut self, plugin: Arc<dyn crate::plugins::ClientPlugin>) -> Self {
        self.plugins.push(plugin);
        self
    }

    /// Add multiple plugins to the client
    ///
    /// # Arguments
    ///
    /// * `plugins` - Plugins to add
    pub fn with_plugins(mut self, plugins: Vec<Arc<dyn crate::plugins::ClientPlugin>>) -> Self {
        self.plugins.extend(plugins);
        self
    }

    /// Clear all plugins
    pub fn without_plugins(mut self) -> Self {
        self.plugins.clear();
        self
    }

    // ============================================================================
    // LLM PROVIDER CONFIGURATION
    // ============================================================================

    /// Add an LLM provider
    ///
    /// # Arguments
    ///
    /// * `name` - Provider name (e.g., "openai", "anthropic")
    /// * `provider` - Provider implementation
    pub fn with_llm_provider(
        mut self,
        name: impl Into<String>,
        provider: Arc<dyn crate::llm::LLMProvider>,
    ) -> Self {
        self.llm_providers.insert(name.into(), provider);
        self
    }

    /// Add multiple LLM providers
    ///
    /// # Arguments
    ///
    /// * `providers` - Map of provider name to implementation
    pub fn with_llm_providers(
        mut self,
        providers: HashMap<String, Arc<dyn crate::llm::LLMProvider>>,
    ) -> Self {
        self.llm_providers.extend(providers);
        self
    }

    /// Clear all LLM providers
    pub fn without_llm_providers(mut self) -> Self {
        self.llm_providers.clear();
        self
    }

    // ============================================================================
    // HANDLER CONFIGURATION
    // ============================================================================

    /// Set elicitation handler
    ///
    /// # Arguments
    ///
    /// * `handler` - Elicitation handler
    pub fn with_elicitation_handler(
        mut self,
        handler: Arc<dyn crate::handlers::ElicitationHandler>,
    ) -> Self {
        self.elicitation_handler = Some(handler);
        self
    }

    /// Set progress handler
    ///
    /// # Arguments
    ///
    /// * `handler` - Progress handler
    pub fn with_progress_handler(
        mut self,
        handler: Arc<dyn crate::handlers::ProgressHandler>,
    ) -> Self {
        self.progress_handler = Some(handler);
        self
    }

    /// Set log handler
    ///
    /// # Arguments
    ///
    /// * `handler` - Log handler
    pub fn with_log_handler(mut self, handler: Arc<dyn crate::handlers::LogHandler>) -> Self {
        self.log_handler = Some(handler);
        self
    }

    /// Set resource update handler
    ///
    /// # Arguments
    ///
    /// * `handler` - Resource update handler
    pub fn with_resource_update_handler(
        mut self,
        handler: Arc<dyn crate::handlers::ResourceUpdateHandler>,
    ) -> Self {
        self.resource_update_handler = Some(handler);
        self
    }

    // ============================================================================
    // SESSION CONFIGURATION
    // ============================================================================

    /// Set session configuration for LLM interactions
    ///
    /// # Arguments
    ///
    /// * `config` - Session configuration
    pub fn with_session_config(mut self, config: crate::llm::SessionConfig) -> Self {
        self.session_config = Some(config);
        self
    }

    // ============================================================================
    // BUILD METHODS
    // ============================================================================

    /// Build a client with the specified transport
    ///
    /// # Arguments
    ///
    /// * `transport` - Transport implementation
    ///
    /// # Returns
    ///
    /// A configured client ready for initialization
    pub async fn build<T: Transport>(self, transport: T) -> turbomcp_core::Result<crate::Client<T>> {
        let mut client = crate::Client::new(transport);

        // Apply capabilities
        client.capabilities = self.capabilities;

        // Register plugins
        for plugin in self.plugins {
            client.plugin_registry.register_plugin(plugin);
        }

        // Register LLM providers
        for (name, provider) in self.llm_providers {
            client.plugin_registry.register_llm_provider(name, provider)?;
        }

        // Register handlers
        if let Some(handler) = self.elicitation_handler {
            client.handlers.register_elicitation_handler(handler);
        }
        if let Some(handler) = self.progress_handler {
            client.handlers.register_progress_handler(handler);
        }
        if let Some(handler) = self.log_handler {
            client.handlers.register_log_handler(handler);
        }
        if let Some(handler) = self.resource_update_handler {
            client.handlers.register_resource_update_handler(handler);
        }

        // Apply session configuration
        if let Some(config) = self.session_config {
            client.plugin_registry.set_session_config(config)?;
        }

        Ok(client)
    }

    /// Build a SharedClient with the specified transport
    ///
    /// This creates a thread-safe client that can be cloned and used
    /// across multiple threads.
    ///
    /// # Arguments
    ///
    /// * `transport` - Transport implementation
    ///
    /// # Returns
    ///
    /// A shared client ready for initialization
    pub async fn build_shared<T: Transport>(
        self,
        transport: T,
    ) -> turbomcp_core::Result<crate::SharedClient<T>> {
        let client = self.build(transport).await?;
        Ok(crate::SharedClient::new(client))
    }

    // ============================================================================
    // INSPECTION METHODS
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
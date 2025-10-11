//! Plugin management operations for MCP client
//!
//! This module provides methods for registering and managing client plugins
//! that extend functionality through middleware.

use turbomcp_protocol::{Error, Result};

impl<T: turbomcp_transport::Transport + 'static> super::super::core::Client<T> {
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
        &self,
        plugin: std::sync::Arc<dyn crate::plugins::ClientPlugin>,
    ) -> Result<()> {
        self.inner
            .plugin_registry
            .lock()
            .await
            .register_plugin(plugin)
            .await
            .map_err(|e| Error::bad_request(format!("Failed to register plugin: {}", e)))
    }

    /// Check if a plugin is registered
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin to check
    pub async fn has_plugin(&self, name: &str) -> bool {
        self.inner.plugin_registry.lock().await.has_plugin(name)
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
    /// if let Some(plugin) = client.get_plugin("metrics").await {
    ///     // Use plugin data
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_plugin(
        &self,
        name: &str,
    ) -> Option<std::sync::Arc<dyn crate::plugins::ClientPlugin>> {
        self.inner.plugin_registry.lock().await.get_plugin(name)
    }
}

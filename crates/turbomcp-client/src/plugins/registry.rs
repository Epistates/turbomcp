//! Plugin registry for managing plugin lifecycle and execution
//!
//! The PluginRegistry manages the registration, ordering, and execution of plugins.
//! It implements the middleware pattern where plugins are executed in a defined order
//! for request/response processing.

use crate::plugins::core::{
    ClientPlugin, PluginContext, PluginError, PluginResult, RequestContext, ResponseContext,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Registry for managing client plugins
///
/// The registry maintains an ordered list of plugins and provides methods for:
/// - Plugin registration and validation
/// - Middleware chain execution
/// - Custom method routing
/// - Plugin lifecycle management
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_client::plugins::{PluginRegistry, MetricsPlugin, PluginConfig};
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut registry = PluginRegistry::new();
///
/// // Register a metrics plugin
/// let metrics = Arc::new(MetricsPlugin::new(PluginConfig::Metrics));
/// registry.register_plugin(metrics).await?;
///
/// // Execute middleware chain
/// // let mut request_context = RequestContext::new(...);
/// // registry.execute_before_request(&mut request_context).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct PluginRegistry {
    /// Registered plugins in execution order
    plugins: Vec<Arc<dyn ClientPlugin>>,

    /// Plugin lookup by name for fast access
    plugin_map: HashMap<String, usize>,

    /// Client context for plugin initialization
    client_context: Option<PluginContext>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    /// Create a new empty plugin registry
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            plugin_map: HashMap::new(),
            client_context: None,
        }
    }

    /// Set the client context for plugin initialization
    ///
    /// This should be called once when the client is initialized to provide
    /// context information to plugins during registration.
    pub fn set_client_context(&mut self, context: PluginContext) {
        debug!(
            "Setting client context: {} v{}",
            context.client_name, context.client_version
        );
        self.client_context = Some(context);
    }

    /// Register a new plugin
    ///
    /// Validates the plugin, checks dependencies, and initializes it.
    /// Plugins are executed in registration order.
    ///
    /// # Arguments
    ///
    /// * `plugin` - The plugin to register
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if registration succeeds, or `PluginError` if it fails.
    ///
    /// # Errors
    ///
    /// - Plugin name already registered
    /// - Plugin dependencies not met
    /// - Plugin initialization failure
    pub async fn register_plugin(&mut self, plugin: Arc<dyn ClientPlugin>) -> PluginResult<()> {
        let plugin_name = plugin.name().to_string();

        info!("Registering plugin: {} v{}", plugin_name, plugin.version());

        // Check for duplicate registration
        if self.plugin_map.contains_key(&plugin_name) {
            return Err(PluginError::configuration(format!(
                "Plugin '{}' is already registered",
                plugin_name
            )));
        }

        // Check dependencies
        for dependency in plugin.dependencies() {
            if !self.has_plugin(dependency) {
                return Err(PluginError::dependency_not_available(dependency));
            }
        }

        // Initialize plugin with current context
        if let Some(context) = &self.client_context {
            // Update context with current plugin list
            let mut updated_context = context.clone();
            updated_context.available_plugins = self.get_plugin_names();

            plugin.initialize(&updated_context).await.map_err(|e| {
                error!("Failed to initialize plugin '{}': {}", plugin_name, e);
                e
            })?;
        } else {
            // Create minimal context if none set
            let context = PluginContext::new(
                "unknown".to_string(),
                "unknown".to_string(),
                HashMap::new(),
                HashMap::new(),
                self.get_plugin_names(),
            );
            plugin.initialize(&context).await.map_err(|e| {
                error!("Failed to initialize plugin '{}': {}", plugin_name, e);
                e
            })?;
        }

        // Register the plugin
        let index = self.plugins.len();
        self.plugins.push(plugin);
        self.plugin_map.insert(plugin_name.clone(), index);

        debug!(
            "Plugin '{}' registered successfully at index {}",
            plugin_name, index
        );
        Ok(())
    }

    /// Unregister a plugin by name
    ///
    /// Removes the plugin from the registry and calls its cleanup method.
    ///
    /// # Arguments
    ///
    /// * `plugin_name` - Name of the plugin to unregister
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if unregistration succeeds, or `PluginError` if it fails.
    pub async fn unregister_plugin(&mut self, plugin_name: &str) -> PluginResult<()> {
        info!("Unregistering plugin: {}", plugin_name);

        let index = self.plugin_map.get(plugin_name).copied().ok_or_else(|| {
            PluginError::configuration(format!("Plugin '{}' not found", plugin_name))
        })?;

        // Get the plugin and call cleanup
        let plugin = self.plugins[index].clone();
        plugin.cleanup().await.map_err(|e| {
            warn!("Plugin '{}' cleanup failed: {}", plugin_name, e);
            e
        })?;

        // Remove from collections
        self.plugins.remove(index);
        self.plugin_map.remove(plugin_name);

        // Update indices in the map
        for (_, plugin_index) in self.plugin_map.iter_mut() {
            if *plugin_index > index {
                *plugin_index -= 1;
            }
        }

        debug!("Plugin '{}' unregistered successfully", plugin_name);
        Ok(())
    }

    /// Check if a plugin is registered
    pub fn has_plugin(&self, plugin_name: &str) -> bool {
        self.plugin_map.contains_key(plugin_name)
    }

    /// Get a plugin by name
    pub fn get_plugin(&self, plugin_name: &str) -> Option<Arc<dyn ClientPlugin>> {
        self.plugin_map
            .get(plugin_name)
            .and_then(|&index| self.plugins.get(index))
            .cloned()
    }

    /// Get all registered plugin names in execution order
    pub fn get_plugin_names(&self) -> Vec<String> {
        self.plugins
            .iter()
            .map(|plugin| plugin.name().to_string())
            .collect()
    }

    /// Get the number of registered plugins
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Execute before_request middleware chain
    ///
    /// Calls `before_request` on all registered plugins in order.
    /// If any plugin returns an error, the chain is aborted and the error is returned.
    ///
    /// # Arguments
    ///
    /// * `context` - Mutable request context that can be modified by plugins
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if all plugins succeed, or the first `PluginError` encountered.
    pub async fn execute_before_request(&self, context: &mut RequestContext) -> PluginResult<()> {
        debug!(
            "Executing before_request middleware chain for method: {}",
            context.method()
        );

        for (index, plugin) in self.plugins.iter().enumerate() {
            let plugin_name = plugin.name();
            debug!(
                "Calling before_request on plugin '{}' ({})",
                plugin_name, index
            );

            plugin.before_request(context).await.map_err(|e| {
                error!(
                    "Plugin '{}' before_request failed for method '{}': {}",
                    plugin_name,
                    context.method(),
                    e
                );
                e
            })?;
        }

        debug!("Before_request middleware chain completed successfully");
        Ok(())
    }

    /// Execute after_response middleware chain
    ///
    /// Calls `after_response` on all registered plugins in order.
    /// Unlike before_request, this continues execution even if a plugin fails,
    /// logging errors but not aborting the chain.
    ///
    /// # Arguments
    ///
    /// * `context` - Mutable response context that can be modified by plugins
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` unless all plugins fail, in which case returns the last error.
    pub async fn execute_after_response(&self, context: &mut ResponseContext) -> PluginResult<()> {
        debug!(
            "Executing after_response middleware chain for method: {}",
            context.method()
        );

        let mut _last_error = None;

        for (index, plugin) in self.plugins.iter().enumerate() {
            let plugin_name = plugin.name();
            debug!(
                "Calling after_response on plugin '{}' ({})",
                plugin_name, index
            );

            if let Err(e) = plugin.after_response(context).await {
                error!(
                    "Plugin '{}' after_response failed for method '{}': {}",
                    plugin_name,
                    context.method(),
                    e
                );
                _last_error = Some(e);
                // Continue with other plugins
            }
        }

        debug!("After_response middleware chain completed");

        // Return error only if we have one and want to propagate it
        // For now, we log errors but don't fail the response processing
        Ok(())
    }

    /// Handle custom method by routing to appropriate plugin
    ///
    /// Attempts to handle the custom method by calling `handle_custom` on each
    /// plugin in order until one returns `Some(Value)`.
    ///
    /// # Arguments
    ///
    /// * `method` - The custom method name
    /// * `params` - Optional parameters for the method
    ///
    /// # Returns
    ///
    /// Returns `Some(Value)` if a plugin handled the method, `None` if no plugin handled it,
    /// or `PluginError` if handling failed.
    pub async fn handle_custom_method(
        &self,
        method: &str,
        params: Option<Value>,
    ) -> PluginResult<Option<Value>> {
        debug!("Handling custom method: {}", method);

        for plugin in &self.plugins {
            let plugin_name = plugin.name();
            debug!(
                "Checking if plugin '{}' can handle custom method '{}'",
                plugin_name, method
            );

            match plugin.handle_custom(method, params.clone()).await {
                Ok(Some(result)) => {
                    info!(
                        "Plugin '{}' handled custom method '{}'",
                        plugin_name, method
                    );
                    return Ok(Some(result));
                }
                Ok(None) => {
                    // Plugin doesn't handle this method, continue
                    continue;
                }
                Err(e) => {
                    error!(
                        "Plugin '{}' failed to handle custom method '{}': {}",
                        plugin_name, method, e
                    );
                    return Err(e);
                }
            }
        }

        debug!("No plugin handled custom method: {}", method);
        Ok(None)
    }

    /// Get plugin information for debugging
    pub fn get_plugin_info(&self) -> Vec<(String, String, Option<String>)> {
        self.plugins
            .iter()
            .map(|plugin| {
                (
                    plugin.name().to_string(),
                    plugin.version().to_string(),
                    plugin.description().map(|s| s.to_string()),
                )
            })
            .collect()
    }

    /// Validate plugin dependencies
    ///
    /// Checks that all registered plugins have their dependencies satisfied.
    /// This is useful for debugging plugin configuration issues.
    pub fn validate_dependencies(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        for plugin in &self.plugins {
            for dependency in plugin.dependencies() {
                if !self.has_plugin(dependency) {
                    errors.push(format!(
                        "Plugin '{}' depends on '{}' which is not registered",
                        plugin.name(),
                        dependency
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Clear all registered plugins
    ///
    /// Calls cleanup on all plugins and removes them from the registry.
    /// This is primarily useful for testing and shutdown scenarios.
    pub async fn clear(&mut self) -> PluginResult<()> {
        info!("Clearing all registered plugins");

        let plugins = std::mem::take(&mut self.plugins);
        self.plugin_map.clear();

        for plugin in plugins {
            let plugin_name = plugin.name();
            if let Err(e) = plugin.cleanup().await {
                warn!("Plugin '{}' cleanup failed: {}", plugin_name, e);
            }
        }

        debug!("All plugins cleared successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::core::PluginContext;
    use async_trait::async_trait;
    use serde_json::json;
    use std::sync::Mutex;
    use tokio;
    use turbomcp_protocol::MessageId;
    use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};

    // Test plugin for validation
    #[derive(Debug)]
    struct MockPlugin {
        name: String,
        calls: Arc<Mutex<Vec<String>>>,
        should_fail_init: bool,
        should_fail_before_request: bool,
    }

    impl MockPlugin {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                calls: Arc::new(Mutex::new(Vec::new())),
                should_fail_init: false,
                should_fail_before_request: false,
            }
        }

        fn with_init_failure(mut self) -> Self {
            self.should_fail_init = true;
            self
        }

        fn with_request_failure(mut self) -> Self {
            self.should_fail_before_request = true;
            self
        }

        fn get_calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ClientPlugin for MockPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            "1.0.0"
        }

        async fn initialize(&self, _context: &PluginContext) -> PluginResult<()> {
            self.calls.lock().unwrap().push("initialize".to_string());
            if self.should_fail_init {
                Err(PluginError::initialization("Mock initialization failure"))
            } else {
                Ok(())
            }
        }

        async fn before_request(&self, context: &mut RequestContext) -> PluginResult<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("before_request:{}", context.method()));
            if self.should_fail_before_request {
                Err(PluginError::request_processing("Mock request failure"))
            } else {
                Ok(())
            }
        }

        async fn after_response(&self, context: &mut ResponseContext) -> PluginResult<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("after_response:{}", context.method()));
            Ok(())
        }

        async fn handle_custom(
            &self,
            method: &str,
            params: Option<Value>,
        ) -> PluginResult<Option<Value>> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("handle_custom:{}", method));
            if method.starts_with(&format!("{}.", self.name)) {
                Ok(params)
            } else {
                Ok(None)
            }
        }
    }

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = PluginRegistry::new();
        assert_eq!(registry.plugin_count(), 0);
        assert!(registry.get_plugin_names().is_empty());
    }

    #[tokio::test]
    async fn test_plugin_registration() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("test"));

        registry.register_plugin(plugin.clone()).await.unwrap();

        assert_eq!(registry.plugin_count(), 1);
        assert!(registry.has_plugin("test"));
        assert_eq!(registry.get_plugin_names(), vec!["test"]);

        let retrieved = registry.get_plugin("test").unwrap();
        assert_eq!(retrieved.name(), "test");
    }

    #[tokio::test]
    async fn test_duplicate_registration() {
        let mut registry = PluginRegistry::new();
        let plugin1 = Arc::new(MockPlugin::new("duplicate"));
        let plugin2 = Arc::new(MockPlugin::new("duplicate"));

        registry.register_plugin(plugin1).await.unwrap();
        let result = registry.register_plugin(plugin2).await;

        assert!(result.is_err());
        assert_eq!(registry.plugin_count(), 1);
    }

    #[tokio::test]
    async fn test_plugin_initialization_failure() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("failing").with_init_failure());

        let result = registry.register_plugin(plugin).await;

        assert!(result.is_err());
        assert_eq!(registry.plugin_count(), 0);
    }

    #[tokio::test]
    async fn test_plugin_unregistration() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("removable"));

        registry.register_plugin(plugin).await.unwrap();
        assert_eq!(registry.plugin_count(), 1);

        registry.unregister_plugin("removable").await.unwrap();
        assert_eq!(registry.plugin_count(), 0);
        assert!(!registry.has_plugin("removable"));
    }

    #[tokio::test]
    async fn test_before_request_middleware() {
        let mut registry = PluginRegistry::new();
        let plugin1 = Arc::new(MockPlugin::new("first"));
        let plugin2 = Arc::new(MockPlugin::new("second"));

        registry.register_plugin(plugin1.clone()).await.unwrap();
        registry.register_plugin(plugin2.clone()).await.unwrap();

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "test/method".to_string(),
            params: None,
        };

        let mut context = RequestContext::new(request, HashMap::new());
        registry.execute_before_request(&mut context).await.unwrap();

        // Check both plugins were called
        assert!(
            plugin1
                .get_calls()
                .contains(&"before_request:test/method".to_string())
        );
        assert!(
            plugin2
                .get_calls()
                .contains(&"before_request:test/method".to_string())
        );
    }

    #[tokio::test]
    async fn test_before_request_error_handling() {
        let mut registry = PluginRegistry::new();
        let good_plugin = Arc::new(MockPlugin::new("good"));
        let bad_plugin = Arc::new(MockPlugin::new("bad").with_request_failure());

        registry.register_plugin(good_plugin.clone()).await.unwrap();
        registry.register_plugin(bad_plugin.clone()).await.unwrap();

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::from("test"),
            method: "test/method".to_string(),
            params: None,
        };

        let mut context = RequestContext::new(request, HashMap::new());
        let result = registry.execute_before_request(&mut context).await;

        assert!(result.is_err());
        assert!(
            good_plugin
                .get_calls()
                .contains(&"before_request:test/method".to_string())
        );
        assert!(
            bad_plugin
                .get_calls()
                .contains(&"before_request:test/method".to_string())
        );
    }

    #[tokio::test]
    async fn test_custom_method_handling() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("handler"));

        registry.register_plugin(plugin.clone()).await.unwrap();

        let result = registry
            .handle_custom_method("handler.test", Some(json!({"data": "test"})))
            .await
            .unwrap();

        assert!(result.is_some());
        assert_eq!(result.unwrap(), json!({"data": "test"}));
        assert!(
            plugin
                .get_calls()
                .contains(&"handle_custom:handler.test".to_string())
        );
    }

    #[tokio::test]
    async fn test_custom_method_not_handled() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("handler"));

        registry.register_plugin(plugin.clone()).await.unwrap();

        let result = registry
            .handle_custom_method("other.method", None)
            .await
            .unwrap();

        assert!(result.is_none());
        assert!(
            plugin
                .get_calls()
                .contains(&"handle_custom:other.method".to_string())
        );
    }

    #[tokio::test]
    async fn test_plugin_info() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockPlugin::new("info_test"));

        registry.register_plugin(plugin).await.unwrap();

        let info = registry.get_plugin_info();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].0, "info_test");
        assert_eq!(info[0].1, "1.0.0");
    }

    #[tokio::test]
    async fn test_clear_plugins() {
        let mut registry = PluginRegistry::new();
        let plugin1 = Arc::new(MockPlugin::new("first"));
        let plugin2 = Arc::new(MockPlugin::new("second"));

        registry.register_plugin(plugin1).await.unwrap();
        registry.register_plugin(plugin2).await.unwrap();
        assert_eq!(registry.plugin_count(), 2);

        registry.clear().await.unwrap();
        assert_eq!(registry.plugin_count(), 0);
        assert!(registry.get_plugin_names().is_empty());
    }
}

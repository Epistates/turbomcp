//! Tower Layer implementation for client plugins

use std::sync::Arc;
use tower::Layer;

use crate::plugins::PluginRegistry;
use crate::plugins::core::{ClientPlugin, PluginContext};

use super::PluginLayerConfig;
use super::service::PluginService;

/// Tower Layer that adds plugin middleware to services
///
/// This layer wraps inner services with [`PluginService`], which executes
/// registered plugins before and after each request.
///
/// # Example
///
/// ```rust,ignore
/// use tower::ServiceBuilder;
/// use turbomcp_client::tower::PluginLayer;
///
/// let plugin_layer = PluginLayer::new();
///
/// let service = ServiceBuilder::new()
///     .layer(plugin_layer)
///     .service(my_inner_service);
/// ```
#[derive(Debug, Clone)]
pub struct PluginLayer {
    /// Registered plugins
    plugins: Vec<Arc<dyn ClientPlugin>>,
    /// Plugin context for initialization
    plugin_context: Option<PluginContext>,
    /// Layer configuration
    config: PluginLayerConfig,
}

impl Default for PluginLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginLayer {
    /// Create a new empty plugin layer
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            plugin_context: None,
            config: PluginLayerConfig::default(),
        }
    }

    /// Create a plugin layer with configuration
    pub fn with_config(config: PluginLayerConfig) -> Self {
        Self {
            plugins: Vec::new(),
            plugin_context: None,
            config,
        }
    }

    /// Create a plugin layer from an existing registry
    ///
    /// This allows existing plugin registries to be converted to Tower layers.
    pub fn from_registry(registry: &PluginRegistry) -> Self {
        let plugin_names = registry.get_plugin_names();
        let plugins: Vec<Arc<dyn ClientPlugin>> = plugin_names
            .iter()
            .filter_map(|name| registry.get_plugin(name))
            .collect();

        Self {
            plugins,
            plugin_context: None,
            config: PluginLayerConfig::default(),
        }
    }

    /// Create a plugin layer from an existing registry with configuration
    pub fn from_registry_with_config(registry: &PluginRegistry, config: PluginLayerConfig) -> Self {
        let mut layer = Self::from_registry(registry);
        layer.config = config;
        layer
    }

    /// Add a plugin to the layer
    ///
    /// Plugins are executed in the order they are added.
    #[must_use]
    pub fn add_plugin<P>(mut self, plugin: P) -> Self
    where
        P: ClientPlugin + 'static,
    {
        self.plugins.push(Arc::new(plugin));
        self
    }

    /// Add an Arc'd plugin to the layer
    #[must_use]
    pub fn add_plugin_arc(mut self, plugin: Arc<dyn ClientPlugin>) -> Self {
        self.plugins.push(plugin);
        self
    }

    /// Set the plugin context for initialization
    #[must_use]
    pub fn with_plugin_context(mut self, context: PluginContext) -> Self {
        self.plugin_context = Some(context);
        self
    }

    /// Set the configuration for this layer
    #[must_use]
    pub fn config(mut self, config: PluginLayerConfig) -> Self {
        self.config = config;
        self
    }

    /// Add a method to bypass plugin processing
    #[must_use]
    pub fn bypass_method(mut self, method: impl Into<String>) -> Self {
        self.config.bypass_methods.push(method.into());
        self
    }

    /// Get the number of registered plugins
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// Get registered plugin names
    pub fn plugin_names(&self) -> Vec<String> {
        self.plugins.iter().map(|p| p.name().to_string()).collect()
    }
}

impl<S> Layer<S> for PluginLayer {
    type Service = PluginService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        match &self.plugin_context {
            Some(ctx) => PluginService::with_context(
                inner,
                self.plugins.clone(),
                self.config.clone(),
                ctx.clone(),
            ),
            None => PluginService::new(inner, self.plugins.clone(), self.config.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::core::PluginError;
    use crate::plugins::core::RequestContext;
    use crate::plugins::core::ResponseContext;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::HashMap;

    #[derive(Debug)]
    struct TestPlugin {
        name: String,
    }

    impl TestPlugin {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
            }
        }
    }

    #[async_trait]
    impl ClientPlugin for TestPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            "1.0.0"
        }

        async fn initialize(&self, _context: &PluginContext) -> Result<(), PluginError> {
            Ok(())
        }

        async fn before_request(&self, _context: &mut RequestContext) -> Result<(), PluginError> {
            Ok(())
        }

        async fn after_response(&self, _context: &mut ResponseContext) -> Result<(), PluginError> {
            Ok(())
        }

        async fn handle_custom(
            &self,
            _method: &str,
            _params: Option<Value>,
        ) -> Result<Option<Value>, PluginError> {
            Ok(None)
        }
    }

    #[test]
    fn test_layer_creation() {
        let layer = PluginLayer::new();
        assert_eq!(layer.plugin_count(), 0);
        assert!(layer.plugin_names().is_empty());
    }

    #[test]
    fn test_add_plugin() {
        let layer = PluginLayer::new()
            .add_plugin(TestPlugin::new("first"))
            .add_plugin(TestPlugin::new("second"));

        assert_eq!(layer.plugin_count(), 2);
        assert_eq!(layer.plugin_names(), vec!["first", "second"]);
    }

    #[test]
    fn test_add_plugin_arc() {
        let plugin: Arc<dyn ClientPlugin> = Arc::new(TestPlugin::new("test"));
        let layer = PluginLayer::new().add_plugin_arc(plugin);

        assert_eq!(layer.plugin_count(), 1);
        assert_eq!(layer.plugin_names(), vec!["test"]);
    }

    #[test]
    fn test_with_config() {
        let config = PluginLayerConfig::strict();
        let layer = PluginLayer::with_config(config.clone());

        assert!(!layer.config.continue_on_response_error);
        assert!(layer.config.abort_on_request_error);
    }

    #[test]
    fn test_bypass_method() {
        let layer = PluginLayer::new()
            .bypass_method("initialize")
            .bypass_method("ping");

        assert!(layer.config.should_bypass("initialize"));
        assert!(layer.config.should_bypass("ping"));
        assert!(!layer.config.should_bypass("tools/call"));
    }

    #[test]
    fn test_with_plugin_context() {
        let context = PluginContext::new(
            "test-client".to_string(),
            "1.0.0".to_string(),
            HashMap::new(),
            HashMap::new(),
            Vec::new(),
        );

        let layer = PluginLayer::new().with_plugin_context(context);
        assert!(layer.plugin_context.is_some());
    }

    #[test]
    fn test_from_registry() {
        let registry = PluginRegistry::new();
        // Note: Can't easily test with registered plugins without async
        let layer = PluginLayer::from_registry(&registry);
        assert_eq!(layer.plugin_count(), 0);
    }
}

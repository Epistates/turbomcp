//! Tower Layer implementation for the proxy

use std::sync::Arc;
use tower::Layer;

use crate::proxy::ProxyService;

use super::ProxyLayerConfig;
use super::service::ProxyTowerService;

/// Tower Layer that wraps the proxy service
///
/// This layer wraps services with [`ProxyTowerService`], providing
/// MCP proxying functionality as a composable middleware layer.
///
/// # Example
///
/// ```rust,ignore
/// use tower::ServiceBuilder;
/// use turbomcp_proxy::tower::ProxyLayer;
///
/// let proxy_layer = ProxyLayer::new(proxy_service);
///
/// let service = ServiceBuilder::new()
///     .layer(proxy_layer)
///     .service(my_inner_service);
/// ```
#[derive(Clone)]
pub struct ProxyLayer {
    /// The proxy service
    proxy: Arc<ProxyService>,
    /// Layer configuration
    config: ProxyLayerConfig,
}

impl std::fmt::Debug for ProxyLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyLayer")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ProxyLayer {
    /// Create a new proxy layer
    #[must_use]
    pub fn new(proxy: ProxyService) -> Self {
        Self {
            proxy: Arc::new(proxy),
            config: ProxyLayerConfig::default(),
        }
    }

    /// Create a new proxy layer from an Arc
    #[must_use]
    pub fn from_arc(proxy: Arc<ProxyService>) -> Self {
        Self {
            proxy,
            config: ProxyLayerConfig::default(),
        }
    }

    /// Create a new proxy layer with configuration
    #[must_use]
    pub fn with_config(proxy: ProxyService, config: ProxyLayerConfig) -> Self {
        Self {
            proxy: Arc::new(proxy),
            config,
        }
    }

    /// Set the configuration for this layer
    #[must_use]
    pub fn config(mut self, config: ProxyLayerConfig) -> Self {
        self.config = config;
        self
    }

    /// Add a method to bypass proxy processing
    #[must_use]
    pub fn bypass_method(mut self, method: impl Into<String>) -> Self {
        self.config.bypass_methods.push(method.into());
        self
    }

    /// Set request timeout
    #[must_use]
    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Enable or disable timing metadata
    #[must_use]
    pub fn include_timing(mut self, include: bool) -> Self {
        self.config.include_timing = include;
        self
    }

    /// Enable or disable logging
    #[must_use]
    pub fn enable_logging(mut self, enable: bool) -> Self {
        self.config.enable_logging = enable;
        self
    }
}

impl<S> Layer<S> for ProxyLayer {
    type Service = ProxyTowerService;

    fn layer(&self, _inner: S) -> Self::Service {
        // Note: The proxy layer replaces the inner service rather than wrapping it
        // This is because the proxy IS the service - it forwards requests to backends
        ProxyTowerService::from_arc(Arc::clone(&self.proxy), self.config.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_layer_creation() {
        // Note: We can't easily create a ProxyService without a backend
        // so we'll test the config methods instead
        let config = ProxyLayerConfig::default();
        assert_eq!(config.timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_config_builder() {
        let config = ProxyLayerConfig::default()
            .timeout(Duration::from_secs(60))
            .bypass_method("ping")
            .include_timing(false)
            .enable_logging(false);

        assert_eq!(config.timeout, Duration::from_secs(60));
        assert!(config.should_bypass("ping"));
        assert!(!config.include_timing);
        assert!(!config.enable_logging);
    }

    #[test]
    fn test_layer_config_methods() {
        // Test that config methods return the expected values
        let mut config = ProxyLayerConfig::default();
        config.bypass_methods.push("test".to_string());

        assert!(config.should_bypass("test"));
        assert!(!config.should_bypass("other"));
    }
}

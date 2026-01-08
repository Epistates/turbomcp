//! # Tower Middleware Integration for TurboMCP Client Plugins
//!
//! This module provides Tower Layer and Service implementations for client plugins,
//! enabling composable middleware stacks that integrate with the Tower ecosystem.
//!
//! ## Overview
//!
//! The Tower integration consists of:
//!
//! - [`PluginLayer`] - A Tower Layer that wraps client plugins
//! - [`PluginService`] - A Tower Service that executes plugin middleware
//! - [`McpRequest`] / [`McpResponse`] - Request/response types for the service
//!
//! ## Usage
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use turbomcp_client::tower::{PluginLayer, PluginLayerConfig};
//! use turbomcp_client::plugins::MetricsPlugin;
//!
//! let plugin_layer = PluginLayer::new()
//!     .add_plugin(MetricsPlugin::new(config));
//!
//! let service = ServiceBuilder::new()
//!     .layer(plugin_layer)
//!     .service(my_inner_service);
//! ```
//!
//! ## Integration with Existing Plugin System
//!
//! The Tower integration wraps the existing [`ClientPlugin`] trait, allowing
//! existing plugins to be used in Tower service stacks without modification.
//!
//! ```rust,ignore
//! // Existing plugins work seamlessly
//! let layer = PluginLayer::from_registry(plugin_registry);
//!
//! // Or build a new layer with plugins
//! let layer = PluginLayer::new()
//!     .add_plugin(metrics_plugin)
//!     .add_plugin(retry_plugin)
//!     .add_plugin(cache_plugin);
//! ```

mod layer;
mod service;

pub use layer::PluginLayer;
pub use service::{McpRequest, McpResponse, PluginService, PluginServiceFuture};

use serde_json::Value;
use std::collections::HashMap;

/// Configuration for the plugin layer
#[derive(Debug, Clone)]
pub struct PluginLayerConfig {
    /// Whether to continue processing on plugin errors in the response chain
    pub continue_on_response_error: bool,
    /// Whether to abort request processing on plugin error
    pub abort_on_request_error: bool,
    /// Methods that bypass plugin processing
    pub bypass_methods: Vec<String>,
    /// Default metadata to include in all requests
    pub default_metadata: HashMap<String, Value>,
}

impl Default for PluginLayerConfig {
    fn default() -> Self {
        Self {
            continue_on_response_error: true,
            abort_on_request_error: true,
            bypass_methods: Vec::new(),
            default_metadata: HashMap::new(),
        }
    }
}

impl PluginLayerConfig {
    /// Create a new config with permissive error handling
    #[must_use]
    pub fn permissive() -> Self {
        Self {
            continue_on_response_error: true,
            abort_on_request_error: false,
            ..Default::default()
        }
    }

    /// Create a new config with strict error handling
    #[must_use]
    pub fn strict() -> Self {
        Self {
            continue_on_response_error: false,
            abort_on_request_error: true,
            ..Default::default()
        }
    }

    /// Add a method to bypass plugin processing
    #[must_use]
    pub fn bypass_method(mut self, method: impl Into<String>) -> Self {
        self.bypass_methods.push(method.into());
        self
    }

    /// Add default metadata
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.default_metadata.insert(key.into(), value);
        self
    }

    /// Check if a method should bypass plugin processing
    #[must_use]
    pub fn should_bypass(&self, method: &str) -> bool {
        self.bypass_methods.iter().any(|m| m == method)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_default_config() {
        let config = PluginLayerConfig::default();
        assert!(config.continue_on_response_error);
        assert!(config.abort_on_request_error);
        assert!(config.bypass_methods.is_empty());
        assert!(config.default_metadata.is_empty());
    }

    #[test]
    fn test_permissive_config() {
        let config = PluginLayerConfig::permissive();
        assert!(config.continue_on_response_error);
        assert!(!config.abort_on_request_error);
    }

    #[test]
    fn test_strict_config() {
        let config = PluginLayerConfig::strict();
        assert!(!config.continue_on_response_error);
        assert!(config.abort_on_request_error);
    }

    #[test]
    fn test_bypass_methods() {
        let config = PluginLayerConfig::default()
            .bypass_method("initialize")
            .bypass_method("ping");
        assert!(config.should_bypass("initialize"));
        assert!(config.should_bypass("ping"));
        assert!(!config.should_bypass("tools/call"));
    }

    #[test]
    fn test_with_metadata() {
        let config = PluginLayerConfig::default()
            .with_metadata("version", json!("1.0.0"))
            .with_metadata("client_id", json!("test-client"));
        assert_eq!(
            config.default_metadata.get("version"),
            Some(&json!("1.0.0"))
        );
        assert_eq!(
            config.default_metadata.get("client_id"),
            Some(&json!("test-client"))
        );
    }
}

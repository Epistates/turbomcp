//! # Tower Middleware Integration for `TurboMCP` Proxy
//!
//! This module provides Tower Layer and Service implementations for the proxy,
//! enabling composable middleware stacks that integrate with the Tower ecosystem.
//!
//! ## Overview
//!
//! The Tower integration consists of:
//!
//! - [`ProxyLayer`] - A Tower Layer that wraps the proxy service
//! - [`ProxyTowerService`] - A Tower Service that forwards requests to backend MCP servers
//!
//! ## Usage
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use turbomcp_proxy::tower::{ProxyLayer, ProxyLayerConfig};
//!
//! let proxy_layer = ProxyLayer::new(proxy_service)
//!     .with_config(config);
//!
//! let service = ServiceBuilder::new()
//!     .layer(auth_layer)
//!     .layer(proxy_layer)
//!     .service(inner_service);
//! ```
//!
//! ## Integration with Auth Layer
//!
//! The proxy Tower service integrates seamlessly with the auth layer from
//! turbomcp-auth, allowing authentication to be composed as a middleware layer.

mod layer;
mod service;

pub use layer::ProxyLayer;
pub use service::{ProxyRequest, ProxyResponse, ProxyTowerService};

use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// Configuration for the proxy layer
#[derive(Debug, Clone)]
pub struct ProxyLayerConfig {
    /// Request timeout
    pub timeout: Duration,
    /// Whether to include timing metadata in responses
    pub include_timing: bool,
    /// Methods that bypass proxy processing (handled directly)
    pub bypass_methods: Vec<String>,
    /// Default metadata to include in all requests
    pub default_metadata: HashMap<String, Value>,
    /// Whether to log request/response details
    pub enable_logging: bool,
}

impl Default for ProxyLayerConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            include_timing: true,
            bypass_methods: Vec::new(),
            default_metadata: HashMap::new(),
            enable_logging: true,
        }
    }
}

impl ProxyLayerConfig {
    /// Create a new config with custom timeout
    #[must_use]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Default::default()
        }
    }

    /// Set request timeout
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Add a method to bypass proxy processing
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

    /// Enable or disable timing metadata
    #[must_use]
    pub fn include_timing(mut self, include: bool) -> Self {
        self.include_timing = include;
        self
    }

    /// Enable or disable logging
    #[must_use]
    pub fn enable_logging(mut self, enable: bool) -> Self {
        self.enable_logging = enable;
        self
    }

    /// Check if a method should bypass proxy processing
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
        let config = ProxyLayerConfig::default();
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert!(config.include_timing);
        assert!(config.bypass_methods.is_empty());
        assert!(config.default_metadata.is_empty());
        assert!(config.enable_logging);
    }

    #[test]
    fn test_with_timeout() {
        let config = ProxyLayerConfig::with_timeout(Duration::from_secs(60));
        assert_eq!(config.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_bypass_methods() {
        let config = ProxyLayerConfig::default()
            .bypass_method("ping")
            .bypass_method("health");
        assert!(config.should_bypass("ping"));
        assert!(config.should_bypass("health"));
        assert!(!config.should_bypass("tools/call"));
    }

    #[test]
    fn test_with_metadata() {
        let config = ProxyLayerConfig::default()
            .with_metadata("version", json!("1.0.0"))
            .with_metadata("proxy_id", json!("proxy-1"));
        assert_eq!(
            config.default_metadata.get("version"),
            Some(&json!("1.0.0"))
        );
        assert_eq!(
            config.default_metadata.get("proxy_id"),
            Some(&json!("proxy-1"))
        );
    }

    #[test]
    fn test_config_builder() {
        let config = ProxyLayerConfig::default()
            .timeout(Duration::from_secs(120))
            .include_timing(false)
            .enable_logging(false);
        assert_eq!(config.timeout, Duration::from_secs(120));
        assert!(!config.include_timing);
        assert!(!config.enable_logging);
    }
}

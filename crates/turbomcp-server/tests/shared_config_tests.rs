//! Shared configuration testing utilities
//!
//! This module provides DRY utilities for testing common configuration patterns
//! across the TurboMCP server crate, eliminating true duplicate test functions
//! while preserving all functionality.

use std::time::Duration;
use turbomcp_server::config::{ServerConfig, TimeoutConfig};
use turbomcp_server::registry::RegistryConfig;
use turbomcp_server::routing::RouterConfig;

/// Test utility for RegistryConfig default values
/// Replaces identical test_registry_config_default functions
pub fn assert_registry_config_defaults(config: &RegistryConfig) {
    assert_eq!(config.max_handlers_per_type, 1000);
    assert!(config.enable_metrics);
    assert!(config.enable_validation);
    assert_eq!(config.handler_timeout_ms, 30_000);
    assert!(!config.enable_hot_reload);
    assert!(config.event_listeners.is_empty());
}

/// Test utility for RouterConfig default values
/// Replaces identical test_router_config_default functions  
pub fn assert_router_config_defaults(config: &RouterConfig) {
    assert!(config.validate_requests);
    assert!(config.validate_responses);
    assert_eq!(config.default_timeout_ms, 30_000);
    assert!(config.enable_tracing);
}

/// Test utility for ServerConfig default values
pub fn assert_server_config_defaults(config: &ServerConfig) {
    assert_eq!(config.name, "turbomcp-server");
    assert_eq!(config.version, "1.0.10");
    // Additional server config assertions can be added here
}

/// Test utility for TimeoutConfig default values
pub fn assert_timeout_config_defaults(config: &TimeoutConfig) {
    assert_eq!(config.request_timeout, Duration::from_secs(30));
    assert_eq!(config.connection_timeout, Duration::from_secs(10));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_config_defaults_shared() {
        let config = RegistryConfig::default();
        assert_registry_config_defaults(&config);
    }

    #[test]
    fn test_router_config_defaults_shared() {
        let config = RouterConfig::default();
        assert_router_config_defaults(&config);
    }

    #[test]
    fn test_server_config_defaults_shared() {
        let config = ServerConfig::default();
        assert_server_config_defaults(&config);
    }

    #[test]
    fn test_timeout_config_defaults_shared() {
        let config = TimeoutConfig::default();
        assert_timeout_config_defaults(&config);
    }
}

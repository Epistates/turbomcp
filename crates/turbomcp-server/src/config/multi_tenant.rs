//! Multi-tenant configuration management
//!
//! Provides traits and implementations for per-tenant configuration in multi-tenant SaaS deployments.
//! Single-tenant applications can ignore this module.
//!
//! ## Features
//!
//! - **Per-tenant rate limits**: Different rate limits per tenant
//! - **Per-tenant timeouts**: Custom timeout settings per tenant
//! - **Per-tenant tool access**: Enable/disable specific tools per tenant
//! - **Dynamic configuration**: Load tenant config from files, databases, or remote services
//!
//! ## Example
//!
//! ```rust
//! use turbomcp_server::config::multi_tenant::{TenantConfigProvider, TenantConfig};
//! use std::collections::HashMap;
//!
//! struct StaticTenantConfigProvider {
//!     configs: HashMap<String, TenantConfig>,
//! }
//!
//! #[async_trait::async_trait]
//! impl TenantConfigProvider for StaticTenantConfigProvider {
//!     async fn get_config(&self, tenant_id: &str) -> Option<TenantConfig> {
//!         self.configs.get(tenant_id).cloned()
//!     }
//! }
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Per-tenant configuration overrides
///
/// All fields are optional - only specified fields override the global configuration.
/// This allows flexible per-tenant customization without duplicating all settings.
///
/// ## Example
///
/// ```rust
/// use turbomcp_server::config::multi_tenant::TenantConfig;
/// use std::collections::HashSet;
///
/// let config = TenantConfig {
///     rate_limit_per_second: Some(100),  // Custom rate limit
///     max_concurrent_requests: Some(10),  // Concurrent request limit
///     enabled_tools: Some(["tool1".to_string(), "tool2".to_string()].into_iter().collect()),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TenantConfig {
    /// Maximum requests per second for this tenant
    pub rate_limit_per_second: Option<u32>,

    /// Maximum concurrent requests for this tenant
    pub max_concurrent_requests: Option<usize>,

    /// Maximum tool execution timeout in milliseconds
    pub tool_timeout_ms: Option<u64>,

    /// Set of enabled tool names (if None, all tools are enabled)
    pub enabled_tools: Option<HashSet<String>>,

    /// Set of disabled tool names (takes precedence over enabled_tools)
    pub disabled_tools: Option<HashSet<String>>,

    /// Maximum request body size in bytes
    pub max_request_body_size: Option<usize>,

    /// Whether this tenant is active (false = reject all requests)
    pub is_active: Option<bool>,

    /// Custom metadata for application-specific use
    pub metadata: HashMap<String, serde_json::Value>,
}

impl TenantConfig {
    /// Check if a tool is enabled for this tenant
    ///
    /// Returns true if:
    /// - No tool restrictions are configured (default: all enabled)
    /// - Tool is in enabled_tools and not in disabled_tools
    /// - disabled_tools takes precedence
    pub fn is_tool_enabled(&self, tool_name: &str) -> bool {
        // Check disabled list first (takes precedence)
        if let Some(disabled) = &self.disabled_tools {
            if disabled.contains(tool_name) {
                return false;
            }
        }

        // Check enabled list
        if let Some(enabled) = &self.enabled_tools {
            enabled.contains(tool_name)
        } else {
            // No restrictions = all enabled
            true
        }
    }

    /// Get the tool timeout for this tenant, or None to use global default
    pub fn tool_timeout(&self) -> Option<Duration> {
        self.tool_timeout_ms.map(Duration::from_millis)
    }

    /// Check if this tenant is active
    pub fn is_active(&self) -> bool {
        self.is_active.unwrap_or(true)
    }
}

/// Provider trait for loading per-tenant configuration
///
/// Implement this trait to load tenant configuration from your preferred source:
/// - Static HashMap (testing)
/// - Configuration files (TOML/YAML)
/// - Database (PostgreSQL, MySQL)
/// - Remote config service (Consul, etcd)
/// - Environment-specific logic
///
/// ## Example: File-based Provider
///
/// ```rust,no_run
/// use turbomcp_server::config::multi_tenant::{TenantConfigProvider, TenantConfig};
/// use std::collections::HashMap;
///
/// struct FileTenantConfigProvider {
///     configs: HashMap<String, TenantConfig>,
/// }
///
/// impl FileTenantConfigProvider {
///     async fn load_from_file(path: &str) -> std::io::Result<Self> {
///         let contents = tokio::fs::read_to_string(path).await?;
///         let configs: HashMap<String, TenantConfig> = serde_yaml::from_str(&contents)
///             .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
///         Ok(Self { configs })
///     }
/// }
///
/// #[async_trait::async_trait]
/// impl TenantConfigProvider for FileTenantConfigProvider {
///     async fn get_config(&self, tenant_id: &str) -> Option<TenantConfig> {
///         self.configs.get(tenant_id).cloned()
///     }
/// }
/// ```
#[async_trait]
pub trait TenantConfigProvider: Send + Sync {
    /// Get configuration for a specific tenant
    ///
    /// Returns `None` if tenant is not found (will use global config)
    async fn get_config(&self, tenant_id: &str) -> Option<TenantConfig>;

    /// Optional: Refresh/reload configuration from source
    ///
    /// Default implementation does nothing. Override to implement hot-reload.
    async fn refresh(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Optional: Get all tenant IDs
    ///
    /// Useful for admin/monitoring endpoints. Default returns empty vec.
    async fn list_tenants(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Static in-memory tenant config provider for testing
///
/// Useful for development and testing. Not recommended for production.
///
/// ## Example
///
/// ```rust
/// use turbomcp_server::config::multi_tenant::{StaticTenantConfigProvider, TenantConfig};
/// use std::collections::HashMap;
///
/// let mut configs = HashMap::new();
/// configs.insert("acme-corp".to_string(), TenantConfig {
///     rate_limit_per_second: Some(1000),
///     ..Default::default()
/// });
///
/// let provider = StaticTenantConfigProvider::new(configs);
/// ```
#[derive(Debug, Clone)]
pub struct StaticTenantConfigProvider {
    configs: HashMap<String, TenantConfig>,
}

impl StaticTenantConfigProvider {
    /// Create a new static tenant config provider
    pub fn new(configs: HashMap<String, TenantConfig>) -> Self {
        Self { configs }
    }
}

#[async_trait]
impl TenantConfigProvider for StaticTenantConfigProvider {
    async fn get_config(&self, tenant_id: &str) -> Option<TenantConfig> {
        self.configs.get(tenant_id).cloned()
    }

    async fn list_tenants(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }
}

/// No-op tenant config provider (always returns None)
///
/// Used as default when multi-tenancy is not configured.
/// Always returns None, causing system to fall back to global configuration.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpTenantConfigProvider;

#[async_trait]
impl TenantConfigProvider for NoOpTenantConfigProvider {
    async fn get_config(&self, _tenant_id: &str) -> Option<TenantConfig> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_tool_enabled_default() {
        let config = TenantConfig::default();
        assert!(config.is_tool_enabled("any_tool"));
    }

    #[test]
    fn test_is_tool_enabled_with_enabled_list() {
        let mut enabled = HashSet::new();
        enabled.insert("tool1".to_string());
        enabled.insert("tool2".to_string());

        let config = TenantConfig {
            enabled_tools: Some(enabled),
            ..Default::default()
        };

        assert!(config.is_tool_enabled("tool1"));
        assert!(config.is_tool_enabled("tool2"));
        assert!(!config.is_tool_enabled("tool3"));
    }

    #[test]
    fn test_is_tool_enabled_with_disabled_list() {
        let mut disabled = HashSet::new();
        disabled.insert("tool2".to_string());

        let config = TenantConfig {
            disabled_tools: Some(disabled),
            ..Default::default()
        };

        assert!(config.is_tool_enabled("tool1"));
        assert!(!config.is_tool_enabled("tool2"));
    }

    #[test]
    fn test_disabled_takes_precedence() {
        let mut enabled = HashSet::new();
        enabled.insert("tool1".to_string());
        enabled.insert("tool2".to_string());

        let mut disabled = HashSet::new();
        disabled.insert("tool1".to_string());

        let config = TenantConfig {
            enabled_tools: Some(enabled),
            disabled_tools: Some(disabled),
            ..Default::default()
        };

        // tool1 is in both - disabled wins
        assert!(!config.is_tool_enabled("tool1"));
        assert!(config.is_tool_enabled("tool2"));
    }

    #[tokio::test]
    async fn test_static_provider() {
        let mut configs = HashMap::new();
        configs.insert(
            "tenant1".to_string(),
            TenantConfig {
                rate_limit_per_second: Some(100),
                ..Default::default()
            },
        );

        let provider = StaticTenantConfigProvider::new(configs);

        let config = provider.get_config("tenant1").await;
        assert!(config.is_some());
        assert_eq!(config.unwrap().rate_limit_per_second, Some(100));

        let config = provider.get_config("tenant2").await;
        assert!(config.is_none());
    }

    #[tokio::test]
    async fn test_noop_provider() {
        let provider = NoOpTenantConfigProvider;
        assert!(provider.get_config("any_tenant").await.is_none());
    }
}

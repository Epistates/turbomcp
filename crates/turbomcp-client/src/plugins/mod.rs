//! Plugin system for TurboMCP client
//!
//! Provides an extensible plugin architecture with lifecycle hooks and middleware patterns
//! for extending client functionality. Plugins can intercept requests and responses,
//! handle custom methods, and add features like metrics, retries, and caching.
//!
//! ## Architecture
//!
//! The plugin system follows a middleware pattern where plugins are executed in order:
//!
//! ```text
//! Request → Plugin 1 → Plugin 2 → Plugin N → Server
//!          ↓            ↓           ↓
//! Response ← Plugin 1 ← Plugin 2 ← Plugin N ← Server
//! ```
//!
//! ## Core Components
//!
//! - **ClientPlugin**: Core trait defining plugin lifecycle and hooks
//! - **PluginRegistry**: Manages plugin registration, ordering, and execution
//! - **RequestContext/ResponseContext**: Context objects passed to plugins
//! - **Example Plugins**: MetricsPlugin, RetryPlugin, CachePlugin
//!
//! ## Usage
//!
//! ```rust,no_run
//! use turbomcp_client::plugins::{PluginRegistry, MetricsPlugin, PluginConfig};
//! use std::sync::Arc;
//!
//! async fn example() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut registry = PluginRegistry::new();
//!
//!     // Register built-in plugins
//!     let metrics = Arc::new(MetricsPlugin::new(PluginConfig::Metrics));
//!     registry.register_plugin(metrics).await?;
//!     Ok(())
//! }
//! ```

pub mod core;
pub mod examples;
pub mod middleware;
pub mod registry;

// Re-export core types for public API
pub use core::{
    ClientPlugin, PluginConfig, PluginContext, PluginError, PluginResult, RequestContext,
    ResponseContext,
};

pub use registry::PluginRegistry;

pub use examples::{
    CacheConfig, CachePlugin, MetricsData, MetricsPlugin, RetryConfig, RetryPlugin,
};

pub use middleware::{MiddlewareChain, MiddlewareResult, RequestMiddleware, ResponseMiddleware};

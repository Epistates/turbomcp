//! Tower-native middleware for MCP client.
//!
//! v3.0 introduces pure Tower middleware, replacing the v2.x plugin system.
//! All middleware is composable via `tower::ServiceBuilder`.
//!
//! ## Migration from v2.x Plugins
//!
//! | v2.x Plugin | v3.0 Tower Layer |
//! |-------------|------------------|
//! | `MetricsPlugin` | [`MetricsLayer`] |
//! | `RetryPlugin` | `tower::retry::RetryLayer` |
//! | `CachePlugin` | [`CacheLayer`] |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use turbomcp_client::middleware::{MetricsLayer, CacheLayer, TracingLayer};
//! use std::time::Duration;
//!
//! let client = ServiceBuilder::new()
//!     .layer(TracingLayer::new())
//!     .layer(MetricsLayer::new())
//!     .layer(CacheLayer::default())
//!     .timeout(Duration::from_secs(30))
//!     .service(transport);
//! ```

mod metrics;
mod cache;
mod tracing_layer;
mod request;

pub use metrics::{Metrics, MetricsLayer, MetricsService, MetricsSnapshot};
pub use cache::{Cache, CacheConfig, CacheLayer, CacheService};
pub use tracing_layer::{TracingLayer, TracingService};
pub use request::{McpRequest, McpResponse};

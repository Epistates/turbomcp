//! Tower middleware for automatic MCP request instrumentation
//!
//! Provides [`TelemetryLayer`] that automatically creates spans and records
//! metrics for all MCP requests passing through the middleware stack.
//!
//! # Example
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use turbomcp_telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};
//!
//! let telemetry_layer = TelemetryLayer::new(TelemetryLayerConfig::default());
//!
//! let service = ServiceBuilder::new()
//!     .layer(telemetry_layer)
//!     .service(my_mcp_handler);
//! ```

mod layer;
mod service;

pub use layer::TelemetryLayer;
pub use service::{TelemetryService, TelemetryServiceFuture};

use std::time::Duration;

/// Configuration for the telemetry middleware layer
#[derive(Debug, Clone)]
pub struct TelemetryLayerConfig {
    /// Service name for span attribution
    pub service_name: String,
    /// Service version for span attribution
    pub service_version: String,
    /// Whether to record request/response sizes
    pub record_sizes: bool,
    /// Whether to record request timing
    pub record_timing: bool,
    /// Methods to exclude from instrumentation
    pub excluded_methods: Vec<String>,
    /// Whether to propagate trace context from incoming requests
    pub propagate_context: bool,
}

impl Default for TelemetryLayerConfig {
    fn default() -> Self {
        Self {
            service_name: "turbomcp-service".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            record_sizes: true,
            record_timing: true,
            excluded_methods: Vec::new(),
            propagate_context: true,
        }
    }
}

impl TelemetryLayerConfig {
    /// Create a new configuration with default values
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the service name
    #[must_use]
    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = name.into();
        self
    }

    /// Set the service version
    #[must_use]
    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.service_version = version.into();
        self
    }

    /// Enable or disable request/response size recording
    #[must_use]
    pub fn record_sizes(mut self, enabled: bool) -> Self {
        self.record_sizes = enabled;
        self
    }

    /// Enable or disable timing recording
    #[must_use]
    pub fn record_timing(mut self, enabled: bool) -> Self {
        self.record_timing = enabled;
        self
    }

    /// Add a method to exclude from instrumentation
    #[must_use]
    pub fn exclude_method(mut self, method: impl Into<String>) -> Self {
        self.excluded_methods.push(method.into());
        self
    }

    /// Enable or disable trace context propagation
    #[must_use]
    pub fn propagate_context(mut self, enabled: bool) -> Self {
        self.propagate_context = enabled;
        self
    }

    /// Check if a method should be instrumented
    #[must_use]
    pub fn should_instrument(&self, method: &str) -> bool {
        !self.excluded_methods.iter().any(|m| m == method)
    }
}

/// Recorded span data for a request
#[derive(Debug, Clone)]
pub struct SpanData {
    /// MCP method name
    pub method: String,
    /// Request ID
    pub request_id: Option<String>,
    /// Request duration
    pub duration: Duration,
    /// Whether the request succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Request size in bytes
    pub request_size: Option<usize>,
    /// Response size in bytes
    pub response_size: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = TelemetryLayerConfig::default();
        assert_eq!(config.service_name, "turbomcp-service");
        assert!(config.record_sizes);
        assert!(config.record_timing);
        assert!(config.excluded_methods.is_empty());
        assert!(config.propagate_context);
    }

    #[test]
    fn test_config_builder() {
        let config = TelemetryLayerConfig::new()
            .service_name("my-service")
            .service_version("2.0.0")
            .record_sizes(false)
            .exclude_method("ping")
            .exclude_method("initialize");

        assert_eq!(config.service_name, "my-service");
        assert_eq!(config.service_version, "2.0.0");
        assert!(!config.record_sizes);
        assert_eq!(config.excluded_methods.len(), 2);
    }

    #[test]
    fn test_should_instrument() {
        let config = TelemetryLayerConfig::new()
            .exclude_method("ping")
            .exclude_method("notifications/initialized");

        assert!(config.should_instrument("tools/call"));
        assert!(config.should_instrument("resources/read"));
        assert!(!config.should_instrument("ping"));
        assert!(!config.should_instrument("notifications/initialized"));
    }
}

//! Tower Layer implementation for telemetry

use super::{TelemetryLayerConfig, TelemetryService};
use std::sync::Arc;
use tower::Layer;

/// Tower Layer that adds telemetry instrumentation to services
///
/// This layer wraps services with automatic span creation, timing recording,
/// and metrics collection for MCP requests.
///
/// # Example
///
/// ```rust,ignore
/// use tower::ServiceBuilder;
/// use turbomcp_telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};
///
/// let config = TelemetryLayerConfig::new()
///     .service_name("my-mcp-server")
///     .exclude_method("ping");
///
/// let telemetry_layer = TelemetryLayer::new(config);
///
/// let service = ServiceBuilder::new()
///     .layer(telemetry_layer)
///     .service(inner_service);
/// ```
#[derive(Debug, Clone)]
pub struct TelemetryLayer {
    config: Arc<TelemetryLayerConfig>,
}

impl TelemetryLayer {
    /// Create a new telemetry layer with the given configuration
    #[must_use]
    pub fn new(config: TelemetryLayerConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }

    /// Create a new telemetry layer with default configuration
    #[must_use]
    pub fn default_layer() -> Self {
        Self::new(TelemetryLayerConfig::default())
    }

    /// Get the configuration
    #[must_use]
    pub fn config(&self) -> &TelemetryLayerConfig {
        &self.config
    }
}

impl Default for TelemetryLayer {
    fn default() -> Self {
        Self::default_layer()
    }
}

impl<S> Layer<S> for TelemetryLayer {
    type Service = TelemetryService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TelemetryService::new(inner, Arc::clone(&self.config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_creation() {
        let layer = TelemetryLayer::new(TelemetryLayerConfig::default());
        assert_eq!(layer.config().service_name, "turbomcp-service");
    }

    #[test]
    fn test_layer_default() {
        let layer = TelemetryLayer::default();
        assert!(layer.config().record_timing);
    }
}

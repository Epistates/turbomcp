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
#[cfg(feature = "opentelemetry")]
mod propagation;
mod service;

pub use layer::TelemetryLayer;
pub use service::{TelemetryService, TelemetryServiceFuture};

use std::time::Duration;

/// Configuration for the telemetry middleware layer
///
/// # Cardinality and PII
///
/// Several recorded fields can be expensive in OTel backends or expose user data:
/// - `mcp.request.id` is unique per request (highest possible cardinality).
/// - `mcp.session.id` / `mcp.user.id` / `mcp.tenant.id` track real principals.
/// - `mcp.resource.uri` is client-controlled and routinely carries user data
///   or credentials (home-directory paths, account IDs, signed-URL tokens),
///   and a hostile client can inflate cardinality with synthetic URIs. It is
///   therefore **not recorded unless you opt in** with
///   [`Self::redact_resource_uri`]`(false)`.
///
/// `mcp.error.message` echoes JSON-RPC error strings verbatim, which routinely
/// contain user input, file paths, SQL fragments, and backend stack traces. The
/// layer truncates error messages to [`Self::error_message_max_len`] before
/// recording; set to `0` to drop entirely. Toggle [`Self::redact_request_id`]
/// to omit the request ID.
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
    /// Whether to continue the caller's trace. Default `true`.
    ///
    /// When set, W3C trace context (`traceparent` / `tracestate`) found in a
    /// JSON-RPC request's `params._meta`, or in an HTTP request's headers,
    /// becomes the parent of the `mcp.request` span. Requests without valid
    /// trace context keep their local parent. Takes effect only when the
    /// subscriber includes a `tracing-opentelemetry` layer, such as the one
    /// [`TelemetryConfig::init`](crate::TelemetryConfig::init) installs when
    /// an OTLP endpoint is configured.
    ///
    /// Trace context is caller-supplied, so a client can choose which trace
    /// its requests land in. Turn this off at a trust boundary where that
    /// matters.
    #[cfg(feature = "opentelemetry")]
    #[cfg_attr(docsrs, doc(cfg(feature = "opentelemetry")))]
    pub propagate_context: bool,
    /// Maximum length (in bytes) at which `mcp.error.message` is truncated
    /// before being recorded on a span. Default `512`. Set to `0` to drop
    /// error messages entirely.
    pub error_message_max_len: usize,
    /// Skip recording `mcp.request.id` to avoid the per-request cardinality
    /// explosion in cardinality-sensitive backends.
    pub redact_request_id: bool,
    /// Skip recording `mcp.resource.uri` (client-controlled, unbounded, and
    /// often sensitive). Default `true`; set to `false` to record it.
    pub redact_resource_uri: bool,
}

impl Default for TelemetryLayerConfig {
    fn default() -> Self {
        Self {
            service_name: "turbomcp-service".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            record_sizes: true,
            record_timing: true,
            excluded_methods: Vec::new(),
            #[cfg(feature = "opentelemetry")]
            propagate_context: true,
            error_message_max_len: 512,
            redact_request_id: false,
            // Resource URIs can carry secrets and PII, so exporting them has
            // to be a deliberate choice rather than the default.
            redact_resource_uri: true,
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

    /// Enable or disable continuing the caller's trace from incoming W3C trace
    /// context. See the `propagate_context` field for what gets extracted.
    #[cfg(feature = "opentelemetry")]
    #[cfg_attr(docsrs, doc(cfg(feature = "opentelemetry")))]
    #[must_use]
    pub fn propagate_context(mut self, enabled: bool) -> Self {
        self.propagate_context = enabled;
        self
    }

    /// Set the maximum length (in bytes) for `mcp.error.message`. Set to `0`
    /// to drop the field entirely.
    #[must_use]
    pub fn error_message_max_len(mut self, max_len: usize) -> Self {
        self.error_message_max_len = max_len;
        self
    }

    /// Skip recording `mcp.request.id` (high cardinality).
    #[must_use]
    pub fn redact_request_id(mut self, enabled: bool) -> Self {
        self.redact_request_id = enabled;
        self
    }

    /// Skip recording `mcp.resource.uri` (client-controlled, unbounded, and
    /// often sensitive). On by default; pass `false` to record resource URIs.
    #[must_use]
    pub fn redact_resource_uri(mut self, enabled: bool) -> Self {
        self.redact_resource_uri = enabled;
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
        #[cfg(feature = "opentelemetry")]
        assert!(config.propagate_context);
        assert!(!config.redact_request_id);
        assert!(config.redact_resource_uri);
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

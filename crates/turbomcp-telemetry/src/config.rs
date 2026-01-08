//! Telemetry configuration
//!
//! Provides flexible configuration for telemetry collection and export.

#[cfg(feature = "opentelemetry")]
use std::time::Duration;

/// Telemetry configuration
///
/// Use [`TelemetryConfigBuilder`] for ergonomic configuration construction.
///
/// # Example
///
/// ```rust
/// use turbomcp_telemetry::TelemetryConfig;
///
/// let config = TelemetryConfig::builder()
///     .service_name("my-mcp-server")
///     .service_version("1.0.0")
///     .log_level("info,turbomcp=debug")
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Service name for telemetry identification
    pub service_name: String,
    /// Service version
    pub service_version: String,
    /// Log level filter (e.g., "info", "debug", "info,turbomcp=debug")
    pub log_level: String,
    /// Enable JSON-formatted log output
    pub json_logs: bool,
    /// Output logs to stderr (required for STDIO transport)
    pub stderr_output: bool,

    /// OpenTelemetry OTLP endpoint (e.g., "http://localhost:4317")
    #[cfg(feature = "opentelemetry")]
    pub otlp_endpoint: Option<String>,
    /// OTLP protocol (grpc or http)
    #[cfg(feature = "opentelemetry")]
    pub otlp_protocol: OtlpProtocol,
    /// Trace sampling ratio (0.0 to 1.0)
    #[cfg(feature = "opentelemetry")]
    pub sampling_ratio: f64,
    /// Export timeout
    #[cfg(feature = "opentelemetry")]
    pub export_timeout: Duration,

    /// Prometheus metrics endpoint port
    #[cfg(feature = "prometheus")]
    pub prometheus_port: Option<u16>,
    /// Prometheus metrics endpoint path
    #[cfg(feature = "prometheus")]
    pub prometheus_path: String,

    /// Additional resource attributes
    pub resource_attributes: Vec<(String, String)>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            service_name: "turbomcp-service".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            log_level: "info,turbomcp=debug".to_string(),
            json_logs: true,
            stderr_output: true,

            #[cfg(feature = "opentelemetry")]
            otlp_endpoint: None,
            #[cfg(feature = "opentelemetry")]
            otlp_protocol: OtlpProtocol::Grpc,
            #[cfg(feature = "opentelemetry")]
            sampling_ratio: 1.0,
            #[cfg(feature = "opentelemetry")]
            export_timeout: Duration::from_secs(10),

            #[cfg(feature = "prometheus")]
            prometheus_port: None,
            #[cfg(feature = "prometheus")]
            prometheus_path: "/metrics".to_string(),

            resource_attributes: Vec::new(),
        }
    }
}

impl TelemetryConfig {
    /// Create a new configuration builder
    #[must_use]
    pub fn builder() -> TelemetryConfigBuilder {
        TelemetryConfigBuilder::default()
    }

    /// Initialize telemetry with this configuration
    ///
    /// Returns a guard that ensures proper cleanup on drop.
    pub fn init(self) -> Result<crate::TelemetryGuard, crate::TelemetryError> {
        crate::TelemetryGuard::init(self)
    }
}

/// OTLP protocol variant
#[cfg(feature = "opentelemetry")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OtlpProtocol {
    /// gRPC protocol (port 4317)
    #[default]
    Grpc,
    /// HTTP/protobuf protocol (port 4318)
    Http,
}

/// Builder for [`TelemetryConfig`]
#[derive(Debug, Clone, Default)]
pub struct TelemetryConfigBuilder {
    service_name: Option<String>,
    service_version: Option<String>,
    log_level: Option<String>,
    json_logs: Option<bool>,
    stderr_output: Option<bool>,

    #[cfg(feature = "opentelemetry")]
    otlp_endpoint: Option<String>,
    #[cfg(feature = "opentelemetry")]
    otlp_protocol: Option<OtlpProtocol>,
    #[cfg(feature = "opentelemetry")]
    sampling_ratio: Option<f64>,
    #[cfg(feature = "opentelemetry")]
    export_timeout: Option<Duration>,

    #[cfg(feature = "prometheus")]
    prometheus_port: Option<u16>,
    #[cfg(feature = "prometheus")]
    prometheus_path: Option<String>,

    resource_attributes: Vec<(String, String)>,
}

impl TelemetryConfigBuilder {
    /// Set the service name
    #[must_use]
    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = Some(name.into());
        self
    }

    /// Set the service version
    #[must_use]
    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.service_version = Some(version.into());
        self
    }

    /// Set the log level filter
    ///
    /// Examples: "info", "debug", "warn,turbomcp=debug,tower=info"
    #[must_use]
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = Some(level.into());
        self
    }

    /// Enable or disable JSON log output
    #[must_use]
    pub fn json_logs(mut self, enabled: bool) -> Self {
        self.json_logs = Some(enabled);
        self
    }

    /// Enable or disable stderr output (required for STDIO transport)
    #[must_use]
    pub fn stderr_output(mut self, enabled: bool) -> Self {
        self.stderr_output = Some(enabled);
        self
    }

    /// Set the OTLP endpoint for trace/metrics export
    #[cfg(feature = "opentelemetry")]
    #[cfg_attr(docsrs, doc(cfg(feature = "opentelemetry")))]
    #[must_use]
    pub fn otlp_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.otlp_endpoint = Some(endpoint.into());
        self
    }

    /// Set the OTLP protocol
    #[cfg(feature = "opentelemetry")]
    #[cfg_attr(docsrs, doc(cfg(feature = "opentelemetry")))]
    #[must_use]
    pub fn otlp_protocol(mut self, protocol: OtlpProtocol) -> Self {
        self.otlp_protocol = Some(protocol);
        self
    }

    /// Set the trace sampling ratio (0.0 to 1.0)
    #[cfg(feature = "opentelemetry")]
    #[cfg_attr(docsrs, doc(cfg(feature = "opentelemetry")))]
    #[must_use]
    pub fn sampling_ratio(mut self, ratio: f64) -> Self {
        self.sampling_ratio = Some(ratio.clamp(0.0, 1.0));
        self
    }

    /// Set the export timeout
    #[cfg(feature = "opentelemetry")]
    #[cfg_attr(docsrs, doc(cfg(feature = "opentelemetry")))]
    #[must_use]
    pub fn export_timeout(mut self, timeout: Duration) -> Self {
        self.export_timeout = Some(timeout);
        self
    }

    /// Set the Prometheus metrics endpoint port
    #[cfg(feature = "prometheus")]
    #[cfg_attr(docsrs, doc(cfg(feature = "prometheus")))]
    #[must_use]
    pub fn prometheus_port(mut self, port: u16) -> Self {
        self.prometheus_port = Some(port);
        self
    }

    /// Set the Prometheus metrics endpoint path
    #[cfg(feature = "prometheus")]
    #[cfg_attr(docsrs, doc(cfg(feature = "prometheus")))]
    #[must_use]
    pub fn prometheus_path(mut self, path: impl Into<String>) -> Self {
        self.prometheus_path = Some(path.into());
        self
    }

    /// Add a resource attribute
    #[must_use]
    pub fn resource_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.resource_attributes.push((key.into(), value.into()));
        self
    }

    /// Add the deployment environment as a resource attribute
    #[must_use]
    pub fn environment(self, env: impl Into<String>) -> Self {
        self.resource_attribute("deployment.environment", env)
    }

    /// Build the configuration
    #[must_use]
    pub fn build(self) -> TelemetryConfig {
        let defaults = TelemetryConfig::default();

        TelemetryConfig {
            service_name: self.service_name.unwrap_or(defaults.service_name),
            service_version: self.service_version.unwrap_or(defaults.service_version),
            log_level: self.log_level.unwrap_or(defaults.log_level),
            json_logs: self.json_logs.unwrap_or(defaults.json_logs),
            stderr_output: self.stderr_output.unwrap_or(defaults.stderr_output),

            #[cfg(feature = "opentelemetry")]
            otlp_endpoint: self.otlp_endpoint.or(defaults.otlp_endpoint),
            #[cfg(feature = "opentelemetry")]
            otlp_protocol: self.otlp_protocol.unwrap_or(defaults.otlp_protocol),
            #[cfg(feature = "opentelemetry")]
            sampling_ratio: self.sampling_ratio.unwrap_or(defaults.sampling_ratio),
            #[cfg(feature = "opentelemetry")]
            export_timeout: self.export_timeout.unwrap_or(defaults.export_timeout),

            #[cfg(feature = "prometheus")]
            prometheus_port: self.prometheus_port.or(defaults.prometheus_port),
            #[cfg(feature = "prometheus")]
            prometheus_path: self.prometheus_path.unwrap_or(defaults.prometheus_path),

            resource_attributes: if self.resource_attributes.is_empty() {
                defaults.resource_attributes
            } else {
                self.resource_attributes
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TelemetryConfig::default();
        assert_eq!(config.service_name, "turbomcp-service");
        assert!(config.json_logs);
        assert!(config.stderr_output);
    }

    #[test]
    fn test_builder() {
        let config = TelemetryConfig::builder()
            .service_name("test-service")
            .service_version("2.0.0")
            .log_level("debug")
            .json_logs(false)
            .environment("production")
            .build();

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.service_version, "2.0.0");
        assert_eq!(config.log_level, "debug");
        assert!(!config.json_logs);
        assert_eq!(config.resource_attributes.len(), 1);
        assert_eq!(
            config.resource_attributes[0],
            (
                "deployment.environment".to_string(),
                "production".to_string()
            )
        );
    }

    #[cfg(feature = "opentelemetry")]
    #[test]
    fn test_otlp_config() {
        let config = TelemetryConfig::builder()
            .otlp_endpoint("http://localhost:4317")
            .otlp_protocol(OtlpProtocol::Grpc)
            .sampling_ratio(0.5)
            .build();

        assert_eq!(
            config.otlp_endpoint,
            Some("http://localhost:4317".to_string())
        );
        assert_eq!(config.otlp_protocol, OtlpProtocol::Grpc);
        assert!((config.sampling_ratio - 0.5).abs() < f64::EPSILON);
    }

    #[cfg(feature = "prometheus")]
    #[test]
    fn test_prometheus_config() {
        let config = TelemetryConfig::builder()
            .prometheus_port(9090)
            .prometheus_path("/custom-metrics")
            .build();

        assert_eq!(config.prometheus_port, Some(9090));
        assert_eq!(config.prometheus_path, "/custom-metrics");
    }
}

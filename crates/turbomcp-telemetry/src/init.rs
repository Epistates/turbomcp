//! Telemetry initialization
//!
//! Provides the [`TelemetryGuard`] for managing telemetry lifecycle.

use crate::{TelemetryConfig, TelemetryError};
use tracing::info;
use tracing_subscriber::{
    Registry, filter::EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

/// Guard that manages telemetry lifecycle
///
/// When dropped, ensures proper cleanup of telemetry resources including
/// flushing any pending trace/metric data to exporters.
///
/// # Critical: Drop Behavior
///
/// **The `TelemetryGuard` MUST outlive all traced code in your application.**
///
/// When the guard is dropped, its `Drop` implementation:
/// 1. Flushes all pending traces to configured exporters (OTLP, etc.)
/// 2. Shuts down the OpenTelemetry tracer provider
/// 3. Releases telemetry resources
///
/// ## Common Pitfall
///
/// ```rust,ignore
/// // ❌ WRONG: Guard dropped too early
/// {
///     let _guard = TelemetryConfig::default().init()?;
/// } // Guard dropped here
/// my_traced_function().await; // Traces lost!
/// ```
///
/// ```rust,ignore
/// // ✅ CORRECT: Guard outlives traced code
/// let _guard = TelemetryConfig::default().init()?;
/// my_traced_function().await;
/// // Guard dropped at end of scope, traces flushed
/// ```
///
/// ## Best Practice
///
/// Store the guard in your main application struct or as a variable in `main()`:
///
/// ```rust,ignore
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     let _telemetry = TelemetryConfig::default().init()?;
///
///     // Run your server
///     run_server().await?;
///
///     Ok(())
///     // Guard dropped here after server shutdown
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_telemetry::{TelemetryConfig, TelemetryGuard};
///
/// let config = TelemetryConfig::builder()
///     .service_name("my-server")
///     .build();
///
/// // Initialize telemetry - guard must be kept alive
/// let _guard = config.init()?;
///
/// // Your application code here...
///
/// // Telemetry is properly cleaned up when guard is dropped
/// ```
pub struct TelemetryGuard {
    config: TelemetryConfig,
    #[cfg(feature = "opentelemetry")]
    tracer_provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    #[cfg(feature = "prometheus")]
    _metrics_handle: Option<MetricsHandle>,
}

impl std::fmt::Debug for TelemetryGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("TelemetryGuard");
        debug.field("config", &self.config);
        #[cfg(feature = "opentelemetry")]
        debug.field(
            "tracer_provider",
            &self.tracer_provider.as_ref().map(|_| "SdkTracerProvider"),
        );
        debug.finish()
    }
}

#[cfg(feature = "prometheus")]
struct MetricsHandle {
    // Handle to the metrics exporter for cleanup
    _handle: metrics_exporter_prometheus::PrometheusHandle,
}

#[cfg(feature = "prometheus")]
impl std::fmt::Debug for MetricsHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetricsHandle").finish()
    }
}

impl TelemetryGuard {
    /// Initialize telemetry with the provided configuration
    pub fn init(config: TelemetryConfig) -> Result<Self, TelemetryError> {
        // Initialize OpenTelemetry provider if configured
        #[cfg(feature = "opentelemetry")]
        let tracer_provider = if config.otlp_endpoint.is_some() {
            Some(init_tracer_provider(&config)?)
        } else {
            None
        };

        // Build and initialize the subscriber based on configuration
        init_subscriber(
            &config,
            #[cfg(feature = "opentelemetry")]
            tracer_provider.as_ref(),
        )?;

        // Initialize Prometheus metrics if configured
        #[cfg(feature = "prometheus")]
        let metrics_handle = if let Some(port) = config.prometheus_port {
            Some(init_prometheus(&config, port)?)
        } else {
            None
        };

        info!(
            service_name = %config.service_name,
            service_version = %config.service_version,
            json_logs = config.json_logs,
            stderr_output = config.stderr_output,
            "TurboMCP telemetry initialized"
        );

        Ok(Self {
            config,
            #[cfg(feature = "opentelemetry")]
            tracer_provider,
            #[cfg(feature = "prometheus")]
            _metrics_handle: metrics_handle,
        })
    }

    /// Get the service name
    #[must_use]
    pub fn service_name(&self) -> &str {
        &self.config.service_name
    }

    /// Get the service version
    #[must_use]
    pub fn service_version(&self) -> &str {
        &self.config.service_version
    }

    /// Get the configuration
    #[must_use]
    pub fn config(&self) -> &TelemetryConfig {
        &self.config
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        info!(
            service_name = %self.config.service_name,
            "Shutting down TurboMCP telemetry"
        );

        // Shutdown OpenTelemetry provider if it was initialized
        #[cfg(feature = "opentelemetry")]
        if let Some(ref provider) = self.tracer_provider
            && let Err(e) = provider.shutdown()
        {
            tracing::error!("Error shutting down tracer provider: {e}");
        }
    }
}

/// Initialize the tracing subscriber with all configured layers
///
/// Due to Rust's type system and tracing's layered architecture, each configuration
/// combination requires its own complete initialization path. The OpenTelemetry layer
/// must be created fresh for each subscriber type.
fn init_subscriber(
    config: &TelemetryConfig,
    #[cfg(feature = "opentelemetry")] tracer_provider: Option<
        &opentelemetry_sdk::trace::SdkTracerProvider,
    >,
) -> Result<(), TelemetryError> {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.log_level))
        .map_err(|e| TelemetryError::InvalidConfiguration(format!("Invalid log level: {e}")))?;

    // Handle all configuration combinations
    // Note: We need completely separate initialization paths because the layer types differ

    #[cfg(feature = "opentelemetry")]
    if let Some(provider) = tracer_provider {
        return init_with_otel(config, env_filter, provider);
    }

    // No OpenTelemetry - just fmt layer
    init_without_otel(config, env_filter)
}

/// Initialize subscriber with OpenTelemetry layer
#[cfg(feature = "opentelemetry")]
fn init_with_otel(
    config: &TelemetryConfig,
    env_filter: EnvFilter,
    provider: &opentelemetry_sdk::trace::SdkTracerProvider,
) -> Result<(), TelemetryError> {
    use opentelemetry::trace::TracerProvider;

    let tracer = provider.tracer("turbomcp-telemetry");

    // Each branch needs its own otel_layer creation because the layer type
    // depends on the subscriber type it's being added to
    if config.json_logs && config.stderr_output {
        let fmt_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .json();

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        Registry::default()
            .with(env_filter)
            .with(otel_layer)
            .with(fmt_layer)
            .try_init()
            .map_err(|e| TelemetryError::TracingError(e.to_string()))
    } else if config.json_logs {
        let fmt_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .json();

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        Registry::default()
            .with(env_filter)
            .with(otel_layer)
            .with(fmt_layer)
            .try_init()
            .map_err(|e| TelemetryError::TracingError(e.to_string()))
    } else if config.stderr_output {
        let fmt_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_thread_ids(false)
            .pretty();

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        Registry::default()
            .with(env_filter)
            .with(otel_layer)
            .with(fmt_layer)
            .try_init()
            .map_err(|e| TelemetryError::TracingError(e.to_string()))
    } else {
        let fmt_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(false)
            .pretty();

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        Registry::default()
            .with(env_filter)
            .with(otel_layer)
            .with(fmt_layer)
            .try_init()
            .map_err(|e| TelemetryError::TracingError(e.to_string()))
    }
}

/// Initialize subscriber without OpenTelemetry
fn init_without_otel(
    config: &TelemetryConfig,
    env_filter: EnvFilter,
) -> Result<(), TelemetryError> {
    if config.json_logs && config.stderr_output {
        let fmt_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .json();

        Registry::default()
            .with(env_filter)
            .with(fmt_layer)
            .try_init()
            .map_err(|e| TelemetryError::TracingError(e.to_string()))
    } else if config.json_logs {
        let fmt_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .json();

        Registry::default()
            .with(env_filter)
            .with(fmt_layer)
            .try_init()
            .map_err(|e| TelemetryError::TracingError(e.to_string()))
    } else if config.stderr_output {
        let fmt_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_thread_ids(false)
            .pretty();

        Registry::default()
            .with(env_filter)
            .with(fmt_layer)
            .try_init()
            .map_err(|e| TelemetryError::TracingError(e.to_string()))
    } else {
        let fmt_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(false)
            .pretty();

        Registry::default()
            .with(env_filter)
            .with(fmt_layer)
            .try_init()
            .map_err(|e| TelemetryError::TracingError(e.to_string()))
    }
}

/// Initialize the OpenTelemetry tracer provider
#[cfg(feature = "opentelemetry")]
fn init_tracer_provider(
    config: &TelemetryConfig,
) -> Result<opentelemetry_sdk::trace::SdkTracerProvider, TelemetryError> {
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::{
        Resource,
        trace::{RandomIdGenerator, Sampler, SdkTracerProvider},
    };

    let endpoint = config.otlp_endpoint.as_ref().ok_or_else(|| {
        TelemetryError::InvalidConfiguration("OTLP endpoint not configured".into())
    })?;

    // Build resource with service info
    let mut resource_attrs = vec![
        opentelemetry::KeyValue::new("service.name", config.service_name.clone()),
        opentelemetry::KeyValue::new("service.version", config.service_version.clone()),
    ];

    for (key, value) in &config.resource_attributes {
        resource_attrs.push(opentelemetry::KeyValue::new(key.clone(), value.clone()));
    }

    let resource = Resource::builder().with_attributes(resource_attrs).build();

    // Configure sampler
    let sampler = if (config.sampling_ratio - 1.0).abs() < f64::EPSILON {
        Sampler::AlwaysOn
    } else if config.sampling_ratio <= 0.0 {
        Sampler::AlwaysOff
    } else {
        Sampler::TraceIdRatioBased(config.sampling_ratio)
    };

    // Build the OTLP exporter
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .with_timeout(config.export_timeout)
        .build()
        .map_err(|e| TelemetryError::OpenTelemetryError(e.to_string()))?;

    // Build the tracer provider (0.31 API - no runtime argument needed)
    let provider = SdkTracerProvider::builder()
        .with_sampler(sampler)
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    Ok(provider)
}

/// Initialize Prometheus metrics exporter
#[cfg(feature = "prometheus")]
fn init_prometheus(config: &TelemetryConfig, port: u16) -> Result<MetricsHandle, TelemetryError> {
    use metrics_exporter_prometheus::PrometheusBuilder;
    use std::net::SocketAddr;

    let addr: SocketAddr = format!("0.0.0.0:{port}")
        .parse()
        .map_err(|e| TelemetryError::InvalidConfiguration(format!("Invalid port: {e}")))?;

    let handle = PrometheusBuilder::new()
        .with_http_listener(addr)
        .install_recorder()
        .map_err(|e| TelemetryError::MetricsError(e.to_string()))?;

    info!(
        port = port,
        path = %config.prometheus_path,
        "Prometheus metrics endpoint started"
    );

    Ok(MetricsHandle { _handle: handle })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_config_builder() {
        let config = TelemetryConfig::builder()
            .service_name("test-service")
            .service_version("1.0.0")
            .log_level("debug")
            .build();

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.service_version, "1.0.0");
        assert_eq!(config.log_level, "debug");
    }

    // Note: Full initialization tests require careful handling to avoid
    // conflicts with the global tracing subscriber. See integration tests.
}

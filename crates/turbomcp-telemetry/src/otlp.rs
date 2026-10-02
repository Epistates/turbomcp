//! OTLP export pipeline (feature `otlp`).
//!
//! A turnkey installer: build an OTLP span exporter (gRPC or HTTP/protobuf),
//! wrap it in an
//! [`SdkTracerProvider`], register it + the W3C propagator globally, and install
//! a `tracing` subscriber whose `tracing-opentelemetry` layer turns the spans
//! opened by [`TraceContextLayer`](crate::TraceContextLayer) into exported OTLP
//! traces. Call it once at startup, inside a Tokio runtime, and keep the
//! returned [`TelemetryGuard`] alive for the process lifetime (its `Drop`
//! flushes pending spans).

use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{MetricExporter, SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

use crate::TelemetryError;

/// How the pipeline talks to the collector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OtlpProtocol {
    /// OTLP over gRPC (default port 4317).
    Grpc,
    /// OTLP over HTTP with protobuf bodies (default port 4318): what the
    /// OpenTelemetry spec makes the default, and what most managed backends
    /// and proxies accept.
    HttpProtobuf,
}

impl OtlpProtocol {
    /// The protocol `OTEL_EXPORTER_OTLP_PROTOCOL` names (`grpc`,
    /// `http/protobuf`), if it is set to one this pipeline speaks.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        match std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL").ok()?.as_str() {
            "grpc" => Some(Self::Grpc),
            "http/protobuf" => Some(Self::HttpProtobuf),
            _ => None,
        }
    }
}

/// Configuration for the OTLP pipeline.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OtlpConfig {
    /// `service.name` resource attribute (how this server appears in traces).
    pub service_name: String,
    /// Collector endpoint. `None` leaves it to the exporter, which reads
    /// `OTEL_EXPORTER_OTLP_ENDPOINT` and otherwise uses the protocol's local
    /// default (`http://localhost:4317` for gRPC, `http://localhost:4318`
    /// for HTTP).
    pub endpoint: Option<String>,
    /// The protocol. `None` reads `OTEL_EXPORTER_OTLP_PROTOCOL`, and
    /// otherwise speaks gRPC.
    pub protocol: Option<OtlpProtocol>,
}

impl OtlpConfig {
    /// A config for `service_name` against the default local collector.
    #[must_use]
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            endpoint: None,
            protocol: None,
        }
    }

    /// Point at a specific collector endpoint. With
    /// [`OtlpProtocol::HttpProtobuf`] this is the base URL; the exporter
    /// appends `/v1/traces` and `/v1/metrics`.
    #[must_use]
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Speak `protocol` to the collector.
    #[must_use]
    pub fn protocol(mut self, protocol: OtlpProtocol) -> Self {
        self.protocol = Some(protocol);
        self
    }

    fn resolved_protocol(&self) -> OtlpProtocol {
        self.protocol
            .or_else(OtlpProtocol::from_env)
            .unwrap_or(OtlpProtocol::Grpc)
    }
}

/// The span and metric exporters for `config`'s protocol and endpoint.
fn exporters(config: &OtlpConfig) -> Result<(SpanExporter, MetricExporter), TelemetryError> {
    let failed =
        |e: opentelemetry_otlp::ExporterBuildError| TelemetryError::Exporter(e.to_string());
    Ok(match config.resolved_protocol() {
        OtlpProtocol::Grpc => {
            let mut spans = SpanExporter::builder().with_tonic();
            let mut metrics = MetricExporter::builder().with_tonic();
            if let Some(endpoint) = &config.endpoint {
                spans = spans.with_endpoint(endpoint.clone());
                metrics = metrics.with_endpoint(endpoint.clone());
            }
            (
                spans.build().map_err(failed)?,
                metrics.build().map_err(failed)?,
            )
        }
        OtlpProtocol::HttpProtobuf => {
            let mut spans = SpanExporter::builder()
                .with_http()
                .with_protocol(opentelemetry_otlp::Protocol::HttpBinary);
            let mut metrics = MetricExporter::builder()
                .with_http()
                .with_protocol(opentelemetry_otlp::Protocol::HttpBinary);
            // The HTTP exporters take each signal's full URL; the
            // conventional base gets the signal paths the spec defines.
            if let Some(endpoint) = &config.endpoint {
                let base = endpoint.trim_end_matches('/');
                spans = spans.with_endpoint(format!("{base}/v1/traces"));
                metrics = metrics.with_endpoint(format!("{base}/v1/metrics"));
            }
            (
                spans.build().map_err(failed)?,
                metrics.build().map_err(failed)?,
            )
        }
    })
}

/// Keeps the tracer + meter providers alive; flushes pending spans and metrics
/// on drop. Hold it for the process lifetime.
#[must_use = "dropping the guard shuts the exporters down and stops trace/metric export"]
pub struct TelemetryGuard {
    provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
}

impl core::fmt::Debug for TelemetryGuard {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TelemetryGuard").finish_non_exhaustive()
    }
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        // Best-effort flush; nothing actionable if the collector is already gone.
        let _ = self.provider.shutdown();
        let _ = self.meter_provider.shutdown();
    }
}

/// Install the OTLP export pipeline (traces **and** metrics) and a `tracing`
/// subscriber wired to it.
///
/// Registers a global tracer provider, a global meter provider (so
/// [`MetricsLayer`](crate::MetricsLayer)'s instruments export), the W3C
/// propagators (trace context and baggage), and a global subscriber (env-filter + fmt + the
/// OpenTelemetry layer). Returns the [`TelemetryGuard`] to hold for the process
/// lifetime.
///
/// # Errors
/// - [`TelemetryError::Exporter`] if either OTLP exporter can't be built.
/// - [`TelemetryError::Subscriber`] if a global subscriber is already installed.
pub fn init_otlp(config: OtlpConfig) -> Result<TelemetryGuard, TelemetryError> {
    let (exporter, metric_exporter) = exporters(&config)?;

    let resource = Resource::builder()
        .with_service_name(config.service_name)
        .build();
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource.clone())
        .build();

    // Metrics pipeline (periodic-reader OTLP export), globally installed so the
    // MetricsLayer's `global::meter("turbomcp")` instruments record and export.
    let meter_provider = SdkMeterProvider::builder()
        .with_periodic_exporter(metric_exporter)
        .with_resource(resource)
        .build();
    global::set_meter_provider(meter_provider.clone());

    let tracer = provider.tracer("turbomcp");
    global::set_tracer_provider(provider.clone());
    global::set_text_map_propagator(crate::propagation::w3c_propagator());

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        // stderr, never stdout: on stdio, stdout *is* the MCP channel ("The
        // server MUST NOT write anything to its stdout that is not a valid
        // MCP message"), and one log line there drops the client's
        // connection. The spec names stderr as the logging channel.
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .try_init()
        .map_err(|e| TelemetryError::Subscriber(e.to_string()))?;

    Ok(TelemetryGuard {
        provider,
        meter_provider,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both protocols assemble their exporters without a live collector
    /// (each connects lazily), and an explicit protocol wins.
    #[tokio::test]
    async fn both_protocols_build() {
        for (protocol, endpoint) in [
            (OtlpProtocol::Grpc, "http://127.0.0.1:4317"),
            (OtlpProtocol::HttpProtobuf, "http://127.0.0.1:4318/"),
        ] {
            let config = OtlpConfig::new("svc").protocol(protocol).endpoint(endpoint);
            assert_eq!(config.resolved_protocol(), protocol);
            exporters(&config).expect("exporters build");
        }
    }
}

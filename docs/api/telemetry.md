# Telemetry API Reference

The `turbomcp-telemetry` crate provides OpenTelemetry integration and observability for TurboMCP v3.

## Overview

Telemetry features include:

- **Distributed Tracing** - OpenTelemetry traces with MCP-specific span attributes
- **Metrics Collection** - Request counts, latencies, error rates
- **Structured Logging** - JSON-formatted logs correlated with traces
- **Tower Middleware** - Automatic instrumentation for MCP request handling
- **Prometheus Export** - Built-in metrics endpoint

## Installation

```toml
[dependencies]
turbomcp-telemetry = "3.0"
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `opentelemetry` | Full OpenTelemetry with OTLP export | Yes |
| `prometheus` | Standalone Prometheus metrics | No |
| `tower` | Tower middleware for instrumentation | Yes |
| `full` | All features | No |

## Quick Start

```rust
use turbomcp_telemetry::{TelemetryConfig, TelemetryGuard};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize telemetry
    let config = TelemetryConfig::builder()
        .service_name("my-mcp-server")
        .service_version("1.0.0")
        .log_level("info,turbomcp=debug")
        .build();

    let _guard = config.init()?;

    // Your MCP server code here...
    Ok(())
}
```

## TelemetryConfig

### Builder

```rust
use turbomcp_telemetry::TelemetryConfig;

let config = TelemetryConfig::builder()
    // Service identification
    .service_name("my-mcp-server")
    .service_version("1.0.0")
    .service_namespace("production")

    // Logging
    .log_level("info,turbomcp=debug")
    .log_format(LogFormat::Json)

    // OpenTelemetry
    .otlp_endpoint("http://localhost:4317")
    .sampling_ratio(0.1)  // Sample 10%

    // Prometheus
    .prometheus_port(9090)

    .build();
```

### Methods

```rust
impl TelemetryConfigBuilder {
    /// Set the service name for traces and metrics
    pub fn service_name(self, name: impl Into<String>) -> Self;

    /// Set the service version
    pub fn service_version(self, version: impl Into<String>) -> Self;

    /// Set the service namespace
    pub fn service_namespace(self, namespace: impl Into<String>) -> Self;

    /// Set the log level filter (e.g., "info,my_crate=debug")
    pub fn log_level(self, level: impl Into<String>) -> Self;

    /// Set the log output format
    pub fn log_format(self, format: LogFormat) -> Self;

    /// Set the OTLP collector endpoint
    pub fn otlp_endpoint(self, endpoint: impl Into<String>) -> Self;

    /// Set the trace sampling ratio (0.0 to 1.0)
    pub fn sampling_ratio(self, ratio: f64) -> Self;

    /// Set the Prometheus metrics port
    pub fn prometheus_port(self, port: u16) -> Self;

    /// Build the configuration
    pub fn build(self) -> TelemetryConfig;
}
```

### Initialization

```rust
impl TelemetryConfig {
    /// Initialize telemetry and return a guard
    /// The guard must be held for the lifetime of the application
    pub fn init(self) -> Result<TelemetryGuard, TelemetryError>;
}
```

## TelemetryGuard

The guard manages telemetry lifecycle. Drop it to flush and shutdown.

```rust
let _guard = config.init()?;

// Application runs here...

// When _guard drops, telemetry is flushed and shutdown
drop(_guard);
```

## Tower Middleware

### TelemetryLayer

Automatic instrumentation for MCP requests.

```rust
use turbomcp_telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};
use tower::ServiceBuilder;

let config = TelemetryLayerConfig::new()
    .service_name("my-mcp-server")
    .exclude_method("ping")
    .record_request_body(false)
    .record_response_body(false);

let service = ServiceBuilder::new()
    .layer(TelemetryLayer::new(config))
    .service(my_handler);
```

### TelemetryLayerConfig

```rust
impl TelemetryLayerConfig {
    pub fn new() -> Self;

    /// Set the service name for spans
    pub fn service_name(self, name: impl Into<String>) -> Self;

    /// Exclude methods from tracing (e.g., "ping")
    pub fn exclude_method(self, method: impl Into<String>) -> Self;

    /// Record request bodies in spans
    pub fn record_request_body(self, enabled: bool) -> Self;

    /// Record response bodies in spans
    pub fn record_response_body(self, enabled: bool) -> Self;

    /// Set custom sampling rate for this layer
    pub fn sample_rate(self, rate: f64) -> Self;
}
```

### MetricsLayer

Prometheus metrics collection.

```rust
use turbomcp_telemetry::tower::{MetricsLayer, MetricsConfig};

let config = MetricsConfig::new()
    .endpoint("/metrics")
    .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0]);

let layer = MetricsLayer::new(config);
```

## MCP Span Attributes

The telemetry layer records MCP-specific attributes on spans:

| Attribute | Description | Example |
|-----------|-------------|---------|
| `mcp.method` | MCP method name | `"tools/call"` |
| `mcp.tool.name` | Tool name (for tools/call) | `"calculator"` |
| `mcp.resource.uri` | Resource URI (for resources/read) | `"file:///data.json"` |
| `mcp.prompt.name` | Prompt name (for prompts/get) | `"greeting"` |
| `mcp.request.id` | JSON-RPC request ID | `"123"` |
| `mcp.session.id` | MCP session ID | `"abc-123"` |
| `mcp.transport` | Transport type | `"http"`, `"websocket"` |
| `mcp.duration_ms` | Request duration | `42` |
| `mcp.status` | Request status | `"success"`, `"error"` |

## Pre-defined Metrics

### Request Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `mcp_requests_total` | Counter | method, status |
| `mcp_request_duration_seconds` | Histogram | method |
| `mcp_active_requests` | Gauge | method |

### Tool Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `mcp_tool_calls_total` | Counter | name, status |
| `mcp_tool_duration_seconds` | Histogram | name |

### Resource Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `mcp_resource_reads_total` | Counter | uri_pattern |
| `mcp_resource_read_duration_seconds` | Histogram | uri_pattern |

### Connection Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `mcp_active_connections` | Gauge | transport |
| `mcp_connections_total` | Counter | transport |

### Error Metrics

| Metric | Type | Labels |
|--------|------|--------|
| `mcp_errors_total` | Counter | kind, method |

## Custom Metrics

Register and use custom metrics:

```rust
use turbomcp_telemetry::metrics::{counter, histogram, gauge};

// Counter
let requests = counter!("my_requests_total", "Total requests");
requests.increment(1);

// Histogram
let latency = histogram!("my_latency_seconds", "Request latency");
latency.record(0.042);

// Gauge
let queue_size = gauge!("my_queue_size", "Current queue size");
queue_size.set(42);
```

## Custom Spans

Create custom spans for detailed tracing:

```rust
use tracing::{info_span, Instrument};

async fn my_operation() {
    let span = info_span!(
        "my_operation",
        operation.type = "database_query",
        db.system = "postgresql"
    );

    async {
        // Operation code here
        query_database().await;
    }
    .instrument(span)
    .await;
}
```

## Logging Integration

Logs are automatically correlated with traces:

```rust
use tracing::{info, warn, error};

#[tool]
async fn my_handler() -> McpResult<String> {
    info!("Processing request");
    warn!(user_id = "123", "Rate limit approaching");
    error!(error.code = -32602, "Invalid params");
    Ok("Done".to_string())
}
```

Log output (JSON format):

```json
{
  "timestamp": "2026-01-10T10:30:45Z",
  "level": "INFO",
  "message": "Processing request",
  "target": "my_server",
  "span": {
    "name": "tools/call",
    "mcp.tool.name": "my_handler"
  },
  "trace_id": "abc123",
  "span_id": "def456"
}
```

## OpenTelemetry Configuration

### OTLP Export

```rust
let config = TelemetryConfig::builder()
    .otlp_endpoint("http://jaeger:4317")  // gRPC
    // or
    .otlp_endpoint("http://jaeger:4318/v1/traces")  // HTTP
    .build();
```

### With Jaeger

```yaml
# docker-compose.yml
services:
  jaeger:
    image: jaegertracing/all-in-one:latest
    ports:
      - "16686:16686"  # UI
      - "4317:4317"    # OTLP gRPC
```

### With Zipkin

```rust
use opentelemetry_zipkin::ZipkinExporter;

let exporter = ZipkinExporter::new()
    .endpoint("http://zipkin:9411/api/v2/spans")
    .build()?;
```

## Prometheus Integration

### Endpoint Configuration

```rust
let config = TelemetryConfig::builder()
    .prometheus_port(9090)
    .build();

// Metrics available at http://localhost:9090/metrics
```

### Prometheus Scrape Config

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'turbomcp'
    static_configs:
      - targets: ['localhost:9090']
    scrape_interval: 15s
```

### Grafana Dashboard

Pre-built dashboard JSON available at:
- `examples/grafana/mcp-dashboard.json`

Includes panels for:
- Request rate and latency
- Error rate by method
- Tool call performance
- Active connections
- Resource usage

## Error Handling

```rust
use turbomcp_telemetry::TelemetryError;

match config.init() {
    Ok(guard) => {
        // Telemetry initialized
    }
    Err(TelemetryError::OtlpConnection(msg)) => {
        eprintln!("Failed to connect to OTLP collector: {}", msg);
        // Continue without telemetry
    }
    Err(TelemetryError::ConfigError(msg)) => {
        eprintln!("Invalid configuration: {}", msg);
    }
    Err(e) => {
        eprintln!("Telemetry error: {:?}", e);
    }
}
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `OTEL_SERVICE_NAME` | Override service name |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP collector endpoint |
| `OTEL_TRACES_SAMPLER_ARG` | Sampling ratio |
| `RUST_LOG` | Log level filter |

## Next Steps

- **[Observability Guide](../guide/observability.md)** - Usage patterns
- **[Tower Middleware](../guide/tower-middleware.md)** - Middleware composition
- **[Deployment](../deployment/monitoring.md)** - Production monitoring

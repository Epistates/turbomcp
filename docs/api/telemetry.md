# Telemetry API Reference

The `turbomcp-telemetry` crate provides OpenTelemetry integration and observability for TurboMCP v3.

## Overview

Telemetry features include:

- **Distributed Tracing** - OpenTelemetry traces with MCP-specific span attributes
- **Metrics Collection** - Request counts, latencies, error rates
- **Structured Logging** - JSON-formatted logs correlated with traces
- **Tower Middleware** - Automatic instrumentation for MCP request handling
- **Prometheus Export** - A Prometheus metrics recorder

## Installation

```toml
[dependencies]
turbomcp-telemetry = { version = "3.5.0", features = ["full"] }
```

Through the `turbomcp` crate, the `telemetry` feature enables this crate with
`full`, as `turbomcp::telemetry`.

## Feature Flags

With no features, the crate installs a `tracing` subscriber (JSON or plain
text, to stderr or stdout).

| Feature | Description | Default |
|---------|-------------|---------|
| `opentelemetry` | OpenTelemetry tracing with OTLP export | No |
| `prometheus` | Prometheus metrics recorder and the `metrics` module | No |
| `tower` | Tower middleware for instrumentation (`tower` module) | No |
| `full` | All features | No |

## Quick Start

```rust
use turbomcp_telemetry::TelemetryConfig;

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

Logs go to stderr by default, so a STDIO server's protocol stream on stdout
stays clean.

## TelemetryConfig

### Builder

```rust
use std::time::Duration;
use turbomcp_telemetry::TelemetryConfig;

let config = TelemetryConfig::builder()
    // Service identification
    .service_name("my-mcp-server")
    .service_version("1.0.0")
    .environment("production")
    .resource_attribute("service.namespace", "tools")

    // Logging
    .log_level("info,turbomcp=debug")
    .json_logs(true)
    .stderr_output(true)

    // OpenTelemetry (feature `opentelemetry`)
    .otlp_endpoint("http://localhost:4318")
    .sampling_ratio(0.1)  // Sample 10%
    .export_timeout(Duration::from_secs(10))

    // Prometheus (feature `prometheus`)
    .prometheus_port(9090)

    .build();
```

### Methods

Signatures only; the OTLP and Prometheus setters exist only with their feature:

```rust,ignore
impl TelemetryConfigBuilder {
    /// Service name for traces and metrics (default "turbomcp-service")
    pub fn service_name(self, name: impl Into<String>) -> Self;

    /// Service version
    pub fn service_version(self, version: impl Into<String>) -> Self;

    /// Log level filter, e.g. "info,my_crate=debug" (default "info").
    /// RUST_LOG, when set, takes precedence.
    pub fn log_level(self, level: impl Into<String>) -> Self;

    /// JSON log lines (default true) or human-readable text
    pub fn json_logs(self, enabled: bool) -> Self;

    /// Log to stderr (default true) or stdout
    pub fn stderr_output(self, enabled: bool) -> Self;

    /// OTLP collector endpoint (feature `opentelemetry`)
    pub fn otlp_endpoint(self, endpoint: impl Into<String>) -> Self;

    /// OTLP protocol (feature `opentelemetry`, default Http). `OtlpProtocol`
    /// is not exported in 3.5.0, so this cannot be called yet.
    pub fn otlp_protocol(self, protocol: OtlpProtocol) -> Self;

    /// Trace sampling ratio, 0.0 to 1.0 (feature `opentelemetry`, default 1.0)
    pub fn sampling_ratio(self, ratio: f64) -> Self;

    /// Export timeout (feature `opentelemetry`)
    pub fn export_timeout(self, timeout: Duration) -> Self;

    /// Prometheus port (feature `prometheus`)
    pub fn prometheus_port(self, port: u16) -> Self;

    /// Prometheus endpoint path (feature `prometheus`, default "/metrics")
    pub fn prometheus_path(self, path: impl Into<String>) -> Self;

    /// Prometheus bind address (feature `prometheus`, default 127.0.0.1)
    pub fn prometheus_bind_addr(self, addr: IpAddr) -> Self;

    /// Add an OpenTelemetry resource attribute
    pub fn resource_attribute(self, key: impl Into<String>, value: impl Into<String>) -> Self;

    /// Shorthand for the `deployment.environment` resource attribute
    pub fn environment(self, env: impl Into<String>) -> Self;

    /// Build the configuration
    pub fn build(self) -> TelemetryConfig;
}
```

### Initialization

```rust,ignore
impl TelemetryConfig {
    /// Install the global subscriber (and exporters) and return a guard.
    /// The guard must be held for the lifetime of the application.
    pub fn init(self) -> Result<TelemetryGuard, TelemetryError>;
}
```

`init` installs a global subscriber, so call it once, early in `main`.

## TelemetryGuard

The guard manages telemetry lifecycle. Drop it to flush and shut down the exporters.

```rust
use turbomcp_telemetry::TelemetryConfig;

fn main() -> Result<(), turbomcp_telemetry::TelemetryError> {
    let guard = TelemetryConfig::builder().service_name("my-mcp-server").build().init()?;
    println!("telemetry for {}", guard.service_name());

    // Application runs here...

    // When the guard drops, telemetry is flushed and shut down
    drop(guard);
    Ok(())
}
```

## Tower Middleware

### TelemetryLayer

Automatic instrumentation for MCP requests (feature `tower`). `TelemetryService`
wraps either a `Service<serde_json::Value>` (JSON-RPC messages) or a
`Service<http::Request<B>>`, so the layer can go straight onto the Axum router
of an HTTP server:

```rust
use turbomcp::prelude::*;
use turbomcp_telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Say hello
    #[tool]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = TelemetryLayerConfig::new()
        .service_name("my-mcp-server")
        .exclude_method("ping")
        .record_sizes(false);

    // Needs the `http` feature on turbomcp
    let app = MyServer
        .builder()
        .into_axum_router()
        .layer(TelemetryLayer::new(config));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

On an HTTP service the span covers the HTTP request, and `mcp.method` is the
URL path (`mcp`), not the JSON-RPC method; wrapped around a
`Service<serde_json::Value>` it is the JSON-RPC method, with the tool name,
prompt name, and request ID recorded too.

### TelemetryLayerConfig

Signatures only:

```rust,ignore
impl TelemetryLayerConfig {
    pub fn new() -> Self;

    /// Service name for spans
    pub fn service_name(self, name: impl Into<String>) -> Self;

    /// Service version for spans
    pub fn service_version(self, version: impl Into<String>) -> Self;

    /// Record request/response sizes (default true)
    pub fn record_sizes(self, enabled: bool) -> Self;

    /// Record duration, status, and error on the span (default true)
    pub fn record_timing(self, enabled: bool) -> Self;

    /// Exclude a method from instrumentation (e.g. "ping")
    pub fn exclude_method(self, method: impl Into<String>) -> Self;

    /// Continue a caller's W3C trace from `params._meta` or HTTP headers
    /// (feature `opentelemetry`, default true)
    pub fn propagate_context(self, enabled: bool) -> Self;

    /// Truncate `mcp.error.message` (default 512 bytes; 0 drops it)
    pub fn error_message_max_len(self, max_len: usize) -> Self;

    /// Omit `mcp.request.id` (default false)
    pub fn redact_request_id(self, enabled: bool) -> Self;

    /// Omit `mcp.resource.uri` (default true: URIs can carry secrets)
    pub fn redact_resource_uri(self, enabled: bool) -> Self;
}
```

## MCP Span Attributes

The telemetry layer records MCP-specific attributes on spans. The names are
constants in `turbomcp_telemetry::span_attributes`:

| Attribute | Description | Example |
|-----------|-------------|---------|
| `mcp.method` | MCP method name | `"tools/call"` |
| `mcp.tool.name` | Tool name (for tools/call) | `"calculator"` |
| `mcp.resource.uri` | Resource URI (for resources/read; redacted by default) | `"file:///data.json"` |
| `mcp.prompt.name` | Prompt name (for prompts/get) | `"greeting"` |
| `mcp.request.id` | JSON-RPC request ID | `"123"` |
| `mcp.session.id` | MCP session ID | `"abc-123"` |
| `mcp.transport` | Transport type | `"http"`, `"websocket"` |
| `mcp.duration_ms` | Request duration | `42` |
| `mcp.status` | Request status | `"success"`, `"error"` |
| `mcp.error.code` | JSON-RPC error code | `-32602` |
| `mcp.error.message` | Error message (truncated) | `"Invalid params"` |

## Pre-defined Metrics

The `metrics` module (feature `prometheus`) defines these metrics and the
functions that record them. Nothing in the SDK calls them for you: record from
your handlers or middleware with `metrics::record_request`, `RequestTimer`, and
`McpMetrics`, and call `metrics::init_metrics()` once to register their
descriptions.

### Request Metrics

| Metric | Type | Labels | Recorded by |
|--------|------|--------|-------------|
| `mcp_requests_total` | Counter | method, status | `record_request`, `RequestTimer::complete` |
| `mcp_request_duration_seconds` | Histogram | method | `record_request`, `RequestTimer::complete` |
| `mcp_request_size_bytes` | Histogram | method | `record_request_size` |
| `mcp_response_size_bytes` | Histogram | method | `record_response_size` |

### Tool, Resource, and Prompt Metrics

| Metric | Type | Labels | Recorded by |
|--------|------|--------|-------------|
| `mcp_tool_calls_total` | Counter | tool, status | `McpMetrics::tool_call` |
| `mcp_tool_duration_seconds` | Histogram | tool | `McpMetrics::tool_call` |
| `mcp_resource_reads_total` | Counter | uri_pattern, status | `McpMetrics::resource_read` |
| `mcp_prompt_gets_total` | Counter | prompt, status | `McpMetrics::prompt_get` |

### Connection Metrics

| Metric | Type | Labels | Recorded by |
|--------|------|--------|-------------|
| `mcp_active_connections` | Gauge | transport | `McpMetrics::set_active_connections` |
| `mcp_connections_total` | Counter | transport | `McpMetrics::connection_established` |
| `mcp_connection_duration_seconds` | Histogram | transport | `McpMetrics::connection_closed` |

### Error Metrics

| Metric | Type | Labels | Recorded by |
|--------|------|--------|-------------|
| `mcp_errors_total` | Counter | kind, method | `McpMetrics::error` |
| `mcp_rate_limited_total` | Counter | tenant | `McpMetrics::rate_limited` |

```rust
use turbomcp_telemetry::metrics::{McpMetrics, RequestTimer, init_metrics};

init_metrics();

let timer = RequestTimer::start("tools/call");
// ... handle the request ...
timer.complete(true);

McpMetrics::tool_call("calculator", true, 0.015);
```

## Custom Metrics

Register and use custom metrics with the [`metrics`](https://docs.rs/metrics)
crate, which the Prometheus recorder installed by `init` collects:

```rust
use metrics::{counter, gauge, histogram};

// Counter
counter!("my_requests_total").increment(1);

// Histogram
histogram!("my_latency_seconds").record(0.042);

// Gauge
gauge!("my_queue_size").set(42.0);
```

## Custom Spans

Create custom spans for detailed tracing:

```rust
use tracing::{Instrument, info_span};

async fn query_database() {}

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
use tracing::{error, info, warn};
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Do the work
    #[tool]
    async fn my_handler(&self) -> McpResult<String> {
        info!("Processing request");
        warn!(user_id = "123", "Rate limit approaching");
        error!(error.code = -32602, "Invalid params");
        Ok("Done".to_string())
    }
}
```

These are server-side logs. To send a log message to the *client*
(`notifications/message`), use `turbomcp_protocol::RichContextExt` on the
request context (`ctx.info(...)`).

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

Only OTLP over HTTP/protobuf is built in (a gRPC selection would log a warning
and still export over HTTP), so point the endpoint at the collector's HTTP port
(4318):

```rust
use turbomcp_telemetry::TelemetryConfig;

let config = TelemetryConfig::builder()
    .otlp_endpoint("http://jaeger:4318")
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
      - "4318:4318"    # OTLP HTTP
```

### With Zipkin

There is no Zipkin exporter. Send OTLP to an OpenTelemetry Collector and export
to Zipkin from there.

## Prometheus Integration

### Endpoint Configuration

```rust
use turbomcp_telemetry::TelemetryConfig;

let config = TelemetryConfig::builder()
    .prometheus_port(9090)
    .build();
```

!!! warning "3.5.0: no HTTP listener"
    `prometheus_port` installs the Prometheus recorder but does not start an
    HTTP listener on the port, so nothing serves `/metrics`. Until that is
    fixed, render the metrics from your own endpoint, or install
    `metrics_exporter_prometheus::PrometheusBuilder` yourself instead of
    setting `prometheus_port`.

The listener, once serving, binds `127.0.0.1` unless `prometheus_bind_addr`
says otherwise.

### Prometheus Scrape Config

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'turbomcp'
    static_configs:
      - targets: ['localhost:9090']
    scrape_interval: 15s
```

## Error Handling

```rust
use turbomcp_telemetry::{TelemetryConfig, TelemetryError};

let config = TelemetryConfig::builder().build();

match config.init() {
    Ok(guard) => {
        // Telemetry initialized; keep `guard` alive
    }
    Err(TelemetryError::InvalidConfiguration(msg)) => {
        eprintln!("Invalid configuration: {}", msg);
    }
    Err(TelemetryError::InitializationFailed(msg)) => {
        eprintln!("Failed to initialize telemetry: {}", msg);
        // Continue without telemetry
    }
    Err(e) => {
        eprintln!("Telemetry error: {}", e);
    }
}
```

The other variants are `ExportFailed`, `TracingError`, `OpenTelemetryError`
(feature `opentelemetry`), and `MetricsError` (feature `prometheus`).

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level filter; overrides `log_level` |

The service name, endpoint, and sampling ratio come from the builder;
`TelemetryConfig` does not consult the `OTEL_*` variables.

## Next Steps

- **[Observability Guide](../guide/observability.md)** - Usage patterns
- **[Tower Middleware](../guide/tower-middleware.md)** - Middleware composition
- **[Deployment](../deployment/monitoring.md)** - Production monitoring

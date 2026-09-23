# turbomcp-telemetry

OpenTelemetry integration and observability for TurboMCP SDK.

## Features

- **Distributed Tracing**: OpenTelemetry traces with MCP-specific span attributes
- **Metrics Collection**: Request counts, latencies, error rates with Prometheus export
- **Structured Logging**: JSON-formatted logs correlated with traces
- **Tower Middleware**: Automatic instrumentation for MCP request handling

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

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `opentelemetry` | no | OpenTelemetry tracing with OTLP export over HTTP/protobuf, plus W3C trace-context propagation in the tower middleware |
| `prometheus` | no | Standalone Prometheus metrics via `metrics` + `metrics-exporter-prometheus` |
| `tower` | no | Tower middleware for automatic request instrumentation |
| `full` | no | Enables `opentelemetry`, `prometheus`, and `tower` |

Log format is a runtime setting, not a feature: `.json_logs(true)` (the
default) emits JSON, `.json_logs(false)` emits human-readable output.

## OpenTelemetry Integration

Enable the `opentelemetry` feature for full distributed tracing:

```rust
use turbomcp_telemetry::TelemetryConfig;

let config = TelemetryConfig::builder()
    .service_name("my-server")
    // OTLP over HTTP/protobuf; the URL is used as-is, so include /v1/traces
    .otlp_endpoint("http://localhost:4318/v1/traces")
    .sampling_ratio(1.0)
    .build();

let _guard = config.init()?;
```

## Prometheus Metrics

Enable the `prometheus` feature for standalone Prometheus metrics:

```rust
use turbomcp_telemetry::TelemetryConfig;

let config = TelemetryConfig::builder()
    .service_name("my-server")
    .prometheus_port(9090)
    .build();

let _guard = config.init()?;
// Metrics available at http://localhost:9090/metrics
```

## Tower Middleware

Enable the `tower` feature for automatic request instrumentation:

```rust
use tower::ServiceBuilder;
use turbomcp_telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};

let config = TelemetryLayerConfig::new()
    .service_name("my-mcp-server")
    .exclude_method("ping");

let service = ServiceBuilder::new()
    .layer(TelemetryLayer::new(config))
    .service(my_mcp_handler);
```

With the `opentelemetry` feature also enabled, the middleware continues the
caller's trace: W3C `traceparent`/`tracestate` from a JSON-RPC request's
`params._meta`, or from HTTP request headers, becomes the parent of the
`mcp.request` span. Turn it off with `.propagate_context(false)`.

## MCP Span Attributes

The telemetry system records MCP-specific attributes on spans:

| Attribute | Description |
|-----------|-------------|
| `mcp.method` | MCP method name (e.g., "tools/call") |
| `mcp.tool.name` | Tool name for tools/call requests |
| `mcp.resource.uri` | Resource URI for resources/read. Off by default in the tower middleware, since URIs can carry credentials or personal data; opt in with `.redact_resource_uri(false)` |
| `mcp.prompt.name` | Prompt name for prompts/get |
| `mcp.request.id` | JSON-RPC request ID |
| `mcp.session.id` | MCP session ID |
| `mcp.transport` | Transport type (stdio, http, websocket, tcp, unix) |
| `mcp.duration_ms` | Request duration in milliseconds |
| `mcp.status` | Request status (success/error) |
| `mcp.error.code` | JSON-RPC error code, when the response is an error |
| `mcp.error.message` | JSON-RPC error message, truncated to `error_message_max_len` (512 bytes by default) |

## Pre-defined Metrics

When using the `prometheus` feature:

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `mcp_requests_total` | Counter | method, status | Total MCP requests processed |
| `mcp_request_duration_seconds` | Histogram | method | Request latency distribution |
| `mcp_request_size_bytes` | Histogram | method | Request payload size |
| `mcp_response_size_bytes` | Histogram | method | Response payload size |
| `mcp_tool_calls_total` | Counter | tool, status | Tool calls |
| `mcp_tool_duration_seconds` | Histogram | tool | Tool execution latency |
| `mcp_resource_reads_total` | Counter | uri_pattern, status | Resource read operations |
| `mcp_prompt_gets_total` | Counter | prompt, status | Prompt get operations |
| `mcp_active_connections` | Gauge | transport | Current active connections |
| `mcp_connections_total` | Counter | transport | Connections established |
| `mcp_connection_duration_seconds` | Histogram | transport | Connection lifetime |
| `mcp_errors_total` | Counter | kind, method | Errors |
| `mcp_rate_limited_total` | Counter | tenant | Rate-limited requests |

## License

MIT

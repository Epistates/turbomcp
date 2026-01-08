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

| Feature | Description |
|---------|-------------|
| `opentelemetry` | Full OpenTelemetry integration with OTLP export |
| `prometheus` | Standalone Prometheus metrics (using metrics.rs) |
| `tower` | Tower middleware for automatic request instrumentation |
| `full` | All features enabled |

## OpenTelemetry Integration

Enable the `opentelemetry` feature for full distributed tracing:

```rust
use turbomcp_telemetry::TelemetryConfig;

let config = TelemetryConfig::builder()
    .service_name("my-server")
    .otlp_endpoint("http://localhost:4317")
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

## MCP Span Attributes

The telemetry system records MCP-specific attributes on spans:

| Attribute | Description |
|-----------|-------------|
| `mcp.method` | MCP method name (e.g., "tools/call") |
| `mcp.tool.name` | Tool name for tools/call requests |
| `mcp.resource.uri` | Resource URI for resources/read |
| `mcp.prompt.name` | Prompt name for prompts/get |
| `mcp.request.id` | JSON-RPC request ID |
| `mcp.session.id` | MCP session ID |
| `mcp.transport` | Transport type (stdio, http, websocket) |
| `mcp.duration_ms` | Request duration in milliseconds |
| `mcp.status` | Request status (success/error) |

## Pre-defined Metrics

When using the `prometheus` feature:

| Metric | Type | Description |
|--------|------|-------------|
| `mcp_requests_total` | Counter | Total requests by method and status |
| `mcp_request_duration_seconds` | Histogram | Request latency distribution |
| `mcp_tool_calls_total` | Counter | Tool calls by name and status |
| `mcp_tool_duration_seconds` | Histogram | Tool execution latency |
| `mcp_resource_reads_total` | Counter | Resource reads by URI pattern |
| `mcp_active_connections` | Gauge | Current active connections |
| `mcp_errors_total` | Counter | Errors by kind and method |

## License

MIT

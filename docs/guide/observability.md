# Observability, Logging & Monitoring

Implement logging, tracing, and monitoring for production MCP servers. TurboMCP v3 introduces OpenTelemetry integration via `turbomcp-telemetry`.

## Overview

TurboMCP's observability is built on the `tracing` ecosystem:

- **Structured Logging** - `tracing` events, as JSON or text, on stderr
- **Distributed Tracing** - OpenTelemetry export over OTLP/HTTP (v3)
- **Metrics** - Prometheus metrics with MCP-specific names (v3)
- **Tower Middleware** - `TelemetryLayer` spans with MCP attributes (v3)
- **Client Logging** - `notifications/message` log messages to the connected client

Enable it with the `telemetry` feature, which turns on every
`turbomcp-telemetry` feature (`opentelemetry`, `prometheus`, `tower`) and
re-exports the crate as `turbomcp::telemetry`:

```toml
[dependencies]
turbomcp = { version = "3.5.0", features = ["telemetry"] }
# Or use the crate directly, choosing features
turbomcp-telemetry = { version = "3.5.0", features = ["opentelemetry", "prometheus", "tower"] }
```

## Quick Start (v3)

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-mcp-server", version = "1.0.0")]
impl MyServer {
    /// Say hello.
    #[tool]
    async fn hello(&self, name: String) -> String {
        tracing::info!(%name, "saying hello");
        format!("Hello, {name}!")
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logs to stderr (required for STDIO), traces over OTLP/HTTP,
    // Prometheus metrics on 127.0.0.1:9090/metrics
    let _guard = TelemetryConfig::builder()
        .service_name("my-mcp-server")
        .service_version("1.0.0")
        .log_level("info,turbomcp=debug")
        .otlp_endpoint("http://localhost:4318/v1/traces")
        .prometheus_port(9090)
        .build()
        .init()?;

    MyServer.run_stdio().await?;
    Ok(())
}
```

Keep the guard alive for the life of the program: dropping it flushes and shuts
down the exporters. The OTLP exporter speaks HTTP/protobuf, so point it at the
collector's HTTP port (4318) and full path; it does not append `/v1/traces`.

## Structured Logging

### Server-Side Logging

Log with `tracing`. `TelemetryConfig` installs the subscriber; without it,
install one yourself, writing to stderr for a STDIO server:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Worker;

#[server]
impl Worker {
    /// Do some work.
    #[tool]
    async fn my_tool(&self, key: String) -> McpResult<String> {
        tracing::info!("Tool starting");
        tracing::warn!(%key, "Cache miss for key");
        tracing::debug!(key_len = key.len(), "Detailed debugging info");
        Ok("Done".to_string())
    }
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();
}
```

### Logging to the Client

MCP clients can receive log messages (`notifications/message`) and choose a
minimum level with `logging/setLevel`. Every TurboMCP server advertises the
`logging` capability. Send them through `RichContextExt` from
`turbomcp-protocol`:

```rust
use turbomcp::prelude::*;
use turbomcp_protocol::RichContextExt;

#[derive(Clone)]
struct Chatty;

#[server]
impl Chatty {
    /// Report progress through log messages.
    #[tool]
    async fn import(&self, ctx: &RequestContext) -> McpResult<String> {
        ctx.debug("Detailed debugging info").await?;
        ctx.info("General information").await?;
        ctx.warning("Warning condition").await?;
        ctx.error("Error occurred").await?;
        Ok("Done".to_string())
    }
}
```

Messages below the client's level are dropped, and a session's messages are
rate limited, so a chatty handler cannot flood the client.

### Configuration

```rust
use turbomcp::prelude::*;

fn telemetry() -> Result<TelemetryGuard, Box<dyn std::error::Error>> {
    let guard = TelemetryConfig::builder()
        .service_name("my-server")
        .log_level("debug")   // an EnvFilter directive; RUST_LOG overrides it
        .json_logs(true)      // JSON instead of human-readable text
        .stderr_output(true)  // the default; keep it for STDIO servers
        .environment("production")
        .build()
        .init()?;
    Ok(guard)
}
```

## Request Correlation

Every request has an ID. Put it on a span so every event inside carries it:

```rust
use tracing::Instrument;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Correlated;

#[server]
impl Correlated {
    /// Process a request.
    #[tool]
    async fn handler(&self, ctx: &RequestContext) -> McpResult<String> {
        let span = tracing::info_span!(
            "handler",
            request_id = %ctx.request_id(),
            session_id = ctx.session_id().unwrap_or("-"),
        );
        async {
            tracing::info!("Processing request");
            Ok("Done".to_string())
        }
        .instrument(span)
        .await
    }
}
```

**Log output** (with `json_logs(true)`):
```json
{
  "timestamp": "2025-12-10T10:30:45Z",
  "level": "INFO",
  "fields": { "message": "Processing request" },
  "span": { "name": "handler", "request_id": "7", "session_id": "550e8400-e29b-41d4-a716-446655440000" }
}
```

## Distributed Tracing

### OpenTelemetry Integration (v3)

```rust
use turbomcp::prelude::*;

fn tracing_only() -> Result<TelemetryGuard, Box<dyn std::error::Error>> {
    let guard = TelemetryConfig::builder()
        .service_name("my-server")
        .otlp_endpoint("http://jaeger:4318/v1/traces")
        .sampling_ratio(1.0) // Sample all requests
        .build()
        .init()?;
    Ok(guard)
}
```

### Tower Middleware (v3)

`TelemetryLayer` creates a span per request with MCP attributes. It wraps a
`tower::Service<serde_json::Value>` that answers JSON-RPC requests, such as one
built on `McpHandlerExt::handle_request`:

```rust
use std::convert::Infallible;
use tower::{ServiceBuilder, ServiceExt};
use turbomcp::prelude::*;
use turbomcp::telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Say hello.
    #[tool]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

async fn handle(request: serde_json::Value) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let config = TelemetryLayerConfig::new()
        .service_name("my-mcp-server")
        .exclude_method("ping"); // Don't trace pings

    let service = ServiceBuilder::new()
        .layer(TelemetryLayer::new(config))
        .service(tower::service_fn(|request: serde_json::Value| async move {
            let response = MyServer
                .handle_request(request, RequestContext::new())
                .await
                .unwrap_or_else(|error| serde_json::json!({ "error": error.to_string() }));
            Ok::<_, Infallible>(response)
        }));

    Ok(service.oneshot(request).await?)
}
```

The layer also implements `Service<http::Request<B>>`, so it can wrap the HTTP
transport's Axum router (`MyServer.builder().into_axum_router().layer(...)`).
There it sees HTTP requests, not JSON-RPC methods: the span's method is the
request path.

### MCP Span Attributes (v3)

The JSON-RPC layer records MCP-specific attributes (names in
`turbomcp_telemetry::span_attributes`):

| Attribute | Description |
|-----------|-------------|
| `mcp.method` | MCP method (e.g., "tools/call") |
| `mcp.tool.name` | Tool name for tools/call |
| `mcp.resource.uri` | Resource URI for resources/read |
| `mcp.prompt.name` | Prompt name for prompts/get |
| `mcp.request.id` | JSON-RPC request ID |
| `mcp.session.id` | MCP session ID |
| `mcp.transport` | Transport type |
| `mcp.duration_ms` | Request duration |
| `mcp.status` | success/error |

`TelemetryLayerConfig::redact_request_id` and `redact_resource_uri` keep those
values out of exported spans.

### Span Creation

Add spans for sub-operations inside a handler with `tracing`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Pipeline;

#[server]
impl Pipeline {
    /// Fetch and store data.
    #[tool]
    async fn complex_operation(&self) -> McpResult<String> {
        {
            let _span = tracing::info_span!("fetch_data").entered();
            // ... synchronous work ...
        }
        Ok("Done".to_string())
    }
}
```

Across an `.await`, attach the span with `.instrument(span)` instead of holding
an entered guard.

## Metrics

### Available Metrics

With the `prometheus` feature, `turbomcp_telemetry::metrics` defines MCP
metrics (`mcp_requests_total`, `mcp_request_duration_seconds`,
`mcp_tool_calls_total`, `mcp_errors_total`, connection gauges, and more).
Nothing records them automatically: call the recorders where the events happen.
A server middleware sees every tool call:

```rust
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;
use turbomcp::prelude::*;
use turbomcp::telemetry::metrics::McpMetrics;
use turbomcp_server::{McpMiddleware, MiddlewareStack, Next};

struct ToolMetrics;

impl McpMiddleware for ToolMetrics {
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let started = Instant::now();
            let result = next.call_tool(name, args, ctx).await;
            let success = matches!(&result, Ok(r) if !r.is_error());
            McpMetrics::tool_call(name, success, started.elapsed().as_secs_f64());
            result
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = TelemetryConfig::builder()
        .service_name("my-server")
        .prometheus_port(9090)
        .build()
        .init()?;
    McpMetrics::init();

    MiddlewareStack::new(MyServer)
        .with_middleware(ToolMetrics)
        .run_stdio()
        .await?;
    Ok(())
}
```

### Custom Metrics

Record your own with the `metrics` crate's macros; the Prometheus exporter
picks them up:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Queue;

#[server]
impl Queue {
    /// Enqueue a job.
    #[tool]
    async fn enqueue(&self, job: String) -> McpResult<String> {
        metrics::counter!("jobs_enqueued_total").increment(1);
        metrics::histogram!("job_name_length").record(job.len() as f64);
        metrics::gauge!("queue_size").set(42.0);
        Ok("queued".to_string())
    }
}
```

This needs the `metrics` crate (0.24) as a direct dependency.

### Exporting Metrics

`prometheus_port(port)` starts a scrape endpoint at
`http://127.0.0.1:{port}/metrics`. It binds to loopback by default; use
`prometheus_bind_addr` to expose it beyond the host, and `prometheus_path` to
change the path.

```yaml
scrape_configs:
  - job_name: 'turbomcp'
    static_configs:
      - targets: ['localhost:9090']
    metrics_path: '/metrics'
```

## Health Checks

### Liveness & Readiness

TurboMCP does not add health endpoints. For an HTTP server, merge them into the
MCP router:

```rust
use axum::{Router, http::StatusCode, routing::get};
use turbomcp::prelude::*;

async fn ready() -> StatusCode {
    // Check dependencies (database, cache) here
    StatusCode::OK
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = Router::new()
        .route("/health/live", get(|| async { "OK" }))
        .route("/health/ready", get(ready))
        .merge(MyServer.builder().into_axum_router());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

**Checking health:**

```bash
# Liveness (is server running?)
curl http://localhost:8080/health/live

# Readiness (is server ready for traffic?)
curl http://localhost:8080/health/ready
```

MCP's own `ping` method is also always available for a client to check a
connection.

## Error Tracking & Reporting

### Error Categorization

Every `McpError` has an `ErrorKind`. A tool error carries it to the client in
`_meta` (`io.turbomcp/errorKind`); record it server-side yourself:

```rust
use turbomcp::prelude::*;

async fn some_operation() -> Result<String, std::io::Error> {
    Err(std::io::Error::other("connection reset"))
}

#[derive(Clone)]
struct Tracked;

#[server]
impl Tracked {
    /// Run the operation.
    #[tool]
    async fn handler(&self) -> McpResult<String> {
        match some_operation().await {
            Ok(result) => Ok(result),
            Err(e) => {
                let error = McpError::from(e);
                tracing::error!(kind = ?error.kind, error = %error, "operation failed");
                Err(error)
            }
        }
    }
}
```

### Error Reporting Services

Error trackers such as Sentry integrate through `tracing` (for example the
`sentry-tracing` layer), so `tracing::error!` events are reported without
TurboMCP-specific configuration.

## Logging Best Practices

### 1. Use Structured Logging

```rust
let user_id = "123";

// ✅ Good
tracing::info!(user_id, action = "delete_resource", "Resource deleted");

// ❌ Avoid
tracing::info!("User {} deleted resource", user_id);
```

### 2. Include Context IDs

Wrap handler work in a span carrying `ctx.request_id()` (see
[Request Correlation](#request-correlation)).

### 3. Don't Log Sensitive Data

```rust
let token = "secret";

// ❌ Never log passwords, tokens, or API keys
tracing::info!("Token: {}", token);

// ✅ Log safely
tracing::info!("User authenticated");
```

### 4. Use Appropriate Log Levels

```rust
tracing::debug!("Cache hit for key");           // Debug details
tracing::info!("Request received");             // Normal flow
tracing::warn!("Slow query detected: 500ms");   // Warnings
tracing::error!("Database connection failed");  // Errors
```

## Troubleshooting

### "Logs not appearing"

- Check the filter: `log_level("debug")`, or `RUST_LOG=debug`, which overrides it.
- A STDIO server's logs are on stderr; the client that launched it decides where
  stderr goes.
- Only one global subscriber can be installed: don't call both
  `TelemetryConfig::init` and `tracing_subscriber::fmt().init()`.

### Client never sees log messages

The client has to set a level with `logging/setLevel` at or below the message's
level, and the transport must be able to send notifications.

## Performance Impact

Disabled `tracing` events cost a filter check. Exporting traces costs in
proportion to the sampling ratio; lower `sampling_ratio` for high-traffic
servers.

## Next Steps

- **[Advanced Patterns](advanced-patterns.md)** - Complex observability setups
- **[Deployment](../deployment/production.md)** - Production monitoring setup
- **[Examples](../examples/basic.md)** - Real-world observability examples

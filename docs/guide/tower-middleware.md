# Tower Middleware

TurboMCP v3 has two kinds of middleware: typed MCP middleware for servers, and
Tower layers for the HTTP, gRPC, and client stacks.

## Overview

- **MCP middleware** (`turbomcp-server`) - implement `McpMiddleware` and wrap a
  handler in `MiddlewareStack`. Hooks see MCP operations (`tools/call` with the
  tool name and arguments, `resources/read` with the URI, list results) and run on
  every transport. This is the way to add logging, metrics, or access control to a
  server.
- **Tower layers** - [Tower](https://docs.rs/tower) `Layer`/`Service`
  implementations in:
  - **Authentication** (`turbomcp-auth`): `AuthLayer`, `RateLimitLayer` for HTTP services
  - **Telemetry** (`turbomcp-telemetry`): `TelemetryLayer`
  - **gRPC** (`turbomcp-grpc`): `McpGrpcLayer`
  - **Client Middleware** (`turbomcp-client`): `CacheLayer`, `MetricsLayer`, `TracingLayer`

  The server's HTTP transport is an Axum router, so any Tower layer whose error
  type is `Infallible` (for example `tower-http`'s) also applies to
  `into_axum_router()`.

## MCP Middleware

```rust
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;
use turbomcp::prelude::*;
use turbomcp_server::{McpMiddleware, MiddlewareStack, Next};

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Delete everything.
    #[tool(destructive = true)]
    async fn purge(&self) -> String {
        "purged".to_string()
    }

    /// Say hello.
    #[tool(read_only = true)]
    async fn hello(&self) -> String {
        "hello".to_string()
    }
}

/// Log every tool call with its duration.
struct Timing;

impl McpMiddleware for Timing {
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
            tracing::info!(tool = name, elapsed = ?started.elapsed(), "tool call");
            result
        })
    }
}

/// Only administrators may call `purge`.
struct AdminOnly;

impl McpMiddleware for AdminOnly {
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            if name == "purge" && !ctx.has_any_role(&["admin"]) {
                return Err(McpError::permission_denied("purge requires the admin role"));
            }
            next.call_tool(name, args, ctx).await
        })
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // Middleware runs in the order added; the stack is itself an McpHandler.
    MiddlewareStack::new(MyServer)
        .with_middleware(Timing)
        .with_middleware(AdminOnly)
        .run_stdio()
        .await
}
```

The other hooks (`on_list_tools`, `on_list_resources`, `on_list_resource_templates`,
`on_list_prompts`, `on_read_resource`, `on_get_prompt`) default to passing the call
through; override only the ones you need. To hide tools rather than refuse them,
see `VisibilityLayer` in the [Server API](../api/server.md).

## Authentication Middleware

For an MCP server, use the HTTP transport's built-in authorization
(`HttpAuthorization`; see [Authentication](authentication.md)): it serves the
metadata and challenges MCP clients expect, and puts the principal on the
request context.

`turbomcp-auth`'s Tower layers (feature `middleware`) are for other HTTP
services. `AuthLayer` validates the `Authorization` header (or an API key header)
with an auth provider and stores the resulting `AuthContext` in the request's
extensions; `RateLimitLayer` limits requests per client IP.

### RateLimitLayer

```rust
use axum::body::Body;
use axum::http::{Request, Response};
use tower::ServiceBuilder;
use turbomcp_auth::rate_limit::RateLimiter;
use turbomcp_auth::tower::RateLimitLayer;

let inner = tower::service_fn(|_request: Request<Body>| async move {
    Ok::<_, std::convert::Infallible>(Response::new(Body::from("ok")))
});

// Limits requests per client IP, with the auth-endpoint defaults
let service = ServiceBuilder::new()
    .layer(RateLimitLayer::new(RateLimiter::for_auth()))
    .service(inner);
```

### AuthLayer

`AuthLayer::new(provider)` takes an `AuthProvider`. Its `Layer` implementation
requires the provider to be `Clone`, which neither built-in provider
(`ApiKeyProvider`, `OAuth2Provider`) is, so in 3.5 it only works with a provider
of your own that derives `Clone`. Its error type is `McpError`, so wrapping an
Axum router with it also needs `axum::error_handling::HandleErrorLayer`.

## Telemetry Middleware

The `turbomcp-telemetry` crate provides OpenTelemetry integration via Tower middleware.

### TelemetryLayer

`TelemetryLayer` wraps a `Service<serde_json::Value>` that answers JSON-RPC
requests (for example one built on `McpHandlerExt::handle_request`), or an HTTP
service:

```rust
use tower::ServiceBuilder;
use turbomcp_telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};

let config = TelemetryLayerConfig::new()
    .service_name("my-mcp-server")
    .exclude_method("ping"); // Don't trace ping requests

let layer = TelemetryLayer::new(config);

let service = ServiceBuilder::new()
    .layer(layer)
    .service(tower::service_fn(|request: serde_json::Value| async move {
        // Dispatch the JSON-RPC request here
        Ok::<_, std::convert::Infallible>(request)
    }));
```

Sampling is set on the exporter, not the layer: `TelemetryConfig::sampling_ratio`.
See [Observability](observability.md#tower-middleware-v3) for a complete
example.

### MCP Span Attributes

The telemetry layer adds MCP-specific attributes to spans for JSON-RPC requests:

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

### Prometheus Metrics

There is no metrics layer. With the `prometheus` feature,
`turbomcp_telemetry::metrics` defines the metrics below and functions to record
them (`record_request`, `McpMetrics::tool_call`, …); call them from an
`McpMiddleware` as shown in [Observability](observability.md#available-metrics).
`TelemetryConfig::prometheus_port` serves them.

| Metric | Type | Description |
|--------|------|-------------|
| `mcp_requests_total` | Counter | Total requests by method and status |
| `mcp_request_duration_seconds` | Histogram | Request latency distribution |
| `mcp_tool_calls_total` | Counter | Tool calls by name and status |
| `mcp_tool_duration_seconds` | Histogram | Tool execution latency |
| `mcp_active_connections` | Gauge | Current active connections |
| `mcp_errors_total` | Counter | Errors by kind and method |

## gRPC Middleware

The `turbomcp-grpc` crate provides a Tower layer for the gRPC transport.

### McpGrpcLayer

`McpGrpcLayer` logs and times each gRPC call inside a `grpc_request` span:

```rust
use turbomcp_grpc::{McpGrpcLayer, McpGrpcServer};

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let server = McpGrpcServer::builder()
        .server_info("my-server", "1.0.0")
        .build();

    tonic::transport::Server::builder()
        .layer(McpGrpcLayer::new().logging(true).timing(true))
        .add_service(server.into_service())
        .serve("[::1]:50051".parse()?)
        .await?;
    Ok(())
}
```

## Client Middleware

The `turbomcp-client` crate provides Tower layers over
`Service<McpRequest, Response = McpResponse>`. The `Client` does not send its
own requests through them; compose them around a service you provide (see the
[turbomcp-client README](https://github.com/Epistates/turbomcp/tree/main/crates/turbomcp-client#tower-middleware)).

### Caching Layer

```rust
use std::time::Duration;
use turbomcp_client::middleware::{CacheConfig, CacheLayer};

let layer = CacheLayer::new(CacheConfig {
    max_entries: 1000,
    ttl: Duration::from_secs(300),
    ..Default::default()
});
```

### Retry and Timeouts

Client retries come from `ClientBuilder::build_resilient` (retry, circuit
breaker, health checks); request timeouts from `ClientBuilder::with_timeout` or
`client.with_timeout(duration)` for a single call. See the
[Client API](../api/client.md).

## Composing Middleware

Tower middleware composes with `ServiceBuilder`. On the server's HTTP router,
use layers that keep the router's `Infallible` error type:

```rust
use axum::Router;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use turbomcp::prelude::*;

fn app() -> Router {
    MyServer.builder().into_axum_router().layer(
        ServiceBuilder::new()
            // Outer layers process first on request, last on response
            .layer(TraceLayer::new_for_http())
            .layer(tower::limit::ConcurrencyLimitLayer::new(512)),
    )
}
```

The HTTP transport already applies origin validation, CORS (when enabled in
`ServerConfig`), rate limits, and authorization; add Tower layers for what it does
not do.

### Execution Order

```
Request:  Trace → ConcurrencyLimit → MCP transport → MiddlewareStack → Handler
Response: Handler → MiddlewareStack → MCP transport → ConcurrencyLimit → Trace
```

## Custom Middleware

Implement custom middleware using Tower's `Layer` and `Service` traits:

```rust
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};

#[derive(Clone)]
pub struct MyConfig {
    pub label: &'static str,
}

// Layer (factory for services)
#[derive(Clone)]
pub struct MyLayer {
    config: MyConfig,
}

impl<S> Layer<S> for MyLayer {
    type Service = MyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MyService {
            inner,
            config: self.config.clone(),
        }
    }
}

// Service (the actual middleware)
#[derive(Clone)]
pub struct MyService<S> {
    inner: S,
    config: MyConfig,
}

impl<S, Request> Service<Request> for MyService<S>
where
    S: Service<Request> + Clone + Send + 'static,
    S::Future: Send,
    Request: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        // Use the service that was polled ready, leave a fresh clone behind
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let label = self.config.label;

        Box::pin(async move {
            // Pre-processing
            tracing::debug!(label, "before request");

            // Call inner service
            let response = inner.call(request).await?;

            // Post-processing
            tracing::debug!(label, "after response");

            Ok(response)
        })
    }
}
```

## MCP-Specific Patterns

### Request Logging

Logging MCP requests is a job for `McpMiddleware`, which sees the operation
already parsed:

```rust
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use turbomcp::prelude::*;
use turbomcp_server::{McpMiddleware, Next};

pub struct RequestLogging;

impl McpMiddleware for RequestLogging {
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(tool = name, id = %ctx.request_id(), "MCP tool call received");
            next.call_tool(name, args, ctx).await
        })
    }

    fn on_read_resource<'a>(
        &'a self,
        uri: &'a str,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ResourceResult>> + Send + 'a>> {
        Box::pin(async move {
            tracing::info!(uri, id = %ctx.request_id(), "MCP resource read received");
            next.read_resource(uri, ctx).await
        })
    }
}
```

### Rate Limiting by Tool

The server's `with_rate_limit` limits requests per client. For a per-tool limit,
count in middleware:

```rust
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use turbomcp::prelude::*;
use turbomcp_server::{McpMiddleware, Next};

/// Allows `limit` calls of `tool` per `window`, across all clients.
pub struct ToolRateLimit {
    tool: &'static str,
    limit: usize,
    window: Duration,
    calls: Mutex<HashMap<&'static str, Vec<Instant>>>,
}

impl McpMiddleware for ToolRateLimit {
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            if name == self.tool {
                let mut calls = self.calls.lock().unwrap();
                let recent = calls.entry(self.tool).or_default();
                recent.retain(|at| at.elapsed() < self.window);
                if recent.len() >= self.limit {
                    return Err(McpError::rate_limited(format!("{name} is rate limited")));
                }
                recent.push(Instant::now());
            }
            next.call_tool(name, args, ctx).await
        })
    }
}
```

## Integration with MCP Server

Both kinds together: MCP middleware wraps the handler, Tower layers wrap the
HTTP router built from it:

```rust
use tower_http::trace::TraceLayer;
use turbomcp::prelude::*;
use turbomcp_server::MiddlewareStack;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let handler = MiddlewareStack::new(MyServer).with_middleware(Timing);

    let app = handler
        .builder()
        .into_axum_router()
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

(`Timing` is the middleware from [MCP Middleware](#mcp-middleware).)

## Migration from v2 Plugin System

TurboMCP v3 replaces the v2 plugin system with Tower middleware on the client
and `McpMiddleware` on the server.

### Before (v2 Plugin System)

```rust,ignore
// v2: Custom plugin system
use turbomcp_client::plugins::{Plugin, PluginContext};

struct MyPlugin;

impl Plugin for MyPlugin {
    fn on_request(&self, ctx: &mut PluginContext) {
        // ...
    }
}

client.register_plugin(MyPlugin);
```

This is the removed v2 API, shown for comparison.

### After (v3)

```rust
use tower::ServiceBuilder;
use turbomcp_client::middleware::{McpRequest, McpResponse, TracingLayer};

let service = ServiceBuilder::new()
    .layer(TracingLayer::new())
    .service(tower::service_fn(|request: McpRequest| async move {
        Ok::<_, turbomcp_client::Error>(McpResponse::success(serde_json::json!({}), request.elapsed()))
    }));
```

## Best Practices

### 1. Order Matters

`MiddlewareStack` runs middleware in the order added, and `ServiceBuilder` runs
its first layer outermost. Put tracing and metrics first to capture the whole
request, and authorization checks last, closest to the handler.

### 2. Use Timeouts

Bound slow work inside the handler, where you can return a meaningful error:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

async fn slow() -> String {
    "done".to_string()
}

#[derive(Clone)]
struct Bounded;

#[server]
impl Bounded {
    /// Give up after 30 seconds.
    #[tool]
    async fn bounded(&self) -> McpResult<String> {
        tokio::time::timeout(Duration::from_secs(30), slow())
            .await
            .map_err(|_| McpError::timeout("Took longer than 30 seconds"))
    }
}
```

### 3. Clone-Friendly

Tower services must be `Clone`. Use `Arc` for shared state:

```rust
use std::sync::Arc;

pub struct SharedState;

#[derive(Clone)]
pub struct MyService<S> {
    inner: S,
    shared_state: Arc<SharedState>,
}
```

### 4. Graceful Errors

An `McpMiddleware` that returns `Err` from `on_call_tool` produces a tool
execution error, exactly as a failing tool would; return the right
`McpError` kind (`permission_denied`, `rate_limited`, …) so the client can tell
why.

## Next Steps

- **[Observability](observability.md)** - OpenTelemetry deep dive
- **[Authentication](authentication.md)** - Auth patterns
- **[gRPC Transport](../api/grpc.md)** - gRPC API reference
- **[Telemetry API](../api/telemetry.md)** - Telemetry API reference

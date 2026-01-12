# Tower Middleware

TurboMCP v3 introduces native Tower integration for composable middleware patterns.

## Overview

[Tower](https://docs.rs/tower) is the de-facto standard for building modular and reusable network services in Rust. TurboMCP v3 provides Tower-native `Layer` and `Service` implementations for:

- **Authentication** (`turbomcp-auth`)
- **Telemetry** (`turbomcp-telemetry`)
- **gRPC** (`turbomcp-grpc`)
- **Client Middleware** (`turbomcp-client`)

## Basic Usage

```rust
use tower::ServiceBuilder;
use turbomcp_auth::tower::AuthLayer;
use turbomcp_telemetry::tower::TelemetryLayer;

let service = ServiceBuilder::new()
    .layer(TelemetryLayer::new())
    .layer(AuthLayer::new(auth_config))
    .service(mcp_handler);
```

## Authentication Middleware

The `turbomcp-auth` crate provides Tower middleware for OAuth 2.1, JWT, and API key authentication.

### AuthLayer

```rust
use turbomcp_auth::tower::{AuthLayer, AuthConfig};

let auth_config = AuthConfig::builder()
    .jwt_secret("your-secret-key")
    .issuer("https://auth.example.com")
    .audience("my-mcp-server")
    .build();

let layer = AuthLayer::new(auth_config);

let service = ServiceBuilder::new()
    .layer(layer)
    .service(my_handler);
```

### OAuth 2.1 Integration

```rust
use turbomcp_auth::tower::{OAuthLayer, OAuthConfig};
use turbomcp_auth::providers::GoogleOAuthProvider;

let oauth_config = OAuthConfig::builder()
    .provider(GoogleOAuthProvider::new(
        "client-id",
        "client-secret",
    ))
    .redirect_uri("https://example.com/callback")
    .scopes(vec!["openid", "profile"])
    .build();

let layer = OAuthLayer::new(oauth_config);
```

### API Key Authentication

```rust
use turbomcp_auth::tower::{ApiKeyLayer, ApiKeyConfig};

let config = ApiKeyConfig::builder()
    .header_name("X-API-Key")
    .validator(|key: &str| {
        // Validate against your database
        validate_api_key(key)
    })
    .build();

let layer = ApiKeyLayer::new(config);
```

### Extracting Auth Context

```rust
use turbomcp_auth::AuthContext;

#[tool]
async fn protected_handler(auth: AuthContext) -> McpResult<String> {
    let user_id = auth.user_id();
    let claims = auth.claims();

    Ok(format!("Hello, user {}!", user_id))
}
```

## Telemetry Middleware

The `turbomcp-telemetry` crate provides OpenTelemetry integration via Tower middleware.

### TelemetryLayer

```rust
use turbomcp_telemetry::tower::{TelemetryLayer, TelemetryLayerConfig};

let config = TelemetryLayerConfig::new()
    .service_name("my-mcp-server")
    .exclude_method("ping")  // Don't trace ping requests
    .sample_rate(0.1);       // Sample 10% of requests

let layer = TelemetryLayer::new(config);

let service = ServiceBuilder::new()
    .layer(layer)
    .service(my_handler);
```

### MCP Span Attributes

The telemetry layer automatically adds MCP-specific attributes to spans:

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

```rust
use turbomcp_telemetry::tower::{MetricsLayer, MetricsConfig};

let config = MetricsConfig::new()
    .endpoint("/metrics")
    .buckets(vec![0.001, 0.01, 0.1, 1.0]);

let layer = MetricsLayer::new(config);
```

Pre-defined metrics:

| Metric | Type | Description |
|--------|------|-------------|
| `mcp_requests_total` | Counter | Total requests by method and status |
| `mcp_request_duration_seconds` | Histogram | Request latency distribution |
| `mcp_tool_calls_total` | Counter | Tool calls by name and status |
| `mcp_tool_duration_seconds` | Histogram | Tool execution latency |
| `mcp_active_connections` | Gauge | Current active connections |
| `mcp_errors_total` | Counter | Errors by kind and method |

## gRPC Middleware

The `turbomcp-grpc` crate provides Tower layers for gRPC transport.

### McpGrpcLayer

```rust
use turbomcp_grpc::layer::McpGrpcLayer;
use tower::ServiceBuilder;
use std::time::Duration;

let layer = McpGrpcLayer::new()
    .timeout(Duration::from_secs(30))
    .logging(true)
    .timing(true);

let service = ServiceBuilder::new()
    .layer(layer)
    .service(inner_service);
```

## Client Middleware

The `turbomcp-client` crate provides middleware for MCP clients.

### Caching Layer

```rust
use turbomcp_client::middleware::{CacheLayer, CacheConfig};

let config = CacheConfig::builder()
    .max_entries(1000)
    .ttl(Duration::from_secs(300))
    .build();

let layer = CacheLayer::new(config);
```

### Retry Layer

```rust
use turbomcp_client::middleware::{RetryLayer, RetryConfig};

let config = RetryConfig::builder()
    .max_retries(3)
    .backoff(ExponentialBackoff::default())
    .retry_on(|err| err.is_transient())
    .build();

let layer = RetryLayer::new(config);
```

### Timeout Layer

```rust
use tower::timeout::TimeoutLayer;
use std::time::Duration;

let layer = TimeoutLayer::new(Duration::from_secs(30));
```

## Composing Middleware

Tower middleware composes naturally using `ServiceBuilder`:

```rust
use tower::ServiceBuilder;
use turbomcp_auth::tower::AuthLayer;
use turbomcp_telemetry::tower::{TelemetryLayer, MetricsLayer};
use tower::timeout::TimeoutLayer;
use std::time::Duration;

let service = ServiceBuilder::new()
    // Outer layers process first on request, last on response
    .layer(TelemetryLayer::new(telemetry_config))
    .layer(MetricsLayer::new(metrics_config))
    .layer(TimeoutLayer::new(Duration::from_secs(30)))
    .layer(AuthLayer::new(auth_config))
    // Inner service
    .service(mcp_handler);
```

### Execution Order

```
Request:  Telemetry → Metrics → Timeout → Auth → Handler
Response: Handler → Auth → Timeout → Metrics → Telemetry
```

## Custom Middleware

Implement custom middleware using Tower's `Layer` and `Service` traits:

```rust
use tower::{Layer, Service};
use std::task::{Context, Poll};
use std::pin::Pin;
use std::future::Future;

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
        let inner = self.inner.clone();
        let config = self.config.clone();

        Box::pin(async move {
            // Pre-processing
            println!("Before request");

            // Call inner service
            let response = inner.call(request).await?;

            // Post-processing
            println!("After response");

            Ok(response)
        })
    }
}
```

## MCP-Specific Patterns

### Request Logging

```rust
use tower::{Layer, Service};
use tracing::info;

#[derive(Clone)]
pub struct RequestLoggingLayer;

impl<S> Layer<S> for RequestLoggingLayer {
    type Service = RequestLoggingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestLoggingService { inner }
    }
}

#[derive(Clone)]
pub struct RequestLoggingService<S> {
    inner: S,
}

impl<S> Service<McpRequest> for RequestLoggingService<S>
where
    S: Service<McpRequest>,
{
    // ... implementation

    fn call(&mut self, request: McpRequest) -> Self::Future {
        info!(
            method = %request.method,
            id = ?request.id,
            "MCP request received"
        );
        self.inner.call(request)
    }
}
```

### Rate Limiting by Tool

```rust
use tower::limit::RateLimitLayer;
use std::time::Duration;

fn rate_limit_for_tool(tool_name: &str) -> RateLimitLayer {
    let (rate, per) = match tool_name {
        "expensive_operation" => (10, Duration::from_secs(60)),
        "normal_operation" => (100, Duration::from_secs(60)),
        _ => (1000, Duration::from_secs(60)),
    };
    RateLimitLayer::new(rate, per)
}
```

## Integration with MCP Server

```rust
use turbomcp_server::McpServer;
use tower::ServiceBuilder;

let middleware = ServiceBuilder::new()
    .layer(TelemetryLayer::new(telemetry_config))
    .layer(AuthLayer::new(auth_config));

let server = McpServer::new()
    .with_tower_middleware(middleware)
    .http(8080)
    .run()
    .await?;
```

## Migration from v2 Plugin System

TurboMCP v3 replaces the v2 plugin system with Tower middleware.

### Before (v2 Plugin System)

```rust
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

### After (v3 Tower Middleware)

```rust
// v3: Standard Tower middleware
use tower::{Layer, Service};

let service = ServiceBuilder::new()
    .layer(MyLayer::new())
    .service(client);
```

## Best Practices

### 1. Order Matters

Place tracing/metrics layers outermost to capture full request lifecycle:

```rust
ServiceBuilder::new()
    .layer(TelemetryLayer::new(...))  // First
    .layer(MetricsLayer::new(...))
    .layer(TimeoutLayer::new(...))
    .layer(AuthLayer::new(...))       // Last before handler
    .service(handler)
```

### 2. Use Timeouts

Always add timeout layers to prevent hanging:

```rust
.layer(TimeoutLayer::new(Duration::from_secs(30)))
```

### 3. Clone-Friendly

Tower services must be `Clone`. Use `Arc` for shared state:

```rust
#[derive(Clone)]
pub struct MyService<S> {
    inner: S,
    shared_state: Arc<SharedState>,
}
```

### 4. Graceful Errors

Handle errors gracefully in middleware:

```rust
async fn call(&mut self, request: Request) -> Result<Response, Error> {
    match self.inner.call(request).await {
        Ok(response) => Ok(response),
        Err(e) => {
            // Log error, update metrics, etc.
            tracing::error!(error = %e, "Request failed");
            Err(e)
        }
    }
}
```

## Next Steps

- **[Observability](observability.md)** - OpenTelemetry deep dive
- **[Authentication](authentication.md)** - Auth patterns
- **[gRPC Transport](../api/grpc.md)** - gRPC API reference
- **[Telemetry API](../api/telemetry.md)** - Telemetry API reference

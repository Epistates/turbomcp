# TurboMCP Server

[![Crates.io](https://img.shields.io/crates/v/turbomcp-server.svg)](https://crates.io/crates/turbomcp-server)
[![Documentation](https://docs.rs/turbomcp-server/badge.svg)](https://docs.rs/turbomcp-server)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Production-ready MCP server framework with OAuth 2.0 authentication, middleware pipeline, and enterprise lifecycle management.**

## Overview

`turbomcp-server` provides a comprehensive server framework for Model Context Protocol implementations. It handles all server-side concerns including request routing, authentication, middleware processing, session management, and production lifecycle operations.

## Key Features

### 🗂️ **Handler Registry & Routing**
- **Type-safe registration** - Compile-time handler validation and automatic discovery
- **Efficient routing** - O(1) method dispatch with parameter injection
- **Schema generation** - Automatic JSON schema creation from handler signatures
- **Hot reloading** - Dynamic handler registration and updates (development mode)

### 🔐 **OAuth 2.0 Integration** 
- **Multiple providers** - Google, GitHub, Microsoft, and custom OAuth 2.0 providers
- **PKCE security** - Proof Key for Code Exchange enabled by default
- **All OAuth flows** - Authorization Code, Client Credentials, Device Code
- **Session management** - Secure user session tracking with automatic cleanup

### 🔀 **Middleware Pipeline**
- **Request processing** - Configurable middleware chain with error handling
- **Security middleware** - CORS, CSP, rate limiting, security headers
- **Authentication** - JWT validation, API key, OAuth token verification
- **Observability** - Request logging, metrics collection, distributed tracing

### 📊 **Health & Metrics**
- **Health endpoints** - Readiness, liveness, and custom health checks
- **Performance metrics** - Request timing, error rates, resource utilization
- **Prometheus integration** - Standard metrics format with custom labels
- **Circuit breaker status** - Transport and dependency health monitoring

### 🛑 **Graceful Shutdown**
- **Signal handling** - SIGTERM/SIGINT graceful shutdown with timeout
- **Connection draining** - Active request completion before shutdown
- **Resource cleanup** - Proper cleanup of connections, files, and threads
- **Health status** - Shutdown status reporting for load balancers

## Architecture

```
┌─────────────────────────────────────────────┐
│              TurboMCP Server                │
├─────────────────────────────────────────────┤
│ Request Processing Pipeline                │
│ ├── Middleware chain                       │
│ ├── Authentication layer                   │
│ ├── Request routing                        │
│ └── Handler execution                      │
├─────────────────────────────────────────────┤
│ Handler Registry                           │
│ ├── Type-safe registration                 │
│ ├── Schema generation                      │
│ ├── Parameter validation                   │
│ └── Response serialization                 │
├─────────────────────────────────────────────┤
│ Authentication & Session                   │
│ ├── OAuth 2.0 providers                   │
│ ├── JWT token validation                   │
│ ├── Session lifecycle                      │
│ └── Security middleware                    │
├─────────────────────────────────────────────┤
│ Observability & Lifecycle                 │
│ ├── Health check endpoints                 │
│ ├── Metrics collection                     │
│ ├── Graceful shutdown                      │
│ └── Resource management                    │
└─────────────────────────────────────────────┘
```

## Server Builder

### Basic Server Setup

```rust
use turbomcp_server::{ServerBuilder, McpServer};

// Simple server creation
let server = ServerBuilder::new()
    .name("MyMCPServer")
    .version("1.0.2")
    .build();

// Run with STDIO transport
server.run_stdio().await?;
```

### Production Server Configuration

```rust
use turbomcp_server::{
    ServerBuilder, 
    middleware::{AuthenticationMiddleware, SecurityHeadersMiddleware, RateLimitMiddleware},
    health::HealthCheckConfig,
    metrics::MetricsConfig,
};

let server = ServerBuilder::new()
    .name("ProductionMCPServer")
    .version("2.1.0")
    .description("Enterprise MCP server with full security")
    
    // Authentication middleware
    .middleware(AuthenticationMiddleware::oauth2(oauth_config)
        .with_jwt_validation(jwt_config)
        .with_api_key_auth("X-API-Key"))
    
    // Security middleware
    .middleware(SecurityHeadersMiddleware::strict()
        .with_csp("default-src 'self'; connect-src 'self' wss:")
        .with_hsts(Duration::from_secs(31536000)))
    
    // Rate limiting
    .middleware(RateLimitMiddleware::new()
        .requests_per_minute(120)
        .burst_capacity(20))
    
    // Health and metrics
    .with_health_checks(HealthCheckConfig::new()
        .readiness_endpoint("/health/ready")
        .liveness_endpoint("/health/live")
        .custom_checks(vec![database_health, cache_health]))
    
    .with_metrics(MetricsConfig::new()
        .prometheus_endpoint("/metrics")
        .custom_metrics(true)
        .histogram_buckets([0.001, 0.01, 0.1, 1.0, 10.0]))
    
    // Graceful shutdown
    .with_graceful_shutdown(Duration::from_secs(30))
    
    .build();
```

## Handler Registry

### Manual Handler Registration

```rust
use turbomcp_server::{HandlerRegistry, ToolHandler, ResourceHandler};

let mut registry = HandlerRegistry::new();

// Register tool handlers
registry.register_tool("calculate", ToolHandler::new(|params| async move {
    let a: f64 = params.get("a")?;
    let b: f64 = params.get("b")?;
    Ok(serde_json::json!({"result": a + b}))
})).await?;

// Register resource handlers  
registry.register_resource("file://*", ResourceHandler::new(|uri| async move {
    let path = uri.strip_prefix("file://").unwrap();
    let content = tokio::fs::read_to_string(path).await?;
    Ok(content)
})).await?;

// Attach to server
let server = ServerBuilder::new()
    .with_registry(registry)
    .build();
```

### Schema Validation

```rust
use turbomcp_server::schema::{SchemaValidator, ValidationConfig};

let validator = SchemaValidator::new(ValidationConfig::strict()
    .validate_tool_params(true)
    .validate_responses(true)
    .custom_formats(["email", "uuid"]));

let server = ServerBuilder::new()
    .with_schema_validation(validator)
    .build();
```

## OAuth 2.0 Authentication

### Google OAuth Setup

```rust
use turbomcp_server::auth::{OAuth2Provider, OAuth2Config, ProviderType};

let google_config = OAuth2Config {
    client_id: std::env::var("GOOGLE_CLIENT_ID")?,
    client_secret: std::env::var("GOOGLE_CLIENT_SECRET")?,
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
    token_url: "https://www.googleapis.com/oauth2/v4/token".to_string(),
    scopes: vec!["openid".to_string(), "profile".to_string(), "email".to_string()],
    redirect_uri: "https://myapp.com/auth/callback".to_string(),
    pkce_enabled: true,
};

let google_provider = OAuth2Provider::new(
    "google",
    google_config,
    ProviderType::Google,
).await?;
```

### GitHub OAuth Setup

```rust
let github_config = OAuth2Config {
    client_id: std::env::var("GITHUB_CLIENT_ID")?,
    client_secret: std::env::var("GITHUB_CLIENT_SECRET")?,
    auth_url: "https://github.com/login/oauth/authorize".to_string(),
    token_url: "https://github.com/login/oauth/access_token".to_string(),
    scopes: vec!["user:email".to_string()],
    redirect_uri: "https://myapp.com/auth/github/callback".to_string(),
    pkce_enabled: true,
};

let github_provider = OAuth2Provider::new(
    "github",
    github_config,
    ProviderType::GitHub,
).await?;
```

### Multi-Provider Authentication

```rust
use turbomcp_server::auth::AuthenticationManager;

let auth_manager = AuthenticationManager::new()
    .add_provider("google", google_provider)
    .add_provider("github", github_provider)
    .add_provider("microsoft", microsoft_provider)
    .with_session_store(session_store)
    .with_token_validation(true);

let server = ServerBuilder::new()
    .with_authentication(auth_manager)
    .build();
```

## Middleware System

### Custom Middleware

```rust
use turbomcp_server::{
    Middleware, Request, Response, Next, 
    middleware::{MiddlewareResult, MiddlewareError}
};
use async_trait::async_trait;

struct CustomLoggingMiddleware;

#[async_trait]
impl Middleware for CustomLoggingMiddleware {
    async fn process(
        &self, 
        request: Request, 
        next: Next
    ) -> MiddlewareResult<Response> {
        let start = std::time::Instant::now();
        let method = request.method().clone();
        
        tracing::info!("Processing request: {}", method);
        
        let response = next.run(request).await?;
        
        let duration = start.elapsed();
        tracing::info!("Request {} completed in {:?}", method, duration);
        
        Ok(response)
    }
}

// Register middleware
let server = ServerBuilder::new()
    .middleware(CustomLoggingMiddleware)
    .build();
```

### Error Handling Middleware

```rust
use turbomcp_server::middleware::ErrorHandlerMiddleware;

let error_handler = ErrorHandlerMiddleware::new()
    .handle_authentication_error(|err| async move {
        tracing::warn!("Authentication failed: {}", err);
        Response::unauthorized("Authentication required")
    })
    .handle_validation_error(|err| async move {
        tracing::debug!("Validation failed: {}", err);
        Response::bad_request(&format!("Invalid input: {}", err))
    })
    .handle_internal_error(|err| async move {
        tracing::error!("Internal error: {}", err);
        Response::internal_server_error("Server error")
    });

let server = ServerBuilder::new()
    .middleware(error_handler)
    .build();
```

## Session Management

### Session Configuration

```rust
use turbomcp_server::session::{SessionManager, SessionConfig, SessionStore};

let session_config = SessionConfig::new()
    .ttl(Duration::from_secs(3600)) // 1 hour
    .max_sessions(10000)
    .cleanup_interval(Duration::from_secs(300)) // 5 minutes
    .secure_cookies(true)
    .same_site_strict(true);

let session_store = SessionStore::redis("redis://localhost:6379").await?;
// or
let session_store = SessionStore::memory_with_persistence("/var/lib/sessions").await?;

let session_manager = SessionManager::new(session_config, session_store);

let server = ServerBuilder::new()
    .with_session_management(session_manager)
    .build();
```

## Health Checks

### Built-in Health Checks

```rust
use turbomcp_server::health::{HealthChecker, HealthCheck, HealthStatus};

let health_checker = HealthChecker::new()
    .add_check("database", HealthCheck::database(database_pool))
    .add_check("redis", HealthCheck::redis(redis_client))
    .add_check("external_api", HealthCheck::http("https://api.example.com/health"))
    .add_check("disk_space", HealthCheck::disk_space("/var/lib/myapp", 1024 * 1024 * 1024)); // 1GB minimum

let server = ServerBuilder::new()
    .with_health_checks(health_checker)
    .build();
```

### Custom Health Checks

```rust
use turbomcp_server::health::{HealthCheck, HealthStatus};
use async_trait::async_trait;

struct CustomServiceHealth {
    service_client: ServiceClient,
}

#[async_trait]
impl HealthCheck for CustomServiceHealth {
    async fn check(&self) -> HealthStatus {
        match self.service_client.ping().await {
            Ok(_) => HealthStatus::Healthy,
            Err(e) if e.is_temporary() => HealthStatus::Degraded(vec![e.to_string()]),
            Err(e) => HealthStatus::Unhealthy(e.to_string()),
        }
    }
}

let server = ServerBuilder::new()
    .with_health_check("custom_service", CustomServiceHealth { service_client })
    .build();
```

## Metrics & Observability

### Prometheus Metrics

```rust
use turbomcp_server::metrics::{MetricsCollector, PrometheusConfig};

let metrics = MetricsCollector::prometheus(PrometheusConfig::new()
    .namespace("turbomcp")
    .subsystem("server")
    .endpoint("/metrics")
    .basic_auth("metrics", "secret"));

let server = ServerBuilder::new()
    .with_metrics(metrics)
    .build();

// Metrics are automatically collected:
// - turbomcp_server_requests_total{method, status}
// - turbomcp_server_request_duration_seconds{method}
// - turbomcp_server_active_connections
// - turbomcp_server_errors_total{error_type}
```

### Custom Metrics

```rust
use turbomcp_server::metrics::{Counter, Histogram, Gauge};

struct CustomMetrics {
    business_operations: Counter,
    processing_time: Histogram,  
    active_users: Gauge,
}

impl CustomMetrics {
    fn new() -> Self {
        Self {
            business_operations: Counter::new("business_operations_total", "Total business operations"),
            processing_time: Histogram::new("processing_seconds", "Processing time"),
            active_users: Gauge::new("active_users", "Current active users"),
        }
    }
    
    fn record_operation(&self, operation: &str) {
        self.business_operations.with_label("operation", operation).inc();
    }
}

let server = ServerBuilder::new()
    .with_custom_metrics(CustomMetrics::new())
    .build();
```

## Integration Examples

### With TurboMCP Framework

Server functionality is automatically provided when using the framework:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct ProductionServer {
    database: Database,
    cache: Cache,
}

#[server]
impl ProductionServer {
    #[tool("Process user data")]
    async fn process_user(&self, ctx: Context, user_id: String) -> McpResult<User> {
        // Context provides:
        // - Authentication info: ctx.user_id(), ctx.permissions()
        // - Request correlation: ctx.request_id()
        // - Metrics: ctx.record_metric()
        // - Logging: ctx.info(), ctx.error()
        
        if let Some(authenticated_user) = ctx.user_id() {
            let user = self.database.get_user(&user_id).await?;
            ctx.record_metric("user_lookups", 1);
            Ok(user)
        } else {
            Err(McpError::Unauthorized("Authentication required".to_string()))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = ProductionServer {
        database: Database::connect(&database_url).await?,
        cache: Cache::connect(&redis_url).await?,
    };
    
    // Server infrastructure handled automatically
    server.run_http("0.0.0.0:8080").await?;
    Ok(())
}
```

### Direct Server Usage

For advanced server customization:

```rust
use turbomcp_server::{McpServer, ServerConfig, HandlerRegistry};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::production()
        .with_authentication(auth_config)
        .with_middleware_stack(middleware_stack)
        .with_observability(observability_config);
    
    let mut server = McpServer::with_config(config);
    
    // Manual handler registration
    server.register_tool_handler("advanced_tool", |params| async {
        // Custom tool implementation
        Ok(serde_json::json!({"status": "processed"}))
    }).await?;
    
    // Start server with graceful shutdown
    let (server, shutdown_handle) = server.with_graceful_shutdown();
    
    let server_task = tokio::spawn(async move {
        server.run_http("0.0.0.0:8080").await
    });
    
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received");
    
    shutdown_handle.shutdown().await;
    server_task.await??;
    
    Ok(())
}
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `oauth` | Enable OAuth 2.0 authentication | ✅ |
| `metrics` | Enable metrics collection | ✅ |
| `health-checks` | Enable health check endpoints | ✅ |
| `session-redis` | Enable Redis session storage | ❌ |
| `session-postgres` | Enable PostgreSQL session storage | ❌ |
| `tracing` | Enable distributed tracing | ✅ |
| `compression` | Enable response compression | ✅ |

## Development

### Building

```bash
# Build with all features
cargo build --all-features

# Build minimal server
cargo build --no-default-features --features basic

# Build with OAuth only
cargo build --no-default-features --features oauth
```

### Testing

```bash
# Run server tests
cargo test

# Test with OAuth providers (requires environment variables)
GOOGLE_CLIENT_ID=test GOOGLE_CLIENT_SECRET=test cargo test oauth

# Integration tests
cargo test --test integration

# Test graceful shutdown
cargo test graceful_shutdown
```

### Development Server

```bash
# Run development server with hot reloading
cargo run --example dev_server

# Run with debug logging
RUST_LOG=debug cargo run --example production_server
```

## Related Crates

- **[turbomcp](../turbomcp/)** - Main framework (uses this crate)
- **[turbomcp-core](../turbomcp-core/)** - Core types and utilities
- **[turbomcp-transport](../turbomcp-transport/)** - Transport layer
- **[turbomcp-protocol](../turbomcp-protocol/)** - MCP protocol implementation

## External Resources

- **[OAuth 2.0 Specification](https://tools.ietf.org/html/rfc6749)** - OAuth 2.0 authorization framework
- **[PKCE Specification](https://tools.ietf.org/html/rfc7636)** - Proof Key for Code Exchange
- **[Prometheus Metrics](https://prometheus.io/docs/concepts/data_model/)** - Metrics format specification

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) high-performance Rust SDK for the Model Context Protocol.*
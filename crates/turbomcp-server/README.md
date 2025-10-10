# TurboMCP Server

[![Crates.io](https://img.shields.io/crates/v/turbomcp-server.svg)](https://crates.io/crates/turbomcp-server)
[![Documentation](https://docs.rs/turbomcp-server/badge.svg)](https://docs.rs/turbomcp-server)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

MCP server framework with OAuth 2.1 MCP compliance, middleware pipeline, and lifecycle management.

## Overview

`turbomcp-server` provides a comprehensive server framework for Model Context Protocol implementations. It handles all server-side concerns including request routing, authentication, middleware processing, session management, and production lifecycle operations.

### Security Hardened
- Zero Known Vulnerabilities - Comprehensive security audit and hardening
- Dependency Security - Eliminated all vulnerable dependency paths
- MIT-Compatible Licensing - Strict open-source license compliance

## Key Features

### Handler Registry & Routing
- Type-safe registration - Compile-time handler validation and automatic discovery
- Efficient routing - O(1) method dispatch with parameter injection
- Schema generation - Automatic JSON schema creation from handler signatures
- Hot reloading - Dynamic handler registration and updates (development mode)

### OAuth 2.1 MCP Compliance 
- Multiple providers - Google, GitHub, Microsoft, and custom OAuth 2.1 providers
- PKCE security - Proof Key for Code Exchange enabled by default
- All OAuth flows - Authorization Code, Client Credentials, Device Code
- Session management - Secure user session tracking with automatic cleanup

### Middleware Pipeline
- Request processing - Configurable middleware chain with error handling
- Security middleware - CORS, CSP, rate limiting, security headers
- Authentication - JWT validation, API key, OAuth token verification
- Observability - Request logging, metrics collection, distributed tracing

### Health & Metrics
- Health endpoints - Readiness, liveness, and custom health checks
- Performance metrics - Request timing, error rates, resource utilization
- Prometheus integration - Standard metrics format with custom labels
- Circuit breaker status - Transport and dependency health monitoring

### Graceful Shutdown
- Signal handling - SIGTERM/SIGINT graceful shutdown with timeout
- Connection draining - Active request completion before shutdown
- Resource cleanup - Proper cleanup of connections, files, and threads
- Health status - Shutdown status reporting for load balancers

### Clone Pattern for Server Sharing (Axum/Tower Standard)
- Cheap cloning - All heavy state is Arc-wrapped (just atomic increments)
- Tower compatible - Same pattern as Axum's Router and Tower services
- No wrapper types - Server is directly Clone (no Arc<McpServer> needed)
- Concurrent access - Share across multiple async tasks for monitoring
- Zero overhead - Same performance as direct server usage
- Type safe - Same type whether cloned or not

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
    .version("2.0.0")
    .build();

// Run with STDIO transport
server.run_stdio().await?;
```

### Production Server with Handlers

```rust
use turbomcp_server::ServerBuilder;
use turbomcp_protocol::types::Root;

let server = ServerBuilder::new()
    .name("ProductionMCPServer")
    .version("2.1.0")
    .description("Enterprise MCP server with comprehensive tooling")

    // Register filesystem roots
    .root("file:///workspace", Some("Workspace".to_string()))
    .root("file:///tmp", Some("Temp".to_string()))

    // Register tool handlers (traits implement ToolHandler)
    .tool("calculate", calculate_tool)?
    .tool("search", search_tool)?

    // Register resource handlers
    .resource("config://settings", config_resource)?
    .resource("db://users/*", user_resource)?

    // Register prompt handlers
    .prompt("code_review", code_review_prompt)?

    .build();

// Middleware, auth, and observability are configured separately
// via the MiddlewareStack (see Middleware System section)
```

## Handler Registry

### Handler Traits

Handlers implement trait interfaces for type-safe registration:

```rust
use turbomcp_server::{ServerBuilder, ToolHandler, ServerResult};
use turbomcp_protocol::{RequestContext, types::{CallToolRequest, CallToolResult, Tool, ContentBlock, TextContent}};
use async_trait::async_trait;
use serde_json::json;

// Example tool handler
struct CalculateTool;

#[async_trait]
impl ToolHandler for CalculateTool {
    async fn handle(
        &self,
        request: CallToolRequest,
        _ctx: RequestContext,
    ) -> ServerResult<CallToolResult> {
        let a: f64 = request.arguments.get("a")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| turbomcp_server::ServerError::InvalidToolInput("Missing 'a' parameter".into()))?;
        let b: f64 = request.arguments.get("b")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| turbomcp_server::ServerError::InvalidToolInput("Missing 'b' parameter".into()))?;

        let result = a + b;

        Ok(CallToolResult {
            content: vec![ContentBlock::Text(TextContent {
                text: format!("Result: {}", result),
                annotations: None,
            })],
            is_error: Some(false),
            structured_content: None,
            _meta: None,
        })
    }

    fn tool_definition(&self) -> Tool {
        Tool::new("calculate")
            .with_description("Add two numbers")
            .with_input_schema(
                turbomcp_protocol::types::ToolInputSchema::empty()
                    .add_property("a".to_string(), json!({"type": "number"}))
                    .add_property("b".to_string(), json!({"type": "number"}))
                    .require_property("a".to_string())
                    .require_property("b".to_string())
            )
    }
}

// Register via builder
let server = ServerBuilder::new()
    .tool("calculate", CalculateTool)?
    .build();
```

## Authentication with turbomcp-auth

The server integrates with the `turbomcp-auth` crate for comprehensive authentication:

### OAuth 2.1 Setup

```rust
use turbomcp_auth::{AuthManager, AuthConfig, OAuth2Config, AuthProviderType};

// Configure OAuth 2.1
let oauth_config = OAuth2Config {
    client_id: std::env::var("GOOGLE_CLIENT_ID")?,
    client_secret: std::env::var("GOOGLE_CLIENT_SECRET")?,
    auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
    token_url: "https://www.googleapis.com/oauth2/v4/token".to_string(),
    redirect_uri: "https://myapp.com/auth/callback".to_string(),
    scopes: vec!["openid".to_string(), "profile".to_string(), "email".to_string()],
    flow_type: OAuth2FlowType::AuthorizationCode,
    additional_params: HashMap::new(),
    security_level: SecurityLevel::Standard,
    #[cfg(feature = "dpop")]
    dpop_config: None,
    mcp_resource_uri: Some("https://myapp.com/mcp".to_string()),
    auto_resource_indicators: true,
};

// Create auth manager
let mut settings_map = HashMap::new();
// Serialize OAuth config to Value, then extract as object
if let serde_json::Value::Object(map) = serde_json::to_value(&oauth_config)? {
    for (key, value) in map {
        settings_map.insert(key, value);
    }
}

let auth_config = AuthConfig {
    enabled: true,
    providers: vec![AuthProviderConfig {
        name: "google".to_string(),
        provider_type: AuthProviderType::OAuth2,
        settings: settings_map,
        enabled: true,
        priority: 1,
    }],
    session: SessionConfig::default(),
    authorization: AuthorizationConfig::default(),
};

let auth_manager = AuthManager::new(auth_config);

// Add to your server implementation
// (integration varies based on transport type)
```

### JWT Authentication via Middleware

For JWT-only authentication, use the server's built-in middleware:

```rust
use turbomcp_server::middleware::AuthConfig;
use secrecy::Secret;
use jsonwebtoken::Algorithm;

// JWT authentication is available via middleware (feature: auth)
#[cfg(feature = "auth")]
let auth_config = AuthConfig {
    secret: Secret::new(std::env::var("JWT_SECRET")?),
    algorithm: Algorithm::HS256,
    issuer: Some("your-issuer".to_string()),
    audience: Some("your-audience".to_string()),
    leeway: 60,
    validate_exp: true,
    validate_nbf: true,
};
```

## Middleware System

The server uses a Tower-based middleware stack for cross-cutting concerns:

```rust
use turbomcp_server::middleware::{
    MiddlewareStack, SecurityConfig, CorsConfig, CorsOrigins, ValidationConfig,
    AuthConfig, RateLimitConfig, RateLimitStrategy, RateLimits,
    AuditConfig, AuditLogLevel, TimeoutConfig
};
use http::Method;
use std::time::Duration;

// Build a comprehensive middleware stack
let middleware = MiddlewareStack::new()
    .with_security(SecurityConfig {
        cors: CorsConfig {
            allowed_origins: CorsOrigins::List(vec![
                "https://app.example.com".to_string()
            ]),
            allowed_methods: vec![Method::GET, Method::POST],
            max_age: Some(Duration::from_secs(86400)),
            ..Default::default()
        },
        ..Default::default()
    })
    .with_validation(ValidationConfig {
        schemas: Default::default(),
        validate_requests: true,
        validate_responses: false,
        strict_mode: true,
    })
    .with_timeout(TimeoutConfig {
        request_timeout: Duration::from_secs(30),
        enabled: true,
    })
    .with_audit(AuditConfig {
        log_success: true,
        log_failures: true,
        log_auth_events: true,
        log_authz_events: true,
        log_level: AuditLogLevel::Info,
    });

// With auth feature enabled:
#[cfg(feature = "auth")]
{
    use secrecy::Secret;
    use jsonwebtoken::Algorithm;

    let middleware = middleware.with_auth(AuthConfig {
        secret: Secret::new("your-secret-key".to_string()),
        algorithm: Algorithm::HS256,
        issuer: Some("your-app".to_string()),
        audience: Some("your-api".to_string()),
        leeway: 60,
        validate_exp: true,
        validate_nbf: true,
    });
}

// With rate-limiting feature enabled:
#[cfg(feature = "rate-limiting")]
{
    use std::num::NonZeroU32;

    let middleware = middleware.with_rate_limit(RateLimitConfig {
        strategy: RateLimitStrategy::PerIp,
        limits: RateLimits {
            requests_per_period: NonZeroU32::new(100).unwrap(),
            period: Duration::from_secs(60), // 100 requests per minute
            burst_size: Some(NonZeroU32::new(20).unwrap()),
        },
        enabled: true,
    });
}

// The middleware stack is automatically applied by the server
```

## Session Management with turbomcp

Session management is provided by the main `turbomcp` crate:

```rust
use turbomcp::{SessionManager, SessionConfig};
use std::time::Duration;

// Configure session management
let session_config = SessionConfig {
    timeout: Duration::from_secs(3600), // 1 hour
    enable_analytics: true,
    max_sessions_per_client: Some(10),
    max_total_sessions: Some(1000),
    cleanup_interval: Duration::from_secs(300), // 5 minutes
    track_activity: true,
    max_session_data_size: Some(1024 * 1024), // 1MB
};

// Or use preset configurations
let session_config = SessionConfig::high_performance();

// Create session manager
let session_manager = SessionManager::new(session_config);

// Session manager handles:
// - Session lifecycle (create, update, expire)
// - Per-client session limits with LRU eviction
// - Automatic cleanup of expired sessions
// - Session analytics and activity tracking
```

## Health & Lifecycle

The server provides built-in health status and graceful shutdown:

```rust
use turbomcp_server::ServerBuilder;

let server = ServerBuilder::new()
    .name("MyServer")
    .version("2.0.0")
    .build();

// Get health status
let health = server.health().await;
if health.healthy {
    println!("Server is healthy (checked at {:?})", health.timestamp);
    for check in &health.details {
        println!("  - {}: {}", check.name, if check.healthy { "OK" } else { "FAILED" });
    }
} else {
    eprintln!("Server is unhealthy!");
}

// Graceful shutdown
let shutdown_handle = server.shutdown_handle();
tokio::spawn(async move {
    tokio::signal::ctrl_c().await.ok();
    shutdown_handle.shutdown().await;
});

server.run_stdio().await?;
```

## Metrics & Observability

The server provides built-in production-grade metrics collection with lock-free atomic operations:

```rust
use turbomcp_server::{ServerBuilder, ServerMetrics};

let server = ServerBuilder::new()
    .name("MyServer")
    .version("2.0.0")
    .build();

// Access server metrics
let metrics = server.metrics();

// Metrics are automatically collected:
println!("Total requests: {}", metrics.requests_total.load(Ordering::Relaxed));
println!("Successful: {}", metrics.requests_successful.load(Ordering::Relaxed));
println!("Failed: {}", metrics.requests_failed.load(Ordering::Relaxed));
println!("In flight: {}", metrics.requests_in_flight.load(Ordering::Relaxed));

// Error metrics
println!("Total errors: {}", metrics.errors_total.load(Ordering::Relaxed));
println!("Validation errors: {}", metrics.errors_validation.load(Ordering::Relaxed));
println!("Auth errors: {}", metrics.errors_auth.load(Ordering::Relaxed));

// Tool execution metrics
println!("Tool calls: {}", metrics.tool_calls_total.load(Ordering::Relaxed));
println!("Tool timeouts: {}", metrics.tool_timeouts_total.load(Ordering::Relaxed));

// Connection metrics
println!("Active connections: {}", metrics.connections_active.load(Ordering::Relaxed));
println!("Total connections: {}", metrics.connections_total.load(Ordering::Relaxed));

// Response time statistics
let avg_response_time_us = metrics.total_response_time_us.load(Ordering::Relaxed)
    / metrics.requests_total.load(Ordering::Relaxed).max(1);
println!("Avg response time: {}μs", avg_response_time_us);

// Custom metrics (use the RwLock-protected HashMap)
{
    let mut custom = metrics.custom.write();
    custom.insert("my_metric".to_string(), 42.0);
}
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
    async fn process_user(&self, ctx: Context, target_user_id: String) -> McpResult<User> {
    // Example: Use Context API for authentication and user info
    if !ctx.is_authenticated() {
        return Err(McpError::Unauthorized("Authentication required".to_string()));
    }
    
    let current_user = ctx.user_id().unwrap_or("anonymous");
    let roles = ctx.roles();
    
    ctx.info(&format!("User {} accessing profile for {}", current_user, target_user_id)).await?;
    
    // Check permissions
    if !roles.contains(&"admin".to_string()) && current_user != target_user_id {
        return Err(McpError::Unauthorized("Insufficient permissions".to_string()));
    }
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

## Server Sharing with Clone (Axum/Tower Pattern)

TurboMCP follows the **Axum/Tower Clone pattern** for sharing server instances across tasks and threads. All heavy state is Arc-wrapped internally, making cloning cheap (just atomic reference count increments).

### Basic Server Cloning

```rust
use turbomcp_server::{ServerBuilder, ServerConfig};

// Create server (Clone-able)
let server = ServerBuilder::new()
    .name("MyServer")
    .version("2.0.0")
    .build();

// Clone for monitoring tasks (cheap - just Arc increments)
let monitor1 = server.clone();
let monitor2 = server.clone();

// Concurrent monitoring operations
let health_task = tokio::spawn(async move {
    loop {
        let health = monitor1.health().await;
        println!("Server health: {:?}", health);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
});

let metrics_task = tokio::spawn(async move {
    loop {
        let metrics = monitor2.metrics();
        println!("Server metrics: request_count={}",
            metrics.requests_total.load(std::sync::atomic::Ordering::Relaxed));
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
});

// Run the server
server.run_stdio().await?;
```

### Advanced Server Monitoring

```rust
use turbomcp_server::{ServerBuilder, HealthStatus};
use std::sync::Arc;
use tokio::sync::Notify;

let server = ServerBuilder::new().build();
let shutdown_notify = Arc::new(Notify::new());

// Health monitoring task
let monitor = server.clone();
let notify = shutdown_notify.clone();
let health_task = tokio::spawn(async move {
    loop {
        let health_status = monitor.health().await;
        if health_status.healthy {
            println!("✅ Server healthy ({} checks passed)", health_status.details.len());
        } else {
            println!("❌ Server unhealthy");
            for check in &health_status.details {
                if !check.healthy {
                    println!("  Failed: {}", check.name);
                    if let Some(msg) = &check.message {
                        println!("    Reason: {}", msg);
                    }
                }
            }
            notify.notify_one();
            break;
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
});

// Metrics collection task
let metrics_monitor = server.clone();
let metrics_task = tokio::spawn(async move {
    loop {
        let metrics = metrics_monitor.metrics();
        send_to_prometheus(metrics).await;
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
});

// Run server with monitoring
let server_task = tokio::spawn(async move {
    server.run_stdio().await
});

// Wait for shutdown signal or server completion
tokio::select! {
    _ = shutdown_notify.notified() => {
        println!("Shutting down due to health check failure");
    }
    result = server_task => {
        println!("Server completed: {:?}", result);
    }
}
```

### Benefits of the Clone Pattern

- **Cheap Cloning**: Just atomic reference count increments (Arc-wrapped state)
- **Tower Compatible**: Follows the same pattern as Axum's Router
- **No Arc Wrappers**: No need for `Arc<McpServer>` - server is directly Clone
- **Type Safe**: Same type whether cloned or not (no wrapper types)
- **Zero Overhead**: Same performance as direct server usage
- **Ecosystem Standard**: Matches Axum, Tower, Hyper conventions

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
- **[turbomcp-protocol](../turbomcp-protocol/)** - Protocol implementation and core utilities
- **[turbomcp-transport](../turbomcp-transport/)** - Transport layer

**Note:** In v2.0.0, `turbomcp-core` was merged into `turbomcp-protocol` to eliminate circular dependencies.

## External Resources

- **[OAuth 2.0 Specification](https://tools.ietf.org/html/rfc6749)** - OAuth 2.0 authorization framework
- **[PKCE Specification](https://tools.ietf.org/html/rfc7636)** - Proof Key for Code Exchange
- **[Prometheus Metrics](https://prometheus.io/docs/concepts/data_model/)** - Metrics format specification

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) high-performance Rust SDK for the Model Context Protocol.*
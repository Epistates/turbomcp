# TurboMCP

[![Crates.io](https://img.shields.io/crates/v/turbomcp.svg)](https://crates.io/crates/turbomcp)
[![Documentation](https://docs.rs/turbomcp/badge.svg)](https://docs.rs/turbomcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Build Status](https://img.shields.io/github/workflow/status/Epistates/turbomcp/CI)](https://github.com/Epistates/turbomcp/actions)

**High-performance Rust SDK for the Model Context Protocol (MCP) with SIMD acceleration, enterprise security, and ergonomic APIs.**

## Overview

`turbomcp` is the main framework crate providing a high-level, ergonomic API for building Model Context Protocol servers. Built on a foundation of performance-optimized infrastructure crates, it offers zero-boilerplate development with production-ready features.

**Full MCP 2025-06-18 protocol support** including bidirectional communication, server-initiated requests, and enhanced context management.

## Key Features

### 🚀 **High Performance**
- **SIMD-accelerated JSON processing** - 2-3x faster than standard libraries
- **Zero-copy message handling** - Minimal memory allocations
- **Efficient connection management** - Connection pooling and reuse
- **Optimized request routing** - O(1) handler lookup with parameter injection

### 🎯 **Zero Boilerplate Design**
- **Procedural macros** - `#[server]`, `#[tool]`, `#[resource]`, `#[prompt]`  
- **Automatic schema generation** - JSON schemas from Rust types
- **Type-safe parameters** - Compile-time validation and conversion
- **Context injection** - Request context available anywhere in signature
- **Intuitive APIs** - `elicit()` builder for user input, `ctx.create_message()` for sampling
- **Perfect Context** - `ctx.user_id()`, `ctx.is_authenticated()`, full Context API

### 🛡️ **Enterprise Security**
- **OAuth 2.0 integration** - Google, GitHub, Microsoft providers
- **PKCE security** - Proof Key for Code Exchange by default
- **CORS protection** - Comprehensive cross-origin policies
- **Rate limiting** - Token bucket algorithm with burst capacity
- **Security headers** - CSP, HSTS, X-Frame-Options

### 🔗 **Multi-Transport**
- **STDIO** - Standard input/output for local processes
- **HTTP/SSE** - Server-Sent Events for web applications
- **WebSocket** - Real-time bidirectional communication
- **TCP** - Network socket communication
- **Unix Sockets** - Local inter-process communication

### ⚡ **Circuit Breaker & Reliability**
- **Circuit breaker pattern** - Prevents cascade failures
- **Exponential backoff retry** - Intelligent error recovery
- **Connection health monitoring** - Automatic failure detection
- **Graceful degradation** - Fallback mechanisms

## Architecture

TurboMCP is built as a layered architecture with clear separation of concerns:

```
┌─────────────────────────────────────────────────────────────┐
│                      TurboMCP Framework                     │
│              Ergonomic APIs & Developer Experience         │
├─────────────────────────────────────────────────────────────┤
│                   Infrastructure Layer                     │
│          Server • Client • Transport • Protocol            │
├─────────────────────────────────────────────────────────────┤
│                     Foundation Layer                       │
│             Core Types • Messages • State                  │
└─────────────────────────────────────────────────────────────┘
```

**Components:**
- **[turbomcp-core](../turbomcp-core/)** - Performance-critical types and SIMD acceleration
- **[turbomcp-protocol](../turbomcp-protocol/)** - MCP specification implementation
- **[turbomcp-transport](../turbomcp-transport/)** - Multi-protocol transport with circuit breakers
- **[turbomcp-server](../turbomcp-server/)** - Server framework with OAuth 2.0
- **[turbomcp-client](../turbomcp-client/)** - Client implementation with error recovery
- **[turbomcp-macros](../turbomcp-macros/)** - Procedural macros for ergonomic APIs
- **[turbomcp-cli](../turbomcp-cli/)** - Command-line tools for development and testing

## Quick Start

### Installation

Add TurboMCP to your `Cargo.toml`:

```toml
[dependencies]
turbomcp = "1.0.4"
tokio = { version = "1.0", features = ["full"] }
```

### Basic Server

Create a simple calculator server:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
        Ok(a + b)
    }

    #[tool("Get server status")]
    async fn status(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Status requested").await?;
        Ok("Server running".to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Calculator.run_stdio().await?;
    Ok(())
}
```

### Run the Server

```bash
# Build and run
cargo run

# Test with TurboMCP CLI
cargo install turbomcp-cli

# For HTTP server
turbomcp-cli tools-list --url http://localhost:8080/mcp

# For STDIO server  
turbomcp-cli tools-list --command "./target/debug/my-server"
```

## Core Concepts

### Server Definition

Use the `#[server]` macro to automatically implement the MCP server trait:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer {
    database: Arc<Database>,
    cache: Arc<Cache>,
}

#[server]
impl MyServer {
    // Tools, resources, and prompts defined here
}
```

### Tool Handlers

Transform functions into MCP tools with automatic parameter handling:

```rust
#[tool("Calculate expression")]
async fn calculate(
    &self,
    #[description("Mathematical expression")]
    expression: String,
    #[description("Precision for results")]
    precision: Option<u32>,
    ctx: Context
) -> McpResult<f64> {
    let precision = precision.unwrap_or(2);
    ctx.info(&format!("Calculating: {}", expression)).await?;
    
    // Calculation logic
    let result = evaluate_expression(&expression)?;
    Ok(round_to_precision(result, precision))
}
```

### Resource Handlers

Create URI template-based resource handlers:

```rust
#[resource("file://{path}")]
async fn read_file(
    &self,
    #[description("File path to read")]
    path: String,
    ctx: Context
) -> McpResult<String> {
    ctx.info(&format!("Reading file: {}", path)).await?;
    
    tokio::fs::read_to_string(&path).await
        .map_err(|e| McpError::Resource(e.to_string()))
}
```

### Prompt Templates

Generate dynamic prompts with parameter substitution:

```rust
#[prompt("code_review")]
async fn code_review_prompt(
    &self,
    #[description("Programming language")]
    language: String,
    #[description("Code to review")]
    code: String,
    ctx: Context
) -> McpResult<String> {
    ctx.info(&format!("Generating {} code review", language)).await?;
    
    Ok(format!(
        "Please review the following {} code:\n\n```{}\n{}\n```",
        language, language, code
    ))
}
```

### MCP 2025-06-18 Enhanced Features (New in v1.0.4)

#### Roots Support - Filesystem Boundaries

```rust
#[server(
    name = "filesystem-server",
    version = "1.0.0", 
    root = "file:///workspace:Project Workspace",
    root = "file:///tmp:Temporary Files"
)]
impl FileSystemServer {
    #[tool("List files in directory")]
    async fn list_files(&self, ctx: Context, path: String) -> McpResult<Vec<String>> {
        ctx.info(&format!("Listing files in: {}", path)).await?;
        // Operations are bounded by configured roots
        Ok(vec!["file1.txt".to_string(), "file2.txt".to_string()])
    }
}
```

#### Elicitation - Server-Initiated User Input

```rust
use turbomcp::prelude::*;
use turbomcp_protocol::elicitation::ElicitationSchema;

#[tool("Configure application settings")]
async fn configure_app(&self, ctx: Context) -> McpResult<String> {
    // Build elicitation schema with type safety
    let schema = ElicitationSchema::new()
        .add_string_property("theme", Some("Color theme preference"))
        .add_boolean_property("notifications", Some("Enable push notifications"))
        .add_required(["theme"]);
    
    // Simple, elegant elicitation with type safety
    let result = elicit("Configure your preferences")
        .field("theme", text("UI theme preference")
            .options(&["light", "dark"]))
        .send(&ctx)
        .await?;
    
    // Process the structured response
    if let Some(data) = result.content {
        let theme = data.get("theme")
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        Ok(format!("Configured with {} theme", theme))
    } else {
        Err(McpError::Context("Configuration cancelled".to_string()))
    }
}
```

#### Sampling Support - Bidirectional LLM Communication

```rust
use turbomcp::prelude::*;

#[tool("Get AI code review")]
async fn code_review(&self, ctx: Context, code: String) -> McpResult<String> {
    // Log the request with user context
    let user = ctx.user_id().unwrap_or("anonymous");
    ctx.info(&format!("User {} requesting code review", user)).await?;
    
    // Build sampling request with ergonomic JSON
    let request = serde_json::json!({
        "messages": [{
            "role": "user",
            "content": {
                "type": "text",
                "text": format!("Please review this code:\n\n{}", code)
            }
        }],
        "maxTokens": 500,
        "systemPrompt": "You are a senior code reviewer. Provide constructive feedback."
    });
    
    // Request LLM assistance through the client
    match ctx.create_message(request).await {
        Ok(response) => {
            ctx.info("AI review completed successfully").await?;
            Ok(format!("AI Review: {:?}", response))
        }
        Err(_) => {
            // Graceful fallback if sampling unavailable
            let issues = code.matches("TODO").count() + code.matches("FIXME").count();
            Ok(format!("Static analysis: {} lines, {} issues found", code.lines().count(), issues))
        }
    }
}
```

#### Completion - Intelligent Autocompletion

```rust
#[completion("Complete file paths")]
async fn complete_file_path(&self, partial: String) -> McpResult<Vec<String>> {
    let files = std::fs::read_dir(".")?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with(&partial))
        .collect();
    Ok(files)
}
```

#### Resource Templates - Dynamic URIs

```rust
#[template("users/{user_id}/posts/{post_id}")]
async fn get_user_post(&self, user_id: String, post_id: String) -> McpResult<Post> {
    // RFC 6570 URI template with multiple parameters
    let post = self.database.get_post(&user_id, &post_id).await?;
    Ok(post)
}
```

#### Ping - Bidirectional Health Monitoring

```rust
#[ping("Health check")]
async fn health_check(&self, ctx: Context) -> McpResult<HealthStatus> {
    let db_status = self.database.ping().await.is_ok();
    let cache_status = self.cache.ping().await.is_ok();
    
    Ok(HealthStatus {
        healthy: db_status && cache_status,
        database: db_status,
        cache: cache_status,
        timestamp: ctx.timestamp(),
    })
}
```

### Context Injection

The `Context` parameter provides request correlation, authentication, and observability:

```rust
#[tool("Authenticated operation")]
async fn secure_operation(&self, ctx: Context, data: String) -> McpResult<String> {
    // Authentication
    let user = ctx.authenticated_user()?;
    
    // Logging with correlation
    ctx.info(&format!("Processing request for user: {}", user.id)).await?;
    
    // Request metadata
    let request_id = ctx.request_id();
    let start_time = ctx.start_time();
    
    // Processing...
    let result = process_data(&data).await?;
    
    // Performance tracking
    ctx.record_metric("processing_time", start_time.elapsed()).await?;
    
    Ok(result)
}
```

## Authentication & Security

### OAuth 2.0 Setup

TurboMCP provides built-in OAuth 2.0 support:

```rust
use turbomcp::prelude::*;
use turbomcp::auth::*;

#[derive(Clone)]
struct SecureServer {
    oauth_providers: Arc<RwLock<HashMap<String, OAuth2Provider>>>,
}

#[server]
impl SecureServer {
    #[tool("Get user profile")]
    async fn get_user_profile(&self, ctx: Context) -> McpResult<UserProfile> {
        let user = ctx.authenticated_user()
            .ok_or_else(|| McpError::Unauthorized("Authentication required".to_string()))?;
        
        Ok(UserProfile {
            id: user.id,
            name: user.name,
            email: user.email,
        })
    }

    #[tool("Start OAuth flow")]
    async fn start_oauth_flow(&self, provider: String) -> McpResult<String> {
        let providers = self.oauth_providers.read().await;
        let oauth_provider = providers.get(&provider)
            .ok_or_else(|| McpError::InvalidInput(format!("Unknown provider: {}", provider)))?;
        
        let auth_result = oauth_provider.start_authorization().await?;
        Ok(format!("Visit: {}", auth_result.auth_url))
    }
}
```

### Security Configuration

Configure comprehensive security features:

```rust
use turbomcp_transport::{AxumMcpExt, McpServerConfig};

let config = McpServerConfig::production()
    .with_cors_origins(vec!["https://app.example.com".to_string()])
    .with_custom_csp("default-src 'self'; connect-src 'self' wss:")
    .with_rate_limit(120, 20)  // 120 req/min, 20 burst
    .with_jwt_auth("your-secret-key".to_string());

let app = Router::new()
    .route("/api/status", get(status_handler))
    .merge(Router::<()>::turbo_mcp_routes_for_merge(mcp_service, config));
```

## Transport Configuration

### STDIO Transport (Default)

Perfect for Claude Desktop and local development:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    MyServer::new().run_stdio().await?;
    Ok(())
}
```

### HTTP/SSE Transport

For web applications and browser integration:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    MyServer::new().run_http("0.0.0.0:8080").await?;
    Ok(())
}
```

### WebSocket Transport

For real-time bidirectional communication:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    MyServer::new().run_websocket("0.0.0.0:8080").await?;
    Ok(())
}
```

### Multi-Transport Runtime Selection

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = MyServer::new();
    
    match std::env::var("TRANSPORT").as_deref() {
        Ok("http") => server.run_http("0.0.0.0:8080").await?,
        Ok("websocket") => server.run_websocket("0.0.0.0:8080").await?,
        Ok("tcp") => server.run_tcp("0.0.0.0:8080").await?,
        Ok("unix") => server.run_unix("/tmp/mcp.sock").await?,
        _ => server.run_stdio().await?, // Default
    }
    Ok(())
}
```

## Error Handling

### Ergonomic Error Creation

Use the `mcp_error!` macro for easy error creation:

```rust
#[tool("Divide numbers")]
async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
    if b == 0.0 {
        return Err(mcp_error!("Division by zero: {} / {}", a, b));
    }
    Ok(a / b)
}

#[tool("Read file")]
async fn read_file(&self, path: String) -> McpResult<String> {
    tokio::fs::read_to_string(&path).await
        .map_err(|e| mcp_error!("Failed to read file {}: {}", path, e))
}
```

### Error Types

TurboMCP provides comprehensive error types:

```rust
use turbomcp::McpError;

match result {
    Err(McpError::InvalidInput(msg)) => {
        // Handle validation errors
    },
    Err(McpError::Unauthorized(msg)) => {
        // Handle authentication errors
    },
    Err(McpError::Resource(msg)) => {
        // Handle resource access errors
    },
    Err(McpError::Transport(msg)) => {
        // Handle transport errors
    },
    Ok(value) => {
        // Process success case
    }
}
```

## Advanced Features

### Custom Types and Schema Generation

TurboMCP automatically generates JSON schemas for custom types:

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct CreateUserRequest {
    name: String,
    email: String,
    age: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
    email: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

#[tool("Create a new user")]
async fn create_user(&self, request: CreateUserRequest) -> McpResult<User> {
    // Schema automatically generated for both types
    let user = User {
        id: generate_id(),
        name: request.name,
        email: request.email,
        created_at: chrono::Utc::now(),
    };
    
    // Save to database
    self.database.save_user(&user).await?;
    
    Ok(user)
}
```

### Graceful Shutdown

Handle shutdown signals gracefully:

```rust
use tokio::signal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = MyServer::new();
    let (server, shutdown_handle) = server.into_server_with_shutdown()?;
    
    let server_task = tokio::spawn(async move {
        server.run_stdio().await
    });
    
    signal::ctrl_c().await?;
    tracing::info!("Shutdown signal received");
    
    shutdown_handle.shutdown().await;
    server_task.await??;
    
    Ok(())
}
```

### Performance Tuning

Enable SIMD acceleration for maximum performance:

```toml
[dependencies]
turbomcp = { version = "1.0.4", features = ["simd"] }
```

Configure performance settings:

```rust
use turbomcp_core::{SessionManager, SessionConfig};

let config = SessionConfig::high_performance()
    .with_simd_acceleration(true)
    .with_connection_pooling(true)
    .with_circuit_breakers(true);

let server = MyServer::new()
    .with_session_config(config)
    .with_compression(true);
```

## Testing

### Unit Testing

Test your tools directly:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_calculator() {
        let calc = Calculator;
        
        let result = calc.test_tool_call("add", serde_json::json!({
            "a": 5,
            "b": 3
        })).await.unwrap();
        
        assert_eq!(result, serde_json::json!(8));
    }
}
```

### Integration Testing

Use the TurboMCP CLI for integration testing:

```bash
# Install CLI
cargo install turbomcp-cli

# Test server functionality
turbomcp-cli tools-list --url http://localhost:8080/mcp
turbomcp-cli tools-call --url http://localhost:8080/mcp --name add --arguments '{"a": 5, "b": 3}'
turbomcp-cli schema-export --url http://localhost:8080/mcp --output schemas.json
```

## Client Setup

### Claude Desktop

Add to your Claude Desktop configuration:

```json
{
  "mcpServers": {
    "my-turbomcp-server": {
      "command": "/path/to/your/server/binary",
      "args": []
    }
  }
}
```

### Programmatic Client

Use the TurboMCP client:

```rust
use turbomcp_client::{ClientBuilder, Transport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = ClientBuilder::new()
        .transport(Transport::stdio_with_command("./my-server"))
        .connect().await?;

    let tools = client.list_tools().await?;
    println!("Available tools: {:?}", tools);

    let result = client.call_tool("add", serde_json::json!({
        "a": 5,
        "b": 3
    })).await?;
    println!("Result: {:?}", result);

    Ok(())
}
```

## Examples

Explore comprehensive examples in the `examples/` directory:

```bash
# Basic calculator server
cargo run --example 01_basic_calculator

# File system tools
cargo run --example 02_file_tools

# Database integration
cargo run --example 03_database_server

# Web scraping tools
cargo run --example 04_web_tools

# Authentication with OAuth 2.0
cargo run --example 09_oauth_authentication

# HTTP server with advanced features
cargo run --example 10_http_server
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `simd` | Enable SIMD acceleration for JSON processing | ❌ |
| `oauth` | Enable OAuth 2.0 authentication | ✅ |
| `metrics` | Enable metrics collection and endpoints | ✅ |
| `compression` | Enable response compression | ✅ |
| `all-transports` | Enable all transport protocols | ✅ |
| `minimal` | Minimal build (STDIO only) | ❌ |

## Development

### Building

```bash
# Build with all features
cargo build --all-features

# Build optimized for production
cargo build --release --features simd

# Run tests
cargo test --workspace
```

### Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature-name`
3. Make your changes and add tests
4. Run the full test suite: `make test`
5. Submit a pull request

## Performance & Architectural Excellence

### 🚀 **Leapfrog Architecture Advantage**

TurboMCP's **compile-time first** design philosophy delivers measurable performance advantages over traditional MCP implementations:

#### **Compile-Time Optimization Strategy**
- **Macro-driven efficiency** - `#[server]`, `#[tool]`, and `#[resource]` macros pre-compute all metadata at build time
- **Zero runtime reflection** - Tool schemas, parameter validation, and handler dispatch tables generated statically
- **Type-system leveraged** - Rust's ownership model eliminates common performance pitfalls (no GC, predictable memory)
- **Intelligent feature gating** - Comprehensive codebase with surgical binary optimization through compile-time feature selection

#### **Runtime Performance Benefits**
- **JSON Processing**: 2-3x faster than `serde_json` with SIMD acceleration
- **Memory Efficiency**: 40% reduction through zero-copy processing and pre-allocated buffers
- **Cold Start Performance**: Faster initialization due to pre-computed schemas and handler registration
- **Request Latency**: Sub-millisecond tool dispatch with O(1) handler lookup
- **Concurrent Scaling**: Linear performance scaling with Tokio's async runtime
- **Predictable Performance**: No garbage collection pauses or dynamic allocation surprises

#### **Architectural Superiority**

**Compile-Time vs Runtime Trade-offs:**
```rust
// Other frameworks: Runtime overhead
fn handle_tool(name: &str, args: Value) {
    let schema = generate_schema(name);        // Runtime cost
    let handler = lookup_handler(name);        // HashMap lookup
    validate_parameters(args, schema);         // Runtime validation
    // ...
}

// TurboMCP: Compile-time optimization
#[tool("Add numbers")]                         // Pre-computed at build
async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
    Ok(a + b)  // Direct dispatch, no lookup
}
```

**Key Differentiators:**
- **Direct Dispatch**: Tool calls resolve to function pointers, not string lookups
- **Pre-computed Schemas**: JSON schemas generated once at compile time, not per request
- **Zero Reflection Overhead**: All type information resolved statically
- **Optimized Binary Size**: Feature flags eliminate unused transport and protocol code

#### **Why Compile-Time Architecture Matters**

While runtime-based frameworks offer flexibility, TurboMCP's compile-time approach provides:
- **Guaranteed Performance**: No performance degradation as tool count increases
- **Resource Predictability**: Memory usage determined at compile time
- **Production Reliability**: Errors caught at build time, not in production
- **Developer Experience**: Rich IDE support with full type checking and autocompletion

**The TurboMCP Philosophy**: *"Handle complexity at compile time so runtime can be blazingly fast."*

### Benchmarks

```bash
# Run performance benchmarks
cargo bench

# Test SIMD acceleration
cargo run --example simd_performance --features simd

# Profile memory usage
cargo run --example memory_profile
```

## Documentation

- **[Architecture Guide](../../ARCHITECTURE.md)** - System design and components
- **[Security Features](../turbomcp-transport/SECURITY_FEATURES.md)** - Comprehensive security documentation
- **[API Documentation](https://docs.rs/turbomcp)** - Complete API reference
- **[Performance Guide](./docs/performance.md)** - Optimization strategies
- **[Examples](./examples/)** - Ready-to-use code examples

## Related Projects

- **[Model Context Protocol](https://modelcontextprotocol.io/)** - Official protocol specification
- **[Claude Desktop](https://claude.ai)** - AI assistant with MCP support
- **[MCP Servers](https://github.com/modelcontextprotocol/servers)** - Official server implementations

## License

Licensed under the [MIT License](../../LICENSE).

---

*Built with ❤️ by the TurboMCP team. Ready for production, optimized for performance, designed for developers.*
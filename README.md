# TurboMCP

[![Crates.io](https://img.shields.io/crates/v/turbomcp.svg)](https://crates.io/crates/turbomcp)
[![Documentation](https://docs.rs/turbomcp/badge.svg)](https://docs.rs/turbomcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Tests](https://github.com/Epistates/turbomcp/actions/workflows/test.yml/badge.svg)](https://github.com/Epistates/turbomcp/actions/workflows/test.yml)

**High-performance Rust SDK for the Model Context Protocol (MCP)** with SIMD acceleration, enterprise security, and ergonomic APIs.

## Overview

TurboMCP is a production-ready Rust implementation of the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) featuring:

- **🚀 High Performance** - SIMD-accelerated JSON processing with `simd-json` and `sonic-rs`
- **🛡️ Enterprise Security** - OAuth 2.0, CORS, rate limiting, security headers, TLS support
- **⚡ Zero-Overhead Macros** - Ergonomic `#[server]`, `#[tool]`, `#[resource]` attributes  
- **🔗 Multi-Transport** - STDIO, HTTP/SSE, WebSocket, TCP, Unix sockets
- **🎯 Type Safety** - Compile-time validation with automatic schema generation
- **📁 Roots Support** - MCP-compliant filesystem boundaries with OS-aware defaults
- **🔄 Production Ready** - Circuit breakers, graceful shutdown, session management
- **🎭 Elicitation Support** - Server-initiated interactive user input (New in 1.0.3)
- **🤖 Sampling Protocol** - Bidirectional LLM sampling capabilities

## Quick Start

Add TurboMCP to your `Cargo.toml`:

```toml
[dependencies]
turbomcp = "1.0"
tokio = { version = "1.0", features = ["full"] }
serde_json = "1.0"
```

Create a simple calculator server:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server(
    name = "calculator-server",
    version = "1.0.0",
    description = "A simple calculator with filesystem access",
    root = "file:///workspace:Workspace",
    root = "file:///tmp:Temporary Files"
)]
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

## Client Setup

### Claude Desktop

Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

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

### Testing Your Server

```bash
# Test with CLI tool
cargo install turbomcp-cli

# For HTTP/WebSocket servers
turbomcp-cli tools-list --url http://localhost:8080/mcp

# For STDIO servers (like Claude Desktop)
turbomcp-cli tools-list --command "./your-server"

# Test directly
echo '{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}' | ./your-server
```

## Key Features

### Enterprise Security

Production-ready security features with environment-aware configurations:

```rust
use turbomcp_transport::{AxumMcpExt, McpServerConfig};

// Production security configuration  
let config = McpServerConfig::production()
    .with_cors_origins(vec!["https://app.example.com".to_string()])
    .with_custom_csp("default-src 'self'; connect-src 'self' wss:")
    .with_rate_limit(120, 20)  // 120 requests/minute, 20 burst
    .with_jwt_auth("your-secret-key".to_string());

let app = Router::new()
    .route("/api/status", get(status_handler))
    .merge(Router::<()>::turbo_mcp_routes_for_merge(mcp_service, config));
```

**Security Features:**
- 🔒 **CORS Protection** - Environment-aware cross-origin policies
- 📋 **Security Headers** - CSP, HSTS, X-Frame-Options, and more
- ⚡ **Rate Limiting** - Token bucket algorithm with flexible strategies
- 🔑 **Multi-Auth** - JWT validation and API key authentication
- 🔐 **TLS Support** - Automatic certificate loading with TLS 1.3

### OAuth 2.0 Authentication

Built-in OAuth 2.0 support with Google, GitHub, Microsoft providers:

```rust
use turbomcp::prelude::*;
use turbomcp::auth::*;

#[derive(Clone)]
pub struct AuthenticatedServer {
    oauth_providers: Arc<RwLock<HashMap<String, OAuth2Provider>>>,
}

#[server]
impl AuthenticatedServer {
    #[tool("Get authenticated user profile")]
    async fn get_user_profile(&self, ctx: Context) -> McpResult<String> {
        if let Some(user_id) = ctx.user_id() {
            Ok(format!("Authenticated user: {}", user_id))
        } else {
            Err(mcp_error!("Authentication required"))
        }
    }

    #[tool("Start OAuth flow")]
    async fn start_oauth_flow(&self, provider: String) -> McpResult<String> {
        let providers = self.oauth_providers.read().await;
        if let Some(oauth_provider) = providers.get(&provider) {
            let auth_result = oauth_provider.start_authorization().await?;
            Ok(format!("Visit: {}", auth_result.auth_url))
        } else {
            Err(mcp_error!("Unknown provider: {}", provider))
        }
    }
}
```

**OAuth Features:**
- 🔐 **Multiple Providers** - Google, GitHub, Microsoft, custom OAuth 2.0
- 🛡️ **Always-On PKCE** - Security enabled by default
- 🔄 **All OAuth Flows** - Authorization Code, Client Credentials, Device Code
- 👥 **Session Management** - User session tracking with cleanup

### Context Injection

Robust dependency injection with request correlation:

```rust
#[server]
impl ProductionServer {
    #[tool("Process with full observability")]
    async fn process_data(&self, ctx: Context, data: String) -> McpResult<String> {
        // Context provides:
        // - Request correlation and distributed tracing  
        // - Structured logging with metadata
        // - Performance monitoring and metrics
        // - Dependency injection container access
        
        ctx.info(&format!("Processing: {}", data)).await?;
        
        let start = std::time::Instant::now();
        let result = self.database.process(&data).await?;
        
        ctx.info(&format!("Completed in {:?}", start.elapsed())).await?;
        Ok(result)
    }
}
```

### Multi-Transport Support

Flexible transport protocols for different deployment scenarios:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = Calculator::new();
    
    match std::env::var("TRANSPORT").as_deref() {
        Ok("tcp") => server.run_tcp("127.0.0.1:8080").await?,
        Ok("unix") => server.run_unix("/tmp/mcp.sock").await?,
        _ => server.run_stdio().await?, // Default
    }
    Ok(())
}
```

### Graceful Shutdown

Production-ready shutdown handling:

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

## Architecture

TurboMCP's **modular, compile-time optimized architecture** separates concerns while maximizing performance:

### 🎯 **Design Philosophy: "Compile-Time Complexity, Runtime Simplicity"**
Our architecture prioritizes compile-time computation over runtime flexibility. While this creates a more sophisticated build process, it delivers unmatched runtime performance and predictability. Each crate is optimized for its specific role, with aggressive feature gating to ensure minimal production footprints.

### 📦 **Focused Crate Organization**

TurboMCP is organized into focused crates:

| Crate | Purpose | Key Features |
|-------|---------|--------------|
| [`turbomcp`](./crates/turbomcp/) | Main SDK | Procedural macros, prelude, integration |
| [`turbomcp-core`](./crates/turbomcp-core/) | Core types | SIMD message handling, sessions, errors |
| [`turbomcp-protocol`](./crates/turbomcp-protocol/) | MCP protocol | JSON-RPC, schema validation, versioning |
| [`turbomcp-transport`](./crates/turbomcp-transport/) | Transport layer | HTTP, WebSocket, circuit breakers |
| [`turbomcp-server`](./crates/turbomcp-server/) | Server framework | Routing, authentication, middleware |
| [`turbomcp-client`](./crates/turbomcp-client/) | Client library | Connection management, error recovery |
| [`turbomcp-macros`](./crates/turbomcp-macros/) | Proc macros | `#[server]`, `#[tool]`, `#[resource]` |
| [`turbomcp-cli`](./crates/turbomcp-cli/) | CLI tools | Testing, schema export, debugging |

## Advanced Usage

### Roots & Filesystem Boundaries (New in 1.0.3)

Configure filesystem roots for secure server operations:

```rust
use turbomcp::prelude::*;

// Method 1: Using macro attributes (recommended)
#[server(
    name = "filesystem-server",
    version = "1.0.0",
    root = "file:///workspace:Project Workspace",
    root = "file:///tmp:Temporary Files",
    root = "file:///Users/shared:Shared Documents"
)]
impl FileSystemServer {
    #[tool("List files in directory")]
    async fn list_files(&self, ctx: Context, path: String) -> McpResult<Vec<String>> {
        ctx.info(&format!("Listing files in: {}", path)).await?;
        // File operations are bounded by configured roots
        Ok(vec!["file1.txt".to_string(), "file2.txt".to_string()])
    }
}

// Method 2: Using ServerBuilder API
use turbomcp_server::ServerBuilder;
use turbomcp_protocol::types::Root;

let server = ServerBuilder::new()
    .name("filesystem-server")
    .version("1.0.0")
    .root("file:///workspace", Some("Workspace".to_string()))
    .roots(vec![
        Root { uri: "file:///tmp".to_string(), name: Some("Temp".to_string()) }
    ])
    .build();
```

**Roots Features:**
- 📁 **Multiple Configuration Methods** - Macro attributes, builder API, and runtime
- 🖥️ **OS-Aware Defaults** - Automatic platform-specific roots (Linux: `/`, macOS: `/`, `/Volumes`, Windows: drive letters)
- 🔒 **Security Foundation** - Establish filesystem operation boundaries
- 🔗 **MCP Compliance** - Full support for `roots/list` protocol method

### Elicitation (New in 1.0.3)

Server-initiated requests for interactive user input:

```rust
use turbomcp::prelude::*;
use turbomcp::elicitation_api::{ElicitationResult, string, boolean};

#[tool("Configure deployment")]
async fn deploy(&self, ctx: Context, project: String) -> McpResult<String> {
    // Request deployment configuration from user
    let config = elicit!("Configure deployment for {}", project)
        .field("environment", string()
            .enum_values(vec!["dev", "staging", "production"])
            .description("Target environment")
            .build())
        .field("auto_scale", boolean()
            .description("Enable auto-scaling")
            .build())
        .field("replicas", integer()
            .range(1.0, 10.0)
            .description("Number of replicas")
            .build())
        .require(vec!["environment"])
        .send(&ctx.request)
        .await?;
    
    match config {
        ElicitationResult::Accept(data) => {
            let env = data.get::<String>("environment")?;
            let replicas = data.get::<i64>("replicas").unwrap_or(1);
            Ok(format!("Deployed {} to {} with {} replicas", project, env, replicas))
        }
        _ => Err(mcp_error!("Deployment cancelled"))
    }
}
```

### Schema Generation

Automatic JSON schema generation with validation:

```rust
#[tool("Process user data")]
async fn process_user(
    &self,
    #[description("User's email address")]
    email: String,
    #[description("User's age in years")] 
    age: u8,
) -> McpResult<UserProfile> {
    // Schema automatically generated and validated
    Ok(UserProfile { email, age })
}
```

### Resource Handlers

URI template-based resource handling:

```rust
#[resource("file://{path}")]
async fn read_file(&self, path: String) -> McpResult<String> {
    tokio::fs::read_to_string(&path).await
        .map_err(|e| mcp_error!("Resource error: {}", e))
}
```

### Feature-Gated Transports

Optimize binary size by selecting only needed transports:

```toml
# Minimal STDIO-only server
turbomcp = { version = "1.0", default-features = false, features = ["minimal"] }

# Network deployment with TCP + Unix
turbomcp = { version = "1.0", default-features = false, features = ["network"] }

# All transports for maximum flexibility  
turbomcp = { version = "1.0", default-features = false, features = ["all-transports"] }
```

## CLI Tools

Install the CLI for development and testing:

```bash
cargo install turbomcp-cli
```

**Usage:**
```bash
# List available tools (HTTP)
turbomcp-cli tools-list --url http://localhost:8080/mcp

# List available tools (STDIO)
turbomcp-cli tools-list --command "./my-server"

# Call a tool with arguments
turbomcp-cli tools-call --url http://localhost:8080/mcp --name add --arguments '{"a": 5, "b": 3}'

# Export JSON schemas to file
turbomcp-cli schema-export --url http://localhost:8080/mcp --output schemas.json
```

## Performance & Architecture Advantages

TurboMCP's **leapfrog architecture** delivers superior performance through fundamental design choices:

### 🏗️ **Compile-Time Optimization Philosophy**
- **Zero-overhead abstractions** - All tool registration and schema generation happens at compile time
- **Macro-powered efficiency** - `#[server]` and `#[tool]` macros eliminate runtime reflection overhead
- **Type-driven performance** - Rust's type system enables aggressive optimizations impossible in dynamic languages
- **Smart feature gating** - While our codebase is comprehensive, compile-time feature selection ensures lean production binaries

### ⚡ **Runtime Performance**
- **JSON Processing** - 2-3x faster than `serde_json` with SIMD acceleration
- **Memory Efficiency** - Zero-copy message handling with `Bytes` eliminates unnecessary allocations
- **Concurrency** - Tokio-based async runtime with efficient task scheduling
- **Cold Start Speed** - Pre-computed schemas and handlers enable faster initialization
- **Reliability** - Circuit breakers and connection pooling with intelligent failover

### 🎯 **Architectural Superiority**
Our **compile-time first** approach means:
- **No runtime schema generation** - Schemas computed at compile time, not during requests
- **Direct handler dispatch** - O(1) tool lookup without HashMap traversals or string matching
- **Zero reflection** - All type information resolved statically
- **Predictable performance** - No garbage collection pauses or dynamic allocation surprises

**The TurboMCP advantage**: While some frameworks trade complexity for simplicity, we've engineered complexity away through compile-time optimization. Users get maximum performance with minimal cognitive overhead.

## Examples

Explore comprehensive examples in the [`crates/turbomcp/examples/`](./crates/turbomcp/examples/) directory:

- **[client_server_e2e.rs](./crates/turbomcp/examples/client_server_e2e.rs)** - Complete E2E client-server demonstration
- **[feature_roots_builder.rs](./crates/turbomcp/examples/feature_roots_builder.rs)** - Filesystem roots configuration
- **[elicitation_websocket_demo.rs](./crates/turbomcp/examples/elicitation_websocket_demo.rs)** - Interactive elicitation over WebSocket
- **[sampling_ai_code_assistant.rs](./crates/turbomcp/examples/sampling_ai_code_assistant.rs)** - AI assistant with sampling
- **[transport_http_sse.rs](./crates/turbomcp/examples/transport_http_sse.rs)** - HTTP Server-Sent Events transport

**Run any example:**
```bash
cargo run --example client_server_e2e
cargo run --example feature_roots_builder
```

## Development

**Setup:**
```bash
git clone https://github.com/Epistates/turbomcp.git
cd turbomcp
cargo build --workspace
```

**Testing:**
```bash
make test                    # Run comprehensive test suite
cargo test --workspace      # Run all tests
cargo test --all-features   # Test with all features
```

**Quality:**
```bash
cargo fmt --all                              # Format code
cargo clippy --workspace --all-targets       # Lint code  
cargo bench --workspace                      # Run benchmarks
```

## Documentation

- **[API Documentation](https://docs.rs/turbomcp)** - Complete API reference
- **[Security Guide](./crates/turbomcp-transport/SECURITY_FEATURES.md)** - Comprehensive security documentation  
- **[Architecture Guide](./ARCHITECTURE.md)** - System design and components
- **[MCP Specification](https://modelcontextprotocol.io)** - Official protocol docs

## Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feature-name`
3. Make your changes and add tests
4. Run the full test suite: `make test`
5. Submit a pull request

Please ensure all tests pass and follow the existing code style.

## License

Licensed under the [MIT License](./LICENSE).

---

## Related Projects

- **[Model Context Protocol](https://modelcontextprotocol.io)** - Official protocol specification
- **[Claude Desktop](https://claude.ai)** - AI assistant with MCP support  
- **[MCP Servers](https://github.com/modelcontextprotocol/servers)** - Official server implementations
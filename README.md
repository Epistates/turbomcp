# TurboMCP

[![Crates.io](https://img.shields.io/crates/v/turbomcp.svg)](https://crates.io/crates/turbomcp)
[![Documentation](https://docs.rs/turbomcp/badge.svg)](https://docs.rs/turbomcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Build Status](https://img.shields.io/github/workflow/status/Epistates/turbomcp/CI)](https://github.com/Epistates/turbomcp/actions)

**High-performance Rust SDK for the Model Context Protocol (MCP)** with SIMD acceleration, enterprise security, and ergonomic APIs.

## Overview

TurboMCP is a production-ready Rust implementation of the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) featuring:

- **🚀 High Performance** - SIMD-accelerated JSON processing with `simd-json` and `sonic-rs`
- **🛡️ Enterprise Security** - OAuth 2.0, DPoP, TLS 1.3, CORS, DoS protection, circuit breakers
- **⚡ Zero-Overhead Macros** - Ergonomic `#[server]`, `#[tool]`, `#[resource]` attributes  
- **🔗 Multi-Transport** - STDIO, HTTP/SSE, WebSocket, TCP, TLS, Unix sockets
- **🎯 Type Safety** - Compile-time validation with automatic schema generation
- **🔄 Production Ready** - Circuit breakers, graceful shutdown, session management

## Quick Start

Add TurboMCP to your `Cargo.toml`:

```toml
[dependencies]
turbomcp = "1.1.0-exp.2"
tokio = { version = "1.0", features = ["full"] }
serde_json = "1.0"
```

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
- ⚡ **Rate Limiting** - Token bucket algorithm with IP and DPoP key tracking
- 🛡️ **DoS Protection** - Automatic IP blocking based on suspicious activity
- 🔧 **Circuit Breakers** - Service protection against cascading failures  
- 🔑 **Multi-Auth** - JWT validation, API key, and DPoP authentication
- 🔐 **TLS Hardening** - rustls 0.23 with certificate pinning and mTLS

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
- 🔒 **DPoP Support** - RFC 9449 token binding for enhanced security
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
        Ok("tls") => server.run_tls("127.0.0.1:8443", "./cert.pem", "./key.pem").await?,
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

TurboMCP is organized into focused crates:

| Crate | Purpose | Key Features |
|-------|---------|--------------|
| [`turbomcp`](./crates/turbomcp/) | Main SDK | Procedural macros, prelude, integration |
| [`turbomcp-core`](./crates/turbomcp-core/) | Core types | SIMD message handling, sessions, errors |
| [`turbomcp-protocol`](./crates/turbomcp-protocol/) | MCP protocol | JSON-RPC, schema validation, versioning |
| [`turbomcp-transport`](./crates/turbomcp-transport/) | Transport layer | HTTP, WebSocket, TLS, TCP, circuit breakers |
| [`turbomcp-server`](./crates/turbomcp-server/) | Server framework | Routing, authentication, middleware |
| [`turbomcp-client`](./crates/turbomcp-client/) | Client library | Connection management, error recovery |
| [`turbomcp-macros`](./crates/turbomcp-macros/) | Proc macros | `#[server]`, `#[tool]`, `#[resource]` |
| [`turbomcp-cli`](./crates/turbomcp-cli/) | CLI tools | Testing, schema export, debugging |

## Advanced Usage

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
turbomcp = { version = "1.1.0-exp.2", default-features = false, features = ["minimal"] }

# Network deployment with TCP + Unix
turbomcp = { version = "1.1.0-exp.2", default-features = false, features = ["network"] }

# All transports for maximum flexibility  
turbomcp = { version = "1.1.0-exp.2", default-features = false, features = ["all-transports"] }
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

## Performance

- **JSON Processing** - 2-3x faster than `serde_json` with SIMD acceleration
- **Memory Efficiency** - Zero-copy message handling with `Bytes`
- **Concurrency** - Tokio-based async runtime with efficient task scheduling  
- **Reliability** - Circuit breakers and connection pooling

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
- **[TLS Security Guide](./crates/turbomcp-transport/TLS_SECURITY.md)** - Production TLS configuration and best practices
- **[Deployment Guide](./DEPLOYMENT.md)** - Production deployment strategies with TLS
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
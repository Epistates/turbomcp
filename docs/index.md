# TurboMCP Documentation

Welcome to **TurboMCP** – a production-ready Rust SDK for the Model Context Protocol (MCP) with zero-boilerplate development and progressive enhancement.

## What is TurboMCP?

TurboMCP enables you to build MCP servers with:

- **Zero Boilerplate** - Automatic schema generation and type-safe handlers
- **Progressive Enhancement** - Start simple with STDIO, add HTTP/OAuth/WebSocket as needed
- **Full Protocol Support** - Tools, resources, prompts, sampling, elicitation, and more
- **Type Safety** - Rust's type system prevents entire classes of bugs
- **Production Ready** - Graceful shutdown, observability, error handling built-in
- **Multiple Transports** - STDIO, HTTP/SSE, WebSocket, TCP, Unix sockets

## Quick Navigation

<div class="grid cards" markdown>

- **🚀 [Getting Started](getting-started/overview.md)**
  Learn the basics and create your first MCP server in minutes

- **📚 [Complete Guide](guide/architecture.md)**
  Deep dive into architecture, handlers, context injection, and authentication

- **🔧 [API Reference](api/protocol.md)**
  Comprehensive reference for all crates and their APIs

- **💡 [Examples](examples/basic.md)**
  Real-world patterns and advanced usage examples

- **🏗️ [Architecture](architecture/system-design.md)**
  System design, context lifecycle, and design decisions

- **🚢 [Deployment](deployment/docker.md)**
  Deploy to production with Docker, Kubernetes, and observability

</div>

## Key Features

### Zero-Boilerplate Development

Define handlers with simple Rust functions – the framework generates everything:

```rust
#[tool]
async fn get_weather(city: String) -> McpResult<String> {
    Ok(format!("Weather for {}", city))
}
```

### Automatic Schema Generation

JSON schemas are generated at compile time from your function signatures:

```rust
#[tool(description = "Get weather for a city")]
async fn get_weather(
    #[description = "City name"]
    city: String,
    #[description = "Units (C/F)"]
    units: Option<String>,
) -> McpResult<String> {
    Ok("Weather data".to_string())
}
```

### Dependency Injection

Request handlers automatically receive injected dependencies:

```rust
#[tool]
async fn process_data(
    config: Config,
    logger: Logger,
    cache: Cache,
    db: Database,
) -> McpResult<String> {
    logger.info("Processing data").await?;
    Ok("Result".to_string())
}
```

### Multiple Transports

Choose the right transport for your use case:

```rust
let server = McpServer::new()
    .stdio()           // Standard I/O transport
    .http(8080)        // HTTP with Server-Sent Events
    .websocket(8081)   // WebSocket support
    .tcp(9000)         // TCP networking
    .run()
    .await?;
```

### Context Injection System

Access request context, correlation IDs, and inject custom services:

```rust
#[tool]
async fn my_handler(
    ctx: InjectContext,
    info: RequestInfo,
    logger: Logger,
) -> McpResult<String> {
    logger.info(&format!("Request {}: {}",
        info.request_id,
        info.handler_name
    )).await?;
    Ok("Success".to_string())
}
```

## Core Crates

### Foundation
- **turbomcp-protocol** - Complete MCP 2025-06-18 implementation
- **turbomcp-transport** - Multi-protocol transport layer
- **turbomcp-macros** - Zero-overhead procedural macros

### Infrastructure
- **turbomcp-server** - Server framework with middleware
- **turbomcp-client** - Client implementation with auto-retry
- **turbomcp-auth** - OAuth 2.1 and authentication

### Developer API
- **turbomcp** - Main SDK combining all layers
- **turbomcp-cli** - CLI tools for testing and debugging
- **turbomcp-proxy** - Universal MCP adapter

## Getting Started

### Installation

Add TurboMCP to your `Cargo.toml`:

```toml
[dependencies]
turbomcp = "2.3"
tokio = { version = "1", features = ["full"] }
```

### Create Your First Server

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    let server = McpServer::new()
        .with_name("hello-world")
        .stdio()
        .run()
        .await?;

    Ok(())
}

#[tool]
async fn hello(name: String) -> McpResult<String> {
    Ok(format!("Hello, {}!", name))
}
```

### Run Your Server

```bash
cargo run
```

## Learn More

- **[Getting Started Guide](getting-started/overview.md)** - Complete introduction
- **[Architecture Overview](guide/architecture.md)** - How TurboMCP works
- **[API Documentation](api/protocol.md)** - Detailed API reference
- **[Examples](examples/basic.md)** - Real-world patterns

## Community & Support

- **GitHub Issues** - Report bugs and request features
- **GitHub Discussions** - Ask questions and share ideas
- **Documentation** - Comprehensive guides and API reference

## License

TurboMCP is licensed under the MIT License. See LICENSE for details.

---

**Ready to build your MCP server?** Start with the [Getting Started guide →](getting-started/overview.md)

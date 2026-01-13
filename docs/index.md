# TurboMCP Documentation

Welcome to **TurboMCP v3** – a production-ready Rust SDK for the Model Context Protocol (MCP) with zero-boilerplate development, modular architecture, and edge computing support.

## What's New in v3

TurboMCP 3.0 introduces a **modular architecture** with significant improvements:

- **`no_std` Core** - Run in embedded and WASM environments with `turbomcp-core`
- **Unified Errors** - Single `McpError` type across the entire SDK
- **Modular Transports** - Individual crates for each transport (STDIO, HTTP, WebSocket, TCP, Unix, gRPC)
- **Wire Codec Abstraction** - Pluggable serialization with JSON, SIMD-JSON, and MessagePack
- **Tower Integration** - Native Tower middleware for authentication and telemetry
- **WASM Support** - Full browser client and WASI Preview 2 runtime support
- **OpenTelemetry** - First-class distributed tracing and metrics
- **MCP 2025-11-25** - Full compliance with the latest MCP specification

## What is TurboMCP?

TurboMCP enables you to build MCP servers with:

- **Zero Boilerplate** - Automatic schema generation and type-safe handlers
- **Progressive Enhancement** - Start simple with STDIO, add HTTP/OAuth/WebSocket as needed
- **Full Protocol Support** - Tools, resources, prompts, sampling, elicitation, and more
- **Type Safety** - Rust's type system prevents entire classes of bugs
- **Production Ready** - Graceful shutdown, observability, error handling built-in
- **Multiple Transports** - STDIO, HTTP/SSE, WebSocket, TCP, Unix sockets, gRPC
- **Edge Ready** - Deploy to browsers, edge workers, and WASI runtimes

## Quick Navigation

<div class="grid cards" markdown>

- **[Getting Started](getting-started/overview.md)**
  Learn the basics and create your first MCP server in minutes

- **[Complete Guide](guide/architecture.md)**
  Deep dive into architecture, handlers, context injection, and authentication

- **[API Reference](api/protocol.md)**
  Comprehensive reference for all crates and their APIs

- **[Examples](examples/basic.md)**
  Real-world patterns and advanced usage examples

- **[Architecture](architecture/system-design.md)**
  System design, context lifecycle, and design decisions

- **[Deployment](deployment/docker.md)**
  Deploy to production with Docker, Kubernetes, edge, and observability

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

### Unified Error Handling (v3)

A single `McpError` type across the entire SDK with JSON-RPC code mapping:

```rust
use turbomcp::McpError;

fn my_handler() -> McpResult<String> {
    // Unified error type with rich context
    Err(McpError::tool_not_found("unknown_tool"))
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
    .grpc(50051)       // gRPC transport (v3)
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

### WASM & Edge Support (v3)

Run MCP clients in browsers and build servers on edge platforms:

=== "Browser Client (JavaScript)"
    ```javascript
    import init, { McpClient } from 'turbomcp-wasm';

    await init();
    const client = new McpClient("https://api.example.com/mcp");
    await client.initialize();

    const tools = await client.listTools();
    const result = await client.callTool("my_tool", { arg: "value" });
    ```

=== "Edge Server (Cloudflare Workers)"
    ```rust
    use turbomcp_wasm::prelude::*;

    #[derive(Clone)]
    struct MyServer;

    #[server(name = "edge-mcp", version = "1.0.0")]
    impl MyServer {
        #[tool("Say hello")]
        async fn hello(&self, args: HelloArgs) -> String {
            format!("Hello, {}!", args.name)
        }
    }

    #[event(fetch)]
    async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
        MyServer.into_mcp_server().handle(req).await
    }
    ```

## Core Crates

### Foundation Layer
- **turbomcp-core** - `no_std` core types for WASM and embedded (v3)
- **turbomcp-protocol** - Complete MCP 2025-11-25 implementation
- **turbomcp-wire** - Wire format codec abstraction (v3)

### Transport Layer
- **turbomcp-transport** - Multi-protocol transport orchestration
- **turbomcp-stdio** - Standard I/O transport (v3 modular)
- **turbomcp-http** - HTTP/SSE transport (v3 modular)
- **turbomcp-websocket** - WebSocket transport (v3 modular)
- **turbomcp-tcp** - TCP transport (v3 modular)
- **turbomcp-unix** - Unix socket transport (v3 modular)
- **turbomcp-grpc** - gRPC transport (v3)

### Infrastructure Layer
- **turbomcp-server** - Server framework with middleware
- **turbomcp-client** - Client implementation with auto-retry
- **turbomcp-auth** - OAuth 2.1 and authentication
- **turbomcp-telemetry** - OpenTelemetry integration (v3)

### Developer API Layer
- **turbomcp** - Main SDK combining all layers
- **turbomcp-macros** - Zero-overhead procedural macros
- **turbomcp-cli** - CLI tools for testing and debugging
- **turbomcp-wasm** - WebAssembly bindings (v3)
- **turbomcp-wasm-macros** - WASM server procedural macros (v3)

## Getting Started

### Installation

Add TurboMCP to your `Cargo.toml`:

```toml
[dependencies]
turbomcp = "3.0"
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
- **[Error Handling](guide/error-handling.md)** - Unified McpError system (v3)
- **[WASM & Edge](guide/wasm.md)** - Browser and edge deployment (v3)
- **[Tower Middleware](guide/tower-middleware.md)** - Composable middleware (v3)
- **[API Documentation](api/protocol.md)** - Detailed API reference
- **[Examples](examples/basic.md)** - Real-world patterns

## Migration from v2

See the **[v3 Migration Guide](architecture/v3-migration.md)** for:

- Unified `McpError` migration
- Modular transport architecture
- Feature flag simplification
- Tower middleware integration
- `no_std` core usage

## Community & Support

- **GitHub Issues** - Report bugs and request features
- **GitHub Discussions** - Ask questions and share ideas
- **Documentation** - Comprehensive guides and API reference

## License

TurboMCP is licensed under the MIT License. See LICENSE for details.

---

**Ready to build your MCP server?** Start with the [Getting Started guide](getting-started/overview.md)

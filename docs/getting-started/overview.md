# Overview

TurboMCP is a production-ready Rust SDK for building Model Context Protocol (MCP) servers. It provides:

- **Zero-boilerplate development** with automatic schema generation
- **Type-safe handlers** with compile-time validation
- **Multiple transports** (STDIO, HTTP/SSE, WebSocket, TCP, Unix sockets)
- **Full protocol support** including tools, resources, prompts, sampling, and elicitation
- **Dependency injection** for clean separation of concerns
- **Production features** like graceful shutdown, observability, and error handling

## What is MCP?

The Model Context Protocol (MCP) is a standard protocol that enables Claude and other AI models to safely interact with external systems. An MCP server exposes:

- **Tools** - Actions the model can perform
- **Resources** - Information the model can access
- **Prompts** - Pre-written instructions and templates
- **Sampling** - Bidirectional model interaction
- **Elicitation** - Prompting users for input

## Why TurboMCP?

### Traditional Approach ❌

Building MCP servers traditionally requires:

```rust
// Manual schema definition
let tool_schema = json!({
    "name": "get_weather",
    "description": "Get weather...",
    "inputSchema": {
        "type": "object",
        "properties": {
            "city": { "type": "string", "description": "..." }
        },
        "required": ["city"]
    }
});

// Manual request handling
match request.method {
    "tools/call" => {
        // Parse arguments manually
        // Validate types manually
        // Handle errors manually
    }
    // ... dozens more cases
}
```

### TurboMCP Approach ✅

With TurboMCP, you just write handlers:

```rust
#[tool]
async fn get_weather(city: String) -> McpResult<String> {
    Ok(format!("Weather for {}", city))
}
```

That's it! TurboMCP handles:
- ✅ Schema generation
- ✅ Request parsing
- ✅ Response serialization
- ✅ Error handling
- ✅ Type validation

## Architecture Layers

TurboMCP is organized in layers, so you can use what you need:

```
┌─────────────────────────────────────────┐
│  Application Layer                      │
│  (Your handlers with #[tool], etc)      │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Framework Layer (turbomcp-server)      │
│  (Handler registry, middleware, auth)   │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Transport Layer (turbomcp-transport)   │
│  (STDIO, HTTP, WebSocket, TCP, Unix)    │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Protocol Layer (turbomcp-protocol)     │
│  (MCP types, JSON-RPC, validation)      │
└─────────────────────────────────────────┘
```

Each layer is independent and can be used separately or together.

## Key Concepts

### Handlers

Handlers are async functions decorated with macros that define what your server can do:

```rust
#[tool]
async fn my_tool(param: String) -> McpResult<String> {
    Ok("result".to_string())
}

#[resource]
async fn my_resource() -> McpResult<String> {
    Ok("resource content".to_string())
}

#[prompt]
async fn my_prompt() -> McpResult<String> {
    Ok("prompt content".to_string())
}
```

### Context Injection

Handlers can request injected dependencies that are available throughout the request:

```rust
#[tool]
async fn my_handler(
    config: Config,      // Application configuration
    logger: Logger,      // Structured logging
    cache: Cache,        // In-memory cache
    db: Database,        // Database connection
) -> McpResult<String> {
    // Use injected dependencies
    Ok("result".to_string())
}
```

### Multiple Transports

Add transports as needed – start with STDIO, add HTTP/OAuth/WebSocket later:

```rust
let server = McpServer::new()
    .stdio()              // Standard I/O
    .http(8080)           // HTTP + Server-Sent Events
    .websocket(8081)      // WebSocket support
    .tcp(9000)            // TCP networking
    .run()
    .await?;
```

### Observability

Built-in logging, tracing, and metrics:

```rust
#[tool]
async fn my_tool(logger: Logger) -> McpResult<String> {
    logger.info("Starting").await?;
    logger.warn("Cache miss").await?;
    logger.error("Failed to connect").await?;
    Ok("result".to_string())
}
```

## Next Steps

- **[Installation](installation.md)** - Set up TurboMCP
- **[Quick Start](quick-start.md)** - 5-minute tutorial
- **[Your First Server](first-server.md)** - Build a real example
- **[Architecture Guide](../guide/architecture.md)** - Deep dive into design

## Examples Repository

See the [examples/](https://github.com/turbomcp/turbomcp/tree/main/crates/turbomcp/examples) directory for:

- `hello_world.rs` - Minimal example
- `macro_server.rs` - Using macros
- `stateful.rs` - Maintaining state
- `sampling_server.rs` - Bidirectional communication
- `http_app.rs` - HTTP transport
- And 20+ more real-world patterns

## Additional Resources

- **[Complete Guide](../guide/architecture.md)** - Comprehensive tutorials
- **[API Reference](../api/protocol.md)** - Full API documentation
- **[Architecture Deep Dives](../architecture/system-design.md)** - Design decisions
- **[Deployment Guide](../deployment/docker.md)** - Production setup

---

Ready to get started? → [Installation](installation.md)

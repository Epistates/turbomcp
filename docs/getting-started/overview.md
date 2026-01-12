# Overview

TurboMCP is a production-ready Rust SDK for building Model Context Protocol (MCP) servers. Version 3 introduces a modular architecture with significant improvements.

## What is TurboMCP?

TurboMCP provides:

- **Zero-boilerplate development** with automatic schema generation
- **Type-safe handlers** with compile-time validation
- **Multiple transports** (STDIO, HTTP/SSE, WebSocket, TCP, Unix sockets, gRPC)
- **Full protocol support** including tools, resources, prompts, sampling, and elicitation
- **Dependency injection** for clean separation of concerns
- **Production features** like graceful shutdown, observability, and error handling
- **Edge computing support** with WASM and WASI (v3)

## What's New in v3

TurboMCP 3.0 represents a major modular architecture redesign:

| Feature | Description |
|---------|-------------|
| **Unified Errors** | Single `McpError` type replaces `ServerError`, `ClientError` |
| **Modular Transports** | Individual crates for each transport |
| **`no_std` Core** | `turbomcp-core` works in WASM and embedded |
| **Wire Codecs** | Pluggable JSON, SIMD-JSON, MessagePack |
| **Tower Integration** | Native Tower middleware for auth and telemetry |
| **WASM Support** | Browser clients and WASI Preview 2 |
| **gRPC Transport** | High-performance gRPC via tonic |
| **OpenTelemetry** | First-class distributed tracing and metrics |
| **MCP 2025-11-25** | Full spec compliance |

## What is MCP?

The Model Context Protocol (MCP) is a standard protocol that enables Claude and other AI models to safely interact with external systems. An MCP server exposes:

- **Tools** - Actions the model can perform
- **Resources** - Information the model can access
- **Prompts** - Pre-written instructions and templates
- **Sampling** - Bidirectional model interaction
- **Elicitation** - Prompting users for input

## Why TurboMCP?

### Traditional Approach

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

### TurboMCP Approach

With TurboMCP, you just write handlers:

```rust
#[tool]
async fn get_weather(city: String) -> McpResult<String> {
    Ok(format!("Weather for {}", city))
}
```

That's it! TurboMCP handles:

- Schema generation
- Request parsing
- Response serialization
- Error handling
- Type validation

## Architecture Layers

TurboMCP v3 is organized in modular layers:

```
┌─────────────────────────────────────────┐
│  Application Layer                      │
│  (Your handlers with #[tool], etc)      │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Developer API (turbomcp)               │
│  (Macros, prelude, configuration)       │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Infrastructure Layer                   │
│  (turbomcp-server, turbomcp-client)     │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Transport Layer (v3 modular)           │
│  turbomcp-stdio  │ turbomcp-http        │
│  turbomcp-websocket │ turbomcp-tcp      │
│  turbomcp-unix │ turbomcp-grpc          │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Wire Layer (turbomcp-wire)             │
│  (JSON, SIMD-JSON, MessagePack codecs)  │
└─────────────────────────────────────────┘
            ↓
┌─────────────────────────────────────────┐
│  Foundation Layer                       │
│  turbomcp-core (no_std)                 │
│  turbomcp-protocol                      │
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

### Unified Error Handling (v3)

All error types unified into `McpError`:

```rust
use turbomcp::{McpError, McpResult};

#[tool]
async fn handler(input: String) -> McpResult<String> {
    if input.is_empty() {
        return Err(McpError::invalid_params("Input required"));
    }
    Ok(input)
}
```

### Context Injection

Handlers can request injected dependencies:

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
    .grpc(50051)          // gRPC (v3)
    .run()
    .await?;
```

### Tower Middleware (v3)

Compose middleware using Tower:

```rust
use tower::ServiceBuilder;
use turbomcp_auth::tower::AuthLayer;
use turbomcp_telemetry::tower::TelemetryLayer;

let service = ServiceBuilder::new()
    .layer(TelemetryLayer::new(config))
    .layer(AuthLayer::new(auth_config))
    .service(my_handler);
```

### WASM Support (v3)

Run MCP clients in browsers:

```javascript
import init, { McpClient } from 'turbomcp-wasm';

await init();
const client = new McpClient("https://api.example.com/mcp");
await client.initialize();

const tools = await client.listTools();
```

## Next Steps

- **[Installation](installation.md)** - Set up TurboMCP
- **[Quick Start](quick-start.md)** - 5-minute tutorial
- **[Your First Server](first-server.md)** - Build a real example
- **[Architecture Guide](../guide/architecture.md)** - Deep dive into design
- **[Error Handling](../guide/error-handling.md)** - Unified McpError (v3)
- **[WASM & Edge](../guide/wasm.md)** - Browser and edge (v3)

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

Ready to get started? [Installation](installation.md)

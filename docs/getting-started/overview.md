# Overview

TurboMCP is a production-ready Rust SDK for building Model Context Protocol (MCP) servers. Version 3 introduces a modular architecture with significant improvements.

## What is TurboMCP?

TurboMCP provides:

- **Zero-boilerplate development** with automatic schema generation
- **Type-safe handlers** with compile-time validation
- **Multiple transports** (STDIO, HTTP/SSE, WebSocket, TCP, Unix sockets, gRPC)
- **Full protocol support** including tools, resources, prompts, sampling, and elicitation
- **Request context** for per-request metadata and server-to-client calls
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

Building MCP servers traditionally requires code like this (a sketch, not a
TurboMCP API):

```rust,ignore
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
use turbomcp::prelude::*;

#[derive(Clone)]
struct Weather;

#[server(name = "weather", version = "1.0.0")]
impl Weather {
    /// Get the weather for a city
    #[tool]
    async fn get_weather(&self, city: String) -> McpResult<String> {
        Ok(format!("Weather for {}", city))
    }
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

Handlers are async methods in a `#[server]` impl block, marked with the kind of
capability they provide:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// A tool the model can call
    #[tool]
    async fn my_tool(&self, param: String) -> McpResult<String> {
        Ok("result".to_string())
    }

    /// A resource the client can read, by URI
    #[resource("app://status")]
    async fn my_resource(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        Ok("resource content".to_string())
    }

    /// A prompt template the user can pick
    #[prompt]
    async fn my_prompt(&self, ctx: &RequestContext) -> McpResult<String> {
        Ok("prompt content".to_string())
    }
}
```

### Unified Error Handling (v3)

All error types unified into `McpError`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn handler(&self, input: String) -> McpResult<String> {
        if input.is_empty() {
            return Err(McpError::invalid_params("Input required"));
        }
        Ok(input)
    }
}
```

### State and Context

Handlers are methods, so application state lives on the server type (behind an
`Arc` so the type stays cheap to clone). Per-request information comes from an
optional `ctx: &RequestContext` parameter:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone, Default)]
struct MyServer {
    cache: Arc<RwLock<HashMap<String, String>>>,
}

#[server]
impl MyServer {
    #[tool]
    async fn my_handler(&self, key: String, ctx: &RequestContext) -> McpResult<String> {
        let cached = self.cache.read().await.get(&key).cloned();
        Ok(format!("{key} = {cached:?} (request {})", ctx.request_id()))
    }
}
```

### Multiple Transports

Add transports as needed – start with STDIO, add HTTP/WebSocket later. Each
`run_*` method needs its transport's Cargo feature; gRPC lives in the separate
`turbomcp-grpc` crate.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn ping(&self) -> String {
        "pong".to_string()
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    match std::env::var("TRANSPORT").as_deref() {
        Ok("http") => MyServer.run_http("0.0.0.0:8080").await,      // Streamable HTTP
        Ok("ws") => MyServer.run_websocket("0.0.0.0:8081").await,   // WebSocket
        Ok("tcp") => MyServer.run_tcp("0.0.0.0:9000").await,        // TCP
        _ => MyServer.run_stdio().await,                            // Standard I/O
    }
}
```

### Middleware (v3)

Wrap any handler in typed MCP middleware; the stack is itself a handler, so it
runs on any transport:

```rust
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use turbomcp::prelude::*;
use turbomcp_server::{McpMiddleware, MiddlewareStack, Next};

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn ping(&self) -> String {
        "pong".to_string()
    }
}

struct Audit;

impl McpMiddleware for Audit {
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            eprintln!("tool {name} called");
            next.call_tool(name, args, ctx).await
        })
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    MiddlewareStack::new(MyServer).with_middleware(Audit).run_stdio().await
}
```

Tower layers for authentication and telemetry are covered in the
[Tower Middleware guide](../guide/tower-middleware.md).

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

See the [examples/](https://github.com/Epistates/turbomcp/tree/main/crates/turbomcp/examples) directory for:

- `hello_world.rs` - Minimal example
- `macro_server.rs` - Using macros
- `stateful.rs` - Maintaining state
- `transports_demo.rs` - Choosing a transport
- `middleware.rs` - Typed middleware
- `test_client.rs` - In-memory testing with `McpTestClient`
- And 10 more patterns

## Additional Resources

- **[Complete Guide](../guide/architecture.md)** - Comprehensive tutorials
- **[API Reference](../api/protocol.md)** - Full API documentation
- **[Architecture Deep Dives](../architecture/system-design.md)** - Design decisions
- **[Deployment Guide](../deployment/docker.md)** - Production setup

---

Ready to get started? [Installation](installation.md)

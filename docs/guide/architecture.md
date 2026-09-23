# Architecture

TurboMCP v3 follows a layered, modular architecture designed for flexibility, performance, and edge computing support.

## Architectural Layers

### Layer 1: Foundation (`turbomcp-core`)

The `no_std` compatible foundation layer provides core types that work everywhere:

```rust
// Works in WASM, embedded, and standard environments
use turbomcp_core::{McpHandler, Prompt, RequestContext, Resource, Tool};
use turbomcp_core::error::{McpError, McpResult};
```

**Provides:**
- Core MCP types (Tool, Resource, Prompt, Content), re-exported from `turbomcp-types`
- Unified `McpError` type
- The `McpHandler` trait and `RequestContext`
- JSON-RPC types

### Layer 2: Wire Format (`turbomcp-wire`)

Pluggable serialization for protocol messages:

```rust
use turbomcp_wire::{Codec, JsonCodec};

let codec = JsonCodec::new();
let message = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
let bytes = codec.encode(&message)?;
let decoded: serde_json::Value = codec.decode(&bytes)?;
```

**Provides:**
- JSON codec (always available)
- SIMD-accelerated JSON (`SimdJsonCodec`, feature `simd`)
- MessagePack binary format (`MsgPackCodec`, feature `msgpack`)
- Streaming decoder for SSE

The server and client do not go through this crate: they use `serde_json`
directly. See [Wire Codecs](wire-codecs.md).

### Layer 3: Protocol (`turbomcp-protocol`)

Complete MCP 2025-11-25 specification implementation:

```rust
use turbomcp_protocol::types::{CallToolRequest, InitializeRequest, Tool};
use turbomcp_protocol::{JsonRpcRequest, JsonRpcResponse};
```

**Provides:**
- JSON-RPC 2.0 handling
- MCP message types
- Schema validation
- Request/response correlation

### Layer 4: Transport (Modular Crates)

Individual transport crates for each protocol:

| Crate | Transport | Use Case |
|-------|-----------|----------|
| `turbomcp-stdio` | STDIO | CLI, Claude desktop |
| `turbomcp-http` | HTTP/SSE | Web applications |
| `turbomcp-websocket` | WebSocket | Real-time bidirectional |
| `turbomcp-tcp` | TCP | High performance |
| `turbomcp-unix` | Unix sockets | Local IPC |
| `turbomcp-grpc` | gRPC | Enterprise, microservices (standalone; not wired into `turbomcp-server`) |

### Layer 5: Infrastructure

Server and client implementations:

```rust
// Server: builder, run_* entry points, middleware, visibility, composition
use turbomcp_server::{McpHandlerExt, McpServerExt, ServerBuilder, ServerConfig};

// Client
use turbomcp_client::{Client, ClientBuilder};
```

**Provides:**
- Handler registration and routing
- Middleware pipeline
- Connection management
- Graceful shutdown

### Layer 6: Developer API (`turbomcp`)

The main SDK combining all layers:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Echo the input back.
    #[tool]
    async fn my_tool(&self, input: String) -> McpResult<String> {
        Ok(input)
    }
}
```

## v3 Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                         │
│              (Your handlers with #[tool], etc)               │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                   turbomcp (Developer API)                   │
│         Macros, Prelude, Configuration, Type-State           │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│              Infrastructure Layer (Tower-native)             │
├─────────────────────────────┬───────────────────────────────┤
│     turbomcp-server         │       turbomcp-client          │
│  • Handler registry         │  • Connection management       │
│  • Middleware stack         │  • Auto-retry                  │
│  • Request routing          │  • Capability negotiation      │
│  • Graceful shutdown        │  • LLM integration             │
└─────────────────────────────┴───────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                   Transport Layer (v3 Modular)               │
├──────────┬──────────┬───────────┬──────────┬────────┬───────┤
│ stdio    │ http     │ websocket │ tcp      │ unix   │channel│
│ (default)│ (+SSE)   │           │          │        │(tests)│
└──────────┴──────────┴───────────┴──────────┴────────┴───────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                      Wire Layer                              │
│                    (turbomcp-wire)                           │
│        JSON │ SIMD-JSON │ MessagePack │ Streaming            │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│                   Foundation Layer                           │
├─────────────────────────────┬───────────────────────────────┤
│     turbomcp-core           │     turbomcp-protocol          │
│     (no_std)                │     (async runtime)            │
│  • Core types               │  • MCP 2025-11-25 spec         │
│  • McpError                 │  • JSON-RPC 2.0                │
│  • JSON-RPC types           │  • Session management          │
└─────────────────────────────┴───────────────────────────────┘
```

## Design Patterns

### Builders

A handler runs directly on one transport, or through `ServerBuilder` when it
needs configuration. The transport is chosen at runtime:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    MyServer
        .builder()
        .transport(Transport::http("0.0.0.0:8080")) // or stdio/websocket/tcp/unix
        .with_rate_limit(100, Duration::from_secs(1))
        .serve()
        .await?;
    Ok(())
}
```

The capability builders in `turbomcp_protocol::capabilities::builders` use
type-state, so a sub-capability such as `enable_tool_list_changed()` only exists
once its parent capability is enabled.

### Unified Error Type (v3)

All errors use `McpError` with semantic constructors:

```rust
use turbomcp::{McpError, McpResult};

fn my_handler(input: &str) -> McpResult<String> {
    match input {
        "" => Err(McpError::invalid_params("Missing field")),
        "calculator" => Err(McpError::tool_not_found("calculator")),
        "db" => Err(McpError::internal("Database error")),
        other => Ok(other.to_string()),
    }
}
```

### Shared State and Context

There is no parameter injection beyond the request context: a handler reads
shared state from `&self` (the server struct, `Clone`, with state behind `Arc`)
and per-request data from `ctx: &RequestContext`:

```rust
use std::sync::Arc;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Config {
    greeting: String,
}

#[derive(Clone)]
struct Greeter {
    config: Arc<Config>,
}

#[server]
impl Greeter {
    #[tool]
    async fn handler(&self, name: String, ctx: &RequestContext) -> McpResult<String> {
        Ok(format!("{} {name} (request {})", self.config.greeting, ctx.request_id()))
    }
}
```

### Middleware (v3)

Server middleware is typed around MCP operations: implement `McpMiddleware` and
wrap a handler in `MiddlewareStack`, which is itself an `McpHandler`. The HTTP
transport is an Axum router, so HTTP-level Tower layers apply to
`into_axum_router()`. See [Tower Middleware](tower-middleware.md).

### Zero-Copy Message Processing

Transports carry message payloads as `Bytes`, so a message moves through the
transport layers without being copied:

```
Request -> Transport -> Protocol -> Handler
```

### Arc-Cloning for Resource Sharing

`McpHandler` requires `Clone`, and every transport clones the handler per
connection or request. Keep state behind `Arc` so a clone is a reference-count
increment:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Default)]
struct AppState {
    items: Arc<RwLock<Vec<String>>>,
}

let state = AppState::default();
let clone = state.clone(); // Cheap clone, shared data
```

## Request Flow

```
Client Request
    ↓
Transport Layer (decode via wire codec)
    ↓
Protocol Layer (parse JSON-RPC)
    ↓
Authorization (HTTP, when configured) and Middleware Stack (logging, metrics)
    ↓
Framework Layer (route to handler)
    ↓
Context Injection (create RequestContext)
    ↓
Handler Execution (your code)
    ↓
Response Serialization
    ↓
Middleware Stack (response processing)
    ↓
Transport Layer (encode via wire codec)
    ↓
Client Response
```

## Data Flow Architecture

```
┌──────────────────┐
│  Handler State   │
└────────┬─────────┘
         │
┌────────▼──────────────────┐
│  Context (Request-scoped) │
├──────────────────────────┤
│ • Request metadata       │
│ • Session handle         │
│ • Request ID             │
│ • Principal (auth info)  │
└────────┬──────────────────┘
         │
┌────────▼──────────────────┐
│  Server State (Shared)    │
├──────────────────────────┤
│ • Configuration          │
│ • Database connections   │
│ • Caches                 │
│ • Telemetry              │
└──────────────────────────┘
```

## Crate Dependency Graph

```
turbomcp
├── turbomcp-server
│   ├── turbomcp-protocol
│   │   └── turbomcp-core
│   └── turbomcp-transport (per-transport features)
│       └── turbomcp-{stdio,http,websocket,tcp,unix}
├── turbomcp-client (optional)
│   └── turbomcp-protocol
├── turbomcp-macros
├── turbomcp-auth (optional)
├── turbomcp-dpop (optional)
└── turbomcp-telemetry (optional)

Standalone: turbomcp-wire, turbomcp-grpc, turbomcp-wasm, turbomcp-openapi, turbomcp-proxy
```

## Features by Layer

### Foundation Layer
- Core type definitions
- Error handling
- JSON-RPC primitives

### Wire Layer
- Serialization abstraction
- Codec selection
- Streaming support

### Transport Layer
- Protocol encoding/decoding
- Connection management
- Reliability (retries, timeouts)
- Security (TLS)

### Infrastructure Layer
- Routing
- Middleware
- Context creation
- Authentication
- Lifecycle management

### Developer API Layer
- Handler definition
- Type-safe parameters
- Error handling
- Macros

## Performance Characteristics

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| Handler registration | O(1) | Done at startup |
| Request routing | O(1) | Generated `match` on the method and tool name |
| Context creation | O(1) | One `RequestContext` per request |
| Schema generation | O(1) | Compile-time |
| Message serialization | O(n) | Linear in message size |
| SIMD JSON parsing | O(n) | 2-4x faster than standard |

## Thread Safety

All components are thread-safe by default:

- `Arc` for shared ownership
- `RwLock` for mutable state
- `Channel` for async communication
- Tokio runtime for concurrency

## Extension Points

TurboMCP is designed for extension:

1. **Custom Handlers** - Use `#[server]`, or implement `McpHandler` by hand
2. **Custom Middleware** - Implement `McpMiddleware`, or a Tower `Layer` on the HTTP router
3. **Custom Transports** - Implement the `Transport` trait
4. **Custom Codecs** - Implement the `Codec` trait
5. **Composition** - Mount several handlers with `CompositeHandler`
6. **Custom Errors** - Use `McpError` constructors and `ErrorKind`

## Next Steps

- **[Handlers Guide](handlers.md)** - Different handler types
- **[Context & DI](context-injection.md)** - Dependency injection details
- **[Transports Guide](transports.md)** - Transport configuration
- **[Error Handling](error-handling.md)** - Unified McpError (v3)
- **[Tower Middleware](tower-middleware.md)** - Middleware patterns (v3)
- **[Advanced Patterns](advanced-patterns.md)** - Complex use cases

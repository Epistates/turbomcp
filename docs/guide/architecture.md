# Architecture

TurboMCP v3 follows a layered, modular architecture designed for flexibility, performance, and edge computing support.

## Architectural Layers

### Layer 1: Foundation (`turbomcp-core`)

The `no_std` compatible foundation layer provides core types that work everywhere:

```rust
// Works in WASM, embedded, and standard environments
use turbomcp_core::types::{Tool, Resource, Prompt};
use turbomcp_core::error::{McpError, McpResult};
```

**Provides:**
- Core MCP types (Tool, Resource, Prompt, Content)
- Unified `McpError` type
- JSON-RPC types
- Capabilities types

### Layer 2: Wire Format (`turbomcp-wire`)

Pluggable serialization for protocol messages:

```rust
use turbomcp_wire::{Codec, JsonCodec, SimdJsonCodec};

let codec = SimdJsonCodec::new();  // 2-4x faster JSON
let bytes = codec.encode(&message)?;
```

**Provides:**
- JSON codec (default)
- SIMD-accelerated JSON
- MessagePack binary format
- Streaming decoder for SSE

### Layer 3: Protocol (`turbomcp-protocol`)

Complete MCP 2025-11-25 specification implementation:

```rust
use turbomcp_protocol::*;
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
| `turbomcp-grpc` | gRPC | Enterprise, microservices |

### Layer 5: Infrastructure

Server and client implementations:

```rust
// Server
use turbomcp_server::McpServer;

// Client
use turbomcp_client::McpClient;
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

#[server]
struct MyServer;

#[tool]
async fn my_tool(input: String) -> McpResult<String> {
    Ok(input)
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
│ stdio    │ http     │ websocket │ tcp      │ unix   │ grpc  │
│ (default)│ (+SSE)   │           │          │        │       │
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

### Type-State Pattern for Builders

Configuration uses type-state to enforce correctness at compile time:

```rust
let server = McpServer::new()    // Returns configured state
    .stdio()                      // Adds STDIO, changes state
    .http(8080)                   // Adds HTTP, changes state
    .run()                        // All required config done
    .await?;                      // Run server
```

### Unified Error Type (v3)

All errors use `McpError` with semantic constructors:

```rust
use turbomcp::{McpError, McpResult};

fn my_handler() -> McpResult<String> {
    Err(McpError::tool_not_found("calculator"))
    // or
    Err(McpError::invalid_params("Missing field"))
    // or
    Err(McpError::internal("Database error"))
}
```

### Dependency Injection

Handlers request dependencies automatically:

```rust
#[tool]
async fn handler(
    config: Config,    // Injected
    logger: Logger,    // Injected
    cache: Cache,      // Injected
) -> McpResult<String> {
    Ok("result".into())
}
```

### Tower Middleware (v3)

Composable middleware using Tower:

```rust
use tower::ServiceBuilder;

let service = ServiceBuilder::new()
    .layer(TelemetryLayer::new(config))
    .layer(AuthLayer::new(auth_config))
    .layer(TimeoutLayer::new(Duration::from_secs(30)))
    .service(handler);
```

### Zero-Copy Message Processing

Uses `Bytes` type for efficient message handling:

```rust
// No copying, just references through layers
Request -> Transport -> Protocol -> Handler
```

### Arc-Cloning for Resource Sharing

Services are shared via Arc for cheap thread-safe cloning:

```rust
let server = Arc::new(McpServer::new());
let clone = server.clone();  // Cheap clone, shared data
```

## Request Flow

```
Client Request
    ↓
Transport Layer (decode via wire codec)
    ↓
Protocol Layer (parse JSON-RPC)
    ↓
Middleware Stack (auth, logging, metrics)
    ↓
Framework Layer (route to handler)
    ↓
Context Injection (create context)
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
│ • Injected services      │
│ • Correlation ID         │
│ • User/auth info         │
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
│   ├── turbomcp-transport (optional orchestration)
│   └── turbomcp-{stdio,http,websocket,tcp,unix,grpc}
├── turbomcp-client
│   └── turbomcp-protocol
├── turbomcp-macros
├── turbomcp-auth (optional)
├── turbomcp-dpop (optional)
├── turbomcp-telemetry (optional)
└── turbomcp-wire
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
| Request routing | O(1) | HashMap lookup |
| Context creation | O(1) | Pool reuse when available |
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

1. **Custom Handlers** - Any async function can be a handler
2. **Custom Middleware** - Implement Tower `Layer` trait
3. **Custom Transports** - Implement `Transport` trait
4. **Custom Codecs** - Implement `Codec` trait
5. **Custom Injectables** - Implement `Injectable` trait
6. **Custom Errors** - Use `McpError` variants

## Next Steps

- **[Handlers Guide](handlers.md)** - Different handler types
- **[Context & DI](context-injection.md)** - Dependency injection details
- **[Transports Guide](transports.md)** - Transport configuration
- **[Error Handling](error-handling.md)** - Unified McpError (v3)
- **[Tower Middleware](tower-middleware.md)** - Middleware patterns (v3)
- **[Advanced Patterns](advanced-patterns.md)** - Complex use cases

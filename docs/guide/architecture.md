# Architecture

TurboMCP follows a layered architecture design that allows you to use what you need and extend what you don't.

## Architectural Layers

### Layer 1: Application Layer (Your Code)

You define handlers using macros:

```rust
#[tool]
async fn my_tool(param: String) -> McpResult<String> {
    Ok(result)
}

#[resource]
async fn my_resource() -> McpResult<String> {
    Ok(content)
}

#[prompt]
async fn my_prompt() -> McpResult<String> {
    Ok(template)
}
```

### Layer 2: Framework Layer (turbomcp-server)

The server framework handles:
- Handler registration and routing
- Request context creation
- Middleware pipeline (auth, CORS, logging)
- Graceful shutdown
- Observability and metrics

### Layer 3: Transport Layer (turbomcp-transport)

Multiple transport implementations:
- **STDIO** - Standard input/output for CLI tools
- **HTTP** - REST with Server-Sent Events (SSE)
- **WebSocket** - Bidirectional communication
- **TCP** - Raw TCP networking
- **Unix Sockets** - Local IPC

### Layer 4: Protocol Layer (turbomcp-protocol)

Complete MCP specification implementation:
- JSON-RPC 2.0 handling
- Type definitions
- Schema validation
- Message serialization

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
Transport Layer (decode)
    ↓
Protocol Layer (parse JSON-RPC)
    ↓
Framework Layer (route to handler)
    ↓
Context Injection (create context)
    ↓
Handler Execution (your code)
    ↓
Response Serialization
    ↓
Transport Layer (encode)
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

## Features by Layer

### Application Layer
- Handler definition
- Type-safe parameters
- Error handling

### Framework Layer
- Routing
- Middleware
- Context creation
- Authentication
- Lifecycle management

### Transport Layer
- Protocol encoding/decoding
- Connection management
- Reliability (retries, timeouts)
- Security (TLS)

### Protocol Layer
- JSON-RPC 2.0
- MCP types
- Validation
- Schema generation

## Composition Over Inheritance

TurboMCP uses composition with traits:

```rust
// Compose transports
McpServer::new()
    .stdio()          // Trait: Transport
    .http(8080)       // Trait: Transport
    .websocket(8081)  // Trait: Transport

// Compose middleware
McpServer::new()
    .with_auth(oauth_config)      // Trait: Middleware
    .with_cors(cors_config)       // Trait: Middleware
    .with_logging(log_config)     // Trait: Middleware
```

## Extension Points

TurboMCP is designed for extension:

1. **Custom Handlers** - Any async function can be a handler
2. **Custom Middleware** - Implement `Middleware` trait
3. **Custom Transports** - Implement `Transport` trait
4. **Custom Injectables** - Implement `Injectable` trait
5. **Custom Errors** - Use `McpError` variants

## Performance Characteristics

| Operation | Complexity | Notes |
|-----------|-----------|-------|
| Handler registration | O(1) | Done at startup |
| Request routing | O(1) | HashMap lookup |
| Context creation | O(1) | Pool reuse when available |
| Schema generation | O(1) | Compile-time |
| Message serialization | O(n) | Linear in message size |
| JSON validation | O(n) | Uses `jsonschema` crate |

## Thread Safety

All components are thread-safe by default:

- `Arc` for shared ownership
- `RwLock` for mutable state
- `Channel` for async communication
- Tokio runtime for concurrency

## Next Steps

- **[Handlers Guide](handlers.md)** - Different handler types
- **[Context & DI](context-injection.md)** - Dependency injection details
- **[Transports Guide](transports.md)** - Transport configuration
- **[Advanced Patterns](advanced-patterns.md)** - Complex use cases

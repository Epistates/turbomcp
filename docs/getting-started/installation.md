# Installation

Get TurboMCP v3 up and running in minutes.

## Prerequisites

- **Rust 1.89.0 or later** - [Install Rust](https://rustup.rs/)
- **Cargo** - Comes with Rust
- **Basic Rust knowledge** - Familiarity with async/await and traits

## Step 1: Create a New Project

```bash
cargo new my-mcp-server
cd my-mcp-server
```

## Step 2: Add TurboMCP

Add TurboMCP to your `Cargo.toml`:

```toml
[package]
name = "my-mcp-server"
version = "0.1.0"
edition = "2021"

[dependencies]
turbomcp = "3.0"
tokio = { version = "1", features = ["full"] }
```

## Step 3: Configure Tokio Runtime

Add the Tokio runtime macro to `src/main.rs`:

```rust
#[tokio::main]
async fn main() {
    println!("Hello, world!");
}
```

## Choosing Your Features

TurboMCP v3 has a modular architecture with optional features for different use cases. Choose what you need:

### Minimal (STDIO only)

```toml
turbomcp = "3.0"
```

- Just STDIO transport
- No extra dependencies
- Perfect for Claude desktop or simple integrations

### Full Stack (All Transports + Auth)

```toml
turbomcp = { version = "3.0", features = ["full"] }
```

- All transports (STDIO, HTTP, WebSocket, TCP, Unix, gRPC)
- OAuth 2.1 authentication
- OpenTelemetry observability
- All built-in injectables
- Production ready

### Common Configurations

**For HTTP servers:**

```toml
turbomcp = { version = "3.0", features = ["http", "websocket"] }
tokio = { version = "1", features = ["full"] }
```

**For gRPC transport (v3):**

```toml
turbomcp = { version = "3.0", features = ["grpc"] }
tokio = { version = "1", features = ["full"] }
```

**For OAuth authentication:**

```toml
turbomcp = { version = "3.0", features = ["http", "auth"] }
```

**For DPoP token binding:**

```toml
turbomcp = { version = "3.0", features = ["http", "auth", "dpop"] }
```

**For performance-critical applications:**

```toml
turbomcp = { version = "3.0", features = ["full", "simd"] }
```

**For OpenTelemetry observability (v3):**

```toml
turbomcp = { version = "3.0", features = ["http", "telemetry"] }
```

**For WASM/browser clients (v3):**

```toml
# In a separate crate targeting wasm32
turbomcp-wasm = "3.0"
```

## Feature Reference

### Transport Features

| Feature | Use Case | Crate |
|---------|----------|-------|
| `stdio` | Standard I/O transport (default) | turbomcp-stdio |
| `http` | HTTP + Server-Sent Events | turbomcp-http |
| `websocket` | WebSocket support | turbomcp-websocket |
| `tcp` | TCP networking | turbomcp-tcp |
| `unix` | Unix socket support | turbomcp-unix |
| `grpc` | gRPC transport (v3) | turbomcp-grpc |

### Security Features

| Feature | Use Case | Extra Dependencies |
|---------|----------|-------------------|
| `auth` | OAuth 2.1 authentication | oauth2, jsonwebtoken |
| `dpop` | DPoP token binding (RFC 9449) | ring, zeroize |
| `redis-storage` | Redis-based DPoP nonce tracking | redis |

### Performance Features

| Feature | Use Case | Extra Dependencies |
|---------|----------|-------------------|
| `simd` | SIMD-accelerated JSON | simd-json, sonic-rs |

### Observability Features (v3)

| Feature | Use Case | Extra Dependencies |
|---------|----------|-------------------|
| `telemetry` | OpenTelemetry integration | opentelemetry, tracing |

### Meta Features

| Feature | Description |
|---------|-------------|
| `full` | All features enabled |
| `network` | STDIO + TCP transports |
| `server-only` | TCP + Unix (no STDIO) |

## v3 Feature Simplification

In TurboMCP v3, all MCP 2025-11-25 specification features are **always available**. The following feature flags have been removed (they're now always on):

| Removed Feature | Now Always Available |
|-----------------|---------------------|
| `mcp-icons` | Icons, IconTheme |
| `mcp-url-elicitation` | URLElicitRequestParams |
| `mcp-sampling-tools` | tools/tool_choice in CreateMessageRequest |
| `mcp-enum-improvements` | EnumSchema, EnumOption |
| `mcp-draft` | description on Implementation |

Only experimental features require feature flags:

```toml
# Experimental tasks API
turbomcp = { version = "3.0", features = ["experimental-tasks"] }
```

## Using Individual Crates

For fine-grained control, you can depend on individual crates:

```toml
[dependencies]
# Core types (no_std compatible)
turbomcp-core = "3.0"

# Protocol implementation
turbomcp-protocol = "3.0"

# Just HTTP transport
turbomcp-http = "3.0"

# Just gRPC transport
turbomcp-grpc = "3.0"

# Wire codec abstraction
turbomcp-wire = "3.0"

# OpenTelemetry integration
turbomcp-telemetry = "3.0"

# WASM bindings (for browser targets)
turbomcp-wasm = "3.0"
```

## Verify Installation

Test that everything works:

```bash
cargo build
```

You should see output like:

```
   Compiling turbomcp v3.0.0
    Finished `dev` [unoptimized + debuginfo] target(s) in 12.34s
```

## Next Steps

- **[Quick Start](quick-start.md)** - Create your first handler
- **[Your First Server](first-server.md)** - Build a complete example
- **[Complete Guide](../guide/architecture.md)** - Learn more
- **[Error Handling](../guide/error-handling.md)** - Unified McpError system (v3)

## Troubleshooting

### `error[E0433]: cannot find crate 'tokio'`

Make sure you have tokio in your dependencies:

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
turbomcp = "3.0"
```

### `error: extern crate 'turbomcp' is unused`

You don't need to explicitly use TurboMCP in code - just having it as a dependency is enough. The macros will bring in what's needed.

### Compilation is slow

TurboMCP has many optional features. If you only need STDIO, don't enable unnecessary features:

```toml
# Fast compilation, minimal features
turbomcp = "3.0"  # Just STDIO

# Slow compilation, all features
turbomcp = { version = "3.0", features = ["full"] }
```

### `error: failed to resolve: use of undeclared crate or module 'McpResult'`

Import the prelude in your code:

```rust
use turbomcp::prelude::*;
```

### Migrating from v2.x

See the [v3 Migration Guide](../architecture/v3-migration.md) for detailed migration steps, including:

- `McpError` unification (replaces `ServerError`, `ClientError`)
- Feature flag changes
- Modular transport architecture

## Get Help

- **[Quick Start](quick-start.md)** - Simple tutorial
- **[Examples](../examples/basic.md)** - Real-world code
- **[API Reference](../api/protocol.md)** - Detailed docs
- **GitHub Issues** - Report problems

---

Ready to code? [Quick Start](quick-start.md)

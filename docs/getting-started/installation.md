# Installation

Get TurboMCP up and running in minutes.

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
turbomcp = "3.0.0-exp"
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

TurboMCP has optional features for different use cases. Choose what you need:

### Minimal (STDIO only)

```toml
turbomcp = "3.0.0-exp"
```

- Just STDIO transport
- No extra dependencies
- Perfect for Claude desktop or simple integrations

### Full Stack (All Transports + Auth)

```toml
turbomcp = { version = "3.0.0-exp", features = ["full"] }
```

- All transports (STDIO, HTTP, WebSocket, TCP, Unix)
- OAuth 2.1 authentication
- All built-in injectables
- Production ready

### Common Configurations

**For HTTP servers:**

```toml
turbomcp = { version = "3.0.0-exp", features = ["http", "websocket"] }
tokio = { version = "1", features = ["full"] }
```

**For OAuth authentication:**

```toml
turbomcp = { version = "3.0.0-exp", features = ["full", "auth"] }
```

**For DPoP token binding:**

```toml
turbomcp = { version = "3.0.0-exp", features = ["full", "auth", "dpop"] }
```

**For performance-critical applications:**

```toml
turbomcp = { version = "3.0.0-exp", features = ["full", "simd"] }
```

## Feature Reference

| Feature | Use Case | Extra Dependencies |
|---------|----------|-------------------|
| `stdio` | Standard I/O transport | minimal |
| `http` | HTTP + Server-Sent Events | axum, tokio-rustls |
| `websocket` | WebSocket support | tokio-tungstenite |
| `tcp` | TCP networking | tokio |
| `unix` | Unix socket support | tokio |
| `auth` | OAuth 2.1 authentication | oauth2, jsonwebtoken |
| `dpop` | DPoP token binding (RFC 9449) | ring, zeroize |
| `simd` | SIMD-accelerated JSON | simd-json, sonic-rs |
| `full` | All features | all dependencies |

## Verify Installation

Test that everything works:

```bash
cargo build
```

You should see output like:

```
   Compiling turbomcp v2.3.3
    Finished `dev` [unoptimized + debuginfo] target(s) in 12.34s
```

## Next Steps

- **[Quick Start](quick-start.md)** - Create your first handler
- **[Your First Server](first-server.md)** - Build a complete example
- **[Complete Guide](../guide/architecture.md)** - Learn more

## Troubleshooting

### `error[E0433]: cannot find crate 'tokio'`

Make sure you have tokio in your dependencies:

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
turbomcp = "3.0.0-exp"
```

### `error: extern crate 'turbomcp' is unused`

You don't need to explicitly use TurboMCP in code - just having it as a dependency is enough. The macros will bring in what's needed.

### Compilation is slow

TurboMCP has many optional features. If you only need STDIO, don't enable unnecessary features:

```toml
# Fast compilation, minimal features
turbomcp = "3.0.0-exp"  # Just STDIO

# Slow compilation, all features
turbomcp = { version = "3.0.0-exp", features = ["full"] }
```

### `error: failed to resolve: use of undeclared crate or module 'McpResult'`

Import the prelude in your code:

```rust
use turbomcp::prelude::*;
```

## Get Help

- **[Quick Start](quick-start.md)** - Simple tutorial
- **[Examples](../examples/basic.md)** - Real-world code
- **[API Reference](../api/protocol.md)** - Detailed docs
- **GitHub Issues** - Report problems

---

Ready to code? → [Quick Start](quick-start.md)

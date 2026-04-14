# TurboMCP

[![Crates.io](https://img.shields.io/crates/v/turbomcp.svg)](https://crates.io/crates/turbomcp)
[![Documentation](https://docs.rs/turbomcp/badge.svg)](https://docs.rs/turbomcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)
[![Tests](https://github.com/Epistates/turbomcp/actions/workflows/test.yml/badge.svg)](https://github.com/Epistates/turbomcp/actions/workflows/test.yml)

**Production-ready Rust SDK for the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) with zero-boilerplate macros, modular transport architecture, and WASM support.**

> **TurboMCP 3.0** is a major architectural release featuring a modular 25-crate workspace, unified error handling, `no_std` core for edge/WASM deployment, individual transport crates, and full MCP 2025-11-25 specification compliance. See the [Migration Guide](./MIGRATION.md) for upgrading from v1 or v2.

---

## Quick Start

```toml
[dependencies]
turbomcp = "3.0.2"
tokio = { version = "1", features = ["full"] }
```

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server(name = "calculator", version = "1.0.0")]
impl Calculator {
    /// Add two numbers together.
    #[tool]
    async fn add(&self, a: i64, b: i64) -> i64 {
        a + b
    }

    /// Multiply two numbers.
    #[tool]
    async fn multiply(&self, a: i64, b: i64) -> i64 {
        a * b
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Calculator.run_stdio().await?;
    Ok(())
}
```

Save, `cargo run`, and connect from Claude Desktop:

```json
{
  "mcpServers": {
    "calculator": {
      "command": "/path/to/your/server",
      "args": []
    }
  }
}
```

---

## Requirements

- **Rust 1.89.0+** (Edition 2024)
- Tokio async runtime

## Feature Flags

TurboMCP uses feature flags for progressive enhancement. The default is `stdio` only.

### Presets

| Preset | Includes | Use Case |
|--------|----------|----------|
| `default` | STDIO | CLI tools, Claude Desktop |
| `minimal` | STDIO | Same as default (explicit) |
| `full` | STDIO, HTTP, WebSocket, TCP, Unix, Telemetry | Production servers |
| `full-stack` | Full + all client transports | Server + Client development |
| `all-transports` | All transports + channel | Testing and benchmarks |

### Individual Features

| Feature | Description |
|---------|-------------|
| `stdio` | Standard I/O transport (default) |
| `http` | HTTP/SSE with Axum integration |
| `websocket` | WebSocket bidirectional transport |
| `tcp` | Raw TCP socket transport |
| `unix` | Unix domain socket transport |
| `channel` | In-process channel (zero-overhead testing) |
| `telemetry` | OpenTelemetry, metrics, structured logging |
| `auth` | OAuth 2.1 with PKCE and multi-provider support |
| `dpop` | DPoP (RFC 9449) proof-of-possession |
| `client-integration` | Client library with STDIO transport |
| `full-client` | Client library with all transports |

```toml
# Production server with all transports and telemetry
turbomcp = { version = "3.0.2", features = ["full"] }

# Add authentication
turbomcp = { version = "3.0.2", features = ["full", "auth"] }

# Server + client for full-stack development
turbomcp = { version = "3.0.2", features = ["full-stack"] }
```

---

## Procedural Macros

TurboMCP provides five attribute macros:

| Macro | Purpose |
|-------|---------|
| `#[server]` | Define an MCP server with name, version, and transport configuration |
| `#[tool]` | Register a method as a tool handler with automatic JSON schema generation |
| `#[resource]` | Register a resource handler with URI pattern matching |
| `#[prompt]` | Register a prompt template with parameter substitution |
| `#[description]` | Add rich descriptions to tool parameters |

### Server Definition

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(
    name = "my-server",
    version = "1.0.0",
    description = "A server with tools, resources, and prompts"
)]
impl MyServer {
    /// Greet someone by name.
    #[tool]
    async fn greet(&self, name: String) -> String {
        format!("Hello, {}!", name)
    }

    /// Process an order with validated parameters.
    #[tool(description = "Process a customer order")]
    async fn process_order(
        &self,
        #[description("Customer order ID")] order_id: String,
        #[description("Priority level 1-10")] priority: u8,
    ) -> McpResult<String> {
        Ok(format!("Order {} queued at priority {}", order_id, priority))
    }

    /// System status prompt.
    #[prompt]
    async fn system_status(&self) -> McpResult<String> {
        Ok("Report the current system status.".to_string())
    }

    /// Configuration resource.
    #[resource(uri = "config://app", mime_type = "application/json")]
    async fn app_config(&self) -> McpResult<String> {
        Ok(r#"{"debug": false, "version": "1.0"}"#.to_string())
    }
}
```

JSON schemas are generated at compile time from function signatures. No runtime schema computation.

### Transport Selection

The `#[server]` macro generates transport-specific methods based on enabled features:

```rust
// STDIO (default feature)
MyServer.run_stdio().await?;

// Or use the builder for more control
MyServer.builder()
    .transport(Transport::Http { addr: "0.0.0.0:8080".to_string() })
    .serve()
    .await?;
```

Available `run_*` methods (when features are enabled):
- `run_stdio()` — STDIO transport
- `run_http(addr)` — HTTP/SSE with Axum
- `run_tcp(addr)` — Raw TCP
- `run_unix(path)` — Unix domain socket

---

## Client Connections

TurboMCP provides a client library for connecting to MCP servers (requires `client-integration` or `full-client` feature):

```rust
use turbomcp_client::Client;

// One-liner connection with auto-initialization
let client = Client::connect_http("http://localhost:8080").await?;

// Call tools
let tools = client.list_tools().await?;
let result = client.call_tool("greet", Some(serde_json::json!({"name": "World"}))).await?;

// Other transports
let client = Client::connect_tcp("127.0.0.1:8765").await?;
let client = Client::connect_unix("/tmp/mcp.sock").await?;
```

---

## Architecture

TurboMCP 3.0 is a modular 25-crate workspace with a layered dependency structure:

```
SDK Layer:        turbomcp (re-exports + prelude)
                  turbomcp-macros (#[server], #[tool], #[resource], #[prompt])

Framework Layer:  turbomcp-server (handler registry, middleware, routing)
                  turbomcp-client (connection management, retry, handlers)

Transport Layer:  turbomcp-transport (aggregator with feature flags)
                  turbomcp-stdio | turbomcp-http | turbomcp-websocket
                  turbomcp-tcp   | turbomcp-unix | turbomcp-transport-streamable

Protocol Layer:   turbomcp-protocol (JSON-RPC 2.0, MCP types, session management)
                  turbomcp-transport-traits (lean Send + Sync trait definitions)

Foundation Layer: turbomcp-core (no_std/alloc: McpError, McpResult, McpHandler)
                  turbomcp-types (unified MCP type definitions)
                  turbomcp-wire (wire format codec abstraction)

Specialized:      turbomcp-auth (OAuth 2.1) | turbomcp-dpop (RFC 9449)
                  turbomcp-grpc | turbomcp-wasm | turbomcp-openapi
                  turbomcp-telemetry | turbomcp-proxy | turbomcp-cli
```

Key design decisions:
- **Compile-time schema generation** from Rust types via `schemars` — zero runtime cost
- **Feature-gated transports** — only compile what you use
- **`no_std` core** — `turbomcp-core` and `turbomcp-wire` work on WASM and embedded targets
- **Arc-cloning pattern** — `McpServer` and `Client` are cheap to clone (Axum/Tower convention)
- **Unified errors** — `McpError`/`McpResult` from `turbomcp-core`, re-exported everywhere

---

## Examples

15 focused examples covering all patterns. Run with `cargo run --example <name>`.

### Server Basics

| Example | What It Teaches |
|---------|----------------|
| [hello_world](./crates/turbomcp/examples/hello_world.rs) | Simplest MCP server — one tool |
| [macro_server](./crates/turbomcp/examples/macro_server.rs) | Clean `#[server]` macro API with multiple tools |
| [calculator](./crates/turbomcp/examples/calculator.rs) | Structured input with `#[tool]` |
| [stateful](./crates/turbomcp/examples/stateful.rs) | `Arc<RwLock<T>>` shared state pattern |
| [validation](./crates/turbomcp/examples/validation.rs) | Parameter validation strategies |
| [tags_versioning](./crates/turbomcp/examples/tags_versioning.rs) | Tags and versioning for components |

### v3 Features

| Example | What It Teaches |
|---------|----------------|
| [visibility](./crates/turbomcp/examples/visibility.rs) | Progressive disclosure with VisibilityLayer |
| [composition](./crates/turbomcp/examples/composition.rs) | Multiple servers with CompositeHandler |
| [middleware](./crates/turbomcp/examples/middleware.rs) | Typed middleware for logging/metrics |
| [test_client](./crates/turbomcp/examples/test_client.rs) | In-memory testing with McpTestClient |

### Transport and Client

| Example | What It Teaches |
|---------|----------------|
| [tcp_server](./crates/turbomcp/examples/tcp_server.rs) | TCP network server |
| [tcp_client](./crates/turbomcp/examples/tcp_client.rs) | TCP client connection |
| [unix_client](./crates/turbomcp/examples/unix_client.rs) | Unix socket client |
| [transports_demo](./crates/turbomcp/examples/transports_demo.rs) | Multi-transport demonstration |

### Advanced

| Example | What It Teaches |
|---------|----------------|
| [type_state_builders_demo](./crates/turbomcp/examples/type_state_builders_demo.rs) | Type-state builder pattern |

See the [Examples Guide](./crates/turbomcp/examples/README.md) for learning paths and detailed usage.

---

## Transport Protocols

| Transport | Feature | Use Case |
|-----------|---------|----------|
| STDIO | `stdio` (default) | Claude Desktop, CLI tools |
| HTTP/SSE | `http` | Web applications, REST APIs |
| WebSocket | `websocket` | Real-time bidirectional |
| TCP | `tcp` | High-throughput clusters |
| Unix Socket | `unix` | Container IPC |
| Channel | `channel` | In-process testing |

```rust
// Runtime transport selection
match std::env::var("TRANSPORT").as_deref() {
    Ok("http") => server.run_http("0.0.0.0:8080").await?,
    Ok("tcp") => server.run_tcp("0.0.0.0:9000").await?,
    Ok("unix") => server.run_unix("/var/run/mcp.sock").await?,
    _ => server.run_stdio().await?,
}
```

---

## Security

- **OAuth 2.1** with PKCE and multi-provider support (Google, GitHub, Microsoft, Apple, Okta, Auth0, Keycloak) via `auth` feature
- **DPoP** (RFC 9449) proof-of-possession via `dpop` feature
- **Session management** with timeout enforcement and cleanup
- **Rate limiting** configuration
- **CORS** and security headers for HTTP transports
- **TLS** support via `rustls`

See [Security Features](./crates/turbomcp-transport/SECURITY_FEATURES.md) for details.

---

## Development

### Build and Test

```bash
# Build workspace
cargo build --workspace

# Run full test suite (tests, clippy, fmt, examples)
just test

# Run only unit tests
just test-only

# Format and lint
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

### CLI Tools

```bash
cargo install --path crates/turbomcp-cli

turbomcp-cli tools list --command "./target/debug/your-server"
turbomcp-cli tools call greet --arguments '{"name": "World"}' --command "./your-server"
```

### Benchmarks

```bash
cargo bench --workspace
./scripts/run_benchmarks.sh
```

---

## Deployment

### Docker

```dockerfile
FROM rust:1.89 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/your-server /usr/local/bin/
EXPOSE 8080
CMD ["your-server"]
```

### Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: mcp-server
spec:
  replicas: 3
  selector:
    matchLabels:
      app: mcp-server
  template:
    metadata:
      labels:
        app: mcp-server
    spec:
      containers:
      - name: server
        image: your-registry/mcp-server:latest
        ports:
        - containerPort: 8080
        env:
        - name: TRANSPORT
          value: "http"
        resources:
          requests:
            memory: "64Mi"
            cpu: "50m"
          limits:
            memory: "256Mi"
            cpu: "500m"
```

---

## Documentation

| Resource | Link |
|----------|------|
| API Reference | [docs.rs/turbomcp](https://docs.rs/turbomcp) |
| Migration Guide (v1/v2/v3) | [MIGRATION.md](./MIGRATION.md) |
| Architecture | [ARCHITECTURE.md](./ARCHITECTURE.md) |
| Crate Overview | [crates/README.md](./crates/README.md) |
| Examples (15) | [examples/](./crates/turbomcp/examples/README.md) |
| Security | [SECURITY_FEATURES.md](./crates/turbomcp-transport/SECURITY_FEATURES.md) |
| Benchmarks | [benches/](./benches/README.md) |
| MCP Specification | [modelcontextprotocol.io](https://modelcontextprotocol.io) |

---

## Contributing

1. Fork the repository and create a feature branch
2. Write tests — run `just test` to validate
3. Ensure `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
4. Submit a pull request

```bash
git clone https://github.com/Epistates/turbomcp.git
cd turbomcp
cargo build --workspace
just test
```

---

## License

[MIT](./LICENSE)

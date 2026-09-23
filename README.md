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
turbomcp = "3.5.0"
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
| `http` | Streamable HTTP transport |
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
turbomcp = { version = "3.5.0", features = ["full"] }

# Add authentication
turbomcp = { version = "3.5.0", features = ["full", "auth"] }

# Server + client for full-stack development
turbomcp = { version = "3.5.0", features = ["full-stack"] }
```

---

## Procedural Macros

`#[server]` turns an impl block into an MCP server. Inside it, marker attributes register handlers:

| Macro | Purpose |
|-------|---------|
| `#[server]` | Define an MCP server: name, version, description, instructions, icons, `page_size` |
| `#[tool]` | Register a method as a tool handler with automatic JSON schema generation |
| `#[resource]` | Register a resource handler for a URI or RFC 6570 URI template |
| `#[prompt]` | Register a prompt template; its parameters become prompt arguments |
| `#[completion]` | Answer `completion/complete` (advertises `completions`) |
| `#[subscribe]` / `#[unsubscribe]` | Answer `resources/subscribe` and `resources/unsubscribe` (must be declared together) |
| `#[set_level]` | Observe `logging/setLevel` |
| `#[roots_changed]` | React to `notifications/roots/list_changed` |
| `#[description]` / `#[title]` | Describe a parameter; `#[title]` gives a prompt argument a display label |

The server advertises exactly the capabilities its handlers can serve. `logging` is always advertised, because every server can send log messages through `ctx.log()` and accepts `logging/setLevel` without a `#[set_level]` handler.

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

    /// Review a piece of code.
    #[prompt]
    async fn code_review(
        &self,
        #[title("Language")]
        #[description("Language the code is written in")]
        language: String,
        ctx: &RequestContext,
    ) -> McpResult<String> {
        Ok(format!("Review this {language} code for bugs."))
    }

    /// Configuration resource.
    #[resource("config://app", mime_type = "application/json")]
    async fn app_config(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"debug": false, "version": "1.0"}"#.to_string())
    }

    /// A user record. The handler receives the full URI that matched the template.
    #[resource("users://{id}", mime_type = "application/json")]
    async fn user(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        let id = uri.trim_start_matches("users://");
        Ok(format!(r#"{{"id": "{id}"}}"#))
    }
}
```

Handler signatures follow a few rules:

- **Tools** take their arguments as parameters; add `ctx: &RequestContext` anywhere to get the request context. Return any `IntoToolResult` type (`String`, numbers, `Json<T>`, `ToolResult`, or `McpResult<T>` of those).
- **Resources** take `(uri: String, ctx: &RequestContext)` and return `McpResult<T>`. The URI is the attribute's first argument. A template's variables are not bound to parameters, so parse them out of `uri`.
- **Prompts** take `String` (required) or `Option<String>` (optional) arguments followed by `ctx: &RequestContext`.

`#[tool]` also accepts `read_only`, `destructive`, `idempotent`, `open_world`, `output_schema = Type`, and `task_support = "forbidden" | "optional" | "required"`. `#[resource]` accepts `mime_type`, `audience = ["user", "assistant"]`, `priority`, `last_modified`, and `size`. All markers accept `description`, `title`, `tags`, `version`, and `icons`, and an unknown key is a compile error.

JSON schemas are generated at compile time from function signatures. No runtime schema computation.

### Transport Selection

Every handler gets `run_*` methods from the `McpHandlerExt` trait, one per enabled transport feature, and a `builder()` for more control. Continuing with `MyServer` from above:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `MyServer.run_stdio().await?` is all a STDIO server needs. The builder
    // adds rate limits, connection limits, and graceful shutdown.
    // `Transport::http` needs the `http` feature.
    MyServer
        .builder()
        .transport(Transport::http("0.0.0.0:8080"))
        .with_rate_limit(100, Duration::from_secs(1))
        .with_graceful_shutdown(Duration::from_secs(30))
        .serve()
        .await?;
    Ok(())
}
```

Available `run_*` methods (when features are enabled):
- `run_stdio()` — STDIO transport
- `run_http(addr)` — Streamable HTTP
- `run_websocket(addr)` — WebSocket
- `run_tcp(addr)` — Raw TCP
- `run_unix(path)` — Unix domain socket

---

## Client Connections

TurboMCP provides a client library for connecting to MCP servers. `client-integration` gives the client with STDIO; `full-client` adds the HTTP, WebSocket, TCP, and Unix transports that the `connect_*` helpers below need:

```rust
use std::collections::HashMap;
use std::time::Duration;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connects to http://localhost:8080/mcp and runs the initialize handshake
    let client = Client::connect_http("http://localhost:8080").await?;

    let tools = client.list_tools().await?;

    // Arguments are a name -> JSON value map; the last parameter is an
    // optional task augmentation.
    let args = HashMap::from([("name".to_string(), serde_json::json!("World"))]);
    let result = client.call_tool("greet", Some(args), None).await?;

    // A per-call timeout for one slow request, without reconfiguring the client
    let slow = client
        .with_timeout(Duration::from_secs(600))
        .call_tool("reindex", None, None)
        .await?;

    client.shutdown().await?;

    // Other transports
    let tcp = Client::connect_tcp("127.0.0.1:8765").await?;
    let unix = Client::connect_unix("/tmp/mcp.sock").await?;
    Ok(())
}
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
- **Arc-cloning pattern** — handlers (`McpHandler: Clone`) and `Client` are cheap to clone (Axum/Tower convention)
- **Unified errors** — `McpError`/`McpResult` from `turbomcp-core`, re-exported everywhere

---

## Examples

16 focused examples covering all patterns. Run with `cargo run -p turbomcp --example <name>`; the TCP and Unix examples need their transport feature (and `full-client` for the clients), e.g. `cargo run -p turbomcp --example tcp_client --features tcp,full-client`.

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
| [unix_server](./crates/turbomcp/examples/unix_server.rs) | Unix socket server |
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
| Streamable HTTP | `http` | Web applications, REST APIs |
| WebSocket | `websocket` | Real-time bidirectional |
| TCP | `tcp` | High-throughput clusters |
| Unix Socket | `unix` | Container IPC |
| Channel | `channel` | In-process testing |

Runtime transport selection, with the `full` feature set and `MyServer` from above:

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = MyServer;
    match std::env::var("TRANSPORT").as_deref() {
        Ok("http") => server.run_http("0.0.0.0:8080").await?,
        Ok("ws") => server.run_websocket("0.0.0.0:8080").await?,
        Ok("tcp") => server.run_tcp("0.0.0.0:9000").await?,
        Ok("unix") => server.run_unix("/var/run/mcp.sock").await?,
        _ => server.run_stdio().await?,
    }
    Ok(())
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
- **MCP authorization** for Streamable HTTP: the server publishes RFC 9728 protected-resource metadata and refuses requests without a valid bearer token. On the client, `StreamableHttpClientConfig::auth_provider` answers a server's `401`/`403` challenge.

With the `auth` and `http` features and `turbomcp-server` as a direct dependency (for `HttpAuthorization`), `JwtBearerValidator` checks JWTs, audience included:

```rust
use turbomcp::auth::jwt::JwtValidator;
use turbomcp::auth::server::JwtBearerValidator;
use turbomcp::prelude::*;
use turbomcp_server::HttpAuthorization;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The server's canonical URL: tokens must name it as their audience
    let resource = "https://mcp.example.com/mcp";
    let jwt = JwtValidator::with_jwks_uri(
        "https://auth.example.com".to_string(),
        resource.to_string(),
        "https://auth.example.com/.well-known/jwks.json".to_string(),
    );
    let config = ServerConfig::builder()
        .authorization(HttpAuthorization::new(
            resource,
            "https://auth.example.com",
            JwtBearerValidator::new(jwt).with_required_scopes(["mcp:tools"]),
        ))
        .build();

    turbomcp_server::transport::http::run_with_config(&MyServer, "0.0.0.0:8080", &config).await?;
    Ok(())
}
```

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
| Examples (16) | [examples/](./crates/turbomcp/examples/README.md) |
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

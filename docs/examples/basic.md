# Basic Examples

Get started with TurboMCP through the current macro API and runnable workspace
examples.

## Minimal STDIO Server

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct HelloServer;

#[turbomcp::server(name = "hello", version = "1.0.0")]
impl HelloServer {
    #[tool(description = "Say hello to someone")]
    async fn hello(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {}!", name))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    HelloServer.run_stdio().await?;
    Ok(())
}
```

Run it:

```bash
cargo run -p turbomcp --example hello_world
```

STDIO is the MCP transport, so stdout is reserved for JSON-RPC responses. Send a
complete session when testing from the shell:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","clientInfo":{"name":"smoke","version":"1.0"},"capabilities":{}}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"hello","arguments":{"name":"World"}}}' \
  | cargo run -p turbomcp --example hello_world
```

## Tools With Validation

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[turbomcp::server(name = "calculator", version = "1.0.0")]
impl Calculator {
    #[tool(description = "Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    #[tool(description = "Divide two numbers")]
    async fn divide(&self, a: f64, b: f64) -> McpResult<f64> {
        if b == 0.0 {
            return Err(McpError::invalid_params("Division by zero"));
        }
        Ok(a / b)
    }
}
```

See `crates/turbomcp/examples/calculator.rs` for a complete arithmetic server
and `crates/turbomcp/examples/validation.rs` for a runnable validation-focused
server.

## Resources And Prompts

Handlers can expose resources and prompts from the same `impl` block:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct HelpServer;

#[turbomcp::server(name = "help-server", version = "1.0.0")]
impl HelpServer {
    #[resource("info://server")]
    async fn server_info(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"name":"help-server","status":"ready"}"#.to_string())
    }

    #[prompt(description = "How to use the server")]
    async fn help(&self, _ctx: &RequestContext) -> McpResult<PromptResult> {
        Ok(PromptResult::user("Call hello(name) to greet a user."))
    }
}
```

The `test_client.rs`, `composition.rs`, and `visibility.rs` examples include
tools, resources, prompts, and metadata inspection patterns.

## Network Transports

Use the explicit transport methods enabled by Cargo features. With
`HelloServer` from above:

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::var("TRANSPORT").as_deref() {
        // TCP, requires the "tcp" feature
        Ok("tcp") => HelloServer.run_tcp("127.0.0.1:8765").await?,
        // Unix sockets, requires the "unix" feature and a Unix platform
        Ok("unix") => HelloServer.run_unix("/tmp/turbomcp.sock").await?,
        // STDIO, enabled by the default feature
        _ => HelloServer.run_stdio().await?,
    }
    Ok(())
}
```

Runnable transport pairs:

```bash
cargo run -p turbomcp --example tcp_server --features tcp
cargo run -p turbomcp --example tcp_client --features "tcp full-client"

cargo run -p turbomcp --example unix_server --features unix
cargo run -p turbomcp --example unix_client --features "unix full-client"
```

## Repository Examples

Primary `turbomcp` examples:

- `hello_world` - minimal STDIO server
- `macro_server` - macro server with several tools
- `calculator` - structured inputs and validation
- `stateful` - shared state
- `validation` - validation patterns
- `tags_versioning` - tags and version metadata
- `visibility` - progressive disclosure
- `composition` - composed handlers with namespaces
- `middleware` - typed middleware
- `test_client` - in-memory testing
- `type_state_builders_demo` - type-state builders
- `tcp_server` / `tcp_client` - TCP pair
- `unix_server` / `unix_client` - Unix socket pair
- `transports_demo` - feature-gated `run_*` methods

Other workspace examples:

- `crates/turbomcp-openapi/examples/petstore.rs`
- `crates/turbomcp-auth/examples/*.rs`
- `crates/turbomcp-proxy/examples/*.rs`
- `crates/turbomcp-server/examples/manual_server.rs`
- `demo/src/main.rs`

## Next Steps

- [Patterns](patterns.md)
- [Advanced](advanced.md)
- [Handlers Guide](../guide/handlers.md)
- [API Reference](../api/server.md)

# TurboMCP Demo

Minimal demonstration of TurboMCP v3 zero-boilerplate server API.

## What It Does

A simple MCP server with three tools:

- **hello** — Greet someone by name (optional parameter)
- **add** — Add two integers
- **current_time** — Get the current UTC time

## Build and Run

```bash
# From the TurboMCP root directory
cargo build -p turbomcp-demo

# Run the server (STDIO transport)
cargo run -p turbomcp-demo
```

## Connect from Claude Desktop

Add to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "turbomcp-demo": {
      "command": "/path/to/turbomcp/target/debug/turbomcp-demo"
    }
  }
}
```

## Source

The entire server is 37 lines of code in `src/main.rs`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct DemoServer;

#[server(name = "turbomcp-demo", version = "3.0.0")]
impl DemoServer {
    #[tool]
    async fn hello(&self, name: Option<String>) -> String {
        let name = name.unwrap_or_else(|| "World".to_string());
        format!("Hello, {name}! Welcome to TurboMCP!")
    }

    #[tool]
    async fn add(&self, a: i64, b: i64) -> i64 {
        a + b
    }

    #[tool]
    async fn current_time(&self) -> String {
        chrono::Utc::now().to_rfc3339()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    DemoServer.run().await?;
    Ok(())
}
```

## Related

- [Examples Guide](../crates/turbomcp/examples/README.md) — 15 focused examples
- [Main README](../README.md) — Full documentation
- [Migration Guide](../MIGRATION.md) — Upgrading from v1 or v2

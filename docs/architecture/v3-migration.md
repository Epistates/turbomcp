# v3 Migration Guide

Complete guide for migrating from TurboMCP v2.x to v3.0.

## Overview

TurboMCP 3.0 introduces a **Zero Boilerplate** architecture using procedural macros. It simplifies server creation by generating `McpHandler` implementations and JSON schemas automatically.

## Quick Migration

### 1. Update Dependencies

```toml
[dependencies]
turbomcp = "3.5.0"
tokio = { version = "1", features = ["full"] }
```

### 2. Update Server Definition

**Before (v2.x, no longer compiles):**

```rust,ignore
// v2 required manual handler registration and schema definition
struct MyServer;

#[async_trait]
impl McpServer for MyServer {
    async fn handle_tool(&self, name: &str, args: Value) -> Result<Value, Error> {
        match name {
            "add" => {
                // Manual argument parsing
                let a = args["a"].as_i64().unwrap();
                let b = args["b"].as_i64().unwrap();
                Ok(json!(a + b))
            }
            _ => Err(Error::MethodNotFound),
        }
    }
    // ... manual list_tools implementation ...
}
```

**After (v3.x):**

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    #[tool("Add two numbers")]
    async fn add(&self, a: i64, b: i64) -> i64 {
        a + b
    }
}
```

### 3. Update Run Command

**Before (v2.x):**

```rust,ignore
let server = MyServer;
let transport = StdioTransport::new(server);
transport.run().await?;
```

**After (v3.x):**

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_stdio().await
}
```

## Key Changes

### 1. `McpHandler` Trait

The core trait is now `McpHandler`, defined in `turbomcp-core`. You typically don't implement this manually anymore; the `#[server]` macro does it for you.

### 2. Procedural Macros

- `#[server]`: Annotates the `impl` block of your server struct.
- `#[tool]`: Marks a method as a tool. Schema is generated from function signature.
- `#[resource]`: Marks a method as a resource handler.
- `#[prompt]`: Marks a method as a prompt handler.
- `#[completion]`, `#[subscribe]`/`#[unsubscribe]`, `#[set_level]`, `#[roots_changed]`: opt into the optional MCP methods (3.5).

### 3. Return Types

Tools return any type that implements `IntoToolResult`: `String`, numbers, `bool`, `serde_json::Value`, `Vec<T: Serialize>`, `Json<T>` (structured content), `ToolResult`, or `McpResult<T>` of those. Resources return `McpResult<T>` of a `String` or `ResourceResult`, and prompts a `String`, `PromptResult`, or `Vec<Message>` (optionally in a `Result`). You don't need to wrap everything in `Value` manually.

### 4. Transports

Transports are now accessed via extension traits (`McpHandlerExt`):
- `run_stdio()`
- `run_http(addr)`
- `run_websocket(addr)`
- `run_tcp(addr)`
- `run_unix(path)`

Each needs its transport's feature. For rate limits, connection limits, or a
transport chosen at runtime, use `builder().transport(...).serve()`.

### 5. `no_std` Support

The core types are now `no_std` compatible, enabling usage in WASM environments (like Cloudflare Workers).

## Migrating specific features

### Error Handling

**v2:**
```rust,ignore
return Err(ServerError::internal("error"));
```

**v3:**
```rust
use turbomcp::prelude::*;

fn load(path: &str) -> McpResult<String> {
    if path.is_empty() {
        return Err(McpError::invalid_params("path must not be empty"));
    }
    std::fs::read_to_string(path).map_err(|e| McpError::internal(e.to_string()))
}
```
(Or just return `Result<T, McpError>` and use `?`)

### Context

**v2:**
```rust,ignore
async fn my_tool(&self, ctx: Context, ...)
```

**v3:**
```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// A parameter of type `&RequestContext` (any name, any position) is the
    /// request context; it is not part of the tool's input schema.
    #[tool]
    async fn my_tool(&self, input: String, ctx: &RequestContext) -> String {
        format!("{input} (request {})", ctx.request_id())
    }
}
```

## Need Help?

Check the [examples](../examples/) for the latest patterns, or ask in the GitHub discussions.

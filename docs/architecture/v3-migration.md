# v3 Migration Guide

Complete guide for migrating from TurboMCP v2.x to v3.0.

## Overview

TurboMCP 3.0 introduces a **Zero Boilerplate** architecture using procedural macros. It simplifies server creation by generating `McpHandler` implementations and JSON schemas automatically.

## Quick Migration

### 1. Update Dependencies

```toml
[dependencies]
turbomcp = "3.0"
tokio = { version = "1", features = ["full"] }
```

### 2. Update Server Definition

**Before (v2.x):**

```rust
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

```rust
let server = MyServer;
let transport = StdioTransport::new(server);
transport.run().await?;
```

**After (v3.x):**

```rust
MyServer.run_stdio().await?;
```

## Key Changes

### 1. `McpHandler` Trait

The core trait is now `McpHandler`, defined in `turbomcp-core`. You typically don't implement this manually anymore; the `#[server]` macro does it for you.

### 2. Procedural Macros

- `#[server]`: Annotates the `impl` block of your server struct.
- `#[tool]`: Marks a method as a tool. Schema is generated from function signature.
- `#[resource]`: Marks a method as a resource handler.
- `#[prompt]`: Marks a method as a prompt handler.

### 3. Return Types

Handlers can now return any type that implements `Serialize` (via `IntoToolResult`, etc.), or `McpResult<T>`. You don't need to wrap everything in `Result` or `Value` manually.

### 4. Transports

Transports are now accessed via extension traits (`McpHandlerExt`):
- `run_stdio()`
- `run_http(addr)`
- `run_websocket(addr)`
- `run_tcp(addr)`

### 5. `no_std` Support

The core types are now `no_std` compatible, enabling usage in WASM environments (like Cloudflare Workers).

## Migrating specific features

### Error Handling

**v2:**
```rust
return Err(ServerError::internal("error"));
```

**v3:**
```rust
return Err(McpError::internal("error"));
```
(Or just return `Result<T, McpError>` and use `?`)

### Context

**v2:**
```rust
async fn my_tool(&self, ctx: Context, ...)
```

**v3:**
```rust
// RequestContext is injected if you add it as an argument named `ctx`
async fn my_tool(&self, ctx: RequestContext, ...)
```

## Need Help?

Check the [examples](../examples/) for the latest patterns, or ask in the GitHub discussions.
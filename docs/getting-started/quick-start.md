# Quick Start

Create your first MCP server in 5 minutes using the TurboMCP v3 "Zero Boilerplate" API.

## Minimal Example

Create `src/main.rs`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct HelloServer;

#[server(name = "hello-server", version = "1.0.0")]
impl HelloServer {
    #[tool("Say hello")]
    async fn hello(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {}!", name))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Run via STDIO transport (default for MCP servers)
    HelloServer.run_stdio().await?;
    Ok(())
}
```

That's all you need! Let's break it down:

## Understanding the Code

### 1. Import the Prelude

```rust
use turbomcp::prelude::*;
```

This imports everything you need: macros (`#[server]`, `#[tool]`), types (`McpResult`), and extension traits (`McpHandlerExt`).

### 2. Define Your Server Struct

```rust
#[derive(Clone)]
struct HelloServer;
```

Your server state goes here. It must derive `Clone` because it's shared across async tasks.

### 3. Implement the Handler

```rust
#[server(name = "hello-server", version = "1.0.0")]
impl HelloServer {
    #[tool("Say hello")]
    async fn hello(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {}!", name))
    }
}
```

The `#[server]` macro:
- Implements the `McpHandler` trait for you.
- Generates `server_info`, `list_tools`, etc.
- Routes incoming JSON-RPC requests to your methods.

The `#[tool]` macro:
- Registers the function as a tool.
- Generates JSON schema automatically from the function signature.
- Handles argument parsing and validation.

### 4. Run the Server

```rust
HelloServer.run_stdio().await?;
```

The `run_stdio()` method comes from the `McpHandlerExt` trait. It starts the server using standard input/output, which is the standard transport for local MCP servers.

## Run It

```bash
cargo run
```

Your server is now running! It will accept requests via stdin/stdout.

## Test It

In another terminal, test with the TurboMCP CLI:

```bash
# Install CLI if you haven't
cargo install turbomcp-cli

# List tools
turbomcp-cli tools list --command "./target/debug/your-server"

# Call the tool
turbomcp-cli tools call hello --arguments '{"name": "World"}' \
  --command "./target/debug/your-server"
```

## Add More Handlers

Add as many handlers as you want:

```rust
#[server(name = "math-server", version = "1.0.0")]
impl MathServer {
    #[tool("Add two numbers")]
    async fn add(&self, a: i32, b: i32) -> i32 {
        a + b
    }

    #[resource("config://app")]
    async fn get_config(&self) -> String {
        r#"{"debug": true}"#.to_string()
    }

    #[prompt("code-review")]
    async fn review_prompt(&self, code: String) -> String {
        format!("Review this code:\n\n{}", code)
    }
}
```

Note: You can return simple types like `i32` or `String` directly! The macros handle the conversion to `McpResult`.

## Add Documentation

Enhance your handlers with descriptions for the LLM:

```rust
#[tool]
async fn add(
    #[description("The first number")]
    a: i32,
    #[description("The second number")]
    b: i32,
) -> i32 {
    a + b
}
```

The `#[description]` attribute adds documentation to the generated JSON schema, helping the LLM understand how to use your tool.

## Next Steps

- **[Your First Server](first-server.md)** - More complete example with state
- **[Handlers Guide](../guide/handlers.md)** - All handler types detailed
- **[Examples](../examples/basic.md)** - Real-world patterns
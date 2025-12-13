# Quick Start

Create your first MCP server in 5 minutes.

## Minimal Example

Create `src/main.rs`:

```rust
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    let server = McpServer::new()
        .stdio()
        .run()
        .await?;

    Ok(())
}

#[tool]
async fn hello(name: String) -> McpResult<String> {
    Ok(format!("Hello, {}!", name))
}
```

That's all you need! Let's break it down:

## Understanding the Code

### 1. Import the Prelude

```rust
use turbomcp::prelude::*;
```

This imports everything you need: macros, types, traits.

### 2. Async Main Function

```rust
#[tokio::main]
async fn main() -> McpResult<()> {
```

- `#[tokio::main]` sets up the Tokio async runtime
- `McpResult<()>` is the standard error type in TurboMCP

### 3. Create and Run Server

```rust
let server = McpServer::new()
    .stdio()                    // Enable STDIO transport
    .run()                      // Start the server
    .await?;                    // Wait (indefinitely)
```

### 4. Define a Handler

```rust
#[tool]
async fn hello(name: String) -> McpResult<String> {
    Ok(format!("Hello, {}!", name))
}
```

The `#[tool]` macro:
- Registers the function as a tool
- Generates JSON schema from the signature
- Handles request parsing and response serialization

## Run It

```bash
cargo run
```

Your server is now running! It will accept requests via stdin/stdout.

## Test It

In another terminal, test with the TurboMCP CLI:

```bash
turbomcp-cli tools list --command "path/to/your/binary"
turbomcp-cli tools call hello --arguments '{"name": "World"}' \
  --command "path/to/your/binary"
```

Or test with raw JSON-RPC:

```bash
echo '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"hello","arguments":{"name":"Alice"}},"id":1}' \
  | ./target/debug/my-mcp-server
```

## Add More Handlers

Add as many handlers as you want:

```rust
#[tool]
async fn add(a: i32, b: i32) -> McpResult<i32> {
    Ok(a + b)
}

#[tool]
async fn echo(text: String) -> McpResult<String> {
    Ok(text)
}

#[resource]
async fn status() -> McpResult<String> {
    Ok("Server is running".to_string())
}

#[prompt]
async fn instructions() -> McpResult<String> {
    Ok("Use the tools to...".to_string())
}
```

Each function becomes:
- A registered tool, resource, or prompt
- Accessible to Claude with auto-generated schema
- Type-safe with validation

## Add Documentation

Enhance your handlers with descriptions:

```rust
#[tool(description = "Add two numbers")]
async fn add(
    #[description = "First number"]
    a: i32,
    #[description = "Second number"]
    b: i32,
) -> McpResult<i32> {
    Ok(a + b)
}
```

The descriptions appear in the generated schema.

## Add Dependency Injection

Request dependencies automatically:

```rust
#[tool]
async fn process(
    logger: Logger,
    cache: Cache,
) -> McpResult<String> {
    logger.info("Processing...").await?;
    cache.set("key", "value").await?;
    Ok("Done".to_string())
}
```

Available injectables:
- `InjectContext` - Full request context
- `RequestInfo` - Request metadata (ID, handler name)
- `Logger` - Structured logging
- `Config` - Application configuration
- `Cache` - In-memory caching
- `Database` - Database access
- `HttpClient` - HTTP requests

## Next Steps

- **[Your First Server](first-server.md)** - More complete example
- **[Handlers Guide](../guide/handlers.md)** - All handler types
- **[Context & DI](../guide/context-injection.md)** - Dependency injection
- **[Examples](../examples/basic.md)** - Real-world patterns

## Common Patterns

### Error Handling

```rust
#[tool]
async fn process(file: String) -> McpResult<String> {
    std::fs::read_to_string(&file)
        .map_err(|e| McpError::InvalidInput(e.to_string()))
}
```

### Logging

```rust
#[tool]
async fn work(logger: Logger) -> McpResult<String> {
    logger.info("Starting work").await?;
    logger.warn("Got unexpected value").await?;
    Ok("Done".to_string())
}
```

### Conditional Responses

```rust
#[tool]
async fn fetch(url: String) -> McpResult<String> {
    if url.starts_with("https://") {
        fetch_secure(&url).await
    } else {
        Err(McpError::InvalidInput("Only HTTPS supported".into()))
    }
}
```

## Troubleshooting

### `error: cannot find attribute macro 'tool'`

Make sure you imported the prelude:

```rust
use turbomcp::prelude::*;
```

### No response when testing

The STDIO transport reads JSON-RPC requests from stdin. Make sure you're sending:

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": { "name": "hello", "arguments": {"name": "World"} },
  "id": 1
}
```

### Timeout waiting for response

The server might be waiting for more input. Send a complete JSON-RPC request with a newline at the end.

---

Done! You have a working MCP server. Now:

- **[Add More Features](../guide/handlers.md)** - Tools, resources, prompts
- **[Add Transports](../guide/transports.md)** - HTTP, WebSocket, TCP
- **[Deploy It](../deployment/docker.md)** - Docker, Kubernetes, production

Happy coding! 🚀

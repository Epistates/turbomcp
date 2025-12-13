# Basic Examples

Get started with TurboMCP through practical, runnable examples.

## Hello World

The simplest MCP server - responds to a single tool call:

```rust
use turbomcp::prelude::*;

#[server]
pub struct HelloWorld;

#[tool]
pub async fn hello(
    #[description("The name to greet")] name: String,
) -> McpResult<String> {
    Ok(format!("Hello, {}!", name))
}

#[tokio::main]
async fn main() -> McpResult<()> {
    HelloWorld.stdio().run().await
}
```

**Run it:**
```bash
cargo run --example hello_world
```

**Test it:**
```bash
# Send a JSON-RPC request
echo '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"hello","arguments":{"name":"World"}},"id":1}' | ./target/debug/hello_world
```

## Calculator Server

Implement a server with multiple tool handlers:

```rust
use turbomcp::prelude::*;

#[server]
pub struct Calculator;

#[tool]
#[description("Add two numbers")]
pub async fn add(a: f64, b: f64) -> McpResult<f64> {
    Ok(a + b)
}

#[tool]
#[description("Subtract two numbers")]
pub async fn subtract(a: f64, b: f64) -> McpResult<f64> {
    Ok(a - b)
}

#[tool]
#[description("Multiply two numbers")]
pub async fn multiply(a: f64, b: f64) -> McpResult<f64> {
    Ok(a * b)
}

#[tool]
#[description("Divide two numbers")]
pub async fn divide(a: f64, b: f64) -> McpResult<f64> {
    if b == 0.0 {
        return Err(McpError::InvalidInput("Division by zero".into()));
    }
    Ok(a / b)
}

#[tokio::main]
async fn main() -> McpResult<()> {
    Calculator.stdio().run().await
}
```

**Test operations:**
```bash
# Call add
echo '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"add","arguments":{"a":5,"b":3}},"id":1}' | cargo run --example calculator

# Call divide (with validation)
echo '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"divide","arguments":{"a":10,"b":0}},"id":1}' | cargo run --example calculator
# Returns error: "Division by zero"
```

## Weather Information Server

Server with structured return types:

```rust
use serde::{Deserialize, Serialize};
use turbomcp::prelude::*;

#[derive(Serialize, Deserialize)]
pub struct WeatherInfo {
    location: String,
    temperature: f32,
    condition: String,
    humidity: u32,
}

#[server]
pub struct WeatherServer;

#[tool]
#[description("Get current weather for a city")]
pub async fn get_weather(
    #[description("City name")] city: String,
) -> McpResult<WeatherInfo> {
    // In a real app, query actual weather API
    Ok(WeatherInfo {
        location: city,
        temperature: 72.0,
        condition: "Sunny".to_string(),
        humidity: 65,
    })
}

#[tokio::main]
async fn main() -> McpResult<()> {
    WeatherServer.stdio().run().await
}
```

## With Error Handling

Proper error handling in tool handlers:

```rust
use turbomcp::prelude::*;

#[server]
pub struct FileServer;

#[tool]
pub async fn read_file(path: String) -> McpResult<String> {
    // Validate input
    if path.contains("..") {
        return Err(McpError::PermissionDenied(
            "Path traversal not allowed".into(),
        ));
    }

    // Read file
    std::fs::read_to_string(&path)
        .map_err(|e| McpError::NotFound(format!(
            "Failed to read file: {}",
            e
        )))
}

#[tokio::main]
async fn main() -> McpResult<()> {
    FileServer.stdio().run().await
}
```

## With Resources

Server that exposes files as resources:

```rust
use turbomcp::prelude::*;

#[server]
pub struct DocumentServer;

#[resource("document://{doc_id}")]
pub async fn get_document(
    #[param("doc_id")] doc_id: String,
) -> McpResult<String> {
    // Load document from storage
    Ok(format!("Document content for {}", doc_id))
}

#[tool]
#[description("List all documents")]
pub async fn list_documents() -> McpResult<Vec<String>> {
    Ok(vec![
        "document://1".to_string(),
        "document://2".to_string(),
        "document://3".to_string(),
    ])
}

#[tokio::main]
async fn main() -> McpResult<()> {
    DocumentServer.stdio().run().await
}
```

## With Prompts

Server that provides prompt templates:

```rust
use turbomcp::prelude::*;

#[server]
pub struct PromptServer;

#[prompt("translate")]
pub async fn translation_prompt(
    #[param("language")] language: String,
) -> McpResult<String> {
    Ok(format!(
        "Translate the following text to {}:\n\n{{{{ user_text }}}}",
        language
    ))
}

#[tool]
pub async fn translate(
    text: String,
    language: String,
) -> McpResult<String> {
    // In a real app, call translation API
    Ok(format!("Translated to {}: [translation]", language))
}

#[tokio::main]
async fn main() -> McpResult<()> {
    PromptServer.stdio().run().await
}
```

## With Context Injection

Using dependency injection in handlers:

```rust
use turbomcp::prelude::*;

#[server]
pub struct CachedServer;

#[tool]
pub async fn cached_lookup(
    #[description("Search key")] key: String,
    cache: Cache,
) -> McpResult<String> {
    // Check cache first
    if let Some(cached) = cache.get(&key).await? {
        return Ok(cached);
    }

    // Compute value
    let value = format!("Result for {}", key);

    // Cache it
    cache.set(&key, value.clone()).await?;

    Ok(value)
}

#[tool]
pub async fn log_action(
    #[description("Action name")] action: String,
    logger: Logger,
) -> McpResult<String> {
    logger.info(&format!("Action: {}", action)).await?;
    Ok(format!("Logged: {}", action))
}

#[tokio::main]
async fn main() -> McpResult<()> {
    CachedServer.stdio().run().await
}
```

## Over HTTP

Server accessible via HTTP:

```rust
use turbomcp::prelude::*;

#[server]
pub struct HttpServer;

#[tool]
pub async fn hello(name: String) -> McpResult<String> {
    Ok(format!("Hello, {}", name))
}

#[tokio::main]
async fn main() -> McpResult<()> {
    HttpServer
        .http(8080)  // Listen on port 8080
        .run()
        .await
}
```

**Use it:**
```bash
# Call via HTTP
curl -X POST http://localhost:8080/tools/call \
  -H "Content-Type: application/json" \
  -d '{"tool":"hello","arguments":{"name":"World"}}'
```

## Over TCP

Server with TCP transport:

```rust
use turbomcp::prelude::*;

#[server]
pub struct TcpServer;

#[tool]
pub async fn greet(name: String) -> McpResult<String> {
    Ok(format!("Greetings, {}!", name))
}

#[tokio::main]
async fn main() -> McpResult<()> {
    TcpServer
        .tcp("127.0.0.1:9000")  // Listen on port 9000
        .run()
        .await
}
```

**Connect to it:**
```bash
# Using netcat
echo '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"greet","arguments":{"name":"Friend"}},"id":1}' | nc localhost 9000
```

## Complete Examples from Repository

The codebase includes 26+ complete examples:

### Transport Examples
- `stdio_app` - Standard input/output transport
- `http_app` - HTTP/SSE transport with REST endpoints
- `tcp_server` - TCP server
- `tcp_client` - TCP client
- `websocket_server` - WebSocket server
- `unix_socket` - Unix domain sockets

### Feature Examples
- `context_injection` - Using dependency injection
- `authentication` - OAuth 2.1 and JWT auth
- `error_handling` - Error handling patterns
- `sampling` - Model sampling integration
- `resources` - Resource capabilities
- `prompts` - Prompt templates

### Pattern Examples
- `hello_world` - Minimal server
- `macro_server` - Using procedural macros
- `stateful` - Maintaining state
- `middleware` - Custom middleware
- `elicitation` - User input requests

## Running Examples

```bash
# List all examples
cargo build --examples

# Run a specific example
cargo run --example hello_world

# Run with specific features
cargo run --example http_app --features http

# See example code
cat crates/turbomcp/examples/hello_world.rs
```

## Next Steps

- **[Patterns](patterns.md)** - Real-world patterns and use cases
- **[Advanced](advanced.md)** - Advanced examples and optimizations
- **[Handlers Guide](../guide/handlers.md)** - Tool/resource/prompt development
- **[API Reference](../api/server.md)** - Complete API documentation


# Server API Reference

Complete API reference for building MCP servers with TurboMCP.

## Overview

The TurboMCP server API provides a high-level framework for building Model Context Protocol servers with minimal boilerplate. The framework automatically handles request routing, schema generation, and transport protocol management.

## Core Types

### McpServer

The main server type that coordinates handlers, transports, and runtime configuration.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[turbomcp::server(
    name = "my-server",
    version = "1.0.0",
    transports = ["stdio", "http"]
)]
impl MyServer {
    // Handler methods here
}
```

#### Server Attributes

| Attribute | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | `&str` | Yes | Server name for identification |
| `version` | `&str` | Yes | Semantic version string |
| `transports` | `[&str]` | No | Enabled transports (default: `["stdio"]`) |

**Available transports:**
- `"stdio"` - Standard input/output
- `"http"` - HTTP with Server-Sent Events
- `"websocket"` - WebSocket bidirectional communication
- `"tcp"` - TCP network sockets
- `"unix"` - Unix domain sockets

#### Generated Methods

The `#[server]` macro automatically generates transport methods:

```rust
impl MyServer {
    /// Run server with STDIO transport
    async fn run_stdio(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// Run server with HTTP transport on specified port
    async fn run_http(&self, port: u16) -> Result<(), Box<dyn std::error::Error>>;

    /// Run server with WebSocket transport on specified port
    async fn run_websocket(&self, port: u16) -> Result<(), Box<dyn std::error::Error>>;

    /// Run server with TCP transport on specified address
    async fn run_tcp(&self, addr: &str) -> Result<(), Box<dyn std::error::Error>>;

    /// Run server with Unix socket at specified path
    async fn run_unix(&self, path: &str) -> Result<(), Box<dyn std::error::Error>>;
}
```

### Example: Basic Server

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[turbomcp::server(name = "calculator", version = "1.0.0")]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Calculator.run_stdio().await?;
    Ok(())
}
```

## Handler Types

TurboMCP supports three types of MCP handlers: tools, resources, and prompts.

### Tool Handlers

Tools are functions that perform actions and return results.

```rust
#[tool]
async fn handler_name(&self, param1: Type1, param2: Type2) -> McpResult<ReturnType> {
    // Implementation
}
```

#### Tool Attributes

| Attribute | Type | Required | Description |
|-----------|------|----------|-------------|
| `description` | `&str` | No | Tool description for schema |

#### Example: Tool with Description

```rust
#[tool(description = "Searches for files matching a pattern")]
async fn search_files(
    &self,
    #[description("Glob pattern to match files")]
    pattern: String,
    #[description("Directory to search in")]
    directory: Option<String>
) -> McpResult<Vec<String>> {
    let dir = directory.unwrap_or_else(|| ".".to_string());
    // Implementation
    Ok(vec![])
}
```

### Resource Handlers

Resources provide read-only access to data or content.

```rust
#[resource]
async fn resource_name(&self, param: Type) -> McpResult<ResourceContent> {
    // Implementation
}
```

#### Resource Types

Resources can return various content types:

```rust
use turbomcp::ResourceContent;

#[resource]
async fn get_config(&self) -> McpResult<ResourceContent> {
    Ok(ResourceContent::Text {
        uri: "config://app".to_string(),
        mime_type: Some("application/json".to_string()),
        text: r#"{"debug": true}"#.to_string(),
    })
}

#[resource]
async fn get_image(&self, path: String) -> McpResult<ResourceContent> {
    let data = std::fs::read(&path)?;
    Ok(ResourceContent::Blob {
        uri: format!("file://{}", path),
        mime_type: Some("image/png".to_string()),
        blob: data,
    })
}
```

### Prompt Handlers

Prompts provide templated text for LLM interactions.

```rust
#[prompt]
async fn prompt_name(&self, param: Type) -> McpResult<PromptResult> {
    // Implementation
}
```

#### Example: Prompt Handler

```rust
use turbomcp::prelude::*;

#[prompt(description = "Generate code review prompt")]
async fn code_review(
    &self,
    #[description("Programming language")]
    language: String,
    #[description("Code to review")]
    code: String
) -> McpResult<PromptResult> {
    // Use the ergonomic builder API
    Ok(PromptResult::user(format!(
        "Please review this {} code:\n\n```{}\n{}\n```",
        language, language, code
    )))
}
```

For multi-message prompts, use the builder pattern:

```rust
Ok(PromptResult::user("Initial context")
    .add_assistant("I understand. What would you like me to do?")
    .add_user("Please analyze this data")
    .with_description("A multi-turn conversation prompt"))
```

## Parameter Types

### Supported Parameter Types

TurboMCP automatically handles serialization for these types:

**Primitives:**
- `bool`, `i8`, `i16`, `i32`, `i64`, `i128`
- `u8`, `u16`, `u32`, `u64`, `u128`
- `f32`, `f64`
- `String`, `&str`
- `char`

**Collections:**
- `Vec<T>`
- `HashMap<K, V>`
- `HashSet<T>`
- `Option<T>`

**Custom Types:**
- Any type implementing `Serialize` and `Deserialize`

### Custom Type Example

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[tool("Create a new user")]
async fn create_user(&self, user: User) -> McpResult<String> {
    Ok(format!("Created user: {}", user.name))
}
```

### Parameter Descriptions

Add descriptions to parameters for better schema documentation:

```rust
#[tool("Process payment")]
async fn process_payment(
    &self,
    #[description("Amount in cents")]
    amount: u64,
    #[description("Currency code (USD, EUR, etc.)")]
    currency: String,
    #[description("Optional payment method ID")]
    payment_method: Option<String>
) -> McpResult<String> {
    Ok(format!("Processed {} {}", amount, currency))
}
```

## Return Types

### McpResult

All handlers must return `McpResult<T>`:

```rust
type McpResult<T> = Result<T, McpError>;
```

### McpError

Standard error types:

```rust
use turbomcp::McpError;

// Invalid input from client
Err(McpError::InvalidInput("Missing required field".into()))

// Internal server error
Err(McpError::InternalError("Database connection failed".into()))

// Method not found
Err(McpError::MethodNotFound("Unknown tool".into()))

// Parse error
Err(McpError::ParseError("Invalid JSON".into()))

// Custom error with code
Err(McpError::Custom {
    code: -32001,
    message: "Rate limit exceeded".into(),
    data: None,
})
```

### Error Conversion

Convert standard errors to McpError:

```rust
#[tool]
async fn read_file(&self, path: String) -> McpResult<String> {
    std::fs::read_to_string(&path)
        .map_err(|e| McpError::InternalError(format!("Failed to read file: {}", e)))
}
```

## State Management

### Stateless Servers

Simple servers with no internal state:

```rust
#[derive(Clone)]
struct StatelessServer;

#[turbomcp::server(name = "stateless", version = "1.0.0")]
impl StatelessServer {
    #[tool]
    async fn pure_function(&self, x: i32) -> McpResult<i32> {
        Ok(x * 2)
    }
}
```

### Stateful Servers

Manage shared state with `Arc<RwLock<T>>`:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct StatefulServer {
    cache: Arc<RwLock<HashMap<String, String>>>,
}

#[turbomcp::server(name = "stateful", version = "1.0.0")]
impl StatefulServer {
    fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[tool("Store a value")]
    async fn set(&self, key: String, value: String) -> McpResult<String> {
        let mut cache = self.cache.write().await;
        cache.insert(key.clone(), value);
        Ok(format!("Stored: {}", key))
    }

    #[tool("Retrieve a value")]
    async fn get(&self, key: String) -> McpResult<Option<String>> {
        let cache = self.cache.read().await;
        Ok(cache.get(&key).cloned())
    }
}
```

### Database Connections

Manage database connection pools:

```rust
use sqlx::{PgPool, Pool, Postgres};

#[derive(Clone)]
struct DatabaseServer {
    pool: PgPool,
}

#[turbomcp::server(name = "db-server", version = "1.0.0")]
impl DatabaseServer {
    async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    #[tool("Query users")]
    async fn get_users(&self) -> McpResult<Vec<String>> {
        let rows = sqlx::query!("SELECT name FROM users")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| McpError::InternalError(e.to_string()))?;

        Ok(rows.into_iter().map(|r| r.name).collect())
    }
}
```

## Configuration

### Server Configuration

Configure server behavior at startup:

```rust
use turbomcp::ServerConfig;

#[derive(Clone)]
struct ConfiguredServer {
    config: ServerConfig,
}

#[turbomcp::server(name = "configured", version = "1.0.0")]
impl ConfiguredServer {
    fn new() -> Self {
        Self {
            config: ServerConfig {
                max_request_size: 10 * 1024 * 1024, // 10MB
                timeout: std::time::Duration::from_secs(30),
                enable_cors: true,
                log_level: "info".to_string(),
            },
        }
    }
}
```

### Environment Variables

Load configuration from environment:

```rust
use std::env;

#[derive(Clone)]
struct EnvServer {
    api_key: String,
    base_url: String,
}

impl EnvServer {
    fn from_env() -> Result<Self, env::VarError> {
        Ok(Self {
            api_key: env::var("API_KEY")?,
            base_url: env::var("BASE_URL").unwrap_or_else(|_| {
                "https://api.example.com".to_string()
            }),
        })
    }
}
```

## Middleware and Hooks

### Lifecycle Hooks

Implement server lifecycle hooks:

```rust
#[turbomcp::server(name = "hooks", version = "1.0.0")]
impl MyServer {
    /// Called before server starts
    async fn on_startup(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Server starting...");
        Ok(())
    }

    /// Called before server shuts down
    async fn on_shutdown(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Server shutting down...");
        Ok(())
    }
}
```

### Request Middleware

Process requests before they reach handlers:

```rust
use turbomcp::RequestContext;

#[derive(Clone)]
struct MiddlewareServer;

#[turbomcp::server(name = "middleware", version = "1.0.0")]
impl MiddlewareServer {
    /// Process all requests
    async fn middleware(&self, ctx: &RequestContext) -> McpResult<()> {
        // Validate authentication
        if let Some(token) = ctx.headers().get("Authorization") {
            if !validate_token(token) {
                return Err(McpError::Unauthorized);
            }
        }
        Ok(())
    }
}
```

## Advanced Features

### Async Tool Execution

Execute long-running operations asynchronously:

```rust
use tokio::time::{sleep, Duration};

#[tool("Long running operation")]
async fn long_operation(&self) -> McpResult<String> {
    sleep(Duration::from_secs(10)).await;
    Ok("Operation completed".to_string())
}
```

### Streaming Responses

Stream large datasets efficiently:

```rust
use futures::stream::{self, StreamExt};

#[tool("Stream large dataset")]
async fn stream_data(&self) -> McpResult<Vec<String>> {
    let data: Vec<String> = stream::iter(0..1000)
        .map(|i| format!("Item {}", i))
        .collect()
        .await;
    Ok(data)
}
```

### Concurrent Operations

Execute multiple operations concurrently:

```rust
use tokio::try_join;

#[tool("Fetch multiple resources")]
async fn fetch_all(&self) -> McpResult<String> {
    let (result1, result2, result3) = try_join!(
        fetch_resource("resource1"),
        fetch_resource("resource2"),
        fetch_resource("resource3")
    )?;

    Ok(format!("{}, {}, {}", result1, result2, result3))
}
```

## Testing

### Unit Testing Handlers

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add() {
        let server = Calculator;
        let result = server.add(2.0, 3.0).await.unwrap();
        assert_eq!(result, 5.0);
    }

    #[tokio::test]
    async fn test_error_handling() {
        let server = FileServer;
        let result = server.read_file("/nonexistent".to_string()).await;
        assert!(result.is_err());
    }
}
```

### Integration Testing

```rust
#[cfg(test)]
mod integration_tests {
    use turbomcp_client::prelude::*;

    #[tokio::test]
    async fn test_server_integration() {
        // Start server in background
        tokio::spawn(async {
            MyServer.run_stdio().await.unwrap();
        });

        // Connect client
        let transport = StdioTransport::new();
        let client = Client::new(transport);

        // Test operations
        client.initialize().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert!(!tools.is_empty());
    }
}
```

## Best Practices

### 1. Use Descriptive Names and Documentation

```rust
// Good
#[tool(description = "Searches the filesystem for files matching a glob pattern")]
async fn search_files(
    &self,
    #[description("Glob pattern (e.g., '*.rs', 'src/**/*.txt')")]
    pattern: String
) -> McpResult<Vec<String>> { }

// Avoid
#[tool]
async fn search(&self, p: String) -> McpResult<Vec<String>> { }
```

### 2. Handle Errors Gracefully

```rust
// Good
#[tool]
async fn process(&self, data: String) -> McpResult<String> {
    validate_input(&data)?;

    match perform_operation(&data).await {
        Ok(result) => Ok(result),
        Err(e) => Err(McpError::InternalError(
            format!("Operation failed: {}", e)
        ))
    }
}

// Avoid
#[tool]
async fn process(&self, data: String) -> McpResult<String> {
    Ok(perform_operation(&data).await.unwrap())
}
```

### 3. Use Appropriate Types

```rust
// Good - Strong types
#[derive(Serialize, Deserialize)]
struct SearchOptions {
    case_sensitive: bool,
    max_results: usize,
    include_hidden: bool,
}

#[tool]
async fn search(&self, query: String, options: SearchOptions) -> McpResult<Vec<String>> { }

// Avoid - Weak types
#[tool]
async fn search(&self, query: String, opts: HashMap<String, String>) -> McpResult<Vec<String>> { }
```

### 4. Minimize Lock Contention

```rust
// Good - Short critical sections
#[tool]
async fn update(&self, key: String, value: String) -> McpResult<()> {
    let mut cache = self.cache.write().await;
    cache.insert(key, value);
    drop(cache); // Release lock immediately
    Ok(())
}

// Avoid - Long critical sections
#[tool]
async fn update(&self, key: String, value: String) -> McpResult<()> {
    let mut cache = self.cache.write().await;
    cache.insert(key, value);
    perform_expensive_operation().await; // Holding lock!
    Ok(())
}
```

### 5. Implement Proper Logging

```rust
#[tool]
async fn critical_operation(&self, data: String) -> McpResult<String> {
    tracing::info!("Starting critical operation");

    match process_data(&data).await {
        Ok(result) => {
            tracing::info!("Operation succeeded");
            Ok(result)
        }
        Err(e) => {
            tracing::error!("Operation failed: {}", e);
            Err(McpError::InternalError(e.to_string()))
        }
    }
}
```

## Troubleshooting

### "Cannot find macro 'tool'"

Ensure you've imported the prelude:

```rust
use turbomcp::prelude::*;
```

### "Server does not implement Clone"

The server struct must implement `Clone`:

```rust
#[derive(Clone)]
struct MyServer {
    // Use Arc for shared state
    state: Arc<RwLock<State>>,
}
```

### "Async trait methods are not supported"

The `#[server]` macro requires direct implementation:

```rust
// Good
#[turbomcp::server(name = "good", version = "1.0.0")]
impl MyServer {
    #[tool]
    async fn handler(&self) -> McpResult<String> { }
}

// Not supported
#[async_trait]
trait MyTrait {
    async fn handler(&self) -> McpResult<String>;
}
```

### "Type does not implement Serialize"

Ensure custom types derive required traits:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CustomType {
    field: String,
}
```

## Next Steps

- **[Client API](client.md)** - Build MCP clients
- **[Macros Reference](macros.md)** - Detailed macro documentation
- **[Context Injection](../guide/context-injection.md)** - Dependency injection guide
- **[Examples](../examples/basic.md)** - Real-world server examples

## See Also

- [MCP Specification](https://modelcontextprotocol.io/specification)
- [API Documentation (docs.rs)](https://docs.rs/turbomcp)
- [Source Code](https://github.com/yourusername/turbomcp)

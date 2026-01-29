# Macros API Reference

Complete reference for TurboMCP procedural macros that enable zero-boilerplate server development.

## Overview

TurboMCP provides procedural macros that automatically generate handler registration, schema generation, and transport integration code. These macros work at compile-time, ensuring zero runtime overhead and full type safety.

## Core Macros

### #[server]

The `#[server]` macro transforms a struct implementation into a fully functional MCP server.

#### Basic Usage

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[turbomcp::server(
    name = "my-server",
    version = "1.0.0"
)]
impl MyServer {
    // Handler methods here
}
```

#### Attributes

| Attribute | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | `&str` | Yes | - | Server identifier |
| `version` | `&str` | Yes | - | Semantic version |
| `transports` | `[&str]` | No | `["stdio"]` | Enabled transports |

#### Transport Options

```rust
#[turbomcp::server(
    name = "multi-transport",
    version = "1.0.0",
    transports = ["stdio", "http", "websocket", "tcp", "unix"]
)]
impl MyServer { }
```

**Available transports:**
- `"stdio"` - Standard input/output
- `"http"` - HTTP with Server-Sent Events
- `"websocket"` - WebSocket
- `"tcp"` - TCP sockets
- `"unix"` - Unix domain sockets

#### Generated Code

The macro generates:

1. **Transport methods:**
   ```rust
   async fn run_stdio(&self) -> Result<(), Box<dyn std::error::Error>>;
   async fn run_http(&self, port: u16) -> Result<(), Box<dyn std::error::Error>>;
   async fn run_websocket(&self, port: u16) -> Result<(), Box<dyn std::error::Error>>;
   async fn run_tcp(&self, addr: &str) -> Result<(), Box<dyn std::error::Error>>;
   async fn run_unix(&self, path: &str) -> Result<(), Box<dyn std::error::Error>>;
   ```

2. **Handler registry:**
   - Automatic registration of all `#[tool]`, `#[resource]`, and `#[prompt]` methods
   - Request routing and dispatch logic
   - Parameter validation and conversion

3. **Schema generation:**
   - JSON Schema for all tool parameters
   - Type-safe serialization/deserialization
   - Compile-time validation

#### Example

```rust
#[derive(Clone)]
struct Calculator;

#[turbomcp::server(name = "calculator", version = "1.0.0")]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a * b)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Calculator.run_stdio().await?;
    Ok(())
}
```

### #[tool]

The `#[tool]` macro marks a method as a tool handler and generates schema.

#### Basic Usage

```rust
#[tool]
async fn my_tool(&self, param: String) -> McpResult<String> {
    Ok(format!("Processed: {}", param))
}
```

#### With Description

```rust
#[tool(description = "Searches files matching a pattern")]
async fn search_files(&self, pattern: String) -> McpResult<Vec<String>> {
    // Implementation
    Ok(vec![])
}
```

#### Parameter Documentation

```rust
#[tool(description = "Create a user account")]
async fn create_user(
    &self,
    #[description("User's full name")]
    name: String,
    #[description("Email address")]
    email: String,
    #[description("Optional phone number")]
    phone: Option<String>
) -> McpResult<String> {
    Ok(format!("Created user: {}", name))
}
```

#### Supported Parameter Types

**Primitives:**
```rust
#[tool]
async fn primitives(
    &self,
    bool_param: bool,
    int_param: i32,
    float_param: f64,
    string_param: String
) -> McpResult<String> { Ok("Done".into()) }
```

**Collections:**
```rust
#[tool]
async fn collections(
    &self,
    vec_param: Vec<String>,
    optional_param: Option<i32>,
    map_param: std::collections::HashMap<String, i32>
) -> McpResult<String> { Ok("Done".into()) }
```

**Custom Types:**
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct User {
    name: String,
    email: String,
}

#[tool]
async fn process_user(&self, user: User) -> McpResult<String> {
    Ok(format!("Processing {}", user.name))
}
```

#### Generated Schema

The macro automatically generates JSON Schema:

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "description": "User's full name"
    },
    "email": {
      "type": "string",
      "description": "Email address"
    },
    "phone": {
      "type": ["string", "null"],
      "description": "Optional phone number"
    }
  },
  "required": ["name", "email"]
}
```

### #[resource]

The `#[resource]` macro marks a method as a resource handler.

**Syntax:**
- `#[resource("uri://template")]` - URI template (required)
- `#[resource("uri://template", mime_type = "application/json")]` - With MIME type

**Note:** Unlike `#[tool]` and `#[prompt]`, the resource macro takes a URI template as
its first argument (not a description). Use doc comments for the description.

#### Basic Usage

```rust
/// Application configuration
#[resource("config://app")]
async fn get_config(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
    Ok(ResourceResult::text(&uri, r#"{"setting": "value"}"#))
}
```

#### With MIME Type

```rust
/// Application configuration file
#[resource("config://app", mime_type = "application/json")]
async fn get_config(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
    Ok(ResourceResult::text(&uri, r#"{"setting": "value"}"#))
}
```

#### URI Templates

Resources use URI templates for dynamic content:

```rust
/// Read a file by path
#[resource("file://{path}")]
async fn read_file(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
    // Extract path from uri: "file:///home/user/file.txt" -> "/home/user/file.txt"
    let path = uri.strip_prefix("file://").unwrap_or(&uri);
    let content = tokio::fs::read_to_string(path).await?;
    Ok(ResourceResult::text(&uri, content))
}
```

#### Resource Result Types

**Text Content:**
```rust
#[resource("file://{path}")]
async fn text_file(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
    let path = uri.strip_prefix("file://").unwrap_or(&uri);
    let content = std::fs::read_to_string(path)?;
    Ok(ResourceResult::text(&uri, content))
}
```

**Binary Content:**
```rust
#[resource("file://{path}")]
async fn binary_file(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
    let path = uri.strip_prefix("file://").unwrap_or(&uri);
    let content = std::fs::read(path)?;
    Ok(ResourceResult::blob(&uri, content, "application/octet-stream"))
}
```

**Dynamic Resources:**
```rust
/// Current server time
#[resource("time://now")]
async fn current_time(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
    use chrono::Utc;
    let now = Utc::now().to_rfc3339();
    Ok(ResourceResult::text(&uri, now))
}
```

### #[prompt]

The `#[prompt]` macro marks a method as a prompt handler.

#### Basic Usage

```rust
#[prompt]
async fn greeting(&self, name: String) -> McpResult<PromptResult> {
    Ok(PromptResult::user(format!("Hello, {}!", name)))
}
```

#### With Description

```rust
#[prompt(description = "Generate a code review prompt")]
async fn code_review(
    &self,
    #[description("Programming language")]
    language: String,
    #[description("Code to review")]
    code: String
) -> McpResult<PromptResult> {
    Ok(PromptResult::user(format!(
        "Please review this {} code:\n\n```{}\n{}\n```",
        language, language, code
    )))
}
```

#### Multi-Message Prompts

Use the builder pattern for multi-turn conversations:

```rust
#[prompt]
async fn conversation(&self) -> McpResult<PromptResult> {
    Ok(PromptResult::user("How do I use TurboMCP?")
        .add_assistant("TurboMCP is a Rust SDK for MCP. Here's how to get started...")
        .with_description("A helpful conversation about TurboMCP"))
}
```

For more control, construct messages directly:

```rust
use turbomcp::prelude::*;

#[prompt]
async fn custom_messages(&self) -> McpResult<PromptResult> {
    let messages = vec![
        Message::user("What is the weather?"),
        Message::assistant("I'll check the weather for you."),
    ];
    Ok(PromptResult::new(messages))
}
```

## Advanced Features

### Context Injection

Inject context and services into handlers:

```rust
use turbomcp::{Context, Logger, Cache};

#[tool]
async fn with_context(
    &self,
    ctx: Context,
    logger: Logger,
    cache: Cache,
    data: String
) -> McpResult<String> {
    logger.info("Processing request").await?;
    cache.set("key", "value").await?;
    Ok(format!("Processed: {}", data))
}
```

**Available Injectable Types:**
- `Context` - Full request context
- `RequestInfo` - Request metadata
- `Logger` - Structured logging
- `Cache` - In-memory caching
- `Config` - Configuration access
- `Database` - Database connections
- `HttpClient` - HTTP client

### Validation Attributes

Add validation to parameters:

```rust
#[tool]
async fn validated_tool(
    &self,
    #[validate(min_length = 1, max_length = 100)]
    name: String,
    #[validate(range(min = 0, max = 150))]
    age: u8,
    #[validate(email)]
    email: String
) -> McpResult<String> {
    Ok("Valid input".to_string())
}
```

### Default Values

Specify default values for optional parameters:

```rust
#[tool]
async fn with_defaults(
    &self,
    required: String,
    #[default = "default_value"]
    optional: Option<String>
) -> McpResult<String> {
    let value = optional.unwrap_or_else(|| "default_value".to_string());
    Ok(format!("{}: {}", required, value))
}
```

### Async Traits

Use async trait methods:

```rust
#[turbomcp::server(name = "async-server", version = "1.0.0")]
impl MyServer {
    #[tool]
    async fn async_operation(&self) -> McpResult<String> {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok("Done".to_string())
    }
}
```

## Schema Generation

### Automatic Schema

The macros automatically generate JSON Schema from Rust types:

```rust
#[derive(Serialize, Deserialize)]
struct Address {
    street: String,
    city: String,
    zip: String,
}

#[derive(Serialize, Deserialize)]
struct Person {
    name: String,
    age: u8,
    email: Option<String>,
    address: Address,
}

#[tool]
async fn create_person(&self, person: Person) -> McpResult<String> {
    Ok(format!("Created {}", person.name))
}
```

**Generated schema:**
```json
{
  "type": "object",
  "properties": {
    "person": {
      "type": "object",
      "properties": {
        "name": {"type": "string"},
        "age": {"type": "integer", "minimum": 0, "maximum": 255},
        "email": {"type": ["string", "null"]},
        "address": {
          "type": "object",
          "properties": {
            "street": {"type": "string"},
            "city": {"type": "string"},
            "zip": {"type": "string"}
          },
          "required": ["street", "city", "zip"]
        }
      },
      "required": ["name", "age", "address"]
    }
  },
  "required": ["person"]
}
```

### Custom Schema Annotations

Use schemars attributes for custom schema:

```rust
use schemars::JsonSchema;

#[derive(Serialize, Deserialize, JsonSchema)]
struct CustomType {
    #[schemars(range(min = 0, max = 100))]
    percentage: f64,

    #[schemars(regex = "^[A-Z]{2}$")]
    country_code: String,

    #[schemars(url)]
    website: String,
}
```

## Compilation

### Macro Expansion

View generated code with `cargo expand`:

```bash
cargo install cargo-expand
cargo expand --lib
```

### Build-Time Validation

Macros perform compile-time validation:

```rust
// ❌ Compile error: missing name
#[turbomcp::server(version = "1.0.0")]
impl MyServer { }

// ❌ Compile error: invalid transport
#[turbomcp::server(name = "test", version = "1.0.0", transports = ["invalid"])]
impl MyServer { }

// ❌ Compile error: return type must be McpResult
#[tool]
async fn bad_return(&self) -> String {  // Error!
    "value".to_string()
}
```

## Best Practices

### 1. Document All Handlers

```rust
// Good
#[tool(description = "Comprehensive description of what this tool does")]
async fn well_documented(
    &self,
    #[description("Clear parameter description")]
    param: String
) -> McpResult<String> { }

// Avoid
#[tool]
async fn undocumented(&self, p: String) -> McpResult<String> { }
```

### 2. Use Strong Types

```rust
// Good
#[derive(Serialize, Deserialize)]
struct SearchOptions {
    case_sensitive: bool,
    max_results: usize,
}

#[tool]
async fn search(&self, query: String, options: SearchOptions) -> McpResult<Vec<String>> { }

// Avoid
#[tool]
async fn search(&self, query: String, case_sensitive: bool, max_results: usize) -> McpResult<Vec<String>> { }
```

### 3. Handle Errors Properly

```rust
// Good
#[tool]
async fn safe_operation(&self, input: String) -> McpResult<String> {
    validate_input(&input)?;
    perform_operation(&input)
        .await
        .map_err(|e| McpError::InternalError(e.to_string()))
}

// Avoid
#[tool]
async fn unsafe_operation(&self, input: String) -> McpResult<String> {
    Ok(perform_operation(&input).await.unwrap())
}
```

### 4. Keep Handlers Focused

```rust
// Good - Single responsibility
#[tool("Validate email format")]
async fn validate_email(&self, email: String) -> McpResult<bool> { }

#[tool("Send email")]
async fn send_email(&self, to: String, subject: String, body: String) -> McpResult<String> { }

// Avoid - Too many responsibilities
#[tool("Validate and send email")]
async fn validate_and_send(&self, email: String, subject: String, body: String) -> McpResult<String> { }
```

### 5. Use Appropriate Handler Types

```rust
// Good - Using correct handler type
/// Get application configuration
#[resource("config://app")]
async fn get_config(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
    Ok(ResourceResult::text(&uri, r#"{"setting": "value"}"#))
}

#[tool(description = "Update configuration")]
async fn update_config(&self, config: String) -> McpResult<String> { }

// Avoid - Wrong handler type
#[tool(description = "Get configuration")]  // Should be #[resource]
async fn get_config(&self) -> McpResult<String> { }
```

## Troubleshooting

### "Cannot find attribute macro"

Import the prelude:

```rust
use turbomcp::prelude::*;
```

### "Expected McpResult"

All handlers must return `McpResult<T>`:

```rust
// Wrong
#[tool]
async fn handler(&self) -> String { }

// Correct
#[tool]
async fn handler(&self) -> McpResult<String> { }
```

### "Type does not implement Serialize"

Custom types need derive macros:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CustomType {
    field: String,
}
```

### "Server does not implement Clone"

The server struct must be `Clone`:

```rust
#[derive(Clone)]
struct MyServer {
    // Use Arc for shared state
    state: Arc<RwLock<State>>,
}
```

## Examples

### Complete Server with All Handler Types

```rust
use turbomcp::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct FullServer {
    cache: Arc<RwLock<HashMap<String, String>>>,
}

#[turbomcp::server(
    name = "full-server",
    version = "1.0.0",
    transports = ["stdio", "http"]
)]
impl FullServer {
    fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[tool(description = "Store a value in cache")]
    async fn set(
        &self,
        #[description("Cache key")]
        key: String,
        #[description("Value to store")]
        value: String
    ) -> McpResult<String> {
        let mut cache = self.cache.write().await;
        cache.insert(key.clone(), value);
        Ok(format!("Stored key: {}", key))
    }

    #[tool(description = "Get a value from cache")]
    async fn get(
        &self,
        #[description("Cache key")]
        key: String
    ) -> McpResult<Option<String>> {
        let cache = self.cache.read().await;
        Ok(cache.get(&key).cloned())
    }

    /// List all cached keys
    #[resource("cache://keys")]
    async fn list_keys(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        let cache = self.cache.read().await;
        let keys: Vec<String> = cache.keys().cloned().collect();
        Ok(ResourceResult::json(&uri, &keys)?)
    }

    #[prompt(description = "Generate cache query prompt")]
    async fn query_prompt(
        &self,
        #[description("Key to query")]
        key: String
    ) -> McpResult<PromptResult> {
        Ok(PromptResult::user(format!("What is the value of cache key '{}'?", key)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    FullServer::new().run_stdio().await?;
    Ok(())
}
```

## Next Steps

- **[Server API](server.md)** - Complete server reference
- **[Context Injection](../guide/context-injection.md)** - Dependency injection guide
- **[Examples](../examples/basic.md)** - Real-world examples

## See Also

- [Procedural Macros - The Rust Book](https://doc.rust-lang.org/book/ch19-06-macros.html)
- [API Documentation (docs.rs)](https://docs.rs/turbomcp)
- [Source Code](https://github.com/yourusername/turbomcp)

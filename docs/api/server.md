# Server API Reference

Complete API reference for building MCP servers with TurboMCP.

## Overview

The TurboMCP server API provides a high-level framework for building Model Context Protocol servers with minimal boilerplate. The framework automatically handles request routing, schema generation, and transport protocol management.

A server is any type that implements `McpHandler`. The `#[server]` macro
generates that implementation from an `impl` block; `turbomcp-server` then
provides the ways to run it:

| API | Crate | Purpose |
|-----|-------|---------|
| `#[server]`, `#[tool]`, `#[resource]`, `#[prompt]`, … | `turbomcp` (from `turbomcp-macros`) | Generate `McpHandler` |
| `McpHandlerExt` | `turbomcp-server` (in the `turbomcp` prelude) | `run_stdio`, `run_http`, `run_websocket`, `run_tcp`, `run_unix`, `handle_request` |
| `McpServerExt` / `ServerBuilder` | `turbomcp-server` (in the prelude) | Transport choice and configuration at runtime |
| `ServerConfig`, `ProtocolConfig` | `turbomcp-server` (in the prelude) | Limits, origin policy, versions, sessions, authorization |
| `McpMiddleware`, `MiddlewareStack` | `turbomcp-server` | Typed middleware |
| `CompositeHandler`, `VisibilityLayer` | `turbomcp-server` (`VisibilityLayer` is in the prelude) | Composition and progressive disclosure |

## Core Types

### The `#[server]` Macro

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(
    name = "my-server",
    version = "1.0.0",
    description = "What this server is",
    instructions = "How a client should use it"
)]
impl MyServer {
    /// Say hello
    #[tool]
    async fn hello(&self, name: String) -> String {
        format!("Hello, {name}!")
    }
}
```

#### Server Attributes

| Attribute | Type | Required | Description |
|-----------|------|----------|-------------|
| `name` | expression | No | Server name (defaults to the type name) |
| `version` | expression | No | Version string (defaults to `"1.0.0"`) |
| `description` | expression | No | What this implementation is |
| `title` | expression | No | Human-readable display name |
| `instructions` | expression | No | Returned in the `initialize` result |
| `website_url` | expression | No | Homepage |
| `icons` | `[expression, …]` | No | Icon source URIs |
| `page_size` | expression | No | Paginate the list methods at this many entries |

Each value is an expression, so `version = env!("CARGO_PKG_VERSION")` works.
An unknown attribute is a compile error. The removed `transports = [...]`
attribute is rejected with a message pointing at Cargo features: transports
are chosen by feature flags and at runtime, not by the macro.

#### Run Methods

`run_*` methods are not generated per server: they come from the
`McpHandlerExt` trait, implemented for every `McpHandler`, each behind its
transport's Cargo feature:

```rust
use std::future::Future;
use turbomcp::McpResult;

// Shape of turbomcp_server::McpHandlerExt (feature gates in brackets)
trait RunMethods {
    fn run(&self) -> impl Future<Output = McpResult<()>>;                      // [stdio]
    fn run_stdio(&self) -> impl Future<Output = McpResult<()>>;                // [stdio]
    fn run_http(&self, addr: &str) -> impl Future<Output = McpResult<()>>;     // [http]
    fn run_websocket(&self, addr: &str) -> impl Future<Output = McpResult<()>>; // [websocket]
    fn run_tcp(&self, addr: &str) -> impl Future<Output = McpResult<()>>;      // [tcp]
    fn run_unix(&self, path: &str) -> impl Future<Output = McpResult<()>>;     // [unix]
}
```

Each `run_*` method uses the default `ServerConfig`. For custom
configuration, use the builder below.

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

### ServerBuilder

`handler.builder()` (from `McpServerExt`) returns a `ServerBuilder` for
choosing the transport at runtime and applying configuration:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> f64 {
        a + b
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    let transport = match std::env::var("TRANSPORT").as_deref() {
        Ok("http") => Transport::http("0.0.0.0:8080"),
        Ok("tcp") => Transport::tcp("0.0.0.0:9000"),
        _ => Transport::stdio(),
    };

    Calculator
        .builder()
        .transport(transport)
        .with_rate_limit(100, Duration::from_secs(1))
        .with_connection_limit(1000)
        .with_graceful_shutdown(Duration::from_secs(30))
        .serve()
        .await
}
```

`into_axum_router()` and `into_service()` (feature `http`) return the MCP
endpoint as an Axum router or Tower service to mount in an existing
application; the router serves `/`, `/mcp`, and `/sse`.

## Handler Types

TurboMCP supports three types of MCP handlers: tools, resources, and prompts, plus
markers for the optional MCP methods. The [Macros Reference](macros.md) covers
every attribute; the signatures are:

| Handler | Signature | Returns |
|---------|-----------|---------|
| `#[tool]` | `(&self, <args…>)`, with an optional `ctx: &RequestContext` anywhere | any `IntoToolResult` (`String`, numbers, `Json<T>`, `ToolResult`, `McpResult<…>`) |
| `#[resource("uri")]` | `(&self, uri: String, ctx: &RequestContext)` | `McpResult<T>` where `T: IntoResourceResult` |
| `#[prompt]` | `(&self, <String / Option<String> args…>, ctx: &RequestContext)` | any `IntoPromptResult` (`String`, `PromptResult`), or a `Result` of one |
| `#[completion]` | `(&self, params: serde_json::Value[, ctx])` | `McpResult<serde_json::Value>` |
| `#[subscribe]` / `#[unsubscribe]` | `(&self, uri: String[, ctx])` | `McpResult<()>` |
| `#[set_level]` | `(&self, level: String[, ctx])` | `McpResult<()>` |
| `#[roots_changed]` | `(&self[, ctx])` | `McpResult<()>` |

### Tool Handlers

Tools are functions that perform actions and return results.

#### Tool Attributes

| Attribute | Type | Description |
|-----------|------|-------------|
| `description` | `"..."` | Tool description (defaults to the doc comment) |
| `title` | `"..."` | Display name |
| `read_only`, `destructive`, `idempotent`, `open_world` | `bool` | `ToolAnnotations` hints |
| `output_schema` | type | Output schema (inferred for a `Json<T>` return) |
| `task_support` | `"forbidden"` \| `"optional"` \| `"required"` | `execution.taskSupport` |
| `tags`, `version`, `icons` | | Metadata |

#### Example: Tool with Description

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Files;

#[server]
impl Files {
    #[tool(description = "Searches for files matching a pattern", read_only = true)]
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
}
```

### Resource Handlers

Resources provide read-only access to data or content. The first attribute
argument is a URI or an RFC 6570 URI template; the handler receives the full
requested URI.

#### Resource Types

Return a `String` for text, or build a `ResourceResult` for other content:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Assets;

#[server]
impl Assets {
    /// Application configuration
    #[resource("config://app", mime_type = "application/json")]
    async fn get_config(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"debug": true}"#.to_string())
    }

    /// Structured JSON, serialized for you
    #[resource("status://server")]
    async fn get_status(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        ResourceResult::json(&uri, &serde_json::json!({"healthy": true}))
            .map_err(|e| McpError::internal(e.to_string()))
    }

    /// Binary content is base64-encoded blob contents
    #[resource("image://logo", mime_type = "image/png")]
    async fn get_image(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        let base64_png = "iVBORw0KGgo="; // encode the file's bytes with the base64 crate
        Ok(ResourceResult::binary(uri, base64_png, "image/png"))
    }
}
```

### Prompt Handlers

Prompts provide templated text for LLM interactions.

#### Example: Prompt Handler

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Reviewer;

#[server]
impl Reviewer {
    #[prompt(description = "Generate code review prompt")]
    async fn code_review(
        &self,
        #[description("Programming language")]
        language: String,
        #[description("Code to review")]
        code: String,
        ctx: &RequestContext,
    ) -> McpResult<PromptResult> {
        Ok(PromptResult::user(format!(
            "Please review this {} code:\n\n{}",
            language, code
        )))
    }

    /// A multi-message prompt, using the builder methods
    #[prompt]
    async fn analysis(&self, ctx: &RequestContext) -> PromptResult {
        PromptResult::user("Initial context")
            .add_assistant("I understand. What would you like me to do?")
            .add_user("Please analyze this data")
            .with_description("A multi-turn conversation prompt")
    }
}
```

## Parameter Types

### Supported Parameter Types

Tool parameters can be any type that implements `serde::Deserialize` and
`schemars::JsonSchema`:

**Primitives:**
- `bool`, `i8`, `i16`, `i32`, `i64`, `i128`
- `u8`, `u16`, `u32`, `u64`, `u128`
- `f32`, `f64`
- `String`
- `char`

**Collections:**
- `Vec<T>`
- `HashMap<K, V>`
- `HashSet<T>`
- `Option<T>` (the argument becomes optional)

**Custom Types:**
- Any type implementing `Deserialize` and `JsonSchema`

Prompt arguments are always strings: `String` for a required argument,
`Option<String>` for an optional one.

### Custom Type Example

```rust
use serde::Deserialize;
use turbomcp::prelude::*;

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[derive(Clone)]
struct Users;

#[server]
impl Users {
    #[tool("Create a new user")]
    async fn create_user(&self, user: User) -> McpResult<String> {
        Ok(format!("Created user: {}", user.name))
    }
}
```

### Parameter Descriptions

Add descriptions to parameters for better schema documentation:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Payments;

#[server]
impl Payments {
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
}
```

## Return Types

### McpResult

Fallible handlers return `McpResult<T>`:

```rust
use turbomcp::McpError;

type McpResult<T> = Result<T, McpError>;
```

A tool's `Err` is reported to the client as a tool execution error
(`isError: true`, with the error kind in `_meta`) so the model can read it and
correct its call. A resource's or prompt's `Err` is a JSON-RPC error.

### McpError

`McpError` is a struct with an `ErrorKind`, built with constructors:

```rust
use turbomcp::McpError;

let errors = [
    // Invalid input from client
    McpError::invalid_params("Missing required field"),
    // Internal server error
    McpError::internal("Database connection failed"),
    // Unknown tool
    McpError::tool_not_found("unknown_tool"),
    // Parse error
    McpError::parse_error("Invalid JSON"),
    // Rate limit exceeded
    McpError::rate_limited("Too many requests"),
    // Extra data for the client
    McpError::invalid_params("Bad date").with_data(serde_json::json!({"field": "date"})),
];
```

### Error Conversion

Convert standard errors to McpError:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Files;

#[server]
impl Files {
    #[tool]
    async fn read_file(&self, path: String) -> McpResult<String> {
        std::fs::read_to_string(&path)
            .map_err(|e| McpError::internal(format!("Failed to read file: {}", e)))
    }
}
```

## State Management

### Stateless Servers

Simple servers with no internal state:

```rust
use turbomcp::prelude::*;

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

Manage shared state with `Arc<RwLock<T>>`. Methods without a handler
attribute are left alone, so constructors can live in the same block:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

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

Database pools are cheap to clone, so they can be fields directly. With
`sqlx` (not a TurboMCP dependency):

```rust,ignore
use sqlx::PgPool;
use turbomcp::prelude::*;

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
        let names: Vec<(String,)> = sqlx::query_as("SELECT name FROM users")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| McpError::internal(e.to_string()))?;

        Ok(names.into_iter().map(|(name,)| name).collect())
    }
}
```

This block is marked `ignore` only because `sqlx` is not in the docs' build.

## Configuration

### Server Configuration

`ServerConfig` covers what the transports enforce. Build it with
`ServerConfig::builder()` and pass it to `ServerBuilder::with_config`:

```rust
use std::time::Duration;
use turbomcp::prelude::*;
use turbomcp_server::RateLimitConfig;

#[derive(Clone)]
struct ConfiguredServer;

#[turbomcp::server(name = "configured", version = "1.0.0")]
impl ConfiguredServer {
    #[tool]
    async fn ping(&self) -> String {
        "pong".to_string()
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    let config = ServerConfig::builder()
        .max_message_size(10 * 1024 * 1024) // 10MB
        .rate_limit(RateLimitConfig::new(100, Duration::from_secs(1)))
        .allow_origin("https://app.example.com")
        .cors(true)
        .protocol(ProtocolConfig::multi_version())
        .build();

    ConfiguredServer
        .builder()
        .transport(Transport::http("0.0.0.0:8080"))
        .with_config(config)
        .serve()
        .await
}
```

| Setting | Default |
|---------|---------|
| `max_message_size` | 10 MB |
| `rate_limit` | none |
| `connection_limits` | 1000 per transport |
| `protocol` | `2025-06-18` and `2025-11-25`, preferring `2025-11-25`; an unsupported request is offered the preferred version |
| origin validation | loopback origins allowed, others must be listed; `allow_missing_origin = false`; `cors = false` |
| `http_sessions` | 1 hour idle timeout, 10,000 sessions |
| `authorization` | none |

A request with no `Origin` header from a non-loopback address is refused
unless `allow_missing_origin(true)` is set, so a networked server for
non-browser clients needs it (paired with authorization).

`ServerBuilder::with_config` applies the protocol, rate limit, connection
limits, required capabilities, message size, and origin settings. Run a server
that needs `authorization` or a custom `http_sessions` policy with
`turbomcp_server::transport::http::run_with_config(&handler, addr, &config)`.

### Authorization

With the `http` feature, `ServerConfig::builder().authorization(...)` makes
the Streamable HTTP transport an OAuth 2.1 protected resource: it serves RFC
9728 metadata, answers requests without a valid bearer token `401` with a
`WWW-Authenticate` challenge, and puts the validated principal on each
request's context. See the [Authentication guide](../guide/authentication.md)
for `JwtBearerValidator` and custom validators.

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

`McpHandler` has `on_initialize` and `on_shutdown` hooks, which every
transport runner calls around serving. A `#[server]` block
generates the `McpHandler` impl, so it cannot override them; to run code at
startup, run it before calling `run_*`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server(name = "hooks", version = "1.0.0")]
impl MyServer {
    #[tool]
    async fn ping(&self) -> String {
        "pong".to_string()
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    eprintln!("Server starting...");
    let result = MyServer.run_stdio().await;
    eprintln!("Server shutting down...");
    result
}
```

### Request Middleware

Process requests before they reach handlers with `McpMiddleware`. A
`MiddlewareStack` wraps a handler and is itself an `McpHandler`:

```rust
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use turbomcp::prelude::*;
use turbomcp_server::{McpMiddleware, MiddlewareStack, Next};

#[derive(Clone)]
struct MiddlewareServer;

#[turbomcp::server(name = "middleware", version = "1.0.0")]
impl MiddlewareServer {
    #[tool]
    async fn secret(&self) -> String {
        "42".to_string()
    }
}

/// Refuse tool calls from unauthenticated requests.
struct RequireAuth;

impl McpMiddleware for RequireAuth {
    fn on_call_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = McpResult<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            if !ctx.is_authenticated() {
                return Err(McpError::permission_denied("authentication required"));
            }
            next.call_tool(name, args, ctx).await
        })
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    MiddlewareStack::new(MiddlewareServer)
        .with_middleware(RequireAuth)
        .run_stdio()
        .await
}
```

The other hooks (`on_list_tools`, `on_read_resource`, `on_get_prompt`, …)
default to passing the request through.

### Composition

`CompositeHandler` mounts several handlers under prefixes. Tools and prompts
become `{prefix}_{name}`:

```rust
use turbomcp::prelude::*;
use turbomcp_server::CompositeHandler;

#[derive(Clone)]
struct Weather;

#[server(name = "weather", version = "1.0.0")]
impl Weather {
    #[tool]
    async fn forecast(&self, city: String) -> String {
        format!("Sunny in {city}")
    }
}

#[derive(Clone)]
struct News;

#[server(name = "news", version = "1.0.0")]
impl News {
    #[tool]
    async fn headlines(&self) -> String {
        "Nothing happened".to_string()
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // Tools: weather_forecast, news_headlines. try_mount reports a duplicate
    // prefix as an error; mount panics on one.
    let server = CompositeHandler::new("gateway", "1.0.0")
        .try_mount(Weather, "weather")
        .and_then(|server| server.try_mount(News, "news"))
        .map_err(McpError::internal)?;
    server.run_stdio().await
}
```

## Advanced Features

### Async Tool Execution

Execute long-running operations asynchronously. Check for cancellation so a
client's `notifications/cancelled` takes effect:

```rust
use tokio::time::{sleep, Duration};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Worker;

#[server]
impl Worker {
    #[tool("Long running operation")]
    async fn long_operation(&self, ctx: &RequestContext) -> McpResult<String> {
        for step in 0..10 {
            if ctx.is_cancelled() {
                return Err(McpError::cancelled("cancelled by client"));
            }
            ctx.report_progress(f64::from(step), Some(10.0), None).await?;
            sleep(Duration::from_secs(1)).await;
        }
        Ok("Operation completed".to_string())
    }
}
```

### Concurrent Operations

Execute multiple operations concurrently:

```rust
use tokio::try_join;
use turbomcp::prelude::*;

async fn fetch_resource(name: &str) -> McpResult<String> {
    Ok(format!("{name}: ok"))
}

#[derive(Clone)]
struct Fetcher;

#[server]
impl Fetcher {
    #[tool("Fetch multiple resources")]
    async fn fetch_all(&self) -> McpResult<String> {
        let (result1, result2, result3) = try_join!(
            fetch_resource("resource1"),
            fetch_resource("resource2"),
            fetch_resource("resource3")
        )?;

        Ok(format!("{}, {}, {}", result1, result2, result3))
    }
}
```

## Testing

### Unit Testing Handlers

Handler methods stay ordinary methods, and `McpTestClient` (in the prelude)
dispatches through the generated `McpHandler` without a transport:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_add() {
        let result = Calculator.add(2.0, 3.0).await.unwrap();
        assert_eq!(result, 5.0);
    }

    #[tokio::test]
    async fn test_through_mcp() {
        let client = McpTestClient::new(Calculator);
        client.assert_tool_exists("add");

        let result = client
            .call_tool("add", serde_json::json!({"a": 2.0, "b": 3.0}))
            .await
            .unwrap();
        assert_eq!(result.first_text(), Some("5"));

        // Unknown arguments are rejected as a tool execution error
        let result = client
            .call_tool("add", serde_json::json!({"a": 1, "b": 2, "c": 3}))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
    }
}
```

### Integration Testing

For an end-to-end test over a real transport, serve on a local port and
connect the client (`full-client` and `http` features):

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn ping(&self) -> String {
        "pong".to_string()
    }
}

#[tokio::test]
async fn test_server_integration() {
    tokio::spawn(async { MyServer.run_http("127.0.0.1:18080").await });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let client = Client::connect_http("http://127.0.0.1:18080").await.unwrap();
    let tools = client.list_tools().await.unwrap();
    assert!(!tools.is_empty());
    client.shutdown().await.unwrap();
}
```

## Best Practices

### 1. Use Descriptive Names and Documentation

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Files;

#[server]
impl Files {
    // Good
    #[tool(description = "Searches the filesystem for files matching a glob pattern")]
    async fn search_files(
        &self,
        #[description("Glob pattern (e.g., '*.rs', 'src/**/*.txt')")]
        pattern: String
    ) -> McpResult<Vec<String>> {
        Ok(vec![])
    }

    // Avoid: no description, and an opaque parameter name
    #[tool]
    async fn search(&self, p: String) -> McpResult<Vec<String>> {
        Ok(vec![])
    }
}
```

### 2. Handle Errors Gracefully

```rust
use turbomcp::prelude::*;

fn validate_input(data: &str) -> McpResult<()> {
    if data.is_empty() {
        return Err(McpError::invalid_params("data must not be empty"));
    }
    Ok(())
}

async fn perform_operation(data: &str) -> Result<String, std::io::Error> {
    Ok(data.to_uppercase())
}

#[derive(Clone)]
struct Processor;

#[server]
impl Processor {
    // Good: errors become tool execution errors the model can act on
    #[tool]
    async fn process(&self, data: String) -> McpResult<String> {
        validate_input(&data)?;

        match perform_operation(&data).await {
            Ok(result) => Ok(result),
            Err(e) => Err(McpError::internal(format!("Operation failed: {}", e))),
        }
    }

    // Avoid: a panic gives the client no usable error
    #[tool]
    async fn process_unchecked(&self, data: String) -> McpResult<String> {
        Ok(perform_operation(&data).await.unwrap())
    }
}
```

### 3. Use Appropriate Types

```rust
use std::collections::HashMap;
use serde::Deserialize;
use turbomcp::prelude::*;

// Good - Strong types
#[derive(Deserialize, schemars::JsonSchema)]
struct SearchOptions {
    case_sensitive: bool,
    max_results: usize,
    include_hidden: bool,
}

#[derive(Clone)]
struct Search;

#[server]
impl Search {
    #[tool]
    async fn search(&self, query: String, options: SearchOptions) -> McpResult<Vec<String>> {
        Ok(vec![])
    }

    // Avoid - Weak types
    #[tool]
    async fn search_loose(&self, query: String, opts: HashMap<String, String>) -> McpResult<Vec<String>> {
        Ok(vec![])
    }
}
```

### 4. Minimize Lock Contention

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

async fn perform_expensive_operation() {}

#[derive(Clone, Default)]
struct Cache {
    cache: Arc<RwLock<HashMap<String, String>>>,
}

#[server]
impl Cache {
    // Good - Short critical sections
    #[tool]
    async fn update(&self, key: String, value: String) -> McpResult<()> {
        let mut cache = self.cache.write().await;
        cache.insert(key, value);
        drop(cache); // Release lock immediately
        perform_expensive_operation().await;
        Ok(())
    }

    // Avoid - Long critical sections
    #[tool]
    async fn update_slow(&self, key: String, value: String) -> McpResult<()> {
        let mut cache = self.cache.write().await;
        cache.insert(key, value);
        perform_expensive_operation().await; // Holding lock!
        Ok(())
    }
}
```

### 5. Implement Proper Logging

Use `tracing` for server-side logs (to stderr on a STDIO server). To send log
messages to the client, use `turbomcp_protocol::RichContextExt`
(`ctx.info(...)`), which respects the level the client set.

```rust
use turbomcp::prelude::*;

async fn process_data(data: &str) -> Result<String, std::io::Error> {
    Ok(data.to_string())
}

#[derive(Clone)]
struct Critical;

#[server]
impl Critical {
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
                Err(McpError::internal(e.to_string()))
            }
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
use std::sync::Arc;
use tokio::sync::RwLock;

struct State;

#[derive(Clone)]
struct MyServer {
    // Use Arc for shared state
    state: Arc<RwLock<State>>,
}
```

### "#[server] cannot be used on trait implementations"

The `#[server]` macro applies to an inherent `impl` block, not to a trait
impl:

```rust,ignore
// Good
#[turbomcp::server(name = "good", version = "1.0.0")]
impl MyServer {
    #[tool]
    async fn handler(&self) -> McpResult<String> { Ok(String::new()) }
}

// Not supported: handlers declared on a trait
#[turbomcp::server(name = "bad", version = "1.0.0")]
impl MyTrait for MyServer {
    #[tool]
    async fn handler(&self) -> McpResult<String> { Ok(String::new()) }
}
```

(Marked `ignore` because the second half is the error being illustrated.)

### "the trait bound `T: JsonSchema` is not satisfied"

Custom parameter types need `Deserialize` and `JsonSchema`:

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct CustomType {
    field: String,
}
```

## Next Steps

- **[Client API](client.md)** - Build MCP clients
- **[Macros Reference](macros.md)** - Detailed macro documentation
- **[Context Injection](../guide/context-injection.md)** - Request context guide
- **[Examples](../examples/basic.md)** - Real-world server examples

## See Also

- [MCP Specification](https://modelcontextprotocol.io/specification)
- [API Documentation (docs.rs)](https://docs.rs/turbomcp)
- [Source Code](https://github.com/Epistates/turbomcp)

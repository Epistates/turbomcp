# Error Handling

TurboMCP v3 introduces a unified error handling system with `McpError` - a single error type across the entire SDK.

## Overview

In v3, all error types have been unified into `McpError`:

- **No more `ServerError`** - Use `McpError` instead
- **No more `ClientError`** - Use `McpError` instead
- **The protocol error type is `McpError`** - `turbomcp_protocol::Error` and `turbomcp_client::Error` are re-exports of it
- **Unified result type** - `McpResult<T>` replaces `ServerResult<T>` and `ClientResult<T>`

`McpError` is a struct with a `kind: ErrorKind`, a `message`, optional structured
`data`, and optional context (operation, component, request ID). Match on
`err.kind` rather than on variants.

## Basic Usage

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Process some input.
    #[tool]
    async fn my_tool(&self, input: String) -> McpResult<String> {
        if input.is_empty() {
            return Err(McpError::invalid_params("Input cannot be empty"));
        }
        Ok(format!("Processed: {}", input))
    }
}
```

## How Handler Errors Reach the Client

Where an error goes depends on the kind of handler:

- **Tools** report an `Err` as a *tool execution error*: a successful JSON-RPC
  response whose result has `isError: true` and the message as text, so the model
  can read it and correct its call. The classification is kept in `_meta`:
  `io.turbomcp/errorKind` (the `ErrorKind` in snake_case), `io.turbomcp/errorCode`
  (the JSON-RPC code it maps to), and `io.turbomcp/errorData` when the error carries
  data. Missing, mistyped, or unknown arguments are reported the same way, as
  `invalid_params`. The one exception is `ErrorKind::UrlElicitationRequired`, which
  MCP defines as a JSON-RPC error (`-32042`).
- **Resources and prompts** return the error as a JSON-RPC error.
- **Protocol failures** (unknown method, unknown tool, malformed request) are
  JSON-RPC errors.

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "content": [{ "type": "text", "text": "Input cannot be empty" }],
    "isError": true,
    "_meta": {
      "io.turbomcp/errorKind": "invalid_params",
      "io.turbomcp/errorCode": -32602
    }
  }
}
```

## Error Constructors

`McpError` provides semantic constructors for common error scenarios:

### Protocol Errors

```rust
use turbomcp::McpError;

// Parse error (-32700)
let _ = McpError::parse_error("Invalid JSON");

// Invalid request (-32600)
let _ = McpError::invalid_request("Missing method field");

// Method not found (-32601)
let _ = McpError::method_not_found("unknown_method");

// Invalid params (-32602)
let _ = McpError::invalid_params("Missing required field: name");

// Internal error (-32603)
let _ = McpError::internal("Database connection failed");
```

### MCP-Specific Errors

```rust
use turbomcp::McpError;

// Tool not found (-32602)
let _ = McpError::tool_not_found("calculator");

// Tool failed while running (-32603)
let _ = McpError::tool_execution_failed("calculator", "overflow");

// Resource not found (-32002)
let _ = McpError::resource_not_found("file:///missing.txt");

// Prompt not found (-32602)
let _ = McpError::prompt_not_found("greeting");

// Capability the server or client does not support (-32601)
let _ = McpError::capability_not_supported("completions");

// Request cancelled by the client
let _ = McpError::cancelled("Cancelled by client");
```

### Operational Errors

```rust
use turbomcp::McpError;

let _ = McpError::authentication("Token expired");
let _ = McpError::permission_denied("Requires the admin role");
let _ = McpError::rate_limited("Too many requests");
let _ = McpError::timeout("Upstream took too long");
let _ = McpError::unavailable("Database temporarily unavailable");
let _ = McpError::external_service("Payment provider returned 502");
let _ = McpError::configuration("API_KEY is not set");
let _ = McpError::transport("Connection dropped");
```

## Error Details

Add structured data and context to errors. `with_data` becomes the JSON-RPC
error's `data`, or `io.turbomcp/errorData` on a tool error:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Store;

#[server]
impl Store {
    /// Look up an item.
    #[tool]
    async fn lookup(&self, id: String) -> McpResult<String> {
        if id.parse::<u64>().is_err() {
            return Err(McpError::invalid_params(format!("Expected a numeric id, got {id}"))
                .with_data(serde_json::json!({ "field": "id", "expected": "integer" }))
                .with_operation("lookup"));
        }
        Ok(format!("item {id}"))
    }
}
```

`with_operation`, `with_component`, and `with_request_id` add context that shows
up in the error's `Display` output and your logs; the message sent to a model on a
tool error is the bare message.

## JSON-RPC Error Codes

`McpError` maps each kind to a JSON-RPC 2.0 error code (`err.jsonrpc_code()`):

| Error Kind | JSON-RPC Code | Description |
|------------|---------------|-------------|
| `ParseError` | -32700 | Invalid JSON received |
| `InvalidRequest` | -32600 | Not a valid request object |
| `MethodNotFound`, `CapabilityNotSupported` | -32601 | Method does not exist or is not supported |
| `InvalidParams`, `ToolNotFound`, `PromptNotFound` | -32602 | Invalid method parameters |
| `Internal`, `ToolExecutionFailed` | -32603 | Internal error |
| `ResourceNotFound` | -32002 | Resource not found |
| `UrlElicitationRequired` | -32042 | The client must complete a URL elicitation |
| Other server errors | -32000 to -32099 | Reserved for implementation-defined server errors |

## Error Handling Patterns

### Converting Other Errors

`McpError` implements `From<std::io::Error>` (classified by the I/O error kind:
`NotFound` becomes `ResourceNotFound`, and so on) and `From<serde_json::Error>`,
so `?` works on those directly:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Files;

#[server]
impl Files {
    /// Read a JSON file and return one field.
    #[tool]
    async fn read_field(&self, path: String, field: String) -> McpResult<String> {
        let text = tokio::fs::read_to_string(&path).await?; // io::Error -> McpError
        let value: serde_json::Value = serde_json::from_str(&text)?; // serde_json::Error -> McpError
        Ok(value[&field].to_string())
    }
}
```

For other error types, convert with `map_err`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Parser;

#[server]
impl Parser {
    /// Parse an integer.
    #[tool]
    async fn parse(&self, input: String) -> McpResult<i64> {
        input
            .trim()
            .parse::<i64>()
            .map_err(|e| McpError::invalid_params(format!("Not an integer: {e}")))
    }
}
```

### Custom Error Conversion

Implement `From` for your own error types so `?` converts them:

```rust
use turbomcp::prelude::*;

#[derive(Debug)]
enum MyError {
    NotFound(String),
    InvalidFormat(String),
    DatabaseError(String),
}

impl From<MyError> for McpError {
    fn from(err: MyError) -> Self {
        match err {
            MyError::NotFound(msg) => McpError::resource_not_found(msg),
            MyError::InvalidFormat(msg) => McpError::invalid_params(msg),
            MyError::DatabaseError(msg) => McpError::internal(msg),
        }
    }
}

fn find_resource(id: &str) -> Result<(), MyError> {
    if id.is_empty() {
        return Err(MyError::InvalidFormat("empty id".into()));
    }
    Ok(())
}

#[derive(Clone)]
struct Resources;

#[server]
impl Resources {
    /// Check that a resource exists.
    #[tool]
    async fn check(&self, id: String) -> McpResult<String> {
        find_resource(&id)?; // MyError converts to McpError
        Ok("Found".to_string())
    }
}
```

### Anyhow Integration

```rust
use anyhow::Context;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Reader;

#[server]
impl Reader {
    /// Read a file.
    #[tool]
    async fn with_context(&self, path: String) -> McpResult<String> {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read file: {}", path))
            .map_err(|e| McpError::internal(format!("{e:#}")))?;

        Ok(content)
    }
}
```

## Error Logging

Nothing is logged for you when a handler returns an error. Log what the
operator needs with `tracing` (to stderr on a STDIO server) and return what the
client needs:

```rust
use turbomcp::prelude::*;

async fn some_operation() -> Result<String, std::io::Error> {
    Err(std::io::Error::other("disk full"))
}

#[derive(Clone)]
struct Logged;

#[server]
impl Logged {
    /// Run the operation.
    #[tool]
    async fn logged_operation(&self, ctx: &RequestContext) -> McpResult<String> {
        match some_operation().await {
            Ok(result) => Ok(result),
            Err(e) => {
                // Full detail stays on the server
                tracing::error!(request_id = %ctx.request_id(), error = %e, "operation failed");

                // The client gets a sanitized message
                Err(McpError::internal("Operation failed"))
            }
        }
    }
}
```

## Error Response Format

A resource, prompt, or protocol error is serialized as a JSON-RPC 2.0 error
response:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32002,
    "message": "Resource not found: file:///missing.txt",
    "data": {
      "uri": "file:///missing.txt"
    }
  }
}
```

## Handling Errors in a Client

```rust
use turbomcp_client::{Client, Transport};
use turbomcp_core::error::ErrorKind;

async fn call(client: &Client<impl Transport + 'static>) {
    match client.call_tool("my_tool", None, None).await {
        // The tool ran; it may still have failed
        Ok(result) if result.is_error == Some(true) => eprintln!("tool failed: {:?}", result.content),
        Ok(result) => println!("Success: {:?}", result),
        // The request itself failed
        Err(err) => match err.kind {
            ErrorKind::Transport => eprintln!("Transport error: {err}"),
            _ if err.is_retryable() => eprintln!("Retryable error: {err}"),
            _ => eprintln!("Error ({:?}): {err}", err.kind),
        },
    }
}
```

## Migration from v2.x

### Before (v2.x)

```rust,ignore
use turbomcp_server::{ServerError, ServerResult};

fn handler() -> ServerResult<Value> {
    Err(ServerError::internal("failed"))
}
```

This is the removed v2 API, shown for comparison; it does not compile against v3.

### After (v3.x)

```rust
use serde_json::Value;
use turbomcp::{McpError, McpResult};

fn handler() -> McpResult<Value> {
    Err(McpError::internal("failed"))
}
```

### Type Mapping

| v2.x Type | v3.x Type |
|-----------|-----------|
| `ServerError` | `McpError` |
| `ServerResult<T>` | `McpResult<T>` |
| `ClientError` | `McpError` |
| `ClientResult<T>` | `McpResult<T>` |
| `Error` (protocol) | `McpError` |

## Best Practices

### 1. Use Semantic Constructors

```rust
use turbomcp::McpError;

// Good - semantic meaning is clear
let good = McpError::tool_not_found("calculator");

// Avoid - less informative
let avoid = McpError::internal("tool not found");
```

### 2. Add Context

```rust
use turbomcp::McpError;

let value = "abc";

// Good - includes helpful context
let good = McpError::invalid_params(format!("Expected integer, got: {}", value))
    .with_data(serde_json::json!({ "field": "count", "received": value }));

// Avoid - no context
let avoid = McpError::invalid_params("bad input");
```

### 3. Don't Leak Internal Details

A tool error's message goes to the model, so keep secrets and infrastructure
details out of it. `McpError::safe_internal` and `.sanitized()` strip
credentials, IP addresses, and paths from a message:

```rust
use turbomcp::McpError;

// Good - user-friendly message
let good = McpError::internal("Database temporarily unavailable");

// Better than passing the raw error through
let pg_error = "connection to postgres://admin:secret@10.0.0.5/db failed";
let sanitized = McpError::safe_internal(pg_error);
assert!(!sanitized.message.contains("secret"));
```

### 4. Log Before Returning

See [Error Logging](#error-logging): log the full error server-side, return a
sanitized one.

## no_std Support

`McpError` is available in `no_std` environments via `turbomcp-core`:

```toml
[dependencies]
turbomcp-core = { version = "3.5.0", default-features = false }
```

```rust
#![no_std]

use turbomcp_core::error::{McpError, McpResult};

fn handler() -> McpResult<&'static str> {
    Err(McpError::invalid_params("missing field"))
}
```

## Next Steps

- **[Architecture](architecture.md)** - Overall system design
- **[Handlers](handlers.md)** - Writing tool handlers
- **[Observability](observability.md)** - Error tracking and logging
- **[API Reference](../api/core.md)** - Full McpError API

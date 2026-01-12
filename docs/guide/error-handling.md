# Error Handling

TurboMCP v3 introduces a unified error handling system with `McpError` - a single error type across the entire SDK.

## Overview

In v3, all error types have been unified into `McpError`:

- **No more `ServerError`** - Use `McpError` instead
- **No more `ClientError`** - Use `McpError` instead
- **No more `Error`** - The protocol error type is now `McpError`
- **Unified result type** - `McpResult<T>` replaces `ServerResult<T>` and `ClientResult<T>`

## Basic Usage

```rust
use turbomcp::{McpError, McpResult};

#[tool]
async fn my_tool(input: String) -> McpResult<String> {
    if input.is_empty() {
        return Err(McpError::invalid_params("Input cannot be empty"));
    }
    Ok(format!("Processed: {}", input))
}
```

## Error Constructors

`McpError` provides semantic constructors for common error scenarios:

### Protocol Errors

```rust
// Parse error (-32700)
McpError::parse_error("Invalid JSON")

// Invalid request (-32600)
McpError::invalid_request("Missing method field")

// Method not found (-32601)
McpError::method_not_found("unknown_method")

// Invalid params (-32602)
McpError::invalid_params("Missing required field: name")

// Internal error (-32603)
McpError::internal("Database connection failed")
```

### MCP-Specific Errors

```rust
// Tool not found
McpError::tool_not_found("calculator")

// Resource not found
McpError::resource_not_found("file:///missing.txt")

// Prompt not found
McpError::prompt_not_found("greeting")

// Not initialized
McpError::not_initialized()

// Already initialized
McpError::already_initialized()

// Request cancelled
McpError::request_cancelled()
```

### Server Extension Errors

```rust
use turbomcp::error::ServerErrorExt;

// Lifecycle errors
McpError::lifecycle("Server not started")

// Shutdown errors
McpError::shutdown("Graceful shutdown timeout")

// Middleware errors
McpError::middleware("Authentication failed")

// Registry errors
McpError::registry("Handler already registered")

// Routing errors
McpError::routing("No handler for method")

// Resource exhausted
McpError::resource_exhausted("Connection pool exhausted")
```

## Error Details

Add rich context to errors:

```rust
#[tool]
async fn complex_operation(id: String) -> McpResult<String> {
    let result = database_query(&id).await.map_err(|e| {
        McpError::internal(&format!("Database error: {}", e))
            .with_detail("query_id", &id)
            .with_detail("retry_after", "5s")
    })?;

    Ok(result)
}
```

## JSON-RPC Error Codes

`McpError` automatically maps to JSON-RPC 2.0 error codes:

| Error Type | JSON-RPC Code | Description |
|------------|---------------|-------------|
| Parse Error | -32700 | Invalid JSON received |
| Invalid Request | -32600 | Not a valid request object |
| Method Not Found | -32601 | Method does not exist |
| Invalid Params | -32602 | Invalid method parameters |
| Internal Error | -32603 | Internal JSON-RPC error |
| Server Errors | -32000 to -32099 | Reserved for server errors |

## Error Handling Patterns

### Basic Try-Catch

```rust
#[tool]
async fn safe_operation(input: String) -> McpResult<String> {
    match risky_operation(&input).await {
        Ok(result) => Ok(result),
        Err(e) => Err(McpError::internal(&e.to_string())),
    }
}
```

### Using the `?` Operator

```rust
#[tool]
async fn chained_operations(input: String) -> McpResult<String> {
    // Errors automatically convert to McpError
    let parsed = parse_input(&input)?;
    let validated = validate(&parsed)?;
    let result = process(&validated)?;
    Ok(result)
}
```

### Custom Error Conversion

Implement `From` for your custom errors:

```rust
use turbomcp::{McpError, McpResult};

#[derive(Debug)]
enum MyError {
    NotFound(String),
    InvalidFormat(String),
    DatabaseError(String),
}

impl From<MyError> for McpError {
    fn from(err: MyError) -> Self {
        match err {
            MyError::NotFound(msg) => McpError::resource_not_found(&msg),
            MyError::InvalidFormat(msg) => McpError::invalid_params(&msg),
            MyError::DatabaseError(msg) => McpError::internal(&msg),
        }
    }
}

#[tool]
async fn my_handler(id: String) -> McpResult<String> {
    find_resource(&id)?;  // MyError converts to McpError
    Ok("Found".to_string())
}
```

### Anyhow Integration

```rust
use anyhow::Context;
use turbomcp::{McpError, McpResult};

#[tool]
async fn with_context(path: String) -> McpResult<String> {
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read file: {}", path))
        .map_err(|e| McpError::internal(&e.to_string()))?;

    Ok(content)
}
```

## Error Logging

Errors are automatically logged with context:

```rust
#[tool]
async fn logged_operation(logger: Logger) -> McpResult<String> {
    match some_operation().await {
        Ok(result) => Ok(result),
        Err(e) => {
            // Log error with context
            logger.error(&format!("Operation failed: {}", e)).await?;

            // Return appropriate error
            Err(McpError::internal("Operation failed"))
        }
    }
}
```

## Error Response Format

Errors are serialized as JSON-RPC 2.0 error responses:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32602,
    "message": "Invalid params: Missing required field: name",
    "data": {
      "field": "name",
      "expected": "string"
    }
  }
}
```

## Migration from v2.x

### Before (v2.x)

```rust
use turbomcp_server::{ServerError, ServerResult};

fn handler() -> ServerResult<Value> {
    Err(ServerError::internal("failed"))
}
```

### After (v3.x)

```rust
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
// Good - semantic meaning is clear
McpError::tool_not_found("calculator")

// Avoid - less informative
McpError::internal("tool not found")
```

### 2. Add Context

```rust
// Good - includes helpful context
McpError::invalid_params(&format!("Expected integer, got: {}", value))
    .with_detail("field", "count")
    .with_detail("received", &value)

// Avoid - no context
McpError::invalid_params("bad input")
```

### 3. Don't Leak Internal Details

```rust
// Good - user-friendly message
McpError::internal("Database temporarily unavailable")

// Avoid - leaks internal implementation
McpError::internal(&format!("PostgreSQL error: {}", pg_error))
```

### 4. Log Before Returning

```rust
#[tool]
async fn handler(logger: Logger) -> McpResult<String> {
    match operation().await {
        Ok(result) => Ok(result),
        Err(e) => {
            // Log full error internally
            logger.error(&format!("Full error: {:?}", e)).await?;

            // Return sanitized error to client
            Err(McpError::internal("Operation failed"))
        }
    }
}
```

## no_std Support

`McpError` is available in `no_std` environments via `turbomcp-core`:

```toml
[dependencies]
turbomcp-core = { version = "3.0", default-features = false }
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

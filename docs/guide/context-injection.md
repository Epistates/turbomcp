# Context & Dependency Injection

Learn how to use TurboMCP's powerful dependency injection system to access resources and configuration in your handlers.

## Overview

TurboMCP provides automatic dependency injection (DI) for handlers. Instead of passing everything through parameters, handlers can request dependencies and they're automatically provided.

```rust
#[tool]
async fn my_handler(
    logger: Logger,      // Automatically injected
    cache: Cache,        // Automatically injected
    config: Config,      // Automatically injected
) -> McpResult<String> {
    logger.info("Working with cache").await?;
    Ok("Done".to_string())
}
```

## Available Injectables

### Built-in Injectables

TurboMCP provides these out of the box:

#### InjectContext
Full request context with all metadata.

```rust
#[tool]
async fn handler(ctx: InjectContext) -> McpResult<String> {
    // Access anything you need
    let config = ctx.config();
    let logger = ctx.logger();
    let cache = ctx.cache();
    Ok("Done".to_string())
}
```

#### RequestInfo
Metadata about the current request.

```rust
#[tool]
async fn handler(info: RequestInfo) -> McpResult<String> {
    println!("Request ID: {}", info.request_id);
    println!("Handler: {}", info.handler_name);
    println!("Correlation: {}", info.correlation_id);
    Ok("Done".to_string())
}
```

#### Logger
Structured logging.

```rust
#[tool]
async fn handler(logger: Logger) -> McpResult<String> {
    logger.info("Starting operation").await?;
    logger.warn("Unexpected value").await?;
    logger.error("Failed to connect").await?;
    Ok("Done".to_string())
}
```

#### Config
Application configuration.

```rust
#[tool]
async fn handler(config: Config) -> McpResult<String> {
    let db_url: Option<String> = config.get("database_url")?;
    let cache_ttl: Option<u32> = config.get("cache_ttl")?;
    Ok("Done".to_string())
}
```

#### Cache
In-memory caching (typed).

```rust
#[tool]
async fn handler(cache: Cache) -> McpResult<String> {
    // Store
    cache.set("key", "value").await?;

    // Retrieve
    if let Some(cached) = cache.get::<String>("key")? {
        println!("Found: {}", cached);
    }

    // Delete
    cache.delete("key").await?;

    Ok("Done".to_string())
}
```

#### Database
Type-safe database access.

```rust
#[tool]
async fn handler(db: Database) -> McpResult<String> {
    let result = db.query("SELECT * FROM users").await?;
    Ok(format!("Found {} users", result.len()))
}
```

#### HttpClient
Async HTTP requests.

```rust
#[tool]
async fn handler(http: HttpClient) -> McpResult<String> {
    let response = http
        .get("https://api.example.com/data")
        .send()
        .await?;

    let text = response.text().await?;
    Ok(text)
}
```

## Injection Patterns

### Pattern 1: Selective Injection

Only request what you need:

```rust
#[tool]
async fn simple_handler(logger: Logger) -> McpResult<String> {
    // Just logging, no config or cache needed
    logger.info("Simple operation").await?;
    Ok("Done".to_string())
}

#[tool]
async fn complex_handler(
    logger: Logger,
    config: Config,
    cache: Cache,
    db: Database,
) -> McpResult<String> {
    // Complex operation needs everything
    Ok("Done".to_string())
}
```

### Pattern 2: Context Wrapping

Use `InjectContext` for maximum flexibility:

```rust
#[tool]
async fn handler(ctx: InjectContext) -> McpResult<String> {
    // Access any injectable through context
    let logger = ctx.logger();
    let config = ctx.config();
    let cache = ctx.cache();

    logger.info("Starting").await?;

    // Do work...

    Ok("Done".to_string())
}
```

### Pattern 3: Feature-Specific Injection

Different handlers request different features:

```rust
#[tool]
async fn logging_handler(logger: Logger) -> McpResult<String> {
    logger.info("Hello").await?;
    Ok("Logged".to_string())
}

#[tool]
async fn caching_handler(cache: Cache) -> McpResult<String> {
    cache.set("key", "value").await?;
    Ok("Cached".to_string())
}

#[tool]
async fn database_handler(db: Database) -> McpResult<String> {
    let count = db.query("SELECT COUNT(*) FROM users").await?;
    Ok(format!("Users: {}", count))
}
```

### Pattern 4: Custom Injectables

Register your own services:

```rust
use turbomcp::injection::{Injectable, InjectionRegistry};

#[derive(Clone)]
struct MyService {
    data: String,
}

impl Injectable for MyService {
    fn inject() -> Self {
        Self {
            data: "initialized".to_string(),
        }
    }
}

#[tool]
async fn handler(service: MyService) -> McpResult<String> {
    Ok(service.data)
}
```

## Configuration Management

### Setting Configuration

```rust
let server = McpServer::new()
    .with_config({
        let mut config = Config::new();
        config.set("api_key", "secret")?;
        config.set("max_retries", 3)?;
        config.set("timeout_seconds", 30)?;
        config
    })
    .stdio()
    .run()
    .await?;
```

### Using Configuration

```rust
#[tool]
async fn handler(config: Config) -> McpResult<String> {
    // Get with default
    let api_key: String = config.get("api_key")
        .unwrap_or("default-key".to_string());

    // Get with Option
    let max_retries: Option<u32> = config.get("max_retries")?;

    // Get with error handling
    let timeout: u32 = config.get("timeout_seconds")?
        .ok_or(McpError::InvalidInput("timeout required".into()))?;

    Ok("Done".to_string())
}
```

## Request Correlation

Track requests across async boundaries:

```rust
#[tool]
async fn handler(info: RequestInfo, logger: Logger) -> McpResult<String> {
    // Every request has a unique ID
    let request_id = &info.request_id;

    // And a correlation ID (same across retries)
    let correlation_id = &info.correlation_id;

    // Log with IDs for tracing
    logger.info(&format!(
        "Request {} (correlation {})",
        request_id, correlation_id
    )).await?;

    Ok("Done".to_string())
}
```

## Caching Patterns

### Simple Caching

```rust
#[tool]
async fn get_data(cache: Cache) -> McpResult<String> {
    // Try cache first
    if let Some(cached) = cache.get::<String>("my_data")? {
        return Ok(cached);
    }

    // Compute if not cached
    let data = "expensive computation".to_string();

    // Store for next time
    cache.set("my_data", &data).await?;

    Ok(data)
}
```

### Cache with TTL

```rust
#[tool]
async fn get_fresh_data(cache: Cache) -> McpResult<String> {
    // Check if cached AND still fresh
    if let Some(cached) = cache.get::<(String, Instant)>("fresh_data")? {
        let (data, created) = cached;

        if created.elapsed() < Duration::from_secs(300) {
            return Ok(data);  // Still fresh
        }
        // Otherwise, recompute
    }

    let data = "fresh data".to_string();
    let created = Instant::now();
    cache.set("fresh_data", &(data.clone(), created)).await?;

    Ok(data)
}
```

## Error Context

Access error context for better error messages:

```rust
#[tool]
async fn handler(info: RequestInfo) -> McpResult<String> {
    // Request ID helps track errors in logs
    let request_id = &info.request_id;

    // If operation fails, log includes request_id
    // Makes it easy to correlate errors

    Err(McpError::InternalError(
        format!("Operation failed for request {}", request_id)
    ))
}
```

## Performance Considerations

### Efficient Injection

Injection is cheap:
- Built-in injectables are pre-allocated
- No allocations for each request
- Cloning is `O(1)` via Arc

```rust
#[tool]
// No performance penalty for injecting multiple services
async fn handler(
    logger: Logger,
    config: Config,
    cache: Cache,
    db: Database,
) -> McpResult<String> {
    // All clones are cheap Arc increments
    Ok("Done".to_string())
}
```

### Context Pooling

RequestContext objects are pooled for efficiency:

```rust
#[tool]
// Context is created from pool, reused after handler
async fn handler(ctx: InjectContext) -> McpResult<String> {
    // Use context freely, no overhead
    Ok("Done".to_string())
}
// Context returns to pool after this
```

## Testing with Injection

### Mock Injectables

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_with_mocks() {
        // Create test instances
        let logger = Logger::test();
        let cache = Cache::test();

        // Call handler with test doubles
        // (requires handlers to accept trait objects)
    }
}
```

## Troubleshooting

### Compilation Error: "cannot find injectable"

Make sure the type is in scope and implements `Injectable`:

```rust
// ❌ Wrong
#[tool]
async fn handler(unknown: UnknownType) -> McpResult<String> {
    Ok("No".to_string())
}

// ✅ Right
#[tool]
async fn handler(logger: Logger) -> McpResult<String> {
    Ok("Yes".to_string())
}
```

### Cannot Inject Custom Type

Register it first:

```rust
use turbomcp::injection::{Injectable, InjectionRegistry, global_injection_registry};

impl Injectable for MyType {
    fn inject() -> Self {
        // Your initialization logic
        Self { /* ... */ }
    }
}

// Then use in handlers
#[tool]
async fn handler(my_type: MyType) -> McpResult<String> {
    Ok("Works".to_string())
}
```

## Advanced Topics

### Conditional Injection

Inject different implementations based on configuration:

```rust
#[tool]
async fn handler(config: Config) -> McpResult<String> {
    let storage_type: Option<String> = config.get("storage")?;

    match storage_type.as_deref() {
        Some("cache") => {
            // Use in-memory cache
            Ok("Using cache".to_string())
        }
        Some("database") => {
            // Use database
            Ok("Using database".to_string())
        }
        _ => {
            // Fallback
            Ok("Using default".to_string())
        }
    }
}
```

### Chaining Operations

Build complex workflows using injected services:

```rust
#[tool]
async fn complex_workflow(
    logger: Logger,
    cache: Cache,
    db: Database,
    http: HttpClient,
) -> McpResult<String> {
    // 1. Log start
    logger.info("Starting workflow").await?;

    // 2. Check cache
    if let Some(cached) = cache.get::<String>("workflow_result")? {
        return Ok(cached);
    }

    // 3. Fetch from database
    let data = db.query("SELECT * FROM data").await?;

    // 4. Enrich with external API
    let enriched = http
        .post("https://api.example.com/enrich")
        .json(&data)
        .send()
        .await?;

    // 5. Cache result
    let result = enriched.text().await?;
    cache.set("workflow_result", &result).await?;

    // 6. Log completion
    logger.info("Workflow complete").await?;

    Ok(result)
}
```

## Next Steps

- **[Transports Guide](transports.md)** - Configure multiple transports
- **[Authentication](authentication.md)** - Add OAuth and security
- **[Observability](observability.md)** - Logging and monitoring
- **[Examples](../examples/basic.md)** - Real-world usage patterns


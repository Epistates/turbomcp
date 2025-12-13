# Utilities API Reference

Complete reference for TurboMCP utility types, helpers, and common patterns.

## Overview

TurboMCP provides a comprehensive set of utility types and functions that handle common patterns like error handling, retry logic, circuit breakers, timeouts, and type conversions. These utilities are battle-tested and production-ready.

## Error Handling

### McpError

The primary error type for TurboMCP operations with rich context preservation.

```rust
use turbomcp::McpError;

pub enum McpError {
    /// Invalid input from client
    InvalidInput(String),

    /// Internal server error
    InternalError(String),

    /// Method not found
    MethodNotFound(String),

    /// Parse error
    ParseError(String),

    /// Timeout error
    Timeout,

    /// Unauthorized access
    Unauthorized,

    /// Custom error with code and data
    Custom {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },
}
```

#### Creating Errors

```rust
// Simple errors
Err(McpError::InvalidInput("Missing required field 'name'".into()))
Err(McpError::InternalError("Database connection failed".into()))
Err(McpError::MethodNotFound("Tool 'unknown' not found".into()))

// Custom errors with additional data
Err(McpError::Custom {
    code: -32001,
    message: "Rate limit exceeded".into(),
    data: Some(serde_json::json!({
        "retry_after": 60,
        "limit": 100
    })),
})
```

#### Error Conversion

```rust
use std::io;

// Convert from std::io::Error
fn read_config() -> Result<String, McpError> {
    std::fs::read_to_string("config.json")
        .map_err(|e| McpError::InternalError(
            format!("Failed to read config: {}", e)
        ))
}

// Using the ? operator with error conversion
impl From<io::Error> for McpError {
    fn from(err: io::Error) -> Self {
        McpError::InternalError(err.to_string())
    }
}

fn read_file(path: &str) -> Result<String, McpError> {
    Ok(std::fs::read_to_string(path)?)
}
```

### McpResult

Type alias for Results with McpError:

```rust
pub type McpResult<T> = Result<T, McpError>;

// Usage
fn process_data(input: String) -> McpResult<ProcessedData> {
    let validated = validate(input)?;
    let processed = transform(validated)?;
    Ok(processed)
}
```

### Error Context

Add context to errors:

```rust
use turbomcp::error::ErrorContext;

fn complex_operation() -> McpResult<String> {
    perform_step_1()
        .map_err(|e| e.with_context("Step 1 failed"))?;

    perform_step_2()
        .map_err(|e| e.with_context("Step 2 failed"))?;

    Ok("Success".to_string())
}
```

## Retry Logic

### RetryConfig

Configure retry behavior with exponential backoff:

```rust
use turbomcp::utils::RetryConfig;
use std::time::Duration;

let config = RetryConfig::new()
    .with_max_attempts(5)
    .with_base_delay(Duration::from_millis(100))
    .with_max_delay(Duration::from_secs(30))
    .with_backoff_multiplier(2.0)
    .with_jitter(true);
```

#### Configuration Options

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_attempts` | `usize` | 3 | Maximum retry attempts |
| `base_delay` | `Duration` | 100ms | Initial delay between retries |
| `max_delay` | `Duration` | 30s | Maximum delay cap |
| `backoff_multiplier` | `f64` | 2.0 | Exponential backoff factor |
| `jitter` | `bool` | true | Add random jitter (±5%) |

#### Delay Calculation

```rust
let config = RetryConfig::new()
    .with_base_delay(Duration::from_millis(100))
    .with_backoff_multiplier(2.0)
    .with_jitter(false);

// Attempt 0: 0ms (immediate)
// Attempt 1: 100ms
// Attempt 2: 200ms (100 * 2^1)
// Attempt 3: 400ms (100 * 2^2)
// Attempt 4: 800ms (100 * 2^3)

let delay = config.delay_for_attempt(3);
assert_eq!(delay, Duration::from_millis(400));
```

### retry_with_backoff

Retry operations with exponential backoff:

```rust
use turbomcp::utils::{retry_with_backoff, RetryConfig};

async fn flaky_operation() -> Result<String, String> {
    // Operation that might fail
    Ok("Success".to_string())
}

let config = RetryConfig::new()
    .with_max_attempts(3)
    .with_base_delay(Duration::from_millis(100));

let result = retry_with_backoff(
    flaky_operation,
    config,
    |error| {
        // Decide if error is retryable
        error.contains("temporary")
    }
).await?;
```

#### Complete Example

```rust
use turbomcp::utils::{retry_with_backoff, RetryConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let attempt_count = Arc::new(AtomicU32::new(0));
    let attempt_count_clone = attempt_count.clone();

    let config = RetryConfig::new()
        .with_max_attempts(5)
        .with_base_delay(Duration::from_millis(50));

    let result = retry_with_backoff(
        move || {
            let count = attempt_count_clone.clone();
            async move {
                let attempt = count.fetch_add(1, Ordering::SeqCst);
                println!("Attempt {}", attempt + 1);

                if attempt < 3 {
                    Err("Temporary failure")
                } else {
                    Ok("Success!")
                }
            }
        },
        config,
        |error| error.contains("Temporary")
    ).await?;

    println!("Result: {}", result);
    println!("Total attempts: {}", attempt_count.load(Ordering::SeqCst));
    Ok(())
}
```

## Circuit Breaker

### CircuitBreaker

Prevent cascading failures with circuit breaker pattern:

```rust
use turbomcp::utils::CircuitBreaker;
use std::time::Duration;

let breaker = CircuitBreaker::new(
    5,                              // Failure threshold
    Duration::from_secs(60)         // Recovery timeout
);
```

#### Circuit States

```rust
use turbomcp::utils::CircuitState;

pub enum CircuitState {
    Closed,    // Normal operation
    Open,      // Failing fast
    HalfOpen,  // Testing recovery
}
```

#### Usage

```rust
use turbomcp::utils::{CircuitBreaker, CircuitBreakerError};

let breaker = CircuitBreaker::new(3, Duration::from_secs(30));

// Call operation through circuit breaker
let result = breaker.call(|| async {
    external_api_call().await
}).await;

match result {
    Ok(data) => println!("Success: {:?}", data),
    Err(CircuitBreakerError::Open) => {
        println!("Circuit is open - failing fast");
    }
    Err(CircuitBreakerError::Operation(e)) => {
        println!("Operation failed: {}", e);
    }
}
```

#### State Management

```rust
// Check circuit state
match breaker.state() {
    CircuitState::Closed => println!("Circuit is healthy"),
    CircuitState::Open => println!("Circuit is open - too many failures"),
    CircuitState::HalfOpen => println!("Circuit is testing recovery"),
}
```

#### Complete Example

```rust
use turbomcp::utils::{CircuitBreaker, CircuitBreakerError};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let breaker = Arc::new(CircuitBreaker::new(
        2,                              // Open after 2 failures
        Duration::from_secs(10)         // Try recovery after 10s
    ));

    let failure_count = Arc::new(AtomicU32::new(0));

    for i in 0..5 {
        let fc = failure_count.clone();

        let result = breaker.call(|| async move {
            let count = fc.load(Ordering::SeqCst);
            if count < 2 {
                fc.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>("Simulated failure")
            } else {
                Ok(())
            }
        }).await;

        match result {
            Ok(_) => println!("Request {} succeeded", i),
            Err(CircuitBreakerError::Open) => {
                println!("Request {} blocked by open circuit", i);
            }
            Err(CircuitBreakerError::Operation(e)) => {
                println!("Request {} failed: {}", i, e);
            }
        }
    }

    Ok(())
}
```

## Timeout

### Timeout Wrapper

Add timeouts to any async operation:

```rust
use turbomcp::utils::{timeout, TimeoutError};
use std::time::Duration;

async fn slow_operation() -> Result<String, McpError> {
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok("Done".to_string())
}

// With timeout
let result = timeout(
    Duration::from_secs(2),
    slow_operation()
).await;

match result {
    Ok(Ok(data)) => println!("Success: {}", data),
    Ok(Err(e)) => println!("Operation error: {}", e),
    Err(TimeoutError) => println!("Operation timed out"),
}
```

### tokio::time Integration

Use with tokio timeout for Result unwrapping:

```rust
use tokio::time::{timeout, Duration};

let result = timeout(
    Duration::from_secs(5),
    async_operation()
).await??;  // Unwrap timeout error and operation error
```

### Timeout Patterns

#### Pattern 1: Timeout with Fallback

```rust
async fn with_fallback() -> McpResult<String> {
    match timeout(Duration::from_secs(1), fetch_data()).await {
        Ok(Ok(data)) => Ok(data),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            // Use cached data on timeout
            get_cached_data()
        }
    }
}
```

#### Pattern 2: Timeout with Retry

```rust
async fn timeout_with_retry() -> McpResult<String> {
    let config = RetryConfig::new().with_max_attempts(3);

    retry_with_backoff(
        || async {
            timeout(
                Duration::from_secs(5),
                fetch_data()
            ).await
                .map_err(|_| "Timeout")?
        },
        config,
        |_| true
    ).await
}
```

## Type Utilities

### Timestamp

UTC timestamp wrapper for consistent time handling:

```rust
use turbomcp::types::Timestamp;

// Create timestamp
let now = Timestamp::now();
let from_dt = Timestamp::from_datetime(chrono::Utc::now());

// Access datetime
let dt = now.datetime();

// Calculate elapsed time
let elapsed = now.elapsed();
println!("Elapsed: {}ms", elapsed.num_milliseconds());

// Formatting
println!("Time: {}", now);  // RFC3339 format
```

### RequestId

Unique request identifier:

```rust
use turbomcp::types::RequestId;

// Generate unique ID
let id = RequestId::new();

// From string or number
let id_str = RequestId::from("request-123");
let id_num = RequestId::from(42);

// Comparison
if request_id == expected_id {
    println!("IDs match");
}
```

### Uri

URI string with validation:

```rust
use turbomcp::types::Uri;

let uri = Uri::from("file:///path/to/resource");
let uri = Uri::from("http://example.com/api");
let uri = Uri::from("config://app/settings");
```

### Domain Types

Validated domain-specific types:

```rust
use turbomcp::types::domain::{Uri, MimeType, Base64String};

// Validated URI
let uri = Uri::parse("https://example.com")?;

// MIME type
let mime = MimeType::parse("application/json")?;

// Base64 encoded data
let encoded = Base64String::encode(b"Hello, World!");
let decoded = encoded.decode()?;
```

## Collection Utilities

### HashMap Extensions

Convenient HashMap construction:

```rust
use std::collections::HashMap;

// Using collect
let map: HashMap<String, i32> = vec![
    ("a".to_string(), 1),
    ("b".to_string(), 2),
    ("c".to_string(), 3),
].into_iter().collect();

// Using macro
use turbomcp::hashmap;

let map = hashmap! {
    "name" => "Alice",
    "role" => "admin",
};
```

### Vec Extensions

Convenient Vec operations:

```rust
// Chunking
let chunks: Vec<Vec<i32>> = vec![1, 2, 3, 4, 5]
    .chunks(2)
    .map(|c| c.to_vec())
    .collect();

// Deduplication
let mut vec = vec![1, 2, 2, 3, 3, 3];
vec.dedup();
assert_eq!(vec, vec![1, 2, 3]);
```

## Conversion Utilities

### JSON Conversion

Convert between types and JSON:

```rust
use serde_json::{json, Value};

// To JSON
let data = MyStruct { field: "value" };
let json = serde_json::to_value(&data)?;
let json_string = serde_json::to_string(&data)?;
let pretty = serde_json::to_string_pretty(&data)?;

// From JSON
let data: MyStruct = serde_json::from_value(json)?;
let data: MyStruct = serde_json::from_str(&json_string)?;
```

### String Conversion

```rust
// To String
let s = format!("Value: {}", 42);
let s = value.to_string();
let s = String::from("literal");

// From String
let num: i32 = "42".parse()?;
let parsed: MyType = serde_json::from_str(&json_str)?;
```

## Async Utilities

### Concurrent Execution

Execute multiple futures concurrently:

```rust
use tokio::try_join;

async fn fetch_all() -> McpResult<(User, Orders, Profile)> {
    let (user, orders, profile) = try_join!(
        fetch_user(),
        fetch_orders(),
        fetch_profile()
    )?;

    Ok((user, orders, profile))
}
```

### Select Operations

Wait for first completed future:

```rust
use tokio::select;

async fn wait_for_event() -> McpResult<String> {
    select! {
        result = network_request() => {
            result
        }
        _ = tokio::time::sleep(Duration::from_secs(5)) => {
            Err(McpError::Timeout)
        }
        _ = tokio::signal::ctrl_c() => {
            Err(McpError::Cancelled)
        }
    }
}
```

### Stream Processing

Process async streams:

```rust
use futures::stream::{self, StreamExt};

async fn process_items(items: Vec<Item>) -> McpResult<Vec<Processed>> {
    let stream = stream::iter(items)
        .map(|item| async move { process_item(item).await })
        .buffer_unordered(10);  // Process 10 concurrently

    stream.collect().await
}
```

## Logging Utilities

### Structured Logging

```rust
use tracing::{info, warn, error, debug};

// Basic logging
info!("Server started on port {}", 8080);
warn!("Cache miss for key: {}", key);
error!("Failed to connect to database: {}", err);

// Structured fields
info!(
    user_id = %user.id,
    action = "login",
    "User logged in successfully"
);
```

### Log Levels

```rust
// Configure log levels
tracing_subscriber::fmt()
    .with_env_filter("info,turbomcp=debug")
    .init();

// Conditional logging
if tracing::enabled!(tracing::Level::DEBUG) {
    let expensive_debug_info = compute_debug_data();
    debug!("Debug info: {:?}", expensive_debug_info);
}
```

## Performance Utilities

### Measure Time

Measure execution time:

```rust
use turbomcp::measure_time;

let result = measure_time!("database_query", {
    database.query("SELECT * FROM users").await
});
// Logs: "database_query took 45.2ms"
```

### Memory Profiling

```rust
#[cfg(debug_assertions)]
fn profile_memory<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let before = get_memory_usage();
    let result = f();
    let after = get_memory_usage();
    println!("{}: {}MB", label, (after - before) / 1024 / 1024);
    result
}
```

## Testing Utilities

### Mock Data

Create test data:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn mock_user() -> User {
        User {
            id: 1,
            name: "Test User".to_string(),
            email: "test@example.com".to_string(),
        }
    }

    #[tokio::test]
    async fn test_user_creation() {
        let user = mock_user();
        assert_eq!(user.name, "Test User");
    }
}
```

### Test Helpers

```rust
#[cfg(test)]
pub mod test_helpers {
    use super::*;

    pub async fn setup_test_server() -> TestServer {
        TestServer::new().await
    }

    pub fn create_test_context() -> Context {
        Context::new_test()
    }
}
```

## Best Practices

### 1. Use Strong Error Types

```rust
// Good - Specific error types
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Query failed: {0}")]
    QueryFailed(String),
}

// Convert to McpError at boundary
impl From<DatabaseError> for McpError {
    fn from(err: DatabaseError) -> Self {
        McpError::InternalError(err.to_string())
    }
}
```

### 2. Configure Retries Appropriately

```rust
// Good - Appropriate for use case
let config = RetryConfig::new()
    .with_max_attempts(3)           // Reasonable limit
    .with_base_delay(Duration::from_millis(100))
    .with_max_delay(Duration::from_secs(5))
    .with_jitter(true);             // Prevent thundering herd

// Avoid - Too aggressive
let bad_config = RetryConfig::new()
    .with_max_attempts(100)         // Too many
    .with_base_delay(Duration::from_millis(1))  // Too fast
    .with_jitter(false);            // No jitter
```

### 3. Use Circuit Breakers for External Services

```rust
// Good - Protect external dependencies
let api_breaker = CircuitBreaker::new(5, Duration::from_secs(60));

async fn call_external_api() -> McpResult<Data> {
    api_breaker.call(|| async {
        reqwest::get("https://api.example.com/data")
            .await?
            .json()
            .await
    }).await
        .map_err(|e| match e {
            CircuitBreakerError::Open => {
                McpError::ServiceUnavailable("API circuit open".into())
            }
            CircuitBreakerError::Operation(e) => {
                McpError::InternalError(e.to_string())
            }
        })
}
```

### 4. Add Timeouts to All External Calls

```rust
// Good - Always use timeouts
async fn fetch_with_timeout() -> McpResult<Data> {
    timeout(
        Duration::from_secs(30),
        external_fetch()
    ).await
        .map_err(|_| McpError::Timeout)?
}

// Avoid - No timeout
async fn fetch_no_timeout() -> McpResult<Data> {
    external_fetch().await  // May hang forever
}
```

### 5. Log Appropriately

```rust
// Good - Structured with context
info!(
    request_id = %ctx.request_id,
    duration_ms = elapsed.as_millis(),
    "Request completed successfully"
);

// Avoid - Unstructured
println!("Request completed");
```

## Troubleshooting

### "Retry loop never succeeds"

Check retry predicate and max attempts:

```rust
// Make sure predicate returns true for retryable errors
retry_with_backoff(
    operation,
    config,
    |error| {
        // Add logging
        tracing::warn!("Retry check for error: {}", error);
        error.is_retryable()
    }
).await
```

### "Circuit breaker stuck open"

Increase recovery timeout or lower failure threshold:

```rust
// If circuit opens too easily
let breaker = CircuitBreaker::new(
    10,                             // Higher threshold
    Duration::from_secs(30)         // Shorter recovery
);
```

### "Timeout too aggressive"

Increase timeout duration:

```rust
// Measure actual operation time first
let start = Instant::now();
let result = operation().await;
println!("Operation took: {:?}", start.elapsed());

// Set timeout with buffer
let timeout_duration = Duration::from_secs(10);  // Add buffer
```

## Next Steps

- **[Server API](server.md)** - Build MCP servers
- **[Client API](client.md)** - Build MCP clients
- **[Advanced Patterns](../guide/advanced-patterns.md)** - Complex utility usage

## See Also

- [tokio Documentation](https://docs.rs/tokio)
- [futures Documentation](https://docs.rs/futures)
- [API Documentation (docs.rs)](https://docs.rs/turbomcp)

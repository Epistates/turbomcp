# Utilities API Reference

Reference for TurboMCP's error type and the small utility helpers in
`turbomcp-protocol`.

## Overview

`McpError` / `McpResult` are the error types used across the stack and are
re-exported by `turbomcp`. The retry, circuit-breaker, and timeout helpers
live in `turbomcp_protocol::utils`, which the `turbomcp` crate does not
re-export: add `turbomcp-protocol = "3.5.0"` to use them.

These helpers are for your own code. The client's transport-level resilience
(`ClientBuilder::build_resilient`) uses the separate types in
`turbomcp_transport::resilience`.

## Error Handling

### McpError

`McpError` is a struct, not an enum: a classification (`kind: ErrorKind`), a
message, and optional context. It carries its JSON-RPC code and HTTP status:

```rust,ignore
// Public fields of turbomcp::McpError (listing only)
pub struct McpError {
    pub id: uuid::Uuid,
    pub kind: ErrorKind,
    pub message: String,
    pub source_location: Option<String>,
    pub context: Option<Box<ErrorContext>>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}
```

`ErrorKind` (in `turbomcp_core::error`, also `turbomcp_protocol::ErrorKind`)
covers the MCP and JSON-RPC cases: `ToolNotFound`, `ToolExecutionFailed`,
`PromptNotFound`, `ResourceNotFound`, `ResourceAccessDenied`,
`CapabilityNotSupported`, `ProtocolVersionMismatch`, `UrlElicitationRequired`,
`UserRejected`, `ParseError`, `InvalidRequest`, `MethodNotFound`,
`InvalidParams`, `Internal`, `Authentication`, `PermissionDenied`,
`Transport`, `Timeout`, `Unavailable`, `RateLimited`, `ServerOverloaded`,
`Configuration`, `ExternalService`, `Cancelled`, `Security`, `Serialization`.

#### Creating Errors

Each kind has a constructor:

```rust
use turbomcp::McpError;

fn examples() -> Vec<McpError> {
    vec![
        McpError::invalid_params("Missing required field 'name'"),
        McpError::internal("Database connection failed"),
        McpError::method_not_found("tools/unknown"),
        McpError::tool_not_found("unknown"),
        McpError::resource_not_found("file:///missing.txt"),
        McpError::timeout("upstream took too long"),
        McpError::cancelled("cancelled by client"),
        McpError::unavailable("maintenance window"),
        // Additional data is sent as the JSON-RPC error's `data`
        McpError::rate_limited("Rate limit exceeded")
            .with_data(serde_json::json!({ "retry_after": 60, "limit": 100 })),
    ]
}
```

#### Error Conversion

`McpError` implements `From<std::io::Error>` (mapping `NotFound`,
`PermissionDenied`, `TimedOut`, and connection errors to the matching kind) and
`From<serde_json::Error>`, so `?` works on both:

```rust
use turbomcp::{McpError, McpResult};

// Explicit conversion with a message of your own
fn read_config() -> McpResult<String> {
    std::fs::read_to_string("config.json")
        .map_err(|e| McpError::internal(format!("Failed to read config: {e}")))
}

// Or let `?` convert
fn read_file(path: &str) -> McpResult<String> {
    Ok(std::fs::read_to_string(path)?)
}
```

### McpResult

Type alias for Results with McpError:

```rust
use turbomcp::{McpError, McpResult};

// pub type McpResult<T> = Result<T, McpError>;

fn validate(input: String) -> McpResult<String> {
    if input.is_empty() {
        return Err(McpError::invalid_params("input must not be empty"));
    }
    Ok(input)
}

fn process_data(input: String) -> McpResult<usize> {
    let validated = validate(input)?;
    Ok(validated.len())
}
```

### Error Context

Attach where an error happened with `with_operation`, `with_component`, and
`with_request_id`; the values land in `error.context`:

```rust
use turbomcp::{McpError, McpResult};

fn perform_step() -> McpResult<()> {
    Err(McpError::internal("disk full"))
}

fn complex_operation() -> McpResult<String> {
    perform_step().map_err(|e| e.with_operation("step_1").with_component("importer"))?;
    Ok("Success".to_string())
}
```

`is_retryable()`, `jsonrpc_code()`, and `http_status()` expose the error's
retry hint and wire mappings.

## Retry Logic

### RetryConfig

Configure retry behavior with exponential backoff:

```rust
use std::time::Duration;
use turbomcp_protocol::utils::RetryConfig;

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
| `max_attempts` | `usize` | 3 | Maximum attempts, the first included (0 is treated as 1) |
| `base_delay` | `Duration` | 100ms | Delay before the first retry |
| `max_delay` | `Duration` | 30s | Maximum delay cap |
| `backoff_multiplier` | `f64` | 2.0 | Exponential backoff factor |
| `jitter` | `bool` | true | Add random jitter (±5%) |

#### Delay Calculation

`delay_for_attempt(n)` is the wait after the `n`th failure:

```rust
use std::time::Duration;
use turbomcp_protocol::utils::RetryConfig;

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

Retry operations with exponential backoff. The third argument decides whether
an error is worth retrying; a non-retryable error is returned at once:

```rust
use std::time::Duration;
use turbomcp_protocol::utils::{retry_with_backoff, RetryConfig};

async fn flaky_operation() -> Result<String, String> {
    // Operation that might fail
    Ok("Success".to_string())
}

async fn run() -> Result<String, String> {
    let config = RetryConfig::new()
        .with_max_attempts(3)
        .with_base_delay(Duration::from_millis(100));

    retry_with_backoff(
        flaky_operation,
        config,
        |error: &String| {
            // Decide if error is retryable
            error.contains("temporary")
        },
    )
    .await
}
```

#### Complete Example

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use turbomcp_protocol::utils::{retry_with_backoff, RetryConfig};

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
        |error| error.contains("Temporary"),
    )
    .await?;

    println!("Result: {}", result);
    println!("Total attempts: {}", attempt_count.load(Ordering::SeqCst));
    Ok(())
}
```

## Circuit Breaker

### CircuitBreaker

Prevent cascading failures with circuit breaker pattern:

```rust
use std::time::Duration;
use turbomcp_protocol::utils::CircuitBreaker;

let breaker = CircuitBreaker::new(
    5,                              // Failure threshold
    Duration::from_secs(60),        // Recovery timeout
);
```

After `failure_threshold` consecutive failures the circuit opens and calls fail
fast. Once the recovery timeout has passed, the next call is let through
(half-open); three successes close the circuit again, and a failure reopens it.

#### Circuit States

```rust,ignore
// turbomcp_protocol::utils::CircuitState (listing only)
pub enum CircuitState {
    Closed,    // Normal operation
    Open,      // Failing fast
    HalfOpen,  // Testing recovery
}
```

#### Usage

```rust
use std::time::Duration;
use turbomcp_protocol::utils::{CircuitBreaker, CircuitBreakerError, CircuitState};

async fn external_api_call() -> Result<String, std::io::Error> {
    Ok("data".to_string())
}

async fn call_through(breaker: &CircuitBreaker) {
    // Call operation through circuit breaker
    let result = breaker.call(|| async { external_api_call().await }).await;

    match result {
        Ok(data) => println!("Success: {:?}", data),
        Err(CircuitBreakerError::Open) => {
            println!("Circuit is open - failing fast");
        }
        Err(CircuitBreakerError::Operation(e)) => {
            println!("Operation failed: {}", e);
        }
    }

    // Check circuit state
    match breaker.state() {
        CircuitState::Closed => println!("Circuit is healthy"),
        CircuitState::Open => println!("Circuit is open - too many failures"),
        CircuitState::HalfOpen => println!("Circuit is testing recovery"),
    }
}
```

#### Complete Example

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use turbomcp_protocol::utils::{CircuitBreaker, CircuitBreakerError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let breaker = Arc::new(CircuitBreaker::new(
        2,                              // Open after 2 failures
        Duration::from_secs(10),        // Try recovery after 10s
    ));

    let failure_count = Arc::new(AtomicU32::new(0));

    for i in 0..5 {
        let fc = failure_count.clone();

        let result = breaker
            .call(|| async move {
                let count = fc.load(Ordering::SeqCst);
                if count < 2 {
                    fc.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>("Simulated failure")
                } else {
                    Ok(())
                }
            })
            .await;

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

`turbomcp_protocol::utils::timeout` wraps any future; the result is
`Result<F::Output, TimeoutError>`:

```rust
use std::time::Duration;
use turbomcp::McpError;
use turbomcp_protocol::utils::{timeout, TimeoutError};

async fn slow_operation() -> Result<String, McpError> {
    tokio::time::sleep(Duration::from_secs(5)).await;
    Ok("Done".to_string())
}

async fn run() {
    let result = timeout(Duration::from_secs(2), slow_operation()).await;

    match result {
        Ok(Ok(data)) => println!("Success: {}", data),
        Ok(Err(e)) => println!("Operation error: {}", e),
        Err(TimeoutError) => println!("Operation timed out"),
    }
}
```

### tokio::time Integration

`tokio::time::timeout` works the same way:

```rust
use tokio::time::{timeout, Duration};
use turbomcp::McpResult;

async fn async_operation() -> McpResult<u32> {
    Ok(42)
}

async fn run() -> Result<u32, Box<dyn std::error::Error>> {
    // Unwrap the timeout error, then the operation error
    let value = timeout(Duration::from_secs(5), async_operation()).await??;
    Ok(value)
}
```

### Timeout Patterns

#### Pattern 1: Timeout with Fallback

```rust
use std::time::Duration;
use turbomcp::McpResult;
use turbomcp_protocol::utils::timeout;

async fn fetch_data() -> McpResult<String> {
    Ok("fresh".to_string())
}

fn get_cached_data() -> McpResult<String> {
    Ok("cached".to_string())
}

async fn with_fallback() -> McpResult<String> {
    match timeout(Duration::from_secs(1), fetch_data()).await {
        Ok(Ok(data)) => Ok(data),
        Ok(Err(e)) => Err(e),
        // Use cached data on timeout
        Err(_) => get_cached_data(),
    }
}
```

#### Pattern 2: Timeout with Retry

```rust
use std::time::Duration;
use turbomcp::{McpError, McpResult};
use turbomcp_protocol::utils::{retry_with_backoff, timeout, RetryConfig};

async fn fetch_data() -> McpResult<String> {
    Ok("data".to_string())
}

async fn timeout_with_retry() -> McpResult<String> {
    let config = RetryConfig::new().with_max_attempts(3);

    retry_with_backoff(
        || async {
            timeout(Duration::from_secs(5), fetch_data())
                .await
                .map_err(|_| McpError::timeout("fetch_data timed out"))?
        },
        config,
        |error: &McpError| error.is_retryable(),
    )
    .await
}
```

## Type Utilities

### Timestamp

UTC timestamp wrapper for consistent time handling (`chrono` is needed for
`from_datetime` and the `elapsed` value):

```rust
use turbomcp_protocol::types::Timestamp;

// Create timestamp
let now = Timestamp::now();
let from_dt = Timestamp::from_datetime(chrono::Utc::now());

// Access datetime
let dt = now.datetime();

// Calculate elapsed time (a chrono::Duration)
let elapsed = now.elapsed();
println!("Elapsed: {}ms", elapsed.num_milliseconds());

// Formatting
println!("Time: {}", now);  // RFC3339 format
```

### RequestId

`RequestId` is an alias for `MessageId`, the JSON-RPC id: a string, a number,
or a UUID.

```rust
use turbomcp_protocol::types::RequestId;

// From string, number, or UUID
let id_str = RequestId::from("request-123");
let id_num = RequestId::from(42);
let id_uuid = RequestId::from(uuid::Uuid::new_v4());

// Comparison
if id_num == RequestId::from(42) {
    println!("IDs match");
}
```

### Uri, MimeType, Base64String

String newtypes that document intent in signatures. They do not validate their
contents: construct them with `new` or `From<&str>`/`From<String>`.

```rust
use turbomcp_protocol::types::{Base64String, MimeType, Uri};

let uri = Uri::from("file:///path/to/resource");
let config = Uri::new("config://app/settings");
let mime = MimeType::from("application/json");
let data = Base64String::new("SGVsbG8sIFdvcmxkIQ==");

assert_eq!(uri.as_str(), "file:///path/to/resource");
```

## Collection Utilities

The standard library covers these; TurboMCP adds no collection helpers or
macros.

```rust
use std::collections::HashMap;

// Using collect
let map: HashMap<String, i32> = vec![
    ("a".to_string(), 1),
    ("b".to_string(), 2),
    ("c".to_string(), 3),
].into_iter().collect();

// From an array of pairs
let roles = HashMap::from([("name", "Alice"), ("role", "admin")]);

// Chunking
let chunks: Vec<Vec<i32>> = vec![1, 2, 3, 4, 5]
    .chunks(2)
    .map(|c| c.to_vec())
    .collect();

// Deduplication
let mut values = vec![1, 2, 2, 3, 3, 3];
values.dedup();
assert_eq!(values, vec![1, 2, 3]);
```

## Conversion Utilities

### JSON Conversion

Convert between types and JSON with `serde_json`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct MyStruct {
    field: String,
}

fn round_trip() -> Result<(), serde_json::Error> {
    // To JSON
    let data = MyStruct { field: "value".to_string() };
    let json = serde_json::to_value(&data)?;
    let json_string = serde_json::to_string(&data)?;
    let pretty = serde_json::to_string_pretty(&data)?;

    // From JSON
    let data: MyStruct = serde_json::from_value(json)?;
    let data: MyStruct = serde_json::from_str(&json_string)?;
    Ok(())
}
```

## Async Utilities

### Concurrent Execution

Execute multiple futures concurrently:

```rust
use tokio::try_join;
use turbomcp::McpResult;

async fn fetch_user() -> McpResult<String> { Ok("alice".into()) }
async fn fetch_orders() -> McpResult<Vec<u32>> { Ok(vec![1, 2]) }
async fn fetch_profile() -> McpResult<String> { Ok("profile".into()) }

async fn fetch_all() -> McpResult<(String, Vec<u32>, String)> {
    let (user, orders, profile) = try_join!(fetch_user(), fetch_orders(), fetch_profile())?;
    Ok((user, orders, profile))
}
```

### Select Operations

Wait for first completed future:

```rust
use std::time::Duration;
use tokio::select;
use turbomcp::{McpError, McpResult};

async fn network_request() -> McpResult<String> {
    Ok("response".to_string())
}

async fn wait_for_event() -> McpResult<String> {
    select! {
        result = network_request() => result,
        _ = tokio::time::sleep(Duration::from_secs(5)) => {
            Err(McpError::timeout("no response within 5s"))
        }
        _ = tokio::signal::ctrl_c() => {
            Err(McpError::cancelled("interrupted"))
        }
    }
}
```

### Stream Processing

Process async streams (with the `futures` crate):

```rust
use futures::stream::{self, StreamExt};
use turbomcp::McpResult;

async fn process_item(item: u32) -> McpResult<u32> {
    Ok(item * 2)
}

async fn process_items(items: Vec<u32>) -> McpResult<Vec<u32>> {
    let results: Vec<McpResult<u32>> = stream::iter(items)
        .map(process_item)
        .buffer_unordered(10) // Process 10 concurrently
        .collect()
        .await;

    results.into_iter().collect()
}
```

## Logging Utilities

### Structured Logging

TurboMCP logs through `tracing`. For a STDIO server, send the output to stderr:
stdout carries the protocol.

```rust
use tracing::{error, info, warn};

fn log_examples(key: &str, err: &std::io::Error, user_id: u64) {
    // Basic logging
    info!("Server started on port {}", 8080);
    warn!("Cache miss for key: {}", key);
    error!("Failed to connect to database: {}", err);

    // Structured fields
    info!(user_id, action = "login", "User logged in successfully");
}
```

### Log Levels

```rust
use tracing::debug;

fn compute_debug_data() -> Vec<u32> {
    vec![1, 2, 3]
}

fn init_logging() {
    // Configure log levels (tracing-subscriber with the `env-filter` feature)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("info,turbomcp=debug")
        .init();

    // Conditional logging
    if tracing::enabled!(tracing::Level::DEBUG) {
        let expensive_debug_info = compute_debug_data();
        debug!("Debug info: {:?}", expensive_debug_info);
    }
}
```

Log messages meant for the MCP client (`notifications/message`) are a
different channel: see `turbomcp_protocol::RichContextExt` (`ctx.info(...)`).

## Performance Utilities

### Measure Time

`turbomcp_protocol::measure_time!` evaluates a block and returns its value.
It logs the elapsed time with `tracing::debug!` only when the crate that
*calls* the macro has a `tracing` feature enabled, so in most applications it
logs nothing; time the block yourself when you need the number:

```rust
use std::time::Instant;
use turbomcp_protocol::measure_time;

async fn query() -> Vec<String> {
    vec!["alice".to_string()]
}

async fn run() {
    let users = measure_time!("database_query", { query().await });

    let start = Instant::now();
    let users = query().await;
    tracing::debug!(elapsed = ?start.elapsed(), "database_query");
}
```

## Testing Utilities

`turbomcp::testing::McpTestClient` (also in the prelude) drives a server
through MCP dispatch without a transport:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Users;

#[server]
impl Users {
    /// Look up a user's name.
    #[tool]
    async fn user_name(&self, id: u64) -> String {
        format!("user-{id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn looks_up_a_user() {
        let client = McpTestClient::new(Users);
        client.assert_tool_exists("user_name");

        let result = client
            .call_tool("user_name", serde_json::json!({ "id": 7 }))
            .await
            .unwrap();
        assert_eq!(result.first_text(), Some("user-7"));
    }
}
```

## Best Practices

### 1. Use Strong Error Types

```rust
use turbomcp::McpError;

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
        match err {
            DatabaseError::ConnectionFailed(_) => McpError::unavailable(err.to_string()),
            DatabaseError::QueryFailed(_) => McpError::internal(err.to_string()),
        }
    }
}
```

### 2. Configure Retries Appropriately

```rust
use std::time::Duration;
use turbomcp_protocol::utils::RetryConfig;

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
use turbomcp::{McpError, McpResult};
use turbomcp_protocol::utils::{CircuitBreaker, CircuitBreakerError};

async fn fetch_remote() -> Result<String, std::io::Error> {
    Ok("data".to_string())
}

// Good - Protect external dependencies with one shared breaker
async fn call_external_api(api_breaker: &CircuitBreaker) -> McpResult<String> {
    api_breaker
        .call(|| async { fetch_remote().await })
        .await
        .map_err(|e| match e {
            CircuitBreakerError::Open => McpError::unavailable("API circuit open"),
            CircuitBreakerError::Operation(e) => McpError::external_service(e.to_string()),
        })
}
```

### 4. Add Timeouts to All External Calls

```rust
use std::time::Duration;
use turbomcp::{McpError, McpResult};
use turbomcp_protocol::utils::timeout;

async fn external_fetch() -> McpResult<String> {
    Ok("data".to_string())
}

// Good - Always use timeouts
async fn fetch_with_timeout() -> McpResult<String> {
    timeout(Duration::from_secs(30), external_fetch())
        .await
        .map_err(|_| McpError::timeout("external_fetch timed out"))?
}

// Avoid - No timeout
async fn fetch_no_timeout() -> McpResult<String> {
    external_fetch().await  // May hang forever
}
```

### 5. Log Appropriately

```rust
use std::time::Duration;
use tracing::info;
use turbomcp::RequestContext;

fn log_completion(ctx: &RequestContext, elapsed: Duration) {
    // Good - Structured with context
    info!(
        request_id = %ctx.request_id,
        duration_ms = elapsed.as_millis() as u64,
        "Request completed successfully"
    );

    // Avoid - Unstructured, and on stdout, which a STDIO server
    // uses for the protocol
    println!("Request completed");
}
```

## Troubleshooting

### "Retry loop never succeeds"

Check the retry predicate and max attempts. The predicate sees each error;
returning `false` stops retrying immediately:

```rust
use turbomcp::{McpError, McpResult};
use turbomcp_protocol::utils::{retry_with_backoff, RetryConfig};

async fn operation() -> McpResult<()> {
    Ok(())
}

async fn run(config: RetryConfig) -> McpResult<()> {
    retry_with_backoff(operation, config, |error: &McpError| {
        // Add logging
        tracing::warn!("Retry check for error: {}", error);
        error.is_retryable()
    })
    .await
}
```

### "Circuit breaker stuck open"

Raise the failure threshold or shorten the recovery timeout:

```rust
use std::time::Duration;
use turbomcp_protocol::utils::CircuitBreaker;

// If circuit opens too easily
let breaker = CircuitBreaker::new(
    10,                             // Higher threshold
    Duration::from_secs(30),        // Shorter recovery
);
```

### "Timeout too aggressive"

Measure the operation, then set the timeout with headroom:

```rust
use std::time::{Duration, Instant};

async fn operation() {}

async fn measure() {
    // Measure actual operation time first
    let start = Instant::now();
    operation().await;
    println!("Operation took: {:?}", start.elapsed());

    // Set timeout with buffer
    let timeout_duration = Duration::from_secs(10);
}
```

## Next Steps

- **[Server API](server.md)** - Build MCP servers
- **[Client API](client.md)** - Build MCP clients
- **[Advanced Patterns](../guide/advanced-patterns.md)** - Complex utility usage

## See Also

- [tokio Documentation](https://docs.rs/tokio)
- [futures Documentation](https://docs.rs/futures)
- [API Documentation (docs.rs)](https://docs.rs/turbomcp)

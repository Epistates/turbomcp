# Advanced Patterns & Optimization

Master advanced TurboMCP patterns for complex workflows and performance optimization.

Every example here is a `#[server]` type. Shared services live on the struct
behind `Arc` (or in types that are already cheap to clone), helper methods sit in
the same `impl` block, and only the methods marked `#[tool]` become tools. See
[Context & Shared State](context-injection.md).

## Workflow Orchestration

### Sequential Tool Chaining

A tool can run several steps, each feeding the next. Keep each step a plain
method so it can be tested on its own:

```rust
use schemars::JsonSchema;
use turbomcp::prelude::*;

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct WeatherData {
    location: String,
    temperature_c: f64,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ForecastData {
    location: String,
    days: Vec<String>,
}

#[derive(Clone)]
pub struct WeatherWorkflow {
    client: reqwest::Client,
    base_url: String,
}

#[server(name = "weather", version = "1.0.0")]
impl WeatherWorkflow {
    /// Current weather, then the forecast for the same place.
    #[tool]
    pub async fn get_forecast(
        &self,
        #[description("City to look up")] city: String,
    ) -> McpResult<Json<ForecastData>> {
        let weather = self.fetch_weather(&city).await?;
        let forecast = self.fetch_forecast(&weather).await?;
        Ok(Json(forecast))
    }

    async fn fetch_weather(&self, city: &str) -> McpResult<WeatherData> {
        self.get_json(&format!("{}/weather/{city}", self.base_url)).await
    }

    async fn fetch_forecast(&self, weather: &WeatherData) -> McpResult<ForecastData> {
        self.get_json(&format!("{}/forecast/{}", self.base_url, weather.location))
            .await
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> McpResult<T> {
        self.client
            .get(url)
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|e| McpError::external_service(e.to_string()))?
            .json()
            .await
            .map_err(|e| McpError::external_service(e.to_string()))
    }
}
```

This uses `reqwest` (with its `json` feature) and `schemars`.

### Parallel Operations

Execute independent operations concurrently with `tokio::try_join!`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Briefing {
    client: reqwest::Client,
}

#[server]
impl Briefing {
    /// Weather and news for a city, fetched at the same time.
    #[tool]
    pub async fn get_weather_and_news(&self, city: String) -> McpResult<serde_json::Value> {
        let weather = self.fetch(format!("https://api.weather.example/{city}"));
        let news = self.fetch(format!("https://api.news.example/{city}"));

        // Execute both concurrently; the first error wins
        let (weather, news) = tokio::try_join!(weather, news)?;

        Ok(serde_json::json!({ "weather": weather, "news": news }))
    }

    async fn fetch(&self, url: String) -> McpResult<String> {
        self.client
            .get(url)
            .send()
            .await
            .map_err(|e| McpError::external_service(e.to_string()))?
            .text()
            .await
            .map_err(|e| McpError::external_service(e.to_string()))
    }
}
```

### Conditional Workflows

Branch on the arguments and on who is asking:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone, Default)]
pub struct Store {
    data: Arc<RwLock<HashMap<String, String>>>,
}

#[server]
impl Store {
    /// Read or write a value.
    #[tool]
    pub async fn smart_action(
        &self,
        #[description("\"read\" or \"write\"")] action_type: String,
        key: String,
        value: Option<String>,
        ctx: &RequestContext,
    ) -> McpResult<String> {
        match action_type.as_str() {
            "read" => self
                .data
                .read()
                .await
                .get(&key)
                .cloned()
                .ok_or_else(|| McpError::invalid_params(format!("No value for {key}"))),
            "write" => {
                if !ctx.has_any_role(&["writer"]) {
                    return Err(McpError::permission_denied("Requires the writer role"));
                }
                let value = value.ok_or_else(|| McpError::invalid_params("write needs a value"))?;
                self.data.write().await.insert(key, value);
                Ok("stored".to_string())
            }
            other => Err(McpError::invalid_params(format!("Unknown action: {other}"))),
        }
    }
}
```

## Advanced Caching Patterns

### Multi-Level Cache

Check the fast cache, then the slower one, then the source, warming each level
on the way back. See [Real-World Patterns](../examples/patterns.md#multi-level-cache)
for a version with Redis; this one uses two in-process tiers:

```rust
use moka::future::Cache;
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Users {
    hot: Cache<String, String>,  // small, short-lived
    warm: Cache<String, String>, // large, long-lived
}

#[server]
impl Users {
    fn new() -> Self {
        Self {
            hot: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(30))
                .build(),
            warm: Cache::builder()
                .max_capacity(100_000)
                .time_to_live(Duration::from_secs(3_600))
                .build(),
        }
    }

    /// Fetch a user profile.
    #[tool]
    pub async fn get_user(&self, user_id: String) -> McpResult<String> {
        let key = format!("user:{user_id}");

        // Level 1
        if let Some(user) = self.hot.get(&key).await {
            return Ok(user);
        }

        // Level 2: restore to L1 for next time
        if let Some(user) = self.warm.get(&key).await {
            self.hot.insert(key, user.clone()).await;
            return Ok(user);
        }

        // Level 3: the source; warm both caches
        let user = self.fetch_from_database(&user_id).await?;
        self.warm.insert(key.clone(), user.clone()).await;
        self.hot.insert(key, user.clone()).await;
        Ok(user)
    }

    async fn fetch_from_database(&self, user_id: &str) -> McpResult<String> {
        Ok(format!(r#"{{"id":"{user_id}"}}"#))
    }
}
```

This uses `moka` with its `future` feature.

### Cache Invalidation

Invalidate every entry derived from data you change, then tell subscribed
clients:

```rust
use moka::future::Cache;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Profiles {
    cache: Cache<String, String>,
}

#[server]
impl Profiles {
    /// Update a user's display name.
    #[tool]
    pub async fn update_user(
        &self,
        user_id: String,
        display_name: String,
        ctx: &RequestContext,
    ) -> McpResult<String> {
        // Update the source of truth first (database write elided)

        // Invalidate all related cache entries
        self.cache.invalidate(&format!("user:{user_id}")).await;
        self.cache.invalidate(&format!("user:profile:{user_id}")).await;
        self.cache.invalidate("users:list").await;

        // Clients subscribed to the resource are told it changed
        ctx.notify_resource_updated(format!("users://{user_id}")).await.ok();

        Ok(format!("{user_id} is now {display_name}"))
    }
}
```

### Cache Stampede Prevention

When many requests miss the same key at once, compute it once. `moka`'s
`get_with` runs the initializer for one caller and makes the others wait for its
result:

```rust
use moka::future::Cache;
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Reports {
    cache: Cache<String, String>,
}

#[server]
impl Reports {
    /// An expensive report, computed at most once per key at a time.
    #[tool]
    pub async fn get_expensive_data(&self, key: String) -> McpResult<String> {
        let report = self
            .cache
            .get_with(key.clone(), async move {
                // Simulate the expensive computation
                tokio::time::sleep(Duration::from_secs(2)).await;
                format!("report for {key}")
            })
            .await;
        Ok(report)
    }
}
```

## Large Results & Backpressure

### Large Datasets

A tool result is one message; MCP has no streamed tool output. For large
datasets, page through them with a cursor argument, or return resource links the
client reads as it needs them. Report progress while the work runs:

```rust
use serde_json::json;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Dataset;

const PAGE: usize = 100;

#[server]
impl Dataset {
    /// Rows matching a query, `PAGE` at a time. Pass back `next_cursor` for more.
    #[tool]
    pub async fn query_rows(
        &self,
        query: String,
        cursor: Option<String>,
        ctx: &RequestContext,
    ) -> McpResult<serde_json::Value> {
        let offset: usize = cursor.as_deref().unwrap_or("0").parse().map_err(|_| {
            McpError::invalid_params("cursor must come from a previous next_cursor")
        })?;

        let total = 1_000; // rows matching `query`
        let rows: Vec<String> = (offset..(offset + PAGE).min(total))
            .map(|i| format!("{query} row {i}"))
            .collect();
        ctx.report_progress(rows.len() as f64, Some(rows.len() as f64), None)
            .await?;

        let next = offset + rows.len();
        Ok(json!({
            "rows": rows,
            "next_cursor": (next < total).then(|| next.to_string()),
        }))
    }
}
```

### Backpressure Handling

A bounded channel slows the producer to the consumer's pace:

```rust
use tokio::sync::mpsc;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Pipeline;

async fn process_item(item: u32) -> McpResult<()> {
    let _ = item;
    Ok(())
}

#[server]
impl Pipeline {
    /// Produce and consume 1000 items.
    #[tool]
    pub async fn producer_consumer_pattern(&self, ctx: &RequestContext) -> McpResult<String> {
        let (tx, mut rx) = mpsc::channel(100); // Bounded channel = backpressure

        // Producer task
        tokio::spawn(async move {
            for i in 0..1000 {
                if tx.send(i).await.is_err() {
                    // Consumer dropped, stop producing
                    break;
                }
            }
        });

        // Consumer processes at its own pace, and stops if the client cancels
        let mut count = 0;
        while let Some(item) = rx.recv().await {
            if ctx.is_cancelled() {
                return Err(McpError::cancelled("Cancelled by client"));
            }
            process_item(item).await?;
            count += 1;
        }

        Ok(format!("Processed {} items", count))
    }
}
```

## Error Recovery & Resilience

### Exponential Backoff Retry

Retry only errors that can succeed on a second attempt (`is_retryable()`):

```rust
use std::future::Future;
use std::time::Duration;
use turbomcp::prelude::*;

async fn with_exponential_backoff<F, Fut, T>(mut operation: F, max_retries: u32) -> McpResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = McpResult<T>>,
{
    let mut retries = 0;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if e.is_retryable() && retries < max_retries => {
                let backoff = Duration::from_millis(2u64.pow(retries) * 100);
                tokio::time::sleep(backoff).await;
                retries += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[derive(Clone)]
pub struct Api {
    client: reqwest::Client,
}

#[server]
impl Api {
    /// Fetch a URL, retrying transient failures.
    #[tool]
    pub async fn resilient_api_call(&self, url: String) -> McpResult<String> {
        with_exponential_backoff(
            || async {
                self.client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|e| McpError::unavailable(e.to_string()))?
                    .text()
                    .await
                    .map_err(|e| McpError::unavailable(e.to_string()))
            },
            3,
        )
        .await
    }
}
```

### Circuit Breaker

Stop calling a failing dependency for a while instead of piling up timeouts:

```rust
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use turbomcp::prelude::*;

pub struct CircuitBreaker {
    failures: AtomicU32,
    threshold: u32,
    reset_timeout: Duration,
    opened_at: Mutex<Option<Instant>>,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            failures: AtomicU32::new(0),
            threshold,
            reset_timeout,
            opened_at: Mutex::new(None),
        }
    }

    pub async fn call<F, Fut, T>(&self, f: F) -> McpResult<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = McpResult<T>>,
    {
        if let Some(opened) = *self.opened_at.lock().unwrap() {
            if opened.elapsed() < self.reset_timeout {
                return Err(McpError::unavailable("Circuit open"));
            }
        }

        match f().await {
            Ok(result) => {
                self.failures.store(0, Ordering::SeqCst);
                *self.opened_at.lock().unwrap() = None;
                Ok(result)
            }
            Err(e) => {
                if self.failures.fetch_add(1, Ordering::SeqCst) + 1 >= self.threshold {
                    *self.opened_at.lock().unwrap() = Some(Instant::now());
                }
                Err(e)
            }
        }
    }
}
```

Keep one breaker per dependency in an `Arc` on your server struct. On the client
side, `ClientBuilder::build_resilient` provides retry and a circuit breaker for
the transport.

## Performance Optimization

### Get-or-Compute Helper

Wrap a cache and a metric in one helper instead of repeating the pattern:

```rust
use moka::future::Cache;
use std::future::Future;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Cached {
    cache: Cache<String, String>,
}

impl Cached {
    pub async fn get_or_compute<F, Fut>(&self, key: &str, compute: F) -> McpResult<String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = McpResult<String>>,
    {
        if let Some(cached) = self.cache.get(key).await {
            tracing::debug!(key, "cache hit");
            return Ok(cached);
        }

        tracing::debug!(key, "cache miss");
        let result = compute().await?;
        self.cache.insert(key.to_string(), result.clone()).await;
        Ok(result)
    }
}
```

### Batch Processing

Take a batch in one call and report per-item outcomes, so one bad item does not
fail the rest:

```rust
use schemars::JsonSchema;
use turbomcp::prelude::*;

#[derive(Deserialize, JsonSchema)]
pub struct Item {
    id: u64,
    name: String,
}

#[derive(Clone)]
pub struct Importer;

#[server]
impl Importer {
    /// Import many items at once.
    #[tool]
    pub async fn batch_process(&self, items: Vec<Item>) -> McpResult<serde_json::Value> {
        let results: Vec<_> = items
            .into_iter()
            .map(|item| {
                if item.name.is_empty() {
                    serde_json::json!({ "id": item.id, "error": "empty name" })
                } else {
                    serde_json::json!({ "id": item.id, "ok": true })
                }
            })
            .collect();

        Ok(serde_json::json!({ "results": results }))
    }
}
```

### Connection Pooling

Create pools once, at startup, and keep them on the server struct; every clone
of the server shares them. Independent queries can run concurrently on one pool:

```rust
use sqlx::PgPool;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Stats {
    db: PgPool,
}

#[server]
impl Stats {
    /// Count rows in three tables at once.
    #[tool]
    pub async fn table_counts(&self) -> McpResult<String> {
        let count = |table: &'static str| {
            sqlx::query_scalar::<_, i64>(match table {
                "users" => "SELECT COUNT(*) FROM users",
                "orders" => "SELECT COUNT(*) FROM orders",
                _ => "SELECT COUNT(*) FROM products",
            })
            .fetch_one(&self.db)
        };

        let (users, orders, products) =
            tokio::try_join!(count("users"), count("orders"), count("products"))
                .map_err(|e| McpError::internal(e.to_string()))?;

        Ok(format!("{users} users, {orders} orders, {products} products"))
    }
}
```

This uses `sqlx` with the `postgres` and `runtime-tokio` features.

## Testing Advanced Patterns

### Testing Through Dispatch

`McpTestClient` calls a server the way a client would, argument validation
included, without a transport:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone, Default)]
pub struct Kv {
    data: Arc<RwLock<HashMap<String, String>>>,
}

#[server]
impl Kv {
    /// Store a value.
    #[tool]
    pub async fn put(&self, key: String, value: String) -> String {
        self.data.write().await.insert(key, value);
        "ok".to_string()
    }

    /// Read a value.
    #[tool]
    pub async fn get(&self, key: String) -> McpResult<String> {
        self.data
            .read()
            .await
            .get(&key)
            .cloned()
            .ok_or_else(|| McpError::invalid_params("missing"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn put_then_get() {
        let client = McpTestClient::new(Kv::default());

        client.call_tool("put", json!({ "key": "a", "value": "1" })).await.unwrap();
        let result = client.call_tool("get", json!({ "key": "a" })).await.unwrap();
        assert_eq!(result.first_text(), Some("1"));

        // A tool error is a result with isError set, not an Err
        let missing = client.call_tool("get", json!({ "key": "b" })).await.unwrap();
        assert!(missing.is_error());
    }
}
```

To swap a dependency in tests, put it behind a trait object on the server struct
(`Arc<dyn Storage>`) and construct the server with a fake.

### Performance Testing

Benchmark with `criterion` on stable Rust (`#[bench]` needs nightly):

```rust,ignore
use criterion::{Criterion, criterion_group, criterion_main};
use turbomcp::prelude::*;

fn bench_tool_call(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let client = McpTestClient::new(Kv::default());

    c.bench_function("put", |b| {
        b.iter(|| {
            runtime
                .block_on(client.call_tool("put", serde_json::json!({ "key": "k", "value": "v" })))
                .unwrap()
        })
    });
}

criterion_group!(benches, bench_tool_call);
criterion_main!(benches);
```

This is a `benches/` file: it assumes the `Kv` server above is importable from
your crate and `criterion` is a dev-dependency.

## Debugging Advanced Patterns

### Request Tracing

Attach the request ID to a span so every event in the workflow carries it:

```rust
use tracing::Instrument;
use turbomcp::prelude::*;

#[derive(Clone)]
pub struct Flow;

#[server]
impl Flow {
    /// A multi-step workflow.
    #[tool]
    pub async fn complex_workflow(&self, ctx: &RequestContext) -> McpResult<String> {
        let span = tracing::info_span!("workflow", request_id = %ctx.request_id(), name = "complex");
        async {
            tracing::info!("Starting workflow");
            // Sub-operations awaited in here are inside the span
            Ok("Complete".to_string())
        }
        .instrument(span)
        .await
    }
}
```

### Timing Operations

Record how long expensive work takes (with `metrics` for Prometheus, or a
`tracing` field):

```rust
use std::time::Instant;
use turbomcp::prelude::*;

async fn expensive_computation() -> McpResult<String> {
    Ok("done".to_string())
}

#[derive(Clone)]
pub struct Heavy;

#[server]
impl Heavy {
    /// Run the expensive computation.
    #[tool]
    pub async fn memory_intensive_operation(&self) -> McpResult<String> {
        let started = Instant::now();
        let result = expensive_computation().await?;
        tracing::info!(elapsed_ms = started.elapsed().as_millis() as u64, "computation finished");
        Ok(result)
    }
}
```

## Best Practices

### 1. Keep Functions Pure When Possible

```rust
// ✅ Pure function - easy to test
fn calculate_discount(price: f64, percentage: f64) -> f64 {
    price * (1.0 - percentage / 100.0)
}

// Handlers gather inputs and call the pure core
async fn apply_discount(price: f64, load_percentage: impl AsyncFn() -> Option<f64>) -> f64 {
    calculate_discount(price, load_percentage().await.unwrap_or(0.0))
}
```

### 2. Use Arc for Shared State

```rust
use std::sync::Arc;

#[derive(Clone)]
struct Config {
    entries: Vec<String>,
}

let config = Arc::new(Config { entries: vec!["a".into(); 1000] });

// ✅ Efficient cloning via Arc
let config1 = Arc::clone(&config);

// ❌ Expensive cloning: copies all config data
let copy: Config = (*config).clone();
```

### 3. Minimize Lock Contention

```rust
use tokio::sync::RwLock;

let config = RwLock::new(vec![1, 2, 3]);

// ✅ Read lock for non-mutable access
{
    let data = config.read().await;
}

// ❌ Write lock when read would suffice
{
    let data = config.write().await;
}
```

## Next Steps

- **[Observability](observability.md)** - Monitor advanced patterns
- **[Examples](../examples/patterns.md)** - Real-world advanced patterns
- **[Architecture](../architecture/system-design.md)** - Design implications

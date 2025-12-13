# Advanced Patterns & Optimization

Master advanced TurboMCP patterns for complex workflows and performance optimization.

## Workflow Orchestration

### Sequential Tool Chaining

Chain multiple tools together with dependency injection:

```rust
#[server]
pub struct WeatherWorkflow;

#[tool]
pub async fn get_weather(
    city: String,
    #[description("The weather service client")] client: HttpClient,
) -> McpResult<WeatherData> {
    let data = client.get(&format!("https://api.weather.com/{}", city))
        .await?;
    Ok(data)
}

#[tool]
pub async fn get_forecast(
    weather: WeatherData,
    client: HttpClient,
) -> McpResult<ForecastData> {
    let forecast = client.get(&format!("https://api.weather.com/forecast/{}", weather.location))
        .await?;
    Ok(forecast)
}
```

### Parallel Operations

Execute multiple independent operations concurrently:

```rust
use tokio::try_join;

#[tool]
pub async fn get_weather_and_news(
    city: String,
    client: HttpClient,
) -> McpResult<(WeatherData, NewsData)> {
    let weather_future = async {
        client.get(&format!("https://api.weather.com/{}", city)).await
    };

    let news_future = async {
        client.get(&format!("https://api.news.com/{}", city)).await
    };

    // Execute both concurrently
    let (weather, news) = try_join!(weather_future, news_future)?;

    Ok((weather, news))
}
```

### Conditional Workflows

Implement branching logic based on context:

```rust
#[tool]
pub async fn smart_action(
    action_type: String,
    ctx: Context,
) -> McpResult<String> {
    match action_type.as_str() {
        "read" => {
            let cache = ctx.cache();
            match cache.get("data").await? {
                Some(cached) => Ok(cached),
                None => fetch_and_cache(&ctx).await,
            }
        }
        "write" => {
            validate_permissions(&ctx).await?;
            persist_data(&ctx).await
        }
        _ => Err(McpError::InvalidInput("Unknown action".into())),
    }
}
```

## Advanced Caching Patterns

### Multi-Level Cache

Implement cache warming and fallback chains:

```rust
#[tool]
pub async fn get_user(
    user_id: String,
    cache: Cache,
) -> McpResult<User> {
    // Level 1: In-memory cache
    if let Some(user) = cache.get(&format!("user:{}", user_id)).await? {
        return Ok(user);
    }

    // Level 2: Redis cache
    if let Some(user) = cache.get_secondary(&format!("user:{}", user_id)).await? {
        // Restore to L1 for next time
        cache.set(&format!("user:{}", user_id), user.clone()).await?;
        return Ok(user);
    }

    // Level 3: Database fetch
    let user = fetch_from_database(&user_id).await?;

    // Warm caches
    cache.set(&format!("user:{}", user_id), user.clone()).await?;

    Ok(user)
}
```

### Cache Invalidation

Handle cache updates and invalidation:

```rust
#[tool]
pub async fn update_user(
    user_id: String,
    updates: UserUpdate,
    cache: Cache,
) -> McpResult<User> {
    // Update database
    let updated_user = persist_to_database(&user_id, updates).await?;

    // Invalidate all related cache entries
    cache.delete(&format!("user:{}", user_id)).await?;
    cache.delete(&format!("user:profile:{}", user_id)).await?;
    cache.delete("users:list").await?;

    Ok(updated_user)
}
```

### Cache Stampede Prevention

Prevent thundering herd with locking:

```rust
#[tool]
pub async fn get_expensive_data(
    key: String,
    cache: Cache,
) -> McpResult<Data> {
    // Try cache first
    if let Some(data) = cache.get(&key).await? {
        return Ok(data);
    }

    // Lock to prevent stampede
    if let Some(_lock) = cache.acquire_lock(&format!("lock:{}", key)).await? {
        // Double-check pattern
        if let Some(data) = cache.get(&key).await? {
            return Ok(data);
        }

        // Compute expensive data
        let data = compute_expensive_data().await?;

        // Cache it
        cache.set(&key, data.clone()).await?;

        return Ok(data);
    }

    // Another request is computing, wait and retry
    tokio::time::sleep(Duration::from_millis(100)).await;
    get_expensive_data(key, cache).await
}
```

## Streaming & Backpressure

### Streaming Large Datasets

Handle large result sets efficiently:

```rust
#[tool]
pub async fn stream_large_dataset(
    query: String,
) -> McpResult<impl Stream<Item = Result<DataRow, McpError>>> {
    let stream = database.stream_query(&query)
        .await?
        .map(|row| Ok(DataRow::from(row)))
        .boxed();

    Ok(stream)
}
```

### Backpressure Handling

Implement flow control for producer/consumer patterns:

```rust
use tokio::sync::mpsc;

#[tool]
pub async fn producer_consumer_pattern(
    ctx: Context,
) -> McpResult<String> {
    let (tx, mut rx) = mpsc::channel(100);  // Bounded channel = backpressure

    // Producer task
    tokio::spawn(async move {
        for i in 0..1000 {
            if tx.send(i).await.is_err() {
                // Consumer dropped, stop producing
                break;
            }
        }
    });

    // Consumer processes at own pace
    let mut count = 0;
    while let Some(item) = rx.recv().await {
        process_item(item).await?;
        count += 1;
    }

    Ok(format!("Processed {} items", count))
}
```

## Error Recovery & Resilience

### Exponential Backoff Retry

Implement intelligent retry with backoff:

```rust
async fn with_exponential_backoff<F, T>(
    mut operation: F,
    max_retries: u32,
) -> McpResult<T>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<T>>>>,
{
    let mut retries = 0;

    loop {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) if retries < max_retries => {
                let backoff = Duration::from_millis(2u64.pow(retries) * 100);
                tokio::time::sleep(backoff).await;
                retries += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[tool]
pub async fn resilient_api_call(
    url: String,
    client: HttpClient,
) -> McpResult<String> {
    with_exponential_backoff(
        || Box::pin(client.get(&url)),
        3,
    ).await
}
```

### Circuit Breaker

Implement circuit breaker pattern:

```rust
pub struct CircuitBreaker {
    failures: Arc<AtomicU32>,
    threshold: u32,
    reset_timeout: Duration,
}

impl CircuitBreaker {
    pub async fn call<F, T>(&self, f: F) -> McpResult<T>
    where
        F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<T>>>>,
    {
        if self.failures.load(Ordering::SeqCst) > self.threshold {
            return Err(McpError::ServiceUnavailable("Circuit open".into()));
        }

        match f().await {
            Ok(result) => {
                self.failures.store(0, Ordering::SeqCst);
                Ok(result)
            }
            Err(e) => {
                self.failures.fetch_add(1, Ordering::SeqCst);
                Err(e)
            }
        }
    }
}
```

## Performance Optimization

### Custom Context Injection

Build specialized contexts for different scenarios:

```rust
pub struct CachedContext {
    base: Context,
    cache: Cache,
    metrics: Metrics,
}

impl CachedContext {
    pub async fn get_or_compute<F, T>(
        &self,
        key: &str,
        compute: F,
    ) -> McpResult<T>
    where
        F: Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<T>>>>,
        T: Serialize + for<'de> Deserialize<'de>,
    {
        if let Some(cached) = self.cache.get(key).await? {
            self.metrics.increment("cache_hit", 1)?;
            return Ok(cached);
        }

        self.metrics.increment("cache_miss", 1)?;
        let result = compute().await?;
        self.cache.set(key, result.clone()).await?;

        Ok(result)
    }
}
```

### Batch Processing

Process requests in batches for efficiency:

```rust
#[tool]
pub async fn batch_process(
    items: Vec<Item>,
    database: Database,
) -> McpResult<Vec<Result<ProcessedItem, String>>> {
    // Batch insert is more efficient than individual inserts
    let results = database
        .batch_insert(items)
        .await?
        .into_iter()
        .map(|r| r.map_err(|e| e.to_string()))
        .collect();

    Ok(results)
}
```

### Connection Pooling

Reuse connections efficiently:

```rust
#[tool]
pub async fn with_pooled_connection(
    database: Database,
) -> McpResult<String> {
    // Database connection pooling is automatic
    // Multiple concurrent requests reuse connections

    let results = futures::future::try_join_all(vec![
        database.query("SELECT * FROM users"),
        database.query("SELECT * FROM orders"),
        database.query("SELECT * FROM products"),
    ])
    .await?;

    Ok(format!("Executed {} queries", results.len()))
}
```

## Testing Advanced Patterns

### Mocking Complex Dependencies

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_workflow_with_mocks() {
        let cache = Arc::new(MockCache::new());
        let client = Arc::new(MockHttpClient::with_responses(vec![
            MockResponse::success("user_data"),
        ]));

        let result = get_user("123".to_string(), cache.clone())
            .await
            .unwrap();

        assert_eq!(cache.hit_count(), 0);  // Not cached yet
        assert_eq!(client.call_count(), 1);
    }
}
```

### Performance Testing

```rust
#[bench]
fn bench_cache_operations(b: &mut Bencher) {
    let cache = Cache::new();

    b.iter(|| {
        // Measure cache get/set performance
        futures::executor::block_on(async {
            cache.set("key", "value").await.unwrap();
            cache.get("key").await.unwrap();
        })
    });
}
```

## Debugging Advanced Patterns

### Request Tracing

Use context correlation for complex workflows:

```rust
#[tool]
pub async fn complex_workflow(
    info: RequestInfo,
    logger: Logger,
) -> McpResult<String> {
    logger.with_field("request_id", &info.request_id)
        .with_field("workflow", "complex")
        .info("Starting workflow")
        .await?;

    // All async sub-operations automatically share request_id
    // via context, making the entire flow traceable

    Ok("Complete".to_string())
}
```

### Memory Profiling

Monitor memory usage of complex operations:

```rust
#[tool]
pub async fn memory_intensive_operation(
    metrics: Metrics,
) -> McpResult<String> {
    let start_memory = get_memory_usage();

    // Do work
    let result = expensive_computation().await?;

    let end_memory = get_memory_usage();
    metrics.record("memory_delta_mb", end_memory - start_memory)?;

    Ok(result)
}
```

## Best Practices

### 1. Keep Functions Pure When Possible

```rust
// ✅ Pure function - easy to test
fn calculate_discount(price: f64, percentage: f64) -> f64 {
    price * (1.0 - percentage / 100.0)
}

// ❌ Impure - depends on external state
async fn apply_discount(price: f64, cache: Cache) -> McpResult<f64> {
    let percentage = cache.get("discount_percentage").await?;
    Ok(calculate_discount(price, percentage.unwrap_or(0.0)))
}
```

### 2. Use Arc for Shared State

```rust
// ✅ Efficient cloning via Arc
let config = Arc::new(Config::load().await?);
let config1 = config.clone();
let config2 = config.clone();

// ❌ Expensive cloning
let config = Config::load().await?;
let config1 = config.clone();  // Allocates all config data
```

### 3. Minimize Lock Contention

```rust
// ✅ Read lock for non-mutable access
let data = config.read().await;

// ❌ Write lock when read would suffice
let data = config.write().await;
```

## Next Steps

- **[Observability](observability.md)** - Monitor advanced patterns
- **[Examples](../examples/patterns.md)** - Real-world advanced patterns
- **[Architecture](../architecture/system-design.md)** - Design implications


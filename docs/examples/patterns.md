# Real-World Patterns

Practical patterns and best practices for building production-ready MCP servers with TurboMCP.

## State Management

### Shared Mutable State

Use `Arc<RwLock<T>>` for thread-safe shared state across requests:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone)]
struct CounterServer {
    counters: Arc<RwLock<HashMap<String, i64>>>,
}

#[server(name = "counter", version = "1.0.0")]
impl CounterServer {
    fn new() -> Self {
        Self {
            counters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[tool("Increment a counter by name")]
    async fn increment(&self, name: String) -> McpResult<i64> {
        let mut counters = self.counters.write().await;
        let counter = counters.entry(name).or_insert(0);
        *counter += 1;
        Ok(*counter)
    }

    #[tool("Get current counter value")]
    async fn get(&self, name: String) -> McpResult<i64> {
        let counters = self.counters.read().await;
        Ok(*counters.get(&name).unwrap_or(&0))
    }

    #[tool("Reset a counter")]
    async fn reset(&self, name: String) -> McpResult<String> {
        let mut counters = self.counters.write().await;
        counters.remove(&name);
        Ok(format!("Counter '{}' reset", name))
    }
}
```

**Best Practices:**
- Use `RwLock` instead of `Mutex` when reads are more common than writes
- Keep lock scopes minimal to prevent blocking
- Consider using `DashMap` for concurrent hashmaps without explicit locking
- Clone the `Arc` cheaply when passing state around

### Session-Scoped State

Store per-session data using request context:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct SessionServer {
    sessions: Arc<RwLock<HashMap<String, SessionData>>>,
}

#[derive(Clone, Debug)]
struct SessionData {
    user_id: String,
    preferences: HashMap<String, String>,
    last_activity: std::time::Instant,
}

#[server]
impl SessionServer {
    #[tool("Store preference in session")]
    async fn set_preference(
        &self,
        ctx: Context,
        key: String,
        value: String,
    ) -> McpResult<String> {
        // Extract session ID from context
        let session_id = ctx.request_id().to_string();

        let mut sessions = self.sessions.write().await;
        let session = sessions.entry(session_id).or_insert_with(|| SessionData {
            user_id: String::new(),
            preferences: HashMap::new(),
            last_activity: std::time::Instant::now(),
        });

        session.preferences.insert(key.clone(), value.clone());
        session.last_activity = std::time::Instant::now();

        Ok(format!("Set {}: {}", key, value))
    }

    #[tool("Get preference from session")]
    async fn get_preference(&self, ctx: Context, key: String) -> McpResult<String> {
        let session_id = ctx.request_id().to_string();
        let sessions = self.sessions.read().await;

        if let Some(session) = sessions.get(&session_id) {
            if let Some(value) = session.preferences.get(&key) {
                return Ok(value.clone());
            }
        }

        Err(McpError::invalid_request("Preference not found"))
    }
}
```

### Database-Backed State

Integrate with databases for persistent state:

```rust
use sqlx::{Pool, Postgres};
use std::sync::Arc;

#[derive(Clone)]
struct DatabaseServer {
    db: Arc<Pool<Postgres>>,
}

#[server]
impl DatabaseServer {
    async fn new(database_url: &str) -> McpResult<Self> {
        let db = Pool::connect(database_url).await
            .map_err(|e| McpError::internal_error(format!("DB connection failed: {}", e)))?;

        Ok(Self {
            db: Arc::new(db),
        })
    }

    #[tool("Store user data")]
    async fn create_user(&self, name: String, email: String) -> McpResult<i64> {
        let result = sqlx::query!(
            "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING id",
            name,
            email
        )
        .fetch_one(&*self.db)
        .await
        .map_err(|e| McpError::internal_error(format!("DB error: {}", e)))?;

        Ok(result.id)
    }

    #[tool("Get user by ID")]
    async fn get_user(&self, user_id: i64) -> McpResult<serde_json::Value> {
        let user = sqlx::query!(
            "SELECT id, name, email FROM users WHERE id = $1",
            user_id
        )
        .fetch_optional(&*self.db)
        .await
        .map_err(|e| McpError::internal_error(format!("DB error: {}", e)))?
        .ok_or_else(|| McpError::invalid_request("User not found"))?;

        Ok(serde_json::json!({
            "id": user.id,
            "name": user.name,
            "email": user.email
        }))
    }
}
```

## Caching Patterns

### In-Memory Caching

Use `moka` or `cached` crate for efficient caching:

```rust
use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
struct CachedServer {
    cache: Arc<Cache<String, String>>,
}

#[server]
impl CachedServer {
    fn new() -> Self {
        let cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(300)) // 5 minutes
            .time_to_idle(Duration::from_secs(60))  // 1 minute idle
            .build();

        Self {
            cache: Arc::new(cache),
        }
    }

    #[tool("Fetch with caching")]
    async fn fetch_data(&self, key: String) -> McpResult<String> {
        // Try cache first
        if let Some(cached) = self.cache.get(&key).await {
            return Ok(format!("[CACHED] {}", cached));
        }

        // Simulate expensive operation
        let data = self.expensive_operation(&key).await?;

        // Store in cache
        self.cache.insert(key.clone(), data.clone()).await;

        Ok(format!("[FRESH] {}", data))
    }

    async fn expensive_operation(&self, key: &str) -> McpResult<String> {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(format!("Data for {}", key))
    }

    #[tool("Invalidate cache entry")]
    async fn invalidate(&self, key: String) -> McpResult<String> {
        self.cache.invalidate(&key).await;
        Ok(format!("Invalidated cache for {}", key))
    }

    #[tool("Clear entire cache")]
    async fn clear_cache(&self) -> McpResult<String> {
        self.cache.invalidate_all();
        Ok("Cache cleared".to_string())
    }
}
```

### Multi-Level Cache

Implement cache layering with fallback:

```rust
use moka::future::Cache;
use redis::AsyncCommands;
use std::sync::Arc;

#[derive(Clone)]
struct MultiLevelCache {
    l1_cache: Arc<Cache<String, String>>, // Local memory
    redis: Arc<redis::Client>,             // Shared cache
}

#[server]
impl MultiLevelCache {
    #[tool("Get with multi-level caching")]
    async fn get_data(&self, key: String) -> McpResult<String> {
        // L1: Check local memory cache
        if let Some(value) = self.l1_cache.get(&key).await {
            return Ok(format!("[L1 CACHE] {}", value));
        }

        // L2: Check Redis
        let mut conn = self.redis.get_multiplexed_async_connection().await
            .map_err(|e| McpError::internal_error(format!("Redis error: {}", e)))?;

        if let Ok(Some(value)) = conn.get::<_, Option<String>>(&key).await {
            // Store in L1 for next time
            self.l1_cache.insert(key.clone(), value.clone()).await;
            return Ok(format!("[L2 CACHE] {}", value));
        }

        // L3: Fetch from source
        let value = self.fetch_from_source(&key).await?;

        // Store in both caches
        self.l1_cache.insert(key.clone(), value.clone()).await;
        let _: () = conn.set_ex(&key, &value, 300).await
            .map_err(|e| McpError::internal_error(format!("Redis error: {}", e)))?;

        Ok(format!("[SOURCE] {}", value))
    }

    async fn fetch_from_source(&self, key: &str) -> McpResult<String> {
        // Simulate database or API call
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(format!("Fresh data for {}", key))
    }
}
```

### Cache Warming

Pre-populate cache on startup:

```rust
#[derive(Clone)]
struct WarmCacheServer {
    cache: Arc<Cache<String, String>>,
}

#[server]
impl WarmCacheServer {
    async fn new() -> McpResult<Self> {
        let cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(3600))
            .build();

        let server = Self {
            cache: Arc::new(cache),
        };

        // Warm the cache on startup
        server.warm_cache().await?;

        Ok(server)
    }

    async fn warm_cache(&self) -> McpResult<()> {
        let popular_keys = vec!["homepage", "pricing", "docs", "api"];

        for key in popular_keys {
            let data = self.fetch_data(key).await?;
            self.cache.insert(key.to_string(), data).await;
        }

        Ok(())
    }

    async fn fetch_data(&self, key: &str) -> McpResult<String> {
        // Simulate fetching
        Ok(format!("Content for {}", key))
    }
}
```

## Validation Patterns

### Input Validation

Validate inputs early and provide clear error messages:

```rust
#[server]
impl ValidationServer {
    #[tool("Create user with comprehensive validation")]
    async fn create_user(
        &self,
        username: String,
        email: String,
        age: i32,
    ) -> McpResult<String> {
        // Validate username
        self.validate_username(&username)?;

        // Validate email
        self.validate_email(&email)?;

        // Validate age
        if age < 18 {
            return Err(McpError::invalid_request("Must be 18 or older"));
        }
        if age > 120 {
            return Err(McpError::invalid_request("Invalid age"));
        }

        Ok(format!("User created: {} ({})", username, email))
    }

    fn validate_username(&self, username: &str) -> McpResult<()> {
        if username.len() < 3 {
            return Err(McpError::invalid_request(
                "Username must be at least 3 characters"
            ));
        }

        if username.len() > 20 {
            return Err(McpError::invalid_request(
                "Username must be 20 characters or less"
            ));
        }

        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(McpError::invalid_request(
                "Username can only contain letters, numbers, and underscores"
            ));
        }

        Ok(())
    }

    fn validate_email(&self, email: &str) -> McpResult<()> {
        if !email.contains('@') || !email.contains('.') {
            return Err(McpError::invalid_request("Invalid email format"));
        }

        let parts: Vec<&str> = email.split('@').collect();
        if parts.len() != 2 {
            return Err(McpError::invalid_request("Invalid email format"));
        }

        Ok(())
    }
}
```

### Type-Safe Validation with Serde

Use serde and validator crate for complex validation:

```rust
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, Validate)]
struct UserRegistration {
    #[validate(length(min = 3, max = 20))]
    username: String,

    #[validate(email)]
    email: String,

    #[validate(range(min = 18, max = 120))]
    age: i32,

    #[validate(length(min = 8))]
    password: String,
}

#[server]
impl ValidatedServer {
    #[tool("Register with type-safe validation")]
    async fn register(&self, data: UserRegistration) -> McpResult<String> {
        // Validate using validator crate
        data.validate()
            .map_err(|e| McpError::invalid_request(format!("Validation failed: {}", e)))?;

        Ok(format!("User {} registered successfully", data.username))
    }
}
```

### Business Logic Validation

Implement custom business rules:

```rust
#[derive(Clone)]
struct BusinessValidator {
    db: Arc<Pool<Postgres>>,
}

#[server]
impl BusinessValidator {
    #[tool("Create order with business validation")]
    async fn create_order(
        &self,
        user_id: i64,
        product_id: i64,
        quantity: i32,
    ) -> McpResult<String> {
        // Validate quantity
        if quantity <= 0 {
            return Err(McpError::invalid_request("Quantity must be positive"));
        }

        // Check user exists
        let user_exists = self.check_user_exists(user_id).await?;
        if !user_exists {
            return Err(McpError::invalid_request("User not found"));
        }

        // Check product availability
        let available = self.check_product_stock(product_id).await?;
        if available < quantity {
            return Err(McpError::invalid_request(
                format!("Only {} units available", available)
            ));
        }

        // Check user credit limit
        let credit_ok = self.check_credit_limit(user_id, product_id, quantity).await?;
        if !credit_ok {
            return Err(McpError::invalid_request("Credit limit exceeded"));
        }

        // Create order
        let order_id = self.create_order_internal(user_id, product_id, quantity).await?;

        Ok(format!("Order {} created successfully", order_id))
    }

    async fn check_user_exists(&self, user_id: i64) -> McpResult<bool> {
        let result = sqlx::query!("SELECT id FROM users WHERE id = $1", user_id)
            .fetch_optional(&*self.db)
            .await
            .map_err(|e| McpError::internal_error(format!("DB error: {}", e)))?;
        Ok(result.is_some())
    }

    async fn check_product_stock(&self, product_id: i64) -> McpResult<i32> {
        let result = sqlx::query!(
            "SELECT stock FROM products WHERE id = $1",
            product_id
        )
        .fetch_optional(&*self.db)
        .await
        .map_err(|e| McpError::internal_error(format!("DB error: {}", e)))?
        .ok_or_else(|| McpError::invalid_request("Product not found"))?;

        Ok(result.stock)
    }

    async fn check_credit_limit(
        &self,
        user_id: i64,
        product_id: i64,
        quantity: i32,
    ) -> McpResult<bool> {
        // Business logic to check credit limit
        Ok(true) // Simplified
    }

    async fn create_order_internal(
        &self,
        user_id: i64,
        product_id: i64,
        quantity: i32,
    ) -> McpResult<i64> {
        let result = sqlx::query!(
            "INSERT INTO orders (user_id, product_id, quantity) VALUES ($1, $2, $3) RETURNING id",
            user_id,
            product_id,
            quantity
        )
        .fetch_one(&*self.db)
        .await
        .map_err(|e| McpError::internal_error(format!("DB error: {}", e)))?;

        Ok(result.id)
    }
}
```

## Multi-Handler Workflows

### Sequential Tool Chaining

Chain tools together with intermediate results:

```rust
#[derive(Clone)]
struct WorkflowServer {
    http_client: reqwest::Client,
}

#[server]
impl WorkflowServer {
    #[tool("Get weather forecast (multi-step)")]
    async fn get_forecast(&self, city: String) -> McpResult<String> {
        // Step 1: Geocode the city
        let coords = self.geocode_city(&city).await?;

        // Step 2: Fetch current weather
        let weather = self.fetch_weather(&coords).await?;

        // Step 3: Fetch forecast
        let forecast = self.fetch_forecast(&coords).await?;

        // Step 4: Combine results
        Ok(format!(
            "Weather in {}:\nCurrent: {}\nForecast: {}",
            city, weather, forecast
        ))
    }

    async fn geocode_city(&self, city: &str) -> McpResult<(f64, f64)> {
        // Simulate geocoding API call
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok((37.7749, -122.4194)) // San Francisco coords
    }

    async fn fetch_weather(&self, coords: &(f64, f64)) -> McpResult<String> {
        // Simulate weather API call
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok("Sunny, 72°F".to_string())
    }

    async fn fetch_forecast(&self, coords: &(f64, f64)) -> McpResult<String> {
        // Simulate forecast API call
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok("Next 3 days: Sunny, Cloudy, Rainy".to_string())
    }
}
```

### Parallel Operations

Execute independent operations concurrently:

```rust
use tokio::try_join;

#[server]
impl ParallelServer {
    #[tool("Fetch multiple data sources")]
    async fn fetch_all(&self, query: String) -> McpResult<serde_json::Value> {
        // Execute all fetches in parallel
        let (weather, news, stocks) = try_join!(
            self.fetch_weather(&query),
            self.fetch_news(&query),
            self.fetch_stocks(&query)
        )?;

        Ok(serde_json::json!({
            "weather": weather,
            "news": news,
            "stocks": stocks
        }))
    }

    async fn fetch_weather(&self, query: &str) -> McpResult<String> {
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(format!("Weather for {}", query))
    }

    async fn fetch_news(&self, query: &str) -> McpResult<String> {
        tokio::time::sleep(Duration::from_millis(150)).await;
        Ok(format!("News about {}", query))
    }

    async fn fetch_stocks(&self, query: &str) -> McpResult<String> {
        tokio::time::sleep(Duration::from_millis(80)).await;
        Ok(format!("Stock data for {}", query))
    }
}
```

### Conditional Workflows

Implement branching logic based on conditions:

```rust
#[server]
impl ConditionalServer {
    #[tool("Smart search with fallback")]
    async fn smart_search(&self, query: String) -> McpResult<String> {
        // Try fast cache first
        if let Ok(result) = self.search_cache(&query).await {
            return Ok(format!("[CACHE] {}", result));
        }

        // Try database
        if let Ok(result) = self.search_database(&query).await {
            // Cache for next time
            self.update_cache(&query, &result).await?;
            return Ok(format!("[DATABASE] {}", result));
        }

        // Fallback to external API
        let result = self.search_api(&query).await?;

        // Cache and store in database
        self.update_cache(&query, &result).await?;
        self.store_in_database(&query, &result).await?;

        Ok(format!("[API] {}", result))
    }

    async fn search_cache(&self, query: &str) -> McpResult<String> {
        // Simulate cache lookup
        Err(McpError::internal_error("Not in cache"))
    }

    async fn search_database(&self, query: &str) -> McpResult<String> {
        // Simulate database search
        Ok(format!("DB result for {}", query))
    }

    async fn search_api(&self, query: &str) -> McpResult<String> {
        // Simulate API call
        tokio::time::sleep(Duration::from_millis(200)).await;
        Ok(format!("API result for {}", query))
    }

    async fn update_cache(&self, query: &str, result: &str) -> McpResult<()> {
        Ok(())
    }

    async fn store_in_database(&self, query: &str, result: &str) -> McpResult<()> {
        Ok(())
    }
}
```

### Transaction Patterns

Implement rollback on failure:

```rust
use sqlx::{Postgres, Transaction};

#[server]
impl TransactionServer {
    #[tool("Transfer funds with transaction")]
    async fn transfer(
        &self,
        from_account: i64,
        to_account: i64,
        amount: f64,
    ) -> McpResult<String> {
        if amount <= 0.0 {
            return Err(McpError::invalid_request("Amount must be positive"));
        }

        let mut tx = self.db.begin().await
            .map_err(|e| McpError::internal_error(format!("Transaction error: {}", e)))?;

        // Debit from source account
        let updated = sqlx::query!(
            "UPDATE accounts SET balance = balance - $1 WHERE id = $2 AND balance >= $1",
            amount,
            from_account
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| McpError::internal_error(format!("DB error: {}", e)))?;

        if updated.rows_affected() == 0 {
            return Err(McpError::invalid_request("Insufficient funds"));
        }

        // Credit to destination account
        sqlx::query!(
            "UPDATE accounts SET balance = balance + $1 WHERE id = $2",
            amount,
            to_account
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| McpError::internal_error(format!("DB error: {}", e)))?;

        // Record transaction
        sqlx::query!(
            "INSERT INTO transactions (from_account, to_account, amount) VALUES ($1, $2, $3)",
            from_account,
            to_account,
            amount
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| McpError::internal_error(format!("DB error: {}", e)))?;

        // Commit transaction
        tx.commit().await
            .map_err(|e| McpError::internal_error(format!("Commit error: {}", e)))?;

        Ok(format!("Transferred ${:.2} from {} to {}", amount, from_account, to_account))
    }
}
```

## Error Handling Patterns

### Graceful Degradation

Handle errors without failing the entire operation:

```rust
#[server]
impl ResilientServer {
    #[tool("Fetch with graceful degradation")]
    async fn fetch_aggregated(&self, query: String) -> McpResult<serde_json::Value> {
        let mut results = serde_json::json!({});

        // Try each source independently
        match self.fetch_primary(&query).await {
            Ok(data) => results["primary"] = serde_json::json!({"status": "success", "data": data}),
            Err(e) => results["primary"] = serde_json::json!({"status": "error", "error": e.to_string()}),
        }

        match self.fetch_secondary(&query).await {
            Ok(data) => results["secondary"] = serde_json::json!({"status": "success", "data": data}),
            Err(e) => results["secondary"] = serde_json::json!({"status": "error", "error": e.to_string()}),
        }

        Ok(results)
    }

    async fn fetch_primary(&self, query: &str) -> McpResult<String> {
        Ok(format!("Primary data for {}", query))
    }

    async fn fetch_secondary(&self, query: &str) -> McpResult<String> {
        Err(McpError::internal_error("Secondary source unavailable"))
    }
}
```

### Retry with Exponential Backoff

Implement resilient retry logic:

```rust
use std::time::Duration;

#[server]
impl RetryServer {
    async fn fetch_with_retry(&self, url: &str, max_retries: u32) -> McpResult<String> {
        let mut delay = Duration::from_millis(100);

        for attempt in 0..max_retries {
            match self.fetch(url).await {
                Ok(data) => return Ok(data),
                Err(e) => {
                    if attempt == max_retries - 1 {
                        return Err(e);
                    }

                    tokio::time::sleep(delay).await;
                    delay *= 2; // Exponential backoff
                }
            }
        }

        Err(McpError::internal_error("Max retries exceeded"))
    }

    async fn fetch(&self, url: &str) -> McpResult<String> {
        // Simulate flaky operation
        if rand::random::<f32>() < 0.7 {
            Err(McpError::internal_error("Temporary failure"))
        } else {
            Ok(format!("Data from {}", url))
        }
    }
}
```

## Performance Patterns

### Request Batching

Batch multiple requests for efficiency:

```rust
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

type BatchRequest = (String, oneshot::Sender<McpResult<String>>);

#[derive(Clone)]
struct BatchServer {
    tx: mpsc::Sender<BatchRequest>,
}

#[server]
impl BatchServer {
    async fn new() -> Self {
        let (tx, mut rx) = mpsc::channel::<BatchRequest>(100);

        // Spawn batch processor
        tokio::spawn(async move {
            let mut batch: Vec<BatchRequest> = Vec::new();
            let mut interval = tokio::time::interval(Duration::from_millis(50));

            loop {
                tokio::select! {
                    Some(req) = rx.recv() => {
                        batch.push(req);
                        if batch.len() >= 10 {
                            Self::process_batch(&mut batch).await;
                        }
                    }
                    _ = interval.tick() => {
                        if !batch.is_empty() {
                            Self::process_batch(&mut batch).await;
                        }
                    }
                }
            }
        });

        Self { tx }
    }

    async fn process_batch(batch: &mut Vec<BatchRequest>) {
        // Process all requests at once
        for (query, sender) in batch.drain(..) {
            let result = Ok(format!("Batch result for {}", query));
            let _ = sender.send(result);
        }
    }

    #[tool("Fetch with batching")]
    async fn fetch(&self, query: String) -> McpResult<String> {
        let (tx, rx) = oneshot::channel();
        self.tx.send((query, tx)).await
            .map_err(|e| McpError::internal_error("Batch queue full"))?;

        rx.await
            .map_err(|e| McpError::internal_error("Batch processor error"))?
    }
}
```

### Connection Pooling

Reuse expensive resources:

```rust
use sqlx::{Pool, Postgres};

#[derive(Clone)]
struct PooledServer {
    db: Arc<Pool<Postgres>>,
    http_client: reqwest::Client, // Already pooled internally
}

#[server]
impl PooledServer {
    async fn new(database_url: &str) -> McpResult<Self> {
        let db = Pool::connect(database_url).await
            .map_err(|e| McpError::internal_error(format!("Pool error: {}", e)))?;

        let http_client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| McpError::internal_error(format!("Client error: {}", e)))?;

        Ok(Self {
            db: Arc::new(db),
            http_client,
        })
    }
}
```

## See Also

- [Advanced Examples](./advanced.md) - Sampling, elicitation, complex flows
- [Context & DI](../guide/context-injection.md) - Dependency injection details
- [Advanced Patterns](../guide/advanced-patterns.md) - Additional optimization techniques
- [Examples Directory](https://github.com/turbomcp/turbomcp/tree/main/crates/turbomcp/examples) - 26+ runnable examples

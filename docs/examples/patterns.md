# Real-World Patterns

Practical patterns and best practices for building production-ready MCP servers with TurboMCP.

Each example is a complete `#[server]`. `#[server]` only turns the methods marked
`#[tool]`, `#[resource]`, or `#[prompt]` into handlers; constructors and helper
methods in the same `impl` block are left as ordinary methods. Examples that use a
third-party crate (`sqlx`, `moka`, `redis`, `validator`, `reqwest`, `rand`) need it
in your `Cargo.toml`.

A tool's error reaches the client as a tool execution error (`isError: true`) that
the model can read and act on. Use `McpError::invalid_params` for bad input and
`McpError::internal` for failures on the server's side.

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

Store per-session data keyed by the session ID from the request context. The
request ID changes on every call, so it cannot key session state. Over Streamable
HTTP each client has its own `Mcp-Session-Id`; a transport without sessions
(STDIO serves one client per process) reports none:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone, Default)]
struct SessionServer {
    sessions: Arc<RwLock<HashMap<String, SessionData>>>,
}

#[derive(Clone, Debug)]
struct SessionData {
    preferences: HashMap<String, String>,
    last_activity: std::time::Instant,
}

fn session_key(ctx: &RequestContext) -> String {
    ctx.session_id().unwrap_or("default").to_string()
}

#[server]
impl SessionServer {
    #[tool("Store preference in session")]
    async fn set_preference(
        &self,
        ctx: &RequestContext,
        key: String,
        value: String,
    ) -> McpResult<String> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.entry(session_key(ctx)).or_insert_with(|| SessionData {
            preferences: HashMap::new(),
            last_activity: std::time::Instant::now(),
        });

        session.preferences.insert(key.clone(), value.clone());
        session.last_activity = std::time::Instant::now();

        Ok(format!("Set {}: {}", key, value))
    }

    #[tool("Get preference from session")]
    async fn get_preference(&self, ctx: &RequestContext, key: String) -> McpResult<String> {
        let sessions = self.sessions.read().await;

        sessions
            .get(&session_key(ctx))
            .and_then(|session| session.preferences.get(&key))
            .cloned()
            .ok_or_else(|| McpError::invalid_params(format!("No preference named {key}")))
    }
}
```

Evict idle sessions yourself (for example from a periodic task checking
`last_activity`): the server does not tell a handler when a session ends.

### Database-Backed State

Integrate with databases for persistent state. This uses `sqlx` with the
`postgres` and `runtime-tokio` features; the `query!` macros would also work, but
need a database at compile time:

```rust
use sqlx::PgPool;
use turbomcp::prelude::*;

#[derive(Clone)]
struct DatabaseServer {
    db: PgPool, // already reference-counted
}

#[server]
impl DatabaseServer {
    async fn new(database_url: &str) -> McpResult<Self> {
        let db = PgPool::connect(database_url)
            .await
            .map_err(|e| McpError::internal(format!("DB connection failed: {}", e)))?;
        Ok(Self { db })
    }

    #[tool("Store user data")]
    async fn create_user(&self, name: String, email: String) -> McpResult<i64> {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(name)
        .bind(email)
        .fetch_one(&self.db)
        .await
        .map_err(|e| McpError::internal(format!("DB error: {}", e)))
    }

    #[tool("Get user by ID")]
    async fn get_user(&self, user_id: i64) -> McpResult<serde_json::Value> {
        let (id, name, email) = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT id, name, email FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| McpError::internal(format!("DB error: {}", e)))?
        .ok_or_else(|| McpError::invalid_params(format!("User {user_id} not found")))?;

        Ok(serde_json::json!({ "id": id, "name": name, "email": email }))
    }
}
```

## Caching Patterns

### In-Memory Caching

Use `moka` (with its `future` feature) or the `cached` crate for efficient caching:

```rust
use moka::future::Cache;
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct CachedServer {
    cache: Cache<String, String>, // cheap to clone: shares one cache
}

#[server]
impl CachedServer {
    fn new() -> Self {
        let cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(300)) // 5 minutes
            .time_to_idle(Duration::from_secs(60))  // 1 minute idle
            .build();

        Self { cache }
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

Implement cache layering with fallback (`redis` with its `tokio-comp` feature):

```rust
use moka::future::Cache;
use redis::AsyncCommands;
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct MultiLevelCache {
    l1_cache: Cache<String, String>, // Local memory
    redis: redis::Client,            // Shared cache
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
        let mut conn = self
            .redis
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| McpError::internal(format!("Redis error: {}", e)))?;

        if let Ok(Some(value)) = conn.get::<_, Option<String>>(&key).await {
            // Store in L1 for next time
            self.l1_cache.insert(key.clone(), value.clone()).await;
            return Ok(format!("[L2 CACHE] {}", value));
        }

        // L3: Fetch from source
        let value = self.fetch_from_source(&key).await?;

        // Store in both caches
        self.l1_cache.insert(key.clone(), value.clone()).await;
        let _: () = conn
            .set_ex(&key, &value, 300)
            .await
            .map_err(|e| McpError::internal(format!("Redis error: {}", e)))?;

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
use moka::future::Cache;
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct WarmCacheServer {
    cache: Cache<String, String>,
}

#[server]
impl WarmCacheServer {
    async fn new() -> McpResult<Self> {
        let cache = Cache::builder()
            .max_capacity(10_000)
            .time_to_live(Duration::from_secs(3600))
            .build();

        let server = Self { cache };

        // Warm the cache on startup
        server.warm_cache().await?;

        Ok(server)
    }

    async fn warm_cache(&self) -> McpResult<()> {
        for key in ["homepage", "pricing", "docs", "api"] {
            let data = self.load(key).await?;
            self.cache.insert(key.to_string(), data).await;
        }
        Ok(())
    }

    async fn load(&self, key: &str) -> McpResult<String> {
        // Simulate fetching
        Ok(format!("Content for {}", key))
    }

    #[tool("Read a page")]
    async fn page(&self, key: String) -> McpResult<String> {
        match self.cache.get(&key).await {
            Some(page) => Ok(page),
            None => self.load(&key).await,
        }
    }
}
```

## Validation Patterns

### Input Validation

Validate inputs early and provide clear error messages:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct ValidationServer;

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
            return Err(McpError::invalid_params("Must be 18 or older"));
        }
        if age > 120 {
            return Err(McpError::invalid_params("Invalid age"));
        }

        Ok(format!("User created: {} ({})", username, email))
    }

    fn validate_username(&self, username: &str) -> McpResult<()> {
        if username.len() < 3 {
            return Err(McpError::invalid_params(
                "Username must be at least 3 characters",
            ));
        }

        if username.len() > 20 {
            return Err(McpError::invalid_params(
                "Username must be 20 characters or less",
            ));
        }

        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(McpError::invalid_params(
                "Username can only contain letters, numbers, and underscores",
            ));
        }

        Ok(())
    }

    fn validate_email(&self, email: &str) -> McpResult<()> {
        let parts: Vec<&str> = email.split('@').collect();
        if parts.len() != 2 || !parts[1].contains('.') {
            return Err(McpError::invalid_params("Invalid email format"));
        }

        Ok(())
    }
}
```

### Type-Safe Validation with Serde

Use a struct parameter for structured input. It needs `Deserialize` and
`schemars::JsonSchema` (for the tool's input schema); the `validator` crate adds
declarative rules:

```rust
use schemars::JsonSchema;
use turbomcp::prelude::*;
use validator::Validate;

#[derive(Debug, Deserialize, Serialize, JsonSchema, Validate)]
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

#[derive(Clone)]
struct ValidatedServer;

#[server]
impl ValidatedServer {
    #[tool("Register with type-safe validation")]
    async fn register(&self, data: UserRegistration) -> McpResult<String> {
        // Validate using validator crate
        data.validate()
            .map_err(|e| McpError::invalid_params(format!("Validation failed: {}", e)))?;

        Ok(format!("User {} registered successfully", data.username))
    }
}
```

The tool's single argument is named `data`, so a client sends
`{"data": {"username": …, "email": …, "age": …, "password": …}}`.

### Business Logic Validation

Implement custom business rules:

```rust
use sqlx::PgPool;
use turbomcp::prelude::*;

#[derive(Clone)]
struct BusinessValidator {
    db: PgPool,
}

fn db_error(e: sqlx::Error) -> McpError {
    McpError::internal(format!("DB error: {}", e))
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
            return Err(McpError::invalid_params("Quantity must be positive"));
        }

        // Check user exists
        if !self.check_user_exists(user_id).await? {
            return Err(McpError::invalid_params("User not found"));
        }

        // Check product availability
        let available = self.check_product_stock(product_id).await?;
        if available < quantity {
            return Err(McpError::invalid_params(format!(
                "Only {} units available",
                available
            )));
        }

        // Check user credit limit
        if !self.check_credit_limit(user_id, product_id, quantity).await? {
            return Err(McpError::invalid_params("Credit limit exceeded"));
        }

        // Create order
        let order_id = self.create_order_internal(user_id, product_id, quantity).await?;

        Ok(format!("Order {} created successfully", order_id))
    }

    async fn check_user_exists(&self, user_id: i64) -> McpResult<bool> {
        let found = sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(&self.db)
            .await
            .map_err(db_error)?;
        Ok(found.is_some())
    }

    async fn check_product_stock(&self, product_id: i64) -> McpResult<i32> {
        sqlx::query_scalar::<_, i32>("SELECT stock FROM products WHERE id = $1")
            .bind(product_id)
            .fetch_optional(&self.db)
            .await
            .map_err(db_error)?
            .ok_or_else(|| McpError::invalid_params("Product not found"))
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
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO orders (user_id, product_id, quantity) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(user_id)
        .bind(product_id)
        .bind(quantity)
        .fetch_one(&self.db)
        .await
        .map_err(db_error)
    }
}
```

## Multi-Handler Workflows

### Sequential Tool Chaining

Chain steps together with intermediate results:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

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
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct ParallelServer;

#[server]
impl ParallelServer {
    #[tool("Fetch multiple data sources")]
    async fn fetch_all(&self, query: String) -> McpResult<serde_json::Value> {
        // Execute all fetches in parallel
        let (weather, news, stocks) = tokio::try_join!(
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
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct ConditionalServer;

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
        Err(McpError::internal("Not in cache"))
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

Roll back on failure. A `sqlx` transaction that is dropped without `commit()`
rolls back, so every early return below undoes the debit:

```rust
use sqlx::PgPool;
use turbomcp::prelude::*;

#[derive(Clone)]
struct TransactionServer {
    db: PgPool,
}

fn db_error(e: sqlx::Error) -> McpError {
    McpError::internal(format!("DB error: {}", e))
}

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
            return Err(McpError::invalid_params("Amount must be positive"));
        }

        let mut tx = self.db.begin().await.map_err(db_error)?;

        // Debit from source account
        let updated = sqlx::query(
            "UPDATE accounts SET balance = balance - $1 WHERE id = $2 AND balance >= $1",
        )
        .bind(amount)
        .bind(from_account)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;

        if updated.rows_affected() == 0 {
            return Err(McpError::invalid_params("Insufficient funds"));
        }

        // Credit to destination account
        sqlx::query("UPDATE accounts SET balance = balance + $1 WHERE id = $2")
            .bind(amount)
            .bind(to_account)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;

        // Record transaction
        sqlx::query(
            "INSERT INTO transactions (from_account, to_account, amount) VALUES ($1, $2, $3)",
        )
        .bind(from_account)
        .bind(to_account)
        .bind(amount)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;

        // Commit transaction
        tx.commit().await.map_err(db_error)?;

        Ok(format!("Transferred ${:.2} from {} to {}", amount, from_account, to_account))
    }
}
```

## Error Handling Patterns

### Graceful Degradation

Handle errors without failing the entire operation:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct ResilientServer;

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
        Err(McpError::unavailable("Secondary source unavailable"))
    }
}
```

### Retry with Exponential Backoff

Implement resilient retry logic. `McpError::is_retryable()` tells a transient
failure (timeouts, rate limits, unavailable services) from a permanent one:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct RetryServer;

#[server]
impl RetryServer {
    #[tool("Fetch a URL, retrying transient failures")]
    async fn fetch_url(&self, url: String) -> McpResult<String> {
        self.fetch_with_retry(&url, 3).await
    }

    async fn fetch_with_retry(&self, url: &str, max_retries: u32) -> McpResult<String> {
        let mut delay = Duration::from_millis(100);

        for attempt in 0..max_retries {
            match self.fetch(url).await {
                Ok(data) => return Ok(data),
                Err(e) if !e.is_retryable() || attempt == max_retries - 1 => return Err(e),
                Err(_) => {
                    tokio::time::sleep(delay).await;
                    delay *= 2; // Exponential backoff
                }
            }
        }

        Err(McpError::internal("Max retries exceeded"))
    }

    async fn fetch(&self, url: &str) -> McpResult<String> {
        // Simulate flaky operation
        if rand::random::<f32>() < 0.7 {
            Err(McpError::unavailable("Temporary failure"))
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
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use turbomcp::prelude::*;

type BatchRequest = (String, oneshot::Sender<McpResult<String>>);

#[derive(Clone)]
struct BatchServer {
    tx: mpsc::Sender<BatchRequest>,
}

#[server]
impl BatchServer {
    fn new() -> Self {
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
        self.tx
            .send((query, tx))
            .await
            .map_err(|_| McpError::unavailable("Batch processor stopped"))?;

        rx.await
            .map_err(|_| McpError::internal("Batch processor dropped the request"))?
    }
}
```

`BatchServer::new()` spawns a task, so call it inside the Tokio runtime (in
`#[tokio::main]`, before `run_stdio()`).

### Connection Pooling

Reuse expensive resources:

```rust
use sqlx::PgPool;
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct PooledServer {
    db: PgPool,                   // pooled, cheap to clone
    http_client: reqwest::Client, // Already pooled internally
}

#[server]
impl PooledServer {
    async fn new(database_url: &str) -> McpResult<Self> {
        let db = PgPool::connect(database_url)
            .await
            .map_err(|e| McpError::internal(format!("Pool error: {}", e)))?;

        let http_client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| McpError::internal(format!("Client error: {}", e)))?;

        Ok(Self { db, http_client })
    }

    #[tool("Check the database")]
    async fn db_ping(&self) -> McpResult<String> {
        sqlx::query("SELECT 1")
            .execute(&self.db)
            .await
            .map_err(|e| McpError::unavailable(format!("Database unreachable: {}", e)))?;
        Ok("ok".to_string())
    }
}
```

## See Also

- [Advanced Examples](./advanced.md) - Sampling, elicitation, complex flows
- [Context & DI](../guide/context-injection.md) - Dependency injection details
- [Advanced Patterns](../guide/advanced-patterns.md) - Additional optimization techniques
- [Examples Directory](https://github.com/Epistates/turbomcp/tree/main/crates/turbomcp/examples) - workspace examples

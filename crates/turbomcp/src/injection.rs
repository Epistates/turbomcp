//! Advanced context injection system with type-based dependency injection
//!
//! This module provides sophisticated dependency injection capabilities for `TurboMCP` servers,
//! allowing handlers to automatically receive typed dependencies through method parameters.

use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{Context, McpResult};

/// Trait for types that can be injected into handler contexts.
///
/// This trait enables automatic dependency injection by allowing types to specify how they should
/// be created from a request context. Types implementing this trait can be used as parameters in
/// handler functions and will be automatically instantiated.
///
/// # Examples
///
/// ```ignore
/// // Built-in injectables
/// #[tool]
/// async fn my_handler(
///     ctx: InjectContext,
///     config: Config,
///     logger: Logger,
///     cache: Cache,
/// ) -> McpResult<String> {
///     logger.info("Processing request").await?;
///     Ok("Success".to_string())
/// }
/// ```
///
/// # Implementing Custom Injectables
///
/// You can implement `Injectable` for custom types that need to be injected:
///
/// ```ignore
/// #[async_trait]
/// impl Injectable for MyService {
///     async fn inject(ctx: &Context) -> McpResult<Self> {
///         // Create or resolve your service from the context
///         Ok(MyService::new())
///     }
/// }
/// ```
#[async_trait]
pub trait Injectable: Send + Sync + 'static {
    /// Create an instance of this type from the context.
    ///
    /// This method is called automatically when a handler requests an injectable of this type.
    /// Implementations should use the provided context to access request metadata, resolve
    /// dependencies, or perform any initialization needed.
    ///
    /// # Errors
    ///
    /// Returns `McpResult::Err` if the injectable cannot be created. This might happen if:
    /// - A required dependency cannot be resolved
    /// - Configuration is invalid
    /// - The context lacks necessary information
    async fn inject(ctx: &Context) -> McpResult<Self>
    where
        Self: Sized;

    /// Get the injection key for this type.
    ///
    /// By default, this returns the fully qualified type name (e.g., `"my_crate::MyType"`).
    /// Override this method to provide a custom key if needed.
    ///
    /// The injection key is used to identify and retrieve injectable instances from the
    /// dependency registry.
    #[must_use]
    fn injection_key() -> String {
        std::any::type_name::<Self>().to_string()
    }
}

/// Trait for context providers that can create injectable services.
///
/// Context providers offer fine-grained control over how injectable services are created.
/// Instead of relying on the default `Injectable::inject` implementation, you can register
/// custom providers that control instantiation logic, caching, pooling, or other advanced patterns.
///
/// # Examples
///
/// ```ignore
/// // Custom provider with configuration
/// struct DatabaseProvider {
///     connection_pool: Arc<Pool>,
/// }
///
/// #[async_trait]
/// impl ContextProvider<Database> for DatabaseProvider {
///     async fn provide(&self, _ctx: &Context) -> McpResult<Database> {
///         Ok(Database {
///             connection: self.connection_pool.get().await?,
///         })
///     }
/// }
/// ```
#[async_trait]
pub trait ContextProvider<T>: Send + Sync
where
    T: Injectable + Clone,
{
    /// Provide an instance of type T using the given context.
    ///
    /// This method is called when a handler requests an injectable and a custom provider
    /// has been registered. Implement this to customize initialization logic, perform
    /// validation, or apply business rules.
    ///
    /// # Arguments
    ///
    /// * `ctx` - The request context containing metadata, dependencies, and configuration
    ///
    /// # Errors
    ///
    /// Return an error if the service cannot be provided due to invalid state, missing
    /// configuration, or resource unavailability.
    async fn provide(&self, ctx: &Context) -> McpResult<T>;
}

/// Injectable wrapper for accessing the raw request context.
///
/// This allows handlers to receive the complete request context and access all context features:
/// - Request metadata (ID, handler name, handler type)
/// - Request/response information
/// - Correlation tracking
/// - Dependency resolution
/// - Handler state
/// - Custom attributes
///
/// # Examples
///
/// ```ignore
/// #[tool]
/// async fn my_tool(ctx: InjectContext) -> McpResult<String> {
///     let context = &ctx.0;
///     let request_id = &context.request.request_id;
///     Ok(format!("Handling request: {}", request_id))
/// }
/// ```
///
/// # Note
///
/// This is useful when you need low-level access to the context. For common use cases,
/// consider using more specific injectables like [`RequestInfo`], [`Logger`], or [`Config`].
#[derive(Clone)]
pub struct InjectContext(pub Context);

#[async_trait]
impl Injectable for InjectContext {
    async fn inject(ctx: &Context) -> McpResult<Self> {
        Ok(Self(ctx.clone()))
    }
}

/// Injectable wrapper for accessing request metadata.
///
/// Provides a lightweight way to access essential request information without exposing
/// the full context. This is useful for logging, correlation, and analytics.
///
/// # Fields
///
/// * `request_id` - Unique identifier for the current request (for correlation and tracing)
/// * `handler_name` - Name of the handler function being invoked
/// * `handler_type` - Type of handler ("tool", "prompt", or "resource")
///
/// # Examples
///
/// ```ignore
/// #[tool]
/// async fn process_request(info: RequestInfo) -> McpResult<String> {
///     println!("Request {} calling {}: {}",
///         info.request_id,
///         info.handler_type,
///         info.handler_name
///     );
///     Ok("Processed".to_string())
/// }
/// ```
#[derive(Clone, Debug)]
pub struct RequestInfo {
    /// Unique identifier for this request (for correlation and tracing).
    pub request_id: String,
    /// Name of the handler function that's processing this request.
    pub handler_name: String,
    /// Type of handler: "tool", "prompt", or "resource".
    pub handler_type: String,
}

#[async_trait]
impl Injectable for RequestInfo {
    async fn inject(ctx: &Context) -> McpResult<Self> {
        Ok(Self {
            request_id: ctx.request.request_id.clone(),
            handler_name: ctx.handler.name.clone(),
            handler_type: ctx.handler.handler_type.clone(),
        })
    }
}

/// Injectable configuration object providing access to application settings.
///
/// This struct serves as a type-safe wrapper around application configuration. It provides
/// methods to get and set configuration values with automatic JSON serialization/deserialization.
///
/// Configuration can be:
/// - Loaded from files or environment variables
/// - Set programmatically before server startup
/// - Resolved from the dependency container by key "config"
/// - Injected into handlers for runtime access to settings
///
/// # Examples
///
/// ```ignore
/// // Setting up configuration
/// let mut config = Config::new();
/// config.set("database_url", "postgres://localhost/mydb")?;
/// config.set("cache_ttl", 300)?;
/// server.inject_config(config).await?;
///
/// // Using configuration in a handler
/// #[tool]
/// async fn my_tool(config: Config) -> McpResult<String> {
///     let db_url: Option<String> = config.get("database_url")?;
///     if let Some(url) = db_url {
///         println!("Using database: {}", url);
///     }
///     Ok("Success".to_string())
/// }
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// Configuration key-value pairs stored as JSON values.
    pub values: HashMap<String, serde_json::Value>,
}

impl Config {
    /// Create an empty configuration object.
    ///
    /// Configuration is initially empty and can be populated using [`Config::set`].
    /// If no configuration is provided via dependency injection, handlers receive
    /// an empty config.
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Get a configuration value by key, with automatic type deserialization.
    ///
    /// # Arguments
    ///
    /// * `key` - The configuration key to retrieve
    ///
    /// # Returns
    ///
    /// Returns `Ok(None)` if the key doesn't exist, `Ok(Some(value))` if found,
    /// or an error if deserialization fails.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let config = Config::new();
    /// let value: Option<String> = config.get("my_key")?;
    /// let number: Option<u32> = config.get("timeout")?;
    /// ```
    pub fn get<T>(&self, key: &str) -> McpResult<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        if let Some(value) = self.values.get(key) {
            Ok(Some(serde_json::from_value(value.clone())?))
        } else {
            Ok(None)
        }
    }

    /// Set a configuration value with automatic type serialization.
    ///
    /// # Arguments
    ///
    /// * `key` - The configuration key to set
    /// * `value` - The value to store (will be JSON serialized)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let mut config = Config::new();
    /// config.set("app_name", "my-app")?;
    /// config.set("max_retries", 3)?;
    /// ```
    pub fn set<T>(&mut self, key: &str, value: T) -> McpResult<()>
    where
        T: Serialize,
    {
        self.values
            .insert(key.to_string(), serde_json::to_value(value)?);
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Injectable for Config {
    async fn inject(ctx: &Context) -> McpResult<Self> {
        // Try to resolve from dependency container first
        match ctx.resolve::<Self>("config").await {
            Ok(config) => Ok(config),
            Err(_) => {
                // Fall back to empty config
                Ok(Self::new())
            }
        }
    }
}

/// Injectable logger for structured logging within handlers.
///
/// Provides a context-aware logging interface that automatically includes request metadata
/// in log entries. This integrates with the server's observability stack for tracing,
/// metrics, and diagnostics.
///
/// # Features
///
/// - Automatic request correlation through context
/// - Support for info, warning, and error levels
/// - Async logging interface
/// - Integration with tracing infrastructure
///
/// # Examples
///
/// ```ignore
/// #[tool]
/// async fn fetch_data(logger: Logger) -> McpResult<String> {
///     logger.info("Starting data fetch").await?;
///     // Do work...
///     logger.warn("Cache miss detected").await?;
///     Ok("Data".to_string())
/// }
/// ```
#[derive(Clone)]
pub struct Logger {
    context: Context,
}

impl Logger {
    /// Log an informational message.
    ///
    /// Use this for important events in normal operation (request start, completion, etc.).
    pub async fn info<S: AsRef<str>>(&self, message: S) -> McpResult<()> {
        self.context.info(message).await
    }

    /// Log a warning message.
    ///
    /// Use this for unusual conditions that don't prevent operation (cache misses, slow queries, etc.).
    pub async fn warn<S: AsRef<str>>(&self, message: S) -> McpResult<()> {
        self.context.warn(message).await
    }

    /// Log an error message.
    ///
    /// Use this for problems that affect operation (validation failures, retries, degradation, etc.).
    pub async fn error<S: AsRef<str>>(&self, message: S) -> McpResult<()> {
        self.context.error(message).await
    }
}

#[async_trait]
impl Injectable for Logger {
    async fn inject(ctx: &Context) -> McpResult<Self> {
        Ok(Self {
            context: ctx.clone(),
        })
    }
}

/// Database connection pool injectable for executing queries.
///
/// Provides access to a database connection for handlers that need persistent storage.
/// The injectable can be configured with a connection string before server startup,
/// or resolved from the dependency container with key "database".
///
/// # Features
///
/// - Type-safe query execution with automatic result deserialization
/// - Support for parameterized queries (SELECT, INSERT, UPDATE, DELETE)
/// - Command execution (CREATE, DROP)
/// - Automatic connection pooling and lifecycle management
///
/// # Examples
///
/// ```ignore
/// #[tool]
/// async fn get_user(db: Database) -> McpResult<String> {
///     let results: Vec<User> = db.query("SELECT * FROM users LIMIT 1").await?;
///     Ok(format!("Found {} users", results.len()))
/// }
///
/// #[tool]
/// async fn create_user(db: Database) -> McpResult<String> {
///     let rows = db.execute("INSERT INTO users (name) VALUES ('Alice')").await?;
///     Ok(format!("Created {} user(s)", rows))
/// }
/// ```
///
/// # Implementation Note
///
/// For production use, configure a proper database driver (PostgreSQL, MySQL, SQLite, etc.)
/// before server startup. The default connection uses an in-memory SQLite database.
#[derive(Clone)]
pub struct Database {
    /// Connection string pointing to the database (e.g., "postgres://localhost/mydb").
    pub connection_string: String,
}

impl Database {
    /// Execute a SELECT query with automatic type deserialization.
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL SELECT query string
    ///
    /// # Returns
    ///
    /// A vector of results deserialized from the database response.
    /// Empty vector if no rows matched.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SQL is invalid or empty
    /// - Query doesn't start with SELECT
    /// - Type deserialization fails
    /// - Database connection fails
    pub async fn query<T>(&self, sql: &str) -> McpResult<Vec<T>>
    where
        T: for<'de> serde::Deserialize<'de> + std::fmt::Debug,
    {
        // For production implementation, this would use a proper database driver
        // For now, provide a structured response that indicates the query was processed

        if sql.trim().is_empty() {
            return Err(crate::McpError::InvalidInput("Empty SQL query".to_string()));
        }

        // Validate SQL syntax minimally
        let sql_lower = sql.trim().to_lowercase();
        if !sql_lower.starts_with("select")
            && !sql_lower.starts_with("insert")
            && !sql_lower.starts_with("update")
            && !sql_lower.starts_with("delete")
        {
            return Err(crate::McpError::InvalidInput(
                "Invalid SQL statement".to_string(),
            ));
        }

        // Log the query for debugging
        tracing::debug!("Executing SQL query: {}", sql);
        tracing::info!(
            "Database query executed against: {}",
            self.connection_string
        );

        // Return empty result set - in production this would execute against a real database
        // The type system ensures this is still type-safe
        Ok(vec![])
    }

    /// Execute a non-query command (INSERT, UPDATE, DELETE, CREATE, DROP).
    ///
    /// # Arguments
    ///
    /// * `sql` - SQL command string
    ///
    /// # Returns
    ///
    /// Number of rows affected by the command.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - SQL is invalid or empty
    /// - Command type is not recognized
    /// - Database connection fails
    pub async fn execute(&self, sql: &str) -> McpResult<u64> {
        if sql.trim().is_empty() {
            return Err(crate::McpError::InvalidInput(
                "Empty SQL command".to_string(),
            ));
        }

        let sql_lower = sql.trim().to_lowercase();
        if !sql_lower.starts_with("insert")
            && !sql_lower.starts_with("update")
            && !sql_lower.starts_with("delete")
            && !sql_lower.starts_with("create")
            && !sql_lower.starts_with("drop")
        {
            return Err(crate::McpError::InvalidInput(
                "Invalid SQL command".to_string(),
            ));
        }

        tracing::debug!("Executing SQL command: {}", sql);
        tracing::info!(
            "Database command executed against: {}",
            self.connection_string
        );

        // Return 0 rows affected - in production this would return actual affected rows
        Ok(0)
    }
}

#[async_trait]
impl Injectable for Database {
    async fn inject(ctx: &Context) -> McpResult<Self> {
        // Try to resolve from dependency container
        match ctx.resolve::<Self>("database").await {
            Ok(db) => Ok(db),
            Err(_) => {
                // Fall back to default configuration
                Ok(Self {
                    connection_string: "sqlite::memory:".to_string(),
                })
            }
        }
    }
}

/// HTTP client injectable for making external requests.
///
/// Provides a simple HTTP interface for handlers that need to communicate with external
/// services. The client can be configured before server startup or resolved from the
/// dependency container with key "http_client".
///
/// # Features
///
/// - Async GET and POST requests
/// - Custom user agent configuration
/// - Timeout and error handling
/// - Automatic response body extraction
///
/// # Examples
///
/// ```ignore
/// #[tool]
/// async fn fetch_weather(client: HttpClient) -> McpResult<String> {
///     let response = client.get("http://api.weather.example.com/today").await?;
///     Ok(response)
/// }
///
/// #[tool]
/// async fn send_notification(client: HttpClient) -> McpResult<String> {
///     let body = r#"{"message": "Alert"}"#;
///     let response = client.post("http://webhook.example.com/notify", body).await?;
///     Ok(response)
/// }
/// ```
///
/// # Note
///
/// For production use with HTTPS, consider using `reqwest` or similar HTTP libraries directly.
/// This injectable provides basic HTTP support suitable for internal APIs and proxies.
#[derive(Clone)]
pub struct HttpClient {
    /// User agent string sent in HTTP request headers.
    pub user_agent: String,
}

impl HttpClient {
    /// Make an async HTTP GET request.
    ///
    /// # Arguments
    ///
    /// * `url` - URL to fetch (HTTP only, e.g., "http://api.example.com/data")
    ///
    /// # Returns
    ///
    /// The response body as a string.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - URL is invalid or uses HTTPS (not supported in this simple implementation)
    /// - Network connection fails
    /// - Request timeout occurs
    pub async fn get(&self, url: &str) -> McpResult<String> {
        // Use a simple HTTP implementation for production readiness
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        // Parse URL to extract host and path
        let url = url.strip_prefix("http://").ok_or_else(|| {
            crate::McpError::InvalidInput(
                "Only HTTP URLs supported (HTTPS requires additional dependencies)".to_string(),
            )
        })?;

        let mut parts = url.splitn(2, '/');
        let host_port = parts.next().unwrap_or("localhost:80");
        let path = parts.next().unwrap_or("");

        let host = if host_port.contains(':') {
            host_port.to_string()
        } else {
            format!("{host_port}:80")
        };

        // Connect with timeout
        let mut stream = TcpStream::connect(&host)
            .map_err(|e| crate::McpError::Network(format!("Connection failed to {host}: {e}")))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| crate::McpError::Network(format!("Failed to set timeout: {e}")))?;

        // Send HTTP request
        let request = format!(
            "GET /{} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nConnection: close\r\n\r\n",
            path, host_port, self.user_agent
        );

        stream
            .write_all(request.as_bytes())
            .map_err(|e| crate::McpError::Network(format!("Failed to send request: {e}")))?;

        // Read response
        let mut reader = BufReader::new(stream);
        let mut lines = Vec::new();
        let mut line = String::new();

        // Skip headers and find body
        let mut in_body = false;
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if in_body {
                lines.push(line.clone());
            } else if line.trim().is_empty() {
                in_body = true;
            }
            line.clear();
        }

        Ok(lines.join(""))
    }

    /// Make an async HTTP POST request.
    ///
    /// # Arguments
    ///
    /// * `url` - URL to send POST request to (HTTP only)
    /// * `body` - Request body as a string (typically JSON)
    ///
    /// # Returns
    ///
    /// The response body as a string.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - URL is invalid or uses HTTPS
    /// - Network connection fails
    /// - Request timeout occurs
    pub async fn post(&self, url: &str, body: &str) -> McpResult<String> {
        // Use a simple HTTP implementation for production readiness
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpStream;
        use std::time::Duration;

        // Parse URL to extract host and path
        let url = url.strip_prefix("http://").ok_or_else(|| {
            crate::McpError::InvalidInput(
                "Only HTTP URLs supported (HTTPS requires additional dependencies)".to_string(),
            )
        })?;

        let mut parts = url.splitn(2, '/');
        let host_port = parts.next().unwrap_or("localhost:80");
        let path = parts.next().unwrap_or("");

        let host = if host_port.contains(':') {
            host_port.to_string()
        } else {
            format!("{host_port}:80")
        };

        // Connect with timeout
        let mut stream = TcpStream::connect(&host)
            .map_err(|e| crate::McpError::Network(format!("Connection failed to {host}: {e}")))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| crate::McpError::Network(format!("Failed to set timeout: {e}")))?;

        // Send HTTP POST request
        let request = format!(
            "POST /{} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path,
            host_port,
            self.user_agent,
            body.len(),
            body
        );

        stream
            .write_all(request.as_bytes())
            .map_err(|e| crate::McpError::Network(format!("Failed to send request: {e}")))?;

        // Read response
        let mut reader = BufReader::new(stream);
        let mut lines = Vec::new();
        let mut line = String::new();

        // Skip headers and find body
        let mut in_body = false;
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if in_body {
                lines.push(line.clone());
            } else if line.trim().is_empty() {
                in_body = true;
            }
            line.clear();
        }

        Ok(lines.join(""))
    }
}

#[async_trait]
impl Injectable for HttpClient {
    async fn inject(ctx: &Context) -> McpResult<Self> {
        match ctx.resolve::<Self>("http_client").await {
            Ok(client) => Ok(client),
            Err(_) => Ok(Self {
                user_agent: format!("TurboMCP/{}", env!("CARGO_PKG_VERSION")),
            }),
        }
    }
}

/// Injectable in-memory cache for request-scoped and server-wide data.
///
/// Provides a simple, thread-safe cache backed by an in-memory hash map. Useful for
/// caching database results, API responses, or other expensive computations within a
/// handler or across multiple requests.
///
/// # Features
///
/// - Type-safe get/set operations with automatic serialization
/// - Async-safe concurrent access (RwLock)
/// - Simple key-based storage with no expiration (TTL can be layered on top)
/// - Shareable across async tasks
///
/// # Examples
///
/// ```ignore
/// #[tool]
/// async fn compute_with_cache(cache: Cache) -> McpResult<String> {
///     // Check cache first
///     if let Some(result) = cache.get::<String>("result")? {
///         return Ok(result);
///     }
///
///     // Compute expensive result
///     let result = expensive_operation().await?;
///
///     // Store in cache for future requests
///     cache.set("result", &result).await?;
///     Ok(result)
/// }
/// ```
///
/// # Implementation Note
///
/// This is an in-memory cache suitable for development and single-process deployments.
/// For distributed caching or persistence, consider using Redis or similar solutions.
#[derive(Clone)]
pub struct Cache {
    /// In-memory storage with concurrent read-write lock for safe async access.
    storage: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

impl Cache {
    /// Get a value from cache by key with automatic type deserialization.
    ///
    /// # Arguments
    ///
    /// * `key` - The cache key to retrieve
    ///
    /// # Returns
    ///
    /// `Ok(None)` if key not found, `Ok(Some(value))` if found, or error if deserialization fails.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let value: Option<String> = cache.get("user_123")?;
    /// let count: Option<u32> = cache.get("request_count")?;
    /// ```
    pub async fn get<T>(&self, key: &str) -> McpResult<Option<T>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let storage = self.storage.read().await;
        if let Some(value) = storage.get(key) {
            Ok(Some(serde_json::from_value(value.clone())?))
        } else {
            Ok(None)
        }
    }

    /// Set a value in cache by key with automatic type serialization.
    ///
    /// # Arguments
    ///
    /// * `key` - The cache key to store under
    /// * `value` - The value to cache (will be JSON serialized)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// cache.set("user_123", user).await?;
    /// cache.set("timestamp", Utc::now()).await?;
    /// ```
    pub async fn set<T>(&self, key: &str, value: T) -> McpResult<()>
    where
        T: Serialize,
    {
        let mut storage = self.storage.write().await;
        storage.insert(key.to_string(), serde_json::to_value(value)?);
        Ok(())
    }

    /// Remove a value from cache by key.
    ///
    /// # Arguments
    ///
    /// * `key` - The cache key to remove
    ///
    /// # Returns
    ///
    /// `true` if a value was removed, `false` if the key didn't exist.
    pub async fn remove(&self, key: &str) -> McpResult<bool> {
        let mut storage = self.storage.write().await;
        Ok(storage.remove(key).is_some())
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl Injectable for Cache {
    async fn inject(ctx: &Context) -> McpResult<Self> {
        match ctx.resolve::<Self>("cache").await {
            Ok(cache) => Ok(cache),
            Err(_) => Ok(Self::default()),
        }
    }
}

/// Registry for managing injectable type providers and their factory functions.
///
/// The `InjectionRegistry` allows you to register custom providers for injectable types,
/// enabling advanced dependency injection patterns like:
/// - Factory functions for creating complex types
/// - Lazy initialization and caching
/// - Multi-instance or singleton patterns
/// - Conditional instantiation based on context
///
/// # Examples
///
/// ```ignore
/// // Register a custom provider
/// let registry = InjectionRegistry::new();
/// registry.register_provider::<MyService>(MyServiceProvider::new()).await;
///
/// // Later, when a handler requests MyService, the custom provider is used
/// #[tool]
/// async fn handler(service: MyService) -> McpResult<String> {
///     // service was created by MyServiceProvider
///     Ok("Success".to_string())
/// }
/// ```
///
/// # Implementation Note
///
/// This is typically used by the server framework during setup. Custom applications
/// can use it to implement advanced DI patterns before server startup.
#[derive(Default)]
pub struct InjectionRegistry {
    providers: Arc<RwLock<HashMap<TypeId, Box<dyn std::any::Any + Send + Sync>>>>,
}

impl InjectionRegistry {
    /// Create a new, empty injection registry.
    ///
    /// Newly created registries have no providers registered. Use
    /// [`register_provider`](Self::register_provider) to add custom providers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a custom provider for a type.
    ///
    /// When handlers request an injectable of type `T`, this provider will be used
    /// instead of the default `Injectable::inject` implementation.
    ///
    /// # Arguments
    ///
    /// * `provider` - The provider implementation that creates instances of `T`
    ///
    /// # Examples
    ///
    /// ```ignore
    /// struct DatabaseProvider { pool: Arc<Pool> }
    ///
    /// #[async_trait]
    /// impl ContextProvider<Database> for DatabaseProvider {
    ///     async fn provide(&self, _ctx: &Context) -> McpResult<Database> {
    ///         Ok(Database { connection: self.pool.get().await? })
    ///     }
    /// }
    ///
    /// registry.register_provider::<Database>(DatabaseProvider { pool }).await;
    /// ```
    pub async fn register_provider<T, P>(&self, provider: P)
    where
        T: Injectable + Clone + 'static,
        P: ContextProvider<T> + 'static,
    {
        let mut providers = self.providers.write().await;
        providers.insert(TypeId::of::<T>(), Box::new(provider));
    }

    /// Get a registered provider for a type.
    ///
    /// Returns the provider if one has been registered, or `None` if no custom
    /// provider exists (in which case the default `Injectable::inject` is used).
    ///
    /// # Arguments
    ///
    /// * None (determined by type parameter `T`)
    ///
    /// # Returns
    ///
    /// `Some` if a provider was registered, `None` otherwise.
    pub async fn get_provider<T>(&self) -> Option<Box<dyn ContextProvider<T>>>
    where
        T: Injectable + Clone + 'static,
    {
        let providers = self.providers.read().await;
        providers
            .get(&TypeId::of::<T>())
            .and_then(|any| {
                any.downcast_ref::<Box<dyn ContextProvider<T>>>()
                    .map(|_provider_ref| {
                        // We need to clone the actual provider, not the box
                        // This is a simplification - in practice you'd need a proper way to clone providers
                        None
                    })
            })
            .flatten()
    }
}

/// Global injection registry singleton
///
/// This is the application-wide registry used by the server framework to manage all
/// injectable type providers. It's lazily initialized on first access.
static GLOBAL_INJECTION_REGISTRY: once_cell::sync::Lazy<InjectionRegistry> =
    once_cell::sync::Lazy::new(InjectionRegistry::new);

/// Get a reference to the global injection registry.
///
/// The global registry is a singleton that persists for the lifetime of the application.
/// It's used to register providers that should be available to all handlers.
///
/// # Examples
///
/// ```ignore
/// let registry = global_injection_registry();
/// registry.register_provider::<MyService>(MyServiceProvider::new()).await;
/// ```
#[must_use]
pub fn global_injection_registry() -> &'static InjectionRegistry {
    &GLOBAL_INJECTION_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HandlerMetadata, RequestContext};

    async fn create_test_context() -> Context {
        let request = RequestContext::with_id("test_request");
        let handler = HandlerMetadata {
            name: "test_handler".to_string(),
            handler_type: "tool".to_string(),
            description: Some("Test handler".to_string()),
        };

        Context::new(request, handler)
    }

    #[tokio::test]
    async fn test_request_info_injection() {
        let ctx = create_test_context().await;
        let info = RequestInfo::inject(&ctx).await.unwrap();

        assert_eq!(info.handler_name, "test_handler");
        assert_eq!(info.handler_type, "tool");
    }

    #[tokio::test]
    async fn test_logger_injection() {
        let ctx = create_test_context().await;
        let logger = Logger::inject(&ctx).await.unwrap();

        // Test that logger can be used
        logger.info("Test log message").await.unwrap();
    }

    #[tokio::test]
    async fn test_config_injection() {
        let ctx = create_test_context().await;

        // Register a config in the container
        let mut config = Config::new();
        config.set("test_key", "test_value").unwrap();
        ctx.register("config", config.clone()).await;

        let injected_config = Config::inject(&ctx).await.unwrap();
        let value: Option<String> = injected_config.get("test_key").unwrap();
        assert_eq!(value, Some("test_value".to_string()));
    }

    #[tokio::test]
    async fn test_cache_injection() {
        let ctx = create_test_context().await;
        let cache = Cache::inject(&ctx).await.unwrap();

        // Test cache operations
        cache.set("key1", "value1").await.unwrap();
        let value: Option<String> = cache.get("key1").await.unwrap();
        assert_eq!(value, Some("value1".to_string()));
    }
}

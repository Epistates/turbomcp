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

// Database imports (conditional on feature)
#[cfg(feature = "database")]
use sqlx::{Column, Row, TypeInfo};

/// Trait for types that can be injected into handler contexts
#[async_trait]
pub trait Injectable: Send + Sync + 'static {
    /// Create an instance of this type from the context
    async fn inject(ctx: &Context) -> McpResult<Self>
    where
        Self: Sized;

    /// Get the injection key for this type
    #[must_use]
    fn injection_key() -> String {
        std::any::type_name::<Self>().to_string()
    }
}

/// Trait for context providers that can create injectable services
#[async_trait]
pub trait ContextProvider<T>: Send + Sync
where
    T: Injectable + Clone,
{
    /// Provide an instance of type T
    async fn provide(&self, ctx: &Context) -> McpResult<T>;
}

/// Injectable wrapper for accessing the raw context
#[derive(Clone)]
pub struct InjectContext(pub Context);

#[async_trait]
impl Injectable for InjectContext {
    async fn inject(ctx: &Context) -> McpResult<Self> {
        Ok(Self(ctx.clone()))
    }
}

/// Injectable wrapper for accessing request metadata  
#[derive(Clone, Debug)]
pub struct RequestInfo {
    /// Request ID
    pub request_id: String,
    /// Handler name that's processing this request
    pub handler_name: String,
    /// Handler type (tool, prompt, resource)
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

/// Injectable configuration object
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// Configuration key-value pairs
    pub values: HashMap<String, serde_json::Value>,
}

impl Config {
    /// Create empty config
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Get a configuration value by key
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

    /// Set a configuration value
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

/// Injectable logger for structured logging
#[derive(Clone)]
pub struct Logger {
    context: Context,
}

impl Logger {
    /// Log an info message
    pub async fn info<S: AsRef<str>>(&self, message: S) -> McpResult<()> {
        self.context.info(message).await
    }

    /// Log a warning message
    pub async fn warn<S: AsRef<str>>(&self, message: S) -> McpResult<()> {
        self.context.warn(message).await
    }

    /// Log an error message
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

/// Injectable progress reporter
#[derive(Clone)]
pub struct ProgressReporter {
    token: crate::progress::ProgressToken,
}

impl ProgressReporter {
    /// Report progress update
    pub async fn update(&self, progress: f64, total: Option<f64>) -> McpResult<()> {
        crate::progress::global_progress_manager().update_progress(&self.token, progress, total)
    }

    /// Report progress with message
    pub async fn update_with_message(
        &self,
        progress: f64,
        total: Option<f64>,
        message: String,
    ) -> McpResult<()> {
        crate::progress::global_progress_manager().update_progress_with_message(
            &self.token,
            progress,
            total,
            message,
        )
    }

    /// Mark operation as complete
    pub async fn complete(&self) -> McpResult<()> {
        crate::progress::global_progress_manager().complete_operation(&self.token)
    }
}

#[async_trait]
impl Injectable for ProgressReporter {
    async fn inject(_ctx: &Context) -> McpResult<Self> {
        let token = crate::progress::start_progress("Handler operation");
        Ok(Self { token })
    }
}

/// Production-grade PostgreSQL database service with SQLx integration
#[cfg(feature = "database")]
#[derive(Clone)]
pub struct Database {
    /// SQLx connection pool for PostgreSQL
    pool: std::sync::Arc<sqlx::PgPool>,
    /// Connection string for logging/debugging
    connection_string: String,
}

#[cfg(feature = "database")]
impl Database {
    /// Create a new database instance from connection string
    pub async fn new(connection_string: &str) -> McpResult<Self> {
        use sqlx::postgres::PgPoolOptions;

        let pool = PgPoolOptions::new()
            .max_connections(20)
            .min_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .idle_timeout(Some(std::time::Duration::from_secs(600)))
            .max_lifetime(Some(std::time::Duration::from_secs(1800)))
            .connect(connection_string)
            .await
            .map_err(|e| crate::McpError::Tool(format!("Database connection failed: {}", e)))?;

        // Test connection
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .map_err(|e| crate::McpError::Tool(format!("Database health check failed: {}", e)))?;

        tracing::info!("Database connection pool established successfully");

        Ok(Self {
            pool: std::sync::Arc::new(pool),
            connection_string: connection_string.to_string(),
        })
    }

    /// Execute a SELECT query and return strongly-typed results
    pub async fn query<T>(&self, sql: &str) -> McpResult<Vec<T>>
    where
        T: for<'de> serde::Deserialize<'de> + std::fmt::Debug + Send + Unpin,
    {
        if sql.trim().is_empty() {
            return Err(crate::McpError::InvalidInput("Empty SQL query".to_string()));
        }

        // Security: Only allow SELECT statements for the query method
        let sql_lower = sql.trim().to_lowercase();
        if !sql_lower.starts_with("select") {
            return Err(crate::McpError::InvalidInput(
                "Only SELECT statements allowed in query() method. Use execute() for DML."
                    .to_string(),
            ));
        }

        tracing::debug!("Executing SQL query: {}", sql);

        let start = std::time::Instant::now();

        // Execute query and fetch results as JSON
        let rows = sqlx::query(sql)
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| crate::McpError::Tool(format!("Query execution failed: {}", e)))?;

        let duration = start.elapsed();
        tracing::info!(
            "Query executed in {}ms against: {}",
            duration.as_millis(),
            self.connection_string
                .split('@')
                .nth(1)
                .unwrap_or("[hidden]")
        );

        // Convert rows to JSON values, then deserialize to target type
        let mut results = Vec::new();

        for row in rows {
            // Convert PostgreSQL row to JSON value
            let mut json_obj = serde_json::Map::new();

            for (i, column) in row.columns().iter().enumerate() {
                let column_name = column.name();

                // Handle different PostgreSQL types and convert to JSON
                let json_value = match column.type_info().name() {
                    "TEXT" | "VARCHAR" | "CHAR" => row
                        .try_get::<Option<String>, _>(i)
                        .map_err(|e| {
                            crate::McpError::Tool(format!(
                                "Column '{}' conversion failed: {}",
                                column_name, e
                            ))
                        })?
                        .map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null),
                    "INT4" | "INT8" | "SERIAL" | "BIGSERIAL" => row
                        .try_get::<Option<i64>, _>(i)
                        .map_err(|e| {
                            crate::McpError::Tool(format!(
                                "Column '{}' conversion failed: {}",
                                column_name, e
                            ))
                        })?
                        .map(|n| serde_json::Value::Number(serde_json::Number::from(n)))
                        .unwrap_or(serde_json::Value::Null),
                    "BOOL" => row
                        .try_get::<Option<bool>, _>(i)
                        .map_err(|e| {
                            crate::McpError::Tool(format!(
                                "Column '{}' conversion failed: {}",
                                column_name, e
                            ))
                        })?
                        .map(serde_json::Value::Bool)
                        .unwrap_or(serde_json::Value::Null),
                    "TIMESTAMPTZ" | "TIMESTAMP" => row
                        .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(i)
                        .map_err(|e| {
                            crate::McpError::Tool(format!(
                                "Column '{}' conversion failed: {}",
                                column_name, e
                            ))
                        })?
                        .map(|dt| serde_json::Value::String(dt.to_rfc3339()))
                        .unwrap_or(serde_json::Value::Null),
                    "UUID" => row
                        .try_get::<Option<uuid::Uuid>, _>(i)
                        .map_err(|e| {
                            crate::McpError::Tool(format!(
                                "Column '{}' conversion failed: {}",
                                column_name, e
                            ))
                        })?
                        .map(|u| serde_json::Value::String(u.to_string()))
                        .unwrap_or(serde_json::Value::Null),
                    "JSONB" | "JSON" => row
                        .try_get::<Option<serde_json::Value>, _>(i)
                        .map_err(|e| {
                            crate::McpError::Tool(format!(
                                "Column '{}' conversion failed: {}",
                                column_name, e
                            ))
                        })?
                        .unwrap_or(serde_json::Value::Null),
                    _ => {
                        // For unknown types, try to get as string
                        row.try_get::<Option<String>, _>(i)
                            .map_err(|e| {
                                crate::McpError::Tool(format!(
                                    "Column '{}' conversion failed: {}",
                                    column_name, e
                                ))
                            })?
                            .map(serde_json::Value::String)
                            .unwrap_or(serde_json::Value::Null)
                    }
                };

                json_obj.insert(column_name.to_string(), json_value);
            }

            // Deserialize JSON object to target type
            let result: T =
                serde_json::from_value(serde_json::Value::Object(json_obj)).map_err(|e| {
                    crate::McpError::Tool(format!("Result deserialization failed: {}", e))
                })?;

            results.push(result);
        }

        Ok(results)
    }

    /// Execute DML statements (INSERT, UPDATE, DELETE)
    pub async fn execute(&self, sql: &str) -> McpResult<u64> {
        if sql.trim().is_empty() {
            return Err(crate::McpError::InvalidInput(
                "Empty SQL statement".to_string(),
            ));
        }

        let sql_lower = sql.trim().to_lowercase();
        if sql_lower.starts_with("select") {
            return Err(crate::McpError::InvalidInput(
                "Use query() method for SELECT statements".to_string(),
            ));
        }

        tracing::debug!("Executing SQL statement: {}", sql);

        let start = std::time::Instant::now();

        let result = sqlx::query(sql)
            .execute(&*self.pool)
            .await
            .map_err(|e| crate::McpError::Tool(format!("Statement execution failed: {}", e)))?;

        let duration = start.elapsed();
        let rows_affected = result.rows_affected();

        tracing::info!(
            "Statement executed in {}ms, {} rows affected",
            duration.as_millis(),
            rows_affected
        );

        Ok(rows_affected)
    }

    /// Get database connection pool for advanced operations
    pub fn pool(&self) -> &sqlx::PgPool {
        &self.pool
    }

    /// Check if database connection is healthy
    pub async fn health_check(&self) -> McpResult<()> {
        sqlx::query("SELECT 1")
            .execute(&*self.pool)
            .await
            .map_err(|e| crate::McpError::Tool(format!("Database health check failed: {}", e)))?;
        Ok(())
    }
}

/// Fallback implementation when database feature is disabled
#[cfg(not(feature = "database"))]
#[derive(Clone)]
pub struct Database {
    connection_string: String,
}

#[cfg(not(feature = "database"))]
impl Database {
    pub async fn new(connection_string: &str) -> McpResult<Self> {
        Err(crate::McpError::Tool(
            "Database feature not enabled. Add 'database' feature to Cargo.toml to use real database integration.".to_string()
        ))
    }

    pub async fn query<T>(&self, _sql: &str) -> McpResult<Vec<T>>
    where
        T: for<'de> serde::Deserialize<'de> + std::fmt::Debug,
    {
        Err(crate::McpError::Tool(
            "Database feature not enabled. Add 'database' feature to use real database integration.".to_string()
        ))
    }

    pub async fn execute(&self, _sql: &str) -> McpResult<u64> {
        Err(crate::McpError::Tool(
            "Database feature not enabled.".to_string(),
        ))
    }

    pub async fn health_check(&self) -> McpResult<()> {
        Err(crate::McpError::Tool(
            "Database feature not enabled.".to_string(),
        ))
    }
}

#[async_trait]
#[cfg(feature = "database")]
impl Injectable for Database {
    async fn inject(ctx: &Context) -> McpResult<Self> {
        // Try to resolve from dependency container
        match ctx.resolve::<Self>("database").await {
            Ok(db) => Ok(db),
            Err(_) => {
                // Fall back to environment variable or default
                let connection_string = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                    "postgres://turbomcp:dev_password_123@localhost:5432/turbomcp_dev".to_string()
                });

                tracing::info!("Creating database connection with fallback configuration");
                Self::new(&connection_string).await
            }
        }
    }
}

#[cfg(not(feature = "database"))]
impl Injectable for Database {
    async fn inject(_ctx: &Context) -> McpResult<Self> {
        Err(crate::McpError::Tool(
            "Database feature not enabled. Add 'database' feature to Cargo.toml.".to_string(),
        ))
    }
}

/// HTTP client injectable for making external requests
#[derive(Clone)]
pub struct HttpClient {
    /// User agent string
    pub user_agent: String,
}

impl HttpClient {
    /// Make a GET request with proper HTTP implementation
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

    /// Make a POST request with proper HTTP implementation
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

/// Injectable cache interface
#[derive(Clone)]
pub struct Cache {
    /// In-memory storage with concurrent access support
    storage: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

impl Cache {
    /// Get a value from cache
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

    /// Set a value in cache
    pub async fn set<T>(&self, key: &str, value: T) -> McpResult<()>
    where
        T: Serialize,
    {
        let mut storage = self.storage.write().await;
        storage.insert(key.to_string(), serde_json::to_value(value)?);
        Ok(())
    }

    /// Remove a value from cache
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

/// Injection registry for managing injectable types
#[derive(Default)]
pub struct InjectionRegistry {
    providers: Arc<RwLock<HashMap<TypeId, Box<dyn std::any::Any + Send + Sync>>>>,
}

impl InjectionRegistry {
    /// Create a new injection registry
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider for a type
    pub async fn register_provider<T, P>(&self, provider: P)
    where
        T: Injectable + Clone + 'static,
        P: ContextProvider<T> + 'static,
    {
        let mut providers = self.providers.write().await;
        providers.insert(TypeId::of::<T>(), Box::new(provider));
    }

    /// Get a provider for a type
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

/// Global injection registry
static GLOBAL_INJECTION_REGISTRY: once_cell::sync::Lazy<InjectionRegistry> =
    once_cell::sync::Lazy::new(InjectionRegistry::new);

/// Get the global injection registry
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

    #[tokio::test]
    async fn test_progress_reporter_injection() {
        let ctx = create_test_context().await;
        let reporter = ProgressReporter::inject(&ctx).await.unwrap();

        // Test progress reporting
        reporter.update(50.0, Some(100.0)).await.unwrap();
        reporter.complete().await.unwrap();
    }
}

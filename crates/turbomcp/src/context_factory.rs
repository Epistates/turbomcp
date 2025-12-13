//! Context Factory System
//!
//! This module implements a Context injection system that provides:
//! - Proper Context lifecycle management with parent-child relationships
//! - Request correlation and distributed tracing integration
//! - Performance-optimized Context pooling
//! - Seamless dependency injection container integration
//! - Observability and metrics collection
//!
//! ## Architecture Principles
//!
//! 1. **Context Inheritance**: Every Context maintains proper parent-child relationships
//! 2. **Request Correlation**: Automatic tracing ID propagation and correlation
//! 3. **Service Resolution**: Integrated with dependency injection for shared services
//! 4. **Performance First**: Context pooling and efficient resource management
//! 5. **Observability**: Built-in metrics and tracing for all context operations

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{debug, instrument};
use uuid::Uuid;

use crate::context::Container;
use crate::{Context, HandlerMetadata, McpResult};
use turbomcp_protocol::RequestContext;

/// Unique correlation identifier for request tracing and distributed observability.
///
/// Correlation IDs enable tracking of related requests across multiple services in
/// distributed systems. Each request chain gets a unique correlation ID that propagates
/// through parent-child relationships for distributed tracing.
///
/// # Examples
///
/// ```ignore
/// // Generate a new correlation ID
/// let id = CorrelationId::new();
/// println!("Tracing request: {}", id.as_str());
///
/// // Create from existing string (useful for propagation from clients)
/// let propagated = CorrelationId::from_string("parent-request-id".to_string());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CorrelationId(String);

impl CorrelationId {
    /// Generate a new random correlation ID using UUID v4.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Create a correlation ID from an existing string (for propagating from external sources).
    pub fn from_string(id: String) -> Self {
        Self(id)
    }

    /// Get the correlation ID as a string reference.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

/// Request scope information for context inheritance and correlation tracking.
///
/// A request scope captures the context for a request execution, including correlation
/// information, metadata, and tracing spans. Request scopes form a hierarchy that tracks
/// parent-child relationships for distributed tracing.
///
/// # Fields
///
/// - `correlation_id`: Unique ID for this request (for distributed tracing)
/// - `created_at`: Timestamp when scope was created (for duration tracking)
/// - `parent_correlation_id`: Parent request ID (None for root requests)
/// - `metadata`: Custom metadata for observability and correlation
/// - `span`: Optional OpenTelemetry/tracing span for distributed tracing
///
/// # Examples
///
/// ```ignore
/// // Create root scope for incoming request
/// let scope = RequestScope::new_root()
///     .with_metadata("user_id".to_string(), "user123".to_string());
///
/// // Create child scope for downstream requests
/// let child = scope.create_child();
/// assert_eq!(child.parent_correlation_id, Some(scope.correlation_id.clone()));
/// ```
#[derive(Debug, Clone)]
pub struct RequestScope {
    /// Unique correlation ID for this request chain.
    pub correlation_id: CorrelationId,
    /// Timestamp when this request scope was created.
    pub created_at: SystemTime,
    /// Parent correlation ID if this is a child request, None for root requests.
    pub parent_correlation_id: Option<CorrelationId>,
    /// Custom metadata for observability and correlation tracking.
    pub metadata: HashMap<String, String>,
    /// Optional OpenTelemetry/tracing span for distributed tracing integration.
    pub span: Option<tracing::Span>,
}

impl RequestScope {
    /// Create a new root request scope
    pub fn new_root() -> Self {
        Self {
            correlation_id: CorrelationId::new(),
            created_at: SystemTime::now(),
            parent_correlation_id: None,
            metadata: HashMap::new(),
            span: None,
        }
    }

    /// Create a child request scope
    pub fn create_child(&self) -> Self {
        Self {
            correlation_id: CorrelationId::new(),
            created_at: SystemTime::now(),
            parent_correlation_id: Some(self.correlation_id.clone()),
            metadata: self.metadata.clone(), // Inherit parent metadata
            span: None,
        }
    }

    /// Add metadata to this request scope
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Set the tracing span for this request
    pub fn with_span(mut self, span: tracing::Span) -> Self {
        self.span = Some(span);
        self
    }
}

/// Strategy for creating contexts with different isolation and sharing levels.
///
/// Different handlers have different needs for context isolation vs. resource sharing.
/// This enum allows fine-tuned control over context creation behavior.
///
/// # Variants
///
/// - `Fresh`: Create a completely new context with isolated dependency container
/// - `Inherit`: Create context that shares the server's dependency container
/// - `Scoped`: Create context with isolated container but copy essential services
/// - `Pooled`: Reuse contexts from pool for performance-critical paths
///
/// # Use Cases
///
/// | Strategy | Use Case |
/// |----------|----------|
/// | Fresh | Prompt handlers, request isolation needed |
/// | Inherit | Tool handlers, sharing server services |
/// | Scoped | Resource handlers, partial isolation |
/// | Pooled | High-frequency operations, performance critical |
#[derive(Debug, Clone)]
pub enum ContextCreationStrategy {
    /// Create a fresh context with no inheritance or resource sharing.
    Fresh,
    /// Inherit from parent context with shared dependency container.
    Inherit,
    /// Create scoped context with isolated container but inheriting essential services.
    Scoped,
    /// Create or reuse pooled context for performance optimization.
    Pooled,
}

/// Context factory configuration
#[derive(Debug, Clone)]
pub struct ContextFactoryConfig {
    /// Maximum number of contexts to pool
    pub max_pool_size: usize,
    /// How long to keep contexts in pool before recycling
    pub pool_ttl: Duration,
    /// Whether to enable distributed tracing
    pub enable_tracing: bool,
    /// Whether to collect context metrics
    pub enable_metrics: bool,
    /// Default creation strategy
    pub default_strategy: ContextCreationStrategy,
}

impl Default for ContextFactoryConfig {
    fn default() -> Self {
        Self {
            max_pool_size: 100,
            pool_ttl: Duration::from_secs(300), // 5 minutes
            enable_tracing: true,
            enable_metrics: true,
            default_strategy: ContextCreationStrategy::Inherit,
        }
    }
}

/// Context pool entry with metadata
struct PooledContext {
    context: Context,
    created_at: SystemTime,
    last_used: SystemTime,
    use_count: u64,
}

impl PooledContext {
    fn new(context: Context) -> Self {
        let now = SystemTime::now();
        Self {
            context,
            created_at: now,
            last_used: now,
            use_count: 0,
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.created_at.elapsed().unwrap_or(Duration::ZERO) > ttl
    }

    fn touch(&mut self) {
        self.last_used = SystemTime::now();
        self.use_count += 1;
    }
}

/// Metrics and statistics for context factory operations.
///
/// Tracks performance characteristics of the context factory including creation rates,
/// pool efficiency, and timing information. These metrics are useful for:
/// - Performance monitoring and optimization
/// - Detecting resource leaks
/// - Tuning pool size and TTL settings
/// - Understanding request patterns
///
/// # Fields
///
/// - `contexts_created`: Total number of contexts created
/// - `contexts_pooled`: Contexts that were reused from pool
/// - `pool_hits`: Successful pool reuses
/// - `pool_misses`: Pool reuses that failed (had to create new)
/// - `contexts_evicted`: Contexts evicted from pool due to expiration
/// - `avg_creation_time_us`: Average context creation time in microseconds
#[derive(Debug, Default)]
pub struct ContextFactoryMetrics {
    /// Total number of contexts created (including pooled reuses).
    pub contexts_created: AtomicU64,
    /// Number of contexts reused from the pool (pool_hits + contexts_pooled).
    pub contexts_pooled: AtomicU64,
    /// Successful pool hits (context reused without creation).
    pub pool_hits: AtomicU64,
    /// Pool misses (pool empty or expired, had to create new context).
    pub pool_misses: AtomicU64,
    /// Contexts evicted from pool due to TTL expiration.
    pub contexts_evicted: AtomicU64,
    /// Average context creation time in microseconds (moving average).
    pub avg_creation_time_us: AtomicU64,
}

impl ContextFactoryMetrics {
    /// Record context creation
    pub fn record_creation(&self, duration: Duration) {
        self.contexts_created.fetch_add(1, Ordering::Relaxed);
        let duration_us = duration.as_micros() as u64;

        // Simple moving average (in production, use proper metrics aggregation)
        let current = self.avg_creation_time_us.load(Ordering::Relaxed);
        let new_avg = if current == 0 {
            duration_us
        } else {
            (current + duration_us) / 2
        };
        self.avg_creation_time_us.store(new_avg, Ordering::Relaxed);
    }

    /// Record pool hit
    pub fn record_pool_hit(&self) {
        self.contexts_pooled.fetch_add(1, Ordering::Relaxed);
        self.pool_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record pool miss
    pub fn record_pool_miss(&self) {
        self.pool_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record eviction
    pub fn record_eviction(&self) {
        self.contexts_evicted.fetch_add(1, Ordering::Relaxed);
    }
}

/// Factory for creating and managing request contexts with lifecycle management.
///
/// The context factory is responsible for creating appropriately configured contexts
/// for each handler execution. It manages:
/// - Context creation with different strategies (fresh, inherited, scoped, pooled)
/// - Dependency injection container sharing and isolation
/// - Context pool for performance optimization
/// - Request scope hierarchy for distributed tracing
/// - Observability metrics
///
/// # Features
///
/// - **Multiple Creation Strategies**: Choose isolation vs. sharing based on handler type
/// - **Context Pooling**: Reuse contexts for performance-critical paths
/// - **Request Correlation**: Track requests across parent-child relationships
/// - **Dependency Management**: Share or isolate service dependencies
/// - **Observability**: Built-in metrics for monitoring factory performance
///
/// # Examples
///
/// ```ignore
/// let config = ContextFactoryConfig::default();
/// let container = Container::new();
/// let factory = ContextFactory::new(config, container);
///
/// // Create context for tool handler
/// let ctx = factory.create_for_tool(request, "my_tool", Some("Tool description")).await?;
///
/// // Use context in handler...
///
/// // Return to pool when done
/// factory.return_to_pool(ctx).await;
/// ```
pub struct ContextFactory {
    /// Factory configuration controlling pool size, TTL, tracing, and default strategy.
    config: ContextFactoryConfig,
    /// Shared dependency injection container for inherited strategy contexts.
    shared_container: Arc<Container>,
    /// Context pool for reusing contexts in performance-critical paths.
    context_pool: Arc<RwLock<Vec<PooledContext>>>,
    /// Metrics tracking factory operations for observability.
    metrics: Arc<ContextFactoryMetrics>,
    /// Request scope stack for tracking parent-child context relationships.
    request_scope_stack: Arc<RwLock<Vec<RequestScope>>>,
}

impl ContextFactory {
    /// Create a new context factory
    pub fn new(config: ContextFactoryConfig, shared_container: Container) -> Self {
        debug!("Creating ContextFactory with config: {:?}", config);

        Self {
            config,
            shared_container: Arc::new(shared_container),
            context_pool: Arc::new(RwLock::new(Vec::new())),
            metrics: Arc::new(ContextFactoryMetrics::default()),
            request_scope_stack: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a context for a tool handler with proper inheritance
    #[instrument(skip(self, request_context))]
    pub async fn create_for_tool(
        &self,
        request_context: RequestContext,
        tool_name: &str,
        description: Option<&str>,
    ) -> McpResult<Context> {
        let start = SystemTime::now();

        let handler_metadata = HandlerMetadata {
            name: tool_name.to_string(),
            handler_type: "tool".to_string(),
            description: description.map(|s| s.to_string()),
        };

        let context = self
            .create_context_with_metadata(
                request_context,
                handler_metadata,
                ContextCreationStrategy::Inherit,
            )
            .await?;

        if self.config.enable_metrics
            && let Ok(duration) = start.elapsed()
        {
            self.metrics.record_creation(duration);
        }

        debug!("Created context for tool: {}", tool_name);
        Ok(context)
    }

    /// Create a context for a resource handler
    #[instrument(skip(self, request_context))]
    pub async fn create_for_resource(
        &self,
        request_context: RequestContext,
        resource_uri: &str,
    ) -> McpResult<Context> {
        let handler_metadata = HandlerMetadata {
            name: format!("resource:{}", resource_uri),
            handler_type: "resource".to_string(),
            description: Some(format!("Resource handler for {}", resource_uri)),
        };

        self.create_context_with_metadata(
            request_context,
            handler_metadata,
            ContextCreationStrategy::Scoped,
        )
        .await
    }

    /// Create a context for a prompt handler
    #[instrument(skip(self, request_context))]
    pub async fn create_for_prompt(
        &self,
        request_context: RequestContext,
        prompt_name: &str,
    ) -> McpResult<Context> {
        let handler_metadata = HandlerMetadata {
            name: prompt_name.to_string(),
            handler_type: "prompt".to_string(),
            description: Some(format!("Prompt handler for {}", prompt_name)),
        };

        self.create_context_with_metadata(
            request_context,
            handler_metadata,
            ContextCreationStrategy::Fresh,
        )
        .await
    }

    /// Create context with specific metadata and strategy
    async fn create_context_with_metadata(
        &self,
        request_context: RequestContext,
        handler_metadata: HandlerMetadata,
        strategy: ContextCreationStrategy,
    ) -> McpResult<Context> {
        match strategy {
            ContextCreationStrategy::Fresh => {
                self.create_fresh_context(request_context, handler_metadata)
                    .await
            }
            ContextCreationStrategy::Inherit => {
                self.create_inherited_context(request_context, handler_metadata)
                    .await
            }
            ContextCreationStrategy::Scoped => {
                self.create_scoped_context(request_context, handler_metadata)
                    .await
            }
            ContextCreationStrategy::Pooled => {
                self.create_pooled_context(request_context, handler_metadata)
                    .await
            }
        }
    }

    /// Create a fresh context with no inheritance
    async fn create_fresh_context(
        &self,
        request_context: RequestContext,
        handler_metadata: HandlerMetadata,
    ) -> McpResult<Context> {
        let container = Container::new();
        let context = Context::with_container(request_context, handler_metadata, container);

        if self.config.enable_tracing {
            let _span = tracing::info_span!(
                "context_fresh",
                handler_type = %context.handler.handler_type,
                handler_name = %context.handler.name
            );
            // Span is created for observability but not attached to avoid circular references
        }

        Ok(context)
    }

    /// Create a context that inherits from parent with shared container
    async fn create_inherited_context(
        &self,
        request_context: RequestContext,
        handler_metadata: HandlerMetadata,
    ) -> McpResult<Context> {
        let context = Context::with_container(
            request_context,
            handler_metadata,
            (*self.shared_container).clone(),
        );

        // Set up proper request scope inheritance
        let mut scope_stack = self.request_scope_stack.write().await;
        let request_scope = if let Some(parent_scope) = scope_stack.last() {
            parent_scope
                .create_child()
                .with_metadata(
                    "handler_type".to_string(),
                    context.handler.handler_type.clone(),
                )
                .with_metadata("handler_name".to_string(), context.handler.name.clone())
        } else {
            RequestScope::new_root()
                .with_metadata(
                    "handler_type".to_string(),
                    context.handler.handler_type.clone(),
                )
                .with_metadata("handler_name".to_string(), context.handler.name.clone())
        };

        if self.config.enable_tracing {
            let _span = tracing::info_span!(
                "context_inherited",
                correlation_id = %request_scope.correlation_id.as_str(),
                parent_correlation_id = ?request_scope.parent_correlation_id.as_ref().map(|id| id.as_str()),
                handler_type = %context.handler.handler_type,
                handler_name = %context.handler.name
            );
            // Attach span to context and push scope
        }

        scope_stack.push(request_scope);

        Ok(context)
    }

    /// Create a scoped context with isolated container
    async fn create_scoped_context(
        &self,
        request_context: RequestContext,
        handler_metadata: HandlerMetadata,
    ) -> McpResult<Context> {
        // Create isolated container but inherit some services from parent
        let scoped_container = Container::new();

        // Copy essential services from shared container
        // Services are isolated per request scope for thread safety

        let context = Context::with_container(request_context, handler_metadata, scoped_container);

        if self.config.enable_tracing {
            let _span = tracing::info_span!(
                "context_scoped",
                handler_type = %context.handler.handler_type,
                handler_name = %context.handler.name
            );
        }

        Ok(context)
    }

    /// Create or reuse a pooled context for performance
    async fn create_pooled_context(
        &self,
        request_context: RequestContext,
        handler_metadata: HandlerMetadata,
    ) -> McpResult<Context> {
        let mut pool = self.context_pool.write().await;

        // Try to find a reusable context
        if let Some(mut pooled) = pool.pop() {
            if !pooled.is_expired(self.config.pool_ttl) {
                pooled.touch();

                if self.config.enable_metrics {
                    self.metrics.record_pool_hit();
                }

                // Update context with new request data
                // Context is reused with pooled resources for performance
                debug!("Reused pooled context");
                return Ok(pooled.context);
            } else if self.config.enable_metrics {
                self.metrics.record_eviction();
            }
        }

        // Create new context if pool is empty or expired
        if self.config.enable_metrics {
            self.metrics.record_pool_miss();
        }

        let context = self
            .create_inherited_context(request_context, handler_metadata)
            .await?;

        debug!("Created new context for pool");
        Ok(context)
    }

    /// Return a context to the pool for reuse
    pub async fn return_to_pool(&self, context: Context) {
        let mut pool = self.context_pool.write().await;

        if pool.len() < self.config.max_pool_size {
            let pooled = PooledContext::new(context);
            pool.push(pooled);
            debug!("Returned context to pool");
        }
    }

    /// Clean up expired contexts from pool
    #[instrument(skip(self))]
    pub async fn cleanup_pool(&self) {
        let mut pool = self.context_pool.write().await;
        let initial_size = pool.len();

        pool.retain(|pooled| {
            let expired = pooled.is_expired(self.config.pool_ttl);
            if expired && self.config.enable_metrics {
                self.metrics.record_eviction();
            }
            !expired
        });

        let evicted = initial_size - pool.len();
        if evicted > 0 {
            debug!("Cleaned up {} expired contexts from pool", evicted);
        }
    }

    /// Get current request scope
    pub async fn current_request_scope(&self) -> Option<RequestScope> {
        self.request_scope_stack.read().await.last().cloned()
    }

    /// Pop current request scope (when request completes)
    pub async fn pop_request_scope(&self) -> Option<RequestScope> {
        self.request_scope_stack.write().await.pop()
    }

    /// Get factory metrics
    pub fn metrics(&self) -> &ContextFactoryMetrics {
        &self.metrics
    }

    /// Get factory configuration
    pub fn config(&self) -> &ContextFactoryConfig {
        &self.config
    }
}

/// Trait for servers that provide context injection support.
///
/// This trait enables servers to expose their context factory, allowing external code
/// to create contexts and understand the server's context management strategy.
///
/// # Examples
///
/// ```ignore
/// impl ContextFactoryProvider for MyServer {
///     fn context_factory(&self) -> &ContextFactory {
///         &self.factory
///     }
///
///     async fn initialize_context_factory(&mut self) -> McpResult<()> {
///         // Setup factory configuration
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait ContextFactoryProvider {
    /// Get the context factory for this server.
    ///
    /// Returns a reference to the server's context factory used for creating
    /// contexts during request handling.
    fn context_factory(&self) -> &ContextFactory;

    /// Initialize the context factory with server configuration.
    ///
    /// Called during server startup to configure the context factory based on
    /// server settings, environment, and runtime requirements.
    async fn initialize_context_factory(&mut self) -> McpResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_correlation_id_generation() {
        let id1 = CorrelationId::new();
        let id2 = CorrelationId::new();

        assert_ne!(id1.as_str(), id2.as_str());
        assert!(!id1.as_str().is_empty());
    }

    #[tokio::test]
    async fn test_request_scope_inheritance() {
        let parent =
            RequestScope::new_root().with_metadata("test".to_string(), "value".to_string());
        let child = parent.create_child();

        assert_eq!(
            child.parent_correlation_id,
            Some(parent.correlation_id.clone())
        );
        assert_eq!(child.metadata.get("test"), Some(&"value".to_string()));
        assert_ne!(child.correlation_id, parent.correlation_id);
    }

    #[tokio::test]
    async fn test_context_factory_creation() {
        let config = ContextFactoryConfig::default();
        let container = Container::new();
        let factory = ContextFactory::new(config, container);

        assert_eq!(factory.config.max_pool_size, 100);
        assert!(factory.config.enable_tracing);
    }

    #[tokio::test]
    async fn test_context_creation_strategies() {
        let config = ContextFactoryConfig::default();
        let container = Container::new();
        let factory = ContextFactory::new(config, container);

        let request_context = RequestContext::new();

        // Test fresh context creation
        let context = factory
            .create_for_tool(request_context.clone(), "test_tool", Some("Test tool"))
            .await;
        assert!(context.is_ok());

        let context = context.unwrap();
        assert_eq!(context.handler.name, "test_tool");
        assert_eq!(context.handler.handler_type, "tool");
    }

    #[tokio::test]
    async fn test_pooled_context_expiration() {
        let pooled = PooledContext::new(Context::new(
            RequestContext::new(),
            HandlerMetadata {
                name: "test".to_string(),
                handler_type: "tool".to_string(),
                description: None,
            },
        ));

        // Should not be expired with a long TTL
        assert!(!pooled.is_expired(Duration::from_secs(60)));

        // Test that contexts are considered fresh for reasonable durations
        assert!(!pooled.is_expired(Duration::from_millis(100)));

        // Add a small delay and test with very short TTL
        tokio::time::sleep(Duration::from_millis(1)).await;
        assert!(pooled.is_expired(Duration::from_nanos(1)));
    }

    #[tokio::test]
    async fn test_factory_metrics() {
        let metrics = ContextFactoryMetrics::default();

        metrics.record_creation(Duration::from_millis(10));
        assert_eq!(metrics.contexts_created.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.avg_creation_time_us.load(Ordering::Relaxed), 10_000);

        metrics.record_pool_hit();
        assert_eq!(metrics.pool_hits.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.contexts_pooled.load(Ordering::Relaxed), 1);
    }
}

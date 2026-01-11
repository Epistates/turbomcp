//! Server lifespan management with startup/shutdown hooks

use crate::McpErrorConstructors;
use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tracing::{error, info /*, warn*/};

use crate::{McpError, McpResult};

/// Lifespan event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifespanEvent {
    /// Server is starting up
    Startup,
    /// Server is shutting down
    Shutdown,
}

/// Priority levels for hooks
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum HookPriority {
    /// Critical system hooks (run first on startup, last on shutdown)
    Critical = 0,
    /// High priority hooks
    High = 100,
    /// Normal priority hooks (default)
    #[default]
    Normal = 500,
    /// Low priority hooks
    Low = 900,
}

/// Lifespan hook trait
#[async_trait]
pub trait LifespanHook: Send + Sync {
    /// Hook name for logging and debugging
    fn name(&self) -> &str;

    /// Hook priority
    fn priority(&self) -> HookPriority {
        HookPriority::Normal
    }

    /// Execute the hook
    async fn execute(&self, event: LifespanEvent) -> McpResult<()>;

    /// Whether this hook should run on the given event
    fn handles_event(&self, event: LifespanEvent) -> bool {
        match event {
            LifespanEvent::Startup => self.handles_startup(),
            LifespanEvent::Shutdown => self.handles_shutdown(),
        }
    }

    /// Whether this hook handles startup events
    fn handles_startup(&self) -> bool {
        true
    }

    /// Whether this hook handles shutdown events
    fn handles_shutdown(&self) -> bool {
        true
    }

    /// Maximum time this hook should take to complete
    fn timeout(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs(30))
    }
}

/// Hook execution result
#[derive(Debug, Clone)]
pub struct HookExecutionResult {
    /// Name of the hook that was executed
    pub hook_name: String,
    /// Event that triggered the hook
    pub event: LifespanEvent,
    /// Whether the hook execution succeeded
    pub success: bool,
    /// How long the hook took to execute
    pub duration: std::time::Duration,
    /// Error if the hook failed
    pub error: Option<McpError>,
}

/// Lifespan manager
pub struct LifespanManager {
    hooks: Arc<RwLock<VecDeque<Box<dyn LifespanHook>>>>,
    execution_results: Arc<RwLock<Vec<HookExecutionResult>>>,
}

impl LifespanManager {
    /// Create a new lifespan manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            hooks: Arc::new(RwLock::new(VecDeque::new())),
            execution_results: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a lifespan hook
    pub async fn register_hook(&self, hook: Box<dyn LifespanHook>) {
        let priority = hook.priority();
        let mut hooks = self.hooks.write().await;

        // Insert hook in priority order
        let insert_pos = hooks
            .iter()
            .position(|existing_hook| priority < existing_hook.priority());

        if let Some(pos) = insert_pos {
            hooks.insert(pos, hook);
        } else {
            hooks.push_back(hook);
        }

        info!("Registered lifespan hook with priority {:?}", priority);
    }

    /// Execute hooks for a specific event
    pub async fn execute_hooks(&self, event: LifespanEvent) -> McpResult<()> {
        let hooks = self.hooks.read().await;
        let applicable_hooks: Vec<_> = hooks
            .iter()
            .filter(|hook| hook.handles_event(event))
            .collect();

        if applicable_hooks.is_empty() {
            info!("No hooks registered for event {:?}", event);
            return Ok(());
        }

        info!(
            "Executing {} hooks for event {:?}",
            applicable_hooks.len(),
            event
        );

        let hooks_to_execute = match event {
            LifespanEvent::Startup => applicable_hooks,
            LifespanEvent::Shutdown => {
                // Reverse order for shutdown
                let mut reversed = applicable_hooks;
                reversed.reverse();
                reversed
            }
        };

        let mut all_succeeded = true;

        for hook in hooks_to_execute {
            let hook_name = hook.name().to_string();
            let start_time = std::time::Instant::now();

            info!("Executing hook: {}", hook_name);

            let result = if let Some(timeout) = hook.timeout() {
                // Execute with timeout
                if let Ok(result) = tokio::time::timeout(timeout, hook.execute(event)).await {
                    result
                } else {
                    let error_msg = format!("Hook '{hook_name}' timed out after {timeout:?}");
                    error!("{}", error_msg);
                    Err(McpError::context(error_msg))
                }
            } else {
                // Execute without timeout
                hook.execute(event).await
            };

            let duration = start_time.elapsed();

            let execution_result = match result {
                Ok(()) => {
                    info!(
                        "Hook '{}' completed successfully in {:?}",
                        hook_name, duration
                    );
                    HookExecutionResult {
                        hook_name,
                        event,
                        success: true,
                        duration,
                        error: None,
                    }
                }
                Err(error) => {
                    error!(
                        "Hook '{}' failed after {:?}: {}",
                        hook_name, duration, error
                    );
                    all_succeeded = false;
                    HookExecutionResult {
                        hook_name,
                        event,
                        success: false,
                        duration,
                        error: Some(error),
                    }
                }
            };

            self.execution_results.write().await.push(execution_result);
        }

        if all_succeeded {
            info!("All hooks for event {:?} completed successfully", event);
            Ok(())
        } else {
            let error_msg = format!("Some hooks failed for event {event:?}");
            error!("{}", error_msg);
            Err(McpError::context(error_msg))
        }
    }

    /// Get execution results
    pub async fn get_execution_results(&self) -> Vec<HookExecutionResult> {
        self.execution_results.read().await.clone()
    }

    /// Clear execution results
    pub async fn clear_execution_results(&self) {
        self.execution_results.write().await.clear();
    }

    /// Get registered hook names
    pub async fn get_hook_names(&self) -> Vec<String> {
        let hooks = self.hooks.read().await;
        hooks.iter().map(|hook| hook.name().to_string()).collect()
    }

    /// Count of registered hooks
    pub async fn hook_count(&self) -> usize {
        self.hooks.read().await.len()
    }
}

impl Default for LifespanManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple function hook wrapper
pub struct FunctionHook<F>
where
    F: Fn(
            LifespanEvent,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send>>
        + Send
        + Sync,
{
    name: String,
    priority: HookPriority,
    func: F,
    startup: bool,
    shutdown: bool,
    timeout: Option<std::time::Duration>,
}

impl<F> FunctionHook<F>
where
    F: Fn(
            LifespanEvent,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send>>
        + Send
        + Sync,
{
    /// Create a new function hook
    pub fn new(name: impl Into<String>, func: F) -> Self {
        Self {
            name: name.into(),
            priority: HookPriority::Normal,
            func,
            startup: true,
            shutdown: true,
            timeout: Some(std::time::Duration::from_secs(30)),
        }
    }

    /// Set the priority for this hook
    pub const fn with_priority(mut self, priority: HookPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Configure hook to run only on startup
    pub const fn startup_only(mut self) -> Self {
        self.startup = true;
        self.shutdown = false;
        self
    }

    /// Configure hook to run only on shutdown
    pub const fn shutdown_only(mut self) -> Self {
        self.startup = false;
        self.shutdown = true;
        self
    }

    /// Set a timeout for this hook
    pub const fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Remove timeout from this hook
    pub const fn no_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }
}

#[async_trait]
impl<F> LifespanHook for FunctionHook<F>
where
    F: Fn(
            LifespanEvent,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = McpResult<()>> + Send>>
        + Send
        + Sync,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> HookPriority {
        self.priority
    }

    async fn execute(&self, event: LifespanEvent) -> McpResult<()> {
        (self.func)(event).await
    }

    fn handles_startup(&self) -> bool {
        self.startup
    }

    fn handles_shutdown(&self) -> bool {
        self.shutdown
    }

    fn timeout(&self) -> Option<std::time::Duration> {
        self.timeout
    }
}

/// Database connection hook example
pub struct DatabaseHook {
    connection_string: String,
}

impl DatabaseHook {
    /// Create a new database hook
    #[must_use]
    pub const fn new(connection_string: String) -> Self {
        Self { connection_string }
    }
}

#[async_trait]
impl LifespanHook for DatabaseHook {
    fn name(&self) -> &'static str {
        "database"
    }

    fn priority(&self) -> HookPriority {
        HookPriority::Critical
    }

    async fn execute(&self, event: LifespanEvent) -> McpResult<()> {
        match event {
            LifespanEvent::Startup => {
                info!("Connecting to database: {}", self.connection_string);
                // Simulate database connection
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                info!("Database connection established");
                Ok(())
            }
            LifespanEvent::Shutdown => {
                info!("Closing database connection");
                // Simulate database cleanup
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                info!("Database connection closed");
                Ok(())
            }
        }
    }
}

/// Cache warming hook example
pub struct CacheWarmupHook {
    cache_size: usize,
}

impl CacheWarmupHook {
    /// Create a new cache warmup hook
    #[must_use]
    pub const fn new(cache_size: usize) -> Self {
        Self { cache_size }
    }
}

#[async_trait]
impl LifespanHook for CacheWarmupHook {
    fn name(&self) -> &'static str {
        "cache_warmup"
    }

    fn priority(&self) -> HookPriority {
        HookPriority::Low
    }

    async fn execute(&self, event: LifespanEvent) -> McpResult<()> {
        match event {
            LifespanEvent::Startup => {
                info!("Warming up cache with {} items", self.cache_size);
                // Simulate cache warming
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                info!("Cache warmed up");
                Ok(())
            }
            LifespanEvent::Shutdown => {
                info!("Flushing cache");
                // Simulate cache flush
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                info!("Cache flushed");
                Ok(())
            }
        }
    }

    fn handles_shutdown(&self) -> bool {
        true
    }
}

/// Metrics collection hook example
pub struct MetricsHook;

#[async_trait]
impl LifespanHook for MetricsHook {
    fn name(&self) -> &'static str {
        "metrics"
    }

    fn priority(&self) -> HookPriority {
        HookPriority::High
    }

    async fn execute(&self, event: LifespanEvent) -> McpResult<()> {
        match event {
            LifespanEvent::Startup => {
                info!("Starting metrics collection");
                Ok(())
            }
            LifespanEvent::Shutdown => {
                info!("Stopping metrics collection and flushing data");
                // Simulate metrics flush
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                info!("Metrics data flushed");
                Ok(())
            }
        }
    }
}

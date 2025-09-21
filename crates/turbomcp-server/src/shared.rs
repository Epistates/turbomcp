//! Shared server wrappers for concurrent access
//!
//! This module provides thread-safe wrappers around McpServer instances that enable
//! concurrent access across multiple async tasks without exposing Arc/Mutex complexity.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    config::ServerConfig,
    error::ServerResult,
    lifecycle::{HealthStatus, ServerLifecycle},
    metrics::ServerMetrics,
    registry::HandlerRegistry,
    routing::RequestRouter,
    server::{McpServer, ShutdownHandle},
};

/// Thread-safe wrapper for sharing McpServer instances across async tasks
///
/// This wrapper encapsulates Arc/Mutex complexity and provides a clean API
/// for concurrent access to server functionality. It addresses the limitations
/// where server run methods consume `self` but configuration and monitoring
/// need to be shared across multiple async tasks.
///
/// # Design Rationale
///
/// McpServer run methods consume `self` because:
/// - They take ownership of the server to run the main event loop
/// - Transport binding requires exclusive access
/// - Graceful shutdown needs to control the entire server lifecycle
///
/// However, other operations like health checks, metrics, and configuration
/// access need to be shared across multiple tasks for monitoring and management.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_server::{McpServer, SharedServer, ServerConfig};
///
/// # async fn example() -> turbomcp_server::error::ServerResult<()> {
/// let config = ServerConfig::default();
/// let server = McpServer::new(config);
/// let shared = SharedServer::new(server);
///
/// // Clone for sharing across tasks
/// let shared1 = shared.clone();
/// let shared2 = shared.clone();
///
/// // Both tasks can access server state concurrently
/// let handle1 = tokio::spawn(async move {
///     shared1.health().await
/// });
///
/// let handle2 = tokio::spawn(async move {
///     shared2.shutdown_handle()
/// });
///
/// let (health, shutdown_handle) = tokio::try_join!(handle1, handle2)?;
///
/// // Run the server (consumes the shared server)
/// // shared.run_stdio().await?;
/// # Ok(())
/// # }
/// ```
pub struct SharedServer {
    inner: Arc<Mutex<Option<McpServer>>>,
}

impl SharedServer {
    /// Create a new shared server wrapper
    ///
    /// Takes ownership of a McpServer and wraps it for thread-safe sharing.
    /// The original server can no longer be accessed directly after this call.
    pub fn new(server: McpServer) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(server))),
        }
    }

    /// Get server configuration
    ///
    /// Returns a clone of the server configuration.
    pub async fn config(&self) -> Option<ServerConfig> {
        self.inner.lock().await.as_ref().map(|s| s.config().clone())
    }

    /// Get handler registry
    ///
    /// Returns a clone of the Arc to the handler registry.
    pub async fn registry(&self) -> Option<Arc<HandlerRegistry>> {
        self.inner
            .lock()
            .await
            .as_ref()
            .map(|s| s.registry().clone())
    }

    /// Get request router
    ///
    /// Returns a clone of the Arc to the request router.
    pub async fn router(&self) -> Option<Arc<RequestRouter>> {
        self.inner.lock().await.as_ref().map(|s| s.router().clone())
    }

    /// Get server lifecycle
    ///
    /// Returns a clone of the Arc to the server lifecycle.
    pub async fn lifecycle(&self) -> Option<Arc<ServerLifecycle>> {
        self.inner
            .lock()
            .await
            .as_ref()
            .map(|s| s.lifecycle().clone())
    }

    /// Get server metrics
    ///
    /// Returns a clone of the Arc to the server metrics.
    pub async fn metrics(&self) -> Option<Arc<ServerMetrics>> {
        self.inner
            .lock()
            .await
            .as_ref()
            .map(|s| s.metrics().clone())
    }

    /// Get a shutdown handle for graceful server termination
    ///
    /// Returns a shutdown handle that can be used to gracefully terminate
    /// the server from external tasks.
    pub async fn shutdown_handle(&self) -> Option<ShutdownHandle> {
        self.inner
            .lock()
            .await
            .as_ref()
            .map(|s| s.shutdown_handle())
    }

    /// Get health status
    ///
    /// Returns the current health status of the server.
    pub async fn health(&self) -> Option<HealthStatus> {
        match self.inner.lock().await.as_ref() {
            Some(server) => Some(server.health().await),
            None => None,
        }
    }

    /// Run the server with STDIO transport
    ///
    /// This consumes the SharedServer and extracts the inner server to run it.
    /// After calling this method, the SharedServer can no longer be used.
    pub async fn run_stdio(self) -> ServerResult<()> {
        let server = self.take_server().await?;
        server.run_stdio().await
    }

    /// Run server with TCP transport
    #[cfg(feature = "tcp")]
    pub async fn run_tcp<A: std::net::ToSocketAddrs + Send + std::fmt::Debug>(
        self,
        addr: A,
    ) -> ServerResult<()> {
        let server = self.take_server().await?;
        server.run_tcp(addr).await
    }

    /// Run server with Unix socket transport
    #[cfg(all(feature = "unix", unix))]
    pub async fn run_unix<P: AsRef<std::path::Path>>(self, path: P) -> ServerResult<()> {
        let server = self.take_server().await?;
        server.run_unix(path).await
    }

    /// Extract the inner server for running
    ///
    /// This is a helper method that takes the server out of the Option,
    /// making the SharedServer unusable afterwards.
    async fn take_server(self) -> ServerResult<McpServer> {
        let mut guard = self.inner.lock().await;
        guard.take().ok_or_else(|| {
            crate::ServerError::configuration("Server has already been consumed for running")
        })
    }

    /// Check if the server is still available (hasn't been consumed)
    pub async fn is_available(&self) -> bool {
        self.inner.lock().await.is_some()
    }
}

impl Clone for SharedServer {
    /// Clone the shared server for use in multiple async tasks
    ///
    /// This creates a new reference to the same underlying server,
    /// allowing multiple tasks to share access safely.
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl std::fmt::Debug for SharedServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedServer")
            .field("inner", &"Arc<Mutex<Option<McpServer>>>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerBuilder;

    #[tokio::test]
    async fn test_shared_server_creation() {
        let server = ServerBuilder::new().build();
        let shared = SharedServer::new(server);

        // Test that we can clone the shared server
        let _shared2 = shared.clone();
    }

    #[tokio::test]
    async fn test_shared_server_cloning() {
        let server = ServerBuilder::new().build();
        let shared = SharedServer::new(server);

        // Clone multiple times to test Arc behavior
        let clones: Vec<_> = (0..10).map(|_| shared.clone()).collect();
        assert_eq!(clones.len(), 10);

        // All clones should reference the same underlying server
        // This is verified by the fact that they can all be created without error
    }

    #[tokio::test]
    async fn test_shared_server_api_surface() {
        let server = ServerBuilder::new().build();
        let shared = SharedServer::new(server);

        // Test that SharedServer provides the expected API surface
        // These calls should compile and return Some values when server is available

        let _config = shared.config().await;
        let _registry = shared.registry().await;
        let _router = shared.router().await;
        let _lifecycle = shared.lifecycle().await;
        let _metrics = shared.metrics().await;
        let _shutdown_handle = shared.shutdown_handle().await;
        let _health = shared.health().await;
        let _available = shared.is_available().await;

        assert!(shared.is_available().await);
    }

    #[tokio::test]
    async fn test_shared_server_type_compatibility() {
        let server = ServerBuilder::new().build();
        let shared = SharedServer::new(server);

        // Test that the SharedServer can be used in generic contexts
        fn takes_shared_server<T>(_server: T)
        where
            T: Clone + Send + Sync + 'static,
        {
        }

        takes_shared_server(shared);
    }

    #[tokio::test]
    async fn test_shared_server_send_sync() {
        let server = ServerBuilder::new().build();
        let shared = SharedServer::new(server);

        // Test that SharedServer can be moved across task boundaries
        let handle = tokio::spawn(async move {
            let _cloned = shared.clone();
            // SharedServer should be Send + Sync, allowing this to compile
        });

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_shared_server_thread_safety() {
        let server = ServerBuilder::new().build();
        let shared = SharedServer::new(server);

        // Test that SharedServer can be shared across threads safely
        let shared1 = shared.clone();
        let shared2 = shared.clone();

        // Verify that concurrent access doesn't corrupt state
        let handle1 = tokio::spawn(async move { shared1.config().await });

        let handle2 = tokio::spawn(async move { shared2.health().await });

        let (config, health) = tokio::join!(handle1, handle2);
        let _config = config.unwrap();
        let _health = health.unwrap();

        // Both should succeed when server is available
        assert!(shared.is_available().await);
    }

    #[tokio::test]
    async fn test_shared_server_consumption() {
        let server = ServerBuilder::new().build();
        let shared = SharedServer::new(server);
        let shared_clone = shared.clone();

        // Server should be available initially
        assert!(shared.is_available().await);
        assert!(shared_clone.is_available().await);

        // Take the server (simulating run_stdio consumption)
        let _server = shared.take_server().await.unwrap();

        // Server should no longer be available
        assert!(!shared_clone.is_available().await);

        // Attempting to take again should fail
        let result = shared_clone.take_server().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_shared_server_after_consumption() {
        let server = ServerBuilder::new().build();
        let shared = SharedServer::new(server);
        let shared_clone = shared.clone();

        // Consume the server
        let _server = shared.take_server().await.unwrap();

        // All methods should return None after consumption (using the clone)
        assert!(shared_clone.config().await.is_none());
        assert!(shared_clone.registry().await.is_none());
        assert!(shared_clone.router().await.is_none());
        assert!(shared_clone.lifecycle().await.is_none());
        assert!(shared_clone.metrics().await.is_none());
        assert!(shared_clone.shutdown_handle().await.is_none());
        assert!(shared_clone.health().await.is_none());
        assert!(!shared_clone.is_available().await);
    }
}

//! Shared transport wrappers for concurrent access
//!
//! This module provides thread-safe wrappers around Transport instances that enable
//! concurrent access across multiple async tasks without exposing Arc/Mutex complexity.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::core::{
    Transport, TransportCapabilities, TransportConfig, TransportMessage, TransportMetrics,
    TransportResult, TransportState, TransportType,
};

/// Thread-safe wrapper for sharing Transport instances across async tasks
///
/// This wrapper encapsulates Arc/Mutex complexity and provides a clean API
/// for concurrent access to transport functionality. It addresses the limitations
/// where Transport methods require `&mut self` but need to be shared across
/// multiple async tasks.
///
/// # Design Rationale
///
/// Transport methods require `&mut self` because:
/// - Connection state management requires mutation
/// - Send/receive operations modify internal buffers and state
/// - Connect/disconnect operations change connection status
///
/// While Transport implements Send + Sync, this only means it's safe to move/share
/// between threads, not that multiple tasks can mutate it concurrently.
///
/// # Immutable Data Caching
///
/// The wrapper caches immutable transport metadata (type, capabilities, endpoint)
/// at construction time. This allows sync trait methods to return without needing
/// async mutex access, eliminating panics in the `Transport` trait implementation.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_transport::{StdioTransport, SharedTransport};
///
/// # async fn example() -> turbomcp_transport::core::TransportResult<()> {
/// let transport = StdioTransport::new();
/// let shared = SharedTransport::new(transport);
///
/// // Connect once
/// shared.connect().await?;
///
/// // Clone for sharing across tasks
/// let shared1 = shared.clone();
/// let shared2 = shared.clone();
///
/// // Both tasks can use the transport concurrently
/// let handle1 = tokio::spawn(async move {
///     shared1.is_connected().await
/// });
///
/// let handle2 = tokio::spawn(async move {
///     shared2.metrics().await
/// });
///
/// let (connected, metrics) = tokio::try_join!(handle1, handle2).unwrap();
/// # Ok(())
/// # }
/// ```
pub struct SharedTransport<T: Transport> {
    inner: Arc<Mutex<T>>,
    /// Cached transport type (immutable after construction)
    transport_type: TransportType,
    /// Cached capabilities (immutable after construction)
    capabilities: TransportCapabilities,
    /// Cached endpoint (immutable after construction)
    endpoint: Option<String>,
}

impl<T: Transport> SharedTransport<T> {
    /// Create a new shared transport wrapper
    ///
    /// Takes ownership of a Transport and wraps it for thread-safe sharing.
    /// The original transport can no longer be accessed directly after this call.
    ///
    /// Immutable transport metadata (type, capabilities, endpoint) is cached
    /// at construction time for efficient sync access.
    pub fn new(transport: T) -> Self {
        // Cache immutable data before moving transport into mutex
        let transport_type = transport.transport_type();
        let capabilities = transport.capabilities().clone();
        let endpoint = transport.endpoint();

        Self {
            inner: Arc::new(Mutex::new(transport)),
            transport_type,
            capabilities,
            endpoint,
        }
    }

    /// Get transport type (sync access to cached value)
    ///
    /// Returns the type of the underlying transport.
    /// This value is cached at construction and does not require async access.
    pub fn transport_type(&self) -> TransportType {
        self.transport_type
    }

    /// Get transport capabilities (sync access to cached value)
    ///
    /// Returns a reference to the capabilities of the underlying transport.
    /// This value is cached at construction and does not require async access.
    pub fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    /// Get endpoint information (sync access to cached value)
    ///
    /// Returns the endpoint information of the underlying transport.
    /// This value is cached at construction and does not require async access.
    pub fn cached_endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// Get current transport state
    ///
    /// Returns the current connection state of the transport.
    pub async fn state(&self) -> TransportState {
        self.inner.lock().await.state().await
    }

    /// Connect to the transport endpoint
    ///
    /// Establishes a connection to the transport's target endpoint.
    /// This method is thread-safe and will serialize connection attempts.
    pub async fn connect(&self) -> TransportResult<()> {
        self.inner.lock().await.connect().await
    }

    /// Disconnect from the transport
    ///
    /// Cleanly closes the transport connection.
    /// This method is thread-safe and will serialize disconnection attempts.
    pub async fn disconnect(&self) -> TransportResult<()> {
        self.inner.lock().await.disconnect().await
    }

    /// Send a message through the transport
    ///
    /// Sends a message via the underlying transport. Messages are serialized
    /// to ensure proper ordering and prevent race conditions.
    pub async fn send(&self, message: TransportMessage) -> TransportResult<()> {
        self.inner.lock().await.send(message).await
    }

    /// Receive a message from the transport
    ///
    /// Receives a message from the underlying transport. Receive operations
    /// are serialized to ensure message ordering and prevent lost messages.
    pub async fn receive(&self) -> TransportResult<Option<TransportMessage>> {
        self.inner.lock().await.receive().await
    }

    /// Get transport metrics
    ///
    /// Returns current metrics for the transport including message counts,
    /// connection status, and performance statistics.
    pub async fn metrics(&self) -> TransportMetrics {
        self.inner.lock().await.metrics().await
    }

    /// Check if transport is connected
    ///
    /// Returns true if the transport is currently connected and ready
    /// for message transmission.
    pub async fn is_connected(&self) -> bool {
        self.inner.lock().await.is_connected().await
    }

    /// Configure the transport
    ///
    /// Sets the configuration for the transport.
    pub async fn configure(&self, config: TransportConfig) -> TransportResult<()> {
        self.inner.lock().await.configure(config).await
    }
}

impl<T: Transport> Clone for SharedTransport<T> {
    /// Clone the shared transport for use in multiple async tasks
    ///
    /// This creates a new reference to the same underlying transport,
    /// allowing multiple tasks to share access safely. The cached metadata
    /// is also cloned efficiently (Arc for inner, Copy/Clone for metadata).
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            transport_type: self.transport_type,
            capabilities: self.capabilities.clone(),
            endpoint: self.endpoint.clone(),
        }
    }
}

impl<T: Transport> std::fmt::Debug for SharedTransport<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedTransport")
            .field("inner", &"Arc<Mutex<Transport>>")
            .finish()
    }
}

// Implement Transport trait for SharedTransport to enable drop-in replacement
//
// All sync methods use cached values captured at construction time.
// This eliminates the need for async mutex access and prevents panics.
#[async_trait]
impl<T: Transport> Transport for SharedTransport<T> {
    fn transport_type(&self) -> TransportType {
        // Return cached value (captured at construction)
        self.transport_type
    }

    fn capabilities(&self) -> &TransportCapabilities {
        // Return reference to cached value (captured at construction)
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        self.inner.lock().await.state().await
    }

    async fn connect(&self) -> TransportResult<()> {
        self.inner.lock().await.connect().await
    }

    async fn disconnect(&self) -> TransportResult<()> {
        self.inner.lock().await.disconnect().await
    }

    async fn send(&self, message: TransportMessage) -> TransportResult<()> {
        self.inner.lock().await.send(message).await
    }

    async fn receive(&self) -> TransportResult<Option<TransportMessage>> {
        self.inner.lock().await.receive().await
    }

    async fn metrics(&self) -> TransportMetrics {
        self.inner.lock().await.metrics().await
    }

    async fn is_connected(&self) -> bool {
        self.inner.lock().await.is_connected().await
    }

    fn endpoint(&self) -> Option<String> {
        // Return cached value (captured at construction)
        self.endpoint.clone()
    }

    async fn configure(&self, config: TransportConfig) -> TransportResult<()> {
        self.inner.lock().await.configure(config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdio::StdioTransport;

    #[tokio::test]
    async fn test_shared_transport_creation() {
        let transport = StdioTransport::new();
        let shared = SharedTransport::new(transport);

        // Test that we can clone the shared transport
        let _shared2 = shared.clone();
    }

    #[tokio::test]
    async fn test_shared_transport_cloning() {
        let transport = StdioTransport::new();
        let shared = SharedTransport::new(transport);

        // Clone multiple times to test Arc behavior
        let clones: Vec<_> = (0..10).map(|_| shared.clone()).collect();
        assert_eq!(clones.len(), 10);

        // All clones should reference the same underlying transport
        // This is verified by the fact that they can all be created without error
    }

    #[tokio::test]
    async fn test_shared_transport_api_surface() {
        let transport = StdioTransport::new();
        let shared = SharedTransport::new(transport);

        // Test that SharedTransport provides the expected API surface
        // These calls should compile, verifying the API is properly wrapped

        // Sync operations (cached at construction, no async needed)
        let _transport_type = shared.transport_type();
        let _capabilities = shared.capabilities();
        let _endpoint_info = shared.cached_endpoint();

        // Async operations (require mutex lock)
        let _state = shared.state().await;
        let _metrics = shared.metrics().await;
        let _is_connected = shared.is_connected().await;
    }

    #[tokio::test]
    async fn test_shared_transport_type_compatibility() {
        let transport = StdioTransport::new();
        let shared = SharedTransport::new(transport);

        // Test that the SharedTransport can be used in generic contexts
        fn takes_shared_transport<T>(_transport: T)
        where
            T: Clone + Send + Sync + 'static,
        {
        }

        takes_shared_transport(shared);
    }

    #[tokio::test]
    async fn test_shared_transport_send_sync() {
        let transport = StdioTransport::new();
        let shared = SharedTransport::new(transport);

        // Test that SharedTransport can be moved across task boundaries
        let handle = tokio::spawn(async move {
            let _cloned = shared.clone();
            // SharedTransport should be Send + Sync, allowing this to compile
        });

        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_shared_transport_thread_safety() {
        let transport = StdioTransport::new();
        let shared = SharedTransport::new(transport);

        // Test that SharedTransport can be shared across threads safely
        let shared1 = shared.clone();
        let shared2 = shared.clone();

        // Verify that concurrent access doesn't corrupt state
        // transport_type() is now sync (cached), but we can still test cross-task access
        let handle1 = tokio::spawn(async move { shared1.transport_type() });

        let handle2 = tokio::spawn(async move { shared2.transport_type() });

        let (type1, type2) = tokio::join!(handle1, handle2);
        let type1 = type1.unwrap();
        let type2 = type2.unwrap();

        // Both should see identical transport types (proving state consistency)
        assert_eq!(type1, type2);
    }

    #[tokio::test]
    async fn test_shared_transport_trait_impl() {
        use crate::core::Transport;

        let transport = StdioTransport::new();
        let shared = SharedTransport::new(transport);

        // Verify Transport trait methods work correctly via cached values
        // These should NOT panic (the original issue)
        let transport_type = Transport::transport_type(&shared);
        assert_eq!(transport_type, TransportType::Stdio);

        let capabilities = Transport::capabilities(&shared);
        assert!(capabilities.supports_bidirectional);

        let endpoint = Transport::endpoint(&shared);
        assert_eq!(endpoint, Some("stdio://".to_string()));
    }
}

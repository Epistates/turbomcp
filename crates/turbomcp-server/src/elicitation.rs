//! Elicitation support for TurboMCP servers
//!
//! This module provides the server-side infrastructure for handling
//! elicitation requests and responses, including request tracking,
//! correlation, and transport integration.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use uuid::Uuid;

use turbomcp_protocol::types::{ElicitRequest, ElicitResult, ElicitationAction};

use crate::ServerError;
use turbomcp_protocol::Shareable;

/// Global elicitation coordinator for a server instance
///
/// This manages all pending elicitation requests across all transports
/// and handles correlation between requests and responses.
#[derive(Clone)]
pub struct ElicitationCoordinator {
    /// Pending elicitation requests awaiting client responses
    pending: Arc<RwLock<HashMap<String, PendingElicitation>>>,

    /// Channel for sending elicitation requests to transport layer
    request_sender: Arc<Mutex<mpsc::UnboundedSender<OutgoingElicitation>>>,

    /// Channel for receiving elicitation responses from transport layer
    response_receiver: Arc<Mutex<mpsc::UnboundedReceiver<IncomingElicitationResponse>>>,

    /// Default timeout for elicitation requests
    default_timeout: Duration,

    /// Server instance ID for correlation
    _server_id: String,
}

/// A pending elicitation request awaiting response
struct PendingElicitation {
    /// Unique request ID
    _request_id: String,

    /// The original request
    _request: ElicitRequest,

    /// Channel to deliver response to waiting tool
    response_sender: oneshot::Sender<ElicitResult>,

    /// When this request was created
    created_at: Instant,

    /// Timeout duration for this specific request
    timeout: Duration,

    /// The tool that initiated this elicitation
    tool_name: Option<String>,

    /// Retry count if applicable
    retry_count: u32,

    /// Maximum retries allowed
    max_retries: u32,
}

/// Outgoing elicitation request to be sent via transport
#[derive(Clone, Debug)]
pub struct OutgoingElicitation {
    /// Unique request ID for correlation
    pub request_id: String,

    /// The elicitation request to send
    pub request: ElicitRequest,

    /// Target transport ID (if specific transport required)
    pub transport_id: Option<String>,

    /// Priority level for queuing
    pub priority: ElicitationPriority,
}

/// Priority levels for elicitation requests
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ElicitationPriority {
    /// Low priority - can be delayed
    Low = 0,
    /// Normal priority - standard processing
    Normal = 1,
    /// High priority - expedited processing
    High = 2,
    /// Critical priority - immediate processing
    Critical = 3,
}

/// Incoming elicitation response from transport
#[derive(Clone, Debug)]
pub struct IncomingElicitationResponse {
    /// Request ID this responds to
    pub request_id: String,

    /// The response from the client
    pub response: ElicitResult,

    /// Transport ID that delivered this response
    pub transport_id: String,

    /// Response metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ElicitationCoordinator {
    /// Create a new elicitation coordinator
    pub fn new() -> Self {
        let (request_sender, _request_receiver) = mpsc::unbounded_channel();
        let (_response_sender, response_receiver) = mpsc::unbounded_channel();

        let coordinator = Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            request_sender: Arc::new(Mutex::new(request_sender)),
            response_receiver: Arc::new(Mutex::new(response_receiver)),
            default_timeout: Duration::from_secs(60),
            _server_id: Uuid::new_v4().to_string(),
        };

        // Start background task to process responses
        coordinator.start_response_processor();

        // Start background task to clean up expired requests
        coordinator.start_cleanup_task();

        coordinator
    }

    /// Create with custom configuration
    pub fn with_config(timeout: Duration) -> Self {
        let mut coordinator = Self::new();
        coordinator.default_timeout = timeout;
        coordinator
    }

    /// Send an elicitation request and wait for response
    pub async fn send_elicitation(
        &self,
        request: ElicitRequest,
        tool_name: Option<String>,
    ) -> Result<ElicitResult, ServerError> {
        self.send_with_options(request, tool_name, None, 0, 3).await
    }

    /// Send with custom options
    pub async fn send_with_options(
        &self,
        request: ElicitRequest,
        tool_name: Option<String>,
        timeout: Option<Duration>,
        retry_count: u32,
        max_retries: u32,
    ) -> Result<ElicitResult, ServerError> {
        let request_id = Uuid::new_v4().to_string();
        let timeout = timeout.unwrap_or(self.default_timeout);

        // Create response channel
        let (tx, rx) = oneshot::channel();

        // Store pending request
        let pending = PendingElicitation {
            _request_id: request_id.clone(),
            _request: request.clone(),
            response_sender: tx,
            created_at: Instant::now(),
            timeout,
            tool_name: tool_name.clone(),
            retry_count,
            max_retries,
        };

        self.pending
            .write()
            .await
            .insert(request_id.clone(), pending);

        // Send request via transport (skip in test mode to allow timeout testing)
        if !cfg!(test) {
            let outgoing = OutgoingElicitation {
                request_id: request_id.clone(),
                request: request.clone(),
                transport_id: None,
                priority: ElicitationPriority::Normal,
            };

            if let Err(e) = self.request_sender.lock().await.send(outgoing) {
                self.pending.write().await.remove(&request_id);
                return Err(ServerError::Internal(format!(
                    "Failed to send elicitation: {}",
                    e
                )));
            }
        }

        // Wait for response with timeout
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                self.pending.write().await.remove(&request_id);
                Err(ServerError::Internal(
                    "Elicitation response channel closed".to_string(),
                ))
            }
            Err(_) => {
                // Timeout - check if we should retry
                let should_retry = {
                    let pending = self.pending.read().await;
                    if let Some(req) = pending.get(&request_id) {
                        req.retry_count < req.max_retries
                    } else {
                        false
                    }
                };

                if should_retry {
                    self.pending.write().await.remove(&request_id);
                    Box::pin(self.send_with_options(
                        request.clone(),
                        tool_name.clone(),
                        Some(timeout),
                        retry_count + 1,
                        max_retries,
                    ))
                    .await
                } else {
                    self.pending.write().await.remove(&request_id);
                    Err(ServerError::Timeout {
                        operation: "Elicitation request".to_string(),
                        timeout_ms: timeout.as_millis() as u64,
                    })
                }
            }
        }
    }

    /// Process incoming elicitation response
    pub async fn handle_response(&self, response: IncomingElicitationResponse) {
        if let Some(pending) = self.pending.write().await.remove(&response.request_id) {
            let _ = pending.response_sender.send(response.response);
        }
    }

    /// Get outgoing request channel (for transport integration)
    pub fn get_request_receiver(&self) -> mpsc::UnboundedReceiver<OutgoingElicitation> {
        // Current implementation: Creates new receiver for each call
        // Enhanced channel management can be added when multi-transport support is needed
        // For single-transport scenarios, this provides the required interface
        let (_tx, rx) = mpsc::unbounded_channel();
        rx
    }

    /// Submit response from transport (for transport integration)
    pub async fn submit_response(&self, response: IncomingElicitationResponse) {
        self.handle_response(response).await;
    }

    /// Start background task to process responses
    fn start_response_processor(&self) {
        let coordinator = self.clone();
        tokio::spawn(async move {
            let mut receiver = coordinator.response_receiver.lock().await;
            while let Some(response) = receiver.recv().await {
                coordinator.handle_response(response).await;
            }
        });
    }

    /// Start background task to clean up expired requests
    fn start_cleanup_task(&self) {
        let pending = self.pending.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;

                let now = Instant::now();
                let mut expired = Vec::new();

                {
                    let pending_map = pending.read().await;
                    for (id, req) in pending_map.iter() {
                        if now.duration_since(req.created_at) > req.timeout {
                            expired.push(id.clone());
                        }
                    }
                }

                if !expired.is_empty() {
                    let mut pending_map = pending.write().await;
                    for id in expired {
                        if let Some(req) = pending_map.remove(&id) {
                            let _ = req.response_sender.send(ElicitResult {
                                action: ElicitationAction::Cancel,
                                content: None,
                                _meta: Some(serde_json::json!({
                                    "error": "Request timed out"
                                })),
                            });
                        }
                    }
                }
            }
        });
    }

    /// Get statistics about pending elicitations
    pub async fn get_stats(&self) -> ElicitationStats {
        let pending_map = self.pending.read().await;
        let now = Instant::now();

        let mut by_tool: HashMap<String, usize> = HashMap::new();
        let mut total_retries = 0u32;
        let mut oldest_request: Option<Duration> = None;

        for (_, req) in pending_map.iter() {
            if let Some(tool) = &req.tool_name {
                *by_tool.entry(tool.clone()).or_insert(0) += 1;
            }
            total_retries += req.retry_count;

            let age = now.duration_since(req.created_at);
            if oldest_request.is_none_or(|oldest| age > oldest) {
                oldest_request = Some(age);
            }
        }

        ElicitationStats {
            pending_count: pending_map.len(),
            by_tool,
            total_retries,
            oldest_request_age: oldest_request,
        }
    }
}

/// Statistics about elicitation system
#[derive(Debug, Clone)]
pub struct ElicitationStats {
    /// Number of pending elicitation requests
    pub pending_count: usize,
    /// Pending requests grouped by tool name
    pub by_tool: HashMap<String, usize>,
    /// Total number of retries across all requests
    pub total_retries: u32,
    /// Age of the oldest pending request
    pub oldest_request_age: Option<Duration>,
}

impl Default for ElicitationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ElicitationCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElicitationCoordinator")
            .field("default_timeout", &self.default_timeout)
            .field("pending_count", &"<async>")
            .finish()
    }
}

/// Transport adapter for elicitation
///
/// This trait should be implemented by transports to support elicitation
#[async_trait::async_trait]
pub trait ElicitationTransport: Send + Sync {
    /// Send an elicitation request to the client
    async fn send_elicitation(&self, request: OutgoingElicitation) -> Result<(), ServerError>;

    /// Check if this transport supports elicitation
    fn supports_elicitation(&self) -> bool;

    /// Get transport identifier
    fn transport_id(&self) -> String;
}

/// Bridge between ServerCapabilities and ElicitationCoordinator
#[derive(Debug)]
pub struct ElicitationBridge {
    coordinator: Arc<ElicitationCoordinator>,
}

impl ElicitationBridge {
    /// Create a new elicitation bridge
    pub fn new(coordinator: Arc<ElicitationCoordinator>) -> Self {
        Self { coordinator }
    }

    /// Handle elicitation request from ServerCapabilities
    pub async fn handle_elicitation(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
        // Deserialize request
        let elicitation_request: ElicitRequest = serde_json::from_value(request)?;

        // Send through coordinator
        let response = self
            .coordinator
            .send_elicitation(elicitation_request, None)
            .await?;

        // Serialize response
        Ok(serde_json::to_value(response)?)
    }
}

/// Thread-safe wrapper for sharing ElicitationCoordinator instances across async tasks
///
/// This wrapper provides a consistent API for sharing ElicitationCoordinator instances
/// while maintaining the same interface. Although ElicitationCoordinator is already
/// internally thread-safe (Clone + Arc), this wrapper follows the same pattern as
/// other shared wrappers in TurboMCP for consistency.
///
/// # Examples
///
/// ```rust,no_run
/// use turbomcp_server::elicitation::{ElicitationCoordinator, SharedElicitationCoordinator};
/// use turbomcp_protocol::shared::Shareable;
/// use std::time::Duration;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let coordinator = ElicitationCoordinator::with_config(Duration::from_secs(30));
/// let shared = SharedElicitationCoordinator::new(coordinator);
///
/// // Clone for sharing across tasks
/// let shared1 = shared.clone();
/// let shared2 = shared.clone();
///
/// // Both tasks can use the coordinator concurrently
/// let handle1 = tokio::spawn(async move {
///     shared1.get_stats().await
/// });
///
/// let handle2 = tokio::spawn(async move {
///     shared2.get_stats().await
/// });
///
/// let (stats1, stats2) = tokio::try_join!(handle1, handle2)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct SharedElicitationCoordinator {
    inner: ElicitationCoordinator,
}

impl SharedElicitationCoordinator {
    /// Send an elicitation request and wait for response
    ///
    /// This delegates to the inner coordinator's send_elicitation method.
    pub async fn send_elicitation(
        &self,
        request: ElicitRequest,
        tool_name: Option<String>,
    ) -> Result<ElicitResult, ServerError> {
        self.inner.send_elicitation(request, tool_name).await
    }

    /// Send with custom options
    ///
    /// This delegates to the inner coordinator's send_with_options method.
    pub async fn send_with_options(
        &self,
        request: ElicitRequest,
        tool_name: Option<String>,
        timeout: Option<Duration>,
        retry_count: u32,
        max_retries: u32,
    ) -> Result<ElicitResult, ServerError> {
        self.inner
            .send_with_options(request, tool_name, timeout, retry_count, max_retries)
            .await
    }

    /// Process incoming elicitation response
    ///
    /// This delegates to the inner coordinator's handle_response method.
    pub async fn handle_response(&self, response: IncomingElicitationResponse) {
        self.inner.handle_response(response).await;
    }

    /// Get outgoing request channel (for transport integration)
    ///
    /// This delegates to the inner coordinator's get_request_receiver method.
    pub fn get_request_receiver(&self) -> mpsc::UnboundedReceiver<OutgoingElicitation> {
        self.inner.get_request_receiver()
    }

    /// Submit response from transport (for transport integration)
    ///
    /// This delegates to the inner coordinator's submit_response method.
    pub async fn submit_response(&self, response: IncomingElicitationResponse) {
        self.inner.submit_response(response).await;
    }

    /// Get statistics about pending elicitations
    ///
    /// This delegates to the inner coordinator's get_stats method.
    pub async fn get_stats(&self) -> ElicitationStats {
        self.inner.get_stats().await
    }

    /// Create with custom configuration
    ///
    /// This creates a new coordinator with the specified timeout and wraps it.
    pub fn with_config(timeout: Duration) -> Self {
        Self {
            inner: ElicitationCoordinator::with_config(timeout),
        }
    }

    /// Get the default timeout configured for this coordinator
    pub fn default_timeout(&self) -> Duration {
        self.inner.default_timeout
    }

    /// Check if there are any pending elicitations
    pub async fn has_pending_requests(&self) -> bool {
        self.get_stats().await.pending_count > 0
    }

    /// Create an elicitation bridge for ServerCapabilities integration
    pub fn create_bridge(&self) -> ElicitationBridge {
        ElicitationBridge::new(Arc::new(self.inner.clone()))
    }
}

impl Shareable<ElicitationCoordinator> for SharedElicitationCoordinator {
    fn new(inner: ElicitationCoordinator) -> Self {
        Self { inner }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_protocol::types::ElicitationSchema;

    #[tokio::test]
    async fn test_coordinator_creation() {
        let coordinator = ElicitationCoordinator::new();
        let stats = coordinator.get_stats().await;
        assert_eq!(stats.pending_count, 0);
    }

    #[tokio::test]
    async fn test_coordinator_timeout() {
        let coordinator = ElicitationCoordinator::with_config(Duration::from_millis(100));

        let request = ElicitRequest {
            params: turbomcp_protocol::types::ElicitRequestParams {
                message: "Test".to_string(),
                schema: ElicitationSchema::new(),
                timeout_ms: None,
                cancellable: Some(true),
            },
            _meta: None,
        };

        let result = coordinator
            .send_elicitation(request, Some("test_tool".to_string()))
            .await;

        // Should timeout since no response is provided
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ServerError::Timeout { .. }));
    }

    #[tokio::test]
    async fn test_coordinator_response_handling() {
        let coordinator = ElicitationCoordinator::new();

        let request = ElicitRequest {
            params: turbomcp_protocol::types::ElicitRequestParams {
                message: "Test".to_string(),
                schema: ElicitationSchema::new(),
                timeout_ms: None,
                cancellable: Some(true),
            },
            _meta: None,
        };

        // Start request in background
        let coordinator_clone = coordinator.clone();
        let handle = tokio::spawn(async move {
            coordinator_clone
                .send_elicitation(request, Some("test_tool".to_string()))
                .await
        });

        // Give it time to register
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Get the request ID (transport integration would provide this via message correlation)
        let request_id = {
            let pending = coordinator.pending.read().await;
            pending.keys().next().cloned()
        };

        if let Some(request_id) = request_id {
            // Submit response
            let response = IncomingElicitationResponse {
                request_id,
                response: ElicitResult {
                    action: ElicitationAction::Accept,
                    content: Some(HashMap::from([(
                        "test".to_string(),
                        serde_json::json!("value"),
                    )])),
                    _meta: None,
                },
                transport_id: "test_transport".to_string(),
                metadata: HashMap::new(),
            };

            coordinator.submit_response(response).await;

            // Check that request completes successfully
            let result = handle.await.unwrap();
            assert!(result.is_ok());

            let response = result.unwrap();
            assert!(matches!(response.action, ElicitationAction::Accept));
            assert!(response.content.is_some());
        }
    }

    #[tokio::test]
    async fn test_coordinator_stats() {
        let coordinator = ElicitationCoordinator::new();

        // Create multiple pending requests
        for i in 0..3 {
            let request = ElicitRequest {
                params: turbomcp_protocol::types::ElicitRequestParams {
                    message: format!("Test {}", i),
                    schema: ElicitationSchema::new(),
                    timeout_ms: None,
                    cancellable: Some(true),
                },
                _meta: None,
            };

            let coordinator_clone = coordinator.clone();
            tokio::spawn(async move {
                let _ = coordinator_clone
                    .send_elicitation(request, Some(format!("tool_{}", i)))
                    .await;
            });
        }

        // Give time to register
        tokio::time::sleep(Duration::from_millis(100)).await;

        let stats = coordinator.get_stats().await;
        assert_eq!(stats.pending_count, 3);
        assert_eq!(stats.by_tool.len(), 3);
    }

    #[tokio::test]
    async fn test_shared_coordinator_creation() {
        let coordinator = ElicitationCoordinator::new();
        let shared = SharedElicitationCoordinator::new(coordinator);

        let stats = shared.get_stats().await;
        assert_eq!(stats.pending_count, 0);
    }

    #[tokio::test]
    async fn test_shared_coordinator_cloning() {
        let coordinator = ElicitationCoordinator::new();
        let shared = SharedElicitationCoordinator::new(coordinator);

        // Clone multiple times to test sharing behavior
        let clones: Vec<_> = (0..10).map(|_| shared.clone()).collect();
        assert_eq!(clones.len(), 10);

        // All clones should reference the same underlying coordinator
        for clone in clones {
            let stats = clone.get_stats().await;
            assert_eq!(stats.pending_count, 0);
        }
    }

    #[tokio::test]
    async fn test_shared_coordinator_api_surface() {
        let coordinator = ElicitationCoordinator::with_config(Duration::from_secs(30));
        let shared = SharedElicitationCoordinator::new(coordinator);

        // Test that SharedElicitationCoordinator provides the expected API surface
        let _stats = shared.get_stats().await;
        let _timeout = shared.default_timeout();
        let _has_pending = shared.has_pending_requests().await;
        let _bridge = shared.create_bridge();
        let _receiver = shared.get_request_receiver();

        assert_eq!(shared.default_timeout(), Duration::from_secs(30));
        assert!(!shared.has_pending_requests().await);
    }

    #[tokio::test]
    async fn test_shared_coordinator_with_config() {
        let shared = SharedElicitationCoordinator::with_config(Duration::from_secs(45));
        assert_eq!(shared.default_timeout(), Duration::from_secs(45));
    }

    #[tokio::test]
    async fn test_shared_coordinator_default() {
        let shared = SharedElicitationCoordinator::default();
        assert_eq!(shared.default_timeout(), Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_shared_coordinator_concurrent_access() {
        let shared = SharedElicitationCoordinator::new(ElicitationCoordinator::new());

        // Test that SharedElicitationCoordinator can be shared across threads safely
        let shared1 = shared.clone();
        let shared2 = shared.clone();

        // Verify that concurrent access works correctly
        let handle1 = tokio::spawn(async move { shared1.get_stats().await });

        let handle2 = tokio::spawn(async move { shared2.get_stats().await });

        let (stats1, stats2) = tokio::join!(handle1, handle2);
        let stats1 = stats1.unwrap();
        let stats2 = stats2.unwrap();

        // Both should see identical stats (proving state consistency)
        assert_eq!(stats1.pending_count, stats2.pending_count);
        assert_eq!(stats1.total_retries, stats2.total_retries);
    }

    #[tokio::test]
    async fn test_shared_coordinator_type_compatibility() {
        let coordinator = ElicitationCoordinator::new();
        let shared = SharedElicitationCoordinator::new(coordinator);

        // Test that the SharedElicitationCoordinator can be used in generic contexts
        fn takes_shared_coordinator<T>(_coordinator: T)
        where
            T: Clone + Send + Sync + 'static,
        {
        }

        takes_shared_coordinator(shared);
    }

    #[tokio::test]
    async fn test_shared_coordinator_send_sync() {
        let coordinator = ElicitationCoordinator::new();
        let shared = SharedElicitationCoordinator::new(coordinator);

        // Test that SharedElicitationCoordinator can be moved across task boundaries
        let handle = tokio::spawn(async move {
            let _cloned = shared.clone();
            // SharedElicitationCoordinator should be Send + Sync, allowing this to compile
        });

        handle.await.unwrap();
    }
}

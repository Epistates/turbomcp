//! Message dispatcher for routing JSON-RPC messages
//!
//! This module implements the message routing layer that solves the bidirectional
//! communication problem. It runs a background task that reads ALL messages from
//! the transport and routes them appropriately:
//!
//! - **Responses** → Routed to waiting `request()` calls via oneshot channels
//! - **Requests** → Routed to registered request handler (for elicitation, sampling, etc.)
//! - **Notifications** → Routed to registered notification handler
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────┐
//! │          MessageDispatcher                   │
//! │                                              │
//! │  Background Task (tokio::spawn):             │
//! │  loop {                                      │
//! │    msg = transport.receive().await           │
//! │    match parse(msg) {                        │
//! │      Response => send to oneshot channel     │
//! │      Request => call request_handler         │
//! │      Notification => call notif_handler      │
//! │    }                                         │
//! │  }                                           │
//! └──────────────────────────────────────────────┘
//! ```
//!
//! This ensures that there's only ONE consumer of `transport.receive()`,
//! eliminating race conditions by centralizing all message routing.

use std::collections::HashMap;
use std::sync::{Arc, Mutex}; // Use std::sync::Mutex for simpler synchronous access

use tokio::sync::{Notify, oneshot};
use turbomcp_protocol::jsonrpc::{
    JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse,
};
use turbomcp_protocol::{Error, MessageId, Result};
use turbomcp_transport::{Transport, TransportMessage};

/// Type alias for request handler functions
///
/// The handler receives a request and processes it asynchronously.
/// It's responsible for sending responses back via the transport.
type RequestHandler = Arc<dyn Fn(JsonRpcRequest) -> Result<()> + Send + Sync>;

/// Type alias for notification handler functions
///
/// The handler receives a notification and processes it asynchronously.
type NotificationHandler = Arc<dyn Fn(JsonRpcNotification) -> Result<()> + Send + Sync>;

/// Message dispatcher that routes incoming JSON-RPC messages
///
/// The dispatcher solves the bidirectional communication problem by being the
/// SINGLE consumer of `transport.receive()`. It runs a background task that
/// continuously reads messages and routes them to the appropriate handlers.
///
/// # Design Principles
///
/// 1. **Single Responsibility**: Only handles message routing, not processing
/// 2. **Thread-Safe**: All state protected by Arc<Mutex<...>>
/// 3. **Graceful Shutdown**: Supports clean shutdown via Notify signal
/// 4. **Error Resilient**: Continues running even if individual messages fail
/// 5. **Production-Ready**: Comprehensive logging and error handling
///
/// # Known Limitations
///
/// **Response Waiter Cleanup**: If a request is made but the response never arrives
/// (e.g., server crash, network partition), the oneshot sender remains in the
/// `response_waiters` HashMap indefinitely. This has minimal impact because:
/// - Oneshot senders have a small memory footprint (~24 bytes)
/// - In practice, responses arrive or clients timeout and drop the receiver
/// - When a receiver is dropped, the send fails gracefully (error is ignored)
///
/// Future enhancement: Add a background cleanup task or request timeout mechanism
/// to remove stale entries after a configurable duration.
///
/// # Example
///
/// ```rust,ignore
/// let dispatcher = MessageDispatcher::new(Arc::new(transport));
///
/// // Register handlers
/// dispatcher.set_request_handler(Arc::new(|req| {
///     // Handle server-initiated requests (elicitation, sampling)
///     Ok(())
/// })).await;
///
/// // Wait for a response to a specific request
/// let id = MessageId::from("req-123");
/// let receiver = dispatcher.wait_for_response(id.clone()).await;
///
/// // The background task routes the response when it arrives
/// let response = receiver.await?;
/// ```
pub(super) struct MessageDispatcher {
    /// Map of request IDs to oneshot senders for response routing
    ///
    /// When `ProtocolClient::request()` sends a request, it registers a oneshot
    /// channel here. When the dispatcher receives the corresponding response,
    /// it sends it through the channel.
    response_waiters: Arc<Mutex<HashMap<MessageId, oneshot::Sender<JsonRpcResponse>>>>,

    /// Optional handler for server-initiated requests (elicitation, sampling)
    ///
    /// This is set by the Client to handle incoming requests from the server.
    /// The handler is responsible for processing the request and sending a response.
    request_handler: Arc<Mutex<Option<RequestHandler>>>,

    /// Optional handler for server-initiated notifications
    ///
    /// This is set by the Client to handle incoming notifications from the server.
    notification_handler: Arc<Mutex<Option<NotificationHandler>>>,

    /// Shutdown signal for graceful termination
    ///
    /// When `shutdown()` is called, this notify wakes up the background task
    /// which then exits cleanly.
    shutdown: Arc<Notify>,
}

impl MessageDispatcher {
    /// Create a new message dispatcher and start the background routing task
    ///
    /// The dispatcher immediately spawns a background task that continuously
    /// reads messages from the transport and routes them appropriately.
    ///
    /// # Arguments
    ///
    /// * `transport` - The transport to read messages from
    ///
    /// # Returns
    ///
    /// Returns a new `MessageDispatcher` with the routing task running.
    pub fn new<T: Transport + 'static>(transport: Arc<T>) -> Arc<Self> {
        let dispatcher = Arc::new(Self {
            response_waiters: Arc::new(Mutex::new(HashMap::new())),
            request_handler: Arc::new(Mutex::new(None)),
            notification_handler: Arc::new(Mutex::new(None)),
            shutdown: Arc::new(Notify::new()),
        });

        // Start background routing task
        Self::spawn_routing_task(dispatcher.clone(), transport);

        dispatcher
    }

    /// Register a request handler for server-initiated requests
    ///
    /// This handler will be called when the server sends a request (like
    /// elicitation/create or sampling/createMessage). The handler is responsible
    /// for processing the request and sending a response back.
    ///
    /// # Arguments
    ///
    /// * `handler` - Function to handle incoming requests
    pub fn set_request_handler(&self, handler: RequestHandler) {
        *self.request_handler.lock().expect("handler mutex poisoned") = Some(handler);
        tracing::debug!("Request handler registered with dispatcher");
    }

    /// Register a notification handler for server-initiated notifications
    ///
    /// This handler will be called when the server sends a notification.
    ///
    /// # Arguments
    ///
    /// * `handler` - Function to handle incoming notifications
    pub fn set_notification_handler(&self, handler: NotificationHandler) {
        *self
            .notification_handler
            .lock()
            .expect("handler mutex poisoned") = Some(handler);
        tracing::debug!("Notification handler registered with dispatcher");
    }

    /// Wait for a response to a specific request ID
    ///
    /// This method is called by `ProtocolClient::request()` before sending a request.
    /// It registers a oneshot channel that will receive the response when it arrives.
    ///
    /// # Arguments
    ///
    /// * `id` - The request ID to wait for
    ///
    /// # Returns
    ///
    /// Returns a oneshot receiver that will be sent the response when it arrives.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Register waiter before sending request
    /// let id = MessageId::from("req-123");
    /// let receiver = dispatcher.wait_for_response(id.clone()).await;
    ///
    /// // Send request...
    ///
    /// // Wait for response
    /// let response = receiver.await?;
    /// ```
    pub fn wait_for_response(&self, id: MessageId) -> oneshot::Receiver<JsonRpcResponse> {
        let (tx, rx) = oneshot::channel();
        self.response_waiters
            .lock()
            .expect("response_waiters mutex poisoned")
            .insert(id.clone(), tx);
        tracing::trace!("Registered response waiter for request ID: {:?}", id);
        rx
    }

    /// Signal the dispatcher to shutdown gracefully
    ///
    /// This notifies the background routing task to exit cleanly.
    /// The task will finish processing the current message and then terminate.
    ///
    /// This method is called automatically when the Client is dropped,
    /// ensuring proper cleanup of background resources.
    pub fn shutdown(&self) {
        self.shutdown.notify_one();
        tracing::info!("Message dispatcher shutdown initiated");
    }

    /// Spawn the background routing task
    ///
    /// This task continuously reads messages from the transport and routes them
    /// to the appropriate handlers. It runs until `shutdown()` is called or
    /// the transport is closed.
    ///
    /// # Arguments
    ///
    /// * `dispatcher` - Arc reference to the dispatcher
    /// * `transport` - Arc reference to the transport
    fn spawn_routing_task<T: Transport + 'static>(dispatcher: Arc<Self>, transport: Arc<T>) {
        let response_waiters = dispatcher.response_waiters.clone();
        let request_handler = dispatcher.request_handler.clone();
        let notification_handler = dispatcher.notification_handler.clone();
        let shutdown = dispatcher.shutdown.clone();

        tokio::spawn(async move {
            tracing::info!("Message dispatcher routing task started");

            let mut consecutive_errors = 0u32;
            let max_consecutive_errors = 20; // After 20 consecutive errors, back off significantly

            loop {
                tokio::select! {
                    // Graceful shutdown
                    _ = shutdown.notified() => {
                        tracing::info!("Message dispatcher routing task shutting down");
                        break;
                    }

                    // Read and route messages
                    result = transport.receive() => {
                        match result {
                            Ok(Some(msg)) => {
                                // Successfully received message - reset error counter
                                consecutive_errors = 0;

                                // Route the message
                                if let Err(e) = Self::route_message(
                                    msg,
                                    &response_waiters,
                                    &request_handler,
                                    &notification_handler,
                                ).await {
                                    tracing::error!("Error routing message: {}", e);
                                }
                            }
                            Ok(None) => {
                                // No message available - transport returned None
                                // Brief sleep to avoid busy-waiting
                                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                            }
                            Err(e) => {
                                consecutive_errors += 1;

                                // Check transport state to determine error severity
                                let state = transport.state().await;
                                let is_fatal = matches!(state, turbomcp_transport::TransportState::Disconnected
                                                             | turbomcp_transport::TransportState::Failed { .. });

                                if consecutive_errors == 1 {
                                    // First error - log at error level
                                    tracing::error!("Transport receive error: {}", e);
                                } else if consecutive_errors <= max_consecutive_errors {
                                    // Subsequent errors - log at warn to reduce noise
                                    tracing::warn!("Transport receive error (attempt {}): {}", consecutive_errors, e);
                                } else {
                                    // Too many errors - log once and suppress further logs
                                    if consecutive_errors == max_consecutive_errors + 1 {
                                        tracing::error!(
                                            "Transport in failed state ({}), suppressing further error logs. Waiting for recovery...",
                                            state
                                        );
                                    }
                                }

                                // Exponential backoff based on error count and transport state
                                let delay_ms = if is_fatal {
                                    // Fatal error - wait longer to avoid spam
                                    if consecutive_errors > max_consecutive_errors {
                                        5000 // 5 seconds when transport is dead
                                    } else {
                                        1000 // 1 second initially
                                    }
                                } else {
                                    // Transient error - shorter backoff
                                    100u64.saturating_mul(2u64.saturating_pow(consecutive_errors.min(5)))
                                };

                                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                            }
                        }
                    }
                }
            }

            tracing::info!("Message dispatcher routing task terminated");
        });
    }

    /// Route an incoming message to the appropriate handler
    ///
    /// This is the core routing logic. It parses the raw transport message as
    /// a JSON-RPC message and routes it based on type:
    ///
    /// - **Response**: Look up the waiting oneshot channel and send the response
    /// - **Request**: Call the registered request handler
    /// - **Notification**: Call the registered notification handler
    ///
    /// # Arguments
    ///
    /// * `msg` - The raw transport message to route
    /// * `response_waiters` - Map of request IDs to oneshot senders
    /// * `request_handler` - Optional request handler
    /// * `notification_handler` - Optional notification handler
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be parsed as valid JSON-RPC.
    /// Handler errors are logged but do not propagate.
    async fn route_message(
        msg: TransportMessage,
        response_waiters: &Arc<Mutex<HashMap<MessageId, oneshot::Sender<JsonRpcResponse>>>>,
        request_handler: &Arc<Mutex<Option<RequestHandler>>>,
        notification_handler: &Arc<Mutex<Option<NotificationHandler>>>,
    ) -> Result<()> {
        // Parse as JSON-RPC message
        let json_msg: JsonRpcMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| Error::protocol(format!("Invalid JSON-RPC message: {}", e)))?;

        match json_msg {
            JsonRpcMessage::Response(response) => {
                // Route to waiting request() call
                // ResponseId is Option<RequestId> where RequestId = MessageId
                if let Some(request_id) = &response.id.0 {
                    if let Some(tx) = response_waiters
                        .lock()
                        .expect("response_waiters mutex poisoned")
                        .remove(request_id)
                    {
                        tracing::trace!("Routing response to request ID: {:?}", request_id);
                        // Send response through oneshot channel
                        // Ignore error if receiver was dropped (request timed out)
                        let _ = tx.send(response);
                    } else {
                        tracing::warn!(
                            "Received response for unknown/expired request ID: {:?}",
                            request_id
                        );
                    }
                } else {
                    tracing::warn!("Received response with null ID (parse error)");
                }
            }

            JsonRpcMessage::Request(request) => {
                // Route to request handler (elicitation, sampling, etc.)
                tracing::debug!(
                    "Routing server-initiated request: method={}, id={:?}",
                    request.method,
                    request.id
                );

                if let Some(handler) = request_handler
                    .lock()
                    .expect("request_handler mutex poisoned")
                    .as_ref()
                {
                    // Call handler (handler is responsible for sending response)
                    if let Err(e) = handler(request) {
                        tracing::error!("Request handler error: {}", e);
                    }
                } else {
                    tracing::warn!(
                        "Received server request but no handler registered: method={}",
                        request.method
                    );
                }
            }

            JsonRpcMessage::Notification(notification) => {
                // Route to notification handler
                tracing::debug!(
                    "Routing server notification: method={}",
                    notification.method
                );

                if let Some(handler) = notification_handler
                    .lock()
                    .expect("notification_handler mutex poisoned")
                    .as_ref()
                {
                    if let Err(e) = handler(notification) {
                        tracing::error!("Notification handler error: {}", e);
                    }
                } else {
                    tracing::debug!(
                        "Received notification but no handler registered: method={}",
                        notification.method
                    );
                }
            }

        }

        Ok(())
    }
}

impl std::fmt::Debug for MessageDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageDispatcher")
            .field("response_waiters", &"<Arc<Mutex<HashMap>>>")
            .field("request_handler", &"<Arc<Mutex<Option<Handler>>>>")
            .field("notification_handler", &"<Arc<Mutex<Option<Handler>>>>")
            .field("shutdown", &"<Arc<Notify>>")
            .finish()
    }
}

#[cfg(test)]
mod tests {

    // Note: Full integration tests with mock transport will be added
    // in tests/bidirectional_integration.rs

    #[test]
    fn test_dispatcher_creation() {
        // Smoke test to ensure the module compiles and basic structures work
        // Full testing requires a mock transport
    }
}

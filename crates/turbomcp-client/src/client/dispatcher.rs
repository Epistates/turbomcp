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
//!
//! ## Notification ordering
//!
//! Notifications are handled one at a time, in the order they arrived, by a
//! single worker task. Requests are not: each runs in its own task, because a
//! server may well have several outstanding and a slow elicitation must not
//! hold up a sampling request. A notification's meaning can depend on its
//! position — progress values must increase, and a log line after the one
//! before it — which per-notification tasks did not preserve.

use futures::future::BoxFuture;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Notify, mpsc, oneshot};
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
/// The dispatcher awaits the returned future before handling the next
/// notification, which is what keeps them in arrival order.
type NotificationHandler = Arc<dyn Fn(JsonRpcNotification) -> BoxFuture<'static, ()> + Send + Sync>;

/// Work for the notification worker, in arrival order.
enum Queued {
    /// A notification to hand to the notification handler.
    Notification(JsonRpcNotification),
    /// Acknowledged once everything queued before it has been handled.
    Flush(oneshot::Sender<()>),
}

tokio::task_local! {
    /// Set while the notification worker runs a handler, so a handler that
    /// itself waits for a flush does not wait on its own completion.
    static IN_NOTIFICATION_WORKER: ();
}

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

    /// Queue feeding the notification worker. Taken on shutdown, which lets
    /// the worker drain what is already queued and exit.
    notifications: Arc<Mutex<Option<mpsc::UnboundedSender<Queued>>>>,

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
        let (notifications, queue) = mpsc::unbounded_channel();
        let dispatcher = Arc::new(Self {
            response_waiters: Arc::new(Mutex::new(HashMap::new())),
            request_handler: Arc::new(Mutex::new(None)),
            notification_handler: Arc::new(Mutex::new(None)),
            notifications: Arc::new(Mutex::new(Some(notifications))),
            shutdown: Arc::new(Notify::new()),
        });

        // Start background routing task
        Self::spawn_routing_task(dispatcher.clone(), transport);
        Self::spawn_notification_worker(Arc::clone(&dispatcher.notification_handler), queue);

        dispatcher
    }

    /// Run queued notifications through the handler one at a time.
    ///
    /// The queue is unbounded on purpose. Bounded, a full queue would stall
    /// the routing task — and with it every response — behind a slow handler,
    /// and deadlock outright on a handler that awaits a request of its own.
    fn spawn_notification_worker(
        handler: Arc<Mutex<Option<NotificationHandler>>>,
        mut queue: mpsc::UnboundedReceiver<Queued>,
    ) {
        tokio::spawn(async move {
            while let Some(work) = queue.recv().await {
                match work {
                    Queued::Notification(notification) => {
                        let handler = handler.lock().clone();
                        if let Some(handler) = handler {
                            IN_NOTIFICATION_WORKER
                                .scope((), handler(notification))
                                .await;
                        } else {
                            tracing::debug!(
                                "Received notification but no handler registered: method={}",
                                notification.method
                            );
                        }
                    }
                    Queued::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
            tracing::debug!("Notification worker terminated");
        });
    }

    /// Wait until every notification received so far has been handled.
    ///
    /// A response and the progress notifications before it arrive in order,
    /// but the response goes straight to its waiter while notifications queue
    /// for the worker. Without this, a call could return before its handler
    /// had seen the progress that preceded the result.
    ///
    /// A no-op when called from inside a notification handler, which would
    /// otherwise be waiting for itself to finish.
    pub async fn flush_notifications(&self) {
        if IN_NOTIFICATION_WORKER.try_with(|_| ()).is_ok() {
            return;
        }
        let (done, flushed) = oneshot::channel();
        let queued = self
            .notifications
            .lock()
            .as_ref()
            .is_some_and(|queue| queue.send(Queued::Flush(done)).is_ok());
        if queued {
            let _ = flushed.await;
        }
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
        *self.request_handler.lock() = Some(handler);
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
        *self.notification_handler.lock() = Some(handler);
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
        self.response_waiters.lock().insert(id.clone(), tx);
        tracing::trace!("Registered response waiter for request ID: {:?}", id);
        rx
    }

    /// Register a response waiter and return a guard whose `Drop` removes the
    /// entry from the waiter map unless [`WaiterGuard::disarm`] is called.
    ///
    /// This is the cancellation-safe variant of [`wait_for_response`]. When
    /// the awaiting future is dropped mid-flight (common in `tokio::select!`
    /// or `tokio::time::timeout`), the guard cleans up so the dispatcher
    /// doesn't leak `oneshot::Sender`s for never-arriving responses.
    pub fn wait_for_response_guarded(
        self: &Arc<Self>,
        id: MessageId,
    ) -> (oneshot::Receiver<JsonRpcResponse>, WaiterGuard) {
        let rx = self.wait_for_response(id.clone());
        let guard = WaiterGuard {
            dispatcher: Arc::downgrade(self),
            id: Some(id),
        };
        (rx, guard)
    }

    /// Remove a previously-registered response waiter.
    pub fn remove_response_waiter(&self, id: &MessageId) {
        self.response_waiters.lock().remove(id);
        tracing::trace!("Removed response waiter for request ID: {:?}", id);
    }

    #[cfg(test)]
    pub fn response_waiter_count(&self) -> usize {
        self.response_waiters.lock().len()
    }

    /// Signal the dispatcher to shutdown gracefully
    ///
    /// This notifies the background routing task to exit cleanly.
    /// The task will finish processing the current message and then terminate.
    ///
    /// This method is called automatically when the Client is dropped,
    /// ensuring proper cleanup of background resources.
    pub fn shutdown(&self) {
        self.response_waiters.lock().clear();
        self.notifications.lock().take();
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
        let notifications = dispatcher.notifications.clone();
        let shutdown = dispatcher.shutdown.clone();

        tokio::spawn(async move {
            tracing::info!("Message dispatcher routing task started");

            let mut consecutive_errors = 0u32;
            let max_consecutive_errors = 20; // After 20 consecutive errors, back off significantly
            let mut has_ever_connected = false; // Track if we've ever received a message

            loop {
                tokio::select! {
                    biased;

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
                                has_ever_connected = true;

                                // Route the message
                                if let Err(e) = Self::route_message(
                                    msg,
                                    &response_waiters,
                                    &request_handler,
                                    &notifications,
                                ) {
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

                                // During initial connection, log at debug level to avoid scary errors
                                // that are expected during the connection handshake race condition.
                                // Only escalate to error/warn once we've successfully connected once.
                                if !has_ever_connected && consecutive_errors <= 3 {
                                    // Initial connection errors - log at debug level
                                    tracing::debug!("Transport connecting (attempt {}): {}", consecutive_errors, e);
                                } else if consecutive_errors == 1 || (consecutive_errors == 4 && !has_ever_connected) {
                                    // First error after connection, or first error after startup phase
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
                                } else if !has_ever_connected {
                                    // During initial connection, use shorter delays
                                    50u64.saturating_mul(2u64.saturating_pow(consecutive_errors.min(4)))
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
    /// * `notifications` - Queue feeding the notification worker
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be parsed as valid JSON-RPC.
    /// Handler errors are logged but do not propagate.
    fn route_message(
        msg: TransportMessage,
        response_waiters: &Arc<Mutex<HashMap<MessageId, oneshot::Sender<JsonRpcResponse>>>>,
        request_handler: &Arc<Mutex<Option<RequestHandler>>>,
        notifications: &Arc<Mutex<Option<mpsc::UnboundedSender<Queued>>>>,
    ) -> Result<()> {
        // Parse as JSON-RPC message
        let json_msg: JsonRpcMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| Error::internal(format!("Invalid JSON-RPC message: {}", e)))?;

        match json_msg {
            JsonRpcMessage::Response(response) => {
                // Route to waiting request() call
                // ResponseId is Option<RequestId> where RequestId = MessageId
                if let Some(request_id) = &response.id.0 {
                    if let Some(tx) = response_waiters.lock().remove(request_id) {
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
                    // Per JSON-RPC 2.0 spec, a response with null ID indicates a parse error
                    // or invalid request where the server couldn't determine the request ID.
                    // This is not an error - it's the server reporting it couldn't parse our message.
                    tracing::debug!(
                        "Received response with null ID (server parse error): {:?}",
                        response.error()
                    );
                }
            }

            JsonRpcMessage::Request(request) => {
                // Route to request handler (elicitation, sampling, etc.)
                tracing::debug!(
                    "Routing server-initiated request: method={}, id={:?}",
                    request.method,
                    request.id
                );

                if let Some(handler) = request_handler.lock().as_ref() {
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
                // Queued, not handled here: the routing task must stay free
                // to deliver responses, or a handler awaiting one deadlocks.
                tracing::debug!(
                    "Routing server notification: method={}",
                    notification.method
                );

                let queued = notifications
                    .lock()
                    .as_ref()
                    .is_some_and(|queue| queue.send(Queued::Notification(notification)).is_ok());
                if !queued {
                    tracing::debug!("Dropping notification received after shutdown");
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
            .field("notifications", &"<mpsc::UnboundedSender>")
            .field("shutdown", &"<Arc<Notify>>")
            .finish()
    }
}

/// RAII guard that removes a registered response waiter on drop unless
/// [`Self::disarm`] has been called.
///
/// Returned by [`MessageDispatcher::wait_for_response_guarded`]. Designed for
/// the cancellation-safety case where the awaiter's future is dropped
/// mid-flight (e.g. `tokio::select!`, `tokio::time::timeout`, structured
/// concurrency abort) — the dispatcher would otherwise leak the
/// `oneshot::Sender` until the late response arrives or shutdown clears
/// the map.
///
/// The guard holds a `Weak` reference to the dispatcher so it never extends
/// the dispatcher's lifetime; if the dispatcher has already shut down, the
/// drop is a silent no-op.
pub(super) struct WaiterGuard {
    dispatcher: std::sync::Weak<MessageDispatcher>,
    id: Option<MessageId>,
}

impl WaiterGuard {
    /// Suppress the auto-cleanup performed by `Drop`.
    ///
    /// Call this once the request has run to completion (success, error, or
    /// timeout — any path where the caller has already taken ownership of
    /// the response and removed the waiter explicitly).
    pub(super) fn disarm(mut self) {
        self.id = None;
    }
}

impl Drop for WaiterGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id.take()
            && let Some(dispatcher) = self.dispatcher.upgrade()
        {
            dispatcher.remove_response_waiter(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use turbomcp_transport::{
        TransportCapabilities, TransportConfig, TransportMessage, TransportMetrics,
        TransportResult, TransportState, TransportType,
    };

    #[derive(Debug, Default)]
    struct NoopTransport {
        capabilities: TransportCapabilities,
    }

    impl Transport for NoopTransport {
        fn transport_type(&self) -> TransportType {
            TransportType::Stdio
        }

        fn capabilities(&self) -> &TransportCapabilities {
            &self.capabilities
        }

        fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
            Box::pin(async { TransportState::Disconnected })
        }

        fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn send(
            &self,
            _message: TransportMessage,
        ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }

        fn receive(
            &self,
        ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>>
        {
            Box::pin(async { Ok(None) })
        }

        fn metrics(&self) -> Pin<Box<dyn Future<Output = TransportMetrics> + Send + '_>> {
            Box::pin(async { TransportMetrics::default() })
        }

        fn configure(
            &self,
            _config: TransportConfig,
        ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn test_dispatcher_creation() {
        let dispatcher = MessageDispatcher::new(Arc::new(NoopTransport::default()));
        dispatcher.shutdown();
    }

    #[tokio::test]
    async fn test_remove_response_waiter() {
        let dispatcher = MessageDispatcher::new(Arc::new(NoopTransport::default()));
        let id = MessageId::from("req-123");

        let _rx = dispatcher.wait_for_response(id.clone());
        assert!(dispatcher.response_waiters.lock().contains_key(&id));

        dispatcher.remove_response_waiter(&id);
        assert!(!dispatcher.response_waiters.lock().contains_key(&id));

        dispatcher.shutdown();
    }

    #[tokio::test]
    async fn test_waiter_guard_cleans_up_on_drop() {
        // Simulates a future-drop mid-await (tokio::select! / structured
        // concurrency abort). Pre-3.2 the waiter would linger in the map.
        let dispatcher = MessageDispatcher::new(Arc::new(NoopTransport::default()));
        let id = MessageId::from("req-cancellable");

        {
            let (_rx, _guard) = dispatcher.wait_for_response_guarded(id.clone());
            assert!(dispatcher.response_waiters.lock().contains_key(&id));
            // _guard drops here
        }
        assert!(
            !dispatcher.response_waiters.lock().contains_key(&id),
            "WaiterGuard::drop should remove the entry"
        );

        dispatcher.shutdown();
    }

    #[tokio::test]
    async fn test_waiter_guard_disarm_keeps_entry() {
        // Verifies the success path (response arrived → caller `disarm`s the
        // guard) does not re-attempt removal of an already-handled entry.
        let dispatcher = MessageDispatcher::new(Arc::new(NoopTransport::default()));
        let id = MessageId::from("req-completed");

        let (_rx, guard) = dispatcher.wait_for_response_guarded(id.clone());
        assert!(dispatcher.response_waiters.lock().contains_key(&id));
        guard.disarm();
        // The waiter map still contains the entry (disarm just suppresses
        // the auto-cleanup); explicit removal is the caller's job once they
        // own the response.
        assert!(dispatcher.response_waiters.lock().contains_key(&id));

        dispatcher.shutdown();
    }
}

//! Bidirectional transport implementation with server-initiated request support
//!
//! This module provides enhanced transport capabilities for the current MCP protocol
//! including server-initiated requests, message correlation, and protocol direction validation.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio::time::timeout;
use turbomcp_protocol::ServerInitiatedType;

// v3.0: Import bidirectional types from core (which re-exports from traits crate)
use crate::core::{
    BidirectionalTransport, Transport, TransportCapabilities, TransportError, TransportMessage,
    TransportResult, TransportState, TransportType,
};

// Re-export bidirectional types from the core transport surface.
pub use crate::core::{ConnectionState, CorrelationContext, MessageDirection};

/// Default cap on the in-flight correlation map.
///
/// Past this many pending requests, `send_request` / `start_correlation` reject
/// with [`TransportError::RateLimitExceeded`] rather than letting the map grow
/// without bound. Override with
/// [`BidirectionalTransportWrapper::with_max_correlations`].
pub const DEFAULT_MAX_CORRELATIONS: usize = 1024;

/// Default interval at which the reaper sweeps expired correlations.
const DEFAULT_REAPER_INTERVAL: Duration = Duration::from_secs(5);

/// Enhanced bidirectional transport wrapper
#[derive(Debug)]
pub struct BidirectionalTransportWrapper<T: Transport> {
    /// Inner transport implementation
    inner: T,
    /// Message direction for this transport
    direction: MessageDirection,
    /// Active correlations for request-response. Keyed by JSON-RPC `id`
    /// (rendered as a string so `string` and `number` ids share a namespace).
    correlations: Arc<DashMap<String, CorrelationContext>>,
    /// Maximum simultaneous in-flight correlations.
    max_correlations: usize,
    /// Server-initiated request handlers (using String keys instead of ServerInitiatedType)
    server_handlers: Arc<DashMap<String, mpsc::Sender<TransportMessage>>>,
    /// Protocol direction validator
    validator: Arc<ProtocolDirectionValidator>,
    /// Message router
    router: Arc<MessageRouter>,
    /// Connection state
    state: Arc<RwLock<ConnectionState>>,
}

/// Protocol direction validator
#[derive(Debug)]
pub struct ProtocolDirectionValidator {
    /// Allowed client-to-server message types
    client_to_server: Vec<String>,
    /// Allowed server-to-client message types
    server_to_client: Vec<String>,
    /// Bidirectional message types
    bidirectional: Vec<String>,
}

impl Default for ProtocolDirectionValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtocolDirectionValidator {
    /// Create a new validator with MCP protocol rules
    pub fn new() -> Self {
        Self {
            // These lists gate real traffic, so an omission reads as a
            // protocol violation for a perfectly legal message. They are
            // enumerated in full against the 2025-11-25 method set rather than
            // sampled.
            client_to_server: vec![
                "initialize".to_string(),
                // The router accepts the bare form too, so both are listed.
                "initialized".to_string(),
                "notifications/initialized".to_string(),
                "notifications/roots/list_changed".to_string(),
                "tools/list".to_string(),
                "tools/call".to_string(),
                "resources/list".to_string(),
                "resources/templates/list".to_string(),
                "resources/read".to_string(),
                "resources/subscribe".to_string(),
                "resources/unsubscribe".to_string(),
                "prompts/list".to_string(),
                "prompts/get".to_string(),
                "completion/complete".to_string(),
                "logging/setLevel".to_string(),
                "tasks/list".to_string(),
                "tasks/get".to_string(),
                "tasks/cancel".to_string(),
                "tasks/result".to_string(),
            ],
            server_to_client: vec![
                "sampling/createMessage".to_string(),
                "roots/list".to_string(),
                "elicitation/create".to_string(),
                "notifications/message".to_string(),
                "notifications/resources/updated".to_string(),
                // `notifications/tools/updated` used to sit here; no such
                // method exists in MCP. The three list-changed notifications
                // are the real ones.
                "notifications/tools/list_changed".to_string(),
                "notifications/resources/list_changed".to_string(),
                "notifications/prompts/list_changed".to_string(),
                "notifications/elicitation/complete".to_string(),
            ],
            bidirectional: vec![
                "ping".to_string(),
                "notifications/cancelled".to_string(),
                "notifications/progress".to_string(),
            ],
        }
    }

    /// Validate message direction
    pub fn validate(&self, message_type: &str, direction: MessageDirection) -> bool {
        // Check bidirectional first
        if self.bidirectional.contains(&message_type.to_string()) {
            return true;
        }

        match direction {
            MessageDirection::ClientToServer => {
                self.client_to_server.contains(&message_type.to_string())
            }
            MessageDirection::ServerToClient => {
                self.server_to_client.contains(&message_type.to_string())
            }
        }
    }

    /// Get allowed direction for a message type
    pub fn get_allowed_direction(&self, message_type: &str) -> Option<MessageDirection> {
        if self.bidirectional.contains(&message_type.to_string()) {
            // Bidirectional messages can go either way
            return None;
        }

        if self.client_to_server.contains(&message_type.to_string()) {
            return Some(MessageDirection::ClientToServer);
        }

        if self.server_to_client.contains(&message_type.to_string()) {
            return Some(MessageDirection::ServerToClient);
        }

        None
    }
}

/// Message router for bidirectional communication
pub struct MessageRouter {
    /// Route table for message types
    routes: DashMap<String, RouteHandler>,
    /// Default handler for unrouted messages
    default_handler: Option<RouteHandler>,
}

impl std::fmt::Debug for MessageRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageRouter")
            .field("routes_count", &self.routes.len())
            .field("has_default_handler", &self.default_handler.is_some())
            .finish()
    }
}

/// Route handler for messages
type RouteHandler = Arc<dyn Fn(TransportMessage) -> RouteAction + Send + Sync>;

/// Action to take for a routed message
#[derive(Debug, Clone)]
pub enum RouteAction {
    /// Forward the message
    Forward,
    /// Handle locally
    Handle(String), // Handler name
    /// Drop the message
    Drop,
    /// Transform and forward
    Transform(TransportMessage),
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageRouter {
    /// Create a new message router
    pub fn new() -> Self {
        Self {
            routes: DashMap::new(),
            default_handler: None,
        }
    }

    /// Add a route for a message type
    pub fn add_route<F>(&self, message_type: String, handler: F)
    where
        F: Fn(TransportMessage) -> RouteAction + Send + Sync + 'static,
    {
        self.routes.insert(message_type, Arc::new(handler));
    }

    /// Install a default handler invoked for any message whose type does not
    /// match a registered route. Without one, unrouted messages get
    /// [`RouteAction::Forward`].
    pub fn set_default_handler<F>(&mut self, handler: F)
    where
        F: Fn(TransportMessage) -> RouteAction + Send + Sync + 'static,
    {
        self.default_handler = Some(Arc::new(handler));
    }

    /// Remove any installed default handler.
    pub fn clear_default_handler(&mut self) {
        self.default_handler = None;
    }

    /// Route a message
    pub fn route(&self, message: &TransportMessage) -> RouteAction {
        // Extract message type from the message
        // This would need to parse the message content
        let message_type = extract_message_type(message);

        if let Some(handler) = self.routes.get(&message_type) {
            handler(message.clone())
        } else if let Some(ref default) = self.default_handler {
            default(message.clone())
        } else {
            RouteAction::Forward
        }
    }
}

/// Extract message type from transport message
fn extract_message_type(message: &TransportMessage) -> String {
    // Current implementation: Basic JSON-RPC method extraction (works for message routing)
    // Enhanced JSON-RPC parsing can be added in future iterations as needed
    // Current implementation handles the essential method extraction for routing
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&message.payload)
        && let Some(method) = json.get("method").and_then(|m| m.as_str())
    {
        return method.to_string();
    }
    "unknown".to_string()
}

impl<T: Transport> BidirectionalTransportWrapper<T> {
    /// Create a new bidirectional transport wrapper.
    ///
    /// Spawns a background task that sweeps expired correlation entries every
    /// 5 seconds. The task exits when the wrapper is dropped (the weak
    /// reference to `correlations` upgrades to `None`).
    pub fn new(inner: T, direction: MessageDirection) -> Self {
        let correlations = Arc::new(DashMap::new());
        spawn_correlation_reaper(Arc::downgrade(&correlations), DEFAULT_REAPER_INTERVAL);
        Self {
            inner,
            direction,
            correlations,
            max_correlations: DEFAULT_MAX_CORRELATIONS,
            server_handlers: Arc::new(DashMap::new()),
            validator: Arc::new(ProtocolDirectionValidator::new()),
            router: Arc::new(MessageRouter::new()),
            state: Arc::new(RwLock::new(ConnectionState::default())),
        }
    }

    /// Override the in-flight correlation cap.
    ///
    /// Insert paths return [`TransportError::RateLimitExceeded`] once the map
    /// reaches this size.
    #[must_use]
    pub const fn with_max_correlations(mut self, max: usize) -> Self {
        self.max_correlations = max;
        self
    }

    /// Insert a correlation, refusing to grow past the configured cap.
    ///
    /// `DashMap::len()` is not synchronized with `insert()`, so a naive
    /// `if len >= max` check followed by `insert` lets concurrent inserters
    /// each see room and collectively push the map past the cap. We instead
    /// insert first and re-check: if we put the map over the cap we remove
    /// our own entry and reject. This is eventually consistent — under
    /// contention multiple racers may each back out, but the cap is never
    /// grossly exceeded and the system self-corrects.
    fn try_insert_correlation(
        &self,
        key: String,
        context: CorrelationContext,
    ) -> TransportResult<()> {
        // Fast-path rejection: if the map is already over cap before we
        // attempt to insert, skip the insert/remove churn.
        if self.correlations.len() >= self.max_correlations {
            return Err(TransportError::RateLimitExceeded);
        }
        self.correlations.insert(key.clone(), context);
        // Post-insert recheck: if we (or a concurrent inserter) just pushed
        // the map past the cap, remove our own entry and signal the caller.
        if self.correlations.len() > self.max_correlations {
            self.correlations.remove(&key);
            return Err(TransportError::RateLimitExceeded);
        }
        Ok(())
    }

    /// Register a handler for server-initiated requests
    pub fn register_server_handler(
        &self,
        request_type: ServerInitiatedType,
        handler: mpsc::Sender<TransportMessage>,
    ) {
        let key = match request_type {
            ServerInitiatedType::Sampling => "sampling/createMessage",
            ServerInitiatedType::Roots => "roots/list",
            ServerInitiatedType::Elicitation => "elicitation/create",
            ServerInitiatedType::Ping => "ping",
        };
        self.server_handlers.insert(key.to_string(), handler);
    }

    /// Process incoming message with direction validation
    async fn process_incoming(&self, message: TransportMessage) -> TransportResult<()> {
        let message_type = extract_message_type(&message);

        // Validate direction
        if !self.validator.validate(&message_type, self.direction) {
            return Err(TransportError::ProtocolError(format!(
                "Invalid message direction for {}: expected {:?}",
                message_type, self.direction
            )));
        }

        // Check for correlation
        if let Some(correlation_id) = extract_correlation_id(&message)
            && let Some((_, context)) = self.correlations.remove(&correlation_id)
        {
            // This is a response to a previous request
            if let Some(tx) = context.response_tx {
                let _ = tx.send(message);
            }
            return Ok(());
        }

        // Route the message
        match self.router.route(&message) {
            RouteAction::Forward => {
                // Forward to standard processing
                self.handle_standard_message(message).await
            }
            RouteAction::Handle(handler_name) => {
                // Route to specific handler
                self.handle_with_handler(message, &handler_name).await
            }
            RouteAction::Drop => Ok(()),
            RouteAction::Transform(transformed) => {
                // Process transformed message
                self.handle_standard_message(transformed).await
            }
        }
    }

    /// Handle standard message processing
    async fn handle_standard_message(&self, message: TransportMessage) -> TransportResult<()> {
        // Check if this is a server-initiated request
        let message_type = extract_message_type(&message);
        if let Some(handler) = self.server_handlers.get(&message_type) {
            handler
                .send(message)
                .await
                .map_err(|e| TransportError::Internal(e.to_string()))?;
        }
        Ok(())
    }

    /// Handle message with specific handler
    async fn handle_with_handler(
        &self,
        _message: TransportMessage,
        _handler_name: &str,
    ) -> TransportResult<()> {
        // This would route to registered handlers
        // Implementation depends on handler registration system
        Ok(())
    }

    /// Send a server-initiated request.
    ///
    /// The correlation key is the JSON-RPC `id` of `message` (extracted from
    /// the payload, falling back to `TransportMessage::id`). Responses are
    /// matched by inspecting the JSON-RPC `id` field of the inbound payload.
    pub async fn send_server_request(
        &self,
        _request_type: ServerInitiatedType,
        message: TransportMessage,
        timeout_duration: Duration,
    ) -> TransportResult<TransportMessage> {
        // Validate this is allowed from server
        if self.direction != MessageDirection::ServerToClient {
            return Err(TransportError::ProtocolError(
                "Cannot send server-initiated request from client transport".to_string(),
            ));
        }

        let correlation_key = correlation_key_for(&message);
        let (tx, rx) = oneshot::channel();
        let context = CorrelationContext {
            correlation_id: correlation_key.clone(),
            request_id: correlation_key.clone(),
            response_tx: Some(tx),
            timeout: timeout_duration,
            created_at: std::time::Instant::now(),
        };

        self.try_insert_correlation(correlation_key.clone(), context)?;

        // Send the message
        if let Err(e) = self.inner.send(message).await {
            self.correlations.remove(&correlation_key);
            return Err(e);
        }

        // Wait for response with timeout
        match timeout(timeout_duration, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                self.correlations.remove(&correlation_key);
                Err(TransportError::Internal(
                    "Response channel closed".to_string(),
                ))
            }
            Err(_) => {
                self.correlations.remove(&correlation_key);
                Err(TransportError::Timeout)
            }
        }
    }

    /// Enable server-initiated requests
    pub async fn enable_server_initiated(&self) {
        let mut state = self.state.write().await;
        state.server_initiated_enabled = true;
    }

    /// Check if server-initiated requests are enabled
    pub async fn is_server_initiated_enabled(&self) -> bool {
        let state = self.state.read().await;
        state.server_initiated_enabled
    }
}

// Helper functions

/// Extract the JSON-RPC `id` from a message and render it as a string.
///
/// MCP rides on JSON-RPC 2.0, where responses match requests by `id`
/// (`string | number`, not just `string`). Both shapes share a namespace here:
/// numeric ids are rendered via `Display` so `42` and `"42"` collide — which
/// matches JSON-RPC's actual interop reality (some peers wrap ints as strings).
fn extract_correlation_id(message: &TransportMessage) -> Option<String> {
    extract_jsonrpc_id(&message.payload)
}

fn extract_jsonrpc_id(payload: &[u8]) -> Option<String> {
    let json = serde_json::from_slice::<serde_json::Value>(payload).ok()?;
    let id = json.get("id")?;
    match id {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Pick a correlation key for an outgoing request.
///
/// Prefers the JSON-RPC `id` baked into the payload (so the responder echoes
/// the same key). Falls back to `TransportMessage::id` rendered as a string —
/// this only matters for non-JSON-RPC payloads, which shouldn't be flowing
/// through this wrapper but is preferable to panicking.
fn correlation_key_for(message: &TransportMessage) -> String {
    extract_jsonrpc_id(&message.payload).unwrap_or_else(|| message.id.to_string())
}

/// Background sweeper that drops correlations whose timeout has elapsed.
///
/// Holds a `Weak<DashMap<…>>` so the wrapper drop'ing the strong reference
/// signals the reaper to exit on the next tick.
fn spawn_correlation_reaper(
    correlations: std::sync::Weak<DashMap<String, CorrelationContext>>,
    interval: Duration,
) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip the immediate-fire first tick.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let Some(map) = correlations.upgrade() else {
                break;
            };
            map.retain(|_, ctx| !ctx.is_expired());
        }
    });
}

// Implement Transport trait for the wrapper
impl<T: Transport> Transport for BidirectionalTransportWrapper<T> {
    fn transport_type(&self) -> TransportType {
        self.inner.transport_type()
    }

    fn capabilities(&self) -> &TransportCapabilities {
        self.inner.capabilities()
    }

    fn state(&self) -> Pin<Box<dyn Future<Output = TransportState> + Send + '_>> {
        Box::pin(async move { self.inner.state().await })
    }

    fn connect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move { self.inner.connect().await })
    }

    fn disconnect(&self) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            // Clean up correlations
            self.correlations.clear();
            self.inner.disconnect().await
        })
    }

    fn send(
        &self,
        message: TransportMessage,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            // Validate direction before sending
            let message_type = extract_message_type(&message);
            if !self.validator.validate(&message_type, self.direction) {
                return Err(TransportError::ProtocolError(format!(
                    "Cannot send {} in direction {:?}",
                    message_type, self.direction
                )));
            }
            self.inner.send(message).await
        })
    }

    fn receive(
        &self,
    ) -> Pin<Box<dyn Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_>> {
        Box::pin(async move {
            if let Some(message) = self.inner.receive().await? {
                self.process_incoming(message.clone()).await?;
                Ok(Some(message))
            } else {
                Ok(None)
            }
        })
    }

    fn metrics(&self) -> Pin<Box<dyn Future<Output = crate::core::TransportMetrics> + Send + '_>> {
        Box::pin(async move { self.inner.metrics().await })
    }
}

// Implement BidirectionalTransport trait
impl<T: Transport> BidirectionalTransport for BidirectionalTransportWrapper<T> {
    fn send_request(
        &self,
        message: TransportMessage,
        timeout_duration: Option<Duration>,
    ) -> Pin<Box<dyn Future<Output = TransportResult<TransportMessage>> + Send + '_>> {
        Box::pin(async move {
            let timeout_duration = timeout_duration.unwrap_or(Duration::from_secs(30));
            let correlation_key = correlation_key_for(&message);
            let (tx, rx) = oneshot::channel();

            let context = CorrelationContext {
                correlation_id: correlation_key.clone(),
                request_id: correlation_key.clone(),
                response_tx: Some(tx),
                timeout: timeout_duration,
                created_at: std::time::Instant::now(),
            };

            self.try_insert_correlation(correlation_key.clone(), context)?;

            // Send message
            if let Err(e) = self.send(message).await {
                self.correlations.remove(&correlation_key);
                return Err(e);
            }

            // Wait for response
            match timeout(timeout_duration, rx).await {
                Ok(Ok(response)) => Ok(response),
                Ok(Err(_)) => {
                    self.correlations.remove(&correlation_key);
                    Err(TransportError::Internal(
                        "Response channel closed".to_string(),
                    ))
                }
                Err(_) => {
                    self.correlations.remove(&correlation_key);
                    Err(TransportError::Timeout)
                }
            }
        })
    }

    fn start_correlation(
        &self,
        correlation_id: String,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            let context = CorrelationContext {
                correlation_id: correlation_id.clone(),
                request_id: correlation_id.clone(),
                response_tx: None,
                timeout: Duration::from_secs(30),
                created_at: std::time::Instant::now(),
            };
            self.try_insert_correlation(correlation_id, context)
        })
    }

    fn stop_correlation(
        &self,
        correlation_id: &str,
    ) -> Pin<Box<dyn Future<Output = TransportResult<()>> + Send + '_>> {
        let correlation_id = correlation_id.to_string();
        Box::pin(async move {
            self.correlations.remove(&correlation_id);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_direction_validator() {
        let validator = ProtocolDirectionValidator::new();

        // Test client-to-server messages
        assert!(validator.validate("tools/call", MessageDirection::ClientToServer));
        assert!(!validator.validate("tools/call", MessageDirection::ServerToClient));

        // Test server-to-client messages
        assert!(validator.validate("sampling/createMessage", MessageDirection::ServerToClient));
        assert!(!validator.validate("sampling/createMessage", MessageDirection::ClientToServer));

        // Test bidirectional messages
        assert!(validator.validate("ping", MessageDirection::ClientToServer));
        assert!(validator.validate("ping", MessageDirection::ServerToClient));
    }

    #[test]
    fn test_message_router() {
        let router = MessageRouter::new();

        router.add_route("test".to_string(), |_msg| {
            RouteAction::Handle("test_handler".to_string())
        });

        let message = TransportMessage {
            id: turbomcp_protocol::MessageId::from("test-message-id"),
            payload: br#"{"method": "test"}"#.to_vec().into(),
            metadata: Default::default(),
        };

        match router.route(&message) {
            RouteAction::Handle(handler) => assert_eq!(handler, "test_handler"),
            _ => panic!("Expected Handle action"),
        }
    }

    #[tokio::test]
    async fn test_connection_state() {
        let state = ConnectionState::default();
        assert!(!state.server_initiated_enabled);
        assert!(state.active_server_requests.is_empty());
        assert!(state.pending_elicitations.is_empty());
    }

    #[test]
    fn test_extract_correlation_id_reads_jsonrpc_id() {
        // Numeric id — must be rendered as a string for the map key.
        let msg = TransportMessage {
            id: turbomcp_protocol::MessageId::from("any-transport-id"),
            payload: br#"{"jsonrpc":"2.0","id":42,"result":{}}"#.to_vec().into(),
            metadata: Default::default(),
        };
        assert_eq!(extract_correlation_id(&msg), Some("42".to_string()));

        // String id — same.
        let msg = TransportMessage {
            id: turbomcp_protocol::MessageId::from("any-transport-id"),
            payload: br#"{"jsonrpc":"2.0","id":"req-7","result":{}}"#.to_vec().into(),
            metadata: Default::default(),
        };
        assert_eq!(extract_correlation_id(&msg), Some("req-7".to_string()));

        // Missing id — None.
        let msg = TransportMessage {
            id: turbomcp_protocol::MessageId::from("x"),
            payload: br#"{"jsonrpc":"2.0","method":"foo"}"#.to_vec().into(),
            metadata: Default::default(),
        };
        assert_eq!(extract_correlation_id(&msg), None);

        // The legacy bespoke `correlation_id` field must NOT be honoured —
        // that's what made the response path silently miss every match.
        let msg = TransportMessage {
            id: turbomcp_protocol::MessageId::from("x"),
            payload: br#"{"correlation_id":"legacy","method":"foo"}"#.to_vec().into(),
            metadata: Default::default(),
        };
        assert_eq!(extract_correlation_id(&msg), None);
    }

    #[test]
    fn test_correlation_key_for_prefers_payload_id() {
        let msg = TransportMessage {
            id: turbomcp_protocol::MessageId::from("transport-id"),
            payload: br#"{"jsonrpc":"2.0","id":"jsonrpc-id","method":"x"}"#
                .to_vec()
                .into(),
            metadata: Default::default(),
        };
        assert_eq!(correlation_key_for(&msg), "jsonrpc-id".to_string());
    }
}

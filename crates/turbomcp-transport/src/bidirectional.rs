//! Bidirectional transport implementation with server-initiated request support
//!
//! This module provides enhanced transport capabilities for MCP 2025-06-18 protocol
//! including server-initiated requests, message correlation, and protocol direction validation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio::time::timeout;
use turbomcp_protocol::ServerInitiatedType;
use uuid::Uuid;

use crate::core::{
    BidirectionalTransport, Transport, TransportCapabilities, TransportError, TransportMessage,
    TransportResult, TransportState, TransportType,
};

/// Message direction in the transport layer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageDirection {
    /// Client to server message
    ClientToServer,
    /// Server to client message
    ServerToClient,
}

/// Correlation context for request-response patterns
#[derive(Debug)]
pub struct CorrelationContext {
    /// Unique correlation ID
    pub correlation_id: String,
    /// Original request message ID
    pub request_id: String,
    /// Response channel (not cloneable, so we don't derive Clone)
    pub response_tx: Option<oneshot::Sender<TransportMessage>>,
    /// Timeout duration
    pub timeout: Duration,
    /// Creation timestamp
    pub created_at: std::time::Instant,
}

/// Enhanced bidirectional transport wrapper
#[derive(Debug)]
pub struct BidirectionalTransportWrapper<T: Transport> {
    /// Inner transport implementation
    inner: T,
    /// Message direction for this transport
    direction: MessageDirection,
    /// Active correlations for request-response
    correlations: Arc<DashMap<String, CorrelationContext>>,
    /// Server-initiated request handlers (using String keys instead of ServerInitiatedType)
    server_handlers: Arc<DashMap<String, mpsc::Sender<TransportMessage>>>,
    /// Protocol direction validator
    validator: Arc<ProtocolDirectionValidator>,
    /// Message router
    router: Arc<MessageRouter>,
    /// Connection state
    state: Arc<RwLock<ConnectionState>>,
}

/// Connection state for bidirectional communication
#[derive(Debug, Clone, Default)]
pub struct ConnectionState {
    /// Whether server-initiated requests are enabled
    pub server_initiated_enabled: bool,
    /// Active server-initiated request IDs
    pub active_server_requests: Vec<String>,
    /// Pending elicitations
    pub pending_elicitations: Vec<String>,
    /// Connection metadata
    pub metadata: HashMap<String, serde_json::Value>,
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
            client_to_server: vec![
                "initialize".to_string(),
                "initialized".to_string(),
                "tools/call".to_string(),
                "resources/read".to_string(),
                "prompts/get".to_string(),
                "completion/complete".to_string(),
                "resources/templates/list".to_string(),
            ],
            server_to_client: vec![
                "sampling/createMessage".to_string(),
                "roots/list".to_string(),
                "elicitation/create".to_string(),
                "notifications/message".to_string(),
                "notifications/resources/updated".to_string(),
                "notifications/tools/updated".to_string(),
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
    /// Create a new bidirectional transport wrapper
    pub fn new(inner: T, direction: MessageDirection) -> Self {
        Self {
            inner,
            direction,
            correlations: Arc::new(DashMap::new()),
            server_handlers: Arc::new(DashMap::new()),
            validator: Arc::new(ProtocolDirectionValidator::new()),
            router: Arc::new(MessageRouter::new()),
            state: Arc::new(RwLock::new(ConnectionState::default())),
        }
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

    /// Send a server-initiated request
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

        // Create correlation context
        let correlation_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        let context = CorrelationContext {
            correlation_id: correlation_id.clone(),
            request_id: Uuid::new_v4().to_string(),
            response_tx: Some(tx),
            timeout: timeout_duration,
            created_at: std::time::Instant::now(),
        };

        self.correlations.insert(correlation_id.clone(), context);

        // Send the message
        self.inner.send(message).await?;

        // Wait for response with timeout
        match timeout(timeout_duration, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(TransportError::Internal(
                "Response channel closed".to_string(),
            )),
            Err(_) => {
                self.correlations.remove(&correlation_id);
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

/// Extract correlation ID from message
fn extract_correlation_id(message: &TransportMessage) -> Option<String> {
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&message.payload) {
        json.get("correlation_id")
            .and_then(|id| id.as_str())
            .map(|s| s.to_string())
    } else {
        None
    }
}

/// Detect server-initiated request type
#[allow(dead_code)]
fn detect_server_initiated_type(message: &TransportMessage) -> Option<ServerInitiatedType> {
    let message_type = extract_message_type(message);

    match message_type.as_str() {
        "sampling/createMessage" => Some(ServerInitiatedType::Sampling),
        "roots/list" => Some(ServerInitiatedType::Roots),
        "elicitation/create" => Some(ServerInitiatedType::Elicitation),
        "ping" => Some(ServerInitiatedType::Ping),
        _ => None,
    }
}

// Implement Transport trait for the wrapper
#[async_trait]
impl<T: Transport> Transport for BidirectionalTransportWrapper<T> {
    fn transport_type(&self) -> TransportType {
        self.inner.transport_type()
    }

    fn capabilities(&self) -> &TransportCapabilities {
        self.inner.capabilities()
    }

    async fn state(&self) -> TransportState {
        self.inner.state().await
    }

    async fn connect(&self) -> TransportResult<()> {
        self.inner.connect().await
    }

    async fn disconnect(&self) -> TransportResult<()> {
        // Clean up correlations
        self.correlations.clear();
        self.inner.disconnect().await
    }

    async fn send(&self, message: TransportMessage) -> TransportResult<()> {
        // Validate direction before sending
        let message_type = extract_message_type(&message);
        if !self.validator.validate(&message_type, self.direction) {
            return Err(TransportError::ProtocolError(format!(
                "Cannot send {} in direction {:?}",
                message_type, self.direction
            )));
        }
        self.inner.send(message).await
    }

    async fn receive(&self) -> TransportResult<Option<TransportMessage>> {
        if let Some(message) = self.inner.receive().await? {
            self.process_incoming(message.clone()).await?;
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    async fn metrics(&self) -> crate::core::TransportMetrics {
        self.inner.metrics().await
    }
}

// Implement BidirectionalTransport trait
#[async_trait]
impl<T: Transport> BidirectionalTransport for BidirectionalTransportWrapper<T> {
    async fn send_request(
        &self,
        message: TransportMessage,
        timeout_duration: Option<Duration>,
    ) -> TransportResult<TransportMessage> {
        let timeout_duration = timeout_duration.unwrap_or(Duration::from_secs(30));

        // Create correlation
        let correlation_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        let context = CorrelationContext {
            correlation_id: correlation_id.clone(),
            request_id: Uuid::new_v4().to_string(),
            response_tx: Some(tx),
            timeout: timeout_duration,
            created_at: std::time::Instant::now(),
        };

        self.correlations.insert(correlation_id.clone(), context);

        // Send message
        self.send(message).await?;

        // Wait for response
        match timeout(timeout_duration, rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(TransportError::Internal(
                "Response channel closed".to_string(),
            )),
            Err(_) => {
                self.correlations.remove(&correlation_id);
                Err(TransportError::Timeout)
            }
        }
    }

    async fn start_correlation(&self, correlation_id: String) -> TransportResult<()> {
        let context = CorrelationContext {
            correlation_id: correlation_id.clone(),
            request_id: Uuid::new_v4().to_string(),
            response_tx: None,
            timeout: Duration::from_secs(30),
            created_at: std::time::Instant::now(),
        };

        self.correlations.insert(correlation_id, context);
        Ok(())
    }

    async fn stop_correlation(&self, correlation_id: &str) -> TransportResult<()> {
        self.correlations.remove(correlation_id);
        Ok(())
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
}

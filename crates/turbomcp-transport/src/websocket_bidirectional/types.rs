//! Core types and type aliases for WebSocket bidirectional transport
//!
//! This module defines the core types used throughout the WebSocket transport
//! implementation, including stream type aliases and pending request structures.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use dashmap::DashMap;
use futures::{stream::SplitSink, stream::SplitStream};
use serde_json::json;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};
use turbomcp_protocol::elicitation::{ElicitationCreateRequest, ElicitationCreateResult};
use uuid::Uuid;

use super::config::WebSocketBidirectionalConfig;
use crate::bidirectional::{ConnectionState, CorrelationContext};
use crate::core::{TransportCapabilities, TransportEventEmitter, TransportMetrics, TransportState};

// Type aliases to reduce complexity and improve readability
/// WebSocket writer handle for sending messages (thread-safe, async-safe)
pub type WebSocketWriter =
    Arc<Mutex<Option<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>>;
/// WebSocket reader handle for receiving messages (thread-safe, async-safe)
pub type WebSocketReader =
    Arc<Mutex<Option<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>>>;

/// Pending elicitation request
#[derive(Debug)]
pub struct PendingElicitation {
    /// Request ID for correlation
    pub request_id: String,

    /// The elicitation request
    pub request: ElicitationCreateRequest,

    /// Response channel
    pub response_tx: oneshot::Sender<ElicitationCreateResult>,

    /// Timeout deadline
    pub deadline: tokio::time::Instant,

    /// Retry count
    pub retry_count: u32,
}

impl PendingElicitation {
    /// Create a new pending elicitation
    pub fn new(
        request: ElicitationCreateRequest,
        response_tx: oneshot::Sender<ElicitationCreateResult>,
        timeout: Duration,
    ) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            request,
            response_tx,
            deadline: tokio::time::Instant::now() + timeout,
            retry_count: 0,
        }
    }

    /// Check if the elicitation has expired
    pub fn is_expired(&self) -> bool {
        tokio::time::Instant::now() >= self.deadline
    }

    /// Get time remaining until expiration
    pub fn time_remaining(&self) -> Duration {
        if self.is_expired() {
            Duration::ZERO
        } else {
            self.deadline.duration_since(tokio::time::Instant::now())
        }
    }

    /// Increment retry count
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}

/// WebSocket bidirectional transport implementation
#[derive(Debug)]
pub struct WebSocketBidirectionalTransport {
    /// Transport state
    pub state: Arc<RwLock<TransportState>>,

    /// Transport capabilities
    pub capabilities: TransportCapabilities,

    /// Configuration (mutex for interior mutability)
    pub config: Arc<std::sync::Mutex<WebSocketBidirectionalConfig>>,

    /// Metrics collector
    pub metrics: Arc<RwLock<TransportMetrics>>,

    /// Event emitter for transport events
    pub event_emitter: Arc<TransportEventEmitter>,

    /// WebSocket write half (sender)
    pub writer: WebSocketWriter,

    /// WebSocket read half (receiver)
    pub reader: WebSocketReader,

    /// Active correlations for request-response patterns
    pub correlations: Arc<DashMap<String, CorrelationContext>>,

    /// Pending elicitation requests
    pub elicitations: Arc<DashMap<String, PendingElicitation>>,

    /// Connection state
    pub connection_state: Arc<RwLock<ConnectionState>>,

    /// Background task handles
    pub task_handles: Arc<RwLock<Vec<tokio::task::JoinHandle<()>>>>,

    /// Shutdown signal (tokio mutex - held across await)
    pub shutdown_tx: Arc<tokio::sync::Mutex<Option<mpsc::Sender<()>>>>,

    /// Session ID for this connection
    pub session_id: String,
}

impl WebSocketBidirectionalTransport {
    /// Create transport capabilities for WebSocket bidirectional transport
    pub fn create_capabilities(config: &WebSocketBidirectionalConfig) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: Some(config.max_message_size),
            supports_compression: config.enable_compression,
            supports_streaming: true,
            supports_bidirectional: true,
            supports_multiplexing: true,
            compression_algorithms: if config.enable_compression {
                vec!["deflate".to_string(), "gzip".to_string()]
            } else {
                Vec::new()
            },
            custom: {
                let mut custom = std::collections::HashMap::new();
                custom.insert("elicitation".to_string(), json!(true));
                custom.insert("sampling".to_string(), json!(true));
                custom.insert("websocket_version".to_string(), json!("13"));
                custom.insert(
                    "max_concurrent_elicitations".to_string(),
                    json!(config.max_concurrent_elicitations),
                );
                custom
            },
        }
    }

    /// Get the current number of pending elicitations
    pub fn pending_elicitations_count(&self) -> usize {
        self.elicitations.len()
    }

    /// Get the current number of active correlations
    pub fn active_correlations_count(&self) -> usize {
        self.correlations.len()
    }

    /// Check if the transport is at elicitation capacity
    pub fn is_at_elicitation_capacity(&self) -> bool {
        self.elicitations.len()
            >= self
                .config
                .lock()
                .expect("config mutex poisoned")
                .max_concurrent_elicitations
    }

    /// Get session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Check if WebSocket is connected
    pub async fn is_writer_connected(&self) -> bool {
        self.writer.lock().await.is_some()
    }

    /// Check if WebSocket reader is available
    pub async fn is_reader_connected(&self) -> bool {
        self.reader.lock().await.is_some()
    }
}

/// Trait for types that can be used as WebSocket stream endpoints
#[async_trait]
pub trait WebSocketStreamHandler {
    /// Setup the WebSocket stream
    async fn setup_stream(
        &mut self,
        stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Handle incoming WebSocket message
    async fn handle_message(
        &self,
        message: Message,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>>;
}

/// WebSocket message processing result
#[derive(Debug)]
pub enum MessageProcessingResult {
    /// Message was processed successfully
    Processed,
    /// Message should be forwarded to the application
    Forward(Bytes),
    /// Message processing failed
    Failed(String),
    /// No action needed (e.g., for ping/pong)
    NoAction,
}

/// Connection statistics for monitoring
#[derive(Debug, Clone)]
pub struct WebSocketConnectionStats {
    /// Number of messages sent
    pub messages_sent: u64,
    /// Number of messages received
    pub messages_received: u64,
    /// Number of ping messages sent
    pub pings_sent: u64,
    /// Number of pong messages received
    pub pongs_received: u64,
    /// Number of connection errors
    pub connection_errors: u64,
    /// Number of reconnection attempts
    pub reconnection_attempts: u64,
    /// Current connection state
    pub connection_state: TransportState,
    /// Time when connection was established
    pub connected_at: Option<std::time::SystemTime>,
    /// Time of last message activity
    pub last_activity: Option<std::time::SystemTime>,
}

impl Default for WebSocketConnectionStats {
    fn default() -> Self {
        Self {
            messages_sent: 0,
            messages_received: 0,
            pings_sent: 0,
            pongs_received: 0,
            connection_errors: 0,
            reconnection_attempts: 0,
            connection_state: TransportState::Disconnected,
            connected_at: None,
            last_activity: None,
        }
    }
}

impl WebSocketConnectionStats {
    /// Create new connection statistics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a sent message
    pub fn record_message_sent(&mut self) {
        self.messages_sent += 1;
        self.last_activity = Some(std::time::SystemTime::now());
    }

    /// Record a received message
    pub fn record_message_received(&mut self) {
        self.messages_received += 1;
        self.last_activity = Some(std::time::SystemTime::now());
    }

    /// Record a sent ping
    pub fn record_ping_sent(&mut self) {
        self.pings_sent += 1;
    }

    /// Record a received pong
    pub fn record_pong_received(&mut self) {
        self.pongs_received += 1;
    }

    /// Record a connection error
    pub fn record_connection_error(&mut self) {
        self.connection_errors += 1;
    }

    /// Record a reconnection attempt
    pub fn record_reconnection_attempt(&mut self) {
        self.reconnection_attempts += 1;
    }

    /// Set connection state
    pub fn set_connection_state(&mut self, state: TransportState) {
        self.connection_state = state.clone();
        if matches!(state, TransportState::Connected) {
            self.connected_at = Some(std::time::SystemTime::now());
        }
    }

    /// Get connection uptime
    pub fn uptime(&self) -> Option<Duration> {
        self.connected_at.and_then(|connected_at| {
            std::time::SystemTime::now()
                .duration_since(connected_at)
                .ok()
        })
    }

    /// Get idle time since last activity
    pub fn idle_time(&self) -> Option<Duration> {
        self.last_activity.and_then(|last_activity| {
            std::time::SystemTime::now()
                .duration_since(last_activity)
                .ok()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_elicitation_creation() {
        use std::collections::HashMap;
        use turbomcp_protocol::elicitation::ElicitationSchema;

        let request = ElicitationCreateRequest {
            message: "Test message".to_string(),
            requested_schema: ElicitationSchema {
                schema_type: "object".to_string(),
                properties: HashMap::new(),
                required: None,
            },
        };
        let (tx, _rx) = oneshot::channel();
        let timeout = Duration::from_secs(30);

        let pending = PendingElicitation::new(request, tx, timeout);

        assert!(!pending.request_id.is_empty());
        assert_eq!(pending.retry_count, 0);
        assert!(!pending.is_expired());
        assert!(pending.time_remaining() > Duration::from_secs(25));
    }

    #[test]
    fn test_websocket_connection_stats() {
        let mut stats = WebSocketConnectionStats::new();

        stats.record_message_sent();
        stats.record_message_received();
        stats.record_ping_sent();
        stats.record_pong_received();
        stats.record_connection_error();

        assert_eq!(stats.messages_sent, 1);
        assert_eq!(stats.messages_received, 1);
        assert_eq!(stats.pings_sent, 1);
        assert_eq!(stats.pongs_received, 1);
        assert_eq!(stats.connection_errors, 1);
        assert!(stats.last_activity.is_some());
    }

    #[test]
    fn test_create_capabilities() {
        let config = WebSocketBidirectionalConfig {
            enable_compression: true,
            max_message_size: 1024 * 1024,
            max_concurrent_elicitations: 5,
            ..Default::default()
        };

        let capabilities = WebSocketBidirectionalTransport::create_capabilities(&config);

        assert!(capabilities.supports_compression);
        assert!(capabilities.supports_bidirectional);
        assert!(capabilities.supports_streaming);
        assert!(capabilities.supports_multiplexing);
        assert_eq!(capabilities.max_message_size, Some(1024 * 1024));
        assert!(!capabilities.compression_algorithms.is_empty());
        assert!(capabilities.custom.contains_key("elicitation"));
    }
}

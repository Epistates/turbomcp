//! HTTP with Server-Sent Events (SSE) transport for bidirectional communication
//!
//! This transport provides bidirectional MCP communication over HTTP:
//! - Server → Client: Server-Sent Events (SSE) for real-time push
//! - Client → Server: HTTP POST for responses
//!
//! This enables elicitation and other server-initiated requests while
//! maintaining compatibility with HTTP infrastructure.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
#[cfg(feature = "http")]
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tracing::{debug, error, info};
use uuid::Uuid;

use turbomcp_core::MessageId;

use crate::core::{
    Transport, TransportCapabilities, TransportError, TransportEventEmitter, TransportMessage,
    TransportMessageMetadata, TransportMetrics, TransportResult, TransportState, TransportType,
};

/// HTTP/SSE transport implementation
pub struct HttpSseTransport {
    /// Transport state
    state: Arc<RwLock<TransportState>>,

    /// Transport capabilities
    capabilities: TransportCapabilities,

    /// Transport configuration
    config: HttpSseConfig,

    /// Metrics collector
    metrics: Arc<RwLock<TransportMetrics>>,

    /// Event emitter
    _event_emitter: TransportEventEmitter,

    /// SSE channel for server → client messages
    sse_sender: Arc<Mutex<mpsc::UnboundedSender<SseMessage>>>,

    /// Channel for receiving client → server messages
    request_receiver: Arc<Mutex<mpsc::UnboundedReceiver<ClientRequest>>>,

    /// Pending responses waiting for client acknowledgment
    pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<serde_json::Value>>>>,

    /// Session manager for tracking connected clients
    sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,

    /// Server handle for shutdown
    server_handle: Option<tokio::task::JoinHandle<()>>,
}

/// HTTP/SSE specific configuration
#[derive(Clone, Debug)]
pub struct HttpSseConfig {
    /// Bind address for HTTP server
    pub bind_addr: String,

    /// Path for SSE endpoint (default: "/events")
    pub sse_path: String,

    /// Path for POST endpoint (default: "/mcp")
    pub post_path: String,

    /// Keep-alive interval for SSE
    pub keep_alive_interval: Duration,

    /// Maximum concurrent sessions
    pub max_sessions: usize,

    /// Session timeout
    pub session_timeout: Duration,

    /// Enable CORS headers
    pub enable_cors: bool,
}

impl Default for HttpSseConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:3000".to_string(),
            sse_path: "/events".to_string(),
            post_path: "/mcp".to_string(),
            keep_alive_interval: Duration::from_secs(30),
            max_sessions: 100,
            session_timeout: Duration::from_secs(300),
            enable_cors: true,
        }
    }
}

/// SSE message types
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum SseMessage {
    /// Regular JSON-RPC request/response
    JsonRpc { id: String, data: serde_json::Value },

    /// Elicitation request
    Elicitation {
        request_id: String,
        request: serde_json::Value,
    },

    /// Control message
    Control {
        action: String,
        data: Option<serde_json::Value>,
    },
}

/// Client request via HTTP POST
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ClientRequest {
    /// Session ID for correlation
    session_id: String,

    /// Request ID for response correlation
    request_id: Option<String>,

    /// The actual request/response data
    data: serde_json::Value,
}

/// Session information
#[derive(Clone, Debug)]
struct SessionInfo {
    _id: String,
    _created_at: std::time::Instant,
    last_activity: std::time::Instant,
    sse_sender: mpsc::UnboundedSender<SseMessage>,
}

impl std::fmt::Debug for HttpSseTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpSseTransport")
            .field("session_count", &self.sessions.blocking_read().len())
            .field("config", &self.config)
            .finish()
    }
}

impl HttpSseTransport {
    /// Create a new HTTP/SSE transport
    pub fn new(config: HttpSseConfig) -> Self {
        let (sse_tx, _sse_rx) = mpsc::unbounded_channel();
        let (_req_tx, req_rx) = mpsc::unbounded_channel();
        let (event_emitter, _) = TransportEventEmitter::new();

        Self {
            state: Arc::new(RwLock::new(TransportState::Disconnected)),
            capabilities: TransportCapabilities {
                max_message_size: Some(turbomcp_core::MAX_MESSAGE_SIZE),
                supports_compression: false,
                supports_streaming: true,
                supports_bidirectional: true,
                supports_multiplexing: true,
                compression_algorithms: Vec::new(),
                custom: HashMap::new(),
            },
            config,
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            _event_emitter: event_emitter,
            sse_sender: Arc::new(Mutex::new(sse_tx)),
            request_receiver: Arc::new(Mutex::new(req_rx)),
            pending_responses: Arc::new(RwLock::new(HashMap::new())),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            server_handle: None,
        }
    }

    /// Start the HTTP server
    pub async fn start_server(&mut self) -> TransportResult<()> {
        let app = self.create_router();
        let addr: std::net::SocketAddr = self.config.bind_addr.parse().map_err(|e| {
            TransportError::ConfigurationError(format!("Invalid bind address: {}", e))
        })?;

        info!("Starting HTTP/SSE server on {}", self.config.bind_addr);

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| TransportError::ConnectionFailed(format!("Failed to bind: {}", e)))?;

        let handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!("HTTP server error: {}", e);
            }
        });

        self.server_handle = Some(handle);
        *self.state.write().await = TransportState::Connected;

        Ok(())
    }

    /// Create the Axum router
    fn create_router(&self) -> Router {
        let state = AppState {
            transport: self.clone_refs(),
        };

        Router::new()
            .route(&self.config.sse_path, get(sse_handler))
            .route(&self.config.post_path, post(post_handler))
            .with_state(state)
    }

    /// Clone internal Arc references for sharing
    fn clone_refs(&self) -> HttpSseTransportRefs {
        HttpSseTransportRefs {
            _sse_sender: self.sse_sender.clone(),
            pending_responses: self.pending_responses.clone(),
            sessions: self.sessions.clone(),
            _metrics: self.metrics.clone(),
            config: self.config.clone(),
        }
    }

    /// Send elicitation request to client
    pub async fn send_elicitation(
        &self,
        session_id: &str,
        request_id: String,
        request: serde_json::Value,
    ) -> TransportResult<()> {
        let sessions = self.sessions.read().await;

        if let Some(session) = sessions.get(session_id) {
            let message = SseMessage::Elicitation {
                request_id,
                request,
            };

            session.sse_sender.send(message).map_err(|e| {
                TransportError::SendFailed(format!("Failed to send elicitation: {}", e))
            })?;

            Ok(())
        } else {
            Err(TransportError::ConnectionFailed(format!(
                "Session not found: {}",
                session_id
            )))
        }
    }
}

/// Shared references for handlers
#[derive(Clone)]
struct HttpSseTransportRefs {
    _sse_sender: Arc<Mutex<mpsc::UnboundedSender<SseMessage>>>,
    pending_responses: Arc<RwLock<HashMap<String, oneshot::Sender<serde_json::Value>>>>,
    sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
    _metrics: Arc<RwLock<TransportMetrics>>,
    config: HttpSseConfig,
}

/// Application state for Axum
#[derive(Clone)]
struct AppState {
    transport: HttpSseTransportRefs,
}

/// SSE handler for server → client messages
async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, axum::Error>>> {
    let (tx, mut rx) = mpsc::unbounded_channel::<SseMessage>();

    // Create new session
    let session_id = Uuid::new_v4().to_string();
    let session = SessionInfo {
        _id: session_id.clone(),
        _created_at: std::time::Instant::now(),
        last_activity: std::time::Instant::now(),
        sse_sender: tx.clone(),
    };

    state
        .transport
        .sessions
        .write()
        .await
        .insert(session_id.clone(), session);

    info!("New SSE connection established: {}", session_id);

    // Send initial connection event
    let _ = tx.send(SseMessage::Control {
        action: "connected".to_string(),
        data: Some(serde_json::json!({
            "session_id": session_id,
            "protocol_version": "2025-06-18",
        })),
    });

    // Create SSE stream
    let stream = async_stream::stream! {
        while let Some(msg) = rx.recv().await {
            let event = match serde_json::to_string(&msg) {
                Ok(json) => Event::default().data(json),
                Err(e) => {
                    error!("Failed to serialize SSE message: {}", e);
                    continue;
                }
            };

            yield Ok(event);
        }
    };

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(state.transport.config.keep_alive_interval))
}

/// POST handler for client → server messages
async fn post_handler(
    State(state): State<AppState>,
    Json(request): Json<ClientRequest>,
) -> impl IntoResponse {
    debug!("Received POST request from session: {}", request.session_id);

    // Update session activity
    if let Some(session) = state
        .transport
        .sessions
        .write()
        .await
        .get_mut(&request.session_id)
    {
        session.last_activity = std::time::Instant::now();
    } else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "Invalid session"
            })),
        );
    }

    // Handle different request types
    if let Some(request_id) = request.request_id {
        // This is a response to a pending request
        if let Some(sender) = state
            .transport
            .pending_responses
            .write()
            .await
            .remove(&request_id)
        {
            let _ = sender.send(request.data.clone());
        }
    } else {
        // This is a new request from client
        // Process through normal MCP handler
        // (Implementation would forward to MCP server)
    }

    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
}

#[async_trait]
impl Transport for HttpSseTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Http
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        self.state.read().await.clone()
    }

    async fn connect(&mut self) -> TransportResult<()> {
        self.start_server().await
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        *self.state.write().await = TransportState::Disconnected;

        // Shutdown server
        if let Some(handle) = self.server_handle.take() {
            handle.abort();
        }

        // Clear sessions
        self.sessions.write().await.clear();

        Ok(())
    }

    async fn send(&mut self, message: TransportMessage) -> TransportResult<()> {
        // For broadcast to all sessions
        let sessions = self.sessions.read().await;

        for session in sessions.values() {
            let sse_msg = SseMessage::JsonRpc {
                id: message.id.to_string(),
                data: serde_json::from_slice(&message.payload).unwrap_or(serde_json::json!({})),
            };

            let _ = session.sse_sender.send(sse_msg);
        }

        Ok(())
    }

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
        // Receive from request channel
        if let Ok(request) = self.request_receiver.lock().await.try_recv() {
            Ok(Some(TransportMessage {
                id: MessageId::from(uuid::Uuid::new_v4()),
                payload: bytes::Bytes::from(serde_json::to_vec(&request.data).unwrap_or_default()),
                metadata: TransportMessageMetadata::default(),
            }))
        } else {
            Ok(None)
        }
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_http_sse_transport_creation() {
        let config = HttpSseConfig::default();
        let transport = HttpSseTransport::new(config);

        assert_eq!(transport.transport_type(), TransportType::Http);
        assert!(transport.capabilities().supports_bidirectional);
        assert!(transport.capabilities().supports_streaming);
    }

    #[tokio::test]
    async fn test_session_management() {
        let config = HttpSseConfig::default();
        let transport = HttpSseTransport::new(config);

        let (tx, _rx) = mpsc::unbounded_channel();
        let session = SessionInfo {
            _id: "test-session".to_string(),
            _created_at: std::time::Instant::now(),
            last_activity: std::time::Instant::now(),
            sse_sender: tx,
        };

        transport
            .sessions
            .write()
            .await
            .insert("test-session".to_string(), session);

        assert_eq!(transport.sessions.read().await.len(), 1);
        assert!(transport.sessions.read().await.contains_key("test-session"));
    }
}

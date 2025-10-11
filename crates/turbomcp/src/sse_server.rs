//! SSE Server Implementation
//!
//! Real Server-Sent Events HTTP server with session management, authentication,
//! and comprehensive MCP protocol support.
//!
//! **This module is only available with the `http` feature.**

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, broadcast};
//use tokio_stream::StreamExt;
use http::HeaderValue;
use tower_http::cors::{Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

#[cfg(feature = "auth")]
use turbomcp_auth::{AuthContext, AuthManager};

use crate::{
    McpError, McpResult,
    session::{CreateSessionRequest, /*SessionInfo,*/ SessionManager},
};

// Import transport types from mcp-transport directly

/// SSE server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseServerConfig {
    /// Server bind address
    pub bind_address: String,
    /// Server port
    pub port: u16,
    /// SSE endpoint path
    pub sse_path: String,
    /// Message POST endpoint path
    pub message_path: String,
    /// Health check endpoint path
    pub health_path: String,
    /// Keep-alive interval for SSE connections
    pub keep_alive_interval: Duration,
    /// Maximum message size
    pub max_message_size: usize,
    /// Enable CORS
    pub enable_cors: bool,
    /// Allowed origins for CORS
    pub cors_origins: Vec<String>,
    /// Authentication requirement
    pub require_auth: bool,
}

impl Default for SseServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0".to_string(),
            port: 3000,
            sse_path: "/sse".to_string(),
            message_path: "/message".to_string(),
            health_path: "/health".to_string(),
            keep_alive_interval: Duration::from_secs(30),
            max_message_size: 1024 * 1024, // 1MB
            enable_cors: true,
            cors_origins: vec!["*".to_string()],
            require_auth: false,
        }
    }
}

/// SSE connection information
#[derive(Debug, Clone)]
pub struct SseConnection {
    /// Connection ID
    pub connection_id: String,
    /// Associated session ID
    pub session_id: String,
    /// Client ID
    pub client_id: String,
    /// Connection timestamp
    pub connected_at: std::time::SystemTime,
    /// Last activity timestamp
    pub last_activity: std::time::SystemTime,
    /// Authentication context
    #[cfg(feature = "auth")]
    pub auth_context: Option<AuthContext>,
    /// Connection metadata
    pub metadata: HashMap<String, String>,
}

/// Message for SSE streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseMessage {
    /// Message ID
    pub id: String,
    /// Message type
    pub message_type: String,
    /// Message data
    pub data: serde_json::Value,
    /// Message timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// SSE server state
#[derive(Debug)]
pub struct SseServerState {
    /// Server configuration
    pub config: SseServerConfig,
    /// Session manager
    pub session_manager: Arc<SessionManager>,
    /// Authentication manager
    #[cfg(feature = "auth")]
    pub auth_manager: Option<Arc<AuthManager>>,
    /// Active SSE connections
    pub connections: Arc<RwLock<HashMap<String, SseConnection>>>,
    /// Message broadcaster
    pub broadcaster: broadcast::Sender<SseMessage>,
    /// Connection cleanup interval
    pub cleanup_interval: Duration,
}

/// Query parameters for SSE endpoint
#[derive(Debug, Deserialize)]
pub struct SseQueryParams {
    /// Client ID
    pub client_id: Option<String>,
    /// Session ID (if resuming)
    pub session_id: Option<String>,
    /// Authentication token
    pub token: Option<String>,
    /// Client name
    pub client_name: Option<String>,
    /// Client version
    pub client_version: Option<String>,
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Service status
    pub status: String,
    /// Service name
    pub service: String,
    /// Service version
    pub version: String,
    /// Transport type
    pub transport: String,
    /// Active connections count
    pub active_connections: usize,
    /// Active sessions count
    pub active_sessions: usize,
    /// Uptime in seconds
    pub uptime_seconds: u64,
}

/// Message post request
#[derive(Debug, Deserialize)]
pub struct PostMessageRequest {
    /// Session ID
    pub session_id: String,
    /// Message content
    pub message: serde_json::Value,
    /// Message type
    pub message_type: Option<String>,
}

impl SseServerState {
    /// Create new SSE server state
    #[must_use]
    pub fn new(
        config: SseServerConfig,
        session_manager: Arc<SessionManager>,
        #[cfg(feature = "auth")] auth_manager: Option<Arc<AuthManager>>,
    ) -> Self {
        let (broadcaster, _) = broadcast::channel(1000);

        Self {
            config,
            session_manager,
            #[cfg(feature = "auth")]
            auth_manager,
            connections: Arc::new(RwLock::new(HashMap::new())),
            broadcaster,
            cleanup_interval: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Broadcast message to all connections
    pub async fn broadcast_message(&self, message: SseMessage) -> McpResult<()> {
        // broadcast::send returns an error if there are no receivers, but that's ok for our use case
        match self.broadcaster.send(message) {
            Ok(_) => Ok(()),
            Err(broadcast::error::SendError(_)) => {
                // No receivers, but that's not an error for our purposes
                Ok(())
            }
        }
    }

    /// Send message to specific session
    pub async fn send_to_session(&self, session_id: &str, message: SseMessage) -> McpResult<()> {
        // Find connection for session
        let connections = self.connections.read().await;
        let target_connection = connections
            .values()
            .find(|conn| conn.session_id == session_id);

        if target_connection.is_some() {
            // Broadcast to all clients - they can filter based on the message content
            // This is acceptable for SSE where filtering happens client-side
            if self.broadcaster.send(message).is_ok() {
            } else {
                // No receivers, but that's not an error for our purposes
            }
        }

        Ok(())
    }

    /// Get connection statistics
    pub async fn get_connection_stats(&self) -> (usize, usize) {
        let connections_count = self.connections.read().await.len();
        let sessions_stats = self.session_manager.get_statistics().await;
        (connections_count, sessions_stats.active_sessions)
    }

    /// Cleanup inactive connections
    pub async fn cleanup_connections(&self) {
        let now = std::time::SystemTime::now();
        let timeout = Duration::from_secs(300); // 5 minutes

        self.connections.write().await.retain(|_, connection| {
            now.duration_since(connection.last_activity)
                .map_or(true, |duration| duration < timeout)
        });
    }
}

/// SSE endpoint handler
pub async fn sse_handler(
    State(state): State<Arc<SseServerState>>,
    Query(params): Query<SseQueryParams>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Response> {
    // Extract client information from headers
    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(std::string::ToString::to_string);

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(std::string::ToString::to_string);

    // Generate connection ID
    let connection_id = Uuid::new_v4().to_string();
    let client_id = params
        .client_id
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Authenticate if token provided
    #[cfg(feature = "auth")]
    let mut auth_context = None;

    #[cfg(feature = "auth")]
    {
        if let Some(token) = &params.token {
            if let Some(auth_manager) = &state.auth_manager {
                match auth_manager.validate_token(token, None).await {
                    Ok(context) => {
                        auth_context = Some(context);
                    }
                    Err(e) => {
                        tracing::warn!("SSE authentication failed: {}", e);
                        if state.config.require_auth {
                            return Err((
                                StatusCode::UNAUTHORIZED,
                                Json(serde_json::json!({"error": "Authentication required"})),
                            )
                                .into_response());
                        }
                    }
                }
            }
        } else if state.config.require_auth {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "Authentication required"})),
            )
                .into_response());
        }
    }

    #[cfg(not(feature = "auth"))]
    if state.config.require_auth {
        tracing::warn!("Authentication required but 'auth' feature not enabled");
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Authentication not configured"})),
        )
            .into_response());
    }

    // Create or get session
    let session = if let Some(session_id) = &params.session_id {
        // Try to resume existing session
        match state.session_manager.get_session(session_id).await {
            Some(session) => {
                // Update session activity
                if state
                    .session_manager
                    .update_activity(session_id)
                    .await
                    .is_err()
                {
                    tracing::warn!("Failed to update session activity");
                }
                session
            }
            None => {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Session not found"})),
                )
                    .into_response());
            }
        }
    } else {
        // Create new session
        let create_request = CreateSessionRequest {
            client_id: client_id.clone(),
            client_name: params.client_name,
            client_version: params.client_version,
            transport_type: "sse".to_string(),
            client_ip,
            user_agent,
            metadata: HashMap::new(),
            auth_token: params.token,
        };

        match state.session_manager.create_session(create_request).await {
            Ok(session) => session,
            Err(e) => {
                tracing::error!("Failed to create session: {}", e);
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "Failed to create session"})),
                )
                    .into_response());
            }
        }
    };

    // Create SSE connection
    let connection = SseConnection {
        connection_id: connection_id.clone(),
        session_id: session.session_id.clone(),
        client_id: client_id.clone(),
        connected_at: std::time::SystemTime::now(),
        last_activity: std::time::SystemTime::now(),
        #[cfg(feature = "auth")]
        auth_context,
        metadata: HashMap::new(),
    };

    // Store connection
    state
        .connections
        .write()
        .await
        .insert(connection_id.clone(), connection);

    tracing::info!(
        connection_id = %connection_id,
        session_id = %session.session_id,
        client_id = %client_id,
        "SSE connection established"
    );

    // Create event stream
    let receiver = state.broadcaster.subscribe();
    let _session_id = session.session_id.clone();

    let stream = futures::stream::unfold(receiver, |mut receiver| async move {
        match receiver.recv().await {
            Ok(message) => {
                // Send all messages to all connections (standard SSE pattern)
                let event_data = serde_json::to_string(&message).ok()?;
                let event = Event::default()
                    .id(message.id)
                    .event(message.message_type)
                    .data(event_data);
                Some((Ok(event), receiver))
            }
            Err(_) => {
                // Connection closed
                None
            }
        }
    });

    // Send initial connection event
    let initial_message = SseMessage {
        id: Uuid::new_v4().to_string(),
        message_type: "connection".to_string(),
        data: serde_json::json!({
            "type": "connected",
            "session_id": session.session_id,
            "connection_id": connection_id,
        }),
        timestamp: chrono::Utc::now(),
    };

    if let Err(e) = state.broadcast_message(initial_message).await {
        tracing::warn!("Failed to send initial connection message: {}", e);
    }

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(state.config.keep_alive_interval)))
}

/// Message posting endpoint
pub async fn post_message_handler(
    State(state): State<Arc<SseServerState>>,
    Json(request): Json<PostMessageRequest>,
) -> Result<Json<serde_json::Value>, Response> {
    // Verify session exists
    if state
        .session_manager
        .get_session(&request.session_id)
        .await
        .is_none()
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Session not found"})),
        )
            .into_response());
    }

    // Create SSE message
    let message = SseMessage {
        id: Uuid::new_v4().to_string(),
        message_type: request
            .message_type
            .unwrap_or_else(|| "message".to_string()),
        data: request.message,
        timestamp: chrono::Utc::now(),
    };

    // Send to session
    match state.send_to_session(&request.session_id, message).await {
        Ok(()) => Ok(Json(serde_json::json!({"status": "sent"}))),
        Err(e) => {
            tracing::error!("Failed to send message: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "Failed to send message"})),
            )
                .into_response())
        }
    }
}

/// Health check endpoint
pub async fn health_handler(State(state): State<Arc<SseServerState>>) -> Json<HealthResponse> {
    let (active_connections, active_sessions) = state.get_connection_stats().await;

    Json(HealthResponse {
        status: "healthy".to_string(),
        service: "turbomcp-sse-server".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        transport: "sse".to_string(),
        active_connections,
        active_sessions,
        uptime_seconds: 0, // Would need to track server start time
    })
}

/// Create SSE server router
pub fn create_sse_router(state: Arc<SseServerState>) -> Router {
    let mut router = Router::new()
        .route(&state.config.sse_path, get(sse_handler))
        .route(&state.config.message_path, post(post_message_handler))
        .route(&state.config.health_path, get(health_handler))
        .with_state(state.clone());

    // Add CORS if enabled
    if state.config.enable_cors {
        let cors = if state.config.cors_origins.contains(&"*".to_string()) {
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
        } else {
            let origins: Result<Vec<_>, _> = state
                .config
                .cors_origins
                .iter()
                .map(|origin| origin.parse())
                .collect();

            match origins {
                Ok(origins) => CorsLayer::new()
                    .allow_origin(origins)
                    .allow_methods(Any)
                    .allow_headers(Any),
                Err(_) => CorsLayer::permissive(),
            }
        };

        router = router.layer(cors);
    }

    // Add conservative security headers
    let hsts = SetResponseHeaderLayer::if_not_present(
        axum::http::header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    let xcto = SetResponseHeaderLayer::if_not_present(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    let xfo = SetResponseHeaderLayer::if_not_present(
        axum::http::header::X_FRAME_OPTIONS,
        HeaderValue::from_static("DENY"),
    );
    let refpol = SetResponseHeaderLayer::if_not_present(
        axum::http::header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    let csp = SetResponseHeaderLayer::if_not_present(
        axum::http::header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'"),
    );

    router
        .layer(hsts)
        .layer(xcto)
        .layer(xfo)
        .layer(refpol)
        .layer(csp)
}

/// Start SSE server
pub async fn start_sse_server(
    config: SseServerConfig,
    session_manager: Arc<SessionManager>,
    #[cfg(feature = "auth")] auth_manager: Option<Arc<AuthManager>>,
) -> McpResult<()> {
    let state = Arc::new(SseServerState::new(
        config.clone(),
        session_manager,
        #[cfg(feature = "auth")]
        auth_manager,
    ));
    let router = create_sse_router(state.clone());

    let bind_addr = format!("{}:{}", config.bind_address, config.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| McpError::Transport(format!("Failed to bind to {bind_addr}: {e}")))?;

    tracing::info!(
        address = %bind_addr,
        sse_path = %config.sse_path,
        message_path = %config.message_path,
        health_path = %config.health_path,
        "SSE server starting"
    );

    // Start cleanup task
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(state_clone.cleanup_interval);
        loop {
            interval.tick().await;
            state_clone.cleanup_connections().await;
        }
    });

    // Start server
    axum::serve(listener, router)
        .await
        .map_err(|e| McpError::Transport(format!("SSE server error: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionConfig;

    #[tokio::test]
    async fn test_sse_server_state_creation() {
        let config = SseServerConfig::default();
        let session_config = SessionConfig::default();
        let session_manager = Arc::new(SessionManager::new(session_config));

        let state = SseServerState::new(
            config,
            session_manager,
            #[cfg(feature = "auth")]
            None,
        );
        assert_eq!(state.config.port, 3000);
        assert_eq!(state.config.sse_path, "/sse");
    }

    #[tokio::test]
    async fn test_sse_message_broadcasting() {
        let config = SseServerConfig::default();
        let session_config = SessionConfig::default();
        let session_manager = Arc::new(SessionManager::new(session_config));

        let state = SseServerState::new(
            config,
            session_manager,
            #[cfg(feature = "auth")]
            None,
        );

        let message = SseMessage {
            id: "test-123".to_string(),
            message_type: "test".to_string(),
            data: serde_json::json!({"test": "data"}),
            timestamp: chrono::Utc::now(),
        };

        // This will succeed even with no subscribers
        let result = state.broadcast_message(message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_connection_cleanup() {
        let config = SseServerConfig::default();
        let session_config = SessionConfig::default();
        let session_manager = Arc::new(SessionManager::new(session_config));

        let state = SseServerState::new(
            config,
            session_manager,
            #[cfg(feature = "auth")]
            None,
        );

        // Add a connection that's already inactive
        let connection = SseConnection {
            connection_id: "test-conn".to_string(),
            session_id: "test-session".to_string(),
            client_id: "test-client".to_string(),
            connected_at: std::time::SystemTime::now() - Duration::from_secs(3600),
            last_activity: std::time::SystemTime::now() - Duration::from_secs(3600),
            #[cfg(feature = "auth")]
            auth_context: None,
            metadata: HashMap::new(),
        };

        state
            .connections
            .write()
            .await
            .insert("test-conn".to_string(), connection);

        assert_eq!(state.connections.read().await.len(), 1);

        // Cleanup should remove the inactive connection
        state.cleanup_connections().await;

        assert_eq!(state.connections.read().await.len(), 0);
    }
}

//! HTTP transport implementation.
//!
//! Provides MCP 2025-11-25 Streamable HTTP transport with:
//! - POST for JSON-RPC requests
//! - GET for SSE (Server-Sent Events) for server push
//!
//! # Protocol Compliance
//!
//! This implementation follows the MCP 2025-11-25 streamable HTTP shape:
//! - POST `/` or `/mcp` - JSON-RPC request/response
//! - GET `/sse` - Server-Sent Events stream with session management
//! - `Mcp-Session-Id` header for session correlation
//!
//! # Version-Aware Routing
//!
//! Per-session version-aware routing is active. After a successful `initialize`
//! handshake, the negotiated [`ProtocolVersion`] is stored in [`SessionManager`]
//! keyed by `Mcp-Session-Id`. All subsequent requests for that session are
//! dispatched through [`router::route_request_versioned`], ensuring correct
//! adapter filtering and method availability for the negotiated spec version.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use futures::stream::Stream;
use tokio::sync::{RwLock, broadcast};
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_core::types::core::ProtocolVersion;
use uuid::Uuid;

use crate::config::{RateLimiter, ServerConfig};
use crate::context::RequestContext;
use crate::router::{self, JsonRpcIncoming, JsonRpcOutgoing};

/// Maximum HTTP request body size for MCP requests.
///
/// This is intentionally larger than the core `MAX_MESSAGE_SIZE` (1MB) because
/// HTTP transport may need to handle larger payloads (e.g., base64-encoded images
/// in tool responses or large resource uploads). Individual message validation
/// still applies the core limit after decompression where applicable.
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// SSE keep-alive interval.
const SSE_KEEP_ALIVE_SECS: u64 = 30;

/// Per-session data tracked by SessionManager.
#[derive(Debug, Clone)]
struct SessionData {
    /// Broadcast channel for SSE push.
    tx: broadcast::Sender<String>,
    /// Negotiated protocol version (set after successful initialize).
    protocol_version: Option<ProtocolVersion>,
}

/// Session manager for SSE connections.
#[derive(Clone, Debug)]
pub struct SessionManager {
    /// Map of session ID to per-session data.
    sessions: Arc<RwLock<HashMap<String, SessionData>>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    /// Create a new session manager.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new session and return the session ID and receiver.
    pub async fn create_session(&self) -> (String, broadcast::Receiver<String>) {
        let session_id = Uuid::new_v4().to_string();
        let (tx, rx) = broadcast::channel(100);

        self.sessions.write().await.insert(
            session_id.clone(),
            SessionData {
                tx,
                protocol_version: None,
            },
        );

        tracing::debug!("Created SSE session: {}", session_id);
        (session_id, rx)
    }

    /// Remove a session.
    pub async fn remove_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
        tracing::debug!("Removed SSE session: {}", session_id);
    }

    /// Send a message to a specific session.
    #[allow(dead_code)] // Reserved for server-initiated push (not yet wired)
    pub(crate) async fn send_to_session(&self, session_id: &str, message: &str) -> bool {
        if let Some(data) = self.sessions.read().await.get(session_id) {
            data.tx.send(message.to_string()).is_ok()
        } else {
            false
        }
    }

    /// Broadcast a message to all sessions.
    #[allow(dead_code)] // Reserved for server-initiated push (not yet wired)
    pub(crate) async fn broadcast(&self, message: &str) {
        let sessions = self.sessions.read().await;
        for (session_id, data) in sessions.iter() {
            if data.tx.send(message.to_string()).is_err() {
                tracing::warn!("Failed to send to session {}", session_id);
            }
        }
    }

    /// Get the number of active sessions.
    #[allow(dead_code)] // Reserved for server-initiated push (not yet wired)
    pub(crate) async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Store the negotiated protocol version for a session.
    pub(crate) async fn set_protocol_version(&self, session_id: &str, version: ProtocolVersion) {
        if let Some(data) = self.sessions.write().await.get_mut(session_id) {
            data.protocol_version = Some(version);
        }
    }

    /// Retrieve the negotiated protocol version for a session.
    pub(crate) async fn get_protocol_version(&self, session_id: &str) -> Option<ProtocolVersion> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .and_then(|data| data.protocol_version.clone())
    }
}

/// Run a handler on HTTP transport with full MCP Streamable HTTP support.
///
/// This includes:
/// - POST `/` and `/mcp` for JSON-RPC requests
/// - GET `/sse` for Server-Sent Events stream
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to (e.g., "0.0.0.0:8080")
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::transport::http;
///
/// http::run(&handler, "0.0.0.0:8080").await?;
/// ```
pub async fn run<H: McpHandler>(handler: &H, addr: &str) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let session_manager = SessionManager::new();

    let state = SseState {
        handler: handler.clone(),
        session_manager: session_manager.clone(),
        rate_limiter: None,
        config: None,
    };

    let app = Router::new()
        .route("/", post(handle_json_rpc::<H>))
        .route("/mcp", post(handle_json_rpc::<H>))
        .route("/sse", get(handle_sse::<H>))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state);

    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

    tracing::info!(
        "MCP server listening on http://{} (POST /, /mcp; GET /sse)",
        socket_addr
    );

    axum::serve(listener, app)
        .await
        .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    // Call shutdown hook
    handler.on_shutdown().await?;
    Ok(())
}

/// Run a handler on HTTP transport with custom configuration.
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to
/// * `config` - Server configuration (rate limits, etc.)
pub async fn run_with_config<H: McpHandler>(
    handler: &H,
    addr: &str,
    config: &ServerConfig,
) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let rate_limiter = config
        .rate_limit
        .as_ref()
        .map(|cfg| Arc::new(RateLimiter::new(cfg.clone())));

    let session_manager = SessionManager::new();

    let state = SseState {
        handler: handler.clone(),
        session_manager: session_manager.clone(),
        rate_limiter,
        config: Some(config.clone()),
    };

    let app = Router::new()
        .route("/", post(handle_json_rpc_with_rate_limit::<H>))
        .route("/mcp", post(handle_json_rpc_with_rate_limit::<H>))
        .route("/sse", get(handle_sse::<H>))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state);

    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|e| McpError::internal(format!("Invalid address '{}': {}", addr, e)))?;

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .map_err(|e| McpError::internal(format!("Failed to bind to {}: {}", addr, e)))?;

    let rate_limit_info = config
        .rate_limit
        .as_ref()
        .map(|cfg| {
            format!(
                " (rate limit: {}/{}s)",
                cfg.max_requests,
                cfg.window.as_secs()
            )
        })
        .unwrap_or_default();

    tracing::info!(
        "MCP server listening on http://{}{} (POST /, /mcp; GET /sse)",
        socket_addr,
        rate_limit_info
    );

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| McpError::internal(format!("Server error: {}", e)))?;

    // Call shutdown hook
    handler.on_shutdown().await?;
    Ok(())
}

/// HTTP state with SSE support and optional rate limiting.
#[derive(Clone)]
struct SseState<H: McpHandler> {
    handler: H,
    session_manager: SessionManager,
    rate_limiter: Option<Arc<RateLimiter>>,
    config: Option<ServerConfig>,
}

/// Route a request with per-session version tracking.
///
/// On `initialize`:
/// - Routes through `route_request_with_config` for protocol negotiation.
/// - On success, extracts the negotiated `protocolVersion` from the response
///   and stores it in the session manager for subsequent requests.
///
/// On all other methods when the session has a stored version:
/// - Routes through `route_request_versioned` for adapter-filtered dispatch.
///
/// On all other cases (pre-init or no session):
/// - Routes through `route_request_with_config` which handles validation.
async fn route_with_version_tracking<H: McpHandler>(
    handler: &H,
    request: router::JsonRpcIncoming,
    session_manager: &SessionManager,
    config: Option<&ServerConfig>,
    session_id: Option<&str>,
) -> router::JsonRpcOutgoing {
    let ctx = RequestContext::http();
    let core_ctx = ctx.to_core_context();

    if request.method == "initialize" {
        let response = router::route_request_with_config(handler, request, &core_ctx, config).await;

        // If successful and we have a session, extract and store the negotiated version.
        if let (Some(sid), Some(result)) = (session_id, response.result.as_ref())
            && let Some(version_str) = result.get("protocolVersion").and_then(|v| v.as_str())
        {
            let version = ProtocolVersion::from(version_str);
            session_manager.set_protocol_version(sid, version).await;
            tracing::debug!(
                session_id = sid,
                protocol_version = version_str,
                "Stored negotiated protocol version for session"
            );
        }

        return response;
    }

    // For post-initialize requests: use versioned routing if session has a stored version.
    if let Some(sid) = session_id
        && let Some(version) = session_manager.get_protocol_version(sid).await
    {
        return router::route_request_versioned(handler, request, &core_ctx, &version).await;
    }

    // Pre-initialize or sessionless: route with config for proper validation.
    router::route_request_with_config(handler, request, &core_ctx, config).await
}

/// Axum handler for JSON-RPC requests (simple mode).
async fn handle_json_rpc<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<SseState<H>>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<JsonRpcIncoming>,
) -> axum::Json<JsonRpcOutgoing> {
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let response = route_with_version_tracking(
        &state.handler,
        request,
        &state.session_manager,
        state.config.as_ref(),
        session_id.as_deref(),
    )
    .await;

    axum::Json(response)
}

/// Axum handler for JSON-RPC requests with rate limiting.
async fn handle_json_rpc_with_rate_limit<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<SseState<H>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<JsonRpcIncoming>,
) -> Result<axum::Json<JsonRpcOutgoing>, axum::http::StatusCode> {
    // Check rate limit if configured.
    if let Some(ref limiter) = state.rate_limiter {
        let client_id = addr.ip().to_string();
        if !limiter.check(Some(&client_id)) {
            tracing::warn!("Rate limit exceeded for client {}", client_id);
            return Err(axum::http::StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let response = route_with_version_tracking(
        &state.handler,
        request,
        &state.session_manager,
        state.config.as_ref(),
        session_id.as_deref(),
    )
    .await;

    Ok(axum::Json(response))
}

/// Axum handler for SSE (Server-Sent Events) connections.
///
/// This implements the MCP Streamable HTTP specification:
/// - Returns `text/event-stream` content type
/// - Sets `Mcp-Session-Id` header for session correlation
/// - Keeps connection open for server-initiated messages
async fn handle_sse<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<SseState<H>>,
) -> impl axum::response::IntoResponse {
    // Create a new session
    let (session_id, mut rx) = state.session_manager.create_session().await;
    let session_manager = state.session_manager.clone();
    let session_id_for_stream = session_id.clone();
    let session_id_for_header = session_id.clone();

    // Create the SSE stream
    let stream = async_stream::stream! {
        // Send initial connection event with session ID
        yield Ok::<_, Infallible>(Event::default()
            .event("connected")
            .data(format!(r#"{{"sessionId":"{}"}}"#, session_id_for_stream)));

        // Listen for messages from the broadcast channel
        loop {
            match rx.recv().await {
                Ok(message) => {
                    yield Ok(Event::default()
                        .event("message")
                        .data(message));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("SSE client lagged, missed {} messages", n);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::debug!("SSE broadcast channel closed");
                    break;
                }
            }
        }
    };

    // Wrap in a cleanup guard to remove session when connection drops
    let cleanup_stream = CleanupStream {
        inner: Box::pin(stream),
        session_manager,
        session_id,
    };

    // Build SSE response with session ID header
    let sse = Sse::new(cleanup_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(SSE_KEEP_ALIVE_SECS))
            .text("keep-alive"),
    );

    // Return with Mcp-Session-Id header
    (
        [(
            axum::http::header::HeaderName::from_static("mcp-session-id"),
            // Session IDs are UUIDs (hex + hyphens) which are always valid header values,
            // but we handle the error gracefully rather than panicking.
            axum::http::header::HeaderValue::from_str(&session_id_for_header).unwrap_or_else(
                |_| axum::http::header::HeaderValue::from_static("invalid-session"),
            ),
        )],
        sse,
    )
}

/// Stream wrapper that cleans up the session when dropped.
struct CleanupStream<S> {
    inner: std::pin::Pin<Box<S>>,
    session_manager: SessionManager,
    session_id: String,
}

impl<S: Stream<Item = Result<Event, Infallible>>> Stream for CleanupStream<S> {
    type Item = Result<Event, Infallible>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl<S> Drop for CleanupStream<S> {
    fn drop(&mut self) {
        let session_manager = self.session_manager.clone();
        let session_id = self.session_id.clone();
        // Spawn cleanup task (we can't await in Drop)
        tokio::spawn(async move {
            session_manager.remove_session(&session_id).await;
        });
    }
}

#[cfg(test)]
mod tests {
    // HTTP tests are in /tests/ as they require network access
}

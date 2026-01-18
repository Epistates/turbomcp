//! HTTP transport implementation.
//!
//! Provides MCP 2025-06-18 Streamable HTTP transport with:
//! - POST for JSON-RPC requests
//! - GET for SSE (Server-Sent Events) for server push
//!
//! # Protocol Compliance
//!
//! This implementation follows the MCP 2025-06-18 specification:
//! - POST `/` or `/mcp` - JSON-RPC request/response
//! - GET `/sse` - Server-Sent Events stream with session management
//! - `Mcp-Session-Id` header for session correlation

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
use tokio::sync::{broadcast, RwLock};
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use uuid::Uuid;

use crate::config::{RateLimiter, ServerConfig};
use crate::context::RequestContext;
use crate::router::{self, JsonRpcIncoming, JsonRpcOutgoing};

/// Maximum request body size (10MB).
const MAX_BODY_SIZE: usize = 10 * 1024 * 1024;

/// SSE keep-alive interval.
const SSE_KEEP_ALIVE_SECS: u64 = 30;

/// Session manager for SSE connections.
#[derive(Clone, Debug)]
pub struct SessionManager {
    /// Map of session ID to broadcast sender for pushing events
    sessions: Arc<RwLock<HashMap<String, broadcast::Sender<String>>>>,
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

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), tx);

        tracing::debug!("Created SSE session: {}", session_id);
        (session_id, rx)
    }

    /// Remove a session.
    pub async fn remove_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
        tracing::debug!("Removed SSE session: {}", session_id);
    }

    /// Send a message to a specific session.
    #[allow(dead_code)]
    pub async fn send_to_session(&self, session_id: &str, message: &str) -> bool {
        if let Some(tx) = self.sessions.read().await.get(session_id) {
            tx.send(message.to_string()).is_ok()
        } else {
            false
        }
    }

    /// Broadcast a message to all sessions.
    #[allow(dead_code)]
    pub async fn broadcast(&self, message: &str) {
        let sessions = self.sessions.read().await;
        for (session_id, tx) in sessions.iter() {
            if tx.send(message.to_string()).is_err() {
                tracing::warn!("Failed to send to session {}", session_id);
            }
        }
    }

    /// Get the number of active sessions.
    #[allow(dead_code)]
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

/// Run a handler on HTTP transport with full MCP 2025-06-18 Streamable HTTP support.
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
}


/// Axum handler for JSON-RPC requests (simple mode).
async fn handle_json_rpc<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<SseState<H>>,
    axum::Json(request): axum::Json<JsonRpcIncoming>,
) -> axum::Json<JsonRpcOutgoing> {
    let ctx = RequestContext::http();
    let core_ctx = ctx.to_core_context();
    let response = router::route_request(&state.handler, request, &core_ctx).await;
    axum::Json(response)
}

/// Axum handler for JSON-RPC requests with rate limiting.
async fn handle_json_rpc_with_rate_limit<H: McpHandler>(
    axum::extract::State(state): axum::extract::State<SseState<H>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
    axum::Json(request): axum::Json<JsonRpcIncoming>,
) -> Result<axum::Json<JsonRpcOutgoing>, axum::http::StatusCode> {
    // Check rate limit if configured
    if let Some(ref limiter) = state.rate_limiter {
        let client_id = addr.ip().to_string();
        if !limiter.check(Some(&client_id)) {
            tracing::warn!("Rate limit exceeded for client {}", client_id);
            return Err(axum::http::StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let ctx = RequestContext::http();
    let core_ctx = ctx.to_core_context();
    let response = router::route_request(&state.handler, request, &core_ctx).await;
    Ok(axum::Json(response))
}

/// Axum handler for SSE (Server-Sent Events) connections.
///
/// This implements the MCP 2025-06-18 Streamable HTTP specification:
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
            axum::http::header::HeaderValue::from_str(&session_id_for_header).unwrap(),
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

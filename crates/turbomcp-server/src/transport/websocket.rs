//! WebSocket transport implementation.
//!
//! Provides bidirectional JSON-RPC over WebSocket using Axum.
//!
//! # Per-Connection Version-Aware Routing
//!
//! Each WebSocket connection maintains its own `SessionState`, mirroring the
//! lifecycle enforcement already present in the STDIO, TCP, and Unix transports:
//! - `initialize` must succeed before any other method is accepted.
//! - Duplicate `initialize` requests are rejected.
//! - Post-initialize requests are routed through `route_request_versioned`,
//!   which applies the negotiated `ProtocolVersion` adapter for response filtering.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_core::types::core::ProtocolVersion;

use super::SessionState;
use crate::config::{ConnectionCounter, RateLimiter, ServerConfig};
use crate::context::RequestContext;
use crate::router::{self, JsonRpcOutgoing};

/// Maximum WebSocket message size (10MB).
const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Run a handler on WebSocket transport.
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to (e.g., "0.0.0.0:8080")
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_server::transport::websocket;
///
/// websocket::run(&handler, "0.0.0.0:8080").await?;
/// ```
pub async fn run<H: McpHandler>(handler: &H, addr: &str) -> McpResult<()> {
    run_with_config(handler, addr, &ServerConfig::default()).await
}

/// Run a handler on WebSocket transport with custom configuration.
///
/// # Arguments
///
/// * `handler` - The MCP handler
/// * `addr` - Address to bind to
/// * `config` - Server configuration (rate limits, connection limits, etc.)
pub async fn run_with_config<H: McpHandler>(
    handler: &H,
    addr: &str,
    config: &ServerConfig,
) -> McpResult<()> {
    // Call lifecycle hooks
    handler.on_initialize().await?;

    let max_connections = config.connection_limits.max_websocket_connections;
    let connection_counter = Arc::new(ConnectionCounter::new(max_connections));

    let rate_limiter = config
        .rate_limit
        .as_ref()
        .map(|cfg| Arc::new(RateLimiter::new(cfg.clone())));

    let state = WebSocketState {
        handler: handler.clone(),
        rate_limiter,
        connection_counter: connection_counter.clone(),
        config: Some(config.clone()),
    };

    let app = Router::new()
        .route("/", get(ws_upgrade_handler::<H>))
        .route("/ws", get(ws_upgrade_handler::<H>))
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
                ", rate limit: {}/{}s",
                cfg.max_requests,
                cfg.window.as_secs()
            )
        })
        .unwrap_or_default();

    tracing::info!(
        "MCP WebSocket server listening on ws://{} (max {} connections{})",
        socket_addr,
        max_connections,
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

/// WebSocket state with rate and connection limiting.
#[derive(Clone)]
struct WebSocketState<H: McpHandler> {
    handler: H,
    rate_limiter: Option<Arc<RateLimiter>>,
    connection_counter: Arc<ConnectionCounter>,
    config: Option<ServerConfig>,
}

/// Axum handler for WebSocket upgrade.
async fn ws_upgrade_handler<H: McpHandler>(
    ws: WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<WebSocketState<H>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    // Check connection limit
    let guard = match state.connection_counter.try_acquire_arc() {
        Some(guard) => guard,
        None => {
            tracing::warn!(
                "WebSocket connection from {} rejected: at capacity ({}/{})",
                addr,
                state.connection_counter.current(),
                state.connection_counter.max()
            );
            return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
        }
    };

    // Check rate limit on connection
    if let Some(ref limiter) = state.rate_limiter {
        let client_id = addr.ip().to_string();
        if !limiter.check(Some(&client_id)) {
            tracing::warn!("Rate limit exceeded for WebSocket client {}", client_id);
            return Err(axum::http::StatusCode::TOO_MANY_REQUESTS);
        }
    }

    tracing::debug!(
        "New WebSocket connection from {} ({}/{})",
        addr,
        state.connection_counter.current(),
        state.connection_counter.max()
    );

    let handler = state.handler.clone();
    let rate_limiter = state.rate_limiter.clone();
    let config = state.config.clone();
    let client_addr = addr;

    Ok(ws.on_upgrade(move |socket| {
        handle_websocket(socket, handler, rate_limiter, client_addr, guard, config)
    }))
}

/// Handle a WebSocket connection with per-connection MCP session lifecycle enforcement.
///
/// Each connection starts `Uninitialized`. The client must send `initialize`
/// before any other method. On success the negotiated `ProtocolVersion` is
/// stored and subsequent requests are routed through `route_request_versioned`
/// so the version adapter filters responses appropriately.
async fn handle_websocket<H: McpHandler>(
    socket: WebSocket,
    handler: H,
    rate_limiter: Option<Arc<RateLimiter>>,
    client_addr: SocketAddr,
    _connection_guard: crate::config::ConnectionGuard,
    config: Option<ServerConfig>,
) {
    let client_id = client_addr.ip().to_string();
    let (mut sender, mut receiver) = socket.split();

    // Per-connection MCP session lifecycle state.
    let mut session_state = SessionState::Uninitialized;

    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!("WebSocket receive error: {}", e);
                break;
            }
        };

        // Extract text from message
        let text = match extract_text(msg) {
            Some(text) => text,
            None => continue, // Skip non-text messages
        };

        // Check message size
        if text.len() > MAX_MESSAGE_SIZE {
            tracing::warn!(
                "WebSocket message exceeds size limit ({} > {})",
                text.len(),
                MAX_MESSAGE_SIZE
            );
            continue;
        }

        // Check per-message rate limit
        if let Some(ref limiter) = rate_limiter
            && !limiter.check(Some(&client_id))
        {
            tracing::warn!(
                "Rate limit exceeded for WebSocket message from {}",
                client_id
            );
            let error = JsonRpcOutgoing::error(None, McpError::rate_limited("Rate limit exceeded"));
            if let Ok(response_str) = router::serialize_response(&error) {
                let _ = sender.send(Message::Text(response_str.into())).await;
            }
            continue;
        }

        // Parse and route with lifecycle-aware dispatch.
        let ctx = RequestContext::websocket();
        let core_ctx = ctx.to_core_context();

        match router::parse_request(&text) {
            Ok(request) => {
                let response = if request.method == "initialize" {
                    // Reject duplicate initialize per MCP spec.
                    if matches!(session_state, SessionState::Initialized(_)) {
                        JsonRpcOutgoing::error(
                            request.id.clone(),
                            McpError::invalid_request("Session already initialized"),
                        )
                    } else {
                        // Route initialize through config-aware handler so protocol
                        // negotiation and capability validation are applied.
                        let initialize_request_id = request.id.clone();
                        let resp = router::route_request_with_config(
                            &handler,
                            request,
                            &core_ctx,
                            config.as_ref(),
                        )
                        .await;

                        // Extract the negotiated version from a successful response.
                        // On failure (error response) the session stays Uninitialized
                        // and subsequent non-init requests will be rejected.
                        if let Some(ref result) = resp.result
                            && let Some(v) = result.get("protocolVersion").and_then(|v| v.as_str())
                        {
                            let version = ProtocolVersion::from(v);
                            tracing::info!(
                                version = %version,
                                client = %client_addr,
                                "Protocol version negotiated"
                            );
                            session_state =
                                SessionState::Initialized(super::InitializedSessionState::new(
                                    version,
                                    initialize_request_id.as_ref(),
                                ));
                        }

                        resp
                    }
                } else if request.method == "notifications/initialized"
                    || request.method == "notifications/cancelled"
                {
                    // Lifecycle notifications pass through unconditionally —
                    // they carry no id and produce no sendable response.
                    router::route_request(&handler, request, &core_ctx).await
                } else {
                    // All other requests require a completed initialize handshake.
                    // Notifications (id=None) MUST NOT receive responses per
                    // JSON-RPC 2.0, so synthesize a drop-able ack instead of
                    // an error that would get serialized to the peer.
                    let is_notification = request.id.is_none();
                    match &mut session_state {
                        SessionState::Initialized(session) => {
                            if !session.register_request_id(request.id.as_ref()) {
                                if is_notification {
                                    JsonRpcOutgoing::notification_ack()
                                } else {
                                    JsonRpcOutgoing::error(
                                        request.id.clone(),
                                        McpError::invalid_request(
                                            "Request ID already used in this session",
                                        ),
                                    )
                                }
                            } else {
                                let version = session.protocol_version().clone();
                                router::route_request_versioned(
                                    &handler, request, &core_ctx, &version,
                                )
                                .await
                            }
                        }
                        SessionState::Uninitialized => {
                            if is_notification {
                                JsonRpcOutgoing::notification_ack()
                            } else {
                                JsonRpcOutgoing::error(
                                    request.id.clone(),
                                    McpError::invalid_request(
                                        "Server not initialized. Send 'initialize' first.",
                                    ),
                                )
                            }
                        }
                    }
                };

                if response.should_send()
                    && let Ok(response_str) = router::serialize_response(&response)
                    && sender
                        .send(Message::Text(response_str.into()))
                        .await
                        .is_err()
                {
                    tracing::error!("Failed to send WebSocket response");
                    break;
                }
            }
            Err(e) => {
                let error = JsonRpcOutgoing::error(None, McpError::parse_error(e.to_string()));
                if let Ok(error_str) = router::serialize_response(&error) {
                    let _ = sender.send(Message::Text(error_str.into())).await;
                }
            }
        }
    }
}

/// Extract text from a WebSocket message.
fn extract_text(msg: Message) -> Option<String> {
    match msg {
        Message::Text(text) => Some(text.to_string()),
        Message::Binary(data) => String::from_utf8(data.to_vec()).ok(),
        Message::Ping(_) | Message::Pong(_) | Message::Close(_) => None,
    }
}

#[cfg(test)]
mod tests {
    // WebSocket tests are in /tests/ as they require network access
}

//! WebSocket transport implementation.
//!
//! Provides bidirectional JSON-RPC over WebSocket using Axum.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;

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
    let client_addr = addr;

    Ok(ws.on_upgrade(move |socket| {
        handle_websocket(socket, handler, rate_limiter, client_addr, guard)
    }))
}

/// Handle a WebSocket connection.
async fn handle_websocket<H: McpHandler>(
    socket: WebSocket,
    handler: H,
    rate_limiter: Option<Arc<RateLimiter>>,
    client_addr: SocketAddr,
    _connection_guard: crate::config::ConnectionGuard,
) {
    let client_id = client_addr.ip().to_string();
    let (mut sender, mut receiver) = socket.split();

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

        // Parse and route
        let ctx = RequestContext::websocket();
        let core_ctx = ctx.to_core_context();

        match router::parse_request(&text) {
            Ok(request) => {
                let response = router::route_request(&handler, request, &core_ctx).await;
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

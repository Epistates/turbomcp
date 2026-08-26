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
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_types::ProtocolVersion;

use super::SessionState;
use crate::config::{ConnectionCounter, RateLimiter, ServerConfig};
use crate::context::{Cancellable, RequestContext};
use crate::router::{self, JsonRpcOutgoing};
use crate::transport::line::jsonrpc_id_key;
use turbomcp_transport::security::{
    OriginConfig, SecurityHeaders, extract_client_ip_with_trust, validate_origin,
};

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
        .route("/mcp/ws", get(ws_upgrade_handler::<H>))
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
    headers: HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
) -> Result<impl axum::response::IntoResponse, axum::http::StatusCode> {
    validate_websocket_origin(&headers, addr, state.config.as_ref())?;

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

fn to_security_headers(headers: &HeaderMap) -> SecurityHeaders {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

fn websocket_origin_config(config: Option<&ServerConfig>) -> OriginConfig {
    let Some(config) = config else {
        return OriginConfig::default();
    };

    OriginConfig {
        allowed_origins: config.origin_validation.allowed_origins.clone(),
        allow_localhost: config.origin_validation.allow_localhost,
        allow_any: config.origin_validation.allow_any,
    }
}

fn validate_websocket_origin(
    headers: &HeaderMap,
    peer_addr: SocketAddr,
    config: Option<&ServerConfig>,
) -> Result<(), StatusCode> {
    let security_headers = to_security_headers(headers);
    let trusted = config
        .map(|config| config.origin_validation.trusted_proxies.as_slice())
        .unwrap_or(&[]);
    let client_ip = extract_client_ip_with_trust(&security_headers, peer_addr.ip(), trusted);
    let origin_config = websocket_origin_config(config);

    validate_origin(&origin_config, &security_headers, client_ip).map_err(|error| {
        tracing::warn!(%error, "Rejected WebSocket upgrade with invalid origin");
        StatusCode::FORBIDDEN
    })
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
    let max_message_size = config
        .as_ref()
        .map_or(MAX_MESSAGE_SIZE, |config| config.max_message_size);
    let (mut sender, mut receiver) = socket.split();

    // Per-connection MCP session lifecycle state.
    let mut session_state = SessionState::Uninitialized;

    // Channel for handler responses produced by spawned tasks.
    let (response_tx, mut response_rx) = mpsc::channel::<JsonRpcOutgoing>(32);

    // In-flight handler cancellation tokens, keyed by JSON-RPC id; signalled
    // by `notifications/cancelled` per MCP 2025-11-25.
    let pending_handlers: Arc<DashMap<String, CancellationToken>> = Arc::new(DashMap::new());

    loop {
        tokio::select! {
            biased;

            // Outgoing: completed handler responses.
            Some(response) = response_rx.recv() => {
                if response.should_send()
                    && let Ok(response_str) = router::serialize_response(&response)
                    && sender.send(Message::Text(response_str.into())).await.is_err()
                {
                    tracing::error!("Failed to send WebSocket response");
                    break;
                }
                continue;
            }

            // Incoming: client → server frames.
            maybe_msg = receiver.next() => {
                let Some(msg) = maybe_msg else { break };
                let msg = match msg {
                    Ok(msg) => msg,
                    Err(e) => {
                        tracing::error!("WebSocket receive error: {}", e);
                        break;
                    }
                };

                let text = match extract_text(msg) {
                    Some(text) => text,
                    None => continue,
                };

                if text.len() > max_message_size {
                    tracing::warn!(
                        "WebSocket message exceeds size limit ({} > {})",
                        text.len(),
                        max_message_size
                    );
                    continue;
                }

                if let Some(ref limiter) = rate_limiter
                    && !limiter.check(Some(&client_id))
                {
                    tracing::warn!(
                        "Rate limit exceeded for WebSocket message from {}",
                        client_id
                    );
                    let error = JsonRpcOutgoing::error(
                        Some(serde_json::Value::Null),
                        McpError::rate_limited("Rate limit exceeded"),
                    );
                    if let Ok(response_str) = router::serialize_response(&error) {
                        let _ = sender.send(Message::Text(response_str.into())).await;
                    }
                    continue;
                }

                let parsed = match router::parse_request(&text) {
                    Ok(req) => req,
                    Err(e) => {
                        let error = JsonRpcOutgoing::error(
                            Some(serde_json::Value::Null),
                            McpError::parse_error(e.to_string()),
                        );
                        if let Ok(error_str) = router::serialize_response(&error) {
                            let _ = sender.send(Message::Text(error_str.into())).await;
                        }
                        continue;
                    }
                };

                // `initialize` mutates `session_state`, so it must run inline
                // on the loop task.
                if parsed.method == "initialize" {
                    let ctx = RequestContext::websocket();
                    let response = if matches!(session_state, SessionState::Initialized(_)) {
                        JsonRpcOutgoing::error(
                            parsed.id.clone(),
                            McpError::invalid_request("Session already initialized"),
                        )
                    } else {
                        let resp = router::route_request_with_config(
                            &handler,
                            parsed,
                            &ctx,
                            config.as_ref(),
                        )
                        .await;
                        if let Some(ref result) = resp.result
                            && let Some(v) =
                                result.get("protocolVersion").and_then(|v| v.as_str())
                        {
                            let version = ProtocolVersion::from(v);
                            tracing::info!(
                                version = %version,
                                client = %client_addr,
                                "Protocol version negotiated"
                            );
                            session_state = SessionState::Initialized(
                                super::InitializedSessionState::new(version),
                            );
                        }
                        resp
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
                    continue;
                }

                // `notifications/cancelled` is consumed inline: parse the
                // referenced request id and signal the matching handler.
                if parsed.method == "notifications/cancelled" {
                    if let Some(req_id) = parsed
                        .params
                        .as_ref()
                        .and_then(|p| p.get("requestId"))
                    {
                        let key = jsonrpc_id_key(req_id);
                        if let Some((_, token)) = pending_handlers.remove(&key) {
                            let reason = parsed
                                .params
                                .as_ref()
                                .and_then(|p| p.get("reason"))
                                .and_then(|r| r.as_str())
                                .unwrap_or("client requested cancellation");
                            tracing::debug!(
                                request_id = %key,
                                reason = %reason,
                                "Cancelling in-flight handler",
                            );
                            token.cancel();
                        }
                    }
                    continue;
                }

                // `notifications/initialized` is a lifecycle no-op (no id, no
                // response). Route it inline since there's nothing to spawn.
                if parsed.method == "notifications/initialized" {
                    let ctx = RequestContext::websocket();
                    let _ = router::route_request(&handler, parsed, &ctx).await;
                    continue;
                }

                if parsed.method == "ping"
                    && matches!(session_state, SessionState::Uninitialized)
                {
                    // Lifecycle permits ping before initialize has completed.
                    let ctx = RequestContext::websocket();
                    let response = router::route_request(&handler, parsed, &ctx).await;
                    if response.should_send()
                        && let Ok(response_str) = router::serialize_response(&response)
                        && sender.send(Message::Text(response_str.into())).await.is_err()
                    {
                        break;
                    }
                    continue;
                }

                // All other methods: enforce post-init gating, then spawn the
                // handler so the receive loop keeps draining (notably
                // `notifications/cancelled` from the same client).
                let is_notification = parsed.id.is_none();
                let version = match &mut session_state {
                    SessionState::Initialized(session) => session.protocol_version().clone(),
                    SessionState::Uninitialized => {
                        if !is_notification {
                            let error = JsonRpcOutgoing::error(
                                parsed.id.clone(),
                                McpError::invalid_request(
                                    "Server not initialized. Send 'initialize' first.",
                                ),
                            );
                            if let Ok(error_str) = router::serialize_response(&error)
                                && sender
                                    .send(Message::Text(error_str.into()))
                                    .await
                                    .is_err()
                            {
                                break;
                            }
                        }
                        continue;
                    }
                };

                let handler_clone = handler.clone();
                let resp_tx = response_tx.clone();
                let token = CancellationToken::new();
                let cancel_key = parsed.id.as_ref().map(jsonrpc_id_key);
                if let Some(ref key) = cancel_key
                    && pending_handlers.insert(key.clone(), token.clone()).is_some()
                {
                    // Sequential id reuse is fine and no longer rejected.
                    // *Concurrent* reuse is not: the client cannot match two
                    // responses carrying one id, and this overwrote the first
                    // handler's cancellation token. Report it rather than
                    // refusing to serve.
                    tracing::warn!(
                        request_id = %key,
                        "Request id reused while the first is still in flight",
                    );
                }
                let ctx = RequestContext::websocket()
                    .with_cancellation_token(Arc::new(token) as Arc<dyn Cancellable>);
                let guard = super::PendingHandlerGuard::new(
                    Arc::clone(&pending_handlers),
                    cancel_key,
                );

                tokio::spawn(async move {
                    // RAII cleanup runs on every exit path, including handler
                    // panic.
                    let _guard = guard;
                    let response = router::route_request_versioned(
                        &handler_clone, parsed, &ctx, &version,
                    )
                    .await;
                    let _ = resp_tx.send(response).await;
                });
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
    use super::*;
    use crate::config::OriginValidationConfig;
    use std::collections::HashSet;
    use std::net::{IpAddr, Ipv4Addr};

    fn loopback_peer() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5000)
    }

    #[test]
    fn websocket_origin_validation_rejects_disallowed_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", "https://evil.example".parse().unwrap());
        let config = ServerConfig::builder()
            .origin_validation(OriginValidationConfig {
                allowed_origins: HashSet::new(),
                allow_localhost: false,
                allow_any: false,
                trusted_proxies: Vec::new(),
            })
            .build();

        let result = validate_websocket_origin(&headers, loopback_peer(), Some(&config));

        assert_eq!(result, Err(StatusCode::FORBIDDEN));
    }

    #[test]
    fn websocket_origin_validation_accepts_allowlisted_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("origin", "https://app.example".parse().unwrap());
        let config = ServerConfig::builder()
            .origin_validation(OriginValidationConfig {
                allowed_origins: ["https://app.example".to_string()].into_iter().collect(),
                allow_localhost: false,
                allow_any: false,
                trusted_proxies: Vec::new(),
            })
            .build();

        let result = validate_websocket_origin(&headers, loopback_peer(), Some(&config));

        assert_eq!(result, Ok(()));
    }

    // WebSocket tests are in /tests/ as they require network access
}

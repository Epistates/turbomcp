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
use super::session::{
    ConnectionCleanup, ConnectionSession, OutboundRequests, SHUTDOWN_GRACE, SessionCommand,
    readable_id,
};
use crate::config::{ConnectionCounter, RateLimiter, ServerConfig};
use crate::context::{Cancellable, RequestContext};
use crate::router::{self, JsonRpcOutgoing};
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

    // The protocol-level limit sits above the configured one so that a
    // message moderately over it reaches the loop and is answered with a
    // JSON-RPC error. Left at tungstenite's 64 MiB default, the configured
    // limit only applied after that much had been buffered.
    let frame_limit = max_message_size(config.as_ref()).saturating_mul(2);

    Ok(ws
        .max_message_size(frame_limit)
        .max_frame_size(frame_limit)
        .on_upgrade(move |socket| {
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
    let max_message_size = max_message_size(config.as_ref());
    let (mut sender, mut receiver) = socket.split();

    // Per-connection MCP session lifecycle state.
    let mut session_state = SessionState::Uninitialized;

    // Channel for handler responses produced by spawned tasks.
    let (response_tx, mut response_rx) = mpsc::channel::<JsonRpcOutgoing>(32);

    // Server-to-client channel. Handlers hold the session via `ctx.session`;
    // this loop is the only writer to the socket.
    let (session, mut cmd_rx) = ConnectionSession::new();
    let session_handle = Arc::new(session);
    let new_ctx = || RequestContext::websocket().with_session_id(session_handle.id());

    // Requests this server has sent the client and not yet seen answered.
    let mut outbound = OutboundRequests::default();

    // In-flight handler cancellation tokens, keyed by JSON-RPC id; signalled
    // by `notifications/cancelled` per MCP 2025-11-25.
    let pending_handlers: Arc<DashMap<String, CancellationToken>> = Arc::new(DashMap::new());
    let _cleanup = ConnectionCleanup::new(&pending_handlers, &session_handle);

    loop {
        tokio::select! {
            biased;

            // Outgoing: server-to-client requests and notifications raised by
            // handlers through `ctx.sample()` / `elicit_*()` / `notify_client()`.
            //
            // Polled *before* completed responses, and deliberately so. A
            // handler emits these while it is still running, so they belong on
            // the wire ahead of the response that concludes it — the progress
            // utility in particular requires notifications to stop once an
            // operation completes, which a response-first order would violate.
            // Starvation is not a concern: this channel is bounded and only
            // in-flight handlers write to it.
            Some(cmd) = cmd_rx.recv() => {
                let Some(frame) = outbound.frame(cmd) else { continue };
                if send_json(&mut sender, &frame).await.is_err() {
                    tracing::error!("Failed to send server-to-client message");
                    break;
                }
            }

            // Outgoing: completed handler responses.
            Some(response) = response_rx.recv() => {
                if response.should_send() && send_outgoing(&mut sender, &response).await.is_err() {
                    tracing::error!("Failed to send WebSocket response");
                    break;
                }
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

                // Answered, not dropped: a request the client never hears
                // back about waits on its own timeout, if it has one.
                if text.len() > max_message_size {
                    tracing::warn!(
                        "WebSocket message exceeds size limit ({} > {})",
                        text.len(),
                        max_message_size
                    );
                    let error = JsonRpcOutgoing::error(
                        Some(serde_json::Value::Null),
                        McpError::invalid_request(format!(
                            "Message exceeds maximum size of {max_message_size} bytes"
                        )),
                    );
                    if send_outgoing(&mut sender, &error).await.is_err() {
                        break;
                    }
                    continue;
                }

                // Parse once as a generic JSON-RPC message: the frame may be a
                // *response* to something this server asked the client, not a
                // request. Treating those as requests would answer the client's
                // reply with a parse error.
                let value: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(e) => {
                        let error = JsonRpcOutgoing::error(
                            Some(serde_json::Value::Null),
                            McpError::parse_error(e.to_string()),
                        );
                        if send_outgoing(&mut sender, &error).await.is_err() {
                            break;
                        }
                        continue;
                    }
                };

                if outbound.resolve(&value) {
                    continue;
                }

                let id = readable_id(&value);

                if let Some(ref limiter) = rate_limiter
                    && !limiter.check(Some(&client_id))
                {
                    tracing::warn!(
                        "Rate limit exceeded for WebSocket message from {}",
                        client_id
                    );
                    // A notification takes no reply, not even this one.
                    if id.is_some() {
                        let error = JsonRpcOutgoing::error(
                            id,
                            McpError::rate_limited("Rate limit exceeded"),
                        );
                        if send_outgoing(&mut sender, &error).await.is_err() {
                            break;
                        }
                    }
                    continue;
                }

                let parsed = match router::parse_request_from_value(value) {
                    Ok(req) => req,
                    Err(e) => {
                        let error =
                            JsonRpcOutgoing::error(Some(id.unwrap_or(serde_json::Value::Null)), e);
                        if send_outgoing(&mut sender, &error).await.is_err() {
                            break;
                        }
                        continue;
                    }
                };

                // `initialize` mutates `session_state`, so it must run inline
                // on the loop task.
                if parsed.method == "initialize" {
                    let response = if matches!(session_state, SessionState::Initialized(_)) {
                        // Rejected outright. Its capabilities are not the
                        // session's: the server stays bound by what the
                        // successful handshake declared.
                        JsonRpcOutgoing::error(
                            parsed.id.clone(),
                            McpError::invalid_request("Session already initialized"),
                        )
                    } else {
                        let client_capabilities =
                            super::client_capabilities_from_initialize_params(parsed.params.as_ref());
                        let ctx = new_ctx().with_session(session_handle.clone());
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
                                super::InitializedSessionState::new(version.clone()),
                            );
                            session_handle
                                .set_initialized(client_capabilities, version)
                                .await;
                        }
                        resp
                    };
                    if response.should_send() && send_outgoing(&mut sender, &response).await.is_err()
                    {
                        tracing::error!("Failed to send WebSocket response");
                        break;
                    }
                    continue;
                }

                // `notifications/cancelled` is consumed inline: parse the
                // referenced request id and signal the matching handler.
                if parsed.method == "notifications/cancelled" {
                    super::cancel_pending_handler(&pending_handlers, parsed.params.as_ref());
                    continue;
                }

                // `notifications/initialized` is a lifecycle no-op (no id, no
                // response). Route it inline since there's nothing to spawn.
                if parsed.method == "notifications/initialized" {
                    let ctx = new_ctx().with_session(session_handle.clone());
                    let _ = router::route_request(&handler, parsed, &ctx).await;
                    continue;
                }

                if parsed.method == "ping"
                    && matches!(session_state, SessionState::Uninitialized)
                {
                    // Lifecycle permits ping before initialize has completed.
                    let ctx = new_ctx().with_session(session_handle.clone());
                    let response = router::route_request(&handler, parsed, &ctx).await;
                    if response.should_send() && send_outgoing(&mut sender, &response).await.is_err()
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
                            if send_outgoing(&mut sender, &error).await.is_err() {
                                break;
                            }
                        }
                        continue;
                    }
                };

                let handler_clone = handler.clone();
                let resp_tx = response_tx.clone();
                let (token, guard) =
                    super::register_pending_handler(&pending_handlers, parsed.id.as_ref());
                // Kept so the spawned task can tell whether it was cancelled
                // before publishing its result.
                let cancel_signal = token.clone();
                let ctx = new_ctx()
                    .with_session(session_handle.clone())
                    .with_cancellation_token(Arc::new(token) as Arc<dyn Cancellable>);

                tokio::spawn(async move {
                    // RAII cleanup runs on every exit path, including handler
                    // panic.
                    let _guard = guard;
                    let response =
                        super::route_catching_panics(&handler_clone, parsed, &ctx, &version).await;
                    // See the note in line.rs: a cancelled request is not
                    // answered, otherwise cancellation only stopped the await.
                    if cancel_signal.is_cancelled() {
                        return;
                    }
                    let _ = resp_tx.send(response).await;
                });
            }
        }
    }

    // The socket is closing. Handlers awaiting a client reply will never get
    // one, and whatever they still produce has nowhere to go; give them
    // `SHUTDOWN_GRACE` to finish their side effects, keeping the command
    // queue drained so none blocks on it.
    outbound.close();
    drop(response_tx);
    let drained = tokio::time::timeout(SHUTDOWN_GRACE, async {
        loop {
            tokio::select! {
                biased;
                Some(cmd) = cmd_rx.recv() => {
                    if let SessionCommand::Request { response_tx, .. } = cmd {
                        let _ = response_tx.send(Err(McpError::internal("Session closed")));
                    }
                }
                response = response_rx.recv() => {
                    if response.is_none() {
                        break;
                    }
                }
            }
        }
    })
    .await;
    if drained.is_err() {
        tracing::warn!(
            "Handlers still running {SHUTDOWN_GRACE:?} after the WebSocket closed; cancelling them"
        );
    }
}

type WsSender = futures::stream::SplitSink<WebSocket, Message>;

async fn send_json(sender: &mut WsSender, value: &serde_json::Value) -> Result<(), ()> {
    let text = serde_json::to_string(value).map_err(|_| ())?;
    sender
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

async fn send_outgoing(sender: &mut WsSender, response: &JsonRpcOutgoing) -> Result<(), ()> {
    let text = router::serialize_response(response).map_err(|_| ())?;
    sender
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

fn max_message_size(config: Option<&ServerConfig>) -> usize {
    config.map_or(MAX_MESSAGE_SIZE, |config| config.max_message_size)
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

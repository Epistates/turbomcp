//! In-process channel transport for zero-overhead MCP communication.
//!
//! This transport uses `tokio::sync::mpsc` channels to pass `TransportMessage`
//! values directly between a server and client in the same process. It eliminates
//! all line framing, string allocation, flushing, and redundant JSON parsing
//! that line-based transports (STDIO, TCP) incur.
//!
//! # Usage
//!
//! ```rust,ignore
//! use turbomcp_server::transport::channel;
//!
//! // One-liner: returns a connected client transport + server join handle
//! let (client_transport, server_handle) = channel::run_in_process(&handler).await?;
//! ```

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use turbomcp_core::error::{McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_types::ProtocolVersion;

use crate::config::ServerConfig;
use crate::context::{Cancellable, RequestContext};
use crate::router;
use crate::transport::session::{
    ConnectionCleanup, ConnectionSession, OutboundRequests, SHUTDOWN_GRACE, SessionCommand,
    readable_id,
};
use crate::transport::{MAX_MESSAGE_SIZE, SessionState};

use turbomcp_transport::{
    Transport, TransportCapabilities, TransportError, TransportMessage, TransportMetrics,
    TransportResult, TransportState, TransportType,
};

/// Default channel buffer size.
const DEFAULT_CHANNEL_BUFFER: usize = 256;

// ── ChannelTransport (client-side Transport impl) ───────────────────────

/// An in-process transport that communicates via `mpsc` channels.
///
/// This is the client-side half of a channel pair. It sends requests to the
/// server runner and receives responses, all without serialization overhead
/// beyond what the client's `ProtocolClient` already does.
#[derive(Debug)]
pub struct ChannelTransport {
    tx: mpsc::Sender<TransportMessage>,
    rx: tokio::sync::Mutex<mpsc::Receiver<TransportMessage>>,
    state: parking_lot::Mutex<TransportState>,
    capabilities: TransportCapabilities,
}

impl ChannelTransport {
    fn new(tx: mpsc::Sender<TransportMessage>, rx: mpsc::Receiver<TransportMessage>) -> Self {
        Self {
            tx,
            rx: tokio::sync::Mutex::new(rx),
            state: parking_lot::Mutex::new(TransportState::Connected),
            capabilities: TransportCapabilities {
                max_message_size: Some(MAX_MESSAGE_SIZE),
                supports_compression: false,
                supports_streaming: false,
                supports_bidirectional: true,
                supports_multiplexing: false,
                compression_algorithms: Vec::new(),
                custom: std::collections::HashMap::new(),
            },
        }
    }
}

impl Transport for ChannelTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Channel
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    fn state(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TransportState> + Send + '_>> {
        Box::pin(async move { self.state.lock().clone() })
    }

    fn connect(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            *self.state.lock() = TransportState::Connected;
            Ok(())
        })
    }

    fn disconnect(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            *self.state.lock() = TransportState::Disconnected;
            Ok(())
        })
    }

    fn send(
        &self,
        message: TransportMessage,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TransportResult<()>> + Send + '_>> {
        Box::pin(async move {
            self.tx
                .send(message)
                .await
                .map_err(|_| TransportError::ConnectionLost("Channel closed".to_string()))?;
            Ok(())
        })
    }

    fn receive(
        &self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = TransportResult<Option<TransportMessage>>> + Send + '_,
        >,
    > {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            Ok(rx.recv().await)
        })
    }

    fn metrics(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = TransportMetrics> + Send + '_>> {
        Box::pin(async move { TransportMetrics::default() })
    }
}

// ── Channel transport runner (server-side) ──────────────────────────────

/// Run a handler on an in-process channel transport.
///
/// Returns a `ChannelTransport` that the client can use, and a `JoinHandle`
/// for the server task. The server runs in the background processing requests
/// received over the channel.
///
/// This eliminates all line framing, flushing, newline scanning, and redundant
/// JSON parsing. Messages are passed as `TransportMessage` values directly
/// through `mpsc` channels.
///
/// # Example
///
/// ```rust,ignore
/// let (transport, server_handle) = channel::run_in_process(&handler).await?;
/// let client = Client::new(transport);
/// client.initialize().await?;
/// let tools = client.list_tools().await?;
/// server_handle.abort(); // shutdown
/// ```
pub async fn run_in_process<H: McpHandler + 'static>(
    handler: &H,
) -> McpResult<(ChannelTransport, tokio::task::JoinHandle<McpResult<()>>)> {
    run_in_process_with_buffer(handler, DEFAULT_CHANNEL_BUFFER).await
}

/// Like `run_in_process` but with a custom channel buffer size.
pub async fn run_in_process_with_buffer<H: McpHandler + 'static>(
    handler: &H,
    buffer_size: usize,
) -> McpResult<(ChannelTransport, tokio::task::JoinHandle<McpResult<()>>)> {
    spawn_server(handler, buffer_size, None).await
}

/// Like `run_in_process` but with a server configuration.
///
/// The configuration governs the `initialize` handshake — protocol version
/// negotiation and required client capabilities — and the message size
/// limit, exactly as it does on the other transports.
pub async fn run_in_process_with_config<H: McpHandler + 'static>(
    handler: &H,
    config: &ServerConfig,
) -> McpResult<(ChannelTransport, tokio::task::JoinHandle<McpResult<()>>)> {
    spawn_server(handler, DEFAULT_CHANNEL_BUFFER, Some(config.clone())).await
}

async fn spawn_server<H: McpHandler + 'static>(
    handler: &H,
    buffer_size: usize,
    config: Option<ServerConfig>,
) -> McpResult<(ChannelTransport, tokio::task::JoinHandle<McpResult<()>>)> {
    handler.on_initialize().await?;

    // Client → Server channel
    let (client_tx, server_rx) = mpsc::channel::<TransportMessage>(buffer_size);
    // Server → Client channel
    let (server_tx, client_rx) = mpsc::channel::<TransportMessage>(buffer_size);

    let client_transport = ChannelTransport::new(client_tx, client_rx);

    let handler = handler.clone();
    let server_handle =
        tokio::spawn(async move { run_server_loop(handler, server_rx, server_tx, config).await });

    Ok((client_transport, server_handle))
}

/// The core server event loop for channel transport.
///
/// This is analogous to `LineTransportRunner::run()` but operates on
/// `TransportMessage` values instead of text lines. The key differences:
///
/// - No line framing or newline scanning
/// - No string allocation for reading
/// - No flush calls
/// - JSON is parsed once from `Bytes` payload (not from a `String`)
async fn run_server_loop<H: McpHandler>(
    handler: H,
    mut incoming: mpsc::Receiver<TransportMessage>,
    outgoing: mpsc::Sender<TransportMessage>,
    config: Option<ServerConfig>,
) -> McpResult<()> {
    let max_message_size = config
        .as_ref()
        .map_or(MAX_MESSAGE_SIZE, |config| config.max_message_size);

    // Handlers reach the client through the session; this loop is the only
    // writer to the channel.
    let (session, mut cmd_rx) = ConnectionSession::new();
    let session_handle = Arc::new(session);
    let new_ctx = || RequestContext::channel().with_session_id(session_handle.id());

    // Channel for completed handler responses
    let (response_tx, mut response_rx) = mpsc::channel::<router::JsonRpcOutgoing>(32);

    // In-flight handler cancellation tokens, keyed by JSON-RPC id; signalled
    // by `notifications/cancelled` per MCP 2025-11-25.
    let pending_handlers: Arc<DashMap<String, CancellationToken>> = Arc::new(DashMap::new());
    let _cleanup = ConnectionCleanup::new(&pending_handlers, &session_handle);

    // Requests this server has sent the client and not yet seen answered.
    let mut outbound = OutboundRequests::default();
    let mut session_state = SessionState::Uninitialized;

    loop {
        tokio::select! {
            // Biased, with outgoing server-to-client messages ahead of
            // completed responses: a handler emits progress and logs while it
            // is still running, so they belong on the wire before the
            // response that concludes it. Unbiased, tokio picked a ready
            // branch at random and roughly half the time put the response
            // first — breaking "progress notifications MUST stop after
            // completion".
            biased;

            // Outgoing server-to-client requests/notifications
            Some(cmd) = cmd_rx.recv() => {
                if let Some(frame) = outbound.frame(cmd) {
                    send_value_msg(&outgoing, &frame).await?;
                }
            }

            // Completed handler responses
            Some(response) = response_rx.recv() => {
                if response.should_send() {
                    send_response_msg(&outgoing, &response).await?;
                }
            }

            // Incoming from client
            msg = incoming.recv() => {
                let Some(msg) = msg else { break; };

                // Check message size
                if msg.payload.len() > max_message_size {
                    send_error_msg(
                        &outgoing,
                        None,
                        McpError::invalid_request(format!(
                            "Message exceeds maximum size of {max_message_size} bytes"
                        )),
                    ).await?;
                    continue;
                }

                // Parse JSON directly from Bytes (no string allocation)
                let value: serde_json::Value = match serde_json::from_slice(&msg.payload) {
                    Ok(v) => v,
                    Err(e) => {
                        send_error_msg(&outgoing, None, McpError::parse_error(e.to_string())).await?;
                        continue;
                    }
                };

                // A response to one of our server-to-client requests.
                if outbound.resolve(&value) {
                    continue;
                }

                let id = readable_id(&value);
                let request = match router::parse_request_from_value(value) {
                    Ok(request) => request,
                    Err(e) => {
                        send_error_msg(&outgoing, id, e).await?;
                        continue;
                    }
                };

                if request.method == "initialize" {
                    let client_capabilities =
                        super::client_capabilities_from_initialize_params(
                            request.params.as_ref(),
                        );

                    if matches!(session_state, SessionState::Initialized(_)) {
                        send_error_msg(
                            &outgoing,
                            request.id.clone(),
                            McpError::invalid_request("Session already initialized"),
                        )
                        .await?;
                        continue;
                    }

                    let ctx = new_ctx();
                    let response = router::route_request_with_config(
                        &handler,
                        request,
                        &ctx,
                        config.as_ref(),
                    )
                    .await;

                    if let Some(ref result) = response.result
                        && let Some(v) =
                            result.get("protocolVersion").and_then(|v| v.as_str())
                    {
                        let version = ProtocolVersion::from(v);
                        session_state = SessionState::Initialized(
                            super::InitializedSessionState::new(version.clone()),
                        );
                        session_handle
                            .set_initialized(client_capabilities, version)
                            .await;
                    }

                    if response.should_send() {
                        send_response_msg(&outgoing, &response).await?;
                    }
                } else if request.method == "notifications/cancelled" {
                    super::cancel_pending_handler(&pending_handlers, request.params.as_ref());
                } else if request.method == "notifications/initialized" {
                    let h = handler.clone();
                    let resp_tx = response_tx.clone();
                    let ctx = new_ctx().with_session(session_handle.clone());

                    tokio::spawn(async move {
                        let response = router::route_request(&h, request, &ctx).await;
                        let _ = resp_tx.send(response).await;
                    });
                } else if request.method == "ping"
                    && matches!(session_state, SessionState::Uninitialized)
                {
                    // Lifecycle permits ping before initialize has completed.
                    let ctx = new_ctx().with_session(session_handle.clone());
                    let response = router::route_request(&handler, request, &ctx).await;
                    if response.should_send() {
                        send_response_msg(&outgoing, &response).await?;
                    }
                } else {
                    // Notifications (id=None) MUST NOT receive responses per
                    // JSON-RPC 2.0, so rejection paths stay silent for them.
                    let is_notification = request.id.is_none();
                    let version = match &mut session_state {
                        SessionState::Initialized(session) => {
                            session.protocol_version().clone()
                        }
                        SessionState::Uninitialized => {
                            if !is_notification {
                                send_error_msg(
                                    &outgoing,
                                    request.id.clone(),
                                    McpError::invalid_request(
                                        "Server not initialized. Send 'initialize' first.",
                                    ),
                                )
                                .await?;
                            }
                            continue;
                        }
                    };

                    // Spawn the handler with a per-request cancellation
                    // token so `notifications/cancelled` can signal it.
                    let h = handler.clone();
                    let resp_tx = response_tx.clone();
                    let (token, guard) =
                        super::register_pending_handler(&pending_handlers, request.id.as_ref());
                    // Kept so the spawned task can tell whether it
                    // was cancelled before publishing its result.
                    let cancel_signal = token.clone();
                    let ctx = new_ctx()
                        .with_session(session_handle.clone())
                        .with_cancellation_token(Arc::new(token) as Arc<dyn Cancellable>);

                    tokio::spawn(async move {
                        // RAII cleanup runs on every exit path,
                        // including handler panic.
                        let _guard = guard;
                        let response =
                            super::route_catching_panics(&h, request, &ctx, &version).await;
                        // See the note in line.rs.
                        if cancel_signal.is_cancelled() {
                            return;
                        }
                        let _ = resp_tx.send(response).await;
                    });
                }
            }
        }
    }

    // The client has gone; see the matching note in line.rs.
    outbound.close();
    drop(response_tx);
    let drained = tokio::time::timeout(SHUTDOWN_GRACE, async {
        loop {
            tokio::select! {
                biased;
                Some(cmd) = cmd_rx.recv() => match cmd {
                    SessionCommand::Request { response_tx, .. } => {
                        let _ = response_tx.send(Err(McpError::internal("Session closed")));
                    }
                    notification => {
                        if let Some(frame) = outbound.frame(notification) {
                            let _ = send_value_msg(&outgoing, &frame).await;
                        }
                    }
                },
                response = response_rx.recv() => {
                    let Some(response) = response else { break };
                    if response.should_send() {
                        let _ = send_response_msg(&outgoing, &response).await;
                    }
                }
            }
        }
    })
    .await;
    if drained.is_err() {
        tracing::warn!(
            "Handlers still running {SHUTDOWN_GRACE:?} after the client disconnected; cancelling them"
        );
    }

    handler.on_shutdown().await?;

    Ok(())
}

/// Serialize and send a server-initiated message over the channel.
async fn send_value_msg(
    tx: &mpsc::Sender<TransportMessage>,
    value: &serde_json::Value,
) -> McpResult<()> {
    let payload = serde_json::to_vec(value).map_err(|e| McpError::internal(e.to_string()))?;
    tx.send(TransportMessage::new(
        turbomcp_protocol::MessageId::from("server-message"),
        payload.into(),
    ))
    .await
    .map_err(|_| McpError::internal("Channel closed"))?;
    Ok(())
}

/// Serialize and send a JSON-RPC response over the channel.
async fn send_response_msg(
    tx: &mpsc::Sender<TransportMessage>,
    response: &router::JsonRpcOutgoing,
) -> McpResult<()> {
    let payload = router::serialize_response(response)?;
    tx.send(TransportMessage::new(
        response
            .id
            .as_ref()
            .map(|id| turbomcp_protocol::MessageId::from(id.to_string()))
            .unwrap_or_else(|| turbomcp_protocol::MessageId::from("response")),
        bytes::Bytes::from(payload),
    ))
    .await
    .map_err(|_| McpError::internal("Channel closed"))?;
    Ok(())
}

/// Serialize and send a JSON-RPC error over the channel.
///
/// Per JSON-RPC 2.0 §5.1, error responses to messages whose id could not be
/// determined (parse errors, oversized input) MUST use `id: null` on the
/// wire. The shared `JsonRpcOutgoing` type currently skips serializing `id`
/// when `None`, so we normalize to `Some(Value::Null)` here to keep the
/// transport boundary spec-correct regardless of how the underlying type
/// evolves.
async fn send_error_msg(
    tx: &mpsc::Sender<TransportMessage>,
    id: Option<serde_json::Value>,
    error: McpError,
) -> McpResult<()> {
    let id = Some(id.unwrap_or(serde_json::Value::Null));
    let response = router::JsonRpcOutgoing::error(id, error);
    send_response_msg(tx, &response).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use turbomcp_core::context::RequestContext as CoreRequestContext;
    use turbomcp_core::error::McpResult;
    use turbomcp_types::{
        Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
    };

    #[derive(Clone)]
    struct TestHandler;

    impl McpHandler for TestHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("channel-test", "1.0.0")
        }

        fn list_tools(&self) -> Vec<Tool> {
            vec![Tool::new("ping", "Ping tool")]
        }

        fn list_resources(&self) -> Vec<Resource> {
            vec![]
        }

        fn list_prompts(&self) -> Vec<Prompt> {
            vec![]
        }

        async fn call_tool(
            &self,
            _name: &str,
            _args: Value,
            _ctx: &CoreRequestContext,
        ) -> McpResult<ToolResult> {
            Ok(ToolResult::text("pong"))
        }

        async fn read_resource(
            &self,
            uri: &str,
            _ctx: &CoreRequestContext,
        ) -> McpResult<ResourceResult> {
            Err(McpError::resource_not_found(uri))
        }

        async fn get_prompt(
            &self,
            name: &str,
            _args: Option<Value>,
            _ctx: &CoreRequestContext,
        ) -> McpResult<PromptResult> {
            Err(McpError::prompt_not_found(name))
        }
    }

    #[tokio::test]
    async fn test_channel_transport_roundtrip() {
        let handler = TestHandler;
        let (transport, server_handle) = run_in_process(&handler).await.unwrap();

        // Send initialize request
        let init_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "clientInfo": { "name": "test", "version": "1.0.0" },
                "capabilities": {}
            }
        });
        let payload = serde_json::to_vec(&init_request).unwrap();
        transport
            .send(TransportMessage::new(
                turbomcp_protocol::MessageId::from("1"),
                payload.into(),
            ))
            .await
            .unwrap();

        // Receive response
        let response = transport.receive().await.unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&response.payload).unwrap();
        assert!(value.get("result").is_some());
        assert_eq!(value["result"]["serverInfo"]["name"], "channel-test");

        // Send ping tool call
        let ping_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": { "name": "ping", "arguments": {} }
        });
        let payload = serde_json::to_vec(&ping_request).unwrap();
        transport
            .send(TransportMessage::new(
                turbomcp_protocol::MessageId::from("2"),
                payload.into(),
            ))
            .await
            .unwrap();

        let response = transport.receive().await.unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_slice(&response.payload).unwrap();
        assert!(value.get("result").is_some());

        // Cleanup
        drop(transport);
        let _ = server_handle.await;
    }

    // JSON-RPC 2.0: notifications (no id) must not receive responses.
    // Rejecting a pre-init notification with an error over the channel
    // is a spec violation.
    #[tokio::test]
    async fn test_channel_transport_silent_on_notification_before_init() {
        let handler = TestHandler;
        let (transport, server_handle) = run_in_process(&handler).await.unwrap();

        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "tools/list"
        });
        let payload = serde_json::to_vec(&notification).unwrap();
        transport
            .send(TransportMessage::new(
                turbomcp_protocol::MessageId::from("n1"),
                payload.into(),
            ))
            .await
            .unwrap();

        let received =
            tokio::time::timeout(std::time::Duration::from_millis(200), transport.receive()).await;
        assert!(
            received.is_err(),
            "notifications must not receive a response, got: {received:?}"
        );

        drop(transport);
        let _ = server_handle.await;
    }

    /// A request id reused *after* the first one was answered is served, not
    /// refused.
    ///
    /// The spec's "MUST NOT have been previously used" binds the requestor; a
    /// receiver's only obligation is to echo the id. This server used to keep a
    /// per-session set of every id it had ever seen and reject repeats, which
    /// locked out real clients that recycle ids over a long session and grew
    /// without bound (#25). Concurrent reuse is still reported — see the
    /// `pending_handlers` warning — but never rejected.
    #[tokio::test]
    async fn test_channel_transport_serves_a_reused_request_id() {
        let handler = TestHandler;
        let (transport, server_handle) = run_in_process(&handler).await.unwrap();

        let init_request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "clientInfo": { "name": "test", "version": "1.0.0" },
                "capabilities": {}
            }
        });
        let init_payload = serde_json::to_vec(&init_request).unwrap();
        transport
            .send(TransportMessage::new(
                turbomcp_protocol::MessageId::from("1"),
                init_payload.into(),
            ))
            .await
            .unwrap();
        let _ = transport.receive().await.unwrap().unwrap();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        });
        let payload = serde_json::to_vec(&request).unwrap();

        transport
            .send(TransportMessage::new(
                turbomcp_protocol::MessageId::from("2-first"),
                payload.clone().into(),
            ))
            .await
            .unwrap();
        let first = transport.receive().await.unwrap().unwrap();
        let first_value: serde_json::Value = serde_json::from_slice(&first.payload).unwrap();
        assert!(first_value.get("result").is_some());

        transport
            .send(TransportMessage::new(
                turbomcp_protocol::MessageId::from("2-duplicate"),
                payload.into(),
            ))
            .await
            .unwrap();
        let duplicate = transport.receive().await.unwrap().unwrap();
        let duplicate_value: serde_json::Value =
            serde_json::from_slice(&duplicate.payload).unwrap();
        assert!(
            duplicate_value.get("error").is_none(),
            "a reused id must not be refused, got: {duplicate_value}"
        );
        assert_eq!(duplicate_value["id"], 2, "the id is echoed back verbatim");
        assert_eq!(
            duplicate_value["result"], first_value["result"],
            "the repeat is served exactly as the first was"
        );

        drop(transport);
        let _ = server_handle.await;
    }
}

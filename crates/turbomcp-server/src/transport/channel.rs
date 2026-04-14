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

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use turbomcp_core::error::{ErrorKind, McpError, McpResult};
use turbomcp_core::handler::McpHandler;
use turbomcp_core::types::core::ProtocolVersion;

use crate::context::{McpSession, RequestContext};
use crate::router;
use crate::transport::{MAX_MESSAGE_SIZE, SessionState};

use turbomcp_transport::{
    Transport, TransportCapabilities, TransportError, TransportMessage, TransportMetrics,
    TransportResult, TransportState, TransportType,
};

/// Default channel buffer size.
const DEFAULT_CHANNEL_BUFFER: usize = 256;

/// Maximum number of in-flight server-to-client requests.
const MAX_PENDING_REQUESTS: usize = 64;

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

// ── Session handle for bidirectional communication ──────────────────────

#[derive(Debug, Clone)]
struct ChannelSessionHandle {
    request_tx: mpsc::Sender<SessionCommand>,
}

#[derive(Debug)]
enum SessionCommand {
    Request {
        method: String,
        params: serde_json::Value,
        response_tx: oneshot::Sender<McpResult<serde_json::Value>>,
    },
    Notify {
        method: String,
        params: serde_json::Value,
    },
}

#[async_trait::async_trait]
impl McpSession for ChannelSessionHandle {
    async fn call(&self, method: &str, params: serde_json::Value) -> McpResult<serde_json::Value> {
        let (response_tx, response_rx) = oneshot::channel();
        self.request_tx
            .send(SessionCommand::Request {
                method: method.to_string(),
                params,
                response_tx,
            })
            .await
            .map_err(|_| McpError::internal("Session closed"))?;

        response_rx
            .await
            .map_err(|_| McpError::internal("Response channel closed"))?
    }

    async fn notify(&self, method: &str, params: serde_json::Value) -> McpResult<()> {
        self.request_tx
            .send(SessionCommand::Notify {
                method: method.to_string(),
                params,
            })
            .await
            .map_err(|_| McpError::internal("Session closed"))?;
        Ok(())
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
    handler.on_initialize().await?;

    // Client → Server channel
    let (client_tx, server_rx) = mpsc::channel::<TransportMessage>(buffer_size);
    // Server → Client channel
    let (server_tx, client_rx) = mpsc::channel::<TransportMessage>(buffer_size);

    let client_transport = ChannelTransport::new(client_tx, client_rx);

    let handler = handler.clone();
    let server_handle =
        tokio::spawn(async move { run_server_loop(handler, server_rx, server_tx).await });

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
) -> McpResult<()> {
    // Channel for session commands (server-to-client requests/notifications)
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<SessionCommand>(32);
    let session_handle = Arc::new(ChannelSessionHandle { request_tx: cmd_tx });

    // Channel for completed handler responses
    let (response_tx, mut response_rx) = mpsc::channel::<router::JsonRpcOutgoing>(32);

    // Server-to-client pending request tracking
    let mut pending_requests =
        HashMap::<serde_json::Value, oneshot::Sender<McpResult<serde_json::Value>>>::new();
    let mut next_request_id = 1u64;
    let mut session_state = SessionState::Uninitialized;

    loop {
        tokio::select! {
            // Incoming from client
            msg = incoming.recv() => {
                let Some(msg) = msg else { break; };

                // Check message size
                if msg.payload.len() > MAX_MESSAGE_SIZE {
                    send_error_msg(
                        &outgoing,
                        None,
                        McpError::invalid_request(format!(
                            "Message exceeds maximum size of {MAX_MESSAGE_SIZE} bytes"
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

                // Check if it's a response to a server-to-client request
                if let Some(id) = value.get("id")
                    && (value.get("result").is_some() || value.get("error").is_some())
                {
                    if let Some(tx) = pending_requests.remove(id) {
                        if let Some(error) = value.get("error") {
                            let mcp_error = serde_json::from_value::<turbomcp_core::jsonrpc::JsonRpcError>(error.clone())
                                .map(|e| McpError::new(ErrorKind::from_i32(e.code), e.message))
                                .unwrap_or_else(|_| McpError::internal("Failed to parse error response"));
                            let _ = tx.send(Err(mcp_error));
                        } else {
                            let result = value.get("result").cloned().unwrap_or(serde_json::Value::Null);
                            let _ = tx.send(Ok(result));
                        }
                    }
                } else {
                    // Parse as JSON-RPC request directly from the Value
                    // (avoids re-serializing to string then re-parsing like LineTransportRunner does)
                    match serde_json::from_value::<turbomcp_core::jsonrpc::JsonRpcIncoming>(value) {
                        Ok(request) => {
                            if request.method == "initialize" {
                                if matches!(session_state, SessionState::Initialized(_)) {
                                    send_error_msg(
                                        &outgoing,
                                        request.id.clone(),
                                        McpError::invalid_request("Session already initialized"),
                                    )
                                    .await?;
                                    continue;
                                }

                                let initialize_request_id = request.id.clone();
                                let ctx =
                                    RequestContext::channel().with_session(session_handle.clone());
                                let core_ctx = ctx.to_core_context();
                                let response = router::route_request_with_config(
                                    &handler,
                                    request,
                                    &core_ctx,
                                    None,
                                )
                                .await;

                                if let Some(ref result) = response.result
                                    && let Some(v) =
                                        result.get("protocolVersion").and_then(|v| v.as_str())
                                {
                                    let version = ProtocolVersion::from(v);
                                    session_state = SessionState::Initialized(
                                        super::InitializedSessionState::new(
                                            version,
                                            initialize_request_id.as_ref(),
                                        ),
                                    );
                                }

                                if response.should_send() {
                                    send_response_msg(&outgoing, &response).await?;
                                }
                            } else if request.method == "notifications/initialized"
                                || request.method == "notifications/cancelled"
                            {
                                let h = handler.clone();
                                let session = session_handle.clone();
                                let resp_tx = response_tx.clone();
                                let ctx = RequestContext::channel().with_session(session);
                                let core_ctx = ctx.to_core_context();

                                tokio::spawn(async move {
                                    let response = router::route_request(&h, request, &core_ctx).await;
                                    let _ = resp_tx.send(response).await;
                                });
                            } else {
                                // Notifications (id=None) MUST NOT receive responses per
                                // JSON-RPC 2.0, so rejection paths stay silent for them.
                                let is_notification = request.id.is_none();
                                let version = match &mut session_state {
                                    SessionState::Initialized(session) => {
                                        if !session.register_request_id(request.id.as_ref()) {
                                            if !is_notification {
                                                send_error_msg(
                                                    &outgoing,
                                                    request.id.clone(),
                                                    McpError::invalid_request(
                                                        "Request ID already used in this session",
                                                    ),
                                                )
                                                .await?;
                                            }
                                            continue;
                                        }

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

                                let h = handler.clone();
                                let session = session_handle.clone();
                                let resp_tx = response_tx.clone();
                                let ctx = RequestContext::channel().with_session(session);
                                let core_ctx = ctx.to_core_context();

                                tokio::spawn(async move {
                                    let response = router::route_request_versioned(
                                        &h, request, &core_ctx, &version,
                                    )
                                    .await;
                                    let _ = resp_tx.send(response).await;
                                });
                            }
                        }
                        Err(e) => {
                            send_error_msg(&outgoing, None, McpError::parse_error(e.to_string())).await?;
                        }
                    }
                }
            }

            // Completed handler responses
            Some(response) = response_rx.recv() => {
                if response.should_send() {
                    send_response_msg(&outgoing, &response).await?;
                }
            }

            // Outgoing server-to-client requests/notifications
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    SessionCommand::Request { method, params, response_tx } => {
                        if pending_requests.len() >= MAX_PENDING_REQUESTS {
                            let _ = response_tx.send(Err(McpError::internal(
                                "Too many pending server-to-client requests"
                            )));
                            continue;
                        }

                        let id = serde_json::json!(format!("s-{next_request_id}"));
                        next_request_id += 1;
                        pending_requests.insert(id.clone(), response_tx);

                        let request = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "method": method,
                            "params": params
                        });

                        let payload = serde_json::to_vec(&request)
                            .map_err(|e| McpError::internal(e.to_string()))?;

                        outgoing.send(TransportMessage::new(
                            turbomcp_protocol::MessageId::from(format!("s-req-{}", next_request_id - 1)),
                            payload.into(),
                        ))
                        .await
                        .map_err(|_| McpError::internal("Channel closed"))?;
                    }
                    SessionCommand::Notify { method, params } => {
                        let notification = serde_json::json!({
                            "jsonrpc": "2.0",
                            "method": method,
                            "params": params
                        });

                        let payload = serde_json::to_vec(&notification)
                            .map_err(|e| McpError::internal(e.to_string()))?;

                        outgoing.send(TransportMessage::new(
                            turbomcp_protocol::MessageId::from("notification"),
                            payload.into(),
                        ))
                        .await
                        .map_err(|_| McpError::internal("Channel closed"))?;
                    }
                }
            }
        }
    }

    // Drain remaining handler responses
    drop(response_tx);
    while let Some(response) = response_rx.recv().await {
        if response.should_send() {
            send_response_msg(&outgoing, &response).await?;
        }
    }

    handler.on_shutdown().await?;

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
async fn send_error_msg(
    tx: &mpsc::Sender<TransportMessage>,
    id: Option<serde_json::Value>,
    error: McpError,
) -> McpResult<()> {
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

    #[tokio::test]
    async fn test_channel_transport_rejects_duplicate_request_ids() {
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
        assert_eq!(duplicate_value["error"]["code"], -32600);
        assert!(
            duplicate_value["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("already used"))
        );

        drop(transport);
        let _ = server_handle.await;
    }
}

//! Shared line-based transport runner for STDIO, TCP, and Unix transports.
//!
//! This module provides the `LineTransportRunner` which handles the common
//! read-parse-route-respond pattern used by all line-based transports.
//!
//! # Bidirectional Communication
//!
//! The transport supports server-to-client requests (sampling, elicitation)
//! by spawning handler dispatch on separate tasks. This prevents deadlocks
//! when a handler awaits a client response via `session.call()`.

use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use turbomcp_core::error::McpError;
use turbomcp_core::handler::McpHandler;
use turbomcp_types::ProtocolVersion;

use crate::config::ServerConfig;
use crate::context::{Cancellable, RequestContext};
use crate::router;

use super::session::{
    ConnectionCleanup, ConnectionSession, OutboundRequests, SHUTDOWN_GRACE, SessionCommand,
    readable_id,
};
use super::{MAX_MESSAGE_SIZE, SessionState};

/// Trait for types that can read lines.
pub trait LineReader: AsyncBufRead + Unpin + Send {}
impl<T: AsyncBufRead + Unpin + Send> LineReader for T {}

/// Trait for types that can write lines.
pub trait LineWriter: AsyncWrite + Unpin + Send {}
impl<T: AsyncWrite + Unpin + Send> LineWriter for T {}

/// Channel for completed handler responses to be written back to the client.
type HandlerResponse = router::JsonRpcOutgoing;

/// One line off the wire, as far as the reader task could make it out.
enum LineRead {
    /// A complete line, newline stripped.
    Line(String),
    /// A line longer than the limit. Its bytes were discarded as they
    /// arrived rather than buffered.
    TooLong,
    /// A line that is not UTF-8, so cannot be JSON.
    NotUtf8,
}

/// Read one newline-terminated line, holding at most `limit` bytes of it.
///
/// `read_line` buffers the whole line before the caller can look at its
/// length, so a peer that sends gigabytes without a newline grows the buffer
/// until the process is killed — reachable by anyone who can connect over TCP
/// or Unix. Here an over-long line is discarded as it streams past and the
/// reader resynchronises at the next newline.
///
/// `Ok(None)` is end of input.
async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> std::io::Result<Option<LineRead>> {
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            // End of input. A final line without a newline still counts.
            return Ok(match (too_long, line.is_empty()) {
                (true, _) => Some(LineRead::TooLong),
                (false, true) => None,
                (false, false) => Some(finish_line(line)),
            });
        }

        let newline = available.iter().position(|&b| b == b'\n');
        let content = &available[..newline.unwrap_or(available.len())];
        if !too_long {
            if line.len() + content.len() > limit {
                too_long = true;
                line = Vec::new();
            } else {
                line.extend_from_slice(content);
            }
        }

        let consumed = newline.map_or(available.len(), |at| at + 1);
        reader.consume(consumed);

        if newline.is_some() {
            return Ok(Some(if too_long {
                LineRead::TooLong
            } else {
                finish_line(line)
            }));
        }
    }
}

fn finish_line(line: Vec<u8>) -> LineRead {
    match String::from_utf8(line) {
        Ok(line) => LineRead::Line(line),
        Err(_) => LineRead::NotUtf8,
    }
}

/// Shared runner for line-based transports (STDIO, TCP, Unix).
#[derive(Debug)]
pub struct LineTransportRunner<H: McpHandler> {
    handler: H,
    config: Option<ServerConfig>,
}

impl<H: McpHandler> LineTransportRunner<H> {
    /// Create a new line transport runner with default configuration.
    ///
    /// Uses strict latest-version-only protocol negotiation.
    pub fn new(handler: H) -> Self {
        Self {
            handler,
            config: None,
        }
    }

    /// Create a line transport runner with custom server configuration.
    ///
    /// Use `ServerConfig` with `ProtocolConfig::multi_version()` to accept
    /// clients requesting older MCP specification versions (e.g. 2025-06-18).
    pub fn with_config(handler: H, config: ServerConfig) -> Self {
        Self {
            handler,
            config: Some(config),
        }
    }

    /// Run the transport loop.
    ///
    /// Handler dispatch is spawned on separate tasks to prevent deadlocks
    /// when handlers use bidirectional communication (sampling, elicitation).
    /// The transport loop remains free to process both incoming messages and
    /// outgoing server-to-client requests concurrently.
    pub async fn run<R, W, F>(
        &self,
        reader: R,
        mut writer: W,
        ctx_factory: F,
    ) -> Result<(), McpError>
    where
        // `'static` so the reader can own the stream on its own task.
        R: LineReader + 'static,
        W: LineWriter,
        F: Fn() -> RequestContext,
    {
        let max_message_size = self
            .config
            .as_ref()
            .map_or(MAX_MESSAGE_SIZE, |config| config.max_message_size);

        // Handlers reach the client through the session; this loop is the
        // only writer to the connection.
        let (session, mut cmd_rx) = ConnectionSession::new();
        let session_handle = Arc::new(session);
        let new_ctx = || ctx_factory().with_session_id(session_handle.id());

        // Channel for completed handler responses
        let (response_tx, mut response_rx) = mpsc::channel::<HandlerResponse>(32);

        // In-flight handler cancellation tokens, keyed by the JSON-RPC `id`
        // of the originating request. Populated when we spawn a handler task,
        // cleared when the task finishes, and signalled when the client sends
        // `notifications/cancelled` per MCP 2025-11-25 §Cancellation.
        let pending_handlers: Arc<DashMap<String, CancellationToken>> = Arc::new(DashMap::new());
        let _cleanup = ConnectionCleanup::new(&pending_handlers, &session_handle);

        // Requests this server has sent the client and not yet seen answered.
        let mut outbound = OutboundRequests::default();

        // MCP session lifecycle state. Enforces that `initialize` succeeds
        // before any other requests are processed, and prevents duplicate init.
        let mut session_state = SessionState::Uninitialized;

        // Read on a dedicated task rather than inside the `select!` below.
        //
        // Reading a line is not cancel safe: when it loses a `select!` race,
        // what was read of the line so far is lost. The other arms here are
        // fed by concurrently spawned handler tasks, so losing that race is
        // routine — any request that does not arrive in a single poll (large
        // `tools/call` arguments, TCP segmentation, a pipe write split across
        // syscalls) could silently lose its prefix and desynchronise the
        // stream.
        //
        // Moving the read onto its own task takes it out of the select
        // entirely: nothing ever cancels it mid-line, and the loop awaits
        // `recv()`, which IS cancel safe.
        let (line_tx, mut line_rx) = mpsc::channel::<std::io::Result<LineRead>>(32);
        tokio::spawn(async move {
            let mut reader = reader;
            loop {
                match read_bounded_line(&mut reader, max_message_size).await {
                    Ok(None) => break, // EOF
                    Ok(Some(line)) => {
                        // Send error means the transport loop is gone.
                        if line_tx.send(Ok(line)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = line_tx.send(Err(e)).await;
                        break;
                    }
                }
            }
        });

        loop {
            tokio::select! {
                biased;

                // Incoming from client
                maybe_line = line_rx.recv() => {
                    // Channel closed: the reader task hit EOF or an error it
                    // already reported.
                    let Some(line_result) = maybe_line else { break };
                    let line = match line_result
                        .map_err(|e| McpError::internal(format!("Failed to read line: {e}")))?
                    {
                        LineRead::Line(line) => line,
                        // Reported as an error and skipped, so an oversized
                        // frame does not take the connection down with it.
                        LineRead::TooLong => {
                            self.send_error(
                                &mut writer,
                                None,
                                McpError::invalid_request(format!(
                                    "Message exceeds maximum size of {max_message_size} bytes",
                                )),
                            ).await?;
                            continue;
                        }
                        LineRead::NotUtf8 => {
                            self.send_error(
                                &mut writer,
                                None,
                                McpError::parse_error("message is not valid UTF-8"),
                            ).await?;
                            continue;
                        }
                    };

                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    // Try parsing as a general JSON-RPC message
                    let value: serde_json::Value = match serde_json::from_str(trimmed) {
                        Ok(v) => v,
                        Err(e) => {
                            self.send_error(&mut writer, None, McpError::parse_error(e.to_string())).await?;
                            continue;
                        }
                    };

                    // A response to one of our server-to-client requests.
                    if outbound.resolve(&value) {
                        continue;
                    }

                    let id = readable_id(&value);
                    // Reuse the already-parsed `Value` rather than re-parsing
                    // the raw line — saves one full JSON parse per message.
                    let request = match router::parse_request_from_value(value) {
                        Ok(request) => request,
                        Err(e) => {
                            self.send_error(&mut writer, id, e).await?;
                            continue;
                        }
                    };

                    if request.method == "initialize" {
                        let client_capabilities =
                            super::client_capabilities_from_initialize_params(
                                request.params.as_ref(),
                            );

                        // Reject duplicate initialize per MCP spec.
                        if matches!(session_state, SessionState::Initialized(_)) {
                            self.send_error(
                                &mut writer,
                                request.id.clone(),
                                McpError::invalid_request("Session already initialized"),
                            )
                            .await?;
                            continue;
                        }

                        // Handle initialize inline (not spawned) so we can
                        // capture the negotiated protocol version. Per the
                        // MCP spec, initialize is always the first request
                        // and the client waits for the response, so there
                        // is no deadlock risk from blocking the loop here.
                        //
                        // NOTE: Handlers MUST NOT call session.call() during
                        // initialize dispatch — the transport loop is blocked
                        // here and cannot process the server-to-client
                        // request, which would deadlock.
                        let ctx = new_ctx();
                        let response = router::route_request_with_config(
                            &self.handler,
                            request,
                            &ctx,
                            self.config.as_ref(),
                        )
                        .await;

                        // Extract the negotiated version from a successful
                        // response. On failure (error response), session
                        // stays Uninitialized and subsequent non-init
                        // requests will be rejected.
                        if let Some(ref result) = response.result
                            && let Some(v) =
                                result.get("protocolVersion").and_then(|v| v.as_str())
                        {
                            let version = ProtocolVersion::from(v);
                            tracing::info!(version = %version, "Protocol version negotiated");
                            session_state = SessionState::Initialized(
                                super::InitializedSessionState::new(version.clone()),
                            );
                            session_handle
                                .set_initialized(client_capabilities, version)
                                .await;
                        }

                        if response.should_send() {
                            self.send_response(&mut writer, &response).await?;
                        }
                    } else if request.method == "notifications/cancelled" {
                        // MCP 2025-11-25 §Cancellation: signal the matching
                        // in-flight handler. Notifications have no response,
                        // so we consume here.
                        super::cancel_pending_handler(&pending_handlers, request.params.as_ref());
                    } else if request.method == "notifications/initialized" {
                        // Lifecycle notification — allowed pre-init.
                        let handler = self.handler.clone();
                        let resp_tx = response_tx.clone();
                        let ctx = new_ctx().with_session(session_handle.clone());

                        tokio::spawn(async move {
                            let response = router::route_request(&handler, request, &ctx).await;
                            let _ = resp_tx.send(response).await;
                        });
                    } else if request.method == "ping"
                        && matches!(session_state, SessionState::Uninitialized)
                    {
                        // Lifecycle permits ping before initialize has completed.
                        let ctx = new_ctx().with_session(session_handle.clone());
                        let response = router::route_request(&self.handler, request, &ctx).await;
                        if response.should_send() {
                            self.send_response(&mut writer, &response).await?;
                        }
                    } else {
                        // All other requests require a successful initialize.
                        // Notifications (id=None) MUST NOT receive responses
                        // per JSON-RPC 2.0, so rejection paths stay silent.
                        let is_notification = request.id.is_none();
                        let version = match &mut session_state {
                            SessionState::Initialized(session) => {
                                session.protocol_version().clone()
                            }
                            SessionState::Uninitialized => {
                                if !is_notification {
                                    self.send_error(
                                        &mut writer,
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

                        // Spawn handler on a separate task to prevent
                        // deadlocks when the handler uses session.call()
                        // for sampling/elicitation. Install a per-request
                        // CancellationToken into the context and register
                        // it so `notifications/cancelled` from the client
                        // can signal the handler.
                        let handler = self.handler.clone();
                        let resp_tx = response_tx.clone();
                        let (token, guard) = super::register_pending_handler(
                            &pending_handlers,
                            request.id.as_ref(),
                        );
                        // Kept so the spawned task can tell whether it was
                        // cancelled before publishing its result.
                        let cancel_signal = token.clone();
                        let ctx = new_ctx()
                            .with_session(session_handle.clone())
                            .with_cancellation_token(Arc::new(token) as Arc<dyn Cancellable>);

                        tokio::spawn(async move {
                            // RAII: the guard removes the registry entry on
                            // every exit path, including a panic in the
                            // handler.
                            let _guard = guard;
                            let response =
                                super::route_catching_panics(&handler, request, &ctx, &version)
                                    .await;
                            // A cancelled request gets no response. The
                            // client has already been told to forget this
                            // id, so answering it now is an unsolicited
                            // reply — and it is what made cancellation
                            // cosmetic: handlers stopped being awaited but
                            // their results were sent regardless.
                            if cancel_signal.is_cancelled() {
                                return;
                            }
                            // If channel is closed the transport loop has exited; ignore.
                            let _ = resp_tx.send(response).await;
                        });
                    }
                }

                // Outgoing server-to-client requests/notifications.
                //
                // Drained ahead of completed responses: a handler emits these
                // while it is still running, so they belong on the wire before
                // the response that concludes it. Progress notifications in
                // particular must stop once an operation completes, which a
                // response-first order would violate. The channel is bounded
                // and only in-flight handlers write to it, so responses cannot
                // be starved.
                Some(cmd) = cmd_rx.recv() => {
                    if let Some(frame) = outbound.frame(cmd) {
                        self.send_value(&mut writer, &frame).await?;
                    }
                }

                // Completed handler responses ready to write back
                Some(response) = response_rx.recv() => {
                    if response.should_send() {
                        self.send_response(&mut writer, &response).await?;
                    }
                }
            }
        }

        // The client has gone. Nothing it could answer will arrive, so
        // handlers awaiting a reply learn that now rather than at their
        // timeout.
        outbound.close();

        // Let in-flight handlers finish, for up to `SHUTDOWN_GRACE`, writing
        // what they produce on a best-effort basis. The command queue has to
        // keep draining meanwhile: a handler blocked sending progress into a
        // full queue would otherwise never finish, and neither would this
        // function — which on TCP and Unix held the connection's slot forever.
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
                                let _ = self.send_value(&mut writer, &frame).await;
                            }
                        }
                    },
                    response = response_rx.recv() => {
                        let Some(response) = response else { break };
                        if response.should_send() {
                            let _ = self.send_response(&mut writer, &response).await;
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

        Ok(())
    }

    /// Write one JSON value as a line.
    async fn send_value<W: LineWriter>(
        &self,
        writer: &mut W,
        value: &serde_json::Value,
    ) -> Result<(), McpError> {
        let line = serde_json::to_string(value).map_err(|e| McpError::internal(e.to_string()))?;
        self.write_line(writer, &line).await
    }

    /// Send a JSON-RPC response.
    async fn send_response<W: LineWriter>(
        &self,
        writer: &mut W,
        response: &router::JsonRpcOutgoing,
    ) -> Result<(), McpError> {
        let response_str = router::serialize_response(response)?;
        self.write_line(writer, &response_str).await
    }

    async fn write_line<W: LineWriter>(&self, writer: &mut W, line: &str) -> Result<(), McpError> {
        writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| McpError::internal(format!("Failed to write: {e}")))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| McpError::internal(format!("Failed to write newline: {e}")))?;
        writer
            .flush()
            .await
            .map_err(|e| McpError::internal(format!("Failed to flush: {e}")))?;
        Ok(())
    }

    /// Send a JSON-RPC error response.
    ///
    /// Per JSON-RPC 2.0 §5.1, error responses to messages whose id could not
    /// be determined (parse errors, oversized input) MUST use `id: null` on
    /// the wire. The shared `JsonRpcOutgoing` type is currently configured
    /// to skip serializing `id` when `None`, so we normalize to
    /// `Some(Value::Null)` here to keep the transport boundary spec-correct
    /// regardless of how the underlying type evolves.
    async fn send_error<W: LineWriter>(
        &self,
        writer: &mut W,
        id: Option<serde_json::Value>,
        error: McpError,
    ) -> Result<(), McpError> {
        let id = Some(id.unwrap_or(serde_json::Value::Null));
        let response = router::JsonRpcOutgoing::error(id, error);
        self.send_response(writer, &response).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io::Cursor;
    use tokio::io::BufReader;
    use turbomcp_core::context::RequestContext as CoreRequestContext;
    use turbomcp_core::error::McpResult;
    use turbomcp_types::{
        Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool, ToolResult,
    };

    #[derive(Clone)]
    struct TestHandler;

    #[allow(clippy::manual_async_fn)]
    impl McpHandler for TestHandler {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("test", "1.0.0")
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

        fn call_tool<'a>(
            &'a self,
            _name: &'a str,
            _args: Value,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<ToolResult>> + Send + 'a {
            async { Ok(ToolResult::text("pong")) }
        }

        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a {
            let uri = uri.to_string();
            async move { Err(McpError::resource_not_found(&uri)) }
        }

        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<Value>,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>> + Send + 'a {
            let name = name.to_string();
            async move { Err(McpError::prompt_not_found(&name)) }
        }
    }

    /// Helper: build an initialize request line followed by notifications/initialized.
    fn init_handshake() -> String {
        let init = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "clientInfo": { "name": "test-client", "version": "1.0.0" },
                "capabilities": {}
            }
        });
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        format!("{}\n{}\n", init, notif)
    }

    #[tokio::test]
    async fn test_line_transport_ping_after_init() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let input = format!("{}{}\n", init_handshake(), ping);
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("\"id\":1"), "Should have ping response");
        // Ping response should be a success (no error)
        let lines: Vec<&str> = output_str.trim().lines().collect();
        let ping_line = lines
            .iter()
            .find(|l| l.contains("\"id\":1"))
            .expect("ping response line");
        assert!(
            ping_line.contains("\"result\""),
            "Ping should succeed after init"
        );
    }

    #[tokio::test]
    async fn test_line_transport_allows_ping_before_init() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        // Send ping without initialize first; MCP lifecycle permits this.
        let input = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let reader = BufReader::new(Cursor::new(format!("{}\n", input)));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("\"result\":{}"));
        assert!(!output_str.contains("\"error\""));
    }

    // JSON-RPC 2.0: notifications (no `id`) MUST NOT receive responses.
    // The uninitialized-session rejection path must stay silent for
    // notifications even though requests with the same shape get an error.
    #[tokio::test]
    async fn test_line_transport_silent_on_notification_before_init() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        let notif = r#"{"jsonrpc":"2.0","method":"tools/list"}"#;
        let reader = BufReader::new(Cursor::new(format!("{}\n", notif)));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        assert!(
            output_str.is_empty(),
            "notifications must not receive responses, got: {output_str}"
        );
    }

    #[tokio::test]
    async fn test_line_transport_rejects_duplicate_init() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        // Send two initialize requests
        let init1 = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "clientInfo": { "name": "test", "version": "1.0.0" },
                "capabilities": {}
            }
        });
        let init2 = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "clientInfo": { "name": "test", "version": "1.0.0" },
                "capabilities": {}
            }
        });
        let input = format!("{}\n{}\n", init1, init2);
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = output_str.trim().lines().collect();
        assert_eq!(lines.len(), 2, "Should have two responses");

        // First init should succeed
        assert!(lines[0].contains("\"result\""), "First init should succeed");
        // Second init should be rejected
        assert!(
            lines[1].contains("\"error\""),
            "Duplicate init should be rejected"
        );
        assert!(
            lines[1].contains("already initialized"),
            "Error should mention already initialized"
        );
    }

    #[tokio::test]
    async fn test_line_transport_empty_lines() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        // Empty lines followed by a ping before init, which MCP lifecycle permits.
        let input = "\n\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n";
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        // Should only have one successful ping response.
        assert_eq!(output_str.matches("jsonrpc").count(), 1);
        assert!(output_str.contains("\"result\":{}"));
    }

    // C-4: MAX_MESSAGE_SIZE enforcement
    #[tokio::test]
    async fn test_line_transport_oversized_message() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        // Create a message that exceeds MAX_MESSAGE_SIZE
        let oversized = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\",\"padding\":\"{}\"}}\n",
            "x".repeat(super::MAX_MESSAGE_SIZE + 1)
        );
        // Follow with another request to prove the loop continues
        let valid = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
        let input = format!("{}{}", oversized, valid);
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        // Should have error responses (oversized + uninitialized)
        assert!(
            output_str.contains("\"error\""),
            "Should contain error for oversized message"
        );
        assert!(
            output_str.contains("\"id\":2"),
            "Should continue processing after oversized message"
        );
    }

    // H-21: Invalid JSON input handling
    #[tokio::test]
    async fn test_line_transport_invalid_json() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        let input = "not valid json\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        // Should have a parse error and then an uninitialized error
        assert!(output_str.contains("\"error\""), "Should contain error");
        assert!(
            output_str.contains("\"id\":1"),
            "Should continue processing after parse error"
        );
    }

    /// A handler that is still emitting when the client disconnects.
    #[derive(Clone)]
    struct Chatty;

    #[allow(clippy::manual_async_fn)]
    impl McpHandler for Chatty {
        fn server_info(&self) -> ServerInfo {
            ServerInfo::new("chatty", "1.0.0")
        }
        fn list_tools(&self) -> Vec<Tool> {
            vec![Tool::new(
                "chatter",
                "Emits more notifications than the queue holds",
            )]
        }
        fn list_resources(&self) -> Vec<Resource> {
            vec![]
        }
        fn list_prompts(&self) -> Vec<Prompt> {
            vec![]
        }
        fn call_tool<'a>(
            &'a self,
            _name: &'a str,
            _args: Value,
            ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<ToolResult>> + Send + 'a {
            async move {
                for n in 0..200 {
                    let _ = ctx
                        .notify_client("notifications/message", serde_json::json!({ "n": n }))
                        .await;
                }
                Ok(ToolResult::text("done"))
            }
        }
        fn read_resource<'a>(
            &'a self,
            uri: &'a str,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<ResourceResult>> + Send + 'a {
            let uri = uri.to_string();
            async move { Err(McpError::resource_not_found(&uri)) }
        }
        fn get_prompt<'a>(
            &'a self,
            name: &'a str,
            _args: Option<Value>,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<PromptResult>> + Send + 'a {
            let name = name.to_string();
            async move { Err(McpError::prompt_not_found(&name)) }
        }
        fn on_roots_list_changed<'a>(
            &'a self,
            _ctx: &'a CoreRequestContext,
        ) -> impl std::future::Future<Output = McpResult<()>> + Send + 'a {
            async { panic!("hook blew up") }
        }
    }

    /// The client closes its end while a handler is mid-stream. The command
    /// queue used to go undrained after EOF, so the handler blocked on it
    /// forever and `run` never returned — on stdio `on_shutdown` never ran,
    /// and on TCP/Unix the connection's slot was never released.
    #[tokio::test]
    async fn eof_while_a_handler_is_emitting_does_not_hang() {
        let runner = LineTransportRunner::new(Chatty);
        let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"chatter"}}"#;
        let input = format!("{}{}\n", init_handshake(), call);
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            runner.run(reader, &mut output, RequestContext::stdio),
        )
        .await
        .expect("run must return once the client has gone")
        .expect("clean shutdown");
    }

    /// A notification is never answered — not even when its hook panics.
    #[tokio::test]
    async fn a_panicking_notification_hook_sends_nothing() {
        let runner = LineTransportRunner::new(Chatty);
        let changed = r#"{"jsonrpc":"2.0","method":"notifications/roots/list_changed"}"#;
        let input = format!("{}{}\n", init_handshake(), changed);
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = output.trim().lines().collect();
        assert_eq!(lines.len(), 1, "only the initialize response: {output}");
    }

    /// An envelope that is invalid but has a readable id is answered on that
    /// id, so the client's waiter resolves.
    #[tokio::test]
    async fn an_invalid_request_is_answered_on_its_own_id() {
        let runner = LineTransportRunner::new(TestHandler);
        let input = "{\"jsonrpc\":\"1.0\",\"id\":7,\"method\":\"ping\"}\n";
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let response: Value =
            serde_json::from_str(String::from_utf8(output).unwrap().trim()).expect("one response");
        assert_eq!(response["id"], 7, "{response}");
        assert_eq!(response["error"]["code"], -32600, "{response}");
    }

    /// An over-long line is dropped as it streams in, and the reader picks
    /// up at the next line. It used to be buffered whole before its length
    /// was checked, which on TCP/Unix let any peer exhaust memory.
    #[tokio::test]
    async fn read_bounded_line_discards_an_over_long_line() {
        let input = format!("{}\nok\n\u{1F600}\nlast", "x".repeat(100));
        let mut reader = BufReader::with_capacity(8, Cursor::new(input.into_bytes()));

        assert!(matches!(
            read_bounded_line(&mut reader, 16).await.unwrap(),
            Some(LineRead::TooLong)
        ));
        assert!(matches!(
            read_bounded_line(&mut reader, 16).await.unwrap(),
            Some(LineRead::Line(line)) if line == "ok"
        ));
        assert!(matches!(
            read_bounded_line(&mut reader, 16).await.unwrap(),
            Some(LineRead::Line(line)) if line == "\u{1F600}"
        ));
        assert!(matches!(
            read_bounded_line(&mut reader, 16).await.unwrap(),
            Some(LineRead::Line(line)) if line == "last"
        ));
        assert!(read_bounded_line(&mut reader, 16).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn read_bounded_line_reports_invalid_utf8() {
        let mut reader = BufReader::new(Cursor::new(vec![0xff, 0xfe, b'\n']));
        assert!(matches!(
            read_bounded_line(&mut reader, 16).await.unwrap(),
            Some(LineRead::NotUtf8)
        ));
    }

    // H-22: Clean EOF returns Ok
    #[tokio::test]
    async fn test_line_transport_clean_eof() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        let reader = BufReader::new(Cursor::new(""));
        let mut output = Vec::new();

        let result = runner.run(reader, &mut output, RequestContext::stdio).await;
        assert!(result.is_ok(), "Clean EOF should return Ok");
        assert!(output.is_empty(), "No output on empty input");
    }
}

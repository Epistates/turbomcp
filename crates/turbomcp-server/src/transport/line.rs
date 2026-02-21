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

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use turbomcp_core::error::{ErrorKind, McpError, McpResult};
use turbomcp_core::handler::McpHandler;

use crate::context::{McpSession, RequestContext};
use crate::router;

use super::MAX_MESSAGE_SIZE;

/// Maximum number of in-flight server-to-client requests before back-pressure.
const MAX_PENDING_REQUESTS: usize = 64;

/// Trait for types that can read lines.
pub trait LineReader: AsyncBufRead + Unpin + Send {}
impl<T: AsyncBufRead + Unpin + Send> LineReader for T {}

/// Trait for types that can write lines.
pub trait LineWriter: AsyncWrite + Unpin + Send {}
impl<T: AsyncWrite + Unpin + Send> LineWriter for T {}

/// Handle for a bidirectional session.
#[derive(Debug, Clone)]
pub struct SessionHandle {
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
impl McpSession for SessionHandle {
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

/// Channel for completed handler responses to be written back to the client.
type HandlerResponse = router::JsonRpcOutgoing;

/// Shared runner for line-based transports (STDIO, TCP, Unix).
#[derive(Debug)]
pub struct LineTransportRunner<H: McpHandler> {
    handler: H,
}

impl<H: McpHandler> LineTransportRunner<H> {
    /// Create a new line transport runner.
    pub fn new(handler: H) -> Self {
        Self { handler }
    }

    /// Run the transport loop.
    ///
    /// Handler dispatch is spawned on separate tasks to prevent deadlocks
    /// when handlers use bidirectional communication (sampling, elicitation).
    /// The transport loop remains free to process both incoming messages and
    /// outgoing server-to-client requests concurrently.
    pub async fn run<R, W, F>(
        &self,
        mut reader: R,
        mut writer: W,
        ctx_factory: F,
    ) -> Result<(), McpError>
    where
        R: LineReader,
        W: LineWriter,
        F: Fn() -> RequestContext,
    {
        // Channel for session commands (server-to-client requests/notifications)
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<SessionCommand>(32);
        let session_handle = Arc::new(SessionHandle { request_tx: cmd_tx });

        // Channel for completed handler responses
        let (response_tx, mut response_rx) = mpsc::channel::<HandlerResponse>(32);

        // Server-to-client pending request tracking
        let mut pending_requests =
            HashMap::<serde_json::Value, oneshot::Sender<McpResult<serde_json::Value>>>::new();
        // Use string-prefixed IDs to avoid collision with client-originated integer IDs
        let mut next_request_id = 1u64;

        let mut line = String::new();

        loop {
            tokio::select! {
                // Incoming from client
                res = reader.read_line(&mut line) => {
                    let bytes_read = res.map_err(|e| McpError::internal(format!("Failed to read line: {e}")))?;
                    if bytes_read == 0 { break; }

                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        line.clear();
                        continue;
                    }

                    // Check message size limit to prevent DoS
                    if line.len() > MAX_MESSAGE_SIZE {
                        self.send_error(
                            &mut writer,
                            None,
                            McpError::invalid_request(format!(
                                "Message exceeds maximum size of {MAX_MESSAGE_SIZE} bytes",
                            )),
                        ).await?;
                        line.clear();
                        continue;
                    }

                    // Try parsing as a general JSON-RPC message
                    let value: serde_json::Value = match serde_json::from_str(trimmed) {
                        Ok(v) => v,
                        Err(e) => {
                            self.send_error(&mut writer, None, McpError::parse_error(e.to_string())).await?;
                            line.clear();
                            continue;
                        }
                    };

                    // Check if it's a response to one of our server-to-client requests
                    if let Some(id) = value.get("id") && (value.get("result").is_some() || value.get("error").is_some()) {
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
                        } else {
                            tracing::warn!(id = %id, "Received response for unknown request ID");
                        }
                    } else {
                        // It's a request or notification from the client.
                        // Spawn handler on a separate task to prevent deadlocks when
                        // the handler uses session.call() for sampling/elicitation.
                        match router::parse_request(trimmed) {
                            Ok(request) => {
                                let handler = self.handler.clone();
                                let session = session_handle.clone();
                                let resp_tx = response_tx.clone();
                                let ctx = ctx_factory().with_session(session);
                                let core_ctx = ctx.to_core_context();

                                tokio::spawn(async move {
                                    let response = router::route_request(&handler, request, &core_ctx).await;
                                    // If channel is closed the transport loop has exited; ignore.
                                    let _ = resp_tx.send(response).await;
                                });
                            }
                            Err(e) => {
                                self.send_error(&mut writer, None, e).await?;
                            }
                        }
                    }
                    line.clear();
                }

                // Completed handler responses ready to write back
                Some(response) = response_rx.recv() => {
                    if response.should_send() {
                        self.send_response(&mut writer, &response).await?;
                    }
                }

                // Outgoing server-to-client requests/notifications
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        SessionCommand::Request { method, params, response_tx } => {
                            // Guard against unbounded pending request growth
                            if pending_requests.len() >= MAX_PENDING_REQUESTS {
                                tracing::error!(
                                    count = pending_requests.len(),
                                    "Too many pending server-to-client requests"
                                );
                                let _ = response_tx.send(Err(McpError::internal(
                                    "Too many pending server-to-client requests"
                                )));
                                continue;
                            }

                            // Use string-prefixed IDs to avoid collision with client IDs
                            let id = serde_json::json!(format!("s-{next_request_id}"));
                            next_request_id += 1;

                            pending_requests.insert(id.clone(), response_tx);

                            let request = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": method,
                                "params": params
                            });

                            let req_str = serde_json::to_string(&request)
                                .map_err(|e| McpError::internal(e.to_string()))?;
                            writer.write_all(req_str.as_bytes()).await
                                .map_err(|e| McpError::internal(format!("Failed to write: {e}")))?;
                            writer.write_all(b"\n").await
                                .map_err(|e| McpError::internal(format!("Failed to write newline: {e}")))?;
                            writer.flush().await
                                .map_err(|e| McpError::internal(format!("Failed to flush: {e}")))?;
                        }
                        SessionCommand::Notify { method, params } => {
                            let notification = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": method,
                                "params": params
                            });

                            let notif_str = serde_json::to_string(&notification)
                                .map_err(|e| McpError::internal(e.to_string()))?;
                            writer.write_all(notif_str.as_bytes()).await
                                .map_err(|e| McpError::internal(format!("Failed to write: {e}")))?;
                            writer.write_all(b"\n").await
                                .map_err(|e| McpError::internal(format!("Failed to write newline: {e}")))?;
                            writer.flush().await
                                .map_err(|e| McpError::internal(format!("Failed to flush: {e}")))?;
                        }
                    }
                }
            }
        }

        // Drop our response_tx so the channel closes once all spawned tasks finish
        drop(response_tx);

        // Drain remaining handler responses from in-flight tasks
        while let Some(response) = response_rx.recv().await {
            if response.should_send() {
                self.send_response(&mut writer, &response).await?;
            }
        }

        // Log abandoned pending requests on shutdown
        if !pending_requests.is_empty() {
            tracing::warn!(
                count = pending_requests.len(),
                "Abandoning pending server-to-client requests on transport shutdown"
            );
        }

        Ok(())
    }

    /// Send a JSON-RPC response.
    async fn send_response<W: LineWriter>(
        &self,
        writer: &mut W,
        response: &router::JsonRpcOutgoing,
    ) -> Result<(), McpError> {
        let response_str = router::serialize_response(response)?;
        writer
            .write_all(response_str.as_bytes())
            .await
            .map_err(|e| McpError::internal(format!("Failed to write response: {e}")))?;
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
    async fn send_error<W: LineWriter>(
        &self,
        writer: &mut W,
        id: Option<serde_json::Value>,
        error: McpError,
    ) -> Result<(), McpError> {
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

    #[tokio::test]
    async fn test_line_transport_ping() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        let input = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let reader = BufReader::new(Cursor::new(format!("{}\n", input)));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("\"jsonrpc\":\"2.0\""));
        assert!(output_str.contains("\"id\":1"));
    }

    #[tokio::test]
    async fn test_line_transport_empty_lines() {
        let handler = TestHandler;
        let runner = LineTransportRunner::new(handler);

        let input = "\n\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n\n";
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        // Should only have one response (for the ping)
        assert_eq!(output_str.matches("jsonrpc").count(), 1);
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
        // Follow with a valid request to prove the loop continues
        let valid = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
        let input = format!("{}{}", oversized, valid);
        let reader = BufReader::new(Cursor::new(input));
        let mut output = Vec::new();

        runner
            .run(reader, &mut output, RequestContext::stdio)
            .await
            .unwrap();

        let output_str = String::from_utf8(output).unwrap();
        // Should have an error response for oversized and a success for valid
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
        // Should have a parse error and then a valid response
        assert!(
            output_str.contains("\"error\""),
            "Should contain parse error"
        );
        assert!(
            output_str.contains("\"id\":1"),
            "Should continue processing after parse error"
        );
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

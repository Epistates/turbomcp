//! Shared line-based transport runner for STDIO, TCP, and Unix transports.
//!
//! This module provides the `LineTransportRunner` which handles the common
//! read-parse-route-respond pattern used by all line-based transports.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};
use turbomcp_core::error::{ErrorKind, McpError, McpResult};
use turbomcp_core::handler::McpHandler;

use crate::context::{McpSession, RequestContext};
use crate::router;

use super::MAX_MESSAGE_SIZE;

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
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<SessionCommand>(32);
        let session_handle = Arc::new(SessionHandle { request_tx: cmd_tx });

        let mut pending_requests =
            HashMap::<serde_json::Value, oneshot::Sender<McpResult<serde_json::Value>>>::new();
        let mut next_request_id = 1u64;

        let mut line = String::new();

        loop {
            tokio::select! {
                // Incoming from client
                res = reader.read_line(&mut line) => {
                    let bytes_read = res.map_err(|e| McpError::internal(format!("Failed to read line: {}", e)))?;
                    if bytes_read == 0 { break; }

                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        line.clear();
                        continue;
                    }

                    // Check message size limit to prevent DoS
                    if trimmed.len() > MAX_MESSAGE_SIZE {
                        self.send_error(
                            &mut writer,
                            None,
                            McpError::invalid_request(format!(
                                "Message exceeds maximum size of {} bytes",
                                MAX_MESSAGE_SIZE
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

                    // Check if it's a response to one of our requests
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
                        }
                    } else {
                        // It's a request or notification from the client
                        match router::parse_request(trimmed) {
                            Ok(request) => {
                                let ctx = ctx_factory().with_session(session_handle.clone());
                                let core_ctx = ctx.to_core_context();
                                let response = router::route_request(&self.handler, request, &core_ctx).await;

                                if response.should_send() {
                                    self.send_response(&mut writer, &response).await?;
                                }
                            }
                            Err(e) => {
                                self.send_error(&mut writer, None, e).await?;
                            }
                        }
                    }
                    line.clear();
                }

                // Outgoing from server
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        SessionCommand::Request { method, params, response_tx } => {
                            let id = serde_json::json!(next_request_id);
                            next_request_id += 1;

                            pending_requests.insert(id.clone(), response_tx);

                            let request = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": method,
                                "params": params
                            });

                            let req_str = serde_json::to_string(&request).map_err(|e| McpError::internal(e.to_string()))?;
                            writer.write_all(req_str.as_bytes()).await.map_err(|e| McpError::internal(format!("Failed to write: {}", e)))?;
                            writer.write_all(b"\n").await.map_err(|e| McpError::internal(format!("Failed to write newline: {}", e)))?;
                            writer.flush().await.map_err(|e| McpError::internal(format!("Failed to flush: {}", e)))?;
                        }
                        SessionCommand::Notify { method, params } => {
                            let notification = serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": method,
                                "params": params
                            });

                            let notif_str = serde_json::to_string(&notification).map_err(|e| McpError::internal(e.to_string()))?;
                            writer.write_all(notif_str.as_bytes()).await.map_err(|e| McpError::internal(format!("Failed to write: {}", e)))?;
                            writer.write_all(b"\n").await.map_err(|e| McpError::internal(format!("Failed to write newline: {}", e)))?;
                            writer.flush().await.map_err(|e| McpError::internal(format!("Failed to flush: {}", e)))?;
                        }
                    }
                }
            }
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
            .map_err(|e| McpError::internal(format!("Failed to write response: {}", e)))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| McpError::internal(format!("Failed to write newline: {}", e)))?;
        writer
            .flush()
            .await
            .map_err(|e| McpError::internal(format!("Failed to flush: {}", e)))?;
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
}

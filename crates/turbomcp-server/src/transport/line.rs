//! Shared line-based transport runner for STDIO, TCP, and Unix transports.
//!
//! This module provides the `LineTransportRunner` which handles the common
//! read-parse-route-respond pattern used by all line-based transports.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use turbomcp_core::error::McpError;
use turbomcp_core::handler::McpHandler;

use super::MAX_MESSAGE_SIZE;
use crate::context::RequestContext;
use crate::router;

/// Trait for types that can read lines.
pub trait LineReader: AsyncBufRead + Unpin + Send {}
impl<T: AsyncBufRead + Unpin + Send> LineReader for T {}

/// Trait for types that can write lines.
pub trait LineWriter: AsyncWrite + Unpin + Send {}
impl<T: AsyncWrite + Unpin + Send> LineWriter for T {}

/// Shared runner for line-based transports (STDIO, TCP, Unix).
///
/// This handles the common pattern of:
/// 1. Reading lines from input
/// 2. Parsing as JSON-RPC
/// 3. Routing to handler
/// 4. Writing response as line
///
/// # Example
///
/// ```rust,ignore
/// use tokio::io::{stdin, stdout, BufReader};
/// use turbomcp_server::transport::LineTransportRunner;
///
/// let runner = LineTransportRunner::new(handler);
/// let reader = BufReader::new(stdin());
/// let writer = stdout();
/// let ctx_factory = || RequestContext::stdio();
///
/// runner.run(reader, writer, ctx_factory).await?;
/// ```
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
    /// # Arguments
    ///
    /// * `reader` - Line reader (stdin, TCP socket, Unix socket)
    /// * `writer` - Line writer (stdout, TCP socket, Unix socket)
    /// * `ctx_factory` - Factory function to create request context
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on clean shutdown (EOF), or error on failure.
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
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader
                .read_line(&mut line)
                .await
                .map_err(|e| McpError::internal(format!("Failed to read line: {}", e)))?;

            if bytes_read == 0 {
                // EOF - clean shutdown
                break;
            }

            // Check message size limit
            if line.len() > MAX_MESSAGE_SIZE {
                self.send_error(
                    &mut writer,
                    None,
                    McpError::invalid_request(format!(
                        "Message exceeds maximum size of {} bytes",
                        MAX_MESSAGE_SIZE
                    )),
                )
                .await?;
                continue;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse and route
            let ctx = ctx_factory();
            let core_ctx = ctx.to_core_context();

            match router::parse_request(trimmed) {
                Ok(request) => {
                    let response = router::route_request(&self.handler, request, &core_ctx).await;

                    // Only send response if it should be sent (not a notification ack)
                    if response.should_send() {
                        self.send_response(&mut writer, &response).await?;
                    }
                }
                Err(e) => {
                    self.send_error(&mut writer, None, e).await?;
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

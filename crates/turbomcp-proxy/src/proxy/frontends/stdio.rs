//! STDIO Frontend for exposing proxy via stdin/stdout
//!
//! This frontend reads JSON-RPC requests from stdin and writes responses to stdout,
//! making the proxy accessible to CLI tools and shell scripts.

use serde_json::Value;
use std::io::Write;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, error, trace, warn};
use turbomcp_protocol::jsonrpc::{
    JsonRpcRequest, JsonRpcResponse, JsonRpcResponsePayload, ResponseId,
};

use crate::error::{ProxyError, ProxyResult};
use crate::proxy::backends::HttpBackend;

/// Maximum line size in bytes (10 MB) - matches `MAX_REQUEST_SIZE` from runtime module
const MAX_LINE_SIZE: usize = 10 * 1024 * 1024;

/// STDIO frontend configuration
#[derive(Debug, Clone)]
pub struct StdioFrontendConfig {
    /// Whether to flush stdout after each message (default: true)
    pub flush_after_message: bool,
}

impl Default for StdioFrontendConfig {
    fn default() -> Self {
        Self {
            flush_after_message: true,
        }
    }
}

/// STDIO frontend for CLI-friendly access
///
/// Reads JSON-RPC requests from stdin line by line and writes responses to stdout.
/// Logs and diagnostics go to stderr to keep stdout clean for protocol messages.
pub struct StdioFrontend {
    /// HTTP backend to forward requests to
    backend: HttpBackend,

    /// Configuration
    config: StdioFrontendConfig,
}

impl StdioFrontend {
    /// Create a new STDIO frontend
    ///
    /// # Arguments
    /// * `backend` - HTTP backend to forward requests to
    /// * `config` - Frontend configuration
    pub fn new(backend: HttpBackend, config: StdioFrontendConfig) -> Self {
        debug!("Created STDIO frontend");
        Self { backend, config }
    }

    /// Run the STDIO frontend event loop
    ///
    /// Reads JSON-RPC requests from stdin, forwards to backend, writes responses to stdout.
    /// Runs until EOF on stdin or error.
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if reading from stdin fails, parsing JSON-RPC requests fails, or forwarding to backend fails.
    pub async fn run(self) -> ProxyResult<()> {
        debug!("Starting STDIO frontend event loop");

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();

            // Read line from stdin
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    // EOF - clean shutdown
                    debug!("STDIO frontend received EOF, shutting down");
                    break;
                }
                Ok(_) => {
                    // Check size limit before processing
                    if line.len() > MAX_LINE_SIZE {
                        error!(
                            "Line exceeds maximum size of {} bytes (got {} bytes)",
                            MAX_LINE_SIZE,
                            line.len()
                        );
                        // Send error response for oversized request
                        self.write_error_response(
                            None,
                            -32700,
                            "Request too large",
                            Some(format!(
                                "Request size {} bytes exceeds maximum {} bytes",
                                line.len(),
                                MAX_LINE_SIZE
                            )),
                        )
                        .await?;
                        continue;
                    }

                    // Process the line
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    trace!("Received STDIO input: {}", trimmed);

                    // Parse as JSON-RPC request
                    match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                        Ok(request) => {
                            // Handle the request
                            if let Err(e) = self.handle_request(request).await {
                                error!("Error handling request: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse JSON-RPC request: {}", e);
                            // Send parse error response
                            self.write_error_response(None, -32700, "Parse error", None)
                                .await?;
                        }
                    }
                }
                Err(e) => {
                    error!("Error reading from stdin: {}", e);
                    return Err(ProxyError::backend(format!("STDIO read error: {e}")));
                }
            }
        }

        debug!("STDIO frontend event loop completed");
        Ok(())
    }

    /// Handle a JSON-RPC request
    async fn handle_request(&self, request: JsonRpcRequest) -> ProxyResult<()> {
        trace!(
            "Handling request: method={}, id={:?}",
            request.method, request.id
        );

        // Route the request to the appropriate backend method
        let result = match request.method.as_str() {
            "initialize" => {
                // Initialize is already handled by backend, just return capabilities
                Ok(self
                    .backend
                    .capabilities()
                    .ok_or_else(|| turbomcp_protocol::Error::internal("Backend not initialized"))?)
            }
            "tools/list" => self.backend.list_tools().await,
            "tools/call" => {
                let params = request.params.ok_or_else(|| {
                    turbomcp_protocol::Error::invalid_params("Missing params for tools/call")
                })?;
                let name = params.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                    turbomcp_protocol::Error::invalid_params("Missing 'name' in tools/call")
                })?;
                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
                self.backend.call_tool(name, arguments).await
            }
            "resources/list" => self.backend.list_resources().await,
            "resources/read" => {
                let params = request.params.ok_or_else(|| {
                    turbomcp_protocol::Error::invalid_params("Missing params for resources/read")
                })?;
                let uri = params.get("uri").and_then(|v| v.as_str()).ok_or_else(|| {
                    turbomcp_protocol::Error::invalid_params("Missing 'uri' in resources/read")
                })?;
                self.backend.read_resource(uri).await
            }
            "prompts/list" => self.backend.list_prompts().await,
            "prompts/get" => {
                let params = request.params.ok_or_else(|| {
                    turbomcp_protocol::Error::invalid_params("Missing params for prompts/get")
                })?;
                let name = params.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                    turbomcp_protocol::Error::invalid_params("Missing 'name' in prompts/get")
                })?;
                let arguments = params.get("arguments").cloned();
                self.backend.get_prompt(name, arguments).await
            }
            _ => {
                // Unknown method
                return self
                    .write_error_response(
                        Some(&request.id),
                        -32601,
                        "Method not found",
                        Some(format!("Unknown method: {}", request.method)),
                    )
                    .await;
            }
        };

        // Write response
        match result {
            Ok(result) => self.write_success_response(&request.id, result).await,
            Err(e) => {
                self.write_error_response(
                    Some(&request.id),
                    -32603,
                    "Internal error",
                    Some(e.to_string()),
                )
                .await
            }
        }
    }

    /// Write a success response to stdout
    async fn write_success_response(
        &self,
        id: &turbomcp_protocol::MessageId,
        result: Value,
    ) -> ProxyResult<()> {
        let response = JsonRpcResponse {
            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
            id: ResponseId(Some(id.clone())),
            payload: JsonRpcResponsePayload::Success { result },
        };

        self.write_response(&response).await
    }

    /// Write an error response to stdout
    async fn write_error_response(
        &self,
        id: Option<&turbomcp_protocol::MessageId>,
        code: i32,
        message: &str,
        data: Option<String>,
    ) -> ProxyResult<()> {
        let error = turbomcp_protocol::jsonrpc::JsonRpcError {
            code,
            message: message.to_string(),
            data: data.map(Value::String),
        };

        let response = JsonRpcResponse {
            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
            id: ResponseId(id.cloned()),
            payload: JsonRpcResponsePayload::Error { error },
        };

        self.write_response(&response).await
    }

    /// Write a JSON-RPC response to stdout
    #[allow(clippy::unused_async)]
    async fn write_response(&self, response: &JsonRpcResponse) -> ProxyResult<()> {
        let json = serde_json::to_string(response)?;

        trace!("Writing response to stdout: {}", json);

        // Write to stdout (blocking, but fast enough for line-based output)
        let mut stdout = std::io::stdout();
        writeln!(stdout, "{json}")
            .map_err(|e| ProxyError::backend(format!("Failed to write to stdout: {e}")))?;

        if self.config.flush_after_message {
            stdout
                .flush()
                .map_err(|e| ProxyError::backend(format!("Failed to flush stdout: {e}")))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These are basic unit tests. Integration tests are in tests/ directory.

    #[test]
    fn test_stdio_frontend_config_default() {
        let config = StdioFrontendConfig::default();
        assert!(config.flush_after_message, "Should flush by default");
    }

    #[test]
    fn test_stdio_frontend_creation() {
        // This test just verifies that the structure compiles and can be created
        // Actual functionality requires integration tests with a running HTTP server
    }
}

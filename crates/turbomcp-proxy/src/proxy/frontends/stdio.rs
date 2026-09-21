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
            // Forwarded verbatim rather than through the backend's argument-less
            // `list_*` helpers, so the client's `cursor` reaches the upstream
            // server. The result relay below already passes `nextCursor` back
            // down untouched; without the cursor going the other way the proxy
            // was handing out a cursor it could not honour, and a client
            // walking pages got page one forever. `_meta`/`progressToken` ride
            // along for the same reason.
            //
            // JSON-RPC 2.0 §4 wants params to be an object or array when
            // present, so an absent or null params becomes `{}`.
            "tools/list" | "resources/list" | "resources/templates/list" | "prompts/list" => {
                let params = request
                    .params
                    .clone()
                    .filter(|params| !params.is_null())
                    .unwrap_or_else(|| serde_json::json!({}));
                self.backend.send_request(&request.method, params).await
            }
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
            "resources/read" => {
                let params = request.params.ok_or_else(|| {
                    turbomcp_protocol::Error::invalid_params("Missing params for resources/read")
                })?;
                let uri = params.get("uri").and_then(|v| v.as_str()).ok_or_else(|| {
                    turbomcp_protocol::Error::invalid_params("Missing 'uri' in resources/read")
                })?;
                self.backend.read_resource(uri).await
            }
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
    // Genuinely synchronous, but the trait it implements is async. Clippy 1.98
    // split a dedicated lint out for the trait-impl case, so both names are
    // needed until the MSRV clears 1.98.
    #[allow(clippy::unused_async)]
    #[allow(unknown_lints, clippy::unused_async_trait_impl)]
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

    /// MCP §Pagination: a `cursor` the client sends means "return results
    /// starting after this". The stdio frontend used to route `tools/list`
    /// through an argument-less backend helper, so the cursor never left the
    /// proxy — while the upstream's `nextCursor` was relayed down verbatim.
    /// A client walking pages therefore got page one, forever: rmcp's
    /// `list_all_tools` never terminates against it, and this SDK's own client
    /// returns a thousand duplicate copies of page one.
    ///
    /// Asserted upstream rather than downstream because that is precisely where
    /// the cursor went missing.
    #[tokio::test]
    async fn a_list_cursor_reaches_the_upstream_server() {
        use crate::proxy::backends::http::HttpBackendConfig;
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let upstream = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/mcp"))
            .and(body_partial_json(
                serde_json::json!({"method": "initialize"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "protocolVersion": turbomcp_protocol::PROTOCOL_VERSION,
                    "capabilities": {},
                    "serverInfo": { "name": "paged-upstream", "version": "1.0.0" }
                }
            })))
            .mount(&upstream)
            .await;

        // Only matches a tools/list that actually carries the cursor. An
        // unmatched request 404s, so the assertion is the mock itself.
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .and(body_partial_json(serde_json::json!({
                "method": "tools/list",
                "params": { "cursor": "page-2" }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": { "tools": [{ "name": "second_page", "description": "" }] }
            })))
            .expect(1)
            .mount(&upstream)
            .await;

        let backend = HttpBackend::new(HttpBackendConfig {
            url: format!("{}/mcp", upstream.uri()),
            auth_token: None,
            timeout_secs: Some(5),
            client_name: "test-proxy".to_string(),
            client_version: "1.0.0".to_string(),
        })
        .await
        .expect("backend connects");

        let frontend = StdioFrontend::new(backend, StdioFrontendConfig::default());
        let request: JsonRpcRequest = serde_json::from_value(serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": { "cursor": "page-2" }
        }))
        .expect("request parses");

        frontend
            .handle_request(request)
            .await
            .expect("the request is served");

        // `.expect(1)` is verified on drop; make the failure explicit here too.
        let cursored = upstream
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .any(|body| body["params"]["cursor"] == "page-2");
        assert!(
            cursored,
            "the client's cursor must reach the upstream server"
        );
    }
}

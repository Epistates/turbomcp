//! HTTP Backend for connecting to remote HTTP MCP servers
//!
//! This backend uses reqwest to communicate with MCP servers over HTTP/SSE.
//! It implements the complete MCP 2025-06-18 protocol over HTTP transport.

use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, trace};
use turbomcp_protocol::{
    InitializeRequest, InitializeResult, MessageId,
    jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcResponsePayload, JsonRpcVersion},
};

use crate::error::{ProxyError, ProxyResult};

/// Configuration for HTTP backend
#[derive(Clone)]
pub struct HttpBackendConfig {
    /// Base URL of the HTTP MCP server (e.g., "<http://localhost:3000/mcp>")
    pub url: String,

    /// Optional authentication token (Bearer) - protected with secrecy
    pub auth_token: Option<SecretString>,

    /// Request timeout in seconds (default: 30)
    pub timeout_secs: Option<u64>,

    /// Client name for initialization
    pub client_name: String,

    /// Client version for initialization
    pub client_version: String,
}

impl std::fmt::Debug for HttpBackendConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpBackendConfig")
            .field("url", &self.url)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "<redacted>"),
            )
            .field("timeout_secs", &self.timeout_secs)
            .field("client_name", &self.client_name)
            .field("client_version", &self.client_version)
            .finish()
    }
}

/// HTTP backend for connecting to remote MCP servers
///
/// Uses reqwest with connection pooling for efficient HTTP communication.
pub struct HttpBackend {
    /// HTTP client with connection pooling
    client: reqwest::Client,

    /// Server base URL
    url: String,

    /// Optional auth token - protected with secrecy
    auth_token: Option<SecretString>,

    /// Message ID counter
    next_id: AtomicU64,

    /// Server capabilities (cached after initialization)
    capabilities: Arc<parking_lot::RwLock<Option<Value>>>,
}

impl std::fmt::Debug for HttpBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpBackend")
            .field("client", &"<reqwest::Client>")
            .field("url", &self.url)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "<redacted>"),
            )
            .field("next_id", &self.next_id)
            .field("capabilities", &"<RwLock>")
            .finish()
    }
}

impl HttpBackend {
    /// Create a new HTTP backend
    ///
    /// # Arguments
    /// * `config` - HTTP backend configuration
    ///
    /// # Errors
    /// Returns error if HTTP client creation fails
    pub async fn new(config: HttpBackendConfig) -> ProxyResult<Self> {
        // Build HTTP client with timeouts and connection pooling
        let timeout = std::time::Duration::from_secs(config.timeout_secs.unwrap_or(30));

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Some(std::time::Duration::from_secs(90)))
            // Explicit TLS configuration for security
            .danger_accept_invalid_certs(false) // Never accept invalid certificates
            .tls_built_in_root_certs(true) // Use system root certificates
            .min_tls_version(reqwest::tls::Version::TLS_1_3) // TLS 1.3 required (v3.0+)
            .https_only(false) // Allow HTTP for localhost (validated by RuntimeProxyBuilder)
            .build()
            .map_err(|e| ProxyError::backend(format!("Failed to create HTTP client: {e}")))?;

        debug!("Created HTTP backend for URL: {}", config.url);

        let backend = Self {
            client,
            url: config.url,
            auth_token: config.auth_token,
            next_id: AtomicU64::new(1),
            capabilities: Arc::new(parking_lot::RwLock::new(None)),
        };

        // Perform initialization
        backend
            .initialize(&config.client_name, &config.client_version)
            .await?;

        Ok(backend)
    }

    /// Initialize connection with the server
    async fn initialize(
        &self,
        client_name: &str,
        client_version: &str,
    ) -> ProxyResult<InitializeResult> {
        debug!("Initializing HTTP backend connection");

        let request = InitializeRequest {
            protocol_version: "2025-06-18".to_string(),
            capabilities: turbomcp_protocol::types::ClientCapabilities {
                experimental: None,
                sampling: None,
                roots: None,
                elicitation: None,
                #[cfg(feature = "mcp-tasks")]
                tasks: None,
            },
            client_info: turbomcp_protocol::types::Implementation {
                name: client_name.to_string(),
                version: client_version.to_string(),
                ..Default::default()
            },
            _meta: None,
        };

        let response = self
            .send_request("initialize", serde_json::to_value(&request)?)
            .await?;

        let result: InitializeResult = serde_json::from_value(response)?;

        // Cache capabilities
        *self.capabilities.write() = Some(serde_json::to_value(&result.capabilities)?);

        debug!("HTTP backend initialized successfully");

        // Send initialized notification
        self.send_notification("notifications/initialized", Value::Null)
            .await?;

        Ok(result)
    }

    /// Send a JSON-RPC request and wait for response
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the HTTP request fails, returns a non-success status, or if parsing the JSON-RPC response fails.
    pub async fn send_request(&self, method: &str, params: Value) -> ProxyResult<Value> {
        let id = self.next_message_id();

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            // Cast u64 to i64 for JSON-RPC MessageId - IDs are sequential and won't overflow in practice
            #[allow(clippy::cast_possible_wrap)]
            id: MessageId::Number(id as i64),
            method: method.to_string(),
            params: Some(params),
        };

        trace!("Sending HTTP request: method={}, id={}", method, id);

        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&request);

        // Add auth header if configured
        if let Some(ref token) = self.auth_token {
            req = req.header("Authorization", format!("Bearer {}", token.expose_secret()));
        }

        let response = req
            .send()
            .await
            .map_err(|e| ProxyError::backend(format!("HTTP request failed: {e}")))?;

        // Check HTTP status
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read response body".to_string());
            return Err(ProxyError::backend(format!("HTTP error {status}: {body}")));
        }

        // Parse JSON-RPC response
        let json_response: JsonRpcResponse = response
            .json()
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to parse JSON-RPC response: {e}")))?;

        // Check for JSON-RPC error and extract result
        match json_response.payload {
            JsonRpcResponsePayload::Success { result } => Ok(result),
            JsonRpcResponsePayload::Error { error } => {
                // Preserve JSON-RPC error code by using rpc() constructor
                Err(turbomcp_protocol::Error::rpc(error.code, &error.message).into())
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected)
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the HTTP request fails.
    pub async fn send_notification(&self, method: &str, params: Value) -> ProxyResult<()> {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });

        trace!("Sending HTTP notification: method={}", method);

        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&notification);

        if let Some(ref token) = self.auth_token {
            req = req.header("Authorization", format!("Bearer {}", token.expose_secret()));
        }

        req.send()
            .await
            .map_err(|e| ProxyError::backend(format!("HTTP notification failed: {e}")))?;

        Ok(())
    }

    /// Get next message ID
    fn next_message_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Get cached server capabilities
    pub fn capabilities(&self) -> Option<Value> {
        self.capabilities.read().clone()
    }

    /// List available tools
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the request fails or the server returns an error.
    pub async fn list_tools(&self) -> ProxyResult<Value> {
        self.send_request("tools/list", Value::Null).await
    }

    /// Call a tool
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the request fails or the tool call fails on the server.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> ProxyResult<Value> {
        let params = serde_json::json!({
            "name": name,
            "arguments": arguments
        });
        self.send_request("tools/call", params).await
    }

    /// List available resources
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the request fails or the server returns an error.
    pub async fn list_resources(&self) -> ProxyResult<Value> {
        self.send_request("resources/list", Value::Null).await
    }

    /// Read a resource
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the request fails or the resource is not found.
    pub async fn read_resource(&self, uri: &str) -> ProxyResult<Value> {
        let params = serde_json::json!({
            "uri": uri
        });
        self.send_request("resources/read", params).await
    }

    /// List available prompts
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the request fails or the server returns an error.
    pub async fn list_prompts(&self) -> ProxyResult<Value> {
        self.send_request("prompts/list", Value::Null).await
    }

    /// Get a prompt
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the request fails or the prompt is not found.
    pub async fn get_prompt(&self, name: &str, arguments: Option<Value>) -> ProxyResult<Value> {
        let mut params = serde_json::json!({
            "name": name
        });
        if let Some(args) = arguments {
            params["arguments"] = args;
        }
        self.send_request("prompts/get", params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require a running HTTP MCP server
    // They are ignored by default and can be run with:
    // cargo test --package turbomcp-proxy --features runtime -- --ignored

    #[tokio::test]
    #[ignore = "requires running HTTP MCP server"]
    async fn test_http_backend_connection() {
        let config = HttpBackendConfig {
            url: "http://localhost:3000/mcp".to_string(),
            auth_token: None,
            timeout_secs: Some(5),
            client_name: "test-client".to_string(),
            client_version: "1.0.0".to_string(),
        };

        let backend = HttpBackend::new(config).await;
        assert!(backend.is_ok(), "HTTP backend should connect successfully");
    }

    #[tokio::test]
    #[ignore = "requires running HTTP MCP server"]
    async fn test_http_backend_list_tools() {
        let config = HttpBackendConfig {
            url: "http://localhost:3000/mcp".to_string(),
            auth_token: None,
            timeout_secs: Some(5),
            client_name: "test-client".to_string(),
            client_version: "1.0.0".to_string(),
        };

        let backend = HttpBackend::new(config).await.unwrap();
        let result = backend.list_tools().await;
        assert!(result.is_ok(), "Should be able to list tools");
    }

    #[test]
    fn test_debug_redaction() {
        let config = HttpBackendConfig {
            url: "http://localhost:3000/mcp".to_string(),
            auth_token: Some(SecretString::from("secret-token-12345".to_string())),
            timeout_secs: Some(5),
            client_name: "test-client".to_string(),
            client_version: "1.0.0".to_string(),
        };

        let debug_output = format!("{config:?}");
        assert!(
            !debug_output.contains("secret-token-12345"),
            "Token should be redacted in debug output"
        );
        assert!(
            debug_output.contains("<redacted>"),
            "Debug output should show <redacted> for token"
        );
    }
}

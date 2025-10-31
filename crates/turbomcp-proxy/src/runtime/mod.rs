//! Runtime proxy layer (dynamic, no code generation)
//!
//! This module provides dynamic proxying capabilities without code generation.
//! Ideal for development, testing, and prototyping.
//!
//! # Security Features
//!
//! - Command injection protection via allowlist
//! - SSRF protection for HTTP backends
//! - Path traversal protection
//! - Request size limits
//! - Timeout enforcement
//!
//! # Example
//!
//! ```no_run
//! # use turbomcp_proxy::runtime::{RuntimeProxyBuilder, RuntimeProxy};
//! # use turbomcp_proxy::config::{BackendConfig, FrontendType};
//! # async fn example() -> turbomcp_proxy::ProxyResult<()> {
//! let proxy = RuntimeProxyBuilder::new()
//!     .with_stdio_backend("python", vec!["server.py".to_string()])
//!     .with_http_frontend("127.0.0.1:3000")
//!     .build()
//!     .await?;
//!
//! // proxy.run().await?;
//! # Ok(())
//! # }
//! ```

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, trace};
use turbomcp_protocol::jsonrpc::{
    JsonRpcError, JsonRpcErrorCode, JsonRpcRequest, JsonRpcResponse, JsonRpcResponsePayload,
    ResponseId,
};
use turbomcp_protocol::types::RequestId;
use turbomcp_protocol::types::{CallToolRequest, GetPromptRequest, ReadResourceRequest};
use turbomcp_protocol::{Error as McpError, Result as McpResult};

use crate::config::{BackendConfig, FrontendType};
use crate::error::{ProxyError, ProxyResult};
use crate::proxy::{AtomicMetrics, BackendConnector, BackendTransport, ProxyService};

/// Maximum request size in bytes (10 MB)
pub const MAX_REQUEST_SIZE: usize = 10 * 1024 * 1024;

/// Default timeout in milliseconds (30 seconds)
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Maximum timeout in milliseconds (5 minutes)
pub const MAX_TIMEOUT_MS: u64 = 300_000;

/// Allowed commands for STDIO backends (security allowlist)
///
/// Only these commands are permitted to prevent command injection attacks.
/// Add new commands here with careful security review.
pub const ALLOWED_COMMANDS: &[&str] = &["python", "python3", "node", "deno", "uv", "npx", "bun"];

/// Secure default bind address (localhost only)
pub const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:3000";

/// Runtime proxy builder following `TurboMCP` builder pattern
///
/// Provides a fluent API for constructing runtime proxies with:
/// - Comprehensive security validation
/// - Sensible defaults
/// - Type-safe configuration
#[derive(Debug)]
pub struct RuntimeProxyBuilder {
    backend_config: Option<BackendConfig>,
    frontend_type: Option<FrontendType>,
    bind_address: Option<String>,
    request_size_limit: usize,
    timeout_ms: u64,
    enable_metrics: bool,
}

impl RuntimeProxyBuilder {
    /// Create a new runtime proxy builder with secure defaults
    #[must_use]
    pub fn new() -> Self {
        Self {
            backend_config: None,
            frontend_type: None,
            bind_address: Some(DEFAULT_BIND_ADDRESS.to_string()),
            request_size_limit: MAX_REQUEST_SIZE,
            timeout_ms: DEFAULT_TIMEOUT_MS,
            enable_metrics: true,
        }
    }

    /// Configure a STDIO backend (subprocess)
    ///
    /// # Arguments
    ///
    /// * `command` - Command to execute (must be in allowlist)
    /// * `args` - Command arguments
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_proxy::runtime::RuntimeProxyBuilder;
    /// let builder = RuntimeProxyBuilder::new()
    ///     .with_stdio_backend("python", vec!["server.py".to_string()]);
    /// ```
    #[must_use]
    pub fn with_stdio_backend(mut self, command: impl Into<String>, args: Vec<String>) -> Self {
        self.backend_config = Some(BackendConfig::Stdio {
            command: command.into(),
            args,
            working_dir: None,
        });
        self
    }

    /// Configure a STDIO backend with working directory
    ///
    /// # Arguments
    ///
    /// * `command` - Command to execute (must be in allowlist)
    /// * `args` - Command arguments
    /// * `working_dir` - Working directory for the subprocess
    #[must_use]
    pub fn with_stdio_backend_and_dir(
        mut self,
        command: impl Into<String>,
        args: Vec<String>,
        working_dir: impl Into<String>,
    ) -> Self {
        self.backend_config = Some(BackendConfig::Stdio {
            command: command.into(),
            args,
            working_dir: Some(working_dir.into()),
        });
        self
    }

    /// Configure an HTTP backend
    ///
    /// # Arguments
    ///
    /// * `url` - Base URL of the HTTP server (HTTPS required for non-localhost)
    /// * `auth_token` - Optional authentication token
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_proxy::runtime::RuntimeProxyBuilder;
    /// let builder = RuntimeProxyBuilder::new()
    ///     .with_http_backend("https://api.example.com", Some("token123".to_string()));
    /// ```
    #[must_use]
    pub fn with_http_backend(mut self, url: impl Into<String>, auth_token: Option<String>) -> Self {
        self.backend_config = Some(BackendConfig::Http {
            url: url.into(),
            auth_token,
        });
        self
    }

    /// Configure a WebSocket backend
    ///
    /// # Arguments
    ///
    /// * `url` - WebSocket URL (e.g., "<ws://localhost:8080>" or "<wss://server.example.com>")
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_proxy::runtime::RuntimeProxyBuilder;
    /// let builder = RuntimeProxyBuilder::new()
    ///     .with_websocket_backend("wss://mcp.example.com");
    /// ```
    #[must_use]
    pub fn with_websocket_backend(mut self, url: impl Into<String>) -> Self {
        self.backend_config = Some(BackendConfig::WebSocket { url: url.into() });
        self
    }

    /// Configure a TCP backend
    ///
    /// # Arguments
    ///
    /// * `host` - Host or IP address to connect to
    /// * `port` - Port number
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_proxy::runtime::RuntimeProxyBuilder;
    /// let builder = RuntimeProxyBuilder::new()
    ///     .with_tcp_backend("localhost", 5000);
    /// ```
    #[must_use]
    pub fn with_tcp_backend(mut self, host: impl Into<String>, port: u16) -> Self {
        self.backend_config = Some(BackendConfig::Tcp {
            host: host.into(),
            port,
        });
        self
    }

    /// Configure a Unix domain socket backend
    ///
    /// # Arguments
    ///
    /// * `path` - Path to Unix socket file
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_proxy::runtime::RuntimeProxyBuilder;
    /// let builder = RuntimeProxyBuilder::new()
    ///     .with_unix_backend("/tmp/mcp.sock");
    /// ```
    #[must_use]
    pub fn with_unix_backend(mut self, path: impl Into<String>) -> Self {
        self.backend_config = Some(BackendConfig::Unix { path: path.into() });
        self
    }

    /// Configure an HTTP frontend
    ///
    /// # Arguments
    ///
    /// * `bind` - Address to bind to (e.g., "127.0.0.1:3000")
    ///
    /// # Security Note
    ///
    /// Default is localhost-only. Only bind to 0.0.0.0 if you have proper
    /// authentication and network security in place.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_proxy::runtime::RuntimeProxyBuilder;
    /// let builder = RuntimeProxyBuilder::new()
    ///     .with_http_frontend("127.0.0.1:8080");
    /// ```
    #[must_use]
    pub fn with_http_frontend(mut self, bind: impl Into<String>) -> Self {
        self.frontend_type = Some(FrontendType::Http);
        self.bind_address = Some(bind.into());
        self
    }

    /// Configure a STDIO frontend
    ///
    /// Reads JSON-RPC from stdin, writes to stdout. Ideal for CLI tools.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_proxy::runtime::RuntimeProxyBuilder;
    /// let builder = RuntimeProxyBuilder::new()
    ///     .with_stdio_frontend();
    /// ```
    #[must_use]
    pub fn with_stdio_frontend(mut self) -> Self {
        self.frontend_type = Some(FrontendType::Stdio);
        self
    }

    /// Configure a WebSocket frontend
    ///
    /// Bidirectional WebSocket server for real-time communication.
    /// Ideal for browser clients and bidirectional elicitation.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_proxy::runtime::RuntimeProxyBuilder;
    /// let builder = RuntimeProxyBuilder::new()
    ///     .with_websocket_frontend("127.0.0.1:8080");
    /// ```
    #[must_use]
    pub fn with_websocket_frontend(mut self, bind: impl Into<String>) -> Self {
        self.frontend_type = Some(FrontendType::WebSocket);
        self.bind_address = Some(bind.into());
        self
    }

    /// Set maximum request size limit
    ///
    /// # Arguments
    ///
    /// * `limit` - Maximum size in bytes (default: 10 MB)
    ///
    /// # Security Note
    ///
    /// Prevents memory exhaustion from large requests.
    #[must_use]
    pub fn with_request_size_limit(mut self, limit: usize) -> Self {
        self.request_size_limit = limit;
        self
    }

    /// Set request timeout
    ///
    /// # Arguments
    ///
    /// * `timeout_ms` - Timeout in milliseconds (max: 5 minutes)
    ///
    /// # Errors
    ///
    /// Returns an error if timeout exceeds maximum.
    pub fn with_timeout(mut self, timeout_ms: u64) -> ProxyResult<Self> {
        if timeout_ms > MAX_TIMEOUT_MS {
            return Err(ProxyError::configuration_with_key(
                format!("Timeout {timeout_ms}ms exceeds maximum {MAX_TIMEOUT_MS}ms"),
                "timeout_ms",
            ));
        }
        self.timeout_ms = timeout_ms;
        Ok(self)
    }

    /// Enable or disable metrics collection
    ///
    /// # Arguments
    ///
    /// * `enable` - Whether to collect metrics (default: true)
    #[must_use]
    pub fn with_metrics(mut self, enable: bool) -> Self {
        self.enable_metrics = enable;
        self
    }

    /// Build and validate the runtime proxy
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Backend configuration is missing
    /// - Frontend type is missing
    /// - Security validation fails (command not in allowlist, invalid URL, etc.)
    /// - Backend connection fails
    ///
    /// # Panics
    ///
    /// Panics if `backend_config` is None after successful validation (should never happen as validation ensures it's Some).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use turbomcp_proxy::runtime::RuntimeProxyBuilder;
    /// # async fn example() -> turbomcp_proxy::ProxyResult<()> {
    /// let proxy = RuntimeProxyBuilder::new()
    ///     .with_stdio_backend("python", vec!["server.py".to_string()])
    ///     .with_http_frontend("127.0.0.1:3000")
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn build(self) -> ProxyResult<RuntimeProxy> {
        // Ensure required fields are set
        let backend_config = self
            .backend_config
            .as_ref()
            .ok_or_else(|| ProxyError::configuration("Backend configuration is required"))?;

        let frontend_type = self
            .frontend_type
            .ok_or_else(|| ProxyError::configuration("Frontend type is required"))?;

        // Validate security constraints
        Self::validate_command(backend_config)?;
        Self::validate_url(backend_config)?;
        Self::validate_working_dir(backend_config)?;

        // Take ownership after validation
        let backend_config = self.backend_config.unwrap();

        // Convert BackendConfig to BackendTransport for BackendConnector
        let transport = match &backend_config {
            BackendConfig::Stdio {
                command,
                args,
                working_dir,
            } => BackendTransport::Stdio {
                command: command.clone(),
                args: args.clone(),
                working_dir: working_dir.clone(),
            },
            BackendConfig::Http { url, auth_token } => BackendTransport::Http {
                url: url.clone(),
                auth_token: auth_token.clone(),
            },
            BackendConfig::Tcp { host, port } => BackendTransport::Tcp {
                host: host.clone(),
                port: *port,
            },
            BackendConfig::Unix { path } => BackendTransport::Unix { path: path.clone() },
            BackendConfig::WebSocket { url } => BackendTransport::WebSocket { url: url.clone() },
        };

        // Create BackendConnector configuration
        let connector_config = crate::proxy::backend::BackendConfig {
            transport,
            client_name: "turbomcp-proxy".to_string(),
            client_version: crate::VERSION.to_string(),
        };

        // Create backend connector
        let backend = BackendConnector::new(connector_config).await?;

        // Create metrics if enabled
        let metrics = if self.enable_metrics {
            Some(Arc::new(AtomicMetrics::new()))
        } else {
            None
        };

        Ok(RuntimeProxy {
            backend,
            frontend_type,
            bind_address: self.bind_address,
            request_size_limit: self.request_size_limit,
            timeout_ms: self.timeout_ms,
            metrics,
        })
    }

    /// Validate command is in allowlist (SECURITY CRITICAL)
    fn validate_command(config: &BackendConfig) -> ProxyResult<()> {
        if let BackendConfig::Stdio { command, .. } = config
            && !ALLOWED_COMMANDS.contains(&command.as_str())
        {
            return Err(ProxyError::configuration_with_key(
                format!("Command '{command}' not in allowlist. Allowed: {ALLOWED_COMMANDS:#?}"),
                "command",
            ));
        }
        Ok(())
    }

    /// Validate URL for SSRF protection (SECURITY CRITICAL)
    fn validate_url(config: &BackendConfig) -> ProxyResult<()> {
        if let BackendConfig::Http { url, .. } = config {
            let parsed = url::Url::parse(url).map_err(|e| {
                ProxyError::configuration_with_key(format!("Invalid URL: {e}"), "url")
            })?;

            // Require HTTPS except for localhost
            if parsed.scheme() != "https" {
                let host = parsed.host_str().unwrap_or("");
                if !is_localhost(host) {
                    return Err(ProxyError::configuration_with_key(
                        format!(
                            "HTTPS required for non-localhost URLs. Got: {scheme}",
                            scheme = parsed.scheme()
                        ),
                        "url",
                    ));
                }
            }

            // Block private IP ranges and metadata endpoints
            if let Some(host) = parsed.host_str() {
                Self::validate_host(host)?;
            }
        }
        Ok(())
    }

    /// Validate host is not private/metadata (SECURITY CRITICAL)
    fn validate_host(host: &str) -> ProxyResult<()> {
        // Block AWS metadata endpoint
        if host == "169.254.169.254" {
            return Err(ProxyError::configuration_with_key(
                "AWS metadata endpoint not allowed",
                "url",
            ));
        }

        // Block GCP metadata endpoint
        if host == "metadata.google.internal" || host == "169.254.169.254" {
            return Err(ProxyError::configuration_with_key(
                "GCP metadata endpoint not allowed",
                "url",
            ));
        }

        // Parse IP address and check for private ranges
        if let Ok(ip) = host.parse::<Ipv4Addr>()
            && (ip.is_private() || ip.is_loopback() || ip.is_link_local())
        {
            // Allow localhost/loopback explicitly
            if !ip.is_loopback() {
                return Err(ProxyError::configuration_with_key(
                    format!("Private IP address not allowed: {ip}"),
                    "url",
                ));
            }
        }

        Ok(())
    }

    /// Validate working directory (path traversal protection)
    fn validate_working_dir(config: &BackendConfig) -> ProxyResult<()> {
        if let BackendConfig::Stdio {
            working_dir: Some(wd),
            ..
        } = config
        {
            let path = PathBuf::from(wd);

            // Ensure path exists
            if !path.exists() {
                return Err(ProxyError::configuration_with_key(
                    format!("Working directory does not exist: {wd}"),
                    "working_dir",
                ));
            }

            // Canonicalize to resolve symlinks and relative paths
            let canonical = path.canonicalize().map_err(|e| {
                ProxyError::configuration_with_key(
                    format!("Failed to canonicalize working directory: {e}"),
                    "working_dir",
                )
            })?;

            // Additional validation: ensure it's a directory
            if !canonical.is_dir() {
                return Err(ProxyError::configuration_with_key(
                    format!("Working directory is not a directory: {wd}"),
                    "working_dir",
                ));
            }
        }
        Ok(())
    }
}

impl Default for RuntimeProxyBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if host is localhost
fn is_localhost(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// Runtime proxy instance
///
/// Manages the proxy lifecycle, routing requests between frontend and backend.
pub struct RuntimeProxy {
    /// Backend connector
    backend: BackendConnector,

    /// Frontend type
    frontend_type: FrontendType,

    /// Bind address (for HTTP frontend)
    bind_address: Option<String>,

    /// Request size limit
    request_size_limit: usize,

    /// Request timeout
    timeout_ms: u64,

    /// Metrics collector
    metrics: Option<Arc<AtomicMetrics>>,
}

impl RuntimeProxy {
    /// Run the proxy
    ///
    /// Starts the appropriate frontend based on configuration and runs
    /// until stopped or an error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if the frontend fails to start or encounters
    /// a fatal error during operation.
    pub async fn run(&mut self) -> ProxyResult<()> {
        match self.frontend_type {
            FrontendType::Http => {
                let bind = self
                    .bind_address
                    .as_ref()
                    .ok_or_else(|| {
                        ProxyError::configuration("Bind address required for HTTP frontend")
                    })?
                    .clone();
                self.run_http(&bind).await
            }
            FrontendType::Stdio => self.run_stdio().await,
            FrontendType::WebSocket => {
                let bind = self
                    .bind_address
                    .as_ref()
                    .ok_or_else(|| {
                        ProxyError::configuration("Bind address required for WebSocket frontend")
                    })?
                    .clone();
                self.run_websocket(&bind).await
            }
        }
    }

    /// Get reference to backend connector
    #[must_use]
    pub fn backend(&self) -> &BackendConnector {
        &self.backend
    }

    /// Get metrics snapshot
    #[must_use]
    pub fn metrics(&self) -> Option<crate::proxy::metrics::ProxyMetrics> {
        self.metrics.as_ref().map(|m| m.snapshot())
    }

    /// Run HTTP frontend using Axum and `ProxyService`
    async fn run_http(&mut self, bind: &str) -> ProxyResult<()> {
        use axum::Router;
        use std::time::Duration;
        use tower_http::limit::RequestBodyLimitLayer;
        use tower_http::timeout::TimeoutLayer;
        use turbomcp_transport::axum::AxumMcpExt;

        debug!("Starting HTTP frontend on {}", bind);

        // 1. Introspect backend to get ServerSpec
        let spec = self.backend.introspect().await?;

        debug!(
            "Backend introspection complete: {} tools, {} resources, {} prompts",
            spec.tools.len(),
            spec.resources.len(),
            spec.prompts.len()
        );

        // 2. Create ProxyService (takes ownership, so clone backend)
        let service = ProxyService::new(self.backend.clone(), spec);

        // 3. Create Axum router with MCP routes and security layers
        // Note: Security layers applied in both STDIO and HTTP frontends:
        //   - request_size_limit: Prevents memory exhaustion DoS
        //   - timeout_ms: Prevents hanging requests (STDIO uses tokio::time::timeout, HTTP uses Tower layer)
        let app = Router::new()
            .turbo_mcp_routes(service)
            .layer(RequestBodyLimitLayer::new(self.request_size_limit))
            .layer(TimeoutLayer::new(Duration::from_millis(self.timeout_ms)));

        // 4. Parse bind address
        let listener = tokio::net::TcpListener::bind(bind).await.map_err(|e| {
            ProxyError::backend_connection(format!("Failed to bind to {bind}: {e}"))
        })?;

        debug!("HTTP frontend listening on {}", bind);

        // 5. Start Axum server
        axum::serve(listener, app)
            .await
            .map_err(|e| ProxyError::backend(format!("Axum serve error: {e}")))?;

        Ok(())
    }

    /// Run WebSocket frontend using Axum and `ProxyService`
    async fn run_websocket(&mut self, bind: &str) -> ProxyResult<()> {
        use axum::Router;
        use std::time::Duration;
        use tower_http::limit::RequestBodyLimitLayer;
        use tower_http::timeout::TimeoutLayer;
        use turbomcp_transport::axum::AxumMcpExt;

        debug!("Starting WebSocket frontend on {}", bind);

        // 1. Introspect backend to get ServerSpec
        let spec = self.backend.introspect().await?;

        debug!(
            "Backend introspection complete: {} tools, {} resources, {} prompts",
            spec.tools.len(),
            spec.resources.len(),
            spec.prompts.len()
        );

        // 2. Create ProxyService (takes ownership, so clone backend)
        let service = ProxyService::new(self.backend.clone(), spec);

        // 3. Create Axum router with MCP routes (WebSocket support included via AxumMcpExt)
        // Note: turbo_mcp_routes() provides both HTTP/SSE and WebSocket endpoints
        // Security layers applied:
        //   - request_size_limit: Prevents memory exhaustion DoS
        //   - timeout_ms: Prevents hanging WebSocket connections
        let app = Router::new()
            .turbo_mcp_routes(service)
            .layer(RequestBodyLimitLayer::new(self.request_size_limit))
            .layer(TimeoutLayer::new(Duration::from_millis(self.timeout_ms)));

        // 4. Parse bind address
        let listener = tokio::net::TcpListener::bind(bind).await.map_err(|e| {
            ProxyError::backend_connection(format!("Failed to bind to {bind}: {e}"))
        })?;

        debug!("WebSocket frontend listening on {}", bind);

        // 5. Start Axum server
        axum::serve(listener, app)
            .await
            .map_err(|e| ProxyError::backend(format!("Axum serve error: {e}")))?;

        Ok(())
    }

    /// Create error response for oversized requests
    fn create_size_limit_error(n: usize) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
            payload: JsonRpcResponsePayload::Error {
                error: JsonRpcError {
                    code: JsonRpcErrorCode::InvalidRequest.code(),
                    message: format!("Request too large: {n} bytes"),
                    data: None,
                },
            },
            id: ResponseId::null(),
        }
    }

    /// Create response for a routed request
    fn create_response(
        result: Result<Result<Value, Box<McpError>>, tokio::time::error::Elapsed>,
        request_id: RequestId,
        timeout_ms: u64,
    ) -> JsonRpcResponse {
        match result {
            Ok(Ok(value)) => JsonRpcResponse::success(value, request_id),
            Ok(Err(mcp_error)) => JsonRpcResponse::error_response(
                JsonRpcError {
                    code: JsonRpcErrorCode::InternalError.code(),
                    message: mcp_error.to_string(),
                    data: None,
                },
                request_id,
            ),
            Err(_) => JsonRpcResponse::error_response(
                JsonRpcError {
                    code: JsonRpcErrorCode::InternalError.code(),
                    message: format!("Request timeout after {timeout_ms}ms"),
                    data: None,
                },
                request_id,
            ),
        }
    }

    /// Write a response to stdout and return success/failure indicator
    async fn write_response_to_stdout(
        stdout: &mut tokio::io::Stdout,
        response: &JsonRpcResponse,
    ) -> Result<(), String> {
        let json = serde_json::to_string(response)
            .map_err(|e| format!("Failed to serialize response: {e}"))?;

        stdout
            .write_all(json.as_bytes())
            .await
            .map_err(|e| format!("Failed to write response: {e}"))?;

        stdout
            .write_all(b"\n")
            .await
            .map_err(|e| format!("Failed to write newline: {e}"))?;

        stdout
            .flush()
            .await
            .map_err(|e| format!("Failed to flush stdout: {e}"))?;

        trace!("STDIO: Sent response: {json}");
        Ok(())
    }

    /// Process a single request line from stdin
    async fn process_request_line(
        &mut self,
        line: &str,
        stdout: &mut tokio::io::Stdout,
    ) -> Result<(), String> {
        let request: JsonRpcRequest = serde_json::from_str(line)
            .map_err(|e| format!("STDIO: Failed to parse JSON-RPC: {e}"))?;

        let request_id = request.id.clone();

        // Route request to backend with timeout
        let timeout = Duration::from_millis(self.timeout_ms);
        let result = tokio::time::timeout(timeout, self.route_request(&request)).await;

        // Create and send response
        let response = Self::create_response(result, request_id, self.timeout_ms);
        Self::write_response_to_stdout(stdout, &response).await?;

        // Update metrics
        if let Some(ref metrics) = self.metrics {
            metrics.inc_requests_forwarded();
        }

        Ok(())
    }

    /// Run STDIO frontend
    async fn run_stdio(&mut self) -> ProxyResult<()> {
        debug!("Starting STDIO frontend");

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    debug!("STDIO: EOF reached, shutting down");
                    break;
                }
                Ok(n) => {
                    // Check size limit
                    if n > self.request_size_limit {
                        error!(
                            "STDIO: Request size {} exceeds limit {}",
                            n, self.request_size_limit
                        );

                        let error_response = Self::create_size_limit_error(n);
                        if let Ok(json) = serde_json::to_string(&error_response) {
                            let _ = stdout.write_all(json.as_bytes()).await;
                            let _ = stdout.write_all(b"\n").await;
                            let _ = stdout.flush().await;
                        }
                        continue;
                    }

                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    trace!("STDIO: Received request: {}", trimmed);

                    // Process request and handle errors
                    match self.process_request_line(trimmed, &mut stdout).await {
                        Ok(()) => {}
                        Err(e)
                            if e.contains("Failed to write") || e.contains("Failed to flush") =>
                        {
                            error!("STDIO: {e}");
                            break;
                        }
                        Err(e) => {
                            error!("{e}");
                            // Send parse error response for invalid JSON-RPC
                            let error_response = JsonRpcResponse::parse_error(None);
                            if let Ok(json) = serde_json::to_string(&error_response) {
                                let _ = stdout.write_all(json.as_bytes()).await;
                                let _ = stdout.write_all(b"\n").await;
                                let _ = stdout.flush().await;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("STDIO: Read error: {}", e);
                    break;
                }
            }
        }

        debug!("STDIO frontend shut down");
        Ok(())
    }

    /// Route a JSON-RPC request to the backend
    async fn route_request(&mut self, request: &JsonRpcRequest) -> McpResult<Value> {
        trace!("Routing request: method={}", request.method);

        match request.method.as_str() {
            // Tools
            "tools/list" => {
                debug!("Forwarding tools/list to backend");
                let tools = self
                    .backend
                    .list_tools()
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::json!({
                    "tools": tools
                }))
            }

            "tools/call" => {
                debug!("Forwarding tools/call to backend");
                let params = request.params.as_ref().ok_or_else(|| {
                    McpError::invalid_params("Missing params for tools/call".to_string())
                })?;

                let call_request: CallToolRequest = serde_json::from_value(params.clone())
                    .map_err(|e| McpError::invalid_params(e.to_string()))?;

                let result = self
                    .backend
                    .call_tool(&call_request.name, call_request.arguments)
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::to_value(result).map_err(|e| McpError::internal(e.to_string()))?)
            }

            // Resources
            "resources/list" => {
                debug!("Forwarding resources/list to backend");
                let resources = self
                    .backend
                    .list_resources()
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::json!({
                    "resources": resources
                }))
            }

            "resources/read" => {
                debug!("Forwarding resources/read to backend");
                let params = request.params.as_ref().ok_or_else(|| {
                    McpError::invalid_params("Missing params for resources/read".to_string())
                })?;

                let read_request: ReadResourceRequest = serde_json::from_value(params.clone())
                    .map_err(|e| McpError::invalid_params(e.to_string()))?;

                let contents = self
                    .backend
                    .read_resource(&read_request.uri)
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::json!({
                    "contents": contents
                }))
            }

            // Prompts
            "prompts/list" => {
                debug!("Forwarding prompts/list to backend");
                let prompts = self
                    .backend
                    .list_prompts()
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::json!({
                    "prompts": prompts
                }))
            }

            "prompts/get" => {
                debug!("Forwarding prompts/get to backend");
                let params = request.params.as_ref().ok_or_else(|| {
                    McpError::invalid_params("Missing params for prompts/get".to_string())
                })?;

                let get_request: GetPromptRequest = serde_json::from_value(params.clone())
                    .map_err(|e| McpError::invalid_params(e.to_string()))?;

                let result = self
                    .backend
                    .get_prompt(&get_request.name, get_request.arguments)
                    .await
                    .map_err(|e| McpError::internal(e.to_string()))?;

                Ok(serde_json::to_value(result).map_err(|e| McpError::internal(e.to_string()))?)
            }

            // Unknown method
            method => {
                error!("Unknown method: {}", method);
                Err(McpError::protocol(format!("Method not found: {method}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let builder = RuntimeProxyBuilder::new();
        assert_eq!(builder.request_size_limit, MAX_REQUEST_SIZE);
        assert_eq!(builder.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert!(builder.enable_metrics);
    }

    #[test]
    fn test_builder_with_stdio_backend() {
        let builder =
            RuntimeProxyBuilder::new().with_stdio_backend("python", vec!["server.py".to_string()]);

        assert!(matches!(
            builder.backend_config,
            Some(BackendConfig::Stdio { .. })
        ));
    }

    #[test]
    fn test_builder_with_http_backend() {
        let builder = RuntimeProxyBuilder::new().with_http_backend("https://api.example.com", None);

        assert!(matches!(
            builder.backend_config,
            Some(BackendConfig::Http { .. })
        ));
    }

    #[test]
    fn test_builder_with_tcp_backend() {
        let builder = RuntimeProxyBuilder::new().with_tcp_backend("localhost", 5000);

        assert!(matches!(
            builder.backend_config,
            Some(BackendConfig::Tcp {
                host: _,
                port: 5000
            })
        ));
    }

    #[test]
    fn test_builder_with_unix_backend() {
        let builder = RuntimeProxyBuilder::new().with_unix_backend("/tmp/mcp.sock");

        assert!(matches!(
            builder.backend_config,
            Some(BackendConfig::Unix { path: _ })
        ));
    }

    #[test]
    fn test_builder_with_frontends() {
        let http_builder = RuntimeProxyBuilder::new().with_http_frontend("0.0.0.0:3000");
        assert_eq!(http_builder.frontend_type, Some(FrontendType::Http));

        let stdio_builder = RuntimeProxyBuilder::new().with_stdio_frontend();
        assert_eq!(stdio_builder.frontend_type, Some(FrontendType::Stdio));
    }

    #[test]
    fn test_builder_with_timeout() {
        let result = RuntimeProxyBuilder::new().with_timeout(60_000);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().timeout_ms, 60_000);
    }

    #[test]
    fn test_builder_timeout_exceeds_max() {
        let result = RuntimeProxyBuilder::new().with_timeout(MAX_TIMEOUT_MS + 1);
        assert!(result.is_err());
        match result {
            Err(ProxyError::Configuration { key, .. }) => {
                assert_eq!(key, Some("timeout_ms".to_string()));
            }
            _ => panic!("Expected Configuration error"),
        }
    }

    #[test]
    fn test_validate_command_allowed() {
        let config = BackendConfig::Stdio {
            command: "python".to_string(),
            args: vec![],
            working_dir: None,
        };

        assert!(RuntimeProxyBuilder::validate_command(&config).is_ok());
    }

    #[test]
    fn test_validate_command_not_allowed() {
        let config = BackendConfig::Stdio {
            command: "malicious_command".to_string(),
            args: vec![],
            working_dir: None,
        };

        let result = RuntimeProxyBuilder::validate_command(&config);
        assert!(result.is_err());
        match result {
            Err(ProxyError::Configuration { message, key }) => {
                assert!(message.contains("not in allowlist"));
                assert_eq!(key, Some("command".to_string()));
            }
            _ => panic!("Expected Configuration error"),
        }
    }

    #[test]
    fn test_validate_url_https_required() {
        let config = BackendConfig::Http {
            url: "http://api.example.com".to_string(),
            auth_token: None,
        };

        let result = RuntimeProxyBuilder::validate_url(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_url_localhost_http_allowed() {
        let config = BackendConfig::Http {
            url: "http://localhost:3000".to_string(),
            auth_token: None,
        };

        assert!(RuntimeProxyBuilder::validate_url(&config).is_ok());
    }

    #[test]
    fn test_validate_url_https_allowed() {
        let config = BackendConfig::Http {
            url: "https://api.example.com".to_string(),
            auth_token: None,
        };

        assert!(RuntimeProxyBuilder::validate_url(&config).is_ok());
    }

    #[test]
    fn test_validate_host_blocks_metadata() {
        // AWS metadata endpoint
        assert!(RuntimeProxyBuilder::validate_host("169.254.169.254").is_err());

        // GCP metadata endpoint
        assert!(RuntimeProxyBuilder::validate_host("metadata.google.internal").is_err());
    }

    #[test]
    fn test_validate_host_blocks_private_ips() {
        // Private IP ranges
        assert!(RuntimeProxyBuilder::validate_host("192.168.1.1").is_err());
        assert!(RuntimeProxyBuilder::validate_host("10.0.0.1").is_err());
        assert!(RuntimeProxyBuilder::validate_host("172.16.0.1").is_err());
    }

    #[test]
    fn test_validate_host_allows_loopback() {
        assert!(RuntimeProxyBuilder::validate_host("127.0.0.1").is_ok());
    }

    #[test]
    fn test_is_localhost() {
        assert!(is_localhost("localhost"));
        assert!(is_localhost("127.0.0.1"));
        assert!(is_localhost("::1"));
        assert!(is_localhost("[::1]"));
        assert!(!is_localhost("example.com"));
        assert!(!is_localhost("192.168.1.1"));
    }

    #[tokio::test]
    async fn test_builder_requires_backend() {
        let result = RuntimeProxyBuilder::new()
            .with_http_frontend("127.0.0.1:3000")
            .build()
            .await;

        assert!(result.is_err());
        match result {
            Err(ProxyError::Configuration { message, .. }) => {
                assert!(message.contains("Backend configuration is required"));
            }
            _ => panic!("Expected Configuration error"),
        }
    }

    #[tokio::test]
    async fn test_builder_requires_frontend() {
        let result = RuntimeProxyBuilder::new()
            .with_stdio_backend("python", vec!["server.py".to_string()])
            .build()
            .await;

        assert!(result.is_err());
        match result {
            Err(ProxyError::Configuration { message, .. }) => {
                assert!(message.contains("Frontend type is required"));
            }
            _ => panic!("Expected Configuration error"),
        }
    }

    #[test]
    fn test_validate_working_dir_nonexistent() {
        let config = BackendConfig::Stdio {
            command: "python".to_string(),
            args: vec![],
            working_dir: Some("/nonexistent/path/that/does/not/exist".to_string()),
        };

        let result = RuntimeProxyBuilder::validate_working_dir(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_constants() {
        assert_eq!(MAX_REQUEST_SIZE, 10 * 1024 * 1024);
        assert_eq!(DEFAULT_TIMEOUT_MS, 30_000);
        assert_eq!(MAX_TIMEOUT_MS, 300_000);
        assert_eq!(DEFAULT_BIND_ADDRESS, "127.0.0.1:3000");
        assert!(ALLOWED_COMMANDS.contains(&"python"));
        assert!(ALLOWED_COMMANDS.contains(&"node"));
    }
}

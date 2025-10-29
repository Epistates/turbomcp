//! Serve command implementation
//!
//! Runs the proxy server to bridge MCP servers across transports.

use axum::Router;
use clap::Args;
use secrecy::SecretString;
use tracing::info;

use crate::cli::args::BackendArgs;
use crate::error::{ProxyError, ProxyResult};
use crate::proxy::backends::http::{HttpBackend, HttpBackendConfig};
use crate::proxy::frontends::stdio::{StdioFrontend, StdioFrontendConfig};
use crate::proxy::{BackendConfig, BackendConnector, BackendTransport, ProxyService};

/// Serve a proxy server to bridge MCP transports
///
/// This command connects to a backend MCP server (e.g., STDIO) and exposes
/// it on a different transport (e.g., HTTP/SSE), enabling web clients to
/// access STDIO-only servers.
///
/// # Examples
///
/// Expose a Python MCP server on HTTP:
///   turbomcp-proxy serve \
///     --backend stdio --cmd python --args server.py \
///     --frontend http --bind 0.0.0.0:3000
///
/// With custom path:
///   turbomcp-proxy serve \
///     --backend stdio --cmd python --args server.py \
///     --frontend http --bind 127.0.0.1:8080 --path /api/mcp
#[derive(Debug, Args)]
pub struct ServeCommand {
    /// Backend configuration
    #[command(flatten)]
    pub backend: BackendArgs,

    /// Frontend transport type
    #[arg(long, value_name = "TYPE", default_value = "http")]
    pub frontend: String,

    /// Bind address for HTTP/WebSocket frontend.
    ///
    /// Default: 127.0.0.1:3000 (localhost only for security)
    ///
    /// WARNING: Binding to 0.0.0.0 exposes the proxy to all network interfaces.
    /// Only use 0.0.0.0 if you have proper authentication/authorization in place.
    #[arg(long, value_name = "ADDR", default_value = "127.0.0.1:3000")]
    pub bind: String,

    /// HTTP endpoint path (for HTTP frontend)
    #[arg(long, value_name = "PATH", default_value = "/mcp")]
    pub path: String,

    /// Client name to send during initialization
    #[arg(long, default_value = "turbomcp-proxy")]
    pub client_name: String,

    /// Client version to send during initialization
    #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
    pub client_version: String,

    /// Authentication token for HTTP backend (Bearer token)
    #[arg(long, value_name = "TOKEN")]
    pub auth_token: Option<String>,
}

impl ServeCommand {
    /// Execute the serve command
    pub async fn execute(self) -> ProxyResult<()> {
        // Validate backend arguments
        self.backend
            .validate()
            .map_err(|e| ProxyError::configuration(e))?;

        info!(
            backend = ?self.backend.backend_type(),
            frontend = %self.frontend,
            bind = %self.bind,
            "Starting proxy server"
        );

        // Handle different frontend types
        match self.frontend.as_str() {
            "http" => self.execute_http_frontend().await,
            "stdio" => self.execute_stdio_frontend().await,
            _ => Err(ProxyError::configuration(format!(
                "Frontend transport '{}' not yet supported. Use 'http' or 'stdio'.",
                self.frontend
            ))),
        }
    }

    /// Execute with HTTP frontend (Phase 2: STDIO → HTTP)
    async fn execute_http_frontend(&self) -> ProxyResult<()> {
        use crate::cli::args::BackendType;

        // Only STDIO backend is supported for HTTP frontend
        if self.backend.backend_type() != Some(BackendType::Stdio) {
            return Err(ProxyError::configuration(
                "HTTP frontend currently only supports STDIO backend".to_string(),
            ));
        }

        // Create backend config
        let backend_config = self.create_backend_config()?;

        // Create backend connector
        info!("Connecting to backend...");
        let mut backend = BackendConnector::new(backend_config).await?;
        info!("Backend connected successfully");

        // Introspect backend
        info!("Introspecting backend capabilities...");
        let spec = backend.introspect().await?;
        info!(
            "Backend introspection complete: {} tools, {} resources, {} prompts",
            spec.tools.len(),
            spec.resources.len(),
            spec.prompts.len()
        );

        // Create proxy service
        let proxy_service = ProxyService::new(backend, spec);

        // Create Axum router with MCP routes
        use turbomcp_transport::axum::{AxumMcpExt, McpServerConfig};

        info!("Building HTTP server with Axum MCP integration...");
        let app = Router::new().turbo_mcp_routes_with_config(
            proxy_service,
            McpServerConfig {
                enable_compression: true,
                enable_tracing: true,
                ..Default::default()
            },
        );

        // Parse bind address
        let addr: std::net::SocketAddr = self
            .bind
            .parse()
            .map_err(|e| ProxyError::configuration(format!("Invalid bind address: {}", e)))?;

        info!("Proxy server listening on http://{}/mcp", addr);
        info!("Backend: STDIO subprocess");
        info!("Frontend: HTTP/SSE");
        info!("MCP endpoints:");
        info!("  POST   /mcp          - JSON-RPC");
        info!("  GET    /mcp/sse      - Server-Sent Events");
        info!("  GET    /mcp/health   - Health check");

        // Run HTTP server using axum::serve
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to bind to {}: {}", addr, e)))?;

        axum::serve(listener, app)
            .await
            .map_err(|e| ProxyError::backend(format!("HTTP server error: {}", e)))?;

        Ok(())
    }

    /// Execute with STDIO frontend (Phase 3: HTTP → STDIO)
    async fn execute_stdio_frontend(&self) -> ProxyResult<()> {
        use crate::cli::args::BackendType;

        // Only HTTP backend is supported for STDIO frontend
        if self.backend.backend_type() != Some(BackendType::Http) {
            return Err(ProxyError::configuration(
                "STDIO frontend currently only supports HTTP backend".to_string(),
            ));
        }

        let url = self
            .backend
            .http
            .as_ref()
            .ok_or_else(|| ProxyError::configuration("HTTP URL not specified".to_string()))?;

        info!("Creating HTTP backend client for URL: {}", url);

        // Create HTTP backend config
        let http_config = HttpBackendConfig {
            url: url.clone(),
            auth_token: self.auth_token.clone().map(SecretString::from),
            timeout_secs: Some(30),
            client_name: self.client_name.clone(),
            client_version: self.client_version.clone(),
        };

        // Create HTTP backend
        let http_backend = HttpBackend::new(http_config).await?;
        info!("HTTP backend connected successfully");

        // Create STDIO frontend
        let stdio_frontend = StdioFrontend::new(http_backend, StdioFrontendConfig::default());

        info!("Starting STDIO frontend...");
        info!("Backend: HTTP ({})", url);
        info!("Frontend: STDIO (stdin/stdout)");
        info!("Reading JSON-RPC requests from stdin...");

        // Run STDIO event loop
        stdio_frontend.run().await?;

        info!("STDIO frontend shut down cleanly");
        Ok(())
    }

    /// Create backend configuration from args
    fn create_backend_config(&self) -> ProxyResult<BackendConfig> {
        use crate::cli::args::BackendType;

        let transport = match self.backend.backend_type() {
            Some(BackendType::Stdio) => {
                let cmd = self.backend.cmd.as_ref().ok_or_else(|| {
                    ProxyError::configuration("Command not specified".to_string())
                })?;

                BackendTransport::Stdio {
                    command: cmd.clone(),
                    args: self.backend.args.clone(),
                    working_dir: self
                        .backend
                        .working_dir
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                }
            }
            Some(BackendType::Http) => {
                let url = self.backend.http.as_ref().ok_or_else(|| {
                    ProxyError::configuration("HTTP URL not specified".to_string())
                })?;

                BackendTransport::Http {
                    url: url.clone(),
                    auth_token: None,
                }
            }
            Some(BackendType::Websocket) => {
                #[cfg(feature = "websocket")]
                {
                    let url = self.backend.websocket.as_ref().ok_or_else(|| {
                        ProxyError::configuration("WebSocket URL not specified".to_string())
                    })?;

                    BackendTransport::WebSocket { url: url.clone() }
                }
                #[cfg(not(feature = "websocket"))]
                {
                    return Err(ProxyError::configuration(
                        "WebSocket backend requires the 'websocket' feature".to_string(),
                    ));
                }
            }
            None => {
                return Err(ProxyError::configuration(
                    "No backend specified".to_string(),
                ));
            }
        };

        Ok(BackendConfig {
            transport,
            client_name: self.client_name.clone(),
            client_version: self.client_version.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendType;

    #[test]
    fn test_backend_config_creation() {
        let cmd = ServeCommand {
            backend: BackendArgs {
                backend: Some(BackendType::Stdio),
                cmd: Some("python".to_string()),
                args: vec!["server.py".to_string()],
                working_dir: None,
                http: None,
                websocket: None,
            },
            frontend: "http".to_string(),
            bind: "127.0.0.1:3000".to_string(),
            path: "/mcp".to_string(),
            client_name: "test-proxy".to_string(),
            client_version: "1.0.0".to_string(),
            auth_token: None,
        };

        let config = cmd.create_backend_config();
        assert!(config.is_ok());

        let config = config.unwrap();
        assert_eq!(config.client_name, "test-proxy");
        assert_eq!(config.client_version, "1.0.0");
    }
}

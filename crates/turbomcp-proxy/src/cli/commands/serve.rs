//! Serve command implementation
//!
//! Runs the proxy server to bridge MCP servers across transports.

use axum::Router;
use clap::Args;
use secrecy::SecretString;
use tracing::{info, warn};
use turbomcp_transport::axum::{AxumMcpExt, McpServerConfig, config::AuthConfig};

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

    // ═══════════════════════════════════════════════════
    // AUTHENTICATION (Frontend HTTP Server Protection)
    // ═══════════════════════════════════════════════════
    /// JWT secret for frontend authentication (symmetric HS256/384/512)
    ///
    /// When provided, the HTTP/SSE frontend will require valid JWT tokens.
    /// Tokens must be provided in the Authorization header: `Bearer <token>`
    /// Use this for symmetric algorithms (HS256, HS384, HS512).
    #[arg(long, env = "TURBOMCP_JWT_SECRET", value_name = "SECRET")]
    pub jwt_secret: Option<String>,

    /// JWKS URI for asymmetric JWT validation (RS256/384/512, ES256/384)
    ///
    /// Fetch public keys from this URI for asymmetric JWT validation.
    /// Use this with OAuth providers (Google, GitHub, Auth0, etc.).
    /// Example: <https://accounts.google.com/.well-known/jwks.json>
    #[arg(long, env = "TURBOMCP_JWT_JWKS_URI", value_name = "URI")]
    pub jwt_jwks_uri: Option<String>,

    /// JWT algorithm for validation
    ///
    /// Specify which algorithm to use for JWT validation.
    /// Symmetric: HS256 (default), HS384, HS512
    /// Asymmetric: RS256, RS384, RS512, ES256, ES384
    #[arg(long, value_name = "ALG", default_value = "HS256")]
    pub jwt_algorithm: String,

    /// JWT audience claim validation (aud)
    ///
    /// Require token to have this audience. Can be specified multiple times.
    /// Example: --jwt-audience "<https://api.example.com>"
    #[arg(long, value_name = "AUD")]
    pub jwt_audience: Vec<String>,

    /// JWT issuer claim validation (iss)
    ///
    /// Require token to have this issuer. Can be specified multiple times.
    /// Example: --jwt-issuer "<https://accounts.google.com>"
    #[arg(long, value_name = "ISS")]
    pub jwt_issuer: Vec<String>,

    /// API key header name for frontend authentication
    ///
    /// When used with --require-auth, requests must include this header
    /// with a valid API key. Common values: "x-api-key", "authorization"
    #[arg(long, value_name = "HEADER", default_value = "x-api-key")]
    pub api_key_header: String,

    /// Require authentication for all frontend requests
    ///
    /// When enabled without --jwt-secret or --jwt-jwks-uri, uses API key authentication.
    /// IMPORTANT: Always enable this when binding to 0.0.0.0
    #[arg(long)]
    pub require_auth: bool,
}

impl ServeCommand {
    /// Execute the serve command
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if backend validation fails, runtime initialization fails, or serving fails.
    pub async fn execute(self) -> ProxyResult<()> {
        // Validate backend arguments
        self.backend.validate().map_err(ProxyError::configuration)?;

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

    /// Execute with HTTP frontend
    ///
    /// Exposes a backend MCP server over HTTP/SSE for web clients.
    /// Supports STDIO, HTTP, TCP, Unix, and WebSocket backends.
    #[allow(clippy::too_many_lines)]
    async fn execute_http_frontend(&self) -> ProxyResult<()> {
        // Create backend config
        let backend_config = self.create_backend_config()?;

        // Create backend connector
        info!("Connecting to backend...");
        let backend = BackendConnector::new(backend_config).await?;
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

        // Configure authentication
        let auth_config = if self.require_auth
            || self.jwt_secret.is_some()
            || self.jwt_jwks_uri.is_some()
        {
            if self.jwt_secret.is_some() || self.jwt_jwks_uri.is_some() {
                use turbomcp_transport::axum::config::{JwtAlgorithm, JwtConfig};

                // Parse algorithm
                let algorithm = match self.jwt_algorithm.to_uppercase().as_str() {
                    "HS256" => JwtAlgorithm::HS256,
                    "HS384" => JwtAlgorithm::HS384,
                    "HS512" => JwtAlgorithm::HS512,
                    "RS256" => JwtAlgorithm::RS256,
                    "RS384" => JwtAlgorithm::RS384,
                    "RS512" => JwtAlgorithm::RS512,
                    "ES256" => JwtAlgorithm::ES256,
                    "ES384" => JwtAlgorithm::ES384,
                    other => {
                        return Err(ProxyError::configuration(format!(
                            "Invalid JWT algorithm: {other}. Valid: HS256, HS384, HS512, RS256, RS384, RS512, ES256, ES384"
                        )));
                    }
                };

                // Build JWT config
                let jwt_config = JwtConfig {
                    secret: self.jwt_secret.clone(),
                    jwks_uri: self.jwt_jwks_uri.clone(),
                    algorithm,
                    audience: (!self.jwt_audience.is_empty()).then(|| self.jwt_audience.clone()),
                    issuer: (!self.jwt_issuer.is_empty()).then(|| self.jwt_issuer.clone()),
                    validate_exp: true,
                    validate_nbf: true,
                    leeway: 60,
                    server_uri: None,
                    introspection_endpoint: None,
                    introspection_client_id: None,
                    introspection_client_secret: None,
                };

                info!("Enabling JWT authentication for frontend");
                if let Some(jwks_uri) = &self.jwt_jwks_uri {
                    info!("   Method: Asymmetric ({:?}) with JWKS", algorithm);
                    info!("   JWKS URI: {}", jwks_uri);
                } else {
                    info!("   Method: Symmetric ({:?})", algorithm);
                }
                if let Some(audience) = &jwt_config.audience {
                    info!("   Audience: {}", audience.join(", "));
                }
                if let Some(issuer) = &jwt_config.issuer {
                    info!("   Issuer: {}", issuer.join(", "));
                }

                Some(AuthConfig::jwt_with_config(jwt_config))
            } else {
                info!(
                    "Enabling API key authentication (header: {})",
                    self.api_key_header
                );
                Some(AuthConfig::api_key(self.api_key_header.clone()))
            }
        } else {
            // Warn if binding to 0.0.0.0 without auth
            if self.bind.starts_with("0.0.0.0") {
                warn!("⚠️  Binding to 0.0.0.0 without authentication enabled!");
                warn!(
                    "   Consider using --require-auth, --jwt-secret, or --jwt-jwks-uri for production"
                );
            }
            None
        };

        // Create Axum router with MCP routes and authentication
        info!("Building HTTP server with Axum MCP integration...");
        let config = McpServerConfig {
            enable_compression: true,
            enable_tracing: true,
            auth: auth_config,
            ..Default::default()
        };

        let app = Router::new().turbo_mcp_routes_with_config(proxy_service, config);

        // Parse bind address
        let addr: std::net::SocketAddr = self
            .bind
            .parse()
            .map_err(|e| ProxyError::configuration(format!("Invalid bind address: {e}")))?;

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
            .map_err(|e| ProxyError::backend(format!("Failed to bind to {addr}: {e}")))?;

        axum::serve(listener, app)
            .await
            .map_err(|e| ProxyError::backend(format!("HTTP server error: {e}")))?;

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
            Some(BackendType::Tcp) => {
                let addr = self.backend.tcp.as_ref().ok_or_else(|| {
                    ProxyError::configuration("TCP address not specified".to_string())
                })?;

                // Parse host and port
                let parts: Vec<&str> = addr.split(':').collect();
                if parts.len() != 2 {
                    return Err(ProxyError::configuration(
                        "Invalid TCP address format. Use host:port".to_string(),
                    ));
                }

                let host = parts[0].to_string();
                let port = parts[1]
                    .parse::<u16>()
                    .map_err(|_| ProxyError::configuration("Invalid port number".to_string()))?;

                BackendTransport::Tcp { host, port }
            }
            Some(BackendType::Unix) => {
                let path = self.backend.unix.as_ref().ok_or_else(|| {
                    ProxyError::configuration("Unix socket path not specified".to_string())
                })?;

                BackendTransport::Unix { path: path.clone() }
            }
            Some(BackendType::Websocket) => {
                let url = self.backend.websocket.as_ref().ok_or_else(|| {
                    ProxyError::configuration("WebSocket URL not specified".to_string())
                })?;

                BackendTransport::WebSocket { url: url.clone() }
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
                tcp: None,
                unix: None,
                websocket: None,
            },
            frontend: "http".to_string(),
            bind: "127.0.0.1:3000".to_string(),
            path: "/mcp".to_string(),
            client_name: "test-proxy".to_string(),
            client_version: "1.0.0".to_string(),
            auth_token: None,
            jwt_secret: None,
            jwt_jwks_uri: None,
            jwt_algorithm: "HS256".to_string(),
            jwt_audience: vec![],
            jwt_issuer: vec![],
            api_key_header: "x-api-key".to_string(),
            require_auth: false,
        };

        let config = cmd.create_backend_config();
        assert!(config.is_ok());

        let config = config.unwrap();
        assert_eq!(config.client_name, "test-proxy");
        assert_eq!(config.client_version, "1.0.0");
    }

    #[test]
    fn test_tcp_backend_config() {
        let cmd = ServeCommand {
            backend: BackendArgs {
                backend: Some(BackendType::Tcp),
                cmd: None,
                args: vec![],
                working_dir: None,
                http: None,
                tcp: Some("localhost:5000".to_string()),
                unix: None,
                websocket: None,
            },
            frontend: "http".to_string(),
            bind: "127.0.0.1:3000".to_string(),
            path: "/mcp".to_string(),
            client_name: "test-proxy".to_string(),
            client_version: "1.0.0".to_string(),
            auth_token: None,
            jwt_secret: None,
            jwt_jwks_uri: None,
            jwt_algorithm: "HS256".to_string(),
            jwt_audience: vec![],
            jwt_issuer: vec![],
            api_key_header: "x-api-key".to_string(),
            require_auth: false,
        };

        let config = cmd.create_backend_config();
        assert!(config.is_ok());
    }

    #[test]
    fn test_unix_backend_config() {
        let cmd = ServeCommand {
            backend: BackendArgs {
                backend: Some(BackendType::Unix),
                cmd: None,
                args: vec![],
                working_dir: None,
                http: None,
                tcp: None,
                unix: Some("/tmp/mcp.sock".to_string()),
                websocket: None,
            },
            frontend: "http".to_string(),
            bind: "127.0.0.1:3000".to_string(),
            path: "/mcp".to_string(),
            client_name: "test-proxy".to_string(),
            client_version: "1.0.0".to_string(),
            auth_token: None,
            jwt_secret: None,
            jwt_jwks_uri: None,
            jwt_algorithm: "HS256".to_string(),
            jwt_audience: vec![],
            jwt_issuer: vec![],
            api_key_header: "x-api-key".to_string(),
            require_auth: false,
        };

        let config = cmd.create_backend_config();
        assert!(config.is_ok());
    }
}

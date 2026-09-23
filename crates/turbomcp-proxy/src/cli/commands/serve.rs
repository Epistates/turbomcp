//! Serve command implementation
//!
//! Runs the proxy server to bridge MCP servers across transports.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderName, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use clap::Args;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use secrecy::{ExposeSecret, SecretString};
use tracing::{info, warn};
use turbomcp_auth::jwt::{JwtValidator, StandardClaims};
use turbomcp_server::{McpServerExt, ServerConfig};

use crate::cli::args::BackendArgs;
use crate::error::{ProxyError, ProxyResult};
use crate::proxy::{
    BackendConfig, BackendConnector, BackendTransport, ProxyService, StdioFrontend,
};

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
    /// When used with --require-auth and --api-key, requests must include this
    /// header with the configured API key. Common values: "x-api-key",
    /// "authorization".
    #[arg(long, value_name = "HEADER", default_value = "x-api-key")]
    pub api_key_header: String,

    /// API key for frontend authentication
    ///
    /// When --require-auth is enabled without JWT configuration, requests must
    /// provide this value in --api-key-header. May also be set via
    /// `TURBOMCP_API_KEY`.
    #[arg(long, env = "TURBOMCP_API_KEY", value_name = "KEY")]
    pub api_key: Option<String>,

    /// Require authentication for all frontend requests
    ///
    /// When enabled without --jwt-secret or --jwt-jwks-uri, requires --api-key.
    /// IMPORTANT: Always enable this when binding to 0.0.0.0
    #[arg(long)]
    pub require_auth: bool,

    /// Browser origin permitted to reach the HTTP frontend
    ///
    /// Specify once per allowed origin (e.g. `--allowed-origin https://app.example.com`).
    /// When empty (the default), any browser-issued request carrying an `Origin`
    /// header is rejected with 403; server-to-server clients without `Origin`
    /// continue to work. When set, a `CorsLayer` advertising the allowlist is
    /// installed in addition to the strict request-time check.
    #[arg(long = "allowed-origin", value_name = "ORIGIN")]
    pub allowed_origins: Vec<String>,
}

#[derive(Clone, Debug)]
enum FrontendAuth {
    ApiKey {
        header: HeaderName,
        expected: SecretString,
    },
    Jwt(Arc<FrontendJwtAuth>),
}

#[derive(Debug)]
enum FrontendJwtAuth {
    Symmetric {
        algorithm: Algorithm,
        secret: SecretString,
        audiences: Vec<String>,
        issuers: Vec<String>,
    },
    Jwks(JwtValidator),
}

async fn frontend_auth_middleware(
    State(auth): State<FrontendAuth>,
    request: Request,
    next: Next,
) -> Response {
    match auth.validate(request.headers()).await {
        Ok(()) => next.run(request).await,
        Err(reason) => {
            warn!(reason = %reason, "Rejecting unauthenticated frontend request");
            unauthorized_response(&auth)
        }
    }
}

impl FrontendAuth {
    async fn validate(&self, headers: &HeaderMap) -> Result<(), &'static str> {
        match self {
            Self::ApiKey { header, expected } => {
                let provided = headers
                    .get(header)
                    .and_then(|value| value.to_str().ok())
                    .ok_or("missing API key")?;
                if turbomcp_auth::api_key_validation::validate_api_key(
                    provided,
                    expected.expose_secret(),
                ) {
                    Ok(())
                } else {
                    Err("invalid API key")
                }
            }
            Self::Jwt(jwt) => {
                let token = bearer_token(headers).ok_or("missing bearer token")?;
                jwt.validate(token).await
            }
        }
    }

    const fn is_jwt(&self) -> bool {
        matches!(self, Self::Jwt(_))
    }
}

impl FrontendJwtAuth {
    async fn validate(&self, token: &str) -> Result<(), &'static str> {
        match self {
            Self::Symmetric {
                algorithm,
                secret,
                audiences,
                issuers,
            } => {
                let mut validation = Validation::new(*algorithm);
                validation.validate_nbf = true;
                validation.leeway = 60;
                if audiences.is_empty() {
                    validation.validate_aud = false;
                } else {
                    validation.set_audience(audiences);
                }
                if !issuers.is_empty() {
                    validation.set_issuer(issuers);
                }
                decode::<StandardClaims>(
                    token,
                    &DecodingKey::from_secret(secret.expose_secret().as_bytes()),
                    &validation,
                )
                .map(|_| ())
                .map_err(|_| "invalid bearer token")
            }
            Self::Jwks(validator) => validator
                .validate_with_refresh(token)
                .await
                .map(|_| ())
                .map_err(|_| "invalid bearer token"),
        }
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

fn unauthorized_response(auth: &FrontendAuth) -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        [(header::CONTENT_TYPE, "application/json")],
        r#"{"error":"unauthorized"}"#,
    )
        .into_response();
    if auth.is_jwt() {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            axum::http::HeaderValue::from_static("Bearer"),
        );
    }
    response
}

fn parse_jwt_algorithm(value: &str) -> ProxyResult<Algorithm> {
    match value.to_uppercase().as_str() {
        "HS256" => Ok(Algorithm::HS256),
        "HS384" => Ok(Algorithm::HS384),
        "HS512" => Ok(Algorithm::HS512),
        "RS256" => Ok(Algorithm::RS256),
        "RS384" => Ok(Algorithm::RS384),
        "RS512" => Ok(Algorithm::RS512),
        "ES256" => Ok(Algorithm::ES256),
        "ES384" => Ok(Algorithm::ES384),
        other => Err(ProxyError::configuration(format!(
            "Invalid JWT algorithm: {other}. Valid: HS256, HS384, HS512, RS256, RS384, RS512, ES256, ES384"
        ))),
    }
}

fn is_symmetric_algorithm(algorithm: Algorithm) -> bool {
    matches!(
        algorithm,
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512
    )
}

fn normalize_endpoint_path(path: &str) -> ProxyResult<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(ProxyError::configuration(
            "HTTP endpoint path cannot be empty",
        ));
    }
    if trimmed.contains('?') || trimmed.contains('#') || trimmed.contains('*') {
        return Err(ProxyError::configuration(
            "HTTP endpoint path must be a plain absolute path",
        ));
    }

    let mut normalized = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    Ok(normalized)
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

    fn build_frontend_auth(&self) -> ProxyResult<Option<FrontendAuth>> {
        let auth_requested = self.require_auth
            || self.jwt_secret.is_some()
            || self.jwt_jwks_uri.is_some()
            || self.api_key.is_some();
        if !auth_requested {
            return Ok(None);
        }

        if self.jwt_secret.is_some() && self.jwt_jwks_uri.is_some() {
            return Err(ProxyError::configuration(
                "Use either --jwt-secret or --jwt-jwks-uri, not both",
            ));
        }

        if let Some(secret) = &self.jwt_secret {
            let algorithm = parse_jwt_algorithm(&self.jwt_algorithm)?;
            if !is_symmetric_algorithm(algorithm) {
                return Err(ProxyError::configuration(format!(
                    "--jwt-secret requires HS256, HS384, or HS512; got {algorithm:?}"
                )));
            }
            info!("Enabling JWT authentication for frontend");
            info!("   Method: Symmetric ({:?})", algorithm);
            if !self.jwt_audience.is_empty() {
                info!("   Audience: {}", self.jwt_audience.join(", "));
            }
            if !self.jwt_issuer.is_empty() {
                info!("   Issuer: {}", self.jwt_issuer.join(", "));
            }
            return Ok(Some(FrontendAuth::Jwt(Arc::new(
                FrontendJwtAuth::Symmetric {
                    algorithm,
                    secret: SecretString::from(secret.clone()),
                    audiences: self.jwt_audience.clone(),
                    issuers: self.jwt_issuer.clone(),
                },
            ))));
        }

        if let Some(jwks_uri) = &self.jwt_jwks_uri {
            let algorithm = parse_jwt_algorithm(&self.jwt_algorithm)?;
            if is_symmetric_algorithm(algorithm) {
                return Err(ProxyError::configuration(format!(
                    "--jwt-jwks-uri requires an asymmetric algorithm; got {algorithm:?}"
                )));
            }
            if self.jwt_issuer.len() != 1 || self.jwt_audience.len() != 1 {
                return Err(ProxyError::configuration(
                    "--jwt-jwks-uri requires exactly one --jwt-issuer and one --jwt-audience",
                ));
            }
            info!("Enabling JWT authentication for frontend");
            info!("   Method: Asymmetric ({:?}) with JWKS", algorithm);
            info!("   JWKS URI: {}", jwks_uri);
            info!("   Audience: {}", self.jwt_audience[0]);
            info!("   Issuer: {}", self.jwt_issuer[0]);

            let validator = JwtValidator::with_jwks_uri(
                self.jwt_issuer[0].clone(),
                self.jwt_audience[0].clone(),
                jwks_uri.clone(),
            )
            .with_algorithms(vec![algorithm]);
            return Ok(Some(FrontendAuth::Jwt(Arc::new(FrontendJwtAuth::Jwks(
                validator,
            )))));
        }

        let api_key = self.api_key.as_ref().ok_or_else(|| {
            ProxyError::configuration(
                "--require-auth without JWT configuration requires --api-key or TURBOMCP_API_KEY",
            )
        })?;
        let header = HeaderName::from_bytes(self.api_key_header.as_bytes())
            .map_err(|e| ProxyError::configuration(format!("Invalid API key header name: {e}")))?;
        info!(
            "Enabling API key authentication (header: {})",
            self.api_key_header
        );
        Ok(Some(FrontendAuth::ApiKey {
            header,
            expected: SecretString::from(api_key.clone()),
        }))
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

        let frontend_auth = self.build_frontend_auth()?;
        if frontend_auth.is_none() {
            // Warn if binding to 0.0.0.0 without auth
            if self.bind.starts_with("0.0.0.0") {
                warn!("Binding to 0.0.0.0 without authentication enabled");
                warn!(
                    "   Consider using --require-auth, --jwt-secret, or --jwt-jwks-uri for production"
                );
            }
        }

        let endpoint_path = normalize_endpoint_path(&self.path)?;

        // Layer the proxy's defensive origin/CORS guards around the supported
        // Streamable HTTP router. The guards reject browser-issued requests
        // that aren't on `--allowed-origin`; without explicit config the proxy
        // refuses any browser traffic, mirroring the spec's recommended posture
        // for localhost-bound MCP servers.
        info!("Building HTTP server with turbomcp-server Streamable HTTP integration...");
        let server_config = ServerConfig::builder()
            .max_message_size(crate::runtime::MAX_REQUEST_SIZE)
            // The proxy owns its stricter browser-origin policy in origin_guard:
            // no Origin is allowed for server-to-server clients, any Origin must
            // match the explicit allowlist.
            .allow_any_origin(true)
            .build();
        let allowlist = crate::runtime::OriginAllowlist::new(self.allowed_origins.clone());
        let mcp_router = proxy_service
            .builder()
            .with_config(server_config)
            .into_axum_router();
        let mut app = if endpoint_path == "/mcp" {
            mcp_router
        } else {
            axum::Router::new().nest(&endpoint_path, mcp_router)
        }
        .layer(middleware::from_fn_with_state(
            allowlist.clone(),
            crate::runtime::origin_guard,
        ));
        if let Some(auth) = frontend_auth {
            app = app.layer(middleware::from_fn_with_state(
                auth,
                frontend_auth_middleware,
            ));
        }
        if let Some(cors) = crate::runtime::build_cors_layer(&allowlist) {
            app = app.layer(cors);
        }

        // Parse bind address
        let addr: std::net::SocketAddr = self
            .bind
            .parse()
            .map_err(|e| ProxyError::configuration(format!("Invalid bind address: {e}")))?;

        info!("Proxy server listening on http://{}{}", addr, endpoint_path);
        info!("Backend: STDIO subprocess");
        info!("Frontend: Streamable HTTP");
        info!("MCP endpoints:");
        info!("  POST   {}      - JSON-RPC", endpoint_path);
        info!("  GET    {}      - Server-Sent Events", endpoint_path);
        info!("  DELETE {}      - Terminate session", endpoint_path);
        info!("  GET    {}/sse  - SSE alias", endpoint_path);

        // Run HTTP server using axum::serve
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to bind to {addr}: {e}")))?;

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .map_err(|e| ProxyError::backend(format!("HTTP server error: {e}")))?;

        Ok(())
    }

    /// Execute with STDIO frontend
    ///
    /// Serves any backend (a Streamable HTTP server is the usual case) to a
    /// local client on stdin/stdout, through the same `ProxyService` and
    /// server stack as the HTTP frontend.
    async fn execute_stdio_frontend(&self) -> ProxyResult<()> {
        let backend = BackendConnector::new(self.create_backend_config()?).await?;
        let spec = backend.introspect().await?;
        let service = ProxyService::new(backend, spec);

        let config = ServerConfig::builder()
            .max_message_size(crate::runtime::MAX_REQUEST_SIZE)
            .build();

        info!("Frontend: STDIO (stdin/stdout)");
        StdioFrontend::new(service, config).run().await?;

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
                    endpoint_path: self.backend.endpoint_path.clone(),
                    // `--auth-token` used to reach only the stdio frontend's
                    // separate HTTP client; the HTTP frontend connected to an
                    // authenticated upstream with no credentials at all.
                    auth_token: self.auth_token.clone().map(SecretString::from),
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
            #[cfg(unix)]
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

    fn base_command() -> ServeCommand {
        ServeCommand {
            backend: BackendArgs {
                endpoint_path: None,
                backend: Some(BackendType::Stdio),
                cmd: Some("python".to_string()),
                args: vec!["server.py".to_string()],
                working_dir: None,
                http: None,
                tcp: None,
                #[cfg(unix)]
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
            api_key: None,
            require_auth: false,
            allowed_origins: Vec::new(),
        }
    }

    #[test]
    fn test_backend_config_creation() {
        let cmd = ServeCommand {
            backend: BackendArgs {
                endpoint_path: None,
                backend: Some(BackendType::Stdio),
                cmd: Some("python".to_string()),
                args: vec!["server.py".to_string()],
                working_dir: None,
                http: None,
                tcp: None,
                #[cfg(unix)]
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
            api_key: None,
            require_auth: false,
            allowed_origins: Vec::new(),
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
                endpoint_path: None,
                backend: Some(BackendType::Tcp),
                cmd: None,
                args: vec![],
                working_dir: None,
                http: None,
                tcp: Some("localhost:5000".to_string()),
                #[cfg(unix)]
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
            api_key: None,
            require_auth: false,
            allowed_origins: Vec::new(),
        };

        let config = cmd.create_backend_config();
        assert!(config.is_ok());
    }

    #[test]
    fn require_auth_without_credentials_errors() {
        let mut cmd = base_command();
        cmd.require_auth = true;

        let err = cmd.build_frontend_auth().unwrap_err();
        assert!(err.to_string().contains("--api-key"));
    }

    #[test]
    fn api_key_auth_configures_header() {
        let mut cmd = base_command();
        cmd.require_auth = true;
        cmd.api_key = Some("test_key_abcdefghijklmnopqrstuvwxyz123456".to_string());

        let auth = cmd.build_frontend_auth().unwrap().unwrap();
        match auth {
            FrontendAuth::ApiKey { header, .. } => {
                assert_eq!(header, HeaderName::from_static("x-api-key"));
            }
            FrontendAuth::Jwt(_) => panic!("expected API key auth"),
        }
    }

    #[test]
    fn jwt_secret_rejects_asymmetric_algorithm() {
        let mut cmd = base_command();
        cmd.jwt_secret = Some("secret".to_string());
        cmd.jwt_algorithm = "RS256".to_string();

        let err = cmd.build_frontend_auth().unwrap_err();
        assert!(err.to_string().contains("--jwt-secret requires"));
    }

    #[test]
    fn custom_endpoint_path_is_normalized() {
        assert_eq!(normalize_endpoint_path("api/mcp/").unwrap(), "/api/mcp");
        assert!(normalize_endpoint_path("/api/mcp?debug=true").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_backend_config() {
        let cmd = ServeCommand {
            backend: BackendArgs {
                endpoint_path: None,
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
            api_key: None,
            require_auth: false,
            allowed_origins: Vec::new(),
        };

        let config = cmd.create_backend_config();
        assert!(config.is_ok());
    }
}
